//! Decoder plugins. Every decoder is compiled in and implements [`Decoder`];
//! for each file the engine tries them in order and uses the first that
//! opens it.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    time::Duration,
};

use symphonia::core::{
    codecs::{
        CodecParameters,
        audio::{AudioCodecParameters, AudioDecoder, well_known::CODEC_ID_OPUS},
    },
    errors::Error as SymError,
    formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint},
    io::MediaSourceStream,
    packet::Packet,
    units::Time,
};

/// A decoded audio stream.
pub trait Decoder: Send {
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> usize;
    /// Appends the next block of interleaved samples to `out`. Returns false
    /// at the end of the stream.
    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, String>;
    /// Moves to `to` and returns the position actually reached.
    fn seek(&mut self, to: Duration) -> Result<Duration, String>;
}

/// A decoder implementation the engine can pick for a file.
pub struct Plugin {
    pub name: &'static str,
    pub open: fn(&Path) -> Result<Box<dyn Decoder>, String>,
}

pub const SYMPHONIA: Plugin = Plugin {
    name: "symphonia",
    open: |path| Ok(Box::new(Symphonia::open(path)?)),
};

pub const OPUS: Plugin = Plugin {
    name: "libopus",
    open: |path| Ok(Box::new(Opus::open(path)?)),
};

pub const FFMPEG: Plugin = Plugin {
    name: "ffmpeg",
    open: |path| Ok(Box::new(Ffmpeg::open(path)?)),
};

/// Every plugin built for this platform, in the default order: specialised
/// decoders first, FFmpeg last as the catch-all.
pub fn all() -> Vec<&'static Plugin> {
    let mut plugins = vec![&OPUS, &SYMPHONIA];
    // Android can't run a separate FFmpeg program.
    if cfg!(not(target_os = "android")) {
        plugins.push(&FFMPEG);
    }
    plugins
}

/// Opens `path` with the first plugin in `plugins` that can. The error lists
/// why each one couldn't.
pub fn open(path: &Path, plugins: &[&Plugin]) -> Result<Box<dyn Decoder>, String> {
    let mut reasons = Vec::new();
    for plugin in plugins {
        match (plugin.open)(path) {
            Ok(decoder) => return Ok(decoder),
            Err(reason) => reasons.push(format!("{}: {reason}", plugin.name)),
        }
    }
    Err(reasons.join("; "))
}

/// Pure-Rust decoding of FLAC, MP3, AAC/ALAC, Vorbis, WAV/AIFF/CAF and ADPCM.
pub struct Symphonia {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    rate: u32,
    channels: usize,
    scratch: Vec<f32>,
}

/// Opens `path` with Symphonia's container readers and returns the reader,
/// its default audio track and that track's parameters.
fn demux(path: &Path) -> Result<(Box<dyn FormatReader>, u32, AudioCodecParameters), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let reader = symphonia::default::get_probe()
        .probe(&hint, stream, Default::default(), Default::default())
        .map_err(|e| e.to_string())?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err("no audio codec parameters".into());
    };
    let (id, params) = (track.id, params.clone());
    Ok((reader, id, params))
}

/// The next packet of `track_id`, or `None` at the end.
fn next_packet(reader: &mut dyn FormatReader, track_id: u32) -> Result<Option<Packet>, String> {
    loop {
        match reader.next_packet() {
            Ok(Some(packet)) if packet.track_id == track_id => return Ok(Some(packet)),
            Ok(Some(_)) => {}
            Ok(None) => return Ok(None),
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Seeks `reader` and returns the position reached.
fn seek_reader(
    reader: &mut dyn FormatReader,
    track_id: u32,
    to: Duration,
) -> Result<Duration, String> {
    let time = Time::try_from_secs_f64(to.as_secs_f64()).ok_or("bad seek time")?;
    let seeked = reader
        .seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: Some(track_id),
            },
        )
        .map_err(|e| e.to_string())?;
    let time_base = reader
        .tracks()
        .iter()
        .find(|t| t.id == track_id)
        .and_then(|t| t.time_base);
    Ok(time_base
        .and_then(|tb| tb.calc_time(seeked.actual_ts))
        .map_or(to, |t| Duration::from_secs_f64(t.as_secs_f64().max(0.0))))
}

impl Symphonia {
    pub fn open(path: &Path) -> Result<Self, String> {
        let (reader, track_id, params) = demux(path)?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &Default::default())
            .map_err(|e| e.to_string())?;
        let rate = params.sample_rate.ok_or("unknown sample rate")?;
        let channels = params.channels.as_ref().map_or(0, |c| c.count());
        if channels == 0 {
            return Err("unknown channel layout".into());
        }
        Ok(Self {
            reader,
            decoder,
            track_id,
            rate,
            channels,
            scratch: Vec::new(),
        })
    }
}

impl Decoder for Symphonia {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        loop {
            let Some(packet) = next_packet(self.reader.as_mut(), self.track_id)? else {
                return Ok(false);
            };
            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    buffer.copy_to_vec_interleaved(&mut self.scratch);
                    out.extend_from_slice(&self.scratch);
                    return Ok(true);
                }
                // A damaged packet costs a few milliseconds of audio, not the track.
                Err(SymError::DecodeError(err)) => tracing::debug!(err, "skipping bad packet"),
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    fn seek(&mut self, to: Duration) -> Result<Duration, String> {
        let reached = seek_reader(self.reader.as_mut(), self.track_id, to)?;
        self.decoder.reset();
        Ok(reached)
    }
}

/// Opus in Ogg: Symphonia's Ogg reader (which knows Opus pre-skip and end
/// trimming) feeding libopus.
pub struct Opus {
    reader: Box<dyn FormatReader>,
    decoder: opus::Decoder,
    track_id: u32,
    channels: usize,
    buffer: Vec<f32>,
    /// Frames still to drop from the start (the stream's pre-skip).
    skip: usize,
}

/// Opus always decodes at 48 kHz.
const OPUS_RATE: u32 = 48_000;
/// The longest Opus packet: 120 ms.
const OPUS_MAX_FRAMES: usize = 5760;

impl Opus {
    pub fn open(path: &Path) -> Result<Self, String> {
        let (reader, track_id, params) = demux(path)?;
        if params.codec != CODEC_ID_OPUS {
            return Err("not Opus".into());
        }
        let channels = params.channels.as_ref().map_or(0, |c| c.count());
        let layout = match channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            // shortcut: surround Opus needs libopus's multistream decoder, FFmpeg plays it until then.
            n => return Err(format!("{n}-channel Opus is not supported")),
        };
        let decoder = opus::Decoder::new(OPUS_RATE, layout).map_err(|e| e.to_string())?;
        // The Ogg reader trims end padding per packet but leaves pre-skip to the decoder.
        let skip = reader
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .and_then(|t| t.delay)
            .unwrap_or(0);
        Ok(Self {
            reader,
            decoder,
            track_id,
            channels,
            buffer: vec![0.0; OPUS_MAX_FRAMES * channels],
            skip: skip as usize,
        })
    }
}

impl Decoder for Opus {
    fn sample_rate(&self) -> u32 {
        OPUS_RATE
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        let Some(packet) = next_packet(self.reader.as_mut(), self.track_id)? else {
            return Ok(false);
        };
        let frames = self
            .decoder
            .decode_float(&packet.data, &mut self.buffer, false)
            .map_err(|e| e.to_string())?;
        let end = frames.saturating_sub(packet.trim_end.get() as usize);
        let start = self.skip.min(end);
        self.skip -= start;
        out.extend_from_slice(&self.buffer[start * self.channels..end * self.channels]);
        Ok(true)
    }

    fn seek(&mut self, to: Duration) -> Result<Duration, String> {
        let reached = seek_reader(self.reader.as_mut(), self.track_id, to)?;
        self.decoder.reset_state().map_err(|e| e.to_string())?;
        // shortcut: no 80 ms pre-roll after a seek, so the first few ms may be slightly off; add it if anyone hears it.
        self.skip = 0;
        Ok(reached)
    }
}

/// Anything else, through an `ffmpeg` process that writes raw samples to a
/// pipe. Its crashes stay in its own process. Seeking restarts it.
pub struct Ffmpeg {
    path: PathBuf,
    rate: u32,
    channels: usize,
    child: Child,
    stdout: ChildStdout,
    /// Bytes read but not yet a whole sample.
    leftover: Vec<u8>,
}

/// `ffmpeg`/`ffprobe` shipped next to Motif's executable, else from the PATH.
fn ffmpeg_tool(name: &str) -> PathBuf {
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        Some(
            exe.parent()?
                .join(name)
                .with_extension(std::env::consts::EXE_EXTENSION),
        )
    });
    bundled
        .filter(|path| path.is_file())
        .unwrap_or_else(|| name.into())
}

fn not_installed(err: std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        "FFmpeg is not installed".into()
    } else {
        err.to_string()
    }
}

impl Ffmpeg {
    pub fn open(path: &Path) -> Result<Self, String> {
        let probe = Command::new(ffmpeg_tool("ffprobe"))
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=sample_rate,channels",
            ])
            .args(["-of", "csv=p=0"])
            .arg(path)
            .stdin(Stdio::null())
            .output()
            .map_err(not_installed)?;
        let info = String::from_utf8_lossy(&probe.stdout);
        let mut fields = info.trim().split(',');
        let (Some(rate), Some(channels)) = (fields.next(), fields.next()) else {
            let err = String::from_utf8_lossy(&probe.stderr);
            return Err(err
                .lines()
                .last()
                .unwrap_or("no audio stream")
                .trim()
                .to_string());
        };
        let rate: u32 = rate
            .parse()
            .map_err(|_| format!("bad sample rate {rate}"))?;
        let channels: usize = channels
            .parse()
            .map_err(|_| format!("bad channel count {channels}"))?;
        if rate == 0 || channels == 0 {
            return Err("not playable audio".into());
        }
        let (child, stdout) = Self::spawn(path, rate, channels, Duration::ZERO)?;
        Ok(Self {
            path: path.into(),
            rate,
            channels,
            child,
            stdout,
            leftover: Vec::new(),
        })
    }

    fn spawn(
        path: &Path,
        rate: u32,
        channels: usize,
        start: Duration,
    ) -> Result<(Child, ChildStdout), String> {
        let mut child = Command::new(ffmpeg_tool("ffmpeg"))
            .args([
                "-v",
                "error",
                "-nostdin",
                "-ss",
                &format!("{:.3}", start.as_secs_f64()),
                "-i",
            ])
            .arg(path)
            .args([
                "-map",
                "0:a:0",
                "-f",
                "f32le",
                "-ac",
                &channels.to_string(),
                "-ar",
                &rate.to_string(),
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(not_installed)?;
        let stdout = child.stdout.take().ok_or("no ffmpeg output")?;
        Ok((child, stdout))
    }
}

impl Decoder for Ffmpeg {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        let mut bytes = std::mem::take(&mut self.leftover);
        let start = bytes.len();
        bytes.resize(start + 64 * 1024, 0);
        let read = self
            .stdout
            .read(&mut bytes[start..])
            .map_err(|e| e.to_string())?;
        bytes.truncate(start + read);
        if read == 0 {
            let status = self.child.wait().map_err(|e| e.to_string())?;
            return if status.success() {
                Ok(false)
            } else {
                Err(format!("ffmpeg failed ({status})"))
            };
        }
        let (samples, rest) = bytes.as_chunks::<4>();
        out.extend(samples.iter().map(|b| f32::from_le_bytes(*b)));
        self.leftover = rest.to_vec();
        Ok(true)
    }

    fn seek(&mut self, to: Duration) -> Result<Duration, String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        (self.child, self.stdout) = Self::spawn(&self.path, self.rate, self.channels, to)?;
        self.leftover.clear();
        Ok(to)
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
