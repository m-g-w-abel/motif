//! Spectrum bands for visualisers: a short FFT over the most recent output
//! samples, summed into log-spaced bands. Runs on the engine task, never in
//! the audio callback.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex};

/// Bands per frame, from [`LOW`] to [`HIGH`] Hz.
pub const BANDS: usize = 32;
const LOW: f32 = 40.0;
const HIGH: f32 = 16_000.0;
/// Samples per FFT: 43 ms at 48 kHz, short enough to follow beats.
const SIZE: usize = 2048;
/// Band levels shown, in dB below a full-scale sine filling one band.
const RANGE_DB: f32 = 60.0;

pub struct Analyser {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// The latest `SIZE` mono samples, oldest first.
    history: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// FFT bins `edges[i]..edges[i + 1]` make up band `i`.
    edges: [usize; BANDS + 1],
    /// Power of a full-scale sine after the window, for 0 dB.
    full_scale: f32,
}

impl Analyser {
    pub fn new(rate: u32) -> Self {
        let fft = RealFftPlanner::new().plan_fft_forward(SIZE);
        let window: Vec<f32> = (0..SIZE)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
            .collect();
        let bin_hz = rate as f32 / SIZE as f32;
        let top = (HIGH.min(rate as f32 / 2.0) / bin_hz) as usize;
        let mut edges = [0; BANDS + 1];
        for (i, edge) in edges.iter_mut().enumerate() {
            let hz = LOW * (HIGH / LOW).powf(i as f32 / BANDS as f32);
            *edge = ((hz / bin_hz).round() as usize).clamp(1, top);
        }
        // Every band gets at least one bin, so low bands aren't empty.
        for i in 1..=BANDS {
            edges[i] = edges[i].max(edges[i - 1] + 1);
        }
        let gain: f32 = window.iter().sum::<f32>() / 2.0;
        Self {
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window,
            history: vec![0.0; SIZE],
            edges,
            full_scale: gain * gain,
        }
    }

    /// Adds interleaved output samples, `channels` wide.
    pub fn push(&mut self, samples: &[f32], channels: usize) {
        let frames = samples.len() / channels;
        let skip = frames.saturating_sub(SIZE);
        let new = frames - skip;
        self.history.copy_within(new.., 0);
        let start = SIZE - new;
        for (out, frame) in self.history[start..]
            .iter_mut()
            .zip(samples.chunks_exact(channels).skip(skip))
        {
            *out = frame.iter().sum::<f32>() / channels as f32;
        }
    }

    /// Band levels of the latest samples, `0` (silence, or `RANGE_DB` down) to `255` (full scale).
    pub fn bands(&mut self) -> Vec<u8> {
        for ((input, sample), w) in self.input.iter_mut().zip(&self.history).zip(&self.window) {
            *input = sample * w;
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_err()
        {
            return vec![0; BANDS];
        }
        self.edges
            .windows(2)
            .map(|band| {
                let power: f32 = self.spectrum[band[0]..band[1]]
                    .iter()
                    .map(|c| c.norm_sqr())
                    .sum();
                let db = 10.0 * (power / self.full_scale).max(1e-12).log10();
                ((db + RANGE_DB) / RANGE_DB * 255.0).clamp(0.0, 255.0) as u8
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
                [s, s]
            })
            .collect()
    }

    #[test]
    fn a_tone_lights_its_band() {
        let mut analyser = Analyser::new(48_000);
        analyser.push(&sine(1000.0, 48_000, 4096), 2);
        let bands = analyser.bands();
        let loudest = (0..BANDS).max_by_key(|&i| bands[i]).unwrap();
        let hz = LOW * (HIGH / LOW).powf(loudest as f32 / BANDS as f32);
        assert!((700.0..=1000.0).contains(&hz), "band {loudest} at {hz} Hz");
        assert!(bands[loudest] > 240, "full scale: {bands:?}");
        assert!(bands[0] < 60 && bands[BANDS - 1] < 60, "{bands:?}");
    }

    #[test]
    fn silence_is_zero_and_pushes_in_pieces() {
        let mut analyser = Analyser::new(44_100);
        analyser.push(&sine(200.0, 44_100, 4096), 2);
        for _ in 0..10 {
            analyser.push(&vec![0.0; 512], 2);
        }
        assert_eq!(analyser.bands(), vec![0; BANDS]);
    }
}
