//! Android's audio focus and "becoming noisy" (#120), as transport commands.
//! A call pauses and resumes after; another player taking focus, or
//! headphones disconnecting, pauses for good. A navigation prompt ducks the
//! engine's output, which Android doesn't do for AAudio streams; the user's
//! volume (the media volume, [`crate::volume`]) never moves.

use protocol::Command;

/// What `MediaPlugin.kt` reports.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Gain,
    Loss,
    LossTransient,
    LossTransientCanDuck,
    Noisy,
}

#[derive(Debug, Default)]
pub struct Focus {
    /// Paused by a transient loss, so focus coming back plays again.
    resume: bool,
    /// Another app is talking over us until focus comes back.
    ducked: bool,
}

impl Focus {
    /// The command for `change`, given whether the engine is `playing`.
    pub fn on(&mut self, change: Change, playing: bool) -> Option<Command> {
        self.ducked = change == Change::LossTransientCanDuck;
        match change {
            Change::LossTransientCanDuck => None,
            Change::LossTransient => {
                self.resume |= playing;
                playing.then_some(Command::Pause)
            }
            Change::Loss | Change::Noisy => {
                self.resume = false;
                playing.then_some(Command::Pause)
            }
            Change::Gain => std::mem::take(&mut self.resume).then_some(Command::Play),
        }
    }

    /// Whether the engine's output should be ducked now.
    pub fn ducked(&self) -> bool {
        self.ducked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Change::*;

    #[test]
    fn a_prompt_ducks_until_focus_comes_back() {
        let mut focus = Focus::default();
        assert_eq!(focus.on(LossTransientCanDuck, true), None);
        assert!(focus.ducked());
        assert_eq!(focus.on(Gain, true), None);
        assert!(!focus.ducked());
    }

    #[test]
    fn losing_focus_while_ducked_unducks() {
        // No gain may follow: Android doesn't report one when we take focus back.
        for change in [Loss, LossTransient, Noisy] {
            let mut focus = Focus::default();
            focus.on(LossTransientCanDuck, true);
            focus.on(change, true);
            assert!(!focus.ducked(), "{change:?}");
        }
    }

    #[test]
    fn a_call_pauses_then_resumes() {
        let mut focus = Focus::default();
        assert_eq!(focus.on(LossTransient, true), Some(Command::Pause));
        assert_eq!(focus.on(Gain, false), Some(Command::Play));
        // Only once.
        assert_eq!(focus.on(Gain, true), None);
    }

    #[test]
    fn a_call_while_paused_doesnt_start_playback() {
        let mut focus = Focus::default();
        assert_eq!(focus.on(LossTransient, false), None);
        assert_eq!(focus.on(Gain, false), None);
    }

    #[test]
    fn another_player_pauses_for_good() {
        let mut focus = Focus::default();
        assert_eq!(focus.on(Loss, true), Some(Command::Pause));
        assert_eq!(focus.on(Gain, false), None);
    }

    #[test]
    fn permanent_loss_after_a_call_cancels_the_resume() {
        let mut focus = Focus::default();
        focus.on(LossTransient, true);
        assert_eq!(focus.on(Loss, false), None);
        assert_eq!(focus.on(Gain, false), None);
    }

    #[test]
    fn unplugging_pauses_and_cancels_the_resume() {
        let mut focus = Focus::default();
        assert_eq!(focus.on(Noisy, true), Some(Command::Pause));
        focus.on(LossTransient, true);
        focus.on(Noisy, false);
        assert_eq!(focus.on(Gain, false), None);
    }

    #[test]
    fn a_second_transient_loss_keeps_the_resume() {
        let mut focus = Focus::default();
        focus.on(LossTransient, true);
        focus.on(LossTransient, false);
        assert_eq!(focus.on(Gain, false), Some(Command::Play));
    }
}
