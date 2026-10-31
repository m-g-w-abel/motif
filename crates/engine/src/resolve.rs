use std::path::PathBuf;

use protocol::{ReplayGain, TrackRef};

/// Something this device can play.
#[derive(Debug, Clone, PartialEq)]
pub enum Playable {
    /// A file and its ReplayGain values.
    File(PathBuf, ReplayGain),
    /// The UI's demo library, which has no audio; playback is simulated.
    // shortcut: goes with the demo library in phase 1 milestone 6.
    Simulated,
}

/// Finds what on this device can play a [`TrackRef`]. The engine knows nothing
/// about libraries or sources; the app gives it a resolver that does.
pub trait Resolver: Send + 'static {
    /// The playable form of `track`, or why there is none (shown to the user).
    fn resolve(&self, track: &TrackRef) -> Result<Playable, String>;
}

/// Plays local files by path, until the library resolver (milestone 5)
/// matches references from other devices by identity keys.
pub struct LocalFiles;

impl Resolver for LocalFiles {
    fn resolve(&self, track: &TrackRef) -> Result<Playable, String> {
        match track.source.as_str() {
            "local" => {
                let path = PathBuf::from(&track.id);
                if path.is_file() {
                    Ok(Playable::File(path, ReplayGain::default()))
                } else {
                    Err(format!("file not found: {}", track.id))
                }
            }
            "demo" => Ok(Playable::Simulated),
            other => Err(format!("no {other} source on this device")),
        }
    }
}
