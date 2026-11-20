//! Audio hashes identify the same recording file across devices even when
//! its tags differ, because only the audio is hashed.

use std::path::Path;

use symphonia::core::{
    codecs::{
        CodecParameters,
        audio::well_known::{
            CODEC_ID_AAC, CODEC_ID_MP1, CODEC_ID_MP2, CODEC_ID_MP3, CODEC_ID_OPUS, CODEC_ID_VORBIS,
        },
    },
    errors::Error as SymError,
    formats::{FormatReader, TrackType},
    io::MediaSourceStream,
    packet::Packet,
};

use crate::tags;

/// `flac-md5:<hex>` from the FLAC stream's own MD5 when present (free to
/// read); `pkt-b3:<hex>` for lossy audio, a BLAKE3 hash of its compressed
/// packets (decoders differ between platforms, packets don't); else
/// `pcm-b3:<hex>`, a BLAKE3 hash of the decoded samples (lossless decodes
/// are exact).
pub fn audio_hash(path: &Path) -> Result<String, String> {
    if let Some(hash) = flac_hash(path) {
        return Ok(hash);
    }
    if let Some(hash) = packet_hash(path)? {
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

/// `pkt-b3:<hex>` of a lossy file's default audio track, or `None` for
/// lossless. Container data (tags, page headers) is not part of the packets.
fn packet_hash(path: &Path) -> Result<Option<String>, String> {
    let mut reader = open(path)?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err("no audio parameters".into());
    };
    if ![
        CODEC_ID_OPUS,
        CODEC_ID_VORBIS,
        CODEC_ID_MP1,
        CODEC_ID_MP2,
        CODEC_ID_MP3,
        CODEC_ID_AAC,
    ]
    .contains(&params.codec)
    {
        return Ok(None);
    }
    let track_id = track.id;
    let mut hasher = blake3::Hasher::new();
    while let Some(packet) = next_packet(&mut *reader)? {
        if packet.track_id == track_id {
            hasher.update(&packet.data);
        }
    }
    Ok(Some(format!(
        "pkt-b3:{}",
        &hasher.finalize().to_hex()[..32]
    )))
}

fn open(path: &Path) -> Result<Box<dyn FormatReader>, String> {
    let stream = MediaSourceStream::new(
        engine::audio::ogg::source(path).map_err(|e| e.to_string())?,
        Default::default(),
    );
    symphonia::default::get_probe()
        .probe(
            &Default::default(),
            stream,
            Default::default(),
            Default::default(),
        )
        .map_err(|e| e.to_string())
}

/// The next packet, `None` at the end (a truncated file ends there too).
fn next_packet(reader: &mut dyn FormatReader) -> Result<Option<Packet>, String> {
    match reader.next_packet() {
        Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        other => other.map_err(|e| e.to_string()),
    }
}

/// A `pcm-b3:` hash fed the decoded samples.
#[derive(Default)]
struct PcmHash {
    hasher: blake3::Hasher,
    bytes: Vec<u8>,
}

impl PcmHash {
    fn update(&mut self, samples: &[f32]) {
        self.bytes.clear();
        self.bytes
            .extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        self.hasher.update(&self.bytes);
    }

    fn finish(&self) -> String {
        format!("pcm-b3:{}", &self.hasher.finalize().to_hex()[..32])
    }
}

/// Appends one packet's samples (interleaved).
type PacketDecoder = dyn FnMut(&Packet, &mut Vec<f32>) -> Result<(), String>;

/// Decodes the file's default audio track, handing each packet's samples
/// (interleaved) to `f` with the sample rate and channel count.
pub fn decode(path: &Path, mut f: impl FnMut(&[f32], u32, usize)) -> Result<(), String> {
    let mut reader = open(path)?;
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
    while let Some(packet) = next_packet(&mut *reader)? {
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
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../engine/tests/fixtures")
            .join(name)
    }

    #[test]
    fn lossy_files_hash_their_packets_the_same_on_every_platform() {
        assert_eq!(
            audio_hash(&fixture("sine.opus")).unwrap(),
            "pkt-b3:b4e1061e89b952cd0912e6af350a754c"
        );
        assert_eq!(
            audio_hash(&fixture("sine.mp3")).unwrap(),
            "pkt-b3:5cab5b06ce1f3a5033a0ab7356f4f8ce"
        );
        assert!(
            audio_hash(&fixture("sine.m4a"))
                .unwrap()
                .starts_with("pkt-b3:")
        );
        assert!(
            audio_hash(&fixture("sine.ogg"))
                .unwrap()
                .starts_with("pkt-b3:")
        );
    }

    #[test]
    fn lossless_files_keep_their_hashes() {
        assert!(
            audio_hash(&fixture("sine.flac"))
                .unwrap()
                .starts_with("flac-md5:")
        );
        assert!(
            audio_hash(&fixture("sine.wav"))
                .unwrap()
                .starts_with("pcm-b3:")
        );
        assert!(
            audio_hash(&fixture("sine-alac.m4a"))
                .unwrap()
                .starts_with("pcm-b3:")
        );
    }

    #[test]
    fn retagging_keeps_the_hash() {
        use lofty::{config::WriteOptions, file::TaggedFileExt, prelude::*, tag::Tag};
        for name in ["sine.opus", "sine.mp3"] {
            let dir =
                std::env::temp_dir().join(format!("motif-retag-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let copy = dir.join(name);
            std::fs::copy(fixture(name), &copy).unwrap();
            let before = audio_hash(&copy).unwrap();
            let mut file = lofty::read_from_path(&copy).unwrap();
            let kind = file.primary_tag_type();
            let mut tag = Tag::new(kind);
            tag.set_title("A new, much longer title than before".into());
            tag.set_artist("Someone".into());
            file.insert_tag(tag);
            file.save_to_path(&copy, WriteOptions::default()).unwrap();
            let after = audio_hash(&copy).unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
            assert_eq!(before, after, "{name}");
        }
    }
}
