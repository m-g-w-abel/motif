//! Discord Rich Presence: "Listening to Motif" with what plays, through the
//! local Discord client's IPC socket (a named pipe on Windows). Opt-in; shown
//! only while playing, so pausing or stopping clears it.

use std::{
    path::Path,
    sync::mpsc::{self, RecvTimeoutError},
    time::Duration,
};

use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{Activity, ActivityType, Assets, Timestamps},
    error::Error,
};
use engine::Engine;
use library::Library;
use protocol::{IntegrationSettings, PlaybackStatus, PlayerState, TrackRef};
use tokio::sync::watch;

/// Motif's Discord application (public, not a secret).
const APP_ID: &str = "1565881998315098212";
/// Motif's icon, uploaded to the application as a Rich Presence art asset.
const FALLBACK_ART: &str = "motif";
/// Waits between tries to reach Discord, doubling up to the second.
const RETRY: (Duration, Duration) = (Duration::from_secs(5), Duration::from_secs(60));
/// How often the presence is sent again, which notices Discord restarting.
const REFRESH: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
struct Presence {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    image: String,
    /// Unix time in ms the track would have started, played straight through.
    start_ms: i64,
    end_ms: Option<i64>,
}

/// Starts showing what `engine` plays on Discord while `settings` allow it.
pub fn start(
    engine: Engine,
    library: Option<Library>,
    settings: watch::Receiver<IntegrationSettings>,
) {
    let (updates, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("discord".into())
        .spawn(move || presence_thread(rx));
    if let Err(err) = spawned {
        tracing::warn!(%err, "can't start Discord presence");
        return;
    }
    tokio::spawn(follow(engine, library, settings, updates));
}

/// Keeps Discord showing the latest presence: connects when there is one
/// (retrying with backoff while Discord isn't running) and disconnects,
/// which clears it, when there is none.
fn presence_thread(updates: mpsc::Receiver<Option<Presence>>) {
    let mut want = None;
    let mut client: Option<DiscordIpcClient> = None;
    let mut retry = RETRY.0;
    loop {
        if want.is_some() {
            let wait = if client.is_some() { REFRESH } else { retry };
            match updates.recv_timeout(wait) {
                Ok(next) => want = next,
                Err(RecvTimeoutError::Timeout) if client.is_none() => {
                    retry = (retry * 2).min(RETRY.1);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match updates.recv() {
                Ok(next) => want = next,
                Err(_) => return,
            }
        }
        if let Some(latest) = updates.try_iter().last() {
            want = latest;
        }
        let Some(presence) = &want else {
            if let Some(mut client) = client.take() {
                let _ = client.close();
            }
            continue;
        };
        if client.is_none() {
            let mut new = DiscordIpcClient::new(APP_ID);
            match new.connect() {
                Ok(()) => {
                    client = Some(new);
                    retry = RETRY.0;
                }
                Err(err) => {
                    tracing::debug!(%err, "Discord isn't reachable");
                    continue;
                }
            }
        }
        if let Some(connected) = &mut client
            && let Err(err) = show(connected, presence)
        {
            tracing::debug!(%err, "lost Discord");
            client = None;
        }
    }
}

fn show(client: &mut DiscordIpcClient, presence: &Presence) -> Result<(), Error> {
    let album = presence.album.as_deref().map(fit);
    let mut assets = Assets::new().large_image(presence.image.as_str());
    if let Some(album) = &album {
        assets = assets.large_text(album.as_str());
    }
    let mut timestamps = Timestamps::new().start(presence.start_ms);
    if let Some(end) = presence.end_ms {
        timestamps = timestamps.end(end);
    }
    let mut activity = Activity::new()
        .activity_type(ActivityType::Listening)
        .details(fit(&presence.title))
        .assets(assets)
        .timestamps(timestamps);
    if let Some(artist) = &presence.artist {
        activity = activity.state(fit(artist));
    }
    client.set_activity(activity)?;
    // Reading the reply keeps the socket drained and notices Discord quitting.
    let (_, reply) = client.recv()?;
    if reply["evt"] == "ERROR" {
        tracing::debug!(%reply, "Discord rejected the presence");
    }
    Ok(())
}

/// Sends the presence thread what to show whenever the engine or the
/// settings change.
async fn follow(
    engine: Engine,
    library: Option<Library>,
    mut settings: watch::Receiver<IntegrationSettings>,
    updates: mpsc::Sender<Option<Presence>>,
) {
    let mut states = engine.subscribe();
    // The release ID of the queue entry last looked up.
    let mut release: Option<(u64, Option<String>)> = None;
    let mut sent = None;
    loop {
        let on = settings.borrow_and_update().discord;
        let state = states.borrow_and_update().clone();
        let want = match &state.current {
            Some(item) if on && state.status == PlaybackStatus::Playing => {
                if release.as_ref().map(|(id, _)| *id) != Some(item.id) {
                    let id = match library.clone() {
                        Some(library) => {
                            let path = item.track.id.clone();
                            tokio::task::spawn_blocking(move || library.musicbrainz_album(&path))
                                .await
                                .ok()
                                .flatten()
                        }
                        None => None,
                    };
                    release = Some((item.id, id));
                }
                let id = release.as_ref().and_then(|(_, id)| id.as_deref());
                Some(presence(&item.track, id, &state))
            }
            _ => None,
        };
        if !same(&want, &sent) {
            if updates.send(want.clone()).is_err() {
                return;
            }
            sent = want;
        }
        tokio::select! {
            changed = states.changed() => if changed.is_err() { return },
            changed = settings.changed() => if changed.is_err() { return },
        }
    }
}

fn presence(track: &TrackRef, release: Option<&str>, state: &PlayerState) -> Presence {
    let now = engine::wall_clock_ms();
    let position = state.position_ms + now.saturating_sub(state.sampled_at_ms);
    let start_ms = now as i64 - position as i64;
    let file_name = || {
        Path::new(&track.id)
            .file_stem()
            .map_or_else(|| track.id.clone(), |s| s.to_string_lossy().into_owned())
    };
    Presence {
        title: track.title.clone().unwrap_or_else(file_name),
        artist: track.artist.clone(),
        album: track.album.clone(),
        // Discord needs a public URL, so local art can't be shown.
        image: release.map_or_else(
            || FALLBACK_ART.into(),
            |id| format!("https://coverartarchive.org/release/{id}/front-250"),
        ),
        start_ms,
        end_ms: track.duration_ms.map(|ms| start_ms + ms as i64),
    }
}

/// Whether showing `a` instead of `b` changes nothing visible: the same, or
/// only the start time drifting under a second.
fn same(a: &Option<Presence>, b: &Option<Presence>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            (a.start_ms - b.start_ms).abs() < 1000
                && Presence {
                    start_ms: b.start_ms,
                    end_ms: b.end_ms,
                    ..a.clone()
                } == *b
        }
        (a, b) => a.is_none() && b.is_none(),
    }
}

/// Discord rejects text under 2 or over 128 characters.
fn fit(text: &str) -> String {
    let mut fitted: String = text.chars().take(128).collect();
    if fitted.len() < text.len() {
        fitted.pop();
        fitted.push('…');
    }
    if fitted.chars().count() < 2 {
        // Not whitespace, so Discord doesn't trim it off again.
        fitted.push('\u{200B}');
    }
    fitted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fits_discord() {
        assert_eq!(fit("Song"), "Song");
        assert_eq!(fit("X"), "X\u{200B}");
        let long = fit(&"a".repeat(200));
        assert_eq!(long.chars().count(), 128);
        assert!(long.ends_with('…'));
        assert_eq!(fit(&"a".repeat(128)), "a".repeat(128));
    }

    #[test]
    fn drift_under_a_second_is_the_same_presence() {
        let a = Presence {
            title: "Song".into(),
            artist: None,
            album: None,
            image: FALLBACK_ART.into(),
            start_ms: 10_000,
            end_ms: Some(70_000),
        };
        let drifted = Presence {
            start_ms: 10_400,
            end_ms: Some(70_400),
            ..a.clone()
        };
        let seeked = Presence {
            start_ms: 5_000,
            end_ms: Some(65_000),
            ..a.clone()
        };
        assert!(same(&Some(a.clone()), &Some(drifted)));
        assert!(!same(&Some(a.clone()), &Some(seeked)));
        assert!(!same(&Some(a), &None));
        assert!(same(&None, &None));
    }
}
