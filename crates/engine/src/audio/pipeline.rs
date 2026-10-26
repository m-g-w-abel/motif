//! Turns decoders into a continuous stream in the output's format, and keeps
//! track of which track and position each output frame belongs to. Nothing
//! here touches the audio device, so it is tested directly.

use std::{collections::VecDeque, time::Duration};

use rubato::{Fft, FixedSync, Indexing, Resampler, audioadapter_buffers::direct::InterleavedSlice};

/// Builds the continuous output stream from decoded blocks: maps channels and
/// resamples to the output rate. One resampler carries on across tracks that
/// share a sample rate, so they join without the ringing a resampler adds at
/// the edges of a stream.
pub struct Stream {
    out_rate: u32,
    out_channels: usize,
    /// Input rate and resampler, when the input isn't at the output rate.
    resample: Option<(u32, Resample)>,
    /// Output frame where the current run of same-rate input started.
    base: u64,
    /// Input frames pushed in that run.
    input_frames: u64,
    rate: u32,
    mapped: Vec<f32>,
}

impl Stream {
    pub fn new(out_rate: u32, out_channels: usize) -> Self {
        Self {
            out_rate,
            out_channels,
            resample: None,
            base: 0,
            input_frames: 0,
            rate: out_rate,
            mapped: Vec::new(),
        }
    }

    /// Starts over at output frame `frame`, e.g. after a flush or seek.
    pub fn reset(&mut self, frame: u64) {
        self.resample = None;
        self.base = frame;
        self.input_frames = 0;
        self.rate = self.out_rate;
    }

    /// The output frame where the next input sample will be heard: where a
    /// track that starts now begins.
    pub fn next_frame(&self) -> u64 {
        let elapsed = self.input_frames as f64 * f64::from(self.out_rate) / f64::from(self.rate);
        self.base + elapsed.round() as u64
    }

    /// Converts `samples` (interleaved, `channels` wide, at `rate`) and
    /// appends the result to `out`.
    pub fn push(
        &mut self,
        rate: u32,
        channels: usize,
        samples: &[f32],
        out: &mut Vec<f32>,
    ) -> Result<(), String> {
        if rate != self.rate {
            // A new run: flush the old resampler's tail first.
            let next = self.next_frame();
            self.finish(out)?;
            self.base = next;
            self.input_frames = 0;
            self.rate = rate;
            if rate != self.out_rate {
                self.resample =
                    Some((rate, Resample::new(rate, self.out_rate, self.out_channels)?));
            }
        }
        self.mapped.clear();
        map_channels(samples, channels, self.out_channels, &mut self.mapped);
        self.input_frames += (samples.len() / channels) as u64;
        match &mut self.resample {
            Some((_, resample)) => resample.push(&self.mapped, out),
            None => {
                out.extend_from_slice(&self.mapped);
                Ok(())
            }
        }
    }

    /// Flushes what the resampler still holds, at the end of the stream.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        match self.resample.take() {
            Some((_, mut resample)) => resample.finish(out),
            None => Ok(()),
        }
    }
}

/// Fixed-ratio resampling that trims the resampler's start-up delay and
/// flushes its tail, so output length matches input length exactly (in time)
/// and tracks join without a gap or overlap.
struct Resample {
    fft: Fft<f32>,
    channels: usize,
    ratio: f64,
    /// Input waiting for a full chunk.
    input: Vec<f32>,
    output: Vec<f32>,
    in_frames: u64,
    out_frames: u64,
    delay_left: usize,
}

const CHUNK: usize = 1024;

impl Resample {
    fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let fft = Fft::new(
            from as usize,
            to as usize,
            CHUNK,
            channels,
            FixedSync::Input,
        )
        .map_err(|e| e.to_string())?;
        let output = vec![0.0; fft.output_frames_max() * channels];
        let delay_left = fft.output_delay();
        Ok(Self {
            fft,
            channels,
            ratio: f64::from(to) / f64::from(from),
            input: Vec::new(),
            output,
            in_frames: 0,
            out_frames: 0,
            delay_left,
        })
    }

    fn push(&mut self, samples: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        self.in_frames += (samples.len() / self.channels) as u64;
        self.input.extend_from_slice(samples);
        let chunk = self.fft.input_frames_next() * self.channels;
        let mut offset = 0;
        while self.input.len() - offset >= chunk {
            self.process(offset, None, out)?;
            offset += chunk;
        }
        self.input.drain(..offset);
        Ok(())
    }

    /// Flushes what is left, padding with silence, until the output covers
    /// all the input.
    fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        let expected = (self.in_frames as f64 * self.ratio).round() as u64;
        let mut partial = self.input.len() / self.channels;
        while self.out_frames < expected {
            self.input
                .resize(self.fft.input_frames_next() * self.channels, 0.0);
            self.process(0, Some(partial), out)?;
            partial = 0;
        }
        // The last chunk overshoots; drop what lies past the real end.
        let extra = (self.out_frames - expected) as usize * self.channels;
        out.truncate(out.len() - extra.min(out.len()));
        self.input.clear();
        Ok(())
    }

    fn process(
        &mut self,
        offset: usize,
        partial: Option<usize>,
        out: &mut Vec<f32>,
    ) -> Result<(), String> {
        let frames_in = self.input.len() / self.channels;
        let input = InterleavedSlice::new(&self.input, self.channels, frames_in)
            .map_err(|e| e.to_string())?;
        let frames_out = self.output.len() / self.channels;
        let mut output = InterleavedSlice::new_mut(&mut self.output, self.channels, frames_out)
            .map_err(|e| e.to_string())?;
        let mut indexing = Indexing::new();
        indexing.input_offset = offset / self.channels;
        indexing.partial_len = partial;
        let (_, produced) = self
            .fft
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|e| e.to_string())?;
        let skip = self.delay_left.min(produced);
        self.delay_left -= skip;
        out.extend_from_slice(&self.output[skip * self.channels..produced * self.channels]);
        self.out_frames += (produced - skip) as u64;
        Ok(())
    }
}

/// Maps interleaved audio between channel counts: mono is copied to every
/// output channel; otherwise channels are kept in order, extra input
/// channels dropped and missing ones silent.
// shortcut: drops surround channels instead of downmixing them, add a proper downmix with the DSP work in phase 9.
pub fn map_channels(input: &[f32], from: usize, to: usize, out: &mut Vec<f32>) {
    if from == to {
        out.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(from) {
        for c in 0..to {
            out.push(if from == 1 {
                frame[0]
            } else {
                frame.get(c).copied().unwrap_or(0.0)
            });
        }
    }
}

/// What the listener is hearing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playhead {
    /// Which load or preload is audible (see [`Timeline::start`]).
    pub token: u64,
    /// The queue item being played.
    pub item: u64,
    pub position: Duration,
    /// Everything queued has been heard and nothing follows.
    pub ended: bool,
}

#[derive(Debug, Clone, Copy)]
enum Mark {
    Start {
        token: u64,
        item: u64,
        position: Duration,
    },
    End,
}

/// Where in the output stream each track starts, so a count of frames
/// played becomes a track and a position.
#[derive(Debug)]
pub struct Timeline {
    rate: u32,
    marks: VecDeque<(u64, Mark)>,
}

impl Timeline {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            marks: VecDeque::new(),
        }
    }

    /// Forgets everything, e.g. after the buffer was flushed.
    pub fn clear(&mut self) {
        self.marks.clear();
    }

    /// Output frame `frame` is `position` into queue item `item`.
    pub fn start(&mut self, frame: u64, token: u64, item: u64, position: Duration) {
        self.marks.push_back((
            frame,
            Mark::Start {
                token,
                item,
                position,
            },
        ));
    }

    /// Nothing follows output frame `frame`.
    pub fn end(&mut self, frame: u64) {
        self.marks.push_back((frame, Mark::End));
    }

    /// What is audible once `played` frames have been heard.
    pub fn playhead(&mut self, played: u64) -> Option<Playhead> {
        // Drop starts that a later, already-reached start replaces.
        while self.marks.len() > 1
            && self.marks[1].0 <= played
            && matches!(self.marks[1].1, Mark::Start { .. })
        {
            self.marks.pop_front();
        }
        let (
            start,
            Mark::Start {
                token,
                item,
                position,
            },
        ) = *self.marks.front()?
        else {
            return None;
        };
        let end = self
            .marks
            .get(1)
            .filter(|(_, m)| matches!(m, Mark::End))
            .map(|(f, _)| *f);
        let heard = played.min(end.unwrap_or(u64::MAX)).saturating_sub(start);
        Some(Playhead {
            token,
            item,
            position: position + Duration::from_secs_f64(heard as f64 / f64::from(self.rate)),
            ended: end.is_some_and(|end| played >= end),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decoder::Decoder;

    /// A sine wave split across decoders, for continuity checks.
    struct Sine {
        rate: u32,
        next: u64,
        end: u64,
    }

    impl Decoder for Sine {
        fn sample_rate(&self) -> u32 {
            self.rate
        }
        fn channels(&self) -> usize {
            1
        }
        fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
            let n = (self.end - self.next).min(4096);
            for i in self.next..self.next + n {
                out.push((i as f32 * 440.0 * std::f32::consts::TAU / self.rate as f32).sin() * 0.5);
            }
            self.next += n;
            Ok(self.next < self.end)
        }
        fn seek(&mut self, to: Duration) -> Result<Duration, String> {
            self.next = (to.as_secs_f64() * f64::from(self.rate)) as u64;
            Ok(to)
        }
    }

    /// Runs decoders back to back through one stream, as the decode thread does.
    fn play(decoders: Vec<Sine>, out_rate: u32, out_channels: usize) -> (Vec<f32>, Vec<u64>) {
        let mut stream = Stream::new(out_rate, out_channels);
        let (mut out, mut starts, mut block) = (Vec::new(), Vec::new(), Vec::new());
        for mut decoder in decoders {
            starts.push(stream.next_frame());
            loop {
                block.clear();
                let more = decoder.decode(&mut block).unwrap();
                stream
                    .push(decoder.sample_rate(), decoder.channels(), &block, &mut out)
                    .unwrap();
                if !more {
                    break;
                }
            }
        }
        stream.finish(&mut out).unwrap();
        (out, starts)
    }

    #[test]
    fn resampling_keeps_length_exact() {
        let (out, _) = play(
            vec![Sine {
                rate: 44_100,
                next: 0,
                end: 44_100,
            }],
            48_000,
            2,
        );
        assert_eq!(out.len(), 48_000 * 2);
    }

    #[test]
    fn halves_of_a_sine_join_without_a_click() {
        for rate in [48_000, 44_100] {
            let halves = vec![
                Sine {
                    rate,
                    next: 0,
                    end: 30_000,
                },
                Sine {
                    rate,
                    next: 30_000,
                    end: 60_000,
                },
            ];
            let (out, starts) = play(halves, 48_000, 1);
            let boundary = starts[1] as usize;
            assert_eq!(
                boundary,
                (30_000.0 * 48_000.0 / f64::from(rate)).round() as usize
            );
            // A clean 440 Hz sine at 0.5 amplitude never steps more than this per sample.
            let max_step = 0.5 * 440.0 * std::f32::consts::TAU / 48_000.0 * 1.2;
            for i in boundary - 64..boundary + 64 {
                let step = (out[i + 1] - out[i]).abs();
                assert!(
                    step < max_step,
                    "rate {rate}: step {step} at {i} (boundary {boundary})"
                );
            }
        }
    }

    #[test]
    fn channels_map_up_and_down() {
        let mut out = Vec::new();
        map_channels(&[0.1, 0.2], 1, 2, &mut out);
        assert_eq!(out, [0.1, 0.1, 0.2, 0.2]);
        out.clear();
        map_channels(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 6, 2, &mut out);
        assert_eq!(out, [1.0, 2.0]);
    }

    #[test]
    fn timeline_follows_tracks_and_the_end() {
        let mut t = Timeline::new(1000);
        t.start(0, 1, 10, Duration::ZERO);
        t.start(500, 2, 11, Duration::ZERO);
        t.end(800);
        let at = |t: &mut Timeline, played| t.playhead(played).unwrap();
        assert_eq!(at(&mut t, 250).item, 10);
        assert_eq!(at(&mut t, 250).position, Duration::from_millis(250));
        assert_eq!(at(&mut t, 600).token, 2);
        assert_eq!(at(&mut t, 600).position, Duration::from_millis(100));
        assert!(!at(&mut t, 799).ended);
        let end = at(&mut t, 900);
        assert!(end.ended);
        assert_eq!(end.position, Duration::from_millis(300));
    }
}
