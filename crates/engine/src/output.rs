//! Keeps the audio output in step with the [`Player`]: loads what it should
//! be playing, preloads what comes next, follows seeks, pause and volume, and
//! reports back what the listener actually hears.

use std::{path::PathBuf, time::Duration, time::Instant};

use protocol::PlaybackStatus;
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
    events: UnboundedSender<AudioEvent>,
    plugins: Vec<&'static Plugin>,
    /// Token and queue item of what is loaded.
    loaded: Option<(u64, u64)>,
    /// Token, queue item and file of what is preloaded to follow it.
    preloaded: Option<(u64, u64, PathBuf)>,
    /// The player's seek count when the output last followed it.
    seeks: u64,
}

impl Output {
    pub fn new(events: UnboundedSender<AudioEvent>, plugins: Vec<&'static Plugin>) -> Self {
        Self {
            audio: None,
            events,
            plugins,
            loaded: None,
            preloaded: None,
            seeks: 0,
        }
    }

    /// Makes the output match the player after it changed.
    pub fn reconcile(&mut self, player: &mut Player, resolver: &dyn Resolver, now: Instant) {
        let wanted = match (player.current_item(), player.playable()) {
            (Some(item), Some(Playable::File(path)))
                if player.status() != PlaybackStatus::Stopped =>
            {
                Some((item.id, path.clone()))
            }
            _ => None,
        };
        let Some((item, path)) = wanted else {
            if self.loaded.take().is_some()
                && let Some(audio) = &self.audio
            {
                audio.stop();
            }
            self.preloaded = None;
            return;
        };

        if self.audio.is_none() {
            match Audio::open(self.events.clone(), self.plugins.clone()) {
                Ok(audio) => self.audio = Some(audio),
                Err(reason) => {
                    player.fail(item, format!("no audio output: {reason}"), now);
                    return;
                }
            }
        }
        let Some(audio) = &mut self.audio else { return };

        if self.loaded.map(|(_, i)| i) != Some(item) {
            let token = audio.load(item, path, player.position(now));
            self.loaded = Some((token, item));
            self.preloaded = None;
            self.seeks = player.seeks();
        } else if self.seeks != player.seeks() {
            audio.seek(player.position(now));
            self.seeks = player.seeks();
        }
        audio.set_playing(player.status() == PlaybackStatus::Playing);
        audio.set_volume(player.volume());

        let upcoming = player
            .upcoming()
            .and_then(|next| match resolver.resolve(&next.track) {
                Ok(Playable::File(path)) => Some((next.id, path)),
                _ => None,
            });
        match upcoming {
            Some((next, path)) => {
                if self
                    .preloaded
                    .as_ref()
                    .is_none_or(|(_, item, p)| *item != next || *p != path)
                {
                    let token = audio.preload(next, path.clone());
                    self.preloaded = Some((token, next, path));
                }
            }
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
        let (Some(audio), Some((token, _))) = (&self.audio, self.loaded) else {
            return false;
        };
        let Some(head) = audio.playhead() else {
            return false;
        };
        if head.token != token {
            // Older audio still drains after a load; only a splice into the
            // preloaded track counts.
            let Some((next_token, next_item, path)) =
                self.preloaded.take_if(|(t, _, _)| *t == head.token)
            else {
                return false;
            };
            self.loaded = Some((next_token, next_item));
            if player.moved_to(next_item, Playable::File(path), now) {
                player.sync_position(head.position, now);
            }
            return true;
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

    pub fn handle(&mut self, event: AudioEvent, player: &mut Player, now: Instant) {
        match event {
            AudioEvent::Failed { item, reason } => {
                if self.loaded.is_some_and(|(_, i)| i == item) {
                    self.loaded = None;
                }
                if self.preloaded.as_ref().is_some_and(|(_, i, _)| *i == item) {
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
