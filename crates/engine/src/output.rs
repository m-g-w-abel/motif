//! Keeps the audio output in step with the [`Player`]: loads what it should
//! be playing, preloads what comes next, follows seeks, pause and volume, and
//! reports back what the listener actually hears.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use protocol::{PlaybackStatus, ReplayGain};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    Playable, Player, Resolver,
    audio::{Audio, AudioEvent, decoder::Plugin},
};

/// Larger differences between the player's clock and the audio output are
/// corrected (and published).
const MAX_DRIFT: Duration = Duration::from_millis(150);

pub struct Output {
    /// Opened on first use and dropped if the device fails.
    audio: Option<Audio>,
    /// The device `audio` was opened on.
    device: Option<String>,
    events: UnboundedSender<AudioEvent>,
    /// Shared with the decode thread, which reads it for every file it opens.
    plugins: Arc<Mutex<Vec<&'static Plugin>>>,
    /// Token, queue item and ReplayGain factor of what is loaded.
    loaded: Option<(u64, u64, f32)>,
    /// What is preloaded to follow it.
    preloaded: Option<Preloaded>,
    /// The player's seek count when the output last followed it.
    seeks: u64,
    underruns: u64,
    /// Whether a visualiser wants [`Output::bands`].
    tap: bool,
}

struct Preloaded {
    token: u64,
    item: u64,
    path: PathBuf,
    tags: ReplayGain,
    gain: f32,
}

/// A ReplayGain adjustment as a linear factor.
fn factor(db: Option<f32>) -> f32 {
    db.map_or(1.0, |db| 10f32.powf(db / 20.0))
}

impl Output {
    pub fn new(events: UnboundedSender<AudioEvent>) -> Self {
        Self {
            audio: None,
            device: None,
            events,
            plugins: Arc::default(),
            loaded: None,
            preloaded: None,
            seeks: 0,
            underruns: 0,
            tap: false,
        }
    }

    /// Makes the output match the player after it changed.
    pub fn reconcile(&mut self, player: &mut Player, resolver: &dyn Resolver, now: Instant) {
        if let Ok(mut plugins) = self.plugins.lock() {
            *plugins = player.decoders();
        }
        if self.audio.is_some() && self.device.as_deref() != player.output() {
            // Switch devices: reopen below and carry on from the current position.
            self.audio = None;
            self.loaded = None;
            self.preloaded = None;
        }
        let wanted = match (player.current_item(), player.playable()) {
            (Some(item), Some(Playable::File(path, tags)))
                if player.status() != PlaybackStatus::Stopped =>
            {
                Some((item.id, path.clone(), factor(player.gain_db(tags, false))))
            }
            _ => None,
        };
        let Some((item, path, gain)) = wanted else {
            if self.loaded.take().is_some()
                && let Some(audio) = &self.audio
            {
                audio.stop();
            }
            self.preloaded = None;
            return;
        };

        if self.audio.is_none() {
            let device = player.output().map(str::to_string);
            match Audio::open(device.clone(), self.events.clone(), self.plugins.clone()) {
                Ok(mut audio) => {
                    audio.set_tap(self.tap);
                    self.audio = Some(audio);
                    self.device = device;
                }
                Err(reason) => {
                    player.fail(item, format!("no audio output: {reason}"), now);
                    return;
                }
            }
        }
        let Some(audio) = &mut self.audio else { return };

        if self.loaded.map(|(_, i, _)| i) != Some(item) {
            let token = audio.load(item, path, player.position(now), gain);
            self.loaded = Some((token, item, gain));
            self.preloaded = None;
            self.seeks = player.seeks();
        } else if self.seeks != player.seeks() {
            audio.seek(player.position(now));
            self.seeks = player.seeks();
        }
        if let Some((token, _, sent)) = &mut self.loaded
            && *sent != gain
        {
            audio.set_gain(*token, gain);
            *sent = gain;
        }
        audio.set_playing(player.status() == PlaybackStatus::Playing);
        // On Android the volume is the system media volume (`apps/desktop/src/media.rs`), so no gain of our own.
        audio.set_volume(if cfg!(target_os = "android") {
            1.0
        } else {
            player.volume()
        });

        let upcoming = player
            .upcoming()
            .and_then(|next| match resolver.resolve(&next.track) {
                Ok(Playable::File(path, tags)) => {
                    if player.measures() && tags.track_gain.is_none() {
                        // Measured before it starts; the engine re-resolves once it is.
                        resolver.measure(std::slice::from_ref(&next.track), false);
                    }
                    Some((next.id, path, tags, factor(player.gain_db(&tags, true))))
                }
                _ => None,
            });
        match upcoming {
            Some((item, path, tags, gain)) => match &mut self.preloaded {
                Some(next) if next.item == item && next.path == path => {
                    if next.gain != gain {
                        audio.set_gain(next.token, gain);
                        next.gain = gain;
                    }
                }
                _ => {
                    let token = audio.preload(item, path.clone(), gain);
                    self.preloaded = Some(Preloaded {
                        token,
                        item,
                        path,
                        tags,
                        gain,
                    });
                }
            },
            None => {
                if self.preloaded.take().is_some() {
                    audio.clear_preload();
                }
            }
        }
    }

    /// Follows what the listener hears: gapless moves to the next track, the
    /// end of the queue, and clock drift. Returns whether the player changed.
    pub fn poll(&mut self, player: &mut Player, now: Instant) -> bool {
        let (Some(audio), Some((token, _, _))) = (&self.audio, self.loaded) else {
            return false;
        };
        let Some(head) = audio.playhead() else {
            return false;
        };
        let underruns = audio.underruns();
        if underruns > self.underruns && !head.ended {
            tracing::warn!(
                count = underruns - self.underruns,
                "audio underrun: decoding fell behind"
            );
        }
        self.underruns = underruns;
        match heard(
            head.token,
            token,
            self.preloaded.as_ref().map(|next| next.token),
        ) {
            Heard::Loaded => {}
            Heard::Draining => return false,
            Heard::Preloaded => {
                let Some(next) = self.preloaded.take() else {
                    return false;
                };
                self.loaded = Some((next.token, next.item, next.gain));
                if player.moved_to(next.item, Playable::File(next.path, next.tags), now) {
                    player.sync_position(head.position, now);
                }
                return true;
            }
            Heard::Stale => {
                // A preload the queue no longer wants was already spliced in:
                // reload what the player says is playing.
                self.loaded = None;
                self.preloaded = None;
                return true;
            }
        }
        if head.ended {
            self.loaded = None;
            player.track_ended(now);
            return true;
        }
        if player.position(now).abs_diff(head.position) > MAX_DRIFT {
            player.sync_position(head.position, now);
            return true;
        }
        false
    }

    /// Starts or stops collecting audio for [`Output::bands`].
    pub fn set_tap(&mut self, on: bool) {
        self.tap = on;
        if let Some(audio) = &mut self.audio {
            audio.set_tap(on);
        }
    }

    /// Spectrum bands of the audio played since the last call, if any.
    pub fn bands(&mut self) -> Option<Vec<u8>> {
        self.audio.as_mut()?.bands()
    }

    pub fn handle(&mut self, event: AudioEvent, player: &mut Player, now: Instant) {
        match event {
            AudioEvent::Failed { item, reason } => {
                if self.loaded.is_some_and(|(_, i, _)| i == item) {
                    self.loaded = None;
                }
                if self
                    .preloaded
                    .as_ref()
                    .is_some_and(|next| next.item == item)
                {
                    self.preloaded = None;
                }
                player.fail(item, reason, now);
            }
            AudioEvent::DeviceLost(reason) => {
                tracing::warn!(%reason, "audio output lost");
                // Reopened (perhaps on another device) when playback resumes.
                self.audio = None;
                self.loaded = None;
                self.preloaded = None;
                let _ = player.apply(protocol::Command::Pause, now);
            }
        }
    }
}

/// What the audible audio is, judged by its token.
#[derive(Debug, PartialEq)]
enum Heard {
    /// What was loaded.
    Loaded,
    /// The preload, spliced in after the loaded track: a gapless move.
    Preloaded,
    /// Audio from before the last load, still draining.
    Draining,
    /// A preload that has since been replaced or dropped.
    Stale,
}

/// Tokens only grow, so older ones are draining audio and newer ones are a
/// preload, wanted or not.
fn heard(audible: u64, loaded: u64, preloaded: Option<u64>) -> Heard {
    if audible == loaded {
        Heard::Loaded
    } else if Some(audible) == preloaded {
        Heard::Preloaded
    } else if audible < loaded {
        Heard::Draining
    } else {
        Heard::Stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audible_tokens_are_judged_by_age() {
        assert_eq!(heard(5, 5, Some(6)), Heard::Loaded);
        assert_eq!(heard(6, 5, Some(6)), Heard::Preloaded);
        assert_eq!(heard(3, 5, Some(6)), Heard::Draining);
        // The next track was removed (no preload) or replaced (7) after it was spliced in.
        assert_eq!(heard(6, 5, None), Heard::Stale);
        assert_eq!(heard(6, 5, Some(7)), Heard::Stale);
    }
}
