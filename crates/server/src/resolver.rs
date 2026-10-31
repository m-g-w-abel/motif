use std::path::PathBuf;

use engine::{Playable, Resolver};
use library::Library;
use protocol::TrackRef;

/// Resolves queue entries on this device: the referenced local file if it is
/// here, else a library track with matching identity keys (a reference queued
/// on another device).
pub struct LibraryResolver {
    pub library: Option<Library>,
}

impl LibraryResolver {
    /// The file on this device that plays `track`, if any.
    fn path(&self, track: &TrackRef) -> Option<PathBuf> {
        let path = PathBuf::from(&track.id);
        if track.source == "local" && path.is_file() {
            return Some(path);
        }
        self.library.as_ref()?.find(track)
    }
}

impl Resolver for LibraryResolver {
    fn resolve(&self, track: &TrackRef) -> Result<Playable, String> {
        // shortcut: the UI's demo library goes in phase 1 milestone 6.
        if track.source == "demo" {
            return Ok(Playable::Simulated);
        }
        if let Some(path) = self.path(track) {
            let gain = self
                .library
                .as_ref()
                .map(|library| library.replay_gain(&path))
                .unwrap_or_default();
            return Ok(Playable::File(path, gain));
        }
        Err(match track.source.as_str() {
            "local" => format!("not on this device: {}", track.id),
            other => format!("no {other} source on this device"),
        })
    }

    fn measure(&self, tracks: &[TrackRef], wait: bool) {
        if let Some(library) = &self.library {
            library.measure(tracks.iter().filter_map(|t| self.path(t)).collect(), wait);
        }
    }

    fn measured(&self) -> bool {
        self.library.as_ref().is_some_and(Library::take_measured)
    }
}
