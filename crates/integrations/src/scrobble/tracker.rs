//! Decides from engine states when a track starts and when it has been
//! listened to long enough to scrobble. Pure, so the rule is unit-tested.

use protocol::{PlaybackStatus, PlayerState};

use super::Listen;

/// Shorter tracks are never scrobbled.
const MIN_TRACK_MS: u64 = 30_000;
/// Listening this long scrobbles even a long track.
const MAX_WAIT_MS: u64 = 4 * 60_000;
/// Jumping back to within this of the start (repeat-one, a seek to the
/// start, Previous) begins a new listen.
const RESTART_MS: u64 = 2_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    NowPlaying(Listen),
    Scrobble(Listen),
}

/// Listened time a track of `duration_ms` needs before it scrobbles.
fn threshold(duration_ms: Option<u64>) -> Option<u64> {
    duration_ms
        .filter(|&d| d >= MIN_TRACK_MS)
        .map(|d| (d / 2).min(MAX_WAIT_MS))
}

#[derive(Default)]
pub struct Tracker {
    play: Option<Play>,
}

/// One listen of the current entry.
struct Play {
    item: u64,
    /// `None` when the track lacks the artist or title services need.
    listen: Option<Listen>,
    threshold: Option<u64>,
    /// Time spent playing, not counting pauses or what seeks skipped.
    listened_ms: u64,
    /// Engine clock and position when last updated.
    at_ms: u64,
    position_ms: u64,
    playing: bool,
    announced: bool,
    scrobbled: bool,
}

impl Tracker {
    /// Follows a new engine state.
    pub fn update(&mut self, state: &PlayerState) -> Vec<Action> {
        let mut actions: Vec<Action> = self.advance(state.sampled_at_ms).into_iter().collect();
        let current = state.current.as_ref();
        let same = match (&self.play, current) {
            (Some(play), Some(item)) => {
                play.item == item.id
                    && !(state.position_ms < RESTART_MS
                        && play.position_ms >= state.position_ms + RESTART_MS)
            }
            _ => false,
        };
        if !same {
            self.play = current.map(|item| Play {
                item: item.id,
                listen: Listen::of(&item.track),
                threshold: threshold(item.track.duration_ms),
                listened_ms: 0,
                at_ms: state.sampled_at_ms,
                position_ms: state.position_ms,
                playing: false,
                announced: false,
                scrobbled: false,
            });
        }
        let Some(play) = &mut self.play else {
            return actions;
        };
        play.at_ms = state.sampled_at_ms;
        play.position_ms = state.position_ms;
        play.playing = state.status == PlaybackStatus::Playing;
        if play.playing && !play.announced {
            play.announced = true;
            if let Some(listen) = &mut play.listen {
                // When the track would have started, had it played straight through.
                listen.listened_at = state.sampled_at_ms.saturating_sub(state.position_ms) / 1000;
                actions.push(Action::NowPlaying(listen.clone()));
            }
        }
        actions
    }

    /// Counts listening up to `now_ms` (the engine clock); returns the
    /// scrobble once the current listen reaches its threshold.
    pub fn advance(&mut self, now_ms: u64) -> Option<Action> {
        let play = self.play.as_mut()?;
        if play.playing {
            let elapsed = now_ms.saturating_sub(play.at_ms);
            play.listened_ms += elapsed;
            play.position_ms += elapsed;
        }
        play.at_ms = play.at_ms.max(now_ms);
        if play.scrobbled || play.threshold.is_none_or(|t| play.listened_ms < t) {
            return None;
        }
        play.scrobbled = true;
        play.listen.clone().map(Action::Scrobble)
    }

    /// How long until the current listen scrobbles, if it is playing.
    pub fn due_in_ms(&self) -> Option<u64> {
        let play = self.play.as_ref()?;
        if !play.playing || play.scrobbled || play.listen.is_none() {
            return None;
        }
        Some(play.threshold?.saturating_sub(play.listened_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{QueueItem, TrackRef};

    const MIN: u64 = 60_000;

    struct Sim {
        tracker: Tracker,
        state: PlayerState,
        actions: Vec<Action>,
    }

    impl Sim {
        fn new() -> Self {
            Self {
                tracker: Tracker::default(),
                state: PlayerState {
                    sampled_at_ms: 1_700_000_000_000,
                    ..Default::default()
                },
                actions: Vec::new(),
            }
        }

        fn send(&mut self) {
            self.actions.extend(self.tracker.update(&self.state));
        }

        /// Starts entry `id`, `duration_ms` long, playing from the start.
        fn play(&mut self, id: u64, duration_ms: u64) {
            self.state.current = Some(QueueItem {
                id,
                track: TrackRef {
                    title: Some(format!("Song {id}")),
                    artist: Some("Band".into()),
                    duration_ms: Some(duration_ms),
                    ..Default::default()
                },
                error: None,
            });
            self.state.position_ms = 0;
            self.state.status = PlaybackStatus::Playing;
            self.send();
        }

        /// Lets `ms` pass, firing the timer the way the integration does.
        fn wait(&mut self, ms: u64) {
            let end = self.state.sampled_at_ms + ms;
            let mut now = self.state.sampled_at_ms;
            while let Some(due) = self.tracker.due_in_ms().filter(|d| now + d <= end) {
                now += due;
                self.actions.extend(self.tracker.advance(now));
            }
            self.actions.extend(self.tracker.advance(end));
            if self.state.status == PlaybackStatus::Playing {
                self.state.position_ms += ms;
            }
            self.state.sampled_at_ms = end;
        }

        fn set(&mut self, status: PlaybackStatus) {
            self.state.status = status;
            self.send();
        }

        fn seek(&mut self, position_ms: u64) {
            self.state.position_ms = position_ms;
            self.send();
        }

        fn scrobbles(&self) -> usize {
            self.actions
                .iter()
                .filter(|a| matches!(a, Action::Scrobble(_)))
                .count()
        }
    }

    #[test]
    fn scrobbles_at_half_or_four_minutes() {
        let mut sim = Sim::new();
        sim.play(1, 3 * MIN);
        assert!(matches!(sim.actions[0], Action::NowPlaying(_)));
        sim.wait(90_000 - 1);
        assert_eq!(sim.scrobbles(), 0);
        sim.wait(1);
        assert_eq!(sim.scrobbles(), 1, "half of three minutes");
        sim.wait(MIN);
        assert_eq!(sim.scrobbles(), 1, "only once");

        let mut sim = Sim::new();
        sim.play(1, 20 * MIN);
        sim.wait(4 * MIN);
        assert_eq!(sim.scrobbles(), 1, "four minutes of a long track");
    }

    #[test]
    fn short_tracks_never_scrobble() {
        let mut sim = Sim::new();
        sim.play(1, 29_999);
        sim.wait(29_999);
        assert_eq!(sim.scrobbles(), 0);
        assert_eq!(sim.actions.len(), 1, "but they are now playing");
    }

    #[test]
    fn pauses_dont_count() {
        let mut sim = Sim::new();
        sim.play(1, 4 * MIN);
        sim.wait(MIN);
        sim.set(PlaybackStatus::Paused);
        sim.wait(10 * MIN);
        assert_eq!(sim.scrobbles(), 0);
        sim.set(PlaybackStatus::Playing);
        sim.wait(MIN);
        assert_eq!(sim.scrobbles(), 1);
        assert_eq!(sim.actions.len(), 2, "resuming isn't a new now playing");
    }

    #[test]
    fn seeks_dont_count() {
        let mut sim = Sim::new();
        sim.play(1, 5 * MIN);
        sim.wait(10_000);
        sim.seek(4 * MIN);
        sim.wait(MIN);
        assert_eq!(sim.scrobbles(), 0, "skipping to the end isn't listening");
        sim.seek(10_000); // back, but not to the start
        sim.wait(80_000);
        assert_eq!(sim.scrobbles(), 1, "10 s + 60 s + 80 s = half");
    }

    #[test]
    fn repeat_one_counts_again() {
        let mut sim = Sim::new();
        sim.play(1, MIN);
        sim.wait(MIN);
        sim.seek(0); // the engine restarts the same entry
        sim.wait(MIN);
        assert_eq!(sim.scrobbles(), 2);
        let now_playing = sim
            .actions
            .iter()
            .filter(|a| matches!(a, Action::NowPlaying(_)))
            .count();
        assert_eq!(now_playing, 2);
    }

    #[test]
    fn a_track_change_finishes_the_last_listen_first() {
        let mut sim = Sim::new();
        sim.play(1, 2 * MIN);
        sim.state.sampled_at_ms += MIN; // no timer fired in between
        sim.play(2, 2 * MIN);
        let Action::Scrobble(first) = &sim.actions[1] else {
            panic!("{:?}", sim.actions)
        };
        assert_eq!(first.title, "Song 1");
        assert_eq!(first.listened_at, 1_700_000_000);
        assert!(matches!(&sim.actions[2], Action::NowPlaying(l) if l.title == "Song 2"));
    }

    #[test]
    fn untitled_tracks_are_skipped() {
        let mut sim = Sim::new();
        sim.play(1, 2 * MIN);
        sim.actions.clear();
        sim.state.current.as_mut().unwrap().track.title = None;
        sim.state.current.as_mut().unwrap().id = 2;
        sim.send();
        sim.wait(2 * MIN);
        assert!(sim.actions.is_empty());
    }
}
