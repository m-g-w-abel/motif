//! Plays tracks held by linked engines ([`net::Alpn::Files`], #243): the
//! reader asks the owner for byte ranges of one of its own tracks by id, so
//! decoders read and seek it as if it were a file here. One connection per
//! stream; each range is a QUIC stream of its own: a [`Range`] frame
//! (sync.rs's framing), then the owner's answer frame, `{"Ok": size of the
//! file}` or `{"Err": why}`, then up to `len` bytes (fewer only at the end of
//! the file) and the end of the stream. The owner serves only its own tracks,
//! by id, never a path, and only to linked engines (the endpoint lets in no
//! others). Originals only; transcoding is #244.

use std::{
    collections::HashMap,
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use engine::{ByteSource, Source};
use library::Library;
use net::{Alpn, Connection, Net, RecvStream, SendStream};
use protocol::Reach;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::{
    Pairing,
    sync::{read, write},
};

/// A request for bytes `offset..offset + len` of the owner's track `id`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Range {
    id: i64,
    offset: u64,
    len: u32,
}

/// Bytes per range: a third of a second of CD-quality FLAC.
const CHUNK: u64 = 64 << 10;
/// Ranges fetched ahead of the decoder while it reads straight through
/// (2 MiB: about 16 s of CD-quality FLAC, 4 s of 24-bit/192 kHz).
const AHEAD: u64 = 32;
/// The largest range served.
const MAX_RANGE: u32 = 1 << 20;
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

/// Answers range requests on `conn` from a linked engine until it closes.
pub(crate) async fn serve(library: Library, conn: Connection, pace: Option<Pace>) {
    // When the paced link is next free.
    let free = Arc::new(tokio::sync::Mutex::new(tokio::time::Instant::now()));
    while let Ok((send, recv)) = conn.accept_bi().await {
        let (library, free) = (library.clone(), free.clone());
        tokio::spawn(async move {
            if let Err(err) = answer(&library, send, recv, pace, &free).await {
                tracing::debug!(%err, "files: range not sent");
            }
        });
    }
}

async fn answer(
    library: &Library,
    mut send: SendStream,
    mut recv: RecvStream,
    pace: Option<Pace>,
    free: &tokio::sync::Mutex<tokio::time::Instant>,
) -> Result<(), String> {
    let range: Range = read(&mut recv).await?;
    let library = library.clone();
    let found = tokio::task::spawn_blocking(move || -> io::Result<(u64, Vec<u8>)> {
        let path = library
            .own_file(range.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not on this engine"))?;
        let mut file = std::fs::File::open(path)?;
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
    let open = Mutex::new(None::<(Result<(Arc<Remote>, u64), String>, Instant)>);
    Source::Stream {
        name,
        open: Arc::new(move || {
            let mut open = open.lock().unwrap_or_else(|e| e.into_inner());
            let (remote, len) = match &*open {
                Some((Ok((remote, len)), _)) if remote.conn.close_reason().is_none() => {
                    (remote.clone(), *len)
                }
                Some((Err(err), at)) if at.elapsed() < DIAL => {
                    return Err(io::Error::other(err.clone()));
                }
                _ => {
                    let dialled = runtime.block_on(dial(&pairing, &copies, runtime.clone()));
                    let (dialled, _) = open.insert((dialled, Instant::now()));
                    dialled.clone().map_err(io::Error::other)?
                }
            };
            Ok(Box::new(Reader::new(remote, len)) as Box<dyn ByteSource>)
        }),
    }
}

/// Connects to the first engine in `copies` that answers with the start of
/// its copy.
async fn dial(
    pairing: &Pairing,
    copies: &[(String, i64)],
    runtime: Handle,
) -> Result<(Arc<Remote>, u64), String> {
    let net = pairing
        .net()
        .ok_or("on a linked engine, but not connected to any now")?;
    let mut copies = copies.to_vec();
    copies.sort_by_key(|(key, _)| pairing.reach(key) == Reach::Unreachable);
    let mut reasons = Vec::new();
    for (key, id) in copies {
        let addr = pairing.linked().into_iter().find(|(k, _)| *k == key);
        let Some((_, addr)) = addr else { continue };
        match tokio::time::timeout(DIAL, connect(&net, &key, &addr, id, runtime.clone())).await {
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

/// Opens track `id` on the linked engine `key`: its first range and size.
async fn connect(
    net: &Net,
    key: &str,
    addr: &net::PeerAddr,
    id: i64,
    runtime: Handle,
) -> Result<(Arc<Remote>, u64), String> {
    let conn = net.connect(key, addr, Alpn::Files).await?;
    let (len, first) = fetch(&conn, id, 0, CHUNK as u32).await?;
    let remote = Remote {
        conn,
        id,
        runtime,
        state: Mutex::new(Fetched {
            chunks: HashMap::from([(0, Chunk::Here(first.into()))]),
            wanted: 0..0,
        }),
        arrived: Condvar::new(),
    };
    Ok((Arc::new(remote), len))
}

/// A connection to the owner of track `id` and the ranges fetched or
/// being fetched.
struct Remote {
    conn: Connection,
    id: i64,
    runtime: Handle,
    state: Mutex<Fetched>,
    arrived: Condvar,
}

struct Fetched {
    /// By index.
    chunks: HashMap<u64, Chunk>,
    /// The chunks to fetch, in order.
    wanted: std::ops::Range<u64>,
}

enum Chunk {
    Coming,
    Here(Arc<[u8]>),
    Failed(String),
}

impl Remote {
    fn state(&self) -> std::sync::MutexGuard<'_, Fetched> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wants chunks `at ..= at + ahead` (of a file `len` long), keeping
    /// those from `at - 1` on and dropping the rest but the first and last,
    /// which every decoder probing the file reads.
    fn want(self: &Arc<Self>, at: u64, ahead: u64, len: u64) {
        let end = len.div_ceil(CHUNK);
        let keep = at.saturating_sub(1)..=at + AHEAD;
        let mut state = self.state();
        state
            .chunks
            .retain(|&i, _| keep.contains(&i) || i == 0 || i + 1 == end);
        state.wanted = at..(at + ahead + 1).min(end);
        self.fill(&mut state);
    }

    /// Fetches the first wanted chunks not here, [`IN_FLIGHT`] at a time:
    /// more at once would share the link and make the next one late. The
    /// first, which the reader waits for, is always fetched.
    fn fill(self: &Arc<Self>, state: &mut Fetched) {
        let coming = |state: &Fetched| {
            let chunks = state.chunks.values();
            chunks.filter(|c| matches!(c, Chunk::Coming)).count()
        };
        for i in state.wanted.clone() {
            if i > state.wanted.start && coming(state) >= IN_FLIGHT {
                return;
            }
            if state.chunks.contains_key(&i) {
                continue;
            }
            state.chunks.insert(i, Chunk::Coming);
            let remote = self.clone();
            self.runtime.spawn(async move {
                let fetched = fetch(&remote.conn, remote.id, i * CHUNK, CHUNK as u32).await;
                let mut state = remote.state();
                if let Some(chunk @ Chunk::Coming) = state.chunks.get_mut(&i) {
                    *chunk = match fetched {
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
    /// longer than [`STALL`] (and never an end of file, which would end
    /// the track as if it were complete).
    fn chunk(&self, i: u64) -> io::Result<Arc<[u8]>> {
        let deadline = Instant::now() + STALL + Duration::from_secs(1);
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
                Some(Chunk::Coming) if !left.is_zero() => {
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
        self.remote.want(at, ahead, self.len);
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
        let (owner, reader) = (endpoint(1).await, endpoint(2).await);
        let serving = owner.clone();
        tokio::spawn(async move {
            while let Some((alpn, conn)) = serving.accept().await {
                assert_eq!(alpn, Alpn::Files);
                tokio::spawn(serve(library.clone(), conn, pace));
            }
        });
        (owner, reader)
    }

    async fn open(owner: &Net, reader: &Net, id: i64) -> Result<Reader, String> {
        let runtime = Handle::current();
        let (remote, len) = connect(reader, &owner.key(), &loopback(owner), id, runtime).await?;
        Ok(Reader::new(remote, len))
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
        assert!(started.elapsed() < STALL + Duration::from_secs(2));
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
        assert_eq!(frames, 3 * 44_100);
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

    /// A stream of the track `remote` opened.
    fn opened(remote: Arc<Remote>, len: u64) -> Source {
        Source::Stream {
            name: "measured".into(),
            open: Arc::new(move || {
                Ok(Box::new(Reader::new(remote.clone(), len)) as Box<dyn ByteSource>)
            }),
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
                    let connected =
                        runtime.block_on(connect(&reader, &key, &addr, id, runtime.clone()));
                    let (remote, len) = connected.unwrap();
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
