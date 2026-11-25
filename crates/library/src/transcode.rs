//! Opus for linked engines off the LAN and for downloads (#244): which files
//! to encode ([`plan`]) and the encoder ([`Encode`]), which turns a file from
//! any point into Opus packets, for a live stream or a whole file alike.

use std::{path::Path, time::Duration};

use engine::audio::{
    decoder::{self, Decoder},
    pipeline,
};
use protocol::Quality;

/// The Opus bitrate (kb/s) to send a file at, or `None` to send it as it
/// is: lossless files and those above the bitrate are encoded, lossy ones at
/// or below it are not (or one whose bitrate is unknown), as encoding them
/// again would lose more for little saved.
pub fn plan(codec: Option<&str>, bitrate: Option<u32>, quality: Quality) -> Option<u32> {
    let kbps = quality.kbps()?;
    let lossless = matches!(codec, Some("FLAC" | "WAV" | "AIFF" | "WavPack" | "APE"));
    (lossless || bitrate.is_some_and(|b| b > kbps)).then_some(kbps)
}

/// Opus always runs at 48 kHz here.
pub const RATE: u32 = 48_000;
/// Frames per packet: 20 ms.
pub const FRAME: usize = 960;

/// One Opus packet and how many of the frames it decodes to are audio: all
/// [`FRAME`] but at the end, where the rest is padding.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub data: Vec<u8>,
    pub frames: u16,
}

/// A file encoded to Opus as it is read.
pub struct Encode {
    decoder: Box<dyn Decoder>,
    stream: pipeline::Stream,
    encoder: opus::Encoder,
    channels: usize,
    reached: Duration,
    pre_skip: u16,
    /// Samples (at 48 kHz, `channels` wide) not encoded yet.
    pcm: Vec<f32>,
    decoded: Vec<f32>,
    /// Frames given to the encoder.
    fed: u64,
    /// Frames of audio in all, once the file has ended.
    total: Option<u64>,
}

impl Encode {
    /// Encodes the file at `path` from `from` at `kbps`.
    pub fn open(path: &Path, from: Duration, kbps: u32) -> Result<Self, String> {
        let mut decoder = decoder::open(&path.to_path_buf().into(), &decoder::all())?;
        let reached = if from.is_zero() {
            from
        } else {
            decoder.seek(from)?
        };
        // shortcut: surround keeps only its front left and right; downmix it if a surround collection shows up.
        let channels = decoder.channels().min(2);
        let layout = match channels {
            1 => opus::Channels::Mono,
            _ => opus::Channels::Stereo,
        };
        let mut encoder = opus::Encoder::new(RATE, layout, opus::Application::Audio)
            .map_err(|e| e.to_string())?;
        encoder
            .set_bitrate(opus::Bitrate::Bits(kbps as i32 * 1000))
            .map_err(|e| e.to_string())?;
        let pre_skip = encoder.get_lookahead().map_err(|e| e.to_string())? as u16;
        Ok(Self {
            decoder,
            stream: pipeline::Stream::new(RATE, channels),
            encoder,
            channels,
            reached,
            pre_skip,
            pcm: Vec::new(),
            decoded: Vec::new(),
            fed: 0,
            total: None,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Where the encoding starts, as near `from` as the file seeks.
    pub fn reached(&self) -> Duration {
        self.reached
    }

    /// Frames to drop from the start of the decoded stream (the encoder's
    /// look-ahead), as in an Ogg Opus header.
    pub fn pre_skip(&self) -> u16 {
        self.pre_skip
    }

    /// The next packet, or `None` once every frame of audio is in one.
    pub fn packet(&mut self) -> Result<Option<Packet>, String> {
        let wide = FRAME * self.channels;
        while self.pcm.len() < wide && self.total.is_none() {
            self.decoded.clear();
            let (rate, channels) = (self.decoder.sample_rate(), self.decoder.channels());
            if self.decoder.decode(&mut self.decoded)? {
                self.stream
                    .push(rate, channels, &self.decoded, &mut self.pcm)?;
            } else {
                self.stream.finish(&mut self.pcm)?;
                self.total = Some(self.fed + (self.pcm.len() / self.channels) as u64);
            }
        }
        // The look-ahead delays the audio, so it ends `pre_skip` frames on.
        let audio = match self.total {
            Some(total) => (total + u64::from(self.pre_skip)).saturating_sub(self.fed),
            None => FRAME as u64,
        };
        if audio == 0 {
            return Ok(None);
        }
        if self.pcm.len() < wide {
            self.pcm.resize(wide, 0.0);
        }
        let data = self
            .encoder
            .encode_vec_float(&self.pcm[..wide], 4000)
            .map_err(|e| e.to_string())?;
        self.pcm.drain(..wide);
        self.fed += FRAME as u64;
        Ok(Some(Packet {
            data,
            frames: audio.min(FRAME as u64) as u16,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_and_high_bitrate_files_are_encoded_lossy_ones_below_are_not() {
        let opus = Quality::Opus256;
        assert_eq!(plan(Some("FLAC"), Some(900), opus), Some(256));
        assert_eq!(plan(Some("WAV"), None, Quality::Opus96), Some(96));
        assert_eq!(plan(Some("MP3"), Some(320), opus), Some(256));
        assert_eq!(plan(Some("MP3"), Some(192), opus), None);
        assert_eq!(plan(Some("MP3"), Some(256), opus), None);
        assert_eq!(plan(Some("Opus"), Some(128), Quality::Opus128), None);
        assert_eq!(plan(Some("AAC"), None, opus), None, "unknown bitrate");
        // ALAC shows as its container; its bitrate gives it away.
        assert_eq!(plan(Some("M4A"), Some(1000), opus), Some(256));
        assert_eq!(plan(Some("FLAC"), Some(900), Quality::Original), None);
    }

    /// `seconds` of a 440 Hz tone, 16-bit stereo at 44.1 kHz, as a WAV file.
    fn tone(path: &Path, seconds: u32) {
        let frames = seconds * 44_100;
        let mut data = Vec::new();
        for i in 0..frames {
            let t = i as f32 / 44_100.0;
            let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 12_000.0) as i16;
            data.extend_from_slice(&s.to_le_bytes());
            data.extend_from_slice(&s.to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[1, 0, 2, 0]);
        wav.extend_from_slice(&44_100u32.to_le_bytes());
        wav.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
        wav.extend_from_slice(&[4, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(path, wav).unwrap();
    }

    /// Decodes every packet as a receiver does: frames of audio, and bytes.
    fn decode_all(encode: &mut Encode) -> (Vec<f32>, usize) {
        let channels = encode.channels();
        let mut decoder = opus::Decoder::new(RATE, opus::Channels::Stereo).unwrap();
        let mut skip = encode.pre_skip() as usize;
        let (mut audio, mut bytes) = (Vec::new(), 0);
        let mut buffer = vec![0.0; FRAME * channels];
        while let Some(packet) = encode.packet().unwrap() {
            bytes += packet.data.len();
            decoder
                .decode_float(&packet.data, &mut buffer, false)
                .unwrap();
            let end = packet.frames as usize;
            let start = skip.min(end);
            skip -= start;
            audio.extend_from_slice(&buffer[start * channels..end * channels]);
        }
        (audio, bytes)
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("motif-transcode-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_encodes_to_exactly_its_length_at_about_the_bitrate() {
        let dir = temp("whole");
        let path = dir.join("tone.wav");
        tone(&path, 2);
        let mut encode = Encode::open(&path, Duration::ZERO, 128).unwrap();
        let (audio, bytes) = decode_all(&mut encode);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(audio.len() / 2, 2 * 48_000, "every frame, no padding");
        let kbps = bytes as f64 * 8.0 / 2.0 / 1000.0;
        assert!((64.0..160.0).contains(&kbps), "{kbps} kb/s");
        // The tone survives: loud in the middle, not silence or noise.
        let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
        let expected = 12_000.0 / 32_768.0 / 2f32.sqrt();
        assert!(
            (rms - expected).abs() < 0.05,
            "rms {rms}, expected {expected}"
        );
    }

    #[test]
    fn an_encode_from_a_point_starts_there() {
        let dir = temp("from");
        let path = dir.join("tone.wav");
        tone(&path, 2);
        let mut encode = Encode::open(&path, Duration::from_millis(1500), 256).unwrap();
        let reached = encode.reached();
        let (audio, _) = decode_all(&mut encode);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(reached.abs_diff(Duration::from_millis(1500)) < Duration::from_millis(30));
        let left = Duration::from_secs(2) - reached;
        let frames = audio.len() as f64 / 2.0;
        assert!(
            (frames - left.as_secs_f64() * 48_000.0).abs() < 2.0,
            "{frames} frames"
        );
    }

    #[test]
    #[ignore = "measures encode speed: cargo test --release -p library encode_speed -- --ignored --nocapture"]
    fn encode_speed() {
        let dir = temp("speed");
        let path = dir.join("tone.wav");
        tone(&path, 60);
        let started = std::time::Instant::now();
        let mut encode = Encode::open(&path, Duration::ZERO, 256).unwrap();
        while encode.packet().unwrap().is_some() {}
        let took = started.elapsed();
        std::fs::remove_dir_all(&dir).unwrap();
        println!(
            "60 s of 44.1 kHz stereo to Opus 256: {:.2} s, {:.0}x real time",
            took.as_secs_f64(),
            60.0 / took.as_secs_f64()
        );
    }
}
