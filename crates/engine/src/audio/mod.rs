//! Audio output. A decode thread runs decoders through the [`pipeline`] into a
//! lock-free ring buffer, and the device callback only copies from it, so a
//! slow disk or a busy CPU never blocks the audio device. Position comes from
//! the frames the callback has actually played.

pub mod decoder;
pub mod pipeline;

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rtrb::{Consumer, Producer, RingBuffer};
use tokio::sync::mpsc::UnboundedSender;

use decoder::{Decoder, Plugin};
use pipeline::{Playhead, Stream, Timeline};

/// Reported by the audio threads to the engine.
#[derive(Debug)]
pub enum AudioEvent {
    /// Queue item `item` could not be opened or decoded.
    Failed { item: u64, reason: String },
    /// The output device failed (e.g. unplugged).
    DeviceLost(String),
}

/// State shared with the device callback. Atomics only: the callback must
/// never wait on a lock.
struct Shared {
    playing: AtomicBool,
    /// Gain as `f32` bits.
    gain: AtomicU32,
    /// Frames of real audio the device has played.
    played: AtomicU64,
    /// Callbacks that ran out of audio while playing.
    underruns: AtomicU64,
    /// Underruns only count once audio has arrived after a flush; an empty
    /// buffer right after a load or seek is expected.
    armed: AtomicBool,
    /// Set by the decode thread to have the callback discard buffered audio;
    /// the callback clears it once done.
    flush: AtomicBool,
}

enum Command {
    Load {
        token: u64,
        item: u64,
        path: PathBuf,
        start: Duration,
    },
    Preload {
        token: u64,
        item: u64,
        path: PathBuf,
    },
    ClearPreload,
    Seek(Duration),
    Stop,
}

/// A handle to an open output device.
pub struct Audio {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
    timeline: Arc<Mutex<Timeline>>,
    next_token: u64,
    /// Dropping this stops the output thread and closes the device.
    _stop: mpsc::Sender<()>,
}

impl Audio {
    /// Opens the default output device. Each file is opened with the first of
    /// `plugins` (as they are at the time) that can.
    pub fn open(
        device: Option<String>,
        events: UnboundedSender<AudioEvent>,
        plugins: Arc<Mutex<Vec<&'static Plugin>>>,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            gain: AtomicU32::new(1.0f32.to_bits()),
            played: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            armed: AtomicBool::new(false),
            flush: AtomicBool::new(false),
        });
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop, stop_rx) = mpsc::channel::<()>();
        {
            let shared = shared.clone();
            let events = events.clone();
            thread::Builder::new()
                .name("audio-output".into())
                .spawn(
                    move || match open_stream(device.as_deref(), shared, events) {
                        Ok((stream, producer, rate, channels)) => {
                            let _ = ready_tx.send(Ok((producer, rate, channels)));
                            // Keep the stream alive until the handle is dropped.
                            let _ = stop_rx.recv();
                            drop(stream);
                        }
                        Err(err) => {
                            let _ = ready_tx.send(Err(err));
                        }
                    },
                )
                .map_err(|e| e.to_string())?;
        }
        let (producer, rate, channels) = ready_rx.recv().map_err(|e| e.to_string())??;
        tracing::info!(rate, channels, "audio output open");

        let timeline = Arc::new(Mutex::new(Timeline::new(rate)));
        let (commands, commands_rx) = mpsc::channel();
        let feeder = Feeder {
            commands: commands_rx,
            producer,
            shared: shared.clone(),
            timeline: timeline.clone(),
            rate,
            channels,
            events,
            plugins,
        };
        thread::Builder::new()
            .name("audio-decode".into())
            .spawn(move || feeder.run())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            commands,
            shared,
            timeline,
            next_token: 1,
            _stop: stop,
        })
    }

    /// Starts decoding queue item `item` from `start`, replacing whatever was
    /// playing. Returns the token that [`Playhead::token`] reports while it is
    /// audible.
    pub fn load(&mut self, item: u64, path: PathBuf, start: Duration) -> u64 {
        let token = self.token();
        let _ = self.commands.send(Command::Load {
            token,
            item,
            path,
            start,
        });
        token
    }

    /// Prepares `item` to follow the current track without a gap.
    pub fn preload(&mut self, item: u64, path: PathBuf) -> u64 {
        let token = self.token();
        let _ = self.commands.send(Command::Preload { token, item, path });
        token
    }

    pub fn clear_preload(&self) {
        let _ = self.commands.send(Command::ClearPreload);
    }

    pub fn seek(&self, to: Duration) {
        let _ = self.commands.send(Command::Seek(to));
    }

    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn set_playing(&self, playing: bool) {
        self.shared.playing.store(playing, Ordering::Relaxed);
    }

    /// `volume` in `0.0..=1.0`, on a cubic curve so the slider feels even.
    pub fn set_volume(&self, volume: f32) {
        self.shared
            .gain
            .store(volume.powi(3).to_bits(), Ordering::Relaxed);
    }

    /// How many times the device ran out of audio while playing (including
    /// at the end of the queue).
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// What the listener is hearing now.
    pub fn playhead(&self) -> Option<Playhead> {
        let played = self.shared.played.load(Ordering::Acquire);
        self.timeline.lock().ok()?.playhead(played)
    }

    fn token(&mut self) -> u64 {
        self.next_token += 1;
        self.next_token
    }
}

type Opened = (cpal::Stream, Producer<f32>, u32, usize);

/// The output devices worth offering. ALSA lists every plugin and alias
/// for every card; only the sound servers, the default and one entry per
/// physical output are kept.
pub fn outputs() -> Vec<protocol::OutputDevice> {
    let Ok(devices) = cpal::default_host().output_devices() else {
        return Vec::new();
    };
    let all = devices.filter_map(|device| {
        Some(protocol::OutputDevice {
            id: device.id().ok()?.to_string(),
            name: device.to_string(),
        })
    });
    let mut kept: Vec<protocol::OutputDevice> = Vec::new();
    for device in all {
        let useful = match device.id.strip_prefix("alsa:") {
            Some(alsa) => {
                matches!(alsa, "default" | "pipewire" | "pulse")
                    || ["sysdefault:", "front:", "hdmi:"]
                        .iter()
                        .any(|p| alsa.starts_with(p))
            }
            None => true,
        };
        if useful && !kept.iter().any(|k| k.name == device.name) {
            kept.push(device);
        }
    }
    kept
}

fn open_stream(
    device: Option<&str>,
    shared: Arc<Shared>,
    events: UnboundedSender<AudioEvent>,
) -> Result<Opened, String> {
    let host = cpal::default_host();
    // A chosen device that has gone away falls back to the default.
    let chosen = device.and_then(|id| host.device_by_id(&id.parse().ok()?));
    if device.is_some() && chosen.is_none() {
        tracing::warn!(?device, "output device not found, using the default");
    }
    let device = chosen
        .or_else(|| host.default_output_device())
        .ok_or("no audio output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = supported.sample_rate();
    let channels = usize::from(supported.channels());
    // Half a second: enough to ride out disk hiccups, short enough that
    // queue changes reach the speakers quickly.
    let (producer, consumer) = RingBuffer::new(rate as usize * channels / 2);
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, consumer, shared, events),
        SampleFormat::I16 => build::<i16>(&device, &config, consumer, shared, events),
        SampleFormat::U16 => build::<u16>(&device, &config, consumer, shared, events),
        SampleFormat::I32 => build::<i32>(&device, &config, consumer, shared, events),
        other => Err(format!("unsupported output sample format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, producer, rate, channels))
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: Consumer<f32>,
    shared: Arc<Shared>,
    events: UnboundedSender<AudioEvent>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    // Volume changes ramp over ~10 ms to avoid clicks.
    let ramp = 1.0 / (config.sample_rate as f32 * 0.01);
    let mut gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                if shared.flush.load(Ordering::Acquire) {
                    if let Ok(chunk) = consumer.read_chunk(consumer.slots()) {
                        chunk.commit_all();
                    }
                    shared.flush.store(false, Ordering::Release);
                }
                let silence = T::from_sample(0.0f32);
                if !shared.playing.load(Ordering::Relaxed) {
                    data.fill(silence);
                    return;
                }
                let target = f32::from_bits(shared.gain.load(Ordering::Relaxed));
                let available = consumer.slots() / channels * channels;
                let take = available.min(data.len());
                if let Ok(chunk) = consumer.read_chunk(take) {
                    let (a, b) = chunk.as_slices();
                    for (i, (out, sample)) in data.iter_mut().zip(a.iter().chain(b)).enumerate() {
                        if i % channels == 0 {
                            gain += (target - gain).clamp(-ramp, ramp);
                        }
                        *out = T::from_sample(sample * gain);
                    }
                    chunk.commit_all();
                }
                if take < data.len() && shared.armed.load(Ordering::Relaxed) {
                    shared.underruns.fetch_add(1, Ordering::Relaxed);
                }
                data[take..].fill(silence);
                shared
                    .played
                    .fetch_add((take / channels) as u64, Ordering::Release);
            },
            move |err| {
                let _ = events.send(AudioEvent::DeviceLost(err.to_string()));
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// The decode thread: keeps the ring buffer full from the current track and
/// splices the preloaded one in at the exact sample when it ends.
struct Feeder {
    commands: mpsc::Receiver<Command>,
    producer: Producer<f32>,
    shared: Arc<Shared>,
    timeline: Arc<Mutex<Timeline>>,
    rate: u32,
    channels: usize,
    events: UnboundedSender<AudioEvent>,
    plugins: Arc<Mutex<Vec<&'static Plugin>>>,
}

struct Loaded {
    token: u64,
    item: u64,
    decoder: Box<dyn Decoder>,
}

impl Feeder {
    fn run(mut self) {
        let mut current: Option<Loaded> = None;
        let mut next: Option<Loaded> = None;
        // A preload waits to be opened until the buffer is full, so opening a
        // file (slow on a busy disk) never starves the current track.
        let mut next_request: Option<(u64, u64, PathBuf)> = None;
        let mut stream = Stream::new(self.rate, self.channels);
        stream.reset(self.shared.played.load(Ordering::Acquire));
        let mut decoded = Vec::new();
        // Converted audio not yet in the ring buffer.
        let mut pending: Vec<f32> = Vec::new();
        let mut sent = 0;

        loop {
            let full = self.producer.slots() < self.channels * 256;
            if full {
                // Filled since the last flush: from now on running dry is an underrun.
                self.shared.armed.store(true, Ordering::Relaxed);
                if let Some((token, item, path)) = next_request.take() {
                    next = self.open(token, item, &path);
                }
            }
            let busy = current.is_some() || sent < pending.len();
            let command = if !busy {
                match self.commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                }
            } else if full {
                match self.commands.recv_timeout(Duration::from_millis(10)) {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match self.commands.try_recv() {
                    Ok(command) => Some(command),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            };

            match command {
                Some(Command::Load {
                    token,
                    item,
                    path,
                    start,
                }) => {
                    self.flush(&mut stream, &mut pending, &mut sent);
                    next = None;
                    next_request = None;
                    current = self.open(token, item, &path).and_then(|mut loaded| {
                        let reached = if start.is_zero() {
                            Duration::ZERO
                        } else {
                            loaded
                                .decoder
                                .seek(start)
                                .map_err(|reason| self.fail(item, reason))
                                .ok()?
                        };
                        self.lock().start(stream.next_frame(), token, item, reached);
                        Some(loaded)
                    });
                }
                Some(Command::Preload { token, item, path }) => {
                    next = None;
                    next_request = Some((token, item, path));
                }
                Some(Command::ClearPreload) => {
                    next = None;
                    next_request = None;
                }
                Some(Command::Seek(to)) => {
                    if let Some(loaded) = &mut current {
                        self.flush(&mut stream, &mut pending, &mut sent);
                        match loaded.decoder.seek(to) {
                            Ok(reached) => self.lock().start(
                                stream.next_frame(),
                                loaded.token,
                                loaded.item,
                                reached,
                            ),
                            Err(reason) => {
                                self.fail(loaded.item, reason);
                                current = None;
                            }
                        }
                    }
                }
                Some(Command::Stop) => {
                    self.flush(&mut stream, &mut pending, &mut sent);
                    current = None;
                    next = None;
                    next_request = None;
                }
                None => {}
            }

            // Decode more once the previous block is in the buffer.
            if sent == pending.len()
                && let Some(loaded) = &mut current
            {
                pending.clear();
                sent = 0;
                decoded.clear();
                let result = loaded.decoder.decode(&mut decoded).and_then(|more| {
                    let (rate, channels) =
                        (loaded.decoder.sample_rate(), loaded.decoder.channels());
                    stream
                        .push(rate, channels, &decoded, &mut pending)
                        .map(|()| more)
                });
                match result {
                    Ok(true) => {}
                    Ok(false) => {
                        // Track done: the preloaded one starts on the very next frame.
                        if let Some((token, item, path)) = next_request.take() {
                            next = self.open(token, item, &path);
                        }
                        current = next.take();
                        match &current {
                            Some(n) => self.lock().start(
                                stream.next_frame(),
                                n.token,
                                n.item,
                                Duration::ZERO,
                            ),
                            None => {
                                if let Err(reason) = stream.finish(&mut pending) {
                                    tracing::warn!(%reason, "resampler flush failed");
                                }
                                self.lock().end(stream.next_frame());
                            }
                        }
                    }
                    Err(reason) => {
                        self.fail(loaded.item, reason);
                        self.lock().end(stream.next_frame());
                        current = None;
                    }
                }
            }

            // Move pending audio into the ring buffer, whole frames at a time.
            let room =
                (self.producer.slots() / self.channels * self.channels).min(pending.len() - sent);
            if room > 0
                && let Ok(mut chunk) = self.producer.write_chunk(room)
            {
                let (a, b) = chunk.as_mut_slices();
                a.copy_from_slice(&pending[sent..sent + a.len()]);
                b.copy_from_slice(&pending[sent + a.len()..sent + room]);
                chunk.commit_all();
                sent += room;
            }
        }
    }

    fn open(&self, token: u64, item: u64, path: &std::path::Path) -> Option<Loaded> {
        let plugins = self.plugins.lock().map(|p| p.clone()).unwrap_or_default();
        decoder::open(path, &plugins)
            .map(|decoder| Loaded {
                token,
                item,
                decoder,
            })
            .map_err(|reason| self.fail(item, reason))
            .ok()
    }

    fn fail(&self, item: u64, reason: String) {
        tracing::warn!(item, %reason, "cannot play");
        let _ = self.events.send(AudioEvent::Failed { item, reason });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Timeline> {
        self.timeline
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Discards everything buffered and restarts the stream at the frame the
    /// device plays next.
    fn flush(&mut self, stream: &mut Stream, pending: &mut Vec<f32>, sent: &mut usize) {
        pending.clear();
        *sent = 0;
        self.shared.armed.store(false, Ordering::Relaxed);
        self.shared.flush.store(true, Ordering::Release);
        // The callback runs every few milliseconds; give up after a while in
        // case the device has stalled.
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.shared.flush.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        self.lock().clear();
        stream.reset(self.shared.played.load(Ordering::Acquire));
    }
}
