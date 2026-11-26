//! Opus for linked engines off the LAN and for downloads (#244): which files
//! to encode ([`plan`]) and the encoder ([`Encode`]), which turns a file from
//! any point into Opus packets, for a live stream or a whole file alike.

use std::{
    io::{self, Write},
    path::Path,
    time::Duration,
};

use symphonia::core::checksum::Crc32;
use symphonia::core::io::Monitor as _;

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

/// Writes Opus packets as an Ogg Opus file (RFC 7845), for downloads:
/// pages of about a second each.
pub struct OggOpus<W: Write> {
    out: W,
    serial: u32,
    /// The number of the page written next.
    sequence: u32,
    /// Frames (at 48 kHz, pre-skip included) through the packets so far.
    granule: u64,
    /// The packets of the page being made, and its lacing values.
    page: Vec<u8>,
    lacing: Vec<u8>,
}

/// Packets per page: a second of Opus.
const PAGE_PACKETS: usize = 50;

impl<W: Write> OggOpus<W> {
    /// Starts the file with its headers: `tags` are Vorbis comments
    /// (name, value).
    pub fn new(out: W, channels: u8, pre_skip: u16, tags: &[(String, String)]) -> io::Result<Self> {
        let mut head = b"OpusHead".to_vec();
        head.extend_from_slice(&[1, channels]);
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&RATE.to_le_bytes());
        // No output gain, mono or stereo.
        head.extend_from_slice(&[0, 0, 0]);
        // Each field after its length: the vendor, then the comments.
        let fields = std::iter::once("Motif".to_string())
            .chain(tags.iter().map(|(name, value)| format!("{name}={value}")));
        let mut comments = b"OpusTags".to_vec();
        for (n, field) in fields.enumerate() {
            if n == 1 {
                comments.extend_from_slice(&(tags.len() as u32).to_le_bytes());
            }
            comments.extend_from_slice(&(field.len() as u32).to_le_bytes());
            comments.extend_from_slice(field.as_bytes());
        }
        if tags.is_empty() {
            comments.extend_from_slice(&0u32.to_le_bytes());
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        let mut ogg = Self {
            out,
            serial: now.map_or(1, |d| d.subsec_nanos()),
            sequence: 0,
            granule: 0,
            page: Vec::new(),
            lacing: Vec::new(),
        };
        // Each header on a page of its own, the first starting the stream.
        ogg.add(&head)?;
        ogg.flush(0x02)?;
        ogg.add(&comments)?;
        ogg.flush(0)?;
        Ok(ogg)
    }

    /// Adds a packet with `frames` of audio.
    pub fn packet(&mut self, data: &[u8], frames: u16) -> io::Result<()> {
        if self.lacing.len() + data.len() / 255 + 1 > 255 || self.lacing.len() >= PAGE_PACKETS * 2 {
            self.flush(0)?;
        }
        self.add(data)?;
        self.granule += u64::from(frames);
        Ok(())
    }

    /// Ends the file: its last page says so.
    pub fn finish(mut self) -> io::Result<W> {
        self.flush(0x04)?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn add(&mut self, data: &[u8]) -> io::Result<()> {
        if data.len() / 255 + 1 > 255 - self.lacing.len() {
            return Err(io::Error::other("an Ogg packet too big for one page"));
        }
        self.lacing
            .extend(std::iter::repeat_n(255, data.len() / 255));
        self.lacing.push((data.len() % 255) as u8);
        self.page.extend_from_slice(data);
        Ok(())
    }

    /// Writes the page being made, with `flags` (0x02 first, 0x04 last).
    fn flush(&mut self, flags: u8) -> io::Result<()> {
        let mut page = b"OggS".to_vec();
        page.extend_from_slice(&[0, flags]);
        // Headers end at granule 0.
        page.extend_from_slice(&self.granule.to_le_bytes());
        page.extend_from_slice(&self.serial.to_le_bytes());
        page.extend_from_slice(&self.sequence.to_le_bytes());
        page.extend_from_slice(&[0; 4]);
        page.push(self.lacing.len() as u8);
        page.extend_from_slice(&self.lacing);
        page.extend_from_slice(&self.page);
        let mut crc = Crc32::new(0);
        crc.process_buf_bytes(&page);
        page[22..26].copy_from_slice(&crc.crc().to_le_bytes());
        self.out.write_all(&page)?;
        self.sequence += 1;
        self.page.clear();
        self.lacing.clear();
        Ok(())
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
    fn an_ogg_opus_file_plays_as_long_as_its_source_with_its_tags() {
        let dir = temp("ogg");
        let path = dir.join("tone.wav");
        tone(&path, 3);
        let mut encode = Encode::open(&path, Duration::ZERO, 256).unwrap();
        let tags = [("TITLE".to_string(), "Tone".to_string())];
        let mut ogg = OggOpus::new(Vec::new(), 2, encode.pre_skip(), &tags).unwrap();
        while let Some(packet) = encode.packet().unwrap() {
            ogg.packet(&packet.data, packet.frames).unwrap();
        }
        let copy = dir.join("tone.opus");
        std::fs::write(&copy, ogg.finish().unwrap()).unwrap();
        let mut decoder = decoder::open(&copy.clone().into(), &decoder::all()).unwrap();
        let mut samples = Vec::new();
        while decoder.decode(&mut samples).unwrap() {}
        let frames = samples.len() / decoder.channels();
        let read = crate::tags::read(&copy);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(decoder.sample_rate(), RATE);
        assert!(frames.abs_diff(3 * 48_000) <= 2, "{frames} frames");
        assert_eq!(read.title.as_deref(), Some("Tone"));
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
