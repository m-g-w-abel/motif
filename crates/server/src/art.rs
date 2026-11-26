//! Covers and waveforms of a linked engine's tracks ([`net::Alpn::Art`],
//! #266): the catalog only carries art keys, so the first view asks the
//! owner. One QUIC stream per request: an [`Ask`] frame (sync.rs's framing),
//! then `{"Ok": null}` or `{"Err": why}`, then the bytes and the end of the
//! stream. Covers are stored like folder images (`Library::save_remote_art`)
//! and waveforms by audio hash, so each is fetched once. The owner serves
//! only linked engines, and covers by key, never a path.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use library::{Library, catalog::valid_art_key};
use net::{Alpn, Connection, RecvStream, SendStream};
use protocol::Reach;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};

use crate::{
    Pairing,
    sync::{read, write},
};

#[derive(Debug, Serialize, Deserialize)]
enum Ask {
    Art { key: String },
    Waveform { id: i64 },
}

/// The biggest cover sent: [`library::Library::art_image`]'s largest size,
/// so one is never larger in memory than the art route's own.
const COVER_SIZE: u32 = 1024;
/// The most bytes read for an answer.
const MAX_ANSWER: usize = 16 << 20;
/// Requests answered at once on a connection (each can hold an image).
const SERVING: usize = 3;
/// How long a request, dialling excluded, may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// How long dialling may take, and how long a failed dial counts for every
/// request meanwhile, so a grid of covers of an engine that can't be reached
/// doesn't hold the art route's slots dialling it again and again.
const DIAL: Duration = Duration::from_secs(5);

/// Answers requests on `conn` from a linked engine (while `linked` says it
/// is) until it closes.
pub(crate) async fn serve(library: Library, conn: Connection, linked: net::Linked) {
    let peer = net::key(&conn);
    let slots = Arc::new(Semaphore::new(SERVING));
    loop {
        let Ok(slot) = slots.clone().acquire_owned().await else {
            return;
        };
        let Ok((send, recv)) = conn.accept_bi().await else {
            return;
        };
        let (library, allowed) = (library.clone(), linked(&peer));
        tokio::spawn(async move {
            let answered = answer(&library, allowed, send, recv);
            match tokio::time::timeout(TIMEOUT, answered).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::debug!(%err, "art: not sent"),
                Err(_) => tracing::debug!("art: request not taken in time"),
            }
            drop(slot);
        });
    }
}

async fn answer(
    library: &Library,
    allowed: bool,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<(), String> {
    let ask: Ask = read(&mut recv).await?;
    let found = if allowed {
        let library = library.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            match ask {
                Ask::Art { key } if !valid_art_key(&key) => Err("not an artwork key".into()),
                Ask::Art { key } => library
                    .art_image(&key, Some(COVER_SIZE))
                    .map(|(bytes, _)| bytes)
                    .map_err(|e| e.to_string()),
                Ask::Waveform { id } => {
                    let own = library.own_file(id).ok_or("not on this engine")?;
                    library
                        .waveform(&own.path.to_string_lossy())
                        .map_err(|e| e.to_string())
                }
            }
        })
        .await
        .map_err(|e| e.to_string())?
    } else {
        Err("not linked".into())
    };
    match found {
        Ok(bytes) => {
            write(&mut send, &Ok::<(), String>(())).await?;
            send.write_all(&bytes).await.map_err(|e| e.to_string())?;
        }
        Err(err) => write(&mut send, &Err::<(), _>(err)).await?,
    }
    send.finish().map_err(|e| e.to_string())
}

/// A kept connection to an engine, or why dialling it failed, and when.
type Dialled = Result<Connection, (String, Instant)>;

/// Asks linked engines for covers and waveforms, over a connection kept
/// for each while it lives.
pub(crate) struct Remote {
    conns: Mutex<HashMap<String, Dialled>>,
    /// Fetches at once: a grid asks for hundreds of covers.
    gate: Semaphore,
}

impl Remote {
    pub(crate) fn new() -> Self {
        Self {
            conns: Mutex::new(HashMap::new()),
            gate: Semaphore::new(3),
        }
    }

    /// Fetches the cover `key` from its owner into `library`, unless a
    /// request just before did. A key nobody linked shows is not an error:
    /// the art route then answers as for any unknown one.
    pub(crate) async fn cover(
        &self,
        pairing: &Pairing,
        library: &Library,
        key: &str,
    ) -> Result<(), String> {
        let _turn = self.gate.acquire().await.map_err(|e| e.to_string())?;
        let owner = {
            let (library, key) = (library.clone(), key.to_string());
            tokio::task::spawn_blocking(move || library.art_owner(&key))
                .await
                .map_err(|e| e.to_string())?
        };
        let Some(owner) = owner else { return Ok(()) };
        let bytes = self
            .ask(pairing, &owner, &Ask::Art { key: key.into() })
            .await?;
        let (library, key) = (library.clone(), key.to_string());
        tokio::task::spawn_blocking(move || library.save_remote_art(&key, &bytes))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    /// The waveform of the linked engine's track at `path` (`remote:…`),
    /// from the cache or its owner; `None` if the track is a local one.
    pub(crate) async fn waveform(
        &self,
        pairing: &Pairing,
        library: &Library,
        path: &str,
    ) -> Option<Result<Vec<u8>, String>> {
        let (device, id, hash) = library.remote_waveform(path)?;
        if let Some(levels) = hash.as_deref().and_then(|h| library.cached_waveform(h)) {
            return Some(Ok(levels));
        }
        let levels = self.ask(pairing, &device, &Ask::Waveform { id }).await;
        if let (Ok(levels), Some(hash)) = (&levels, &hash) {
            library.store_waveform(hash, levels);
        }
        Some(levels)
    }

    async fn ask(&self, pairing: &Pairing, owner: &str, ask: &Ask) -> Result<Vec<u8>, String> {
        if pairing.reach(owner) == Reach::Unreachable {
            return Err("the linked engine can't be reached now".into());
        }
        let conn = self.connection(pairing, owner).await?;
        let asked = async {
            let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
            write(&mut send, ask).await?;
            send.finish().map_err(|e| e.to_string())?;
            let answer: Result<(), String> = read(&mut recv).await?;
            answer?;
            recv.read_to_end(MAX_ANSWER)
                .await
                .map_err(|e| e.to_string())
        };
        tokio::time::timeout(TIMEOUT, asked)
            .await
            .unwrap_or_else(|_| Err("the linked engine stopped answering".into()))
    }

    async fn connection(&self, pairing: &Pairing, owner: &str) -> Result<Connection, String> {
        // Held while dialling: requests wait for that one dial, which fails
        // or is remembered as failed for the others.
        let mut conns = self.conns.lock().await;
        match conns.get(owner) {
            Some(Ok(conn)) if conn.close_reason().is_none() => return Ok(conn.clone()),
            Some(Err((err, at))) if at.elapsed() < DIAL => return Err(err.clone()),
            _ => {}
        }
        let dialled = async {
            let (_, addr) = pairing
                .linked()
                .into_iter()
                .find(|(key, _)| key == owner)
                .ok_or("the engine with it is no longer linked")?;
            let net = pairing
                .net()
                .ok_or("on a linked engine, but not connected to any now")?;
            let dialled = net.connect(owner, &addr, Alpn::Art);
            let conn = tokio::time::timeout(DIAL, dialled)
                .await
                .map_err(|_| "the linked engine didn't answer".to_string())??;
            Ok::<_, String>(conn)
        };
        let dialled = dialled.await;
        conns.insert(
            owner.into(),
            dialled.clone().map_err(|err| (err, Instant::now())),
        );
        dialled
    }
}
