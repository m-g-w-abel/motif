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
use library::{Library, sync::Unscrobbled};
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

/// Plays from history older than this aren't scrobbled: Last.fm takes none
/// older than 14 days, and for the others it keeps a newly signed-in account
/// from sending the whole history.
const MAX_AGE: Duration = Duration::from_secs(14 * 86_400);

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

impl Listen {
    /// A play from the synced history; `None` without artist and title.
    fn of_play(play: &Unscrobbled) -> Option<Self> {
        let track = &play.track;
        Some(Self {
            listened_at: play.started,
            artist: track.artist.clone()?,
            title: track.title.clone()?,
            album: track.album.clone(),
            duration_ms: track.duration_ms,
            recording_mbid: track.musicbrainz.clone(),
            isrc: track.isrc.clone(),
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
/// `settings`. Secrets and unsent scrobbles are kept in `dir`. Listens that
/// count as played go to the library's history, if there is one.
pub fn start(
    engine: Engine,
    library: Option<Library>,
    settings: watch::Sender<IntegrationSettings>,
    dir: &Path,
) {
    let (actions, _) = broadcast::channel(64);
    match client() {
        Ok(http) => {
            tokio::spawn(run(
                ListenBrainz::new(http.clone()),
                actions.subscribe(),
                settings.clone(),
                dir.to_owned(),
                library.clone(),
            ));
            tokio::spawn(run(
                LastFm::new(http.clone()),
                actions.subscribe(),
                settings.clone(),
                dir.to_owned(),
                library.clone(),
            ));
            tokio::spawn(run(
                LastFm::libre_fm(http),
                actions.subscribe(),
                settings,
                dir.to_owned(),
                library.clone(),
            ));
        }
        // History is kept anyway.
        Err(err) => tracing::warn!(%err, "no scrobbling: can't make an HTTP client"),
    }
    tokio::spawn(follow(engine, library, actions));
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
async fn follow(engine: Engine, library: Option<Library>, actions: broadcast::Sender<Action>) {
    let mut states = engine.subscribe();
    let mut tracker = Tracker::default();
    let mut found = tracker.update(&states.borrow_and_update());
    // The scrobble of a listen, held until its play is known to be in the history.
    let mut held = None;
    loop {
        for action in found.drain(..) {
            match (action, &library) {
                (Action::Scrobble(listen), Some(_)) => held = Some(listen),
                (Action::Played { track, started_at }, Some(library)) => {
                    let library = library.clone();
                    // SQLite waits on the scanner's writes, so not on this task.
                    let recorded = tokio::task::spawn_blocking(move || {
                        library
                            .record_play(&track, started_at)
                            .unwrap_or_else(|err| {
                                tracing::warn!(%err, "can't record the play");
                                false
                            })
                    })
                    .await
                    .unwrap_or(false);
                    // Plays in the history are scrobbled from there (`pull_history`),
                    // once, whichever linked device has an account; others go now.
                    if let Some(listen) = held.take().filter(|_| !recorded) {
                        let _ = actions.send(Action::Scrobble(listen));
                    }
                }
                (action, _) => {
                    let _ = actions.send(action);
                }
            }
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

/// Queues the plays in the synced history that `name` hasn't been given
/// (made here or on a linked device without an account), then marks them
/// for every device. With `send` off (signed in but switched off) it only
/// marks what was played here, so switching on later doesn't send it. Marked after queueing: a crash between sends a play
/// twice, which the services' own dedupe (same track and time) absorbs.
async fn pull_history(library: &Library, name: &'static str, queue: &mut Queue, send: bool) {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().saturating_sub(MAX_AGE.as_secs()));
    let lib = library.clone();
    let plays = match tokio::task::spawn_blocking(move || {
        let own = lib.device();
        lib.unscrobbled(name, since).map(|plays| (plays, own))
    })
    .await
    {
        Ok(Ok((mut plays, own))) => {
            if !send {
                plays.retain(|p| p.device == own);
            }
            plays
        }
        Ok(Err(err)) => return tracing::warn!(%err, "can't read the play history"),
        _ => return,
    };
    if plays.is_empty() {
        return;
    }
    for listen in plays.iter().filter(|_| send).filter_map(Listen::of_play) {
        if let Err(err) = queue.push(listen) {
            return tracing::warn!(%err, "can't save the {name} scrobble queue");
        }
    }
    let lib = library.clone();
    // Plays without an artist or title can't be sent; they are marked too, so they aren't read again.
    let marked = tokio::task::spawn_blocking(move || {
        plays
            .iter()
            .try_for_each(|p| lib.mark_scrobbled(name, &p.device, p.seq).map(drop))
    })
    .await;
    if let Ok(Err(err)) = marked {
        tracing::warn!(%err, "can't mark plays as scrobbled");
    }
}

/// Signs in and out of one service and submits to it.
async fn run<S: Scrobbler>(
    service: S,
    mut actions: broadcast::Receiver<Action>,
    settings: watch::Sender<IntegrationSettings>,
    dir: PathBuf,
    library: Option<Library>,
) {
    let name = service.name();
    let secrets = Secrets::new(&dir);
    let mut secret = secrets.get(name);
    let mut queue = Queue::open(dir.join(format!("scrobbles-{name}.json")));
    let mut changes = settings.subscribe();
    let mut history = library.as_ref().map(Library::subscribe);
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
        if let Some(library) = &library
            && secret.is_some()
        {
            pull_history(library, name, &mut queue, account.enabled).await;
        }
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
            // Plays were added here or synced from another device.
            _ = async {
                match &mut history {
                    Some(history) => drop(history.changed().await),
                    None => std::future::pending().await,
                }
            } => {}
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
            None,
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

    /// Takes everything; remembers what it was given.
    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl Scrobbler for Sink {
        const BATCH: usize = 10;
        fn name(&self) -> &'static str {
            "sink"
        }
        fn account<'a>(&self, settings: &'a mut IntegrationSettings) -> &'a mut ScrobbleAccount {
            &mut settings.lastfm
        }
        async fn sign_in(
            &self,
            _: &str,
            _: &(dyn Fn(&str) + Sync),
        ) -> Result<(String, String), Error> {
            unreachable!()
        }
        async fn now_playing(&self, _: &str, _: &Listen) -> Result<(), Error> {
            unreachable!()
        }
        async fn scrobble(&self, _: &str, listens: &[Listen]) -> Result<(), Error> {
            let mut got = self.0.lock().unwrap();
            got.extend(
                listens
                    .iter()
                    .map(|l| format!("{}@{}", l.title, l.listened_at)),
            );
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_play_synced_from_a_device_without_an_account_is_scrobbled_once() {
        use library::sync::{Logged, Op, Track};

        let dir =
            std::env::temp_dir().join(format!("motif-history-scrobble-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        library.set_device("desktop").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let play = |seq, title: &str, artist: Option<&str>, started| Logged {
            device: "phone".into(),
            seq,
            at: now * 1000,
            op: Op::Play {
                track: Track {
                    title: Some(title.into()),
                    artist: artist.map(Into::into),
                    ..Default::default()
                },
                started,
            },
        };
        library
            .merge(vec![
                play(1, "Song", Some("Band"), now - 60),
                play(2, "Old", Some("Band"), now - 15 * 86_400),
                play(3, "Nameless", None, now - 30),
            ])
            .unwrap();

        let sink = Sink::default();
        let mut queue = Queue::open(dir.join("scrobbles-sink.json"));
        for _ in 0..2 {
            pull_history(&library, "sink", &mut queue, true).await;
            queue.flush(&sink, "secret").await.unwrap();
        }
        assert_eq!(
            *sink.0.lock().unwrap(),
            [format!("Song@{}", now - 60)],
            "once; not the one past 14 days, nor the one services can't take"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn plays_made_here_while_a_service_is_switched_off_are_never_sent() {
        use library::sync::{Logged, Op, Track};

        let dir = std::env::temp_dir().join(format!("motif-off-scrobble-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        library.set_device("desktop").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let play = |device: &str, title: &str| Logged {
            device: device.into(),
            seq: 1,
            at: now * 1000,
            op: Op::Play {
                track: Track {
                    title: Some(title.into()),
                    artist: Some("Band".into()),
                    ..Default::default()
                },
                started: now - 60,
            },
        };
        library
            .merge(vec![play("desktop", "Private"), play("phone", "Shared")])
            .unwrap();

        let sink = Sink::default();
        let mut queue = Queue::open(dir.join("scrobbles-sink.json"));
        // Signed in but switched off: what was played here is done with, unsent.
        pull_history(&library, "sink", &mut queue, false).await;
        assert!(queue.is_empty());
        // Switched back on.
        pull_history(&library, "sink", &mut queue, true).await;
        queue.flush(&sink, "secret").await.unwrap();
        assert_eq!(
            *sink.0.lock().unwrap(),
            [format!("Shared@{}", now - 60)],
            "only what another device played"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
