//! The phone engine's volume is Android's media volume (#138): engine
//! volumes snap to the system's steps, and system changes (keys, the system
//! panel, other apps) become the engine's volume. Only acting when the step
//! differs keeps the two from chasing each other.

/// Android's media volume: step `index` of `max`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct System {
    pub index: i32,
    pub max: i32,
}

impl System {
    fn step(self, volume: f32) -> i32 {
        (volume * self.max as f32).round() as i32
    }

    /// The step to set for engine `volume`, unless the system is on it already.
    pub fn to_set(self, volume: f32) -> Option<i32> {
        let step = self.step(volume);
        (step != self.index).then_some(step)
    }

    /// The volume the engine should take on, unless `volume` already snaps to this step.
    pub fn to_follow(self, volume: f32) -> Option<f32> {
        (self.step(volume) != self.index).then(|| self.index as f32 / self.max as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT_8: System = System { index: 8, max: 15 };

    #[test]
    fn engine_volume_snaps_to_the_nearest_step() {
        assert_eq!(AT_8.to_set(0.0), Some(0));
        assert_eq!(AT_8.to_set(0.6), Some(9));
        assert_eq!(AT_8.to_set(1.0), Some(15));
    }

    #[test]
    fn no_set_when_already_on_that_step() {
        assert_eq!(AT_8.to_set(0.52), None);
        assert_eq!(AT_8.to_set(8.0 / 15.0), None);
    }

    #[test]
    fn engine_follows_a_new_system_step() {
        assert_eq!(AT_8.to_follow(0.0), Some(8.0 / 15.0));
        assert_eq!(System { index: 0, max: 15 }.to_follow(0.5), Some(0.0));
        assert_eq!(System { index: 15, max: 15 }.to_follow(0.0), Some(1.0));
    }

    #[test]
    fn engine_keeps_its_volume_within_the_step() {
        // A drag at 0.52 isn't pulled back to 0.533 by its own echo.
        assert_eq!(AT_8.to_follow(0.52), None);
    }

    #[test]
    fn following_then_setting_settles() {
        let followed = AT_8.to_follow(0.1).unwrap();
        assert_eq!(AT_8.to_set(followed), None);
    }
}
