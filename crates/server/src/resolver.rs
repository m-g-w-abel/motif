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

impl Resolver for LibraryResolver {
    fn resolve(&self, track: &TrackRef) -> Result<Playable, String> {
        match track.source.as_str() {
            // shortcut: the UI's demo library goes in phase 1 milestone 6.
            "demo" => return Ok(Playable::Simulated),
            "local" => {
                let path = PathBuf::from(&track.id);
                if path.is_file() {
                    return Ok(Playable::File(path));
                }
            }
            _ => {}
        }
        if let Some(path) = self
            .library
            .as_ref()
            .and_then(|library| library.find(track))
        {
            return Ok(Playable::File(path));
        }
        Err(match track.source.as_str() {
            "local" => format!("not on this device: {}", track.id),
            other => format!("no {other} source on this device"),
        })
    }
}
