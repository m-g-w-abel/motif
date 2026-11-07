//! Audio hashes identify the same recording file across devices even when
//! its tags differ, because only the audio is hashed.

use std::{fs::File, path::Path};

use symphonia::core::{
    codecs::{CodecParameters, audio::well_known::CODEC_ID_OPUS},
    errors::Error as SymError,
    formats::TrackType,
    io::MediaSourceStream,
    packet::Packet,
};

use crate::tags;

/// `flac-md5:<hex>` from the FLAC stream's own MD5 when present (free to
/// read), else `pcm-b3:<hex>`, a BLAKE3 hash of the decoded samples.
pub fn audio_hash(path: &Path) -> Result<String, String> {
    if let Some(hash) = flac_hash(path) {
        return Ok(hash);
    }
    let mut hash = PcmHash::default();
    decode(path, |samples, _, _| hash.update(samples))?;
    Ok(hash.finish())
}

/// The audio hash read from a FLAC stream's MD5, if it has one.
pub fn flac_hash(path: &Path) -> Option<String> {
    tags::flac_md5(path).map(|md5| format!("flac-md5:{md5:032x}"))
}

/// A `pcm-b3:` hash fed the decoded samples.
#[derive(Default)]
pub struct PcmHash {
    hasher: blake3::Hasher,
    bytes: Vec<u8>,
}

impl PcmHash {
    pub fn update(&mut self, samples: &[f32]) {
        self.bytes.clear();
        self.bytes
            .extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        self.hasher.update(&self.bytes);
    }

    pub fn finish(&self) -> String {
        format!("pcm-b3:{}", &self.hasher.finalize().to_hex()[..32])
    }
}

/// Appends one packet's samples (interleaved).
type PacketDecoder = dyn FnMut(&Packet, &mut Vec<f32>) -> Result<(), String>;

/// Decodes the file's default audio track, handing each packet's samples
/// (interleaved) to `f` with the sample rate and channel count.
pub fn decode(path: &Path, mut f: impl FnMut(&[f32], u32, usize)) -> Result<(), String> {
    let stream = MediaSourceStream::new(
        Box::new(File::open(path).map_err(|e| e.to_string())?),
        Default::default(),
    );
    let mut reader = symphonia::default::get_probe()
        .probe(
            &Default::default(),
            stream,
            Default::default(),
            Default::default(),
        )
        .map_err(|e| e.to_string())?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err("no audio parameters".into());
    };
    let track_id = track.id;
    let rate = params.sample_rate.unwrap_or(0);
    let channels = params.channels.as_ref().map_or(0, |c| c.count());
    // Symphonia has no Opus decoder; libopus (as the engine plays it) does.
    let mut decode_packet: Box<PacketDecoder> = if params.codec == CODEC_ID_OPUS {
        let layout = match channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            n => return Err(format!("{n}-channel Opus is not supported")),
        };
        let mut decoder = opus::Decoder::new(48_000, layout).map_err(|e| e.to_string())?;
        let mut skip = track.delay.unwrap_or(0) as usize;
        let mut buffer = vec![0.0; 5760 * channels];
        Box::new(move |packet, samples| {
            let frames = decoder
                .decode_float(&packet.data, &mut buffer, false)
                .map_err(|e| e.to_string())?;
            let end = frames.saturating_sub(packet.trim_end.get() as usize);
            let start = skip.min(end);
            skip -= start;
            samples.extend_from_slice(&buffer[start * channels..end * channels]);
            Ok(())
        })
    } else {
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &Default::default())
            .map_err(|e| e.to_string())?;
        Box::new(move |packet, samples| match decoder.decode(packet) {
            Ok(buffer) => {
                buffer.copy_to_vec_interleaved(samples);
                Ok(())
            }
            Err(SymError::DecodeError(_)) => Ok(()),
            Err(e) => Err(e.to_string()),
        })
    };
    let mut samples = Vec::<f32>::new();
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id != track_id {
            continue;
        }
        samples.clear();
        decode_packet(&packet, &mut samples)?;
        f(&samples, rate, channels);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_decodes_through_libopus_with_pre_skip_trimmed() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures/sine.opus");
        let (mut frames, mut format) = (0, (0, 0));
        decode(&path, |samples, rate, channels| {
            frames += samples.len() / channels;
            format = (rate, channels);
        })
        .unwrap();
        assert_eq!((frames, format), (48_000, (48_000, 2)));
        assert!(audio_hash(&path).unwrap().starts_with("pcm-b3:"));
    }
}
