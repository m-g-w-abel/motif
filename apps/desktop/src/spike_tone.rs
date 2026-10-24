//! Spike (phase 0): a quiet 440 Hz tone while the engine is playing, to hear
//! whether Android keeps the process running with the screen off. Real
//! decoding and output replace this in phase 1.

use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use protocol::PlaybackStatus;

pub fn spawn(engine: engine::Engine) {
    // cpal streams aren't Send on every platform, so one thread owns it.
    std::thread::spawn(move || {
        let mut stream = None;
        loop {
            let playing = engine.state().status == PlaybackStatus::Playing;
            if playing && stream.is_none() {
                stream = open()
                    .inspect_err(|err| tracing::error!(%err, "could not open the tone stream"))
                    .ok();
            } else if !playing {
                stream = None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

fn open() -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device")?;
    let config = device.default_output_config()?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!("unsupported sample format {}", config.sample_format()).into());
    }
    let rate = config.sample_rate() as f32;
    let channels = usize::from(config.channels());
    let mut phase = 0.0f32;
    let stream = device.build_output_stream(
        config.into(),
        move |data: &mut [f32], _| {
            for frame in data.chunks_mut(channels) {
                frame.fill((phase * std::f32::consts::TAU).sin() * 0.1);
                phase = (phase + 440.0 / rate) % 1.0;
            }
        },
        |err| tracing::error!(%err, "tone stream failed"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}
