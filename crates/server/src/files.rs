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
//! after the last. A seek past what came asks again from there. Files that
//! would lose more than they save come as they are
//! ([`library::transcode::plan`]), as ranges.
//!
//! Either way the reader fetches the whole track into a file of the
//! [`Store`] (#282): playback rides out a lost link once the bytes are here,
//! and a seek into them asks for nothing. Downloads (#245) fetch through it
//! too ([`fetch_whole`]).

use std::{
    collections::HashMap,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use engine::{ByteSource, Opened, Source, StreamState, audio::decoder::Decoder};
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
/// How often a download tells how far it is.
const TELL: Duration = Duration::from_millis(250);

/// A slower link, for measuring: `rate` bytes a second shared by every range
/// on the connection, each answered `latency` late. Notes the offset of each
/// range asked for in `served`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Pace {
    pub rate: u64,
    pub latency: Duration,
    pub served: Arc<Mutex<Vec<u64>>>,
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
        let (library, free, pace) = (library.clone(), free.clone(), pace.clone());
        let allowed = linked(&peer);
        tokio::spawn(async move {
            let mut recv = recv;
            let asked = tokio::time::timeout(STALL, read::<Ask>(&mut recv)).await;
            let done = match asked {
                Ok(Ok(Ask::Range(range))) => {
                    if let Some(pace) = &pace {
                        pace.served
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(range.offset);
                    }
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
            // Being fetched for another stream of it, or a download.
            if open.is_none() {
                let store = pairing.store();
                let quality = pairing.stream_quality();
                let live = match (store.live(&copies), store.live_encoded(&copies, quality)) {
                    (Some(remote), _) => Some(Dialled::Original(remote)),
                    (None, Some(packets)) => {
                        Some(Dialled::Opus(packets.link.clone(), quality, packets))
                    }
                    (None, None) => None,
                };
                *open = live.map(|live| (Ok(live), Instant::now()));
            }
            let dialled = match &*open {
                Some((Ok(dialled), _)) if dialled.link().conn().close_reason().is_none() => {
                    dialled.clone()
                }
                Some((Err(err), at)) if at.elapsed() < DIAL => {
                    return Err(io::Error::other(err.clone()));
                }
                _ => {
                    let dialled = dial(&pairing, &copies, tell.clone(), None);
                    let dialled = runtime.block_on(dialled);
                    if dialled.is_err() {
                        let from = pairing.linked_name(&copies[0].0).unwrap_or_default();
                        tell.send_replace(Some(StreamState::Unreachable(from)));
                    }
                    let (dialled, _) = open.insert((dialled, Instant::now()));
                    dialled.clone().map_err(io::Error::other)?
                }
            };
            dialled.open()
        }),
    }
}

/// A linked engine's track, connected to: as ranges of its file, or as Opus
/// encoded live, whose packets every decoder of it reads.
#[derive(Clone)]
enum Dialled {
    Original(Arc<Remote>),
    Opus(Arc<Link>, Quality, Arc<Packets>),
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
            Dialled::Opus(link, quality, packets) => {
                let live = Live::new(link.clone(), *quality, packets.clone())?;
                Opened::Decoded(Box::new(live))
            }
        })
    }
}

/// Connects to the first engine in `copies` that answers with the start of
/// its copy.
async fn dial(
    pairing: &Pairing,
    copies: &[(String, i64)],
    tell: watch::Sender<Option<StreamState>>,
    download: Option<Quality>,
) -> Result<Dialled, String> {
    let net = pairing
        .net()
        .ok_or("on a linked engine, but not connected to any now")?;
    let mut copies = copies.to_vec();
    copies.sort_by_key(|(key, _)| pairing.reach(key) == Reach::Unreachable);
    let quality = |conn: &Connection| match (download, net::lan(conn)) {
        (Some(quality), _) => quality,
        (None, true) => Quality::Original,
        (None, false) => pairing.stream_quality(),
    };
    let mut reasons = Vec::new();
    for (key, id) in copies {
        let addr = pairing.linked().into_iter().find(|(k, _)| *k == key);
        let Some((_, addr)) = addr else { continue };
        let locate = locate(pairing.clone(), key.clone());
        let from = (pairing.linked_name(&key).unwrap_or_default(), tell.clone());
        let store = pairing.store();
        let connected = connect(&store, &net, (&key, &addr), id, locate, &quality, from);
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

/// Fetches the track the linked engines in `copies` hold whole into the
/// [`Store`], kept, at `quality` (#245): its file there, and whether it came
/// encoded (an Ogg Opus file with `tags`). One being fetched for playing is
/// shared. Safe to repeat: ranges carry on from what is here; an encoded
/// copy starts again. Tells the share of it here (0 to 1) as it comes, every
/// [`TELL`] at most; an encoded one only knowing its `length`.
pub(crate) async fn fetch_whole(
    pairing: &Pairing,
    copies: &[(String, i64)],
    quality: Quality,
    tags: &[(String, String)],
    length: Option<Duration>,
    mut tell: impl FnMut(f32) + Send + 'static,
) -> Result<(PathBuf, bool), String> {
    let store = pairing.store();
    // Being fetched for playing (or another download): fetched once.
    let dialled = match (quality, store.live(copies)) {
        (Quality::Original, Some(remote)) => Dialled::Original(remote),
        _ => match store.live_encoded(copies, quality) {
            Some(packets) => Dialled::Opus(packets.link.clone(), quality, packets),
            None => dial(pairing, copies, watch::channel(None).0, Some(quality)).await?,
        },
    };
    match dialled {
        Dialled::Original(remote) => {
            remote.kept.store(true, Ordering::Relaxed);
            remote.fetched(tell).await?;
            Ok((store.path(&remote.link.key, remote.link.id), false))
        }
        Dialled::Opus(link, _, packets) => {
            let path = store.path(&link.key, link.id).with_extension("opus");
            let (tags, written) = (tags.to_vec(), path.clone());
            // As the packets come (waiting for them blocks).
            tokio::task::spawn_blocking(move || {
                let stored = |err: io::Error| match err.kind() {
                    io::ErrorKind::StorageFull => "the disk is full".to_string(),
                    _ => format!("can't store it here: {err}"),
                };
                let file = std::fs::File::create(&written).map_err(stored)?;
                let file = io::BufWriter::new(file);
                let (channels, pre_skip) = (packets.channels, packets.pre_skip);
                let mut ogg =
                    transcode::OggOpus::new(file, channels, pre_skip, &tags).map_err(stored)?;
                let (mut i, mut frames, mut told) = (0, 0, Instant::now());
                while let Some(packet) = packets.packet(i)? {
                    ogg.packet(&packet.data, packet.frames).map_err(stored)?;
                    i += 1;
                    frames += u64::from(packet.frames);
                    if let Some(length) = length.filter(|_| told.elapsed() >= TELL) {
                        told = Instant::now();
                        tell((frames as f64 / 48_000.0 / length.as_secs_f64()).min(1.0) as f32);
                    }
                }
                ogg.finish().map_err(stored)?;
                Ok::<_, String>(())
            })
            .await
            .map_err(|e| e.to_string())??;
            Ok((path, true))
        }
    }
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
    store: &Store,
    net: &Net,
    (key, addr): (&str, &net::PeerAddr),
    id: i64,
    locate: Locate,
    quality: &(dyn Fn(&Connection) -> Quality + Sync),
    (from, tell): (String, watch::Sender<Option<StreamState>>),
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
        store: store.clone(),
    };
    link.follow(&conn);
    if let Some(started) = started {
        let link = Arc::new(link);
        let packets = Packets::fetch(link.clone(), started).map_err(|e| e.to_string())?;
        return Ok(Dialled::Opus(link, quality, packets));
    }
    let (len, first) = fetch(&conn, id, 0, CHUNK as u32).await?;
    let remote = store.adopt(Arc::new(link), len, &first);
    Ok(Dialled::Original(remote.map_err(|e| e.to_string())?))
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
    streamed: watch::Sender<Option<StreamState>>,
    /// Where what it fetches goes.
    store: Store,
}

/// Whole files of linked engines' tracks, fetched here (#282, #245): one
/// per track, shared by every stream of it. A track's file goes once
/// nothing plays it (the engine holds only the current and next tracks),
/// unless it is kept for a download, which moves it out when it is whole.
/// The bytes of each file, and a byte per chunk saying whether it is here,
/// so an interrupted fetch resumes where it was.
#[derive(Clone)]
pub(crate) struct Store(Arc<Stored>);

struct Stored {
    dir: PathBuf,
    open: Mutex<HashMap<(String, i64), Weak<Remote>>>,
    /// The files of [`Packets`] being played.
    live: Mutex<std::collections::HashSet<PathBuf>>,
    /// Tracks encoded live from their start, by track, shared like `open`.
    encoded: Mutex<HashMap<(String, i64), Weak<Packets>>>,
    /// By engine: one track encoded for this one at a time, as encoding
    /// faster than real time is a burst of work for it (it may be a phone).
    encoding: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl Store {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self(Arc::new(Stored {
            dir,
            open: Mutex::default(),
            live: Mutex::default(),
            encoded: Mutex::default(),
            encoding: Mutex::default(),
        }))
    }

    fn encoded(&self) -> std::sync::MutexGuard<'_, HashMap<(String, i64), Weak<Packets>>> {
        self.0.encoded.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The first of `copies` encoded live at `quality` from its start now.
    fn live_encoded(&self, copies: &[(String, i64)], quality: Quality) -> Option<Arc<Packets>> {
        let encoded = self.encoded();
        let live = |copy| encoded.get(copy).and_then(Weak::upgrade);
        copies
            .iter()
            .filter_map(live)
            .find(|packets| packets.link.sent == quality)
    }

    /// Engine `key`'s turn to encode a track for this one.
    fn encoder(&self, key: &str) -> Arc<tokio::sync::Semaphore> {
        let mut encoding = self.0.encoding.lock().unwrap_or_else(|e| e.into_inner());
        let slot = encoding.entry(key.to_string());
        slot.or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }

    fn live_files(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<PathBuf>> {
        self.0.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, i64), Weak<Remote>>> {
        self.0.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Where linked engine `key`'s track `id` is fetched to.
    fn path(&self, key: &str, id: i64) -> PathBuf {
        self.0.dir.join(format!("{key}-{id}"))
    }

    /// Deletes what isn't being fetched for playing, nor in `keep` (the
    /// downloads still to make), as left by an engine that stopped.
    pub(crate) fn prune(&self, keep: &[(String, i64)]) {
        let Ok(dir) = std::fs::read_dir(&self.0.dir) else {
            return;
        };
        let open = self.lock();
        let live = self.live_files();
        for file in dir.flatten() {
            let name = file.file_name();
            let stem = name.to_string_lossy();
            let stem = stem.split('.').next().unwrap_or_default();
            let track = stem
                .rsplit_once('-')
                .and_then(|(key, id)| Some((key.to_string(), id.parse::<i64>().ok()?)));
            let wanted = track.is_some_and(|t| {
                keep.contains(&t) || open.get(&t).and_then(Weak::upgrade).is_some()
            });
            if !wanted && !live.contains(&file.path()) {
                let _ = std::fs::remove_file(file.path());
            }
        }
    }

    /// Done with a file [`fetch_whole`] fetched in the original: deleted, or,
    /// while it is still open for playing, once that is done with it.
    pub(crate) fn release(&self, fetched: &Path) {
        let stem = fetched.file_name().unwrap_or_default().to_string_lossy();
        let Some((key, id)) = stem.rsplit_once('-') else {
            return;
        };
        let Ok(id) = id.parse::<i64>() else {
            return;
        };
        let open = self.lock();
        let live = open.get(&(key.to_string(), id)).and_then(Weak::upgrade);
        if live.is_none() {
            let _ = std::fs::remove_file(fetched.with_extension("have"));
            let _ = std::fs::remove_file(fetched);
        }
        drop(open);
        // Dropped outside the lock: the last one deletes the file, taking it.
        if let Some(remote) = live {
            remote.kept.store(false, Ordering::Relaxed);
        }
    }

    /// The first of `copies` being fetched now.
    fn live(&self, copies: &[(String, i64)]) -> Option<Arc<Remote>> {
        let open = self.lock();
        copies
            .iter()
            .find_map(|copy| open.get(copy).and_then(Weak::upgrade))
    }

    /// Track `id` of `link`'s engine, `len` bytes long and starting with
    /// `first`: the one being fetched already, else one fetched to its file
    /// here, carrying on from what that holds.
    fn adopt(&self, link: Arc<Link>, len: u64, first: &[u8]) -> io::Result<Arc<Remote>> {
        let key = (link.key.clone(), link.id);
        let mut open = self.lock();
        if let Some(remote) = open.get(&key).and_then(Weak::upgrade)
            && remote.len == len
        {
            return Ok(remote);
        }
        std::fs::create_dir_all(&self.0.dir)?;
        let path = self.path(&key.0, key.1);
        let rw = || {
            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            options
        };
        let data = rw().open(&path)?;
        let mut marks = rw().open(path.with_extension("have"))?;
        let chunks = len.div_ceil(CHUNK) as usize;
        let mut have = Vec::with_capacity(chunks);
        let resumed = data.metadata()?.len() == len
            && marks.metadata()?.len() == chunks as u64
            && marks.read_to_end(&mut have).is_ok_and(|n| n == chunks);
        if !resumed {
            // Of another size: another file, or none.
            data.set_len(0)?;
            data.set_len(len)?;
            marks.set_len(0)?;
            marks.set_len(chunks as u64)?;
            have = vec![0; chunks];
        }
        let remote = Arc::new(Remote {
            link,
            len,
            store: self.clone(),
            files: Mutex::new((data, marks)),
            kept: AtomicBool::new(false),
            state: Mutex::new(Fetched {
                have: have.into_iter().map(|b| b == 1).collect(),
                chunks: HashMap::new(),
                at: 0,
                fetches: 0,
            }),
            arrived: Condvar::new(),
        });
        if chunks > 0 && !remote.state().have[0] {
            remote.write(0, first).map_err(io::Error::other)?;
            remote.state().have[0] = true;
        }
        open.insert(key, Arc::downgrade(&remote));
        remote.fill(&mut remote.state());
        Ok(remote)
    }
}

/// A connection to the owner of track `id` and its file in the [`Store`],
/// fetched whole: the chunk the reader is at first, then on to the end and
/// round from the start, [`IN_FLIGHT`] at a time.
struct Remote {
    link: Arc<Link>,
    /// The file's size when it was opened.
    len: u64,
    store: Store,
    /// The file and its chunks' marks.
    files: Mutex<(std::fs::File, std::fs::File)>,
    /// Kept when nothing plays it (a download's).
    kept: AtomicBool,
    state: Mutex<Fetched>,
    arrived: Condvar,
}

struct Fetched {
    /// Which chunks are in the file.
    have: Vec<bool>,
    /// Chunks on their way or failed, by index.
    chunks: HashMap<u64, Chunk>,
    /// The chunk the reader is at.
    at: u64,
    /// Fetches started, numbering each [`Chunk::Coming`].
    fetches: u64,
}

enum Chunk {
    /// Being fetched by fetch number `n`; a later one replaces it.
    Coming(u64, tokio::task::AbortHandle),
    /// Read as an error, and fetched again if read again or once another
    /// chunk arrives.
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
            Some(StreamState::Streams(Streamed {
                from: from.clone(),
                sent,
                route,
            }))
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
                let from = self.from.clone();
                self.streamed
                    .send_replace(Some(StreamState::Unreachable(from)));
                Err(err)
            }
        }
    }
}

impl Remote {
    fn state(&self) -> std::sync::MutexGuard<'_, Fetched> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether every chunk is here.
    #[cfg(test)]
    fn whole(&self) -> bool {
        self.state().have.iter().all(|&h| h)
    }

    /// Once every chunk is here, fetching what failed again a few times;
    /// telling the share here every [`TELL`] meanwhile.
    async fn fetched(self: &Arc<Self>, mut tell: impl FnMut(f32)) -> Result<(), String> {
        let mut tries = 0;
        loop {
            {
                let mut state = self.state();
                let here = state.have.iter().filter(|&&h| h).count();
                if here == state.have.len() {
                    return Ok(());
                }
                tell(here as f32 / state.have.len() as f32);
                let coming = (state.chunks.values()).any(|c| matches!(c, Chunk::Coming(..)));
                if !coming {
                    let failed = state.chunks.values().find_map(|c| match c {
                        Chunk::Failed(err) => Some(err.clone()),
                        Chunk::Coming(..) => None,
                    });
                    if let Some(err) = failed {
                        tries += 1;
                        if tries > 3 {
                            return Err(err);
                        }
                    }
                    state.chunks.clear();
                    self.fill(&mut state);
                }
            }
            tokio::time::sleep(TELL).await;
        }
    }

    /// The reader is at chunk `at`: it is fetched now, then the rest from
    /// there. After a jump (a seek), chunks still coming that aren't needed
    /// soon are dropped, as they would share the link with it; they are
    /// fetched again later. A failed one read again is fetched again.
    fn want(self: &Arc<Self>, at: u64, jumped: bool) {
        let mut state = self.state();
        let failed = matches!(state.chunks.get(&at), Some(Chunk::Failed(_)));
        // Within the chunk it was at: what it started still stands.
        if at == state.at && !jumped && !failed {
            return;
        }
        state.at = at;
        if failed {
            state.chunks.remove(&at);
        }
        if jumped {
            let soon = at..at + IN_FLIGHT as u64;
            state.chunks.retain(|i, chunk| match chunk {
                Chunk::Coming(_, task) if !soon.contains(i) => {
                    task.abort();
                    false
                }
                _ => true,
            });
        }
        self.fill(&mut state);
    }

    /// Fetches the next chunks not here from where the reader is,
    /// [`IN_FLIGHT`] at a time: more at once would share the link and make
    /// the next one late. The one the reader waits for is always fetched.
    /// Fetches hold the track only weakly, so they stop once nothing plays it.
    fn fill(self: &Arc<Self>, state: &mut Fetched) {
        let end = state.have.len() as u64;
        let mut coming = (state.chunks.values())
            .filter(|c| matches!(c, Chunk::Coming(..)))
            .count();
        for i in (state.at..end).chain(0..state.at) {
            if i != state.at && coming >= IN_FLIGHT {
                return;
            }
            if state.have[i as usize] || state.chunks.contains_key(&i) {
                continue;
            }
            state.fetches += 1;
            let n = state.fetches;
            let (link, track) = (self.link.clone(), Arc::downgrade(self));
            let task = self.link.runtime.spawn(async move {
                let id = link.id;
                let fetched = |conn: Connection| async move {
                    fetch(&conn, id, i * CHUNK, CHUNK as u32).await
                };
                let fetched = link.retry(fetched).await;
                if let Some(remote) = track.upgrade() {
                    remote.arrive(i, n, fetched);
                }
            });
            state
                .chunks
                .insert(i, Chunk::Coming(n, task.abort_handle()));
            coming += 1;
        }
    }

    /// Stores chunk `i` from fetch `n`, unless a later fetch replaced it.
    fn arrive(self: &Arc<Self>, i: u64, n: u64, fetched: Result<(u64, Vec<u8>), String>) {
        let mut state = self.state();
        if !matches!(state.chunks.get(&i), Some(Chunk::Coming(m, _)) if *m == n) {
            return;
        }
        let size = (self.len - i * CHUNK).min(CHUNK) as usize;
        let stored = match fetched {
            // Bytes of another file would be spliced into this one.
            Ok((len, _)) if len != self.len => Err("the file changed on the linked engine".into()),
            Ok((_, bytes)) if bytes.len() != size => {
                Err("the linked engine sent a short range".into())
            }
            Ok((_, bytes)) => self.write(i, &bytes),
            Err(err) => Err(err),
        };
        match stored {
            Ok(()) => {
                state.have[i as usize] = true;
                // The link works: what failed is tried again.
                state
                    .chunks
                    .retain(|&j, c| j != i && matches!(c, Chunk::Coming(..)));
            }
            Err(err) => {
                state.chunks.insert(i, Chunk::Failed(err));
            }
        }
        self.fill(&mut state);
        self.arrived.notify_all();
    }

    /// Writes chunk `i` to the file, then marks it there.
    fn write(&self, i: u64, bytes: &[u8]) -> Result<(), String> {
        let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        let (data, marks) = &mut *files;
        let written = data
            .seek(SeekFrom::Start(i * CHUNK))
            .and_then(|_| data.write_all(bytes))
            .and_then(|_| marks.seek(SeekFrom::Start(i)))
            .and_then(|_| marks.write_all(&[1]));
        written.map_err(|err| match err.kind() {
            io::ErrorKind::StorageFull => "the disk is full".into(),
            _ => format!("can't store it here: {err}"),
        })
    }

    /// Waits for chunk `i`. A lost link is an error, never a wait longer
    /// than a dial and a [`STALL`] (and never an end of file, which would
    /// end the track as if it were complete).
    fn ready(&self, i: u64) -> io::Result<()> {
        let deadline = Instant::now() + DIAL + STALL + Duration::from_secs(1);
        let mut state = self.state();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if state.have[i as usize] {
                return Ok(());
            }
            match state.chunks.get(&i) {
                Some(Chunk::Failed(err)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        err.clone(),
                    ));
                }
                Some(Chunk::Coming(..)) if !left.is_zero() => {
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

    /// Reads from `pos` into `buf`, as far as the end of its chunk, once
    /// that is here.
    fn read_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        let i = pos / CHUNK;
        self.ready(i)?;
        let end = ((i + 1) * CHUNK).min(self.len);
        let n = buf.len().min((end - pos) as usize);
        let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        files.0.seek(SeekFrom::Start(pos))?;
        files.0.read_exact(&mut buf[..n])?;
        Ok(n)
    }
}

impl Drop for Remote {
    /// Deletes the file, unless it is kept or the track was opened again
    /// since (it carries on with what is here).
    fn drop(&mut self) {
        if self.kept.load(Ordering::Relaxed) {
            return;
        }
        let mut open = self.store.lock();
        let key = (self.link.key.clone(), self.link.id);
        if open.get(&key).and_then(Weak::upgrade).is_none() {
            open.remove(&key);
            let path = self.store.path(&key.0, key.1);
            let _ = std::fs::remove_file(path.with_extension("have"));
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Reads a linked engine's track as a file: from its [`Remote`]'s file.
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
        // Every decoder probing the file reads its first and last chunks:
        // not a seek, so what it reads straight through after goes on.
        let probe = at == 0 || at + 1 == self.len.div_ceil(CHUNK);
        let jumped = !probe && at != self.last && at != self.last + 1;
        if !probe {
            self.last = at;
        }
        self.remote.want(at, jumped);
        let n = self.remote.read_at(self.pos, buf)?;
        self.pos += n as u64;
        Ok(n)
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

/// A linked engine's track encoded live, fetched whole (#282): a task takes
/// its packets as fast as the owner encodes them, into a file of the
/// [`Store`] and an index, and decoders read them from there, so playback
/// rides out a lost link once they are here and a seek into them asks for
/// nothing. Deleted once nothing plays it.
struct Packets {
    link: Arc<Link>,
    channels: u8,
    pre_skip: u16,
    /// Where the packets start in the track.
    from: Duration,
    /// What they were asked on.
    conn: Connection,
    path: PathBuf,
    file: Mutex<std::fs::File>,
    got: Mutex<Got>,
    arrived: Condvar,
}

#[derive(Default)]
struct Got {
    /// Each packet's place in the file, its length and the frames before
    /// it (pre-skip included).
    index: Vec<(u64, u16, u64)>,
    /// Frames through the last packet.
    frames: u64,
    /// The end, or why they stopped coming.
    end: Option<Result<(), String>>,
    /// Coming: its turn to be encoded came.
    started: bool,
}

/// One packet.
struct Packet {
    data: Vec<u8>,
    /// Frames of audio in it ([`transcode::Packet::frames`]).
    frames: u16,
}

impl Packets {
    /// Takes the packets of `started` into a new file of `store`.
    fn fetch(link: Arc<Link>, started: Started) -> io::Result<Arc<Self>> {
        static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = MADE.fetch_add(1, Ordering::Relaxed);
        let store = &link.store;
        std::fs::create_dir_all(&store.0.dir)?;
        let path = store
            .path(&link.key, link.id)
            .with_extension(format!("{n}.live"));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        store.live_files().insert(path.clone());
        let Started {
            channels,
            pre_skip,
            from,
            mut recv,
            conn,
        } = started;
        let packets = Arc::new(Self {
            link: link.clone(),
            channels,
            pre_skip,
            from,
            conn,
            path,
            file: Mutex::new(file),
            got: Mutex::default(),
            arrived: Condvar::new(),
        });
        let held = Arc::downgrade(&packets);
        if from.is_zero() {
            store
                .encoded()
                .insert((link.key.clone(), link.id), held.clone());
        }
        // A seek's packets come at once: the track is the one playing.
        let turn = from.is_zero().then(|| store.encoder(&link.key));
        link.runtime.spawn(async move {
            let _turn = match turn {
                Some(turn) => Some(turn.acquire_owned().await),
                None => None,
            };
            match held.upgrade() {
                Some(packets) => packets.got().started = true,
                None => return,
            }
            loop {
                let next = packet(&mut recv).await;
                let Some(packets) = held.upgrade() else {
                    return;
                };
                let end = match next {
                    Ok(Some((frames, data))) => match packets.store(frames, &data) {
                        Ok(()) => continue,
                        Err(err) => Err(err),
                    },
                    Ok(None) => Ok(()),
                    Err(err) => Err(err),
                };
                packets.got().end = Some(end);
                packets.arrived.notify_all();
                return;
            }
        });
        Ok(packets)
    }

    fn got(&self) -> std::sync::MutexGuard<'_, Got> {
        self.got.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn store(&self, frames: u16, data: &[u8]) -> Result<(), String> {
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let at = file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        file.write_all(data).map_err(|err| match err.kind() {
            io::ErrorKind::StorageFull => "the disk is full".to_string(),
            _ => format!("can't store it here: {err}"),
        })?;
        drop(file);
        let mut got = self.got();
        let before = got.frames;
        got.index.push((at, data.len() as u16, before));
        got.frames += u64::from(frames);
        drop(got);
        self.arrived.notify_all();
        Ok(())
    }

    /// Packet `i` once it is here; `None` after the last. Packets that
    /// stopped coming are an error, as a lost link is to [`Remote::ready`].
    fn packet(&self, i: usize) -> Result<Option<Packet>, String> {
        let wait = DIAL + STALL + Duration::from_secs(1);
        let mut deadline = Instant::now() + wait;
        let mut got = self.got();
        loop {
            // Waiting for its turn isn't a lost link.
            if !got.started {
                deadline = Instant::now() + wait;
            }
            if let Some(&(at, len, before)) = got.index.get(i) {
                let frames = got.index.get(i + 1).map_or(got.frames, |next| next.2) - before;
                drop(got);
                let mut data = vec![0; len.into()];
                let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
                file.seek(SeekFrom::Start(at))
                    .and_then(|_| file.read_exact(&mut data))
                    .map_err(|e| e.to_string())?;
                return Ok(Some(Packet {
                    data,
                    frames: frames as u16,
                }));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match &got.end {
                Some(Ok(())) => return Ok(None),
                Some(Err(err)) => return Err(err.clone()),
                None if left.is_zero() => return Err("the linked engine stopped sending".into()),
                None => {
                    got = (self.arrived.wait_timeout(got, left))
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
            }
        }
    }

    /// The frames before packet `i`, if it is here.
    fn start(&self, i: usize) -> Option<u64> {
        self.got().index.get(i).map(|p| p.2)
    }

    /// The packet holding frame `frame` (pre-skip included) if it is here,
    /// and the frames before it.
    fn find(&self, frame: u64) -> Option<(usize, u64)> {
        let got = self.got();
        if frame >= got.frames {
            return None;
        }
        let i = got.index.partition_point(|&(_, _, before)| before <= frame) - 1;
        Some((i, got.index[i].2))
    }
}

impl Drop for Packets {
    fn drop(&mut self) {
        let store = &self.link.store;
        let mut encoded = store.encoded();
        let key = (self.link.key.clone(), self.link.id);
        if encoded.get(&key).is_some_and(|p| p.upgrade().is_none()) {
            encoded.remove(&key);
        }
        drop(encoded);
        store.live_files().remove(&self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Packets decoded again before a seek's: the Opus decoder needs 80 ms to
/// settle.
const PREROLL: usize = 4;

/// A linked engine's track, encoded to Opus there as it is decoded here,
/// from its [`Packets`]. A seek into those here decodes from there; one
/// past them asks for it again from that point.
struct Live {
    link: Arc<Link>,
    quality: Quality,
    packets: Arc<Packets>,
    /// The packet decoded next.
    next: usize,
    decoder: opus::Decoder,
    /// Frames still to drop before the audio given out.
    skip: usize,
    /// Frames of audio given out since [`Packets::from`].
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
            Some(started) => {
                let packets = Packets::fetch(link.clone(), started)?;
                Self::new(link, quality, packets)
            }
            // Bytes of another file would follow on from this one's.
            None => Err(io::Error::other("the file changed on the linked engine")),
        }
    }

    fn new(link: Arc<Link>, quality: Quality, packets: Arc<Packets>) -> io::Result<Self> {
        Ok(Self {
            link,
            quality,
            skip: packets.pre_skip.into(),
            buffer: vec![0.0; transcode::FRAME * usize::from(packets.channels)],
            decoder: Self::decoder(packets.channels)?,
            packets,
            next: 0,
            frames: 0,
        })
    }

    fn decoder(channels: u8) -> io::Result<opus::Decoder> {
        let layout = match channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            n => return Err(io::Error::other(format!("{n}-channel Opus"))),
        };
        opus::Decoder::new(transcode::RATE, layout).map_err(io::Error::other)
    }

    fn position(&self) -> Duration {
        let rate = f64::from(transcode::RATE);
        self.packets.from + Duration::from_secs_f64(self.frames as f64 / rate)
    }

    /// Decodes from the packet before `to` that is here, if it is: where
    /// that leaves it.
    fn seek_here(&mut self, to: Duration) -> Option<Duration> {
        let packets = &self.packets;
        let rate = f64::from(transcode::RATE);
        let pre_skip = u64::from(packets.pre_skip);
        let audio = (to.checked_sub(packets.from)?.as_secs_f64() * rate) as u64;
        let (at, start) = packets.find(audio + pre_skip)?;
        let begin = at.saturating_sub(PREROLL);
        let begun = packets.start(begin)?;
        let decoder = Self::decoder(packets.channels).ok()?;
        // Out from the start of that packet (never in the pre-skip).
        let out = start.max(pre_skip);
        self.decoder = decoder;
        self.next = begin;
        self.skip = (out - begun) as usize;
        self.frames = out - pre_skip;
        Some(self.position())
    }
}

impl Decoder for Live {
    fn sample_rate(&self) -> u32 {
        transcode::RATE
    }

    fn channels(&self) -> usize {
        self.packets.channels.into()
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        let packet = match self.packets.packet(self.next) {
            // Its connection closed (the owner restarted or dropped it): on
            // from here over a new one.
            Err(_) if self.packets.conn.close_reason().is_some() => {
                let at = self.position();
                *self =
                    Self::open(self.link.clone(), self.quality, at).map_err(|e| e.to_string())?;
                self.packets.packet(self.next)?
            }
            packet => packet?,
        };
        let Some(Packet { data, frames }) = packet else {
            return Ok(false);
        };
        self.next += 1;
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
        if let Some(reached) = self.seek_here(to) {
            return Ok(reached);
        }
        *self = Self::open(self.link.clone(), self.quality, to).map_err(|e| e.to_string())?;
        Ok(self.packets.from)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    /// `seconds` of 16-bit stereo noise at 44.1 kHz (CD bitrate, 1411 kb/s).
    pub(crate) fn wav(seconds: u32) -> Vec<u8> {
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

    /// A store of its own.
    fn test_store() -> Store {
        static MADE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = MADE.fetch_add(1, Ordering::Relaxed);
        let dir = format!("motif-store-{n}-{}", std::process::id());
        Store::new(std::env::temp_dir().join(dir))
    }

    /// A library holding `files` (name, bytes), and its folder.
    pub(crate) async fn library(
        name: &str,
        files: &[(&str, Vec<u8>)],
    ) -> (Library, std::path::PathBuf) {
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
    pub(crate) fn ids(library: &Library) -> Vec<i64> {
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
                tokio::spawn(serve(library.clone(), conn, linked.clone(), pace.clone()));
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
        // Slow, so the rest of it is still to come after the change.
        let pace = Pace {
            rate: 1 << 30,
            latency: Duration::from_millis(500),
            ..Default::default()
        };
        let (owner, reader) = pair(library, Some(pace)).await;
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
        open_in(&test_store(), owner, locate, id).await
    }

    /// [`open_with`], fetched into `store`.
    async fn open_in(
        store: &Store,
        owner: &Net,
        locate: Locate,
        id: i64,
    ) -> Result<Reader, String> {
        match dialled_in(store, owner, locate, id, Quality::Original).await? {
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
        dialled_in(&test_store(), owner, locate, id, quality).await
    }

    async fn dialled_in(
        store: &Store,
        owner: &Net,
        locate: Locate,
        id: i64,
        quality: Quality,
    ) -> Result<Dialled, String> {
        let (net, at) = locate()?;
        let from = ("Owner".into(), watch::channel(None).0);
        connect(
            store,
            &net,
            (&owner.key(), &at),
            id,
            locate,
            &|_| quality,
            from,
        )
        .await
    }

    /// Where `reader` dials `owner`.
    fn locate_at(owner: &Net, reader: &Net) -> Locate {
        let (net, at) = (reader.clone(), loopback(owner));
        Arc::new(move || Ok((net.clone(), at.clone())))
    }

    async fn whole(remote: &Arc<Remote>) {
        let remote = remote.clone();
        until("fetched whole", move || remote.whole()).await;
    }

    /// The files in `store` (without their marks).
    fn stored(store: &Store) -> usize {
        let Ok(dir) = std::fs::read_dir(&store.0.dir) else {
            return 0;
        };
        dir.filter(|f| {
            let path = f.as_ref().unwrap().path();
            path.extension().is_none()
        })
        .count()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_track_plays_to_its_end_through_an_outage_once_its_bytes_are_here() {
        // 3.5 MB: more than the old 2 MiB read-ahead.
        let file = wav(20);
        let (library, dir) = library("outage", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        whole(&stream.remote).await;
        owner.close().await;
        let (_, all) = blocking(stream, |r| {
            let mut all = Vec::new();
            r.read_to_end(&mut all).map(|_| all)
        })
        .await;
        assert!(all.unwrap() == file, "read differs from the file");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_next_track_is_whole_before_the_current_one_ends() {
        let files = [("1.wav", wav(4)), ("2.wav", wav(4))];
        let (library, dir) = library("next", &files).await;
        let (first, then) = (id_of(&library, "1.wav"), id_of(&library, "2.wav"));
        // 1 MB/s: under four times the two tracks' bitrate.
        let pace = Pace {
            rate: 1_000_000,
            latency: Duration::from_millis(20),
            ..Default::default()
        };
        let (owner, reader) = pair(library, Some(pace)).await;
        let current = open(&owner, &reader, first).await.unwrap();
        let next = open(&owner, &reader, then).await.unwrap();
        // Read as it plays: 4 s of CD audio.
        let (_, ended) = blocking(current, move |r| {
            let mut buf = vec![0; 44_100 * 4 / 10];
            loop {
                match r.read(&mut buf)? {
                    0 => return io::Result::Ok(next.remote.whole()),
                    _ => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        })
        .await;
        assert!(
            ended.unwrap(),
            "the next track wasn't whole when this one ended"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_seek_into_what_is_here_asks_for_nothing() {
        let file = wav(10);
        let (library, dir) = library("here", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        let pace = Pace {
            rate: 1 << 30,
            ..Default::default()
        };
        let served = pace.served.clone();
        let (owner, reader) = pair(library, Some(pace)).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        whole(&stream.remote).await;
        let asked = served.lock().unwrap().len();
        let (_, read) = blocking(stream, |r| {
            let mut bytes = Vec::new();
            for at in [20 * CHUNK, 0, 15 * CHUNK + 7] {
                bytes.push(read_at(r, at, 1000)?);
            }
            io::Result::Ok(bytes)
        })
        .await;
        let read = read.unwrap();
        assert!(read[2] == file[15 * CHUNK as usize + 7..][..1000]);
        assert_eq!(served.lock().unwrap().len(), asked, "asked the owner again");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_seek_past_what_is_here_is_fetched_before_any_read_ahead() {
        let file = wav(20);
        let (library, dir) = library("past", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        // Every range answers late, so reads ahead are still coming at the seek.
        let latency = Duration::from_secs(2);
        let pace = Pace {
            rate: 1 << 30,
            latency,
            ..Default::default()
        };
        let served = pace.served.clone();
        let (owner, reader) = pair(library, Some(pace)).await;
        let stream = open(&owner, &reader, id).await.unwrap();
        let (_, read) = blocking(stream, move |r| {
            read_at(r, 0, 10)?;
            let seeking = Instant::now();
            let read = read_at(r, 40 * CHUNK, 10)?;
            io::Result::Ok((read, seeking.elapsed()))
        })
        .await;
        let (read, took) = read.unwrap();
        assert_eq!(read, file[40 * CHUNK as usize..][..10]);
        // Not behind the reads ahead from the start.
        assert!(took < latency + Duration::from_secs(1), "took {took:?}");
        let served = served.lock().unwrap().clone();
        // Asked at once, so they may arrive in any order.
        let after = served.iter().find(|&&at| at >= 5 * CHUNK);
        let soon = 40 * CHUNK..(40 + IN_FLIGHT as u64) * CHUNK;
        assert!(after.is_some_and(|at| soon.contains(at)), "{served:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_store_holds_only_the_tracks_open_and_those_kept() {
        let files = [("1.wav", wav(2)), ("2.wav", wav(2)), ("3.wav", wav(2))];
        let (library, dir) = library("held", &files).await;
        let ids = ids(&library);
        let (owner, reader) = pair(library, None).await;
        let store = test_store();
        let mut open = Vec::new();
        for &id in &ids {
            open.push(
                open_in(&store, &owner, locate_at(&owner, &reader), id)
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(stored(&store), 3);
        // The current track ended: the next one and the one after it stay.
        drop(open.remove(0));
        assert_eq!(stored(&store), 2);
        // A download's stays when nothing plays it.
        open[0].remote.kept.store(true, Ordering::Relaxed);
        open.clear();
        assert_eq!(stored(&store), 1);
        let _ = std::fs::remove_dir_all(&store.0.dir);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_interrupted_fetch_resumes_and_is_safe_to_repeat() {
        let file = wav(10);
        let (library, dir) = library("resume", &[("a.wav", file.clone())]).await;
        let id = ids(&library)[0];
        // About 3 s for the whole file.
        let pace = Pace {
            rate: 600_000,
            ..Default::default()
        };
        let served = pace.served.clone();
        let (owner, reader) = pair(library, Some(pace)).await;
        let store = test_store();
        let stream = open_in(&store, &owner, locate_at(&owner, &reader), id)
            .await
            .unwrap();
        let chunks = file.len().div_ceil(CHUNK as usize);
        let remote = stream.remote.clone();
        let here = move || remote.state().have.iter().filter(|&&h| h).count();
        until("some of it here", || here() >= 10).await;
        assert!(here() < chunks, "fetched whole already");
        // Stopped, kept (as a download is), then opened again.
        stream.remote.kept.store(true, Ordering::Relaxed);
        drop((stream, here));
        let again = open_in(&store, &owner, locate_at(&owner, &reader), id)
            .await
            .unwrap();
        // Opened twice, fetched once.
        let twice = open_in(&store, &owner, locate_at(&owner, &reader), id)
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&again.remote, &twice.remote));
        whole(&again.remote).await;
        // Each chunk once, but those on their way when it stopped and the
        // first, which says the size on opening again.
        let asked = served.lock().unwrap().len();
        assert!(
            asked <= chunks + IN_FLIGHT + 2,
            "{asked} ranges for {chunks}"
        );
        let (_, all) = blocking(again, |r| {
            let mut all = Vec::new();
            r.read_to_end(&mut all).map(|_| all)
        })
        .await;
        assert!(all.unwrap() == file, "read differs from the file");
        let _ = std::fs::remove_dir_all(&store.0.dir);
        let _ = std::fs::remove_dir_all(dir);
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
            ..Default::default()
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
            ..Default::default()
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

    pub(crate) async fn until(what: &str, done: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(20), async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out: {what}"));
    }

    /// What `library` lists, as queue references.
    pub(crate) fn tracks(library: &Library) -> Vec<protocol::TrackRef> {
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
        tokio::spawn(crate::links::run(
            b.clone(),
            0,
            Some(library_b.clone()),
            None,
        ));
        until("B bound", || b.iroh_port().is_some()).await;
        let at_b = format!("127.0.0.1:{}", b.iroh_port().unwrap());
        a.link("B", b.engine_key(), &[at_b]).unwrap();
        tokio::spawn(crate::links::run(
            a.clone(),
            0,
            Some(library_a.clone()),
            None,
        ));
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
        // Named as the owner calls itself, which the link takes on meeting it.
        let lan = protocol::Streamed {
            from: a.engine_name(),
            sent: Quality::Original,
            route: protocol::Route::Lan,
        };
        assert_eq!(*streamed.borrow(), Some(StreamState::Streams(lan)));
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

    #[tokio::test(flavor = "multi_thread")]
    async fn a_track_whose_engine_cant_be_reached_says_so() {
        let pairing = Pairing::in_memory();
        let (owner, reader) = (endpoint(1).await, endpoint(2).await);
        let key = owner.key();
        // Gone: nothing answers where it was.
        let at = format!("127.0.0.1:{}", owner.port().unwrap());
        owner.close().await;
        pairing.link("Desktop", &key, &[at]).unwrap();
        pairing.set_net(Some(&reader));
        let source = source(pairing, vec![(key, 1)], Handle::current());
        let Source::Stream { open, streamed, .. } = &source else {
            panic!("not a stream");
        };
        let open = open.clone();
        let opened = tokio::task::spawn_blocking(move || open().map(|_| ()))
            .await
            .unwrap();
        assert!(opened.is_err());
        let said = streamed.borrow().clone();
        assert_eq!(said, Some(StreamState::Unreachable("Desktop".into())));
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
        let Some(StreamState::Streams(streamed)) = streamed else {
            panic!("{streamed:?}");
        };
        assert_eq!(streamed.sent, Quality::Opus256);
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
    async fn off_the_lan_a_track_plays_and_seeks_through_a_drop_mid_track_once_its_packets_are_here()
     {
        let (library, dir) = library("live", &[("a.flac", tone("flac", &[]))]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        let store = test_store();
        let opened = dialled_in(
            &store,
            &owner,
            locate_at(&owner, &reader),
            id,
            Quality::Opus256,
        );
        let Ok(dialled @ Dialled::Opus(..)) = opened.await else {
            panic!("not encoded");
        };
        let Dialled::Opus(_, _, packets) = &dialled else {
            unreachable!()
        };
        let here = packets.clone();
        until("all here", move || matches!(here.got().end, Some(Ok(())))).await;
        let file = packets.path.clone();
        let runtime = Handle::current();
        let decoded = tokio::task::spawn_blocking(move || {
            let Ok(Opened::Decoded(mut decoder)) = dialled.open() else {
                panic!("not decoded");
            };
            let mut samples = Vec::new();
            while samples.len() < 48_000 * 2 {
                decoder.decode(&mut samples)?;
            }
            // A second in, the link drops for good.
            runtime.block_on(owner.close());
            while decoder.decode(&mut samples)? {}
            let frames = samples.len() / decoder.channels();
            // Back into what is here: nothing to ask for, and nobody to ask.
            let reached = decoder.seek(Duration::from_millis(1500))?;
            samples.clear();
            while decoder.decode(&mut samples)? {}
            let after = samples.len() / decoder.channels();
            Ok::<_, String>((frames, reached, after))
        })
        .await
        .unwrap();
        let (frames, reached, after) = decoded.unwrap();
        assert!(frames.abs_diff(3 * 48_000) <= 2, "{frames} frames");
        let near = reached.abs_diff(Duration::from_millis(1500)) <= Duration::from_millis(20);
        assert!(near, "reached {reached:?}");
        let left = (Duration::from_secs(3) - reached).as_secs_f64() * 48_000.0;
        assert!(
            (after as f64 - left).abs() <= 2.0,
            "{after} frames after the seek"
        );
        // Gone with the stream.
        assert!(!file.exists(), "{} kept", file.display());
        let _ = std::fs::remove_dir_all(&store.0.dir);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Opus 256 of linked engine `owner`'s track `id`, fetched into `store`.
    async fn encoded(store: &Store, owner: &Net, reader: &Net, id: i64) -> (Dialled, Arc<Packets>) {
        let opened = dialled_in(store, owner, locate_at(owner, reader), id, Quality::Opus256);
        let dialled = opened.await.unwrap();
        let Dialled::Opus(_, _, packets) = &dialled else {
            panic!("not encoded");
        };
        let packets = packets.clone();
        (dialled, packets)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_engine_encodes_one_track_at_a_time_for_another() {
        // 23 s each, so encoding takes a while.
        let long = || tone("flac", &["-af", "apad=pad_dur=20"]);
        let files = [("1.flac", long()), ("2.flac", long())];
        let (library, dir) = library("turns", &files).await;
        let (first, then) = (id_of(&library, "1.flac"), id_of(&library, "2.flac"));
        let (owner, reader) = pair(library, None).await;
        let store = test_store();
        // The current track and the next, opened at once.
        let (_current, current) = encoded(&store, &owner, &reader, first).await;
        let (_next, next) = encoded(&store, &owner, &reader, then).await;
        while current.got().end.is_none() {
            assert!(!next.got().started, "both encoded at once");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let done = next.clone();
        until("the next encoded", move || {
            matches!(done.got().end, Some(Ok(())))
        })
        .await;
        let _ = std::fs::remove_dir_all(&store.0.dir);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_track_streamed_off_the_lan_downloads_without_being_fetched_again() {
        let (library, dir) = library("reuse", &[("a.flac", tone("flac", &[]))]).await;
        let id = ids(&library)[0];
        let (owner, reader) = pair(library, None).await;
        // Not connected to any engine: one dial would fail.
        let pairing = Pairing::in_memory();
        let store = pairing.store();
        let (_playing, packets) = encoded(&store, &owner, &reader, id).await;
        let copies = [(owner.key(), id)];
        let tags = [("TITLE".to_string(), "Tone".to_string())];
        let (path, encoded) = fetch_whole(&pairing, &copies, Quality::Opus256, &tags, None, |_| {})
            .await
            .unwrap();
        assert!(encoded);
        let source = path.clone().into();
        let mut decoder =
            engine::audio::decoder::open(&source, &engine::audio::decoder::all()).unwrap();
        let mut samples = Vec::new();
        while decoder.decode(&mut samples).unwrap() {}
        let frames = samples.len() / decoder.channels();
        assert!(frames.abs_diff(3 * 48_000) <= 2, "{frames} frames");
        drop(packets);
        let _ = std::fs::remove_dir_all(&store.0.dir);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_download_tells_how_far_it_is_a_few_times_a_second() {
        let (library, dir) = library("progress", &[("a.wav", wav(10))]).await;
        let id = ids(&library)[0];
        // 1 MB/s: about two seconds for its 1.8 MB.
        let pace = Pace {
            rate: 1_000_000,
            ..Default::default()
        };
        let (owner, reader) = pair(library, Some(pace)).await;
        let pairing = Pairing::in_memory();
        let store = pairing.store();
        let _playing = open_in(&store, &owner, locate_at(&owner, &reader), id)
            .await
            .unwrap();
        let told = Arc::new(Mutex::new(Vec::new()));
        let tell = told.clone();
        let started = Instant::now();
        fetch_whole(
            &pairing,
            &[(owner.key(), id)],
            Quality::Original,
            &[],
            None,
            move |share| tell.lock().unwrap().push(share),
        )
        .await
        .unwrap();
        let told = told.lock().unwrap().clone();
        assert!(told.windows(2).all(|w| w[0] <= w[1]), "{told:?}");
        assert!(told.iter().any(|&s| s > 0.0 && s < 1.0), "{told:?}");
        let most = 1 + (started.elapsed().as_secs_f64() * 5.0) as usize;
        assert!(told.len() <= most, "told {} times", told.len());
        let _ = std::fs::remove_dir_all(&store.0.dir);
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
                ..Default::default()
            };
            let (owner, reader) = pair(library.clone(), Some(pace)).await;
            let (key, addr, runtime) = (owner.key(), loopback(&owner), Handle::current());
            let ids = ids.clone();
            let played = tokio::task::spawn_blocking(move || {
                let open: Open = Arc::new(move |id| {
                    let (net, at) = (reader.clone(), addr.clone());
                    let locate: Locate = Arc::new(move || Ok((net.clone(), at.clone())));
                    let connected = runtime.block_on(connect(
                        &test_store(),
                        &reader,
                        (&key, &addr),
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
