use std::time::{Duration, Instant};

use protocol::{Command, PlaybackStatus, PlayerState, QueueItem, RepeatMode, TrackInfo};

use crate::EngineError;

/// "Previous" restarts the current track instead when past this point.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);

/// Queue and transport state, with no I/O. Time is passed in so behaviour is
/// deterministic under test.
#[derive(Debug)]
pub struct Player {
    queue: Vec<QueueItem>,
    next_id: u64,
    current: Option<usize>,
    status: PlaybackStatus,
    volume: f32,
    repeat: RepeatMode,
    /// Position when playback last started or was moved.
    base: Duration,
    /// Set while playing: the instant `base` was recorded.
    since: Option<Instant>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            queue: Vec::new(),
            next_id: 1,
            current: None,
            status: PlaybackStatus::Stopped,
            volume: 1.0,
            repeat: RepeatMode::Off,
            base: Duration::ZERO,
            since: None,
        }
    }
}

impl Player {
    pub fn apply(&mut self, command: Command, now: Instant) -> Result<(), EngineError> {
        match command {
            Command::Play => self.play(now),
            Command::Pause => self.pause(now),
            Command::TogglePlayback => match self.status {
                PlaybackStatus::Playing => self.pause(now),
                _ => self.play(now),
            },
            Command::Stop => self.stop(),
            Command::Next => self.advance(now, false),
            Command::Previous => {
                if self.position(now) > RESTART_THRESHOLD || self.current == Some(0) {
                    self.seek(Duration::ZERO, now);
                } else if let Some(i) = self.current {
                    self.jump(i - 1, now);
                }
            }
            Command::Seek { position_ms } => self.seek(Duration::from_millis(position_ms), now),
            Command::SetVolume { volume } => {
                if !(0.0..=1.0).contains(&volume) {
                    return Err(EngineError::InvalidVolume(volume));
                }
                self.volume = volume;
            }
            Command::SetRepeat { mode } => self.repeat = mode,
            Command::Enqueue { tracks } => self.push(tracks),
            Command::SetQueue { tracks, start } => {
                if let Some(start) = start
                    && start as usize >= tracks.len()
                {
                    return Err(EngineError::IndexOutOfRange(start));
                }
                self.queue.clear();
                self.current = None;
                self.stop();
                self.push(tracks);
                if let Some(start) = start {
                    self.jump(start as usize, now);
                    self.play(now);
                }
            }
            Command::PlayItem { item_id } => {
                let index = self.index_of(item_id)?;
                self.jump(index, now);
                self.play(now);
            }
            Command::RemoveItem { item_id } => self.remove(self.index_of(item_id)?, now),
            Command::ClearQueue => {
                self.queue.clear();
                self.current = None;
                self.stop();
            }
        }
        Ok(())
    }

    /// Advances past the end of the current track. Returns whether state
    /// changed. Phase 1 replaces this with end-of-stream events from the
    /// decoder.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.status != PlaybackStatus::Playing {
            return false;
        }
        match self.current_duration() {
            Some(duration) if self.position(now) >= duration => {
                self.advance(now, true);
                true
            }
            _ => false,
        }
    }

    pub fn snapshot(&self, now: Instant, wall_ms: u64) -> PlayerState {
        PlayerState {
            status: self.status,
            volume: self.volume,
            repeat: self.repeat,
            position_ms: self.position(now).as_millis() as u64,
            sampled_at_ms: wall_ms,
            current: self.current.map(|i| i as u32),
        }
    }

    pub fn queue(&self) -> &[QueueItem] {
        &self.queue
    }

    fn position(&self, now: Instant) -> Duration {
        let position = match self.since {
            Some(since) => self.base + now.saturating_duration_since(since),
            None => self.base,
        };
        match self.current_duration() {
            Some(duration) => position.min(duration),
            None => position,
        }
    }

    fn current_duration(&self) -> Option<Duration> {
        let item = self.queue.get(self.current?)?;
        item.track.duration_ms.map(Duration::from_millis)
    }

    fn push(&mut self, tracks: Vec<TrackInfo>) {
        for track in tracks {
            let id = self.next_id;
            self.next_id += 1;
            self.queue.push(QueueItem { id, track });
        }
    }

    fn index_of(&self, item_id: u64) -> Result<usize, EngineError> {
        self.queue
            .iter()
            .position(|item| item.id == item_id)
            .ok_or(EngineError::UnknownItem(item_id))
    }

    fn play(&mut self, now: Instant) {
        if self.current.is_none() && !self.queue.is_empty() {
            self.current = Some(0);
        }
        if self.current.is_some() && self.status != PlaybackStatus::Playing {
            self.status = PlaybackStatus::Playing;
            self.since = Some(now);
        }
    }

    fn pause(&mut self, now: Instant) {
        if self.status == PlaybackStatus::Playing {
            self.base = self.position(now);
            self.since = None;
            self.status = PlaybackStatus::Paused;
        }
    }

    fn stop(&mut self) {
        self.status = PlaybackStatus::Stopped;
        self.base = Duration::ZERO;
        self.since = None;
    }

    fn seek(&mut self, position: Duration, now: Instant) {
        self.base = position;
        if self.since.is_some() {
            self.since = Some(now);
        }
        self.base = self.position(now);
    }

    /// Moves to another queue entry, keeping the play/pause status.
    fn jump(&mut self, index: usize, now: Instant) {
        self.current = Some(index);
        self.base = Duration::ZERO;
        if self.since.is_some() {
            self.since = Some(now);
        }
    }

    /// Moves to the next entry. `natural` is true when the current track ended
    /// on its own, which is the only case where repeat-one applies.
    fn advance(&mut self, now: Instant, natural: bool) {
        let Some(i) = self.current else { return };
        if natural && self.repeat == RepeatMode::One {
            self.jump(i, now);
        } else if i + 1 < self.queue.len() {
            self.jump(i + 1, now);
        } else if self.repeat != RepeatMode::Off {
            self.jump(0, now);
        } else {
            self.stop();
        }
    }

    fn remove(&mut self, index: usize, now: Instant) {
        self.queue.remove(index);
        let Some(current) = self.current else { return };
        if index < current {
            self.current = Some(current - 1);
        } else if index == current {
            if self.queue.is_empty() {
                self.current = None;
                self.stop();
            } else {
                // The following entry slides into place; at the end, step back.
                self.jump(current.min(self.queue.len() - 1), now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str, duration_ms: u64) -> TrackInfo {
        TrackInfo {
            uri: format!("file:///{name}.flac"),
            title: Some(name.into()),
            duration_ms: Some(duration_ms),
            ..Default::default()
        }
    }

    fn player_with(tracks: &[(&str, u64)], t0: Instant) -> Player {
        let mut player = Player::default();
        let tracks = tracks.iter().map(|(n, d)| track(n, *d)).collect();
        player.apply(Command::Enqueue { tracks }, t0).unwrap();
        player
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn position_advances_only_while_playing() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 10_000)], t0);
        p.apply(Command::Play, t0).unwrap();
        assert_eq!(p.position(t0 + ms(1500)), ms(1500));

        p.apply(Command::Pause, t0 + ms(2000)).unwrap();
        assert_eq!(p.position(t0 + ms(9000)), ms(2000));

        p.apply(Command::Play, t0 + ms(9000)).unwrap();
        assert_eq!(p.position(t0 + ms(9500)), ms(2500));
    }

    #[test]
    fn seek_while_playing_and_paused() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 10_000)], t0);
        p.apply(Command::Play, t0).unwrap();
        p.apply(Command::Seek { position_ms: 5000 }, t0 + ms(100))
            .unwrap();
        assert_eq!(p.position(t0 + ms(600)), ms(5500));

        p.apply(Command::Pause, t0 + ms(600)).unwrap();
        p.apply(
            Command::Seek {
                position_ms: 99_000,
            },
            t0 + ms(700),
        )
        .unwrap();
        assert_eq!(p.position(t0 + ms(800)), ms(10_000), "clamped to duration");
    }

    #[test]
    fn tracks_advance_at_end_and_stop_after_last() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 1000), ("b", 1000)], t0);
        p.apply(Command::Play, t0).unwrap();

        assert!(p.tick(t0 + ms(1000)));
        assert_eq!(p.current, Some(1));
        assert_eq!(p.position(t0 + ms(1200)), ms(200));

        assert!(p.tick(t0 + ms(2000)));
        assert_eq!(p.status, PlaybackStatus::Stopped);
        assert!(!p.tick(t0 + ms(5000)));
    }

    #[test]
    fn repeat_modes() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 1000), ("b", 1000)], t0);
        p.apply(
            Command::SetRepeat {
                mode: RepeatMode::One,
            },
            t0,
        )
        .unwrap();
        p.apply(Command::Play, t0).unwrap();
        p.tick(t0 + ms(1000));
        assert_eq!(p.current, Some(0), "repeat-one replays on natural end");
        p.apply(Command::Next, t0 + ms(1100)).unwrap();
        assert_eq!(p.current, Some(1), "but Next still skips");

        p.apply(
            Command::SetRepeat {
                mode: RepeatMode::All,
            },
            t0,
        )
        .unwrap();
        p.apply(Command::Next, t0 + ms(1200)).unwrap();
        assert_eq!(p.current, Some(0), "repeat-all wraps");
        assert_eq!(p.status, PlaybackStatus::Playing);
    }

    #[test]
    fn previous_restarts_after_threshold() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 60_000), ("b", 60_000)], t0);
        p.apply(Command::Play, t0).unwrap();
        p.apply(Command::Next, t0).unwrap();

        p.apply(Command::Previous, t0 + ms(5000)).unwrap();
        assert_eq!(p.current, Some(1));
        assert_eq!(p.position(t0 + ms(5000)), Duration::ZERO);

        p.apply(Command::Previous, t0 + ms(6000)).unwrap();
        assert_eq!(p.current, Some(0));
    }

    #[test]
    fn removing_items_keeps_current_track() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 1000), ("b", 1000), ("c", 1000)], t0);
        let ids: Vec<u64> = p.queue.iter().map(|i| i.id).collect();
        p.apply(Command::PlayItem { item_id: ids[1] }, t0).unwrap();

        p.apply(Command::RemoveItem { item_id: ids[0] }, t0)
            .unwrap();
        assert_eq!(p.current, Some(0));
        assert_eq!(p.queue[0].id, ids[1]);

        p.apply(Command::RemoveItem { item_id: ids[1] }, t0)
            .unwrap();
        assert_eq!(p.queue[p.current.unwrap()].id, ids[2], "next slides in");

        p.apply(Command::RemoveItem { item_id: ids[2] }, t0)
            .unwrap();
        assert_eq!(p.current, None);
        assert_eq!(p.status, PlaybackStatus::Stopped);
    }

    #[test]
    fn set_queue_replaces_and_starts_playback() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 1000)], t0);
        p.apply(Command::Play, t0).unwrap();

        let tracks = vec![track("x", 1000), track("y", 1000), track("z", 1000)];
        p.apply(
            Command::SetQueue {
                tracks: tracks.clone(),
                start: Some(1),
            },
            t0 + ms(500),
        )
        .unwrap();
        assert_eq!(p.queue.len(), 3);
        assert_eq!(p.current, Some(1));
        assert_eq!(p.status, PlaybackStatus::Playing);
        assert_eq!(p.position(t0 + ms(700)), ms(200));

        assert_eq!(
            p.apply(
                Command::SetQueue {
                    tracks,
                    start: Some(3)
                },
                t0
            ),
            Err(EngineError::IndexOutOfRange(3))
        );
        assert_eq!(p.queue.len(), 3, "rejected command leaves the queue alone");
    }

    #[test]
    fn invalid_volume_is_rejected() {
        let mut p = Player::default();
        assert_eq!(
            p.apply(Command::SetVolume { volume: 1.5 }, Instant::now()),
            Err(EngineError::InvalidVolume(1.5))
        );
    }
}
