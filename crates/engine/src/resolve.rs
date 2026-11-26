use std::{
    io::{self, Read, Seek},
    path::PathBuf,
    sync::Arc,
};

use protocol::{ReplayGain, Streamed, TrackRef};
use tokio::sync::watch;

use crate::audio::decoder::Decoder;

/// Something this device can play.
#[derive(Debug, Clone, PartialEq)]
pub enum Playable {
    /// Audio and its ReplayGain values.
    File(Source, ReplayGain),
    /// The UI's demo library, which has no audio; playback is simulated.
    // shortcut: goes with the demo library in phase 1 milestone 6.
    Simulated,
}

/// Where a track's bytes are.
#[derive(Clone)]
pub enum Source {
    File(PathBuf),
    /// Audio from elsewhere (a linked engine): bytes read as from a file, or
    /// audio decoded already. `name` tells streams apart; `open` starts it at
    /// the beginning and is called on the decode thread, so it may block on
    /// the network. `streamed` says how it comes once opened, or that its
    /// device couldn't be reached.
    Stream {
        name: String,
        open: Opener,
        streamed: watch::Receiver<Option<StreamState>>,
    },
}

/// What a [`Source::Stream`] says of itself.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamState {
    Streams(Streamed),
    /// Named: the device holding it.
    Unreachable(String),
}

/// Opens a [`Source::Stream`].
pub type Opener = Arc<dyn Fn() -> io::Result<Opened> + Send + Sync>;

/// A [`Source::Stream`], opened.
pub enum Opened {
    /// For the decoders to read.
    Bytes(Box<dyn ByteSource>),
    /// Decoded at its end (encoded there as it is read).
    Decoded(Box<dyn Decoder>),
}

/// A seekable reader of a stream's bytes.
pub trait ByteSource: Read + Seek + Send + Sync {
    fn byte_len(&self) -> u64;
}

impl Source {
    pub fn name(&self) -> String {
        match self {
            Source::File(path) => path.display().to_string(),
            Source::Stream { name, .. } => name.clone(),
        }
    }
}

impl From<PathBuf> for Source {
    fn from(path: PathBuf) -> Self {
        Source::File(path)
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::File(path) => f.debug_tuple("File").field(path).finish(),
            Source::Stream { name, .. } => f.debug_tuple("Stream").field(name).finish(),
        }
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Source::File(a), Source::File(b)) => a == b,
            (Source::Stream { name: a, .. }, Source::Stream { name: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// Finds what on this device can play a [`TrackRef`]. The engine knows nothing
/// about libraries or sources; the app gives it a resolver that does.
pub trait Resolver: Send + 'static {
    /// The playable form of `track`, or why there is none (shown to the user).
    fn resolve(&self, track: &TrackRef) -> Result<Playable, String>;

    /// Measures loudness for ReplayGain where `tracks` have no gain tags or
    /// measurement yet, in order in the background (see [`Self::measured`]).
    fn measure(&self, _tracks: &[TrackRef]) {}

    /// Whether measurements finished since the last call, changing what
    /// [`Self::resolve`] returns.
    fn measured(&self) -> bool {
        false
    }
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
                    Ok(Playable::File(path.into(), ReplayGain::default()))
                } else {
                    Err(format!("file not found: {}", track.id))
                }
            }
            "demo" => Ok(Playable::Simulated),
            other => Err(format!("no {other} source on this device")),
        }
    }
}
