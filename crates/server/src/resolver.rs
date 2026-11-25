use std::path::{Path, PathBuf};

use engine::{Playable, Resolver};
use library::Library;
use protocol::TrackRef;

use crate::{Pairing, files};

/// Resolves queue entries on this device: the referenced local file if it is
/// here, else a library track with matching identity keys (a reference queued
/// on another device), else a stream of a linked engine's copy.
pub struct LibraryResolver {
    pub library: Option<Library>,
    /// The linked engines to stream from; `None` plays only what is here.
    pub pairing: Option<Pairing>,
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

    /// A linked engine's copy of `track` to stream. Whether one is reachable
    /// is found out when it is opened, so resolving never waits on the
    /// network and a queue restored before the links are up keeps it.
    fn linked(&self, track: &TrackRef) -> Option<Playable> {
        let (library, pairing) = (self.library.as_ref()?, self.pairing.as_ref()?);
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let copies: Vec<_> = library
            .copies(track)
            .into_iter()
            .filter(|(key, _)| pairing.is_linked(key))
            .collect();
        let (key, id) = copies.first()?;
        let gain = library.replay_gain(Path::new(&format!("remote:{key}:{id}")));
        let source = files::source(pairing.clone(), copies, runtime);
        Some(Playable::File(source, gain))
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
            return Ok(Playable::File(path.into(), gain));
        }
        // A downloaded copy comes next, once there are any.
        if let Some(linked) = self.linked(track) {
            return Ok(linked);
        }
        Err(match track.source.as_str() {
            "local" => format!("not on this device: {}", track.id),
            other => format!("no {other} source on this device"),
        })
    }

    fn measure(&self, tracks: &[TrackRef]) {
        if let Some(library) = &self.library {
            library.measure(tracks.iter().filter_map(|t| self.path(t)).collect());
        }
    }

    fn measured(&self) -> bool {
        self.library.as_ref().is_some_and(Library::take_measured)
    }
}
