//! Scrobbling: tells services what plays (now playing) and what was listened
//! to (scrobbles). Runs on the host, so it works headless and submits once
//! however many windows are open. Scrobbles wait in a queue on disk until
//! the service takes them.

mod listenbrainz;
mod queue;
mod tracker;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use engine::Engine;
use protocol::{IntegrationSettings, ScrobbleAccount, TrackRef};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{broadcast, watch},
    time::Instant,
};

pub use listenbrainz::ListenBrainz;
pub use queue::Queue;
pub use tracker::{Action, Tracker};

use crate::settings::Secrets;

/// Retries after a failed submission start at this and double up to an hour.
const FIRST_RETRY: Duration = Duration::from_secs(30);
const LAST_RETRY: Duration = Duration::from_secs(3600);

/// A track as services want it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listen {
    /// When it started playing, Unix seconds.
    pub listened_at: u64,
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub recording_mbid: Option<String>,
    pub isrc: Option<String>,
}

impl Listen {
    /// `None` without the artist and title every service needs.
    fn of(track: &TrackRef) -> Option<Self> {
        Some(Self {
            listened_at: 0,
            artist: track.artist.clone()?,
            title: track.title.clone()?,
            album: track.album.clone(),
            duration_ms: track.duration_ms,
            recording_mbid: track.keys.musicbrainz_recording.clone(),
            isrc: track.keys.isrc.clone(),
        })
    }
}

#[derive(Debug)]
pub enum Error {
    /// Worth trying again later: offline, the service is down, a bad token.
    Retry(String),
    /// The service refuses these listens; retrying won't help.
    Rejected(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retry(message) | Self::Rejected(message) => f.write_str(message),
        }
    }
}

/// A scrobbling service.
pub trait Scrobbler: Clone + Send + Sync + 'static {
    /// Names its secret and its queue file.
    const NAME: &str;
    /// The most listens one [`Scrobbler::scrobble`] takes.
    const BATCH: usize;
    /// Its part of the integration settings.
    fn account(settings: &mut IntegrationSettings) -> &mut ScrobbleAccount;
    /// Checks a credential the user pasted. Returns the account name and the
    /// secret to keep for submitting.
    fn sign_in(
        &self,
        credential: &str,
    ) -> impl Future<Output = Result<(String, String), Error>> + Send;
    fn now_playing(
        &self,
        secret: &str,
        listen: &Listen,
    ) -> impl Future<Output = Result<(), Error>> + Send;
    /// Submits listens, oldest first.
    fn scrobble(
        &self,
        secret: &str,
        listens: &[Listen],
    ) -> impl Future<Output = Result<(), Error>> + Send;
}

/// Starts scrobbling what `engine` plays to the services signed in through
/// `settings`. Secrets and unsent scrobbles are kept in `dir`.
pub fn start(engine: Engine, settings: watch::Sender<IntegrationSettings>, dir: &Path) {
    let http = match client() {
        Ok(http) => http,
        Err(err) => {
            tracing::warn!(%err, "no scrobbling: can't make an HTTP client");
            return;
        }
    };
    let (actions, _) = broadcast::channel(64);
    tokio::spawn(run(
        ListenBrainz::new(http),
        actions.subscribe(),
        settings,
        dir.to_owned(),
    ));
    tokio::spawn(follow(engine, actions));
}

fn client() -> reqwest::Result<reqwest::Client> {
    // Fails harmlessly once a provider is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(concat!("Motif/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
}

/// Turns engine states into now playing and scrobble actions.
async fn follow(engine: Engine, actions: broadcast::Sender<Action>) {
    let mut states = engine.subscribe();
    let mut tracker = Tracker::default();
    let mut found = tracker.update(&states.borrow_and_update());
    loop {
        for action in found.drain(..) {
            let _ = actions.send(action);
        }
        let due = tracker.due_in_ms();
        tokio::select! {
            changed = states.changed() => {
                if changed.is_err() {
                    return;
                }
                found = tracker.update(&states.borrow_and_update());
            }
            _ = tokio::time::sleep(Duration::from_millis(due.unwrap_or(0))), if due.is_some() => {
                found.extend(tracker.advance(engine::wall_clock_ms()));
            }
        }
    }
}

/// Signs in and out of one service and submits to it.
async fn run<S: Scrobbler>(
    service: S,
    mut actions: broadcast::Receiver<Action>,
    settings: watch::Sender<IntegrationSettings>,
    dir: PathBuf,
) {
    let secrets = Secrets::new(&dir);
    let mut secret = secrets.get(S::NAME);
    let mut queue = Queue::open(dir.join(format!("scrobbles-{}.json", S::NAME)));
    let mut changes = settings.subscribe();
    // When to try again and the wait before that, after a failure.
    let mut retry: Option<(Instant, Duration)> = None;
    let set = |change: &dyn Fn(&mut ScrobbleAccount)| {
        settings.send_if_modified(|s| {
            let before = S::account(s).clone();
            change(S::account(s));
            *S::account(s) != before
        })
    };
    // Saved errors are from the last run.
    set(&|a| a.error = None);
    loop {
        let account = S::account(&mut changes.borrow_and_update().clone()).clone();
        if let Some(credential) = account.sign_in {
            set(&|a| a.sign_in = None);
            let credential = credential.trim();
            if credential.is_empty() {
                if let Err(err) = secrets.set(S::NAME, None) {
                    tracing::warn!(%err, "can't forget the {} secret", S::NAME);
                }
                secret = None;
                set(&|a| *a = ScrobbleAccount::default());
                continue;
            }
            match service.sign_in(credential).await {
                Ok((user, new)) => {
                    let kept = secrets.set(S::NAME, Some(&new));
                    let error = kept.err().map(|e| format!("Can't keep the sign-in: {e}"));
                    secret = error.is_none().then_some(new);
                    set(&|a| {
                        a.enabled = error.is_none();
                        a.user = error.is_none().then(|| user.clone());
                        a.error = error.clone();
                    });
                }
                Err(err) => {
                    set(&|a| a.error = Some(err.to_string()));
                }
            }
            continue;
        }
        let active = account.enabled && secret.is_some();
        if active
            && !queue.is_empty()
            && retry.is_none_or(|(at, _)| at <= Instant::now())
            && let Some(secret) = &secret
        {
            match queue.flush(&service, secret).await {
                Ok(()) => {
                    retry = None;
                    set(&|a| a.error = None);
                }
                Err(err) => {
                    let wait = retry.map_or(FIRST_RETRY, |(_, wait)| (wait * 2).min(LAST_RETRY));
                    retry = Some((Instant::now() + wait, wait));
                    tracing::debug!(%err, ?wait, "{} submission failed", S::NAME);
                    let waiting = queue.len();
                    set(&|a| {
                        a.error = Some(format!("{err} ({waiting} waiting to be sent)"));
                    });
                }
            }
        }
        let retry_at = retry
            .filter(|_| active && !queue.is_empty())
            .map(|(at, _)| at);
        tokio::select! {
            action = actions.recv() => match action {
                Ok(Action::NowPlaying(listen)) if active => {
                    let (service, secret) = (service.clone(), secret.clone().unwrap_or_default());
                    tokio::spawn(async move {
                        if let Err(err) = service.now_playing(&secret, &listen).await {
                            tracing::debug!(%err, "{} now playing failed", S::NAME);
                        }
                    });
                }
                Ok(Action::Scrobble(listen)) if active => {
                    if let Err(err) = queue.push(listen) {
                        tracing::warn!(%err, "can't save the {} scrobble queue", S::NAME);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "{} missed scrobbles", S::NAME);
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            changed = changes.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = tokio::time::sleep_until(retry_at.unwrap_or_else(Instant::now)), if retry_at.is_some() => {}
        }
    }
}
