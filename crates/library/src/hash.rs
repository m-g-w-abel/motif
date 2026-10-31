//! Audio hashes identify the same recording file across devices even when
//! its tags differ, because only the audio is hashed.

use std::{fs::File, path::Path};

use symphonia::core::{
    codecs::CodecParameters, errors::Error as SymError, formats::TrackType, io::MediaSourceStream,
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
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &Default::default())
        .map_err(|e| e.to_string())?;
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
        match decoder.decode(&packet) {
            Ok(buffer) => buffer.copy_to_vec_interleaved(&mut samples),
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
        f(&samples, rate, channels);
    }
    Ok(())
}
