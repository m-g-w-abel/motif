//! Plays tracks held by linked engines ([`net::Alpn::Files`], #243): the
//! reader asks the owner for byte ranges of one of its own tracks by id, so
//! decoders read and seek it as if it were a file here. One connection per
//! stream; each range is a QUIC stream of its own: a [`Range`] frame
//! (sync.rs's framing), then the owner's answer frame, `{"Ok": size of the
//! file}` or `{"Err": why}`, then up to `len` bytes (fewer only at the end of
//! the file) and the end of the stream. The owner serves only its own tracks,
//! by id, never a path, and only to linked engines: the endpoint lets in no
//! others, and each range is refused once the asker is unlinked.
//!
//! Off the LAN the reader asks for the track as Opus instead ([`Opus`],
//! #244), from a point: the answer frame says what comes, [`Sent`], then the
//! owner encodes as the reader takes it, packet by packet (2 bytes of
//! length, 2 of the frames of audio in it, the packet), and ends the stream
//! after the last. A seek asks again from there. Files that would lose more
//! than they save come as they are ([`library::transcode::plan`]), as ranges.

use std::{
    collections::HashMap,
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use engine::{ByteSource, Opened, Source, audio::decoder::Decoder};
use library::{
    Library,
    transcode::{self, Encode},
};
use net::{Alpn, Connection, Net, RecvStream, SendStream};
use protocol::{Quality, Reach, Streamed};
use serde::{Deserialize, Serialize};
use tokio::{runtime::Handle, sync::watch};

use crate::{
    Pairing,
    sync::{read, write},
};

/// What a stream asks for.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum Ask {
    Range(Range),
    Opus(Opus),
}

/// A request for bytes `offset..offset + len` of the owner's track `id`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Range {
    id: i64,
    offset: u64,
    len: u32,
}

/// A request for the owner's track `id` from `from_ms` on, as Opus at
/// `quality` where that is worth it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Opus {
    id: i64,
    from_ms: u64,
    quality: Quality,
}

/// What the owner sends for an [`Opus`] request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Sent {
    /// Packets from `from_ms` (the point asked for, as near as the file
    /// seeks); the first `pre_skip` frames decoded are not audio.
    Opus {
        channels: u8,
        pre_skip: u16,
        from_ms: u64,
        /// The source's, which the copy shares (#241).
        audio_hash: Option<String>,
    },
    /// Nothing: read the file's ranges instead.
    Original,
}

/// Bytes per range: a third of a second of CD-quality FLAC.
const CHUNK: u64 = 64 << 10;
/// Ranges fetched ahead of the decoder while it reads straight through
/// (2 MiB: about 16 s of CD-quality FLAC, 4 s of 24-bit/192 kHz).
const AHEAD: u64 = 32;
/// The largest range served.
const MAX_RANGE: u32 = CHUNK as u32;
/// Ranges answered at once on a connection; more wait to be read.
const SERVING: usize = 2 * IN_FLIGHT;
/// Ranges asked for at once.
const IN_FLIGHT: usize = 4;
/// How long a range may take before the link counts as lost.
const STALL: Duration = Duration::from_secs(8);
/// How long dialling a linked engine may take.
const DIAL: Duration = Duration::from_secs(5);

/// A slower link, for measuring: `rate` bytes a second shared by every range
/// on the connection, each answered `latency` late.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pace {
    pub rate: u64,
    pub latency: Duration,
}

/// Answers range requests on `conn` from a linked engine (while `linked`
/// says it is) until it closes.
pub(crate) async fn serve(
    library: Library,
    conn: Connection,
    linked: net::Linked,
    pace: Option<Pace>,
) {
    let peer = net::key(&conn);
    let slots = Arc::new(tokio::sync::Semaphore::new(SERVING));
    // When the paced link is next free.
    let free = Arc::new(tokio::sync::Mutex::new(tokio::time::Instant::now()));
    loop {
        let Ok(slot) = slots.clone().acquire_owned().await else {
            return;
        };
        let Ok((send, recv)) = conn.accept_bi().await else {
            return;
        };
        let (library, free) = (library.clone(), free.clone());
        let allowed = linked(&peer);
        tokio::spawn(async move {
            let mut recv = recv;
            let asked = tokio::time::timeout(STALL, read::<Ask>(&mut recv)).await;
            let done = match asked {
                Ok(Ok(Ask::Range(range))) => {
                    let answered = answer(&library, allowed, range, send, pace, &free);
                    tokio::time::timeout(STALL, answered)
                        .await
                        .unwrap_or_else(|_| Err("range not taken in time".into()))
                }
                // Lasts as long as the track plays: it holds no slot.
                Ok(Ok(Ask::Opus(opus))) => {
                    drop(slot);
                    encode(library, allowed, opus, send).await
                }
                Ok(Err(err)) => Err(err),
                Err(_) => Err("nothing asked in time".into()),
            };
            if let Err(err) = done {
                tracing::debug!(%err, "files: not sent");
            }
        });
    }
}

async fn answer(
    library: &Library,
    allowed: bool,
    range: Range,
    mut send: SendStream,
    pace: Option<Pace>,
    free: &tokio::sync::Mutex<tokio::time::Instant>,
) -> Result<(), String> {
    if !allowed {
        write(&mut send, &Err::<u64, _>("not linked")).await?;
        return send.finish().map_err(|e| e.to_string());
    }
    let library = library.clone();
    let found = tokio::task::spawn_blocking(move || -> io::Result<(u64, Vec<u8>)> {
        let own = library
            .own_file(range.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not on this engine"))?;
        let mut file = std::fs::File::open(own.path)?;
        let size = file.metadata()?.len();
        file.seek(SeekFrom::Start(range.offset))?;
        let mut bytes = Vec::new();
        file.take(range.len.min(MAX_RANGE).into())
            .read_to_end(&mut bytes)?;
        Ok((size, bytes))
    })
    .await
    .map_err(|e| e.to_string())?;
    let (size, bytes) = match found {
        Ok(found) => found,
        Err(err) => {
            write(&mut send, &Err::<u64, _>(err.to_string())).await?;
            return send.finish().map_err(|e| e.to_string());
        }
    };
    if let Some(pace) = pace {
        tokio::time::sleep(pace.latency).await;
        let mut free = free.lock().await;
        let took = Duration::from_secs_f64(bytes.len() as f64 / pace.rate as f64);
        *free = (*free).max(tokio::time::Instant::now()) + took;
        tokio::time::sleep_until(*free).await;
    }
    write(&mut send, &Ok::<_, String>(size)).await?;
    send.write_all(&bytes).await.map_err(|e| e.to_string())?;
    send.finish().map_err(|e| e.to_string())
}

/// Bytes of packets written at once: half a second of Opus 256.
const BATCH: usize = 16 << 10;

/// Answers an [`Opus`] request: encodes the track as the reader takes it,
/// on a thread of its own, until its end or the reader stops it.
async fn encode(
    library: Library,
    allowed: bool,
    opus: Opus,
    mut send: SendStream,
) -> Result<(), String> {
    let runtime = Handle::current();
    tokio::task::spawn_blocking(move || {
        let refuse = |send: &mut SendStream, why: &str| {
            runtime.block_on(write(send, &Err::<Sent, _>(why)))?;
            send.finish().map_err(|e| e.to_string())
        };
        if !allowed {
            return refuse(&mut send, "not linked");
        }
        let Some(own) = library.own_file(opus.id) else {
            return refuse(&mut send, "not on this engine");
        };
        let codec = own.codec.as_deref();
        let Some(kbps) = transcode::plan(codec, own.bitrate, opus.quality) else {
            runtime.block_on(write(&mut send, &Ok::<_, String>(Sent::Original)))?;
            return send.finish().map_err(|e| e.to_string());
        };
        let from = Duration::from_millis(opus.from_ms);
        let mut encode = match Encode::open(&own.path, from, kbps) {
            Ok(encode) => encode,
            Err(err) => return refuse(&mut send, &err),
        };
        let sent = Sent::Opus {
            channels: encode.channels() as u8,
            pre_skip: encode.pre_skip(),
            from_ms: encode.reached().as_millis() as u64,
            audio_hash: own.audio_hash,
        };
        runtime.block_on(write(&mut send, &Ok::<_, String>(sent)))?;
        let mut batch = Vec::with_capacity(BATCH);
        loop {
            batch.clear();
            let mut ended = false;
            while batch.len() < BATCH && !ended {
                match encode.packet() {
                    Ok(Some(packet)) => {
                        batch.extend_from_slice(&(packet.data.len() as u16).to_be_bytes());
                        batch.extend_from_slice(&packet.frames.to_be_bytes());
                        batch.extend_from_slice(&packet.data);
                    }
                    Ok(None) => ended = true,
                    // Reset, so the reader can't take it for the end.
                    Err(err) => {
                        let _ = send.reset(0u32.into());
                        return Err(err);
                    }
                }
            }
            runtime
                .block_on(send.write_all(&batch))
                .map_err(|e| e.to_string())?;
            if ended {
                return send.finish().map_err(|e| e.to_string());
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Bytes `offset..offset + len` of track `id` over `conn`, and the file's size.
async fn fetch(
    conn: &Connection,
    id: i64,
    offset: u64,
    len: u32,
) -> Result<(u64, Vec<u8>), String> {
    let ask = async {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        write(&mut send, &Range { id, offset, len }).await?;
        send.finish().map_err(|e| e.to_string())?;
        let size: Result<u64, String> = read(&mut recv).await?;
        let size = size?;
        let bytes = recv
            .read_to_end(len as usize)
            .await
            .map_err(|e| e.to_string())?;
        Ok((size, bytes))
    };
    tokio::time::timeout(STALL, ask)
        .await
        .unwrap_or_else(|_| Err("the linked engine stopped sending".into()))
}

/// The source playing a track the linked engines in `copies` (key, their
/// id for it) hold, tried reachable ones first. Opening it dials.
pub(crate) fn source(pairing: Pairing, copies: Vec<(String, i64)>, runtime: Handle) -> Source {
    let (key, id) = &copies[0];
    let name = format!("remote:{key}:{id}");
    // Opened again for each decoder tried: they share the connection and
    // what it fetched while it lives, or a failure to dial for a while.
    let open = Mutex::new(None::<(Result<Dialled, String>, Instant)>);
    let (tell, streamed) = watch::channel(None);
    Source::Stream {
        name,
        streamed,
        open: Arc::new(move || {
            let mut open = open.lock().unwrap_or_else(|e| e.into_inner());
            let dialled = match &*open {
                Some((Ok(dialled), _)) if dialled.link().conn().close_reason().is_none() => {
                    dialled.clone()
                }
                Some((Err(err), at)) if at.elapsed() < DIAL => {
                    return Err(io::Error::other(err.clone()));
                }
                _ => {
                    let dialled = dial(&pairing, &copies, tell.clone());
                    let dialled = runtime.block_on(dialled);
                    let (dialled, _) = open.insert((dialled, Instant::now()));
                    dialled.clone().map_err(io::Error::other)?
                }
            };
            dialled.open()
        }),
    }
}

/// A linked engine's track, connected to: as ranges of its file, or as Opus
/// encoded live (with the first answer, for the first decoder to take).
#[derive(Clone)]
enum Dialled {
    Original(Arc<Remote>),
    Opus(Arc<Link>, Quality, Arc<Mutex<Option<Started>>>),
}

impl Dialled {
    fn link(&self) -> &Link {
        match self {
            Dialled::Original(remote) => &remote.link,
            Dialled::Opus(link, ..) => link,
        }
    }

    fn open(&self) -> io::Result<Opened> {
        Ok(match self {
            Dialled::Original(remote) => {
                Opened::Bytes(Box::new(Reader::new(remote.clone(), remote.len)))
            }
            Dialled::Opus(link, quality, first) => {
                let first = first.lock().unwrap_or_else(|e| e.into_inner()).take();
                let live = match first {
                    Some(started) => Live::new(link.clone(), *quality, started),
                    None => Live::open(link.clone(), *quality, Duration::ZERO),
                };
                Opened::Decoded(Box::new(live?))
            }
        })
    }
}

/// Connects to the first engine in `copies` that answers with the start of
/// its copy.
async fn dial(
    pairing: &Pairing,
    copies: &[(String, i64)],
    tell: watch::Sender<Option<Streamed>>,
) -> Result<Dialled, String> {
    let net = pairing
        .net()
        .ok_or("on a linked engine, but not connected to any now")?;
    let mut copies = copies.to_vec();
    copies.sort_by_key(|(key, _)| pairing.reach(key) == Reach::Unreachable);
    let quality = |conn: &Connection| match net::lan(conn) {
        true => Quality::Original,
        false => pairing.stream_quality(),
    };
    let mut reasons = Vec::new();
    for (key, id) in copies {
        let addr = pairing.linked().into_iter().find(|(k, _)| *k == key);
        let Some((_, addr)) = addr else { continue };
        let locate = locate(pairing.clone(), key.clone());
        let from = (pairing.linked_name(&key).unwrap_or_default(), tell.clone());
        let connected = connect(&net, &key, &addr, id, locate, &quality, from);
        match tokio::time::timeout(DIAL, connected).await {
            Ok(Ok(opened)) => return Ok(opened),
            Ok(Err(err)) => reasons.push(err),
            Err(_) => reasons.push("no answer".into()),
        }
    }
    Err(format!(
        "no linked engine with it is reachable ({})",
        reasons.join("; ")
    ))
}

/// Where a linked engine can be dialled now: this engine's endpoint (it
/// changes when it restarts) and its address, or why not (it was unlinked).
type Locate = Arc<dyn Fn() -> Result<(Net, net::PeerAddr), String> + Send + Sync>;

fn locate(pairing: Pairing, key: String) -> Locate {
    Arc::new(move || {
        let addr = pairing.linked().into_iter().find(|(k, _)| *k == key);
        let (_, addr) = addr.ok_or("the engine with it is no longer linked")?;
        let net = pairing
            .net()
            .ok_or("on a linked engine, but not connected to any now")?;
        Ok((net, addr))
    })
}

/// Opens track `id` on the linked engine `key` at the `quality` wanted over
/// the connection: Opus if the owner encodes it, else its first range and
/// size. Tells how it comes as `from` (its name, and where to tell).
async fn connect(
    net: &Net,
    key: &str,
    addr: &net::PeerAddr,
    id: i64,
    locate: Locate,
    quality: &(dyn Fn(&Connection) -> Quality + Sync),
    (from, tell): (String, watch::Sender<Option<Streamed>>),
) -> Result<Dialled, String> {
    let conn = net.connect(key, addr, Alpn::Files).await?;
    let quality = quality(&conn);
    let started = match quality {
        Quality::Original => None,
        _ => ask(&conn, id, Duration::ZERO, quality).await?,
    };
    let link = Link {
        conn: Mutex::new(conn.clone()),
        redial: tokio::sync::Mutex::new(None),
        key: key.into(),
        locate,
        id,
        runtime: Handle::current(),
        from,
        sent: match started {
            Some(_) => quality,
            None => Quality::Original,
        },
        streamed: tell,
    };
    link.follow(&conn);
    if let Some(started) = started {
        let first = Arc::new(Mutex::new(Some(started)));
        return Ok(Dialled::Opus(Arc::new(link), quality, first));
    }
    let (len, first) = fetch(&conn, id, 0, CHUNK as u32).await?;
    let remote = Remote {
        link,
        len,
        state: Mutex::new(Fetched {
            chunks: HashMap::from([(0, Chunk::Here(first.into()))]),
            wanted: 0..0,
            fetches: 0,
        }),
        arrived: Condvar::new(),
    };
    Ok(Dialled::Original(Arc::new(remote)))
}

/// A connection to the owner of track `id`, dialled again when it closes.
struct Link {
    conn: Mutex<Connection>,
    /// Held while one request dials again for all of them; the last failure
    /// to, which the others share for [`DIAL`].
    redial: tokio::sync::Mutex<Option<(String, Instant)>>,
    key: String,
    locate: Locate,
    id: i64,
    runtime: Handle,
    /// The owner's name, what it sends and where to say so.
    from: String,
    sent: Quality,
    streamed: watch::Sender<Option<Streamed>>,
}

/// A connection to the owner of track `id` and the ranges fetched or
/// being fetched.
struct Remote {
    link: Link,
    /// The file's size when it was opened.
    len: u64,
    state: Mutex<Fetched>,
    arrived: Condvar,
}

struct Fetched {
    /// By index.
    chunks: HashMap<u64, Chunk>,
    /// The chunks to fetch, in order.
    wanted: std::ops::Range<u64>,
    /// Fetches started, numbering each [`Chunk::Coming`].
    fetches: u64,
}

enum Chunk {
    /// Being fetched by fetch number `n`; a later one replaces it.
    Coming(u64),
    Here(Arc<[u8]>),
    /// Read as an error, and fetched again if read again.
    Failed(String),
}

/// How often a stream's route is told at most, as it changes.
const ROUTE_EVERY: Duration = Duration::from_secs(3);

impl Link {
    /// Tells how the track comes over `conn` now, and as that changes while
    /// it is open and the track is still wanted.
    fn follow(&self, conn: &Connection) {
        let (from, sent) = (self.from.clone(), self.sent);
        let streamed = move |route| {
            Some(Streamed {
                from: from.clone(),
                sent,
                route,
            })
        };
        self.streamed.send_replace(streamed(net::route(conn)));
        let (conn, tell) = (conn.clone(), self.streamed.clone());
        self.runtime.spawn(async move {
            let told = net::follow_route(&conn, ROUTE_EVERY, |route| {
                let now = streamed(route);
                tell.send_if_modified(|was| (*was != now).then(|| *was = now).is_some());
            });
            tokio::select! {
                _ = told => {}
                _ = tell.closed() => {}
            }
        });
    }

    fn conn(&self) -> Connection {
        self.conn.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// `ask` on the connection, asked again on a new one if this one closed
    /// (the owner restarted or closed it) so a track that started plays on.
    /// One that stalls on a live connection fails: a silent network change
    /// shows so, as the connection stays open until it times out.
    async fn retry<T, F: Future<Output = Result<T, String>>>(
        &self,
        ask: impl Fn(Connection) -> F,
    ) -> Result<T, String> {
        let conn = self.conn();
        match ask(conn.clone()).await {
            Err(_) if conn.close_reason().is_some() => ask(self.reconnect(&conn).await?).await,
            asked => asked,
        }
    }

    /// A connection in place of `lost`: dialled once for every request that
    /// lost it.
    async fn reconnect(&self, lost: &Connection) -> Result<Connection, String> {
        let mut failed = self.redial.lock().await;
        let now = self.conn();
        if now.stable_id() != lost.stable_id() {
            return Ok(now);
        }
        if let Some((err, at)) = &*failed
            && at.elapsed() < DIAL
        {
            return Err(err.clone());
        }
        let dialled = async {
            let (net, addr) = (self.locate)()?;
            let dialled = net.connect(&self.key, &addr, Alpn::Files);
            let conn = tokio::time::timeout(DIAL, dialled)
                .await
                .map_err(|_| "the linked engine stopped answering".to_string())??;
            Ok::<_, String>(conn)
        };
        match dialled.await {
            Ok(conn) => {
                *self.conn.lock().unwrap_or_else(|e| e.into_inner()) = conn.clone();
                self.follow(&conn);
                *failed = None;
                Ok(conn)
            }
            Err(err) => {
                *failed = Some((err.clone(), Instant::now()));
                Err(err)
            }
        }
    }
}

impl Remote {
    /// Chunk `i` ([`Link::retry`]).
    async fn fetch(&self, i: u64) -> Result<(u64, Vec<u8>), String> {
        let id = self.link.id;
        let fetched =
            |conn: Connection| async move { fetch(&conn, id, i * CHUNK, CHUNK as u32).await };
        self.link.retry(fetched).await
    }

    fn state(&self) -> std::sync::MutexGuard<'_, Fetched> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wants chunks `at ..= at + ahead`, keeping those from `at - 1` on and
    /// dropping the rest but the first and last, which every decoder probing
    /// the file reads, and those still coming (they hold the link). Failed
    /// ones wanted again are fetched again.
    fn want(self: &Arc<Self>, at: u64, ahead: u64) {
        let end = self.len.div_ceil(CHUNK);
        let keep = at.saturating_sub(1)..=at + AHEAD;
        let mut state = self.state();
        let wanted = at..(at + ahead + 1).min(end);
        state.chunks.retain(|&i, chunk| match chunk {
            Chunk::Coming(_) => true,
            Chunk::Failed(_) if wanted.contains(&i) => false,
            _ => keep.contains(&i) || i == 0 || i + 1 == end,
        });
        state.wanted = wanted;
        self.fill(&mut state);
    }

    /// Fetches the first wanted chunks not here, [`IN_FLIGHT`] at a time:
    /// more at once would share the link and make the next one late. The
    /// first, which the reader waits for, is always fetched.
    fn fill(self: &Arc<Self>, state: &mut Fetched) {
        let coming = |state: &Fetched| {
            let chunks = state.chunks.values();
            chunks.filter(|c| matches!(c, Chunk::Coming(_))).count()
        };
        for i in state.wanted.clone() {
            if i > state.wanted.start && coming(state) >= IN_FLIGHT {
                return;
            }
            if state.chunks.contains_key(&i) {
                continue;
            }
            state.fetches += 1;
            let n = state.fetches;
            state.chunks.insert(i, Chunk::Coming(n));
            let remote = self.clone();
            self.link.runtime.spawn(async move {
                let fetched = remote.fetch(i).await;
                let mut state = remote.state();
                if let Some(chunk @ Chunk::Coming(_)) = state.chunks.get_mut(&i)
                    && matches!(chunk, Chunk::Coming(m) if *m == n)
                {
                    *chunk = match fetched {
                        // Bytes of another file would be spliced into this one.
                        Ok((size, _)) if size != remote.len => {
                            Chunk::Failed("the file changed on the linked engine".into())
                        }
                        Ok((_, bytes)) => Chunk::Here(bytes.into()),
                        Err(err) => Chunk::Failed(err),
                    };
                }
                remote.fill(&mut state);
                remote.arrived.notify_all();
            });
        }
    }

    /// Chunk `i` once it is here. A lost link is an error, never a wait
    /// longer than a dial and a [`STALL`] (and never an end of file, which
    /// would end the track as if it were complete).
    fn chunk(&self, i: u64) -> io::Result<Arc<[u8]>> {
        let deadline = Instant::now() + DIAL + STALL + Duration::from_secs(1);
        let mut state = self.state();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match state.chunks.get(&i) {
                Some(Chunk::Here(bytes)) => return Ok(bytes.clone()),
                Some(Chunk::Failed(err)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        err.clone(),
                    ));
                }
                Some(Chunk::Coming(_)) if !left.is_zero() => {
                    state = self
                        .arrived
                        .wait_timeout(state, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                _ => return Err(io::ErrorKind::TimedOut.into()),
            }
        }
    }
}

/// Reads a linked engine's track as a file: from the chunks of its
/// [`Remote`], fetching ahead while read straight through.
struct Reader {
    remote: Arc<Remote>,
    len: u64,
    pos: u64,
    /// The chunk read last.
    last: u64,
}

impl Reader {
    fn new(remote: Arc<Remote>, len: u64) -> Self {
        Self {
            remote,
            len,
            pos: 0,
            last: 0,
        }
    }
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let at = self.pos / CHUNK;
        // A jump (a seek probing the file) fetches only what it reads.
        let ahead = if at == self.last || at == self.last + 1 {
            AHEAD
        } else {
            0
        };
        self.last = at;
        self.remote.want(at, ahead);
        let chunk = self.remote.chunk(at)?;
        let from = (self.pos - at * CHUNK) as usize;
        let Some(bytes) = chunk.get(from..).filter(|b| !b.is_empty()) else {
            return Err(io::Error::other("the linked engine sent a short range"));
        };
        let n = bytes.len().min(buf.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Drop for Reader {
    /// Stops reading ahead for it (another reader of the track may want on).
    fn drop(&mut self) {
        self.remote.state().wanted = 0..0;
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(pos) => Some(pos),
            SeekFrom::End(by) => self.len.checked_add_signed(by),
            SeekFrom::Current(by) => self.pos.checked_add_signed(by),
        };
        self.pos = pos.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

impl ByteSource for Reader {
    fn byte_len(&self) -> u64 {
        self.len
    }
}

/// The start of a track encoded live: what comes, then its packets.
struct Started {
    channels: u8,
    pre_skip: u16,
    from: Duration,
    recv: RecvStream,
    /// What the stream was asked on.
    conn: Connection,
}

/// Asks for track `id` over `conn` as Opus from `from`: `None` if the owner
/// sends this one as it is.
async fn ask(
    conn: &Connection,
    id: i64,
    from: Duration,
    quality: Quality,
) -> Result<Option<Started>, String> {
    let asked = async {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        let from_ms = from.as_millis() as u64;
        write(
            &mut send,
            &Opus {
                id,
                from_ms,
                quality,
            },
        )
        .await?;
        send.finish().map_err(|e| e.to_string())?;
        let sent: Result<Sent, String> = read(&mut recv).await?;
        Ok(match sent? {
            Sent::Original => None,
            Sent::Opus {
                channels,
                pre_skip,
                from_ms,
                ..
            } => Some(Started {
                channels,
                pre_skip,
                from: Duration::from_millis(from_ms),
                recv,
                conn: conn.clone(),
            }),
        })
    };
    tokio::time::timeout(STALL, asked)
        .await
        .unwrap_or_else(|_| Err("the linked engine stopped sending".into()))
}

/// The next packet on `recv` and its frames of audio, or `None` at the end.
async fn packet(recv: &mut RecvStream) -> Result<Option<(u16, Vec<u8>)>, String> {
    let read = async {
        let mut head = [0; 4];
        match recv.read_exact(&mut head).await {
            Ok(()) => {}
            Err(net::ReadExactError::FinishedEarly(0)) => return Ok(None),
            Err(err) => return Err(err.to_string()),
        }
        let mut data = vec![0; u16::from_be_bytes([head[0], head[1]]).into()];
        recv.read_exact(&mut data)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some((u16::from_be_bytes([head[2], head[3]]), data)))
    };
    tokio::time::timeout(STALL, read)
        .await
        .unwrap_or_else(|_| Err("the linked engine stopped sending".into()))
}

/// A linked engine's track, encoded to Opus there as it is decoded here. A
/// seek asks for it again from there.
struct Live {
    link: Arc<Link>,
    quality: Quality,
    started: Started,
    decoder: opus::Decoder,
    /// Frames still to drop from the start.
    skip: usize,
    /// Frames of audio given out since [`Started::from`].
    frames: u64,
    buffer: Vec<f32>,
}

impl Live {
    /// The track from `from`.
    fn open(link: Arc<Link>, quality: Quality, from: Duration) -> io::Result<Self> {
        let id = link.id;
        let asked = |conn: Connection| async move { ask(&conn, id, from, quality).await };
        let started = link.runtime.block_on(link.retry(asked));
        match started.map_err(io::Error::other)? {
            Some(started) => Self::new(link, quality, started),
            // Bytes of another file would follow on from this one's.
            None => Err(io::Error::other("the file changed on the linked engine")),
        }
    }

    fn new(link: Arc<Link>, quality: Quality, started: Started) -> io::Result<Self> {
        let layout = match started.channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            n => return Err(io::Error::other(format!("{n}-channel Opus"))),
        };
        let decoder = opus::Decoder::new(transcode::RATE, layout).map_err(io::Error::other)?;
        Ok(Self {
            link,
            quality,
            skip: started.pre_skip.into(),
            buffer: vec![0.0; transcode::FRAME * usize::from(started.channels)],
            started,
            decoder,
            frames: 0,
        })
    }

    fn position(&self) -> Duration {
        let rate = f64::from(transcode::RATE);
        self.started.from + Duration::from_secs_f64(self.frames as f64 / rate)
    }
}

impl Decoder for Live {
    fn sample_rate(&self) -> u32 {
        transcode::RATE
    }

    fn channels(&self) -> usize {
        self.started.channels.into()
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        let runtime = self.link.runtime.clone();
        let packet = match runtime.block_on(packet(&mut self.started.recv)) {
            // Its connection closed (the owner restarted or dropped it): on
            // from here over a new one.
            Err(_) if self.started.conn.close_reason().is_some() => {
                let at = self.position();
                *self =
                    Self::open(self.link.clone(), self.quality, at).map_err(|e| e.to_string())?;
                runtime.block_on(packet(&mut self.started.recv))?
            }
            packet => packet?,
        };
        let Some((frames, data)) = packet else {
            return Ok(false);
        };
        let decoded = self
            .decoder
            .decode_float(&data, &mut self.buffer, false)
            .map_err(|e| e.to_string())?;
        let end = decoded.min(frames.into());
        let start = self.skip.min(end);
        self.skip -= start;
        let channels = self.channels();
        out.extend_from_slice(&self.buffer[start * channels..end * channels]);
        self.frames += (end - start) as u64;
        Ok(true)
    }

    fn seek(&mut self, to: Duration) -> Result<Duration, String> {
        *self = Self::open(self.link.clone(), self.quality, to).map_err(|e| e.to_string())?;
        Ok(self.started.from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    /// `seconds` of 16-bit stereo noise at 44.1 kHz (CD bitrate, 1411 kb/s).
    fn wav(seconds: u32) -> Vec<u8> {
        let mut state = 1u32;
        let data: Vec<u8> = (0..seconds * 44_100 * 4)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8 / 8
            })
            .collect();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for field in [
            16u32.to_le_bytes(),
            [1, 0, 2, 0],
            44_100u32.to_le_bytes(),
            (44_100u32 * 4).to_le_bytes(),
        ] {
            wav.extend_from_slice(&field);
        }
        wav.extend_from_slice(&[4, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    /// A library holding `files` (name, bytes), and its folder.
    async fn library(name: &str, files: &[(&str, Vec<u8>)]) -> (Library, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("motif-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        for (file, bytes) in files {
            std::fs::write(music.join(file), bytes).unwrap();
        }
        let library = Library::open(&dir).unwrap();
        let path = music.to_string_lossy().into_owned();
        library
            .query(protocol::LibraryQuery::AddRoot { path })
            .unwrap();
        let mut changes = library.subscribe();
        let want = files.len() as u32;
        tokio::time::timeout(
            Duration::from_secs(20),
            changes.wait_for(|s| s.tracks >= want && !s.scanning),
        )
        .await
        .expect("scan timed out")
        .unwrap();
        (library, dir)
    }

    /// The owner's ids for its tracks, oldest first.
    fn ids(library: &Library) -> Vec<i64> {
        let page = library.catalog_page(&Default::default(), 100).unwrap();
        page.changes.iter().map(|c| c.id).collect()
    }

    /// Engine `secret`'s endpoint, linked with everyone (the gate is net's).
    async fn endpoint(secret: u8) -> Net {
        Net::bind([secret; 32], 0, &net::Relays::Off, Arc::new(|_| true))
            .await
            .unwrap()
    }

    fn loopback(net: &Net) -> net::PeerAddr {
        let port = net.port().unwrap();
        net::PeerAddr {
            direct: vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)],
            relay: None,
        }
    }

    /// The owner (serving `library` at `pace`) and the reader, connected.
    async fn pair(library: Library, pace: Option<Pace>) -> (Net, Net) {
        pair_while(library, pace, Arc::default()).await
    }

    /// [`pair`], the reader linked unless `unlinked`.
    async fn pair_while(
        library: Library,
        pace: Option<Pace>,
        unlinked: Arc<std::sync::atomic::AtomicBool>,
    ) -> (Net, Net) {
        let (owner, reader) = (endpoint(1).await, endpoint(2).await);
        let serving = owner.clone();
        let linked: net::Linked =
            Arc::new(move |_| !unlinked.load(std::sync::atomic::Ordering::Relaxed));
        tokio::spawn(async move {
            while let Some((alpn, conn)) = serving.accept().await {
                assert_eq!(alpn, Alpn::Files);
                tokio::spawn(serve(library.clone(), conn, linked.clone(), pace));
            }
        });
        (owner, reader)
    }

    /// Reads `len` bytes at `at`.
    fn read_at(r: &mut Reader, at: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut bytes = vec![0; len];
        r.seek(SeekFrom::Start(at))?;
        r.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_engine_unlinked_while_reading_gets_no_more() {
        let (library, dir) = library("unlinked", &[("a.wav", wav(5))]).await;
        let id = ids(&library)[0];
        let unlinked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (owner, reader) = pair_while(library, None, unlinked.clone()).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        unlinked.store(true, std::sync::atomic::Ordering::Relaxed);
        let (_, read) = blocking(stream, |r| read_at(r, 10 * CHUNK, 10)).await;
        let err = read.expect_err("read after unlinking");
        assert!(err.to_string().contains("not linked"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_range_is_asked_for_again() {
        let file = wav(5);
        let (library, dir) = library("again", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        let path = dir.join("music/a.wav");
        let moved = dir.join("a.wav");
        std::fs::rename(&path, &moved).unwrap();
        let (stream, read) = blocking(stream, |r| read_at(r, 10 * CHUNK, 10)).await;
        read.expect_err("read while the file is away");
        std::fs::rename(&moved, &path).unwrap();
        let (_, read) = blocking(stream, |r| read_at(r, 10 * CHUNK, 10)).await;
        let at = (10 * CHUNK) as usize;
        assert_eq!(read.unwrap(), file[at..at + 10]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_changed_on_its_owner_fails_rather_than_mixing() {
        let (library, dir) = library("changed", &[("a.wav", wav(5))]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        std::fs::write(dir.join("music/a.wav"), wav(6)).unwrap();
        let (_, read) = blocking(stream, |r| read_at(r, 10 * CHUNK, 10)).await;
        let err = read.expect_err("read of a changed file");
        assert!(err.to_string().contains("changed"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn open(owner: &Net, reader: &Net, id: i64) -> Result<Reader, String> {
        let (reader, at) = (reader.clone(), loopback(owner));
        open_with(
            owner,
            Arc::new(move || Ok((reader.clone(), at.clone()))),
            id,
        )
        .await
    }

    /// [`open`], redialling where `locate` says.
    async fn open_with(owner: &Net, locate: Locate, id: i64) -> Result<Reader, String> {
        match dialled(owner, locate, id, Quality::Original).await? {
            Dialled::Original(remote) => Ok(Reader::new(remote.clone(), remote.len)),
            Dialled::Opus(..) => panic!("encoded on the LAN"),
        }
    }

    /// Track `id` at `quality`, as off the LAN.
    async fn dialled(
        owner: &Net,
        locate: Locate,
        id: i64,
        quality: Quality,
    ) -> Result<Dialled, String> {
        let (net, at) = locate()?;
        let from = ("Owner".into(), watch::channel(None).0);
        connect(&net, &owner.key(), &at, id, locate, &|_| quality, from).await
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_track_redials_only_a_still_linked_engine_and_only_once() {
        let file = wav(5);
        let (library, dir) = library("redial", &[("a.wav", file)]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        // Unlinked since it opened.
        let linked = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (net, at, still) = (reader.clone(), loopback(&owner), linked.clone());
        let locate: Locate =
            Arc::new(
                move || match still.load(std::sync::atomic::Ordering::Relaxed) {
                    true => Ok((net.clone(), at.clone())),
                    false => Err("no longer linked".into()),
                },
            );
        let stream = open_with(&owner, locate, id).await.unwrap();
        linked.store(false, std::sync::atomic::Ordering::Relaxed);
        stream.remote.link.conn().close(0u32.into(), b"gone");
        let (_, read) = blocking(stream, |r| read_at(r, 10 * CHUNK, 10)).await;
        let err = read.expect_err("redialled an engine no longer linked");
        assert!(err.to_string().contains("no longer linked"), "{err}");

        // Gone for good: the redial fails once, not once per read.
        let stream = open(&owner, &reader, id).await.unwrap();
        owner.close().await;
        stream.remote.link.conn().close(0u32.into(), b"gone");
        let (stream, read) = blocking(stream, |r| read_at(r, 10 * CHUNK, 10)).await;
        read.expect_err("read from a closed engine");
        let started = Instant::now();
        let (_, read) = blocking(stream, |r| read_at(r, 20 * CHUNK, 10)).await;
        read.expect_err("read from a closed engine");
        assert!(started.elapsed() < Duration::from_secs(1), "dialled again");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Runs `f` with `reader` off the runtime, as the decode thread does.
    async fn blocking<T: Send + 'static>(
        mut reader: Reader,
        f: impl FnOnce(&mut Reader) -> T + Send + 'static,
    ) -> (Reader, T) {
        tokio::task::spawn_blocking(move || {
            let out = f(&mut reader);
            (reader, out)
        })
        .await
        .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_linked_engines_track_reads_and_seeks_as_its_file() {
        let file = wav(3);
        let (library, dir) = library("read", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        assert_eq!(stream.byte_len(), file.len() as u64);
        let expected = file.clone();
        let (stream, all) = blocking(stream, |r| {
            let mut all = Vec::new();
            r.read_to_end(&mut all).map(|_| all)
        })
        .await;
        assert!(all.unwrap() == expected, "read differs from the file");
        // Seeks: a jump into the middle, then from the end.
        let (_, read) = blocking(stream, |r| {
            let mut at = vec![0; 100_000];
            r.seek(SeekFrom::Start(300_000))?;
            r.read_exact(&mut at)?;
            let mut end = vec![0; 10];
            r.seek(SeekFrom::End(-10))?;
            r.read_exact(&mut end)?;
            io::Result::Ok((at, end))
        })
        .await;
        let (at, end) = read.unwrap();
        assert!(at == file[300_000..400_000], "seeked read differs");
        assert_eq!(end, file[file.len() - 10..]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_seek_back_while_reading_ahead_still_reads() {
        let file = wav(10);
        let (library, dir) = library("back", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        // Every range answers late, so reads ahead stay in flight.
        let pace = Pace {
            rate: 1 << 30,
            latency: Duration::from_millis(500),
        };
        let (owner, reader) = pair(library, Some(pace)).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        let (_, read) = blocking(stream, |r| {
            let mut byte = [0; 1];
            r.seek(SeekFrom::Start(10 * CHUNK))?;
            r.read_exact(&mut byte)?;
            // Straight on: reads ahead start.
            r.read_exact(&mut byte)?;
            r.seek(SeekFrom::Start(0))?;
            r.read_exact(&mut byte)?;
            io::Result::Ok(byte[0])
        })
        .await;
        assert_eq!(read.unwrap(), file[0]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_the_owners_own_present_tracks_are_served() {
        let (library, dir) = library("refuse", &[("a.wav", wav(1))]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        let unknown = open(&owner, &reader, id + 1000).await.err().unwrap();
        assert!(unknown.contains("not on this engine"), "{unknown}");
        std::fs::remove_dir_all(&dir).unwrap();
        let gone = open(&owner, &reader, id).await.err().unwrap();
        assert!(!gone.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_lost_link_mid_track_is_an_error_not_a_hang_or_an_end() {
        let (library, dir) = library("lost", &[("a.wav", wav(20))]).await;
        let id = ids(&library)[0];
        // Slow enough that most of the track is still to come.
        let pace = Pace {
            rate: 200_000,
            latency: Duration::ZERO,
        };
        let (owner, reader) = pair(library, Some(pace)).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        let (stream, first) = blocking(stream, |r| r.read_exact(&mut [0; 1000])).await;
        first.unwrap();
        owner.close().await;
        let started = Instant::now();
        let (_, rest) = blocking(stream, |r| r.read_to_end(&mut Vec::new())).await;
        let err = rest.expect_err("read past a lost link");
        assert_ne!(
            err.kind(),
            io::ErrorKind::UnexpectedEof,
            "would end the track"
        );
        assert!(started.elapsed() < DIAL + STALL + Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_track_reads_on_when_its_connection_closes() {
        let file = wav(5);
        let (library, dir) = library("reconnect", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        let (stream, first) = blocking(stream, |r| read_at(r, 0, 10)).await;
        assert_eq!(first.unwrap(), file[..10]);
        // As when the owner restarts or drops it.
        stream.remote.link.conn().close(0u32.into(), b"gone");
        let (_, read) = blocking(stream, |r| read_at(r, 10 * CHUNK, 10)).await;
        let at = (10 * CHUNK) as usize;
        assert_eq!(read.unwrap(), file[at..at + 10]);
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn until(what: &str, done: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(20), async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out: {what}"));
    }

    /// What `library` lists, as queue references.
    fn tracks(library: &Library) -> Vec<protocol::TrackRef> {
        let query = protocol::LibraryQuery::Tracks {
            search: None,
            sort: vec![],
            filter: Default::default(),
            offset: 0,
            limit: 10,
        };
        let protocol::QueryResult::Tracks { tracks, .. } = library.query(query).unwrap() else {
            panic!()
        };
        tracks.into_iter().map(|t| t.track).collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_engine_plays_and_seeks_a_track_only_a_linked_engine_holds() {
        use engine::{Playable, Resolver, audio::decoder};

        let (a, b) = (Pairing::in_memory(), Pairing::in_memory());
        for p in [&a, &b] {
            p.command(protocol::PairingCommand::SetInternet { allowed: false })
                .unwrap();
        }
        let (library_a, dir_a) = library("owner", &[("a.wav", wav(3))]).await;
        let dir_b = std::env::temp_dir().join(format!("motif-files-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir_b);
        let library_b = Library::open(&dir_b).unwrap();
        library_a.set_device(a.engine_key()).unwrap();
        library_b.set_device(b.engine_key()).unwrap();
        b.link("A", a.engine_key(), &[]).unwrap();
        tokio::spawn(crate::links::run(b.clone(), 0, Some(library_b.clone())));
        until("B bound", || b.iroh_port().is_some()).await;
        let at_b = format!("127.0.0.1:{}", b.iroh_port().unwrap());
        a.link("B", b.engine_key(), &[at_b]).unwrap();
        tokio::spawn(crate::links::run(a.clone(), 0, Some(library_a.clone())));
        until("A's catalog on B", || tracks(&library_b).len() == 1).await;
        let track = tracks(&library_b).remove(0);
        assert!(track.id.starts_with("remote:"), "{}", track.id);

        let resolver = crate::LibraryResolver {
            library: Some(library_b.clone()),
            pairing: Some(b.clone()),
        };
        let Ok(Playable::File(source, _)) = resolver.resolve(&track) else {
            panic!("not resolved");
        };
        let Source::Stream { streamed, .. } = &source else {
            panic!("not a stream");
        };
        let streamed = streamed.clone();
        // On the decode thread: every frame, then a seek.
        let (frames, reached) = tokio::task::spawn_blocking(move || {
            let mut decoder = decoder::open(&source, &decoder::all())?;
            let mut samples = Vec::new();
            while decoder.decode(&mut samples)? {}
            let frames = samples.len() / decoder.channels();
            let reached = decoder.seek(Duration::from_millis(1500))?;
            samples.clear();
            decoder.decode(&mut samples)?;
            Ok::<_, String>((frames, reached))
        })
        .await
        .unwrap()
        .unwrap();
        // On the LAN (loopback here), the original: a WAV at its own rate.
        assert_eq!(frames, 3 * 44_100);
        let lan = protocol::Streamed {
            from: "A".into(),
            sent: Quality::Original,
            route: protocol::Route::Lan,
        };
        assert_eq!(*streamed.borrow(), Some(lan));
        assert!(reached.abs_diff(Duration::from_millis(1500)) < Duration::from_millis(100));

        // Unlinked, it no longer resolves.
        let device = b.subscribe().borrow().devices[0].id.clone();
        b.command(protocol::PairingCommand::Revoke { device })
            .unwrap();
        assert!(resolver.resolve(&track).is_err());
        for dir in [dir_a, dir_b] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Three seconds of a stereo tone in the format ffmpeg's `args` make,
    /// as `ext`.
    fn tone(ext: &str, args: &[&str]) -> Vec<u8> {
        static MADE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("motif-tone-{n}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("tone.{ext}"));
        let ffmpeg = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
            .args(["sine=frequency=440:duration=3", "-ac", "2"])
            .args(args)
            .arg(&path)
            .status()
            .expect("ffmpeg is needed for this test");
        assert!(ffmpeg.success());
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        bytes
    }

    /// The owner's id for its file `name`.
    fn id_of(library: &Library, name: &str) -> i64 {
        let ids = ids(library);
        let named = |&&id: &&i64| library.own_file(id).is_some_and(|f| f.path.ends_with(name));
        *ids.iter().find(named).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn off_the_lan_a_flac_comes_as_opus_and_an_mp3_192_as_it_is() {
        let files = [
            ("a.flac", tone("flac", &[])),
            ("b.mp3", tone("mp3", &["-b:a", "192k"])),
        ];
        let (library, dir) = library("opus", &files).await;
        let (flac, mp3) = (id_of(&library, "a.flac"), id_of(&library, "b.mp3"));
        let (owner, reader) = pair(library, None).await;
        let (net, at) = (reader.clone(), loopback(&owner));
        let locate: Locate = Arc::new(move || Ok((net.clone(), at.clone())));
        let opened = dialled(&owner, locate.clone(), mp3, Quality::Opus256).await;
        assert!(
            matches!(opened, Ok(Dialled::Original(_))),
            "the MP3 encoded again"
        );
        let opened = dialled(&owner, locate, flac, Quality::Opus256).await;
        let Ok(dialled @ Dialled::Opus(..)) = opened else {
            panic!("the FLAC not encoded");
        };
        let streamed = dialled.link().streamed.borrow().clone();
        assert_eq!(streamed.map(|s| s.sent), Some(Quality::Opus256));
        let decoded = tokio::task::spawn_blocking(move || {
            let Ok(Opened::Decoded(mut decoder)) = dialled.open() else {
                panic!("not decoded");
            };
            let mut samples = Vec::new();
            while decoder.decode(&mut samples)? {}
            let frames = samples.len() / decoder.channels();
            // Mid-stream: the owner encodes again from there.
            let reached = decoder.seek(Duration::from_millis(1500))?;
            samples.clear();
            while decoder.decode(&mut samples)? {}
            let after = samples.len() / decoder.channels();
            Ok::<_, String>((decoder.sample_rate(), frames, reached, after))
        })
        .await
        .unwrap();
        let (rate, frames, reached, after) = decoded.unwrap();
        assert_eq!(rate, 48_000);
        assert!(frames.abs_diff(3 * 48_000) <= 2, "{frames} frames");
        // FLAC seeks to the start of a block.
        let near = reached.abs_diff(Duration::from_millis(1500)) < Duration::from_millis(100);
        assert!(near, "reached {reached:?}");
        let left = (Duration::from_secs(3) - reached).as_secs_f64() * 48_000.0;
        // Where it reached comes in whole milliseconds.
        assert!(
            (after as f64 - left).abs() < 50.0,
            "{after} frames after the seek"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_encoded_copy_carries_its_sources_audio_hash() {
        let (library, dir) = library("hash", &[("a.flac", tone("flac", &[]))]).await;
        let id = ids(&library)[0];
        // Hashed after the scan.
        until("hashed", || {
            library.own_file(id).unwrap().audio_hash.is_some()
        })
        .await;
        let hash = library.own_file(id).unwrap().audio_hash;
        let (owner, reader) = pair(library, None).await;
        let conn = reader
            .connect(&owner.key(), &loopback(&owner), Alpn::Files)
            .await
            .unwrap();
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        let quality = Quality::Opus128;
        write(
            &mut send,
            &Opus {
                id,
                from_ms: 0,
                quality,
            },
        )
        .await
        .unwrap();
        let sent: Result<Sent, String> = read(&mut recv).await.unwrap();
        let Ok(Sent::Opus { audio_hash, .. }) = sent else {
            panic!("{sent:?}");
        };
        assert_eq!(audio_hash, hash);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A stream of the track `remote` opened.
    fn opened(remote: Arc<Remote>, len: u64) -> Source {
        Source::Stream {
            name: "measured".into(),
            open: Arc::new(move || Ok(Opened::Bytes(Box::new(Reader::new(remote.clone(), len))))),
            streamed: watch::channel(None).1,
        }
    }

    /// What playing over a paced link was like.
    #[derive(Debug, Default)]
    struct Played {
        /// From asking to the decoder having opened it (dialled, probed).
        open: Duration,
        /// From asking to the audio buffer first being full (playback starts).
        start: Duration,
        /// Opening the next track (on a thread of its own while the current
        /// one plays, as the decode thread does).
        preload: Duration,
        underruns: u32,
        /// Audio time the device went without.
        starved: f64,
        /// From a seek to the buffer being full again.
        seek: Duration,
    }

    type Open = Arc<dyn Fn(i64) -> Box<dyn engine::audio::decoder::Decoder> + Send + Sync>;

    /// The audio output's ring buffer: half a second (audio/mod.rs).
    const BUFFER: f64 = 0.5;

    /// Plays `first` to its end and `then` for 5 s, as the decode thread
    /// would into a [`BUFFER`] the device plays in real time, then seeks.
    fn play(open: Open, first: i64, then: i64) -> Played {
        let mut played = Played::default();
        let asked = Instant::now();
        let mut decoder = open(first);
        played.open = asked.elapsed();
        let (mut decoded, mut heard, mut since) = (0.0f64, 0.0f64, None::<Instant>);
        let mut next = None;
        let mut samples = Vec::new();
        loop {
            let now = Instant::now();
            if let Some(at) = since {
                let wanted = heard + (now - at).as_secs_f64();
                if wanted > decoded {
                    played.underruns += 1;
                    played.starved += wanted - decoded;
                    (heard, since) = (decoded, Some(now));
                } else if decoded - wanted > BUFFER {
                    std::thread::sleep(Duration::from_secs_f64(decoded - wanted - BUFFER));
                }
            } else if decoded >= BUFFER {
                played.start = asked.elapsed();
                since = Some(Instant::now());
                let open = open.clone();
                next = Some(std::thread::spawn(move || {
                    let opening = Instant::now();
                    (open(then), opening.elapsed())
                }));
            }
            samples.clear();
            let more = decoder.decode(&mut samples).unwrap();
            decoded +=
                samples.len() as f64 / decoder.channels() as f64 / decoder.sample_rate() as f64;
            if !more {
                match next.take() {
                    Some(gapless) => (decoder, played.preload) = gapless.join().unwrap(),
                    None => break,
                }
            }
            if next.is_none() && since.is_some() && decoded > 25.0 {
                break;
            }
        }
        let mut decoder = open(first);
        let seeking = Instant::now();
        decoder.seek(Duration::from_secs(10)).unwrap();
        let mut buffered = 0.0;
        while buffered < BUFFER {
            samples.clear();
            decoder.decode(&mut samples).unwrap();
            buffered +=
                samples.len() as f64 / decoder.channels() as f64 / decoder.sample_rate() as f64;
        }
        played.seek = seeking.elapsed();
        played
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "measures in real time (~2 min): cargo test -p server slow_link -- --ignored --nocapture"]
    async fn slow_link_buffering() {
        // 20 s tracks at CD bitrate (1411 kb/s): more than CD-quality FLAC
        // (~900 kb/s), about 24-bit/48 kHz FLAC.
        let (library, dir) = library("slow", &[("1.wav", wav(20)), ("2.wav", wav(20))]).await;
        let ids = ids(&library);
        println!("| link | open | start | preload open | seek | underruns | starved |");
        println!("|---|---|---|---|---|---|---|");
        for (kbps, latency) in [
            (100_000, 1),
            (8_000, 20),
            (2_000, 50),
            (1_600, 50),
            (1_200, 50),
        ] {
            let pace = Pace {
                rate: kbps * 1000 / 8,
                latency: Duration::from_millis(latency),
            };
            let (owner, reader) = pair(library.clone(), Some(pace)).await;
            let (key, addr, runtime) = (owner.key(), loopback(&owner), Handle::current());
            let ids = ids.clone();
            let played = tokio::task::spawn_blocking(move || {
                let open: Open = Arc::new(move |id| {
                    let (net, at) = (reader.clone(), addr.clone());
                    let locate: Locate = Arc::new(move || Ok((net.clone(), at.clone())));
                    let connected = runtime.block_on(connect(
                        &reader,
                        &key,
                        &addr,
                        id,
                        locate,
                        &|_| Quality::Original,
                        ("Owner".into(), watch::channel(None).0),
                    ));
                    let Ok(Dialled::Original(remote)) = connected else {
                        panic!("not opened");
                    };
                    let len = remote.len;
                    engine::audio::decoder::open(
                        &opened(remote, len),
                        &engine::audio::decoder::all(),
                    )
                    .unwrap()
                });
                play(open, ids[0], ids[1])
            })
            .await
            .unwrap();
            println!(
                "| {:.1} Mb/s, {latency} ms | {:.2} s | {:.2} s | {:.2} s | {:.2} s | {} | {:.2} s |",
                kbps as f64 / 1000.0,
                played.open.as_secs_f64(),
                played.start.as_secs_f64(),
                played.preload.as_secs_f64(),
                played.seek.as_secs_f64(),
                played.underruns,
                played.starved,
            );
            owner.close().await;
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
