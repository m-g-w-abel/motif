//! Scrobbling: tells services what plays (now playing) and what was listened
//! to (scrobbles). Runs on the host, so it works headless and submits once
//! however many windows are open. Scrobbles wait in a queue on disk until
//! the service takes them.

mod lastfm;
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

pub use lastfm::LastFm;
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
    /// The service no longer takes the secret (a revoked session): sign in again.
    SignedOut(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retry(message) | Self::Rejected(message) | Self::SignedOut(message) => {
                f.write_str(message)
            }
        }
    }
}

/// A scrobbling service.
pub trait Scrobbler: Clone + Send + Sync + 'static {
    /// Names its secret and its queue file.
    fn name(&self) -> &'static str;
    /// The most listens one [`Scrobbler::scrobble`] takes.
    const BATCH: usize;
    /// Its part of the integration settings.
    fn account<'a>(&self, settings: &'a mut IntegrationSettings) -> &'a mut ScrobbleAccount;
    /// Checks a credential the user pasted, or signs in through the browser,
    /// giving `show` the page to open. Returns the account name and the
    /// secret to keep for submitting.
    fn sign_in(
        &self,
        credential: &str,
        show: &(dyn Fn(&str) + Sync),
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
        ListenBrainz::new(http.clone()),
        actions.subscribe(),
        settings.clone(),
        dir.to_owned(),
    ));
    tokio::spawn(run(
        LastFm::new(http.clone()),
        actions.subscribe(),
        settings.clone(),
        dir.to_owned(),
    ));
    tokio::spawn(run(
        LastFm::libre_fm(http),
        actions.subscribe(),
        settings,
        dir.to_owned(),
    ));
    tokio::spawn(follow(engine, actions));
}

fn client() -> reqwest::Result<reqwest::Client> {
    // Fails harmlessly once a provider is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = reqwest::Client::builder()
        .user_agent(concat!("Motif/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30));
    match library::tls_roots() {
        Some(roots) => builder.tls_certs_only(roots),
        None => builder,
    }
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

/// Whether `id` looks like a MusicBrainz ID; services refuse malformed ones.
fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Signs in and out of one service and submits to it.
async fn run<S: Scrobbler>(
    service: S,
    mut actions: broadcast::Receiver<Action>,
    settings: watch::Sender<IntegrationSettings>,
    dir: PathBuf,
) {
    let name = service.name();
    let secrets = Secrets::new(&dir);
    let mut secret = secrets.get(name);
    let mut queue = Queue::open(dir.join(format!("scrobbles-{name}.json")));
    let mut changes = settings.subscribe();
    // When to try again and the wait before that, after a failure.
    let mut retry: Option<(Instant, Duration)> = None;
    let set = |change: &dyn Fn(&mut ScrobbleAccount)| {
        settings.send_if_modified(|s| {
            let before = service.account(s).clone();
            change(service.account(s));
            *service.account(s) != before
        })
    };
    // Saved errors and sign-ins are from the last run.
    set(&|a| {
        a.error = None;
        a.auth_url = None;
    });
    loop {
        let account = service
            .account(&mut changes.borrow_and_update().clone())
            .clone();
        if let Some(credential) = account.sign_in {
            set(&|a| a.sign_in = None);
            let credential = credential.trim();
            if credential.is_empty() {
                if let Err(err) = secrets.set(name, None) {
                    tracing::warn!(%err, "can't forget the {} secret", name);
                }
                secret = None;
                set(&|a| *a = ScrobbleAccount::default());
                continue;
            }
            let show = |url: &str| {
                tracing::info!(url, "{} sign-in: allow Motif on this page", name);
                set(&|a| a.auth_url = Some(url.into()));
            };
            let signed_in = {
                // A new request (signing out, or in again) stops this one.
                let asked_again = async {
                    while changes.changed().await.is_ok() {
                        if service
                            .account(&mut changes.borrow_and_update().clone())
                            .sign_in
                            .is_some()
                        {
                            return;
                        }
                    }
                    std::future::pending().await
                };
                tokio::select! {
                    signed_in = service.sign_in(credential, &show) => signed_in,
                    () = asked_again => {
                        set(&|a| a.auth_url = None);
                        continue;
                    }
                }
            };
            set(&|a| a.auth_url = None);
            match signed_in {
                Ok((user, new)) => {
                    let kept = secrets.set(name, Some(&new));
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
            && let Some(current) = &secret
        {
            match queue.flush(&service, current).await {
                Ok(()) => {
                    retry = None;
                    set(&|a| a.error = None);
                }
                // Keeps the queue for signing in again.
                Err(Error::SignedOut(err)) => {
                    if let Err(err) = secrets.set(name, None) {
                        tracing::warn!(%err, "can't forget the {} secret", name);
                    }
                    secret = None;
                    retry = None;
                    set(&|a| {
                        *a = ScrobbleAccount {
                            error: Some(format!("{err}. Sign in again.")),
                            ..Default::default()
                        }
                    });
                }
                Err(err) => {
                    let wait = retry.map_or(FIRST_RETRY, |(_, wait)| (wait * 2).min(LAST_RETRY));
                    retry = Some((Instant::now() + wait, wait));
                    tracing::debug!(%err, ?wait, "{} submission failed", name);
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
                            tracing::debug!(%err, "{} now playing failed", name);
                        }
                    });
                }
                Ok(Action::Scrobble(listen)) if active => {
                    if let Err(err) = queue.push(listen) {
                        tracing::warn!(%err, "can't save the {} scrobble queue", name);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "{} missed scrobbles", name);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Signs in through a page unless asked to "wait" there forever; every
    /// session it gives is revoked by the time it scrobbles.
    #[derive(Clone)]
    struct Fake;

    impl Scrobbler for Fake {
        const BATCH: usize = 1;
        fn name(&self) -> &'static str {
            "fake-run"
        }
        fn account<'a>(&self, settings: &'a mut IntegrationSettings) -> &'a mut ScrobbleAccount {
            &mut settings.lastfm
        }
        async fn sign_in(
            &self,
            credential: &str,
            show: &(dyn Fn(&str) + Sync),
        ) -> Result<(String, String), Error> {
            show("http://allow");
            if credential == "wait" {
                std::future::pending::<()>().await;
            }
            Ok(("rob".into(), "s3cret".into()))
        }
        async fn now_playing(&self, _: &str, _: &Listen) -> Result<(), Error> {
            Ok(())
        }
        async fn scrobble(&self, _: &str, _: &[Listen]) -> Result<(), Error> {
            Err(Error::SignedOut("revoked".into()))
        }
    }

    async fn until(
        seen: &mut watch::Receiver<IntegrationSettings>,
        what: &str,
        done: fn(&ScrobbleAccount) -> bool,
    ) {
        let waited =
            tokio::time::timeout(Duration::from_secs(5), seen.wait_for(|s| done(&s.lastfm)));
        assert!(waited.await.is_ok(), "{what}: {:?}", seen.borrow().lastfm);
    }

    #[tokio::test]
    async fn sign_in_can_be_stopped_and_a_revoked_session_signs_out() {
        let dir = std::env::temp_dir().join(format!("motif-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (settings, mut seen) = watch::channel(IntegrationSettings::default());
        let (actions, _) = broadcast::channel(8);
        tokio::spawn(run(
            Fake,
            actions.subscribe(),
            settings.clone(),
            dir.clone(),
        ));
        let ask =
            |credential: &str| settings.send_modify(|s| s.lastfm.sign_in = Some(credential.into()));

        ask("wait");
        until(&mut seen, "page shown", |a| {
            a.auth_url.as_deref() == Some("http://allow")
        })
        .await;
        ask("");
        until(&mut seen, "stopped", |a| {
            a.auth_url.is_none() && a.sign_in.is_none()
        })
        .await;
        assert_eq!(seen.borrow().lastfm.user, None);

        ask("go");
        until(&mut seen, "signed in", |a| a.user.is_some() && a.enabled).await;
        let listen = Listen::of(&TrackRef {
            artist: Some("Band".into()),
            title: Some("Song".into()),
            ..Default::default()
        })
        .unwrap();
        actions.send(Action::Scrobble(listen)).unwrap();
        until(&mut seen, "signed out", |a| {
            a.user.is_none() && a.error.is_some()
        })
        .await;
        assert!(!seen.borrow().lastfm.enabled);
        assert_eq!(Secrets::new(&dir).get("fake-run"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
