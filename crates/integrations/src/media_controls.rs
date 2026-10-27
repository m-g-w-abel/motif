//! OS media controls: MPRIS on Linux, the System Media Transport Controls on
//! Windows, Now Playing on macOS. Media keys, the lock screen and desktop
//! widgets show what plays and control the engine.

use std::{ffi::c_void, sync::mpsc, time::Duration};

use engine::Engine;
use library::Library;
use protocol::{Command, LibraryQuery, PlaybackStatus, QueryResult, TrackRef};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};

/// How far the seek buttons jump.
const SEEK_STEP: Duration = Duration::from_secs(10);

enum Update {
    Metadata {
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        duration: Option<Duration>,
        cover: Option<String>,
    },
    Playback(MediaPlayback),
}

/// Starts mirroring `engine` in the OS media controls, on a thread of its
/// own. `hwnd` is the app window, required on Windows.
// shortcut: only exercised on Linux (MPRIS); Windows and macOS compile in CI but are untested, check them when those builds get real use.
pub fn start(engine: Engine, library: Option<Library>, hwnd: Option<*mut c_void>) {
    let (updates, rx) = mpsc::channel();
    let runtime = tokio::runtime::Handle::current();
    // Raw pointers aren't Send; the window handle only travels to the thread.
    let hwnd = hwnd.map(|h| h as usize);
    let commands = engine.clone();
    let spawned = std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || {
            controls_thread(rx, hwnd, move |command| {
                let engine = commands.clone();
                runtime.spawn(async move {
                    if let Err(err) = engine.send(command).await {
                        tracing::debug!(%err, "media control command rejected");
                    }
                });
            })
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "can't start media controls");
        return;
    }
    tokio::spawn(follow(engine, library, updates));
}

fn controls_thread(
    updates: mpsc::Receiver<Update>,
    hwnd: Option<usize>,
    send: impl Fn(Command) + Send + 'static,
) {
    let config = PlatformConfig {
        display_name: "Motif",
        dbus_name: "motif",
        hwnd: hwnd.map(|h| h as *mut c_void),
    };
    let mut controls = match MediaControls::new(config) {
        Ok(controls) => controls,
        Err(err) => {
            tracing::warn!(?err, "no OS media controls");
            return;
        }
    };
    // Remember the position so relative seeks know where they start from.
    let position = std::sync::Arc::new(std::sync::Mutex::new(Duration::ZERO));
    let seek_from = position.clone();
    let attached = controls.attach(move |event| {
        let from = *seek_from.lock().unwrap_or_else(|e| e.into_inner());
        let seek = |forward: bool, by: Duration| Command::Seek {
            position_ms: if forward {
                from + by
            } else {
                from.saturating_sub(by)
            }
            .as_millis() as u64,
        };
        let command = match event {
            MediaControlEvent::Play => Command::Play,
            MediaControlEvent::Pause => Command::Pause,
            MediaControlEvent::Toggle => Command::TogglePlayback,
            MediaControlEvent::Next => Command::Next,
            MediaControlEvent::Previous => Command::Previous,
            MediaControlEvent::Stop => Command::Stop,
            MediaControlEvent::Seek(direction) => {
                seek(direction == SeekDirection::Forward, SEEK_STEP)
            }
            MediaControlEvent::SeekBy(direction, by) => {
                seek(direction == SeekDirection::Forward, by)
            }
            MediaControlEvent::SetPosition(MediaPosition(at)) => Command::Seek {
                position_ms: at.as_millis() as u64,
            },
            MediaControlEvent::SetVolume(volume) => Command::SetVolume {
                volume: volume.clamp(0.0, 1.0) as f32,
            },
            _ => return,
        };
        send(command);
    });
    if let Err(err) = attached {
        tracing::warn!(?err, "can't receive media control events");
    }
    for update in updates {
        let result = match update {
            Update::Metadata {
                title,
                artist,
                album,
                duration,
                cover,
            } => controls.set_metadata(MediaMetadata {
                title: title.as_deref(),
                artist: artist.as_deref(),
                album: album.as_deref(),
                duration,
                cover_url: cover.as_deref(),
            }),
            Update::Playback(playback) => {
                if let MediaPlayback::Playing {
                    progress: Some(MediaPosition(at)),
                }
                | MediaPlayback::Paused {
                    progress: Some(MediaPosition(at)),
                } = &playback
                {
                    *position.lock().unwrap_or_else(|e| e.into_inner()) = *at;
                }
                controls.set_playback(playback)
            }
        };
        if let Err(err) = result {
            tracing::debug!(?err, "media controls update failed");
        }
    }
}

/// Sends the controls thread what to show whenever the engine changes.
async fn follow(engine: Engine, library: Option<Library>, updates: mpsc::Sender<Update>) {
    let mut states = engine.subscribe();
    let mut shown: Option<u64> = None;
    loop {
        let state = states.borrow_and_update().clone();
        let current = state.current.clone();
        if current.as_ref().map(|item| item.id) != shown {
            shown = current.as_ref().map(|item| item.id);
            let track = current.map(|item| item.track).unwrap_or_default();
            let cover = match &library {
                Some(library) => cover_url(library.clone(), &track).await,
                None => None,
            };
            let update = Update::Metadata {
                title: track.title,
                artist: track.artist,
                album: track.album,
                duration: track.duration_ms.map(Duration::from_millis),
                cover,
            };
            if updates.send(update).is_err() {
                return;
            }
        }
        // Where playback is now, counting time since the snapshot was taken.
        let elapsed = engine::wall_clock_ms().saturating_sub(state.sampled_at_ms);
        let at = |playing: bool| {
            let ms = state.position_ms + if playing { elapsed } else { 0 };
            Some(MediaPosition(Duration::from_millis(ms)))
        };
        let playback = match state.status {
            PlaybackStatus::Playing => MediaPlayback::Playing { progress: at(true) },
            PlaybackStatus::Paused => MediaPlayback::Paused {
                progress: at(false),
            },
            PlaybackStatus::Stopped => MediaPlayback::Stopped,
        };
        if updates.send(Update::Playback(playback)).is_err() || states.changed().await.is_err() {
            return;
        }
    }
}

/// A `file://` URL of the track's artwork in the library's cache.
async fn cover_url(library: Library, track: &TrackRef) -> Option<String> {
    let path = track.id.clone();
    tokio::task::spawn_blocking(move || {
        let QueryResult::Tracks { tracks, .. } = library
            .query(LibraryQuery::Lookup { paths: vec![path] })
            .ok()?
        else {
            return None;
        };
        let art = tracks.into_iter().next()?.art?;
        let file = library.art(&art, Some(512)).ok()?;
        Some(file_url(&file.to_string_lossy()))
    })
    .await
    .ok()
    .flatten()
}

/// Percent-encodes a path into a `file://` URL.
fn file_url(path: &str) -> String {
    let mut url = String::from("file://");
    if !path.starts_with('/') {
        url.push('/'); // Windows drive paths
    }
    for byte in path.replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_urls_are_escaped() {
        assert_eq!(
            super::file_url("/home/a b/ü.jpg"),
            "file:///home/a%20b/%C3%BC.jpg"
        );
        assert_eq!(super::file_url("C:\\Art\\x.jpg"), "file:///C:/Art/x.jpg");
    }
}
