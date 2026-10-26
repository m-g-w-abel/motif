//! Decoder plugins. Every decoder is compiled in and implements [`Decoder`];
//! for each file the engine tries them in order and uses the first that
//! opens it.

use std::{fs::File, path::Path, time::Duration};

use symphonia::core::{
    codecs::{CodecParameters, audio::AudioDecoder},
    errors::Error as SymError,
    formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint},
    io::MediaSourceStream,
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

impl Symphonia {
    pub fn open(path: &Path) -> Result<Self, String> {
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
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &Default::default())
            .map_err(|e| e.to_string())?;
        let rate = params.sample_rate.ok_or("unknown sample rate")?;
        let channels = params.channels.as_ref().map_or(0, |c| c.count());
        if channels == 0 {
            return Err("unknown channel layout".into());
        }
        let track_id = track.id;
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
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
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
        let time = Time::try_from_secs_f64(to.as_secs_f64()).ok_or("bad seek time")?;
        let seeked = self
            .reader
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| e.to_string())?;
        self.decoder.reset();
        let time_base = self
            .reader
            .tracks()
            .iter()
            .find(|t| t.id == self.track_id)
            .and_then(|t| t.time_base);
        let reached = time_base
            .and_then(|tb| tb.calc_time(seeked.actual_ts))
            .map_or(to, |t| Duration::from_secs_f64(t.as_secs_f64().max(0.0)));
        Ok(reached)
    }
}
