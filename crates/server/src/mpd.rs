//! An MPD protocol server, so MPD clients (mpc, ncmpcpp, M.A.L.P.,
//! Cantata…) can drive the engine: another client of the same [`Engine`]
//! and [`Library`], like `/ws`.
//!
//! Off by default; turned on with [`Pairing::enable_mpd`] (Settings →
//! Devices, or `MOTIF_MPD` headless). It listens on this machine only, or on
//! every interface (advertised as `_mpd._tcp`) while the LAN switch is on.
//! With a password set, `password` gives a connection control; without one,
//! every client is read-only and control commands get `ACK [4@0]`.
//!
//! The MPD playlist is what plays, in order: the context up to the current
//! track, then the queue, then the rest of the context. Queue entries leave
//! once played, as in MPD's consume mode, and only they move; `clear`
//! empties both. Song URIs are library paths without the leading `/`;
//! `lsinfo` lists the library folders at the top.
//!
//! Supported commands (announced as MPD 0.21). Filters are tag/value pairs
//! (`find artist "X" album "Y"`) or simple expressions: `==`, `!=`,
//! `contains`, `starts_with`, `base`, `!` and `AND`, no regular expressions.
//! - Status: `status`, `currentsong`, `stats`, `idle`/`noidle` (player,
//!   mixer, options, playlist, database), `replay_gain_status`, `outputs`,
//!   `getvol`.
//! - Playback (control): `play`, `playid`, `pause`, `stop`, `next`,
//!   `previous`, `seek`, `seekid`, `seekcur`, `setvol`, `volume`, `repeat`,
//!   `single` (repeat + single is repeat one), `random 0`, `consume 0`.
//! - Playlist: `playlistinfo`, `playlistid`, `plchanges`, `plchangesposid`
//!   (both answer the whole playlist); control: `add`, `addid`, `delete`,
//!   `deleteid`, `move`, `moveid`, `clear`.
//! - Library: `find`, `search`, `list` (with `group`), `count`, `lsinfo`,
//!   `listall`, `listallinfo`; control: `findadd`, `searchadd`, `update`,
//!   `rescan`.
//! - Connection: `password`, `ping`, `close`, `commands`, `notcommands`,
//!   `tagtypes`, `urlhandlers`, `decoders`, `listplaylists` (none),
//!   `binarylimit`, command lists.
//!
//! Anything else answers `ACK [5@0]` (unknown command); `random 1`,
//! `consume 1`, `shuffle` and stored playlists aren't supported.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use engine::Engine;
use library::Library;
use protocol::{
    Command, LibraryQuery, LibraryTrack, PlaybackStatus, QueryResult, QueueItem, RepeatMode,
    TrackRef,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc},
    task::JoinSet,
};

use crate::{Pairing, lan::Advert};

const GREETING: &str = "OK MPD 0.21.0\n";
/// Longer request lines close the connection.
const MAX_LINE: u64 = 64 * 1024;
/// Longer command lists close the connection, as MPD's
/// `max_command_list_size` does.
const MAX_LIST: usize = 2 * 1024 * 1024;
/// Connections served at once; more are closed, as MPD's
/// `max_connections` does.
const MAX_CLIENTS: usize = 100;

/// (command, needs control). Also the answer to `commands`.
const COMMANDS: &[(&str, bool)] = &[
    ("add", true),
    ("addid", true),
    ("binarylimit", false),
    ("clear", true),
    ("close", false),
    ("commands", false),
    ("consume", true),
    ("count", false),
    ("currentsong", false),
    ("decoders", false),
    ("delete", true),
    ("deleteid", true),
    ("find", false),
    ("findadd", true),
    ("getvol", false),
    ("idle", false),
    ("list", false),
    ("listall", false),
    ("listallinfo", false),
    ("listplaylists", false),
    ("lsinfo", false),
    ("move", true),
    ("moveid", true),
    ("next", true),
    ("noidle", false),
    ("notcommands", false),
    ("outputs", false),
    ("password", false),
    ("pause", true),
    ("ping", false),
    ("play", true),
    ("playid", true),
    ("playlistid", false),
    ("playlistinfo", false),
    ("plchanges", false),
    ("plchangesposid", false),
    ("previous", true),
    ("random", true),
    ("repeat", true),
    ("replay_gain_status", false),
    ("rescan", true),
    ("search", false),
    ("searchadd", true),
    ("seek", true),
    ("seekcur", true),
    ("seekid", true),
    ("setvol", true),
    ("single", true),
    ("stats", false),
    ("status", false),
    ("stop", true),
    ("tagtypes", false),
    ("update", true),
    ("urlhandlers", false),
    ("volume", true),
];

const TAGS: &[&str] = &[
    "Artist",
    "AlbumArtist",
    "Album",
    "Title",
    "Track",
    "Disc",
    "Date",
    "Genre",
];

/// Serves MPD while [`Pairing::mpd_wanted`] says so, moving the listener
/// when that changes (which also closes every connection).
pub(crate) async fn serve(engine: Engine, library: Option<Library>, pairing: Pairing) {
    let mut states = pairing.subscribe();
    let mut serving = None;
    let mut listener: Option<TcpListener> = None;
    let mut clients = JoinSet::new();
    let slots = Arc::new(Semaphore::new(MAX_CLIENTS));
    let mut _advert = None;
    loop {
        let wanted = pairing.mpd_wanted();
        if wanted != serving {
            clients.abort_all();
            (listener, _advert) = (None, None);
            if let Some((addr, _)) = &wanted {
                match TcpListener::bind(addr).await {
                    Ok(bound) => {
                        tracing::info!(%addr, "MPD listening");
                        listener = Some(bound);
                        if addr.ip().is_unspecified() {
                            _advert = Advert::start("_mpd._tcp", &pairing, addr.port());
                        }
                    }
                    Err(err) => tracing::warn!(%addr, %err, "can't serve MPD"),
                }
            }
            pairing.set_mpd_listening(listener.is_some());
            serving = wanted;
        }
        let retry = listener.is_none() && serving.is_some();
        tokio::select! {
            Some(accepted) = async { Some(listener.as_ref()?.accept().await) } => match accepted {
                Ok((stream, peer)) => {
                    // Dropping the stream turns the extra connection away.
                    let Ok(slot) = slots.clone().try_acquire_owned() else {
                        tracing::debug!(%peer, "too many MPD clients");
                        continue;
                    };
                    let password = serving.as_ref().and_then(|(_, p)| p.clone());
                    let client = Client::new(engine.clone(), library.clone(), password);
                    clients.spawn(async move {
                        client.run(stream, peer).await;
                        drop(slot);
                    });
                }
                Err(err) => {
                    tracing::debug!(%err, "MPD accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
            changed = states.changed() => if changed.is_err() { return },
            // The port may be free now.
            () = tokio::time::sleep(Duration::from_secs(5)), if retry => serving = None,
        }
    }
}

/// An MPD error: `ACK [code@index] {command} message`.
#[derive(Debug, PartialEq)]
struct Ack(u8, String);

const ACK_ARG: u8 = 2;
const ACK_PASSWORD: u8 = 3;
const ACK_PERMISSION: u8 = 4;
const ACK_UNKNOWN: u8 = 5;
const ACK_NO_EXIST: u8 = 50;
const ACK_SYSTEM: u8 = 52;

fn arg_error(message: impl Into<String>) -> Ack {
    Ack(ACK_ARG, message.into())
}

/// Splits a request line into words: bare, or in double quotes with `\`
/// escaping the next character.
fn split(line: &str) -> Result<Vec<String>, Ack> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else {
            return Ok(words);
        };
        let mut word = String::new();
        if first == '"' {
            chars.next();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => word.push(
                        chars
                            .next()
                            .ok_or_else(|| arg_error("Missing closing '\"'"))?,
                    ),
                    Some(c) => word.push(c),
                    None => return Err(arg_error("Missing closing '\"'")),
                }
            }
            if chars.peek().is_some_and(|c| !c.is_whitespace()) {
                return Err(arg_error("Space expected after closing '\"'"));
            }
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                word.push(c);
            }
        }
        words.push(word);
    }
}

// shortcut: Unix paths only (a Windows path keeps its drive and backslashes); fine until Windows users ask.
fn uri(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

fn path(uri: &str) -> String {
    if cfg!(unix) && !uri.starts_with('/') {
        format!("/{uri}")
    } else {
        uri.to_owned()
    }
}

fn number<T: std::str::FromStr>(word: &str) -> Result<T, Ack> {
    word.parse()
        .map_err(|_| arg_error(format!("Integer expected: {word}")))
}

/// `"5"` or `"2:7"` (`"2:"` to the end) as a range of positions.
fn range(word: &str, len: usize) -> Result<std::ops::Range<usize>, Ack> {
    let range = match word.split_once(':') {
        Some((start, "")) => number(start)?..len,
        Some((start, end)) => number(start)?..number(end)?,
        None => {
            let at = number(word)?;
            at..at + 1
        }
    };
    if range.start > range.end || range.end > len {
        return Err(arg_error("Bad song index"));
    }
    Ok(range)
}

fn seconds(word: &str) -> Result<u64, Ack> {
    let s: f64 = word
        .parse()
        .map_err(|_| arg_error(format!("Number expected: {word}")))?;
    Ok((s.max(0.0) * 1000.0) as u64)
}

fn bool_arg(word: Option<&String>) -> Result<bool, Ack> {
    match word.map(String::as_str) {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(arg_error("Boolean (0/1) expected")),
    }
}

/// Appends `key: value`, keeping the value on one line.
fn line(out: &mut String, key: &str, value: impl std::fmt::Display) {
    let value = value.to_string().replace(['\n', '\r'], " ");
    out.push_str(&format!("{key}: {value}\n"));
}

/// The MPD playlist, in play order (see the module header).
struct Playlist {
    items: Vec<QueueItem>,
    /// Where the queue's entries are.
    queue: std::ops::Range<usize>,
    current: Option<usize>,
}

impl Playlist {
    fn of(engine: &Engine) -> Self {
        let state = engine.subscribe().borrow().clone();
        let window = engine.subscribe_context().borrow().clone();
        let queue = engine.subscribe_queue().borrow().clone();
        let offset = window.offset as usize;
        let context = state.context.as_ref();
        let split = match context.and_then(|c| c.index) {
            Some(index) => index as usize + 1,
            None => context
                .and_then(|c| c.next)
                .map_or(usize::MAX, |n| n as usize),
        }
        .saturating_sub(offset)
        .min(window.items.len());
        let mut items = window.items;
        let rest = items.split_off(split);
        // A track played from the queue has left it, but it plays now.
        if let Some(current) = &state.current
            && !items
                .iter()
                .chain(&rest)
                .chain(&queue)
                .any(|i| i.id == current.id)
        {
            items.push(current.clone());
        }
        let start = items.len();
        items.extend(queue);
        let queue = start..items.len();
        items.extend(rest);
        let current = state
            .current
            .and_then(|c| items.iter().position(|i| i.id == c.id));
        Self {
            items,
            queue,
            current,
        }
    }

    /// Changes whenever the entries do, as MPD's playlist version does.
    fn version(&self) -> u32 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        for item in &self.items {
            (item.id, &item.track.id).hash(&mut hasher);
        }
        hasher.finish() as u32 | 1
    }

    fn position_of(&self, id: u64) -> Result<usize, Ack> {
        self.items
            .iter()
            .position(|i| i.id == id)
            .ok_or_else(|| Ack(ACK_NO_EXIST, "No such song".into()))
    }
}

/// What `idle` compares to tell what changed.
#[derive(Clone)]
struct Seen {
    status: PlaybackStatus,
    current: Option<u64>,
    /// When the track would have started at its pace (wall clock ms), or
    /// the position while not playing; moves on a seek.
    start: i64,
    volume: u32,
    repeat: RepeatMode,
    playlist: u32,
    database: u64,
}

impl Seen {
    fn changes(&self, now: &Seen) -> Vec<&'static str> {
        let mut changes = Vec::new();
        if (self.status, self.current) != (now.status, now.current)
            || (self.start - now.start).abs() > 1000
        {
            changes.push("player");
        }
        if self.volume != now.volume {
            changes.push("mixer");
        }
        if self.repeat != now.repeat {
            changes.push("options");
        }
        if self.playlist != now.playlist {
            changes.push("playlist");
        }
        if self.database != now.database {
            changes.push("database");
        }
        changes
    }
}

struct Client {
    engine: Engine,
    library: Option<Library>,
    password: Option<String>,
    control: bool,
    seen: Seen,
}

impl Client {
    fn new(engine: Engine, library: Option<Library>, password: Option<String>) -> Self {
        let mut client = Self {
            engine,
            library,
            password,
            control: false,
            seen: Seen {
                status: PlaybackStatus::Stopped,
                current: None,
                start: 0,
                volume: 0,
                repeat: RepeatMode::Off,
                playlist: 0,
                database: 0,
            },
        };
        client.seen = client.seen_now();
        client
    }

    async fn run(mut self, stream: TcpStream, peer: SocketAddr) {
        tracing::debug!(%peer, "MPD client connected");
        let (read, mut write) = stream.into_split();
        // Read by a task of its own so `idle` can wait on lines and changes.
        let (lines_tx, mut lines) = mpsc::channel::<String>(16);
        let reader = tokio::spawn(async move {
            let mut read = BufReader::new(read);
            loop {
                let mut buf = Vec::new();
                match (&mut read).take(MAX_LINE).read_until(b'\n', &mut buf).await {
                    Ok(_) if buf.ends_with(b"\n") => {}
                    _ => break,
                }
                buf.pop();
                if buf.ends_with(b"\r") {
                    buf.pop();
                }
                let line = String::from_utf8_lossy(&buf).into_owned();
                if lines_tx.send(line).await.is_err() {
                    break;
                }
            }
        });
        let mut out = String::from(GREETING);
        loop {
            if !out.is_empty() {
                if write.write_all(out.as_bytes()).await.is_err() {
                    break;
                }
                out.clear();
            }
            let Some(request) = lines.recv().await else {
                break;
            };
            let words = match split(&request) {
                Ok(words) => words,
                Err(ack) => {
                    respond_ack(&mut out, 0, "", ack);
                    continue;
                }
            };
            match words.first().map(String::as_str) {
                Some("close") => break,
                // Idle ended before it arrived: MPD answers nothing.
                Some("noidle") => {}
                Some("idle") => {
                    if !self.idle(&words[1..], &mut lines, &mut out).await {
                        break;
                    }
                }
                Some(begin @ ("command_list_begin" | "command_list_ok_begin")) => {
                    let ok_each = begin == "command_list_ok_begin";
                    match read_list(&mut lines).await {
                        Some(Ok(list)) => self.list(&list, ok_each, &mut out).await,
                        Some(Err(ack)) => {
                            respond_ack(&mut out, 0, begin, ack);
                            let _ = write.write_all(out.as_bytes()).await;
                            break;
                        }
                        None => break,
                    }
                }
                _ => {
                    let name = words.first().cloned().unwrap_or_default();
                    match self.execute(&words, &mut out).await {
                        Ok(()) => out.push_str("OK\n"),
                        Err(ack) => respond_ack(&mut out, 0, &name, ack),
                    }
                }
            }
        }
        reader.abort();
    }

    async fn list(&mut self, list: &[String], ok_each: bool, out: &mut String) {
        for (index, request) in list.iter().enumerate() {
            let result = match split(request) {
                Ok(words) => self.execute(&words, out).await.map_err(|a| (words, a)),
                Err(ack) => Err((Vec::new(), ack)),
            };
            match result {
                Ok(()) if ok_each => out.push_str("list_OK\n"),
                Ok(()) => {}
                Err((words, ack)) => {
                    let name = words.first().map_or("", String::as_str);
                    return respond_ack(out, index, name, ack);
                }
            }
        }
        out.push_str("OK\n");
    }

    /// Waits for a change in one of `subsystems` (all if empty) or `noidle`.
    /// False if the connection should close.
    async fn idle(
        &mut self,
        subsystems: &[String],
        lines: &mut mpsc::Receiver<String>,
        out: &mut String,
    ) -> bool {
        let mut states = self.engine.subscribe();
        let mut queue = self.engine.subscribe_queue();
        let mut context = self.engine.subscribe_context();
        let mut library = self.library.as_ref().map(Library::subscribe);
        states.mark_unchanged();
        queue.mark_unchanged();
        context.mark_unchanged();
        if let Some(library) = &mut library {
            library.mark_unchanged();
        }
        loop {
            let now = self.seen_now();
            let changes: Vec<_> = self
                .seen
                .changes(&now)
                .into_iter()
                .filter(|c| subsystems.is_empty() || subsystems.iter().any(|s| s == c))
                .collect();
            if !changes.is_empty() {
                self.seen = now;
                for change in changes {
                    line(out, "changed", change);
                }
                out.push_str("OK\n");
                return true;
            }
            tokio::select! {
                Ok(()) = states.changed() => {}
                Ok(()) = queue.changed() => {}
                Ok(()) = context.changed() => {}
                Some(Ok(())) = async { Some(library.as_mut()?.changed().await) } => {}
                request = lines.recv() => {
                    // Anything but `noidle` while idle is a protocol error.
                    if request.as_deref() != Some("noidle") {
                        return false;
                    }
                    out.push_str("OK\n");
                    return true;
                }
            }
        }
    }

    fn seen_now(&self) -> Seen {
        let state = self.engine.state();
        let playing = state.status == PlaybackStatus::Playing;
        let position = state.position_ms as i64;
        Seen {
            status: state.status,
            current: state.current.map(|c| c.id),
            start: if playing {
                state.sampled_at_ms as i64 - position
            } else {
                position
            },
            volume: percent(state.volume),
            repeat: state.repeat,
            playlist: Playlist::of(&self.engine).version(),
            database: self
                .library
                .as_ref()
                .map_or(0, |l| l.subscribe().borrow().revision),
        }
    }

    /// Runs one command, writing its answer (without the final `OK`).
    async fn execute(&mut self, words: &[String], out: &mut String) -> Result<(), Ack> {
        let Some((name, args)) = words.split_first() else {
            return Err(Ack(ACK_UNKNOWN, "No command given".into()));
        };
        let Some(&(_, control)) = COMMANDS.iter().find(|(c, _)| c == name) else {
            return Err(Ack(ACK_UNKNOWN, format!("unknown command \"{name}\"")));
        };
        if control && !self.control {
            return Err(Ack(
                ACK_PERMISSION,
                format!("you don't have permission for \"{name}\""),
            ));
        }
        let arg = |i: usize| {
            args.get(i)
                .map(String::as_str)
                .ok_or_else(|| arg_error("too few arguments"))
        };
        match name.as_str() {
            "ping" | "binarylimit" | "urlhandlers" | "decoders" | "listplaylists"
            | "notcommands" => {}
            "password" => {
                if self.password.is_some() && self.password.as_deref() == Some(arg(0)?) {
                    self.control = true;
                } else {
                    return Err(Ack(ACK_PASSWORD, "incorrect password".into()));
                }
            }
            "commands" => {
                for (command, control) in COMMANDS {
                    if self.control || !control {
                        line(out, "command", command);
                    }
                }
            }
            "tagtypes" => {
                if args.is_empty() {
                    for tag in TAGS {
                        line(out, "tagtype", tag);
                    }
                }
            }
            "outputs" => {
                line(out, "outputid", 0);
                line(out, "outputname", "Motif");
                line(out, "plugin", "motif");
                line(out, "outputenabled", 1);
            }
            "replay_gain_status" => {
                let mode = match self.engine.state().replay_gain.mode {
                    protocol::ReplayGainMode::Off => "off",
                    protocol::ReplayGainMode::Track => "track",
                    protocol::ReplayGainMode::Album => "album",
                    protocol::ReplayGainMode::Auto => "auto",
                };
                line(out, "replay_gain_mode", mode);
            }
            "getvol" => line(out, "volume", percent(self.engine.state().volume)),
            "status" => self.status(out),
            "currentsong" => {
                let playlist = Playlist::of(&self.engine);
                if let Some(at) = playlist.current {
                    self.songs(&playlist, at..at + 1, out).await;
                }
            }
            "playlistinfo" => {
                let playlist = Playlist::of(&self.engine);
                let len = playlist.items.len();
                let range = args.first().map_or(Ok(0..len), |w| range(w, len))?;
                self.songs(&playlist, range, out).await;
            }
            "playlistid" => {
                let playlist = Playlist::of(&self.engine);
                let range = match args.first() {
                    Some(id) => {
                        let at = playlist.position_of(number(id)?)?;
                        at..at + 1
                    }
                    None => 0..playlist.items.len(),
                };
                self.songs(&playlist, range, out).await;
            }
            // shortcut: answers the whole playlist rather than what changed since the version; fine below thousands of entries.
            "plchanges" => {
                let playlist = Playlist::of(&self.engine);
                self.songs(&playlist, 0..playlist.items.len(), out).await;
            }
            "plchangesposid" => {
                for (pos, item) in Playlist::of(&self.engine).items.iter().enumerate() {
                    line(out, "cpos", pos);
                    line(out, "Id", item.id);
                }
            }
            "play" => match args.first() {
                Some(pos) => {
                    let playlist = Playlist::of(&self.engine);
                    let at: usize = number(pos)?;
                    let item = playlist
                        .items
                        .get(at)
                        .ok_or_else(|| arg_error("Bad song index"))?;
                    self.send(Command::PlayItem { item_id: item.id }).await?;
                }
                None => self.send(Command::Play).await?,
            },
            "playid" => match args.first() {
                Some(id) => {
                    self.send(Command::PlayItem {
                        item_id: number(id)?,
                    })
                    .await?
                }
                None => self.send(Command::Play).await?,
            },
            "pause" => {
                let command = match args.first() {
                    None => Command::TogglePlayback,
                    Some(_) if bool_arg(args.first())? => Command::Pause,
                    Some(_) => Command::Play,
                };
                self.send(command).await?;
            }
            "stop" => self.send(Command::Stop).await?,
            "next" => self.send(Command::Next).await?,
            "previous" => self.send(Command::Previous).await?,
            "seek" | "seekid" => {
                let playlist = Playlist::of(&self.engine);
                let at = if name == "seek" {
                    number(arg(0)?)?
                } else {
                    playlist.position_of(number(arg(0)?)?)?
                };
                let item = playlist
                    .items
                    .get(at)
                    .ok_or_else(|| arg_error("Bad song index"))?;
                if playlist.current != Some(at) {
                    self.send(Command::PlayItem { item_id: item.id }).await?;
                }
                let position_ms = seconds(arg(1)?)?;
                self.send(Command::Seek { position_ms }).await?;
            }
            "seekcur" => {
                let time = arg(0)?;
                let by = seconds(time.trim_start_matches(['+', '-']))? as i64;
                let now = elapsed_ms(&self.engine.state()) as i64;
                let position = match time.chars().next() {
                    Some('+') => now + by,
                    Some('-') => now - by,
                    _ => by,
                };
                self.send(Command::Seek {
                    position_ms: position.max(0) as u64,
                })
                .await?;
            }
            "setvol" | "volume" => {
                let given: i32 = number(arg(0)?)?;
                let volume = if name == "volume" {
                    percent(self.engine.state().volume) as i32 + given
                } else {
                    given
                };
                self.send(Command::SetVolume {
                    volume: volume.clamp(0, 100) as f32 / 100.0,
                })
                .await?;
            }
            "repeat" | "single" => {
                let on = bool_arg(args.first())?;
                let now = self.engine.state().repeat;
                // shortcut: single without repeat (stop after this track) has no engine mode; single repeats the track.
                let mode = match (name.as_str(), on) {
                    ("repeat", false) => RepeatMode::Off,
                    ("single", true) => RepeatMode::One,
                    ("repeat", true) if now == RepeatMode::One => RepeatMode::One,
                    ("single", false) if now == RepeatMode::Off => RepeatMode::Off,
                    _ => RepeatMode::All,
                };
                self.send(Command::SetRepeat { mode }).await?;
            }
            "random" | "consume" => {
                if bool_arg(args.first())? {
                    return Err(arg_error(format!("{name} isn't supported")));
                }
            }
            "add" | "addid" => {
                let uri = arg(0)?;
                let tracks: Vec<_> = if name == "add" {
                    self.under(uri)
                        .await?
                        .into_iter()
                        .map(|t| t.track)
                        .collect()
                } else {
                    vec![self.track(uri).await?]
                };
                if tracks.is_empty() {
                    return Err(Ack(ACK_NO_EXIST, "No such directory".into()));
                }
                let before = self.engine.subscribe_queue().borrow().len();
                self.send(Command::Enqueue { tracks }).await?;
                if name == "addid" {
                    let queue = self.engine.subscribe_queue().borrow().clone();
                    let added = queue
                        .get(before)
                        .ok_or_else(|| Ack(ACK_SYSTEM, "not added".into()))?;
                    let id = added.id;
                    if let Some(pos) = args.get(1) {
                        let playlist = Playlist::of(&self.engine);
                        let to = queue_index(&playlist, number(pos)?);
                        self.send(Command::MoveItem { item_id: id, to }).await?;
                    }
                    line(out, "Id", id);
                }
            }
            "delete" | "deleteid" => {
                let playlist = Playlist::of(&self.engine);
                let ids: Vec<u64> = if name == "delete" {
                    playlist.items[range(arg(0)?, playlist.items.len())?]
                        .iter()
                        .map(|i| i.id)
                        .collect()
                } else {
                    vec![playlist.items[playlist.position_of(number(arg(0)?)?)?].id]
                };
                for item_id in ids {
                    self.send(Command::RemoveItem { item_id }).await?;
                }
            }
            "move" | "moveid" => {
                let playlist = Playlist::of(&self.engine);
                let from = if name == "move" {
                    number(arg(0)?)?
                } else {
                    playlist.position_of(number(arg(0)?)?)?
                };
                if !playlist.queue.contains(&from) {
                    return Err(arg_error("Only queued songs can move"));
                }
                let to = queue_index(&playlist, number(arg(1)?)?);
                let item_id = playlist.items[from].id;
                self.send(Command::MoveItem { item_id, to }).await?;
            }
            "clear" => {
                self.send(Command::SetContext {
                    tracks: Vec::new(),
                    start: None,
                    kind: protocol::ContextKind::List,
                    label: None,
                })
                .await?;
            }
            "find" | "search" | "findadd" | "searchadd" | "count" => {
                let exact = name.starts_with("find");
                let filters = filters(args, exact)?;
                let tracks = self.tracks().await?;
                let found = tracks.into_iter().filter(|t| matches(t, &filters));
                match name.as_str() {
                    "find" | "search" => {
                        for track in found {
                            song(out, &track.track, Some(&track));
                        }
                    }
                    "count" => {
                        let (songs, ms) = found.fold((0, 0), |(n, ms), t| {
                            (n + 1, ms + t.track.duration_ms.unwrap_or(0))
                        });
                        line(out, "songs", songs);
                        line(out, "playtime", ms / 1000);
                    }
                    _ => {
                        let tracks: Vec<_> = found.map(|t| t.track).collect();
                        if !tracks.is_empty() {
                            self.send(Command::Enqueue { tracks }).await?;
                        }
                    }
                }
            }
            "list" => {
                let tag = arg(0)?.to_lowercase();
                let mut rest = &args[1..];
                let mut groups = Vec::new();
                while let [.., g, by] = rest
                    && g.eq_ignore_ascii_case("group")
                {
                    groups.insert(0, by.to_lowercase());
                    rest = &rest[..rest.len() - 2];
                }
                // `list album "Artist"`, from before filters had tags.
                let filters = match rest {
                    [artist] if tag == "album" && !artist.starts_with('(') => {
                        filters(&["artist".into(), artist.clone()], true)?
                    }
                    rest => filters(rest, true)?,
                };
                let tracks = self.tracks().await?;
                let mut rows: Vec<Vec<String>> = tracks
                    .iter()
                    .filter(|t| matches(t, &filters))
                    .map(|t| {
                        groups
                            .iter()
                            .chain([&tag])
                            .map(|g| tag_value(t, g).unwrap_or_default())
                            .collect()
                    })
                    .collect();
                rows.sort();
                rows.dedup();
                let names: Vec<_> = groups.iter().chain([&tag]).map(|t| tag_name(t)).collect();
                let mut last: Vec<String> = Vec::new();
                for row in rows {
                    for (i, value) in row.iter().enumerate() {
                        if i + 1 == row.len() || last.get(i) != Some(value) {
                            line(out, names[i], value);
                        }
                    }
                    last = row;
                }
            }
            "lsinfo" | "listall" | "listallinfo" => {
                let dir = uri(args
                    .first()
                    .map_or("", String::as_str)
                    .trim_end_matches('/'));
                if dir.is_empty() && name == "lsinfo" {
                    let library = self.library.as_ref().map(Library::subscribe);
                    for root in library
                        .map(|l| l.borrow().roots.clone())
                        .unwrap_or_default()
                    {
                        line(out, "directory", uri(&root));
                    }
                    return Ok(());
                }
                let prefix = if dir.is_empty() {
                    String::new()
                } else {
                    format!("{dir}/")
                };
                let tracks = self.tracks().await?;
                let mut dirs = Vec::new();
                let mut found = false;
                for track in &tracks {
                    let file = uri(&track.track.id);
                    let Some(rest) = file.strip_prefix(&prefix) else {
                        if file == dir {
                            found = true;
                            song(out, &track.track, Some(track));
                        }
                        continue;
                    };
                    found = true;
                    match rest.split_once('/') {
                        Some((sub, _)) if name == "lsinfo" => dirs.push(format!("{prefix}{sub}")),
                        _ if name == "listall" => line(out, "file", file),
                        _ => song(out, &track.track, Some(track)),
                    }
                }
                if !found && !dir.is_empty() {
                    return Err(Ack(ACK_NO_EXIST, "Not found".into()));
                }
                dirs.sort();
                dirs.dedup();
                for dir in dirs {
                    line(out, "directory", dir);
                }
            }
            "stats" => {
                let tracks = self.tracks().await?;
                let count = |f: fn(&LibraryTrack) -> Option<&String>| {
                    let mut all: Vec<_> = tracks.iter().filter_map(f).collect();
                    all.sort();
                    all.dedup();
                    all.len()
                };
                line(out, "artists", count(|t| t.track.artist.as_ref()));
                line(out, "albums", count(|t| t.track.album.as_ref()));
                line(out, "songs", tracks.len());
                line(out, "uptime", 0);
                line(out, "playtime", 0);
                let ms: u64 = tracks.iter().filter_map(|t| t.track.duration_ms).sum();
                line(out, "db_playtime", ms / 1000);
            }
            "update" | "rescan" => {
                self.query(LibraryQuery::Rescan).await?;
                line(out, "updating_db", 1);
            }
            // `noidle` outside idle and `idle` in a command list do nothing.
            _ => {}
        }
        Ok(())
    }

    fn status(&self, out: &mut String) {
        let state = self.engine.state();
        let playlist = Playlist::of(&self.engine);
        line(out, "volume", percent(state.volume));
        line(out, "repeat", u8::from(state.repeat != RepeatMode::Off));
        line(out, "random", 0);
        line(out, "single", u8::from(state.repeat == RepeatMode::One));
        line(out, "consume", 0);
        line(out, "playlist", playlist.version());
        line(out, "playlistlength", playlist.items.len());
        let status = match state.status {
            PlaybackStatus::Playing => "play",
            PlaybackStatus::Paused => "pause",
            PlaybackStatus::Stopped => "stop",
        };
        line(out, "state", status);
        if let Some(at) = playlist.current {
            line(out, "song", at);
            line(out, "songid", playlist.items[at].id);
            if let Some(next) = playlist.items.get(at + 1) {
                line(out, "nextsong", at + 1);
                line(out, "nextsongid", next.id);
            }
        }
        if state.status != PlaybackStatus::Stopped
            && let Some(current) = &state.current
        {
            let elapsed = elapsed_ms(&state);
            let duration = current.track.duration_ms.unwrap_or(0);
            line(
                out,
                "time",
                format!("{}:{}", elapsed / 1000, duration / 1000),
            );
            line(out, "elapsed", format!("{:.3}", elapsed as f64 / 1000.0));
            line(out, "duration", format!("{:.3}", duration as f64 / 1000.0));
        }
        if let Some(error) = state.current.and_then(|c| c.error) {
            line(out, "error", error);
        }
    }

    /// Writes the playlist's entries in `range`, with library tags where
    /// the library has the file.
    async fn songs(&self, playlist: &Playlist, range: std::ops::Range<usize>, out: &mut String) {
        let items = &playlist.items[range.clone()];
        let paths = items.iter().map(|i| i.track.id.clone()).collect();
        let found: HashMap<String, LibraryTrack> =
            match self.query(LibraryQuery::Lookup { paths }).await {
                Ok(QueryResult::Tracks { tracks, .. }) => tracks
                    .into_iter()
                    .map(|t| (t.track.id.clone(), t))
                    .collect(),
                _ => HashMap::new(),
            };
        for (pos, item) in range.zip(items) {
            song(out, &item.track, found.get(&item.track.id));
            line(out, "Pos", pos);
            line(out, "Id", item.id);
        }
    }

    /// Sends a command and waits until the engine has published its result,
    /// so the next request sees it.
    async fn send(&self, command: Command) -> Result<(), Ack> {
        let mut states = self.engine.subscribe();
        states.mark_unchanged();
        self.engine
            .send(command)
            .await
            .map_err(|e| Ack(ACK_ARG, e.to_string()))?;
        let _ = tokio::time::timeout(Duration::from_secs(1), states.changed()).await;
        Ok(())
    }

    async fn query(&self, query: LibraryQuery) -> Result<QueryResult, Ack> {
        let library = self
            .library
            .clone()
            .ok_or_else(|| Ack(ACK_NO_EXIST, "this engine has no library".into()))?;
        tokio::task::spawn_blocking(move || library.query(query))
            .await
            .map_err(|e| Ack(ACK_SYSTEM, e.to_string()))?
            .map_err(|e| Ack(ACK_SYSTEM, e.to_string()))
    }

    // shortcut: every library request reads all tracks and filters here, a few MB per request for 50k tracks; add SQL filters if that shows.
    async fn tracks(&self) -> Result<Vec<LibraryTrack>, Ack> {
        let query = LibraryQuery::Tracks {
            search: None,
            sort: Vec::new(),
            offset: 0,
            limit: u32::MAX,
        };
        match self.query(query).await? {
            QueryResult::Tracks { tracks, .. } => Ok(tracks),
            _ => Err(Ack(ACK_SYSTEM, "unexpected library answer".into())),
        }
    }

    /// The library track at `uri`.
    async fn track(&self, uri: &str) -> Result<TrackRef, Ack> {
        match self
            .query(LibraryQuery::Lookup {
                paths: vec![path(uri)],
            })
            .await?
        {
            QueryResult::Tracks { mut tracks, .. } if !tracks.is_empty() => {
                Ok(tracks.swap_remove(0).track)
            }
            _ => Err(Ack(ACK_NO_EXIST, "No such song".into())),
        }
    }

    /// The library tracks at or under `uri` (`""` or `/` for all).
    async fn under(&self, uri_: &str) -> Result<Vec<LibraryTrack>, Ack> {
        let dir = uri(uri_.trim_end_matches('/'));
        let prefix = format!("{dir}/");
        let tracks = self.tracks().await?;
        Ok(tracks
            .into_iter()
            .filter(|t| {
                let file = uri(&t.track.id);
                dir.is_empty() || file == dir || file.starts_with(&prefix)
            })
            .collect())
    }
}

/// A command list's requests up to `command_list_end`; `None` if the
/// connection closed first, an error once it passes [`MAX_LIST`].
async fn read_list(lines: &mut mpsc::Receiver<String>) -> Option<Result<Vec<String>, Ack>> {
    let (mut list, mut size) = (Vec::new(), 0);
    loop {
        let request = lines.recv().await?;
        if request == "command_list_end" {
            return Some(Ok(list));
        }
        size += request.len() + 1;
        if size > MAX_LIST {
            return Some(Err(Ack(ACK_SYSTEM, "command list is too long".into())));
        }
        list.push(request);
    }
}

fn respond_ack(out: &mut String, index: usize, command: &str, Ack(code, message): Ack) {
    out.push_str(&format!("ACK [{code}@{index}] {{{command}}} {message}\n"));
}

/// The index in the queue a move to playlist position `pos` lands at.
fn queue_index(playlist: &Playlist, pos: usize) -> u32 {
    let last = playlist.queue.len().saturating_sub(1);
    pos.saturating_sub(playlist.queue.start).min(last) as u32
}

fn percent(volume: f32) -> u32 {
    (volume * 100.0).round() as u32
}

fn elapsed_ms(state: &protocol::PlayerState) -> u64 {
    let mut elapsed = state.position_ms;
    if state.status == PlaybackStatus::Playing {
        elapsed += engine::wall_clock_ms().saturating_sub(state.sampled_at_ms);
    }
    match state.current.as_ref().and_then(|c| c.track.duration_ms) {
        Some(duration) => elapsed.min(duration),
        None => elapsed,
    }
}

/// One condition of `find`, `search`, `list`…
#[derive(Debug, PartialEq)]
struct Filter {
    /// Lower-case MPD tag, or `any`, `file`, `base`.
    tag: String,
    value: String,
    op: Op,
    negate: bool,
    /// Ignoring case, as `search` does.
    fold: bool,
}

#[derive(Debug, PartialEq)]
enum Op {
    Eq,
    Contains,
    StartsWith,
}

/// Tag/value pairs (equal for `find`, containing for `search`) or filter
/// expressions; `sort` and `window` (ignored) may follow.
fn filters(args: &[String], exact: bool) -> Result<Vec<Filter>, Ack> {
    let mut filters = Vec::new();
    let mut args = args.iter();
    while let Some(tag) = args.next() {
        if tag.starts_with('(') {
            let mut chars = tag.chars().peekable();
            expression(&mut chars, false, !exact, &mut filters)?;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| arg_error("Incorrect number of filter arguments"))?;
        let tag = tag.to_lowercase();
        if tag != "sort" && tag != "window" {
            filters.push(Filter {
                tag,
                value: value.clone(),
                op: if exact { Op::Eq } else { Op::Contains },
                negate: false,
                fold: !exact,
            });
        }
    }
    Ok(filters)
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// Parses one `(…)` filter expression into `filters`: `(TAG == 'v')`, `!=`,
/// `contains`, `starts_with`, `(base 'dir')`, `(!(…))` and `((…) AND (…))`.
fn expression(
    chars: &mut Chars,
    negate: bool,
    fold: bool,
    filters: &mut Vec<Filter>,
) -> Result<(), Ack> {
    let bad = || arg_error("bad filter expression");
    let skip = |chars: &mut Chars| while chars.next_if(|c| c.is_whitespace()).is_some() {};
    let word = |chars: &mut Chars| {
        let mut word = String::new();
        while let Some(c) = chars.next_if(|c| !c.is_whitespace() && !"()'\"".contains(*c)) {
            word.push(c);
        }
        word
    };
    skip(chars);
    if chars.next() != Some('(') {
        return Err(bad());
    }
    skip(chars);
    match chars.peek() {
        Some('!') => {
            chars.next();
            let before = filters.len();
            expression(chars, !negate, fold, filters)?;
            if filters.len() != before + 1 {
                return Err(arg_error("only single conditions can be negated"));
            }
        }
        Some('(') => loop {
            expression(chars, negate, fold, filters)?;
            skip(chars);
            if chars.peek() == Some(&')') {
                break;
            }
            if negate || word(chars) != "AND" {
                return Err(bad());
            }
        },
        _ => {
            let tag = word(chars).to_lowercase();
            skip(chars);
            let (op, negate) = if tag == "base" {
                (Op::StartsWith, negate)
            } else {
                let op = word(chars);
                skip(chars);
                let negate = negate != (op == "!=");
                match op.as_str() {
                    "==" | "!=" => (Op::Eq, negate),
                    "contains" => (Op::Contains, negate),
                    "starts_with" => (Op::StartsWith, negate),
                    other => {
                        return Err(arg_error(format!("unsupported filter operator {other}")));
                    }
                }
            };
            let quote = chars
                .next()
                .filter(|q| *q == '\'' || *q == '"')
                .ok_or_else(bad)?;
            let mut value = String::new();
            loop {
                match chars.next().ok_or_else(bad)? {
                    c if c == quote => break,
                    '\\' => value.push(chars.next().ok_or_else(bad)?),
                    c => value.push(c),
                }
            }
            filters.push(Filter {
                tag,
                value,
                op,
                negate,
                fold,
            });
        }
    }
    skip(chars);
    if chars.next() != Some(')') {
        return Err(bad());
    }
    Ok(())
}

/// Whether `track` passes every filter. `any` is any tag; `base` a
/// directory.
fn matches(track: &LibraryTrack, filters: &[Filter]) -> bool {
    filters.iter().all(|f| {
        let test = |value: Option<String>| {
            let Some(mut value) = value else { return false };
            let mut wanted = f.value.clone();
            if f.fold {
                (value, wanted) = (value.to_lowercase(), wanted.to_lowercase());
            }
            match f.op {
                Op::Eq => value == wanted,
                Op::Contains => value.contains(&wanted),
                Op::StartsWith => value.starts_with(&wanted),
            }
        };
        let passes = match f.tag.as_str() {
            "any" => ["artist", "albumartist", "album", "title", "genre", "file"]
                .iter()
                .any(|t| test(tag_value(track, t))),
            "base" => {
                let base = uri(f.value.trim_end_matches('/'));
                uri(&track.track.id).starts_with(&format!("{base}/"))
            }
            tag => test(tag_value(track, tag)),
        };
        passes != f.negate
    })
}

/// A tag's value, by lower-case MPD tag name.
fn tag_value(track: &LibraryTrack, tag: &str) -> Option<String> {
    let t = &track.track;
    match tag {
        "artist" => t.artist.clone(),
        "albumartist" => track.album_artist.clone().or_else(|| t.artist.clone()),
        "album" => t.album.clone(),
        "title" => t.title.clone(),
        "genre" => track.genre.clone(),
        "date" => track.original_year.or(track.year).map(|y| y.to_string()),
        "track" => track.track_no.map(|n| n.to_string()),
        "disc" => track.disc_no.map(|n| n.to_string()),
        "file" => Some(uri(&t.id).to_owned()),
        _ => None,
    }
}

fn tag_name(tag: &str) -> &'static str {
    TAGS.iter()
        .find(|t| t.eq_ignore_ascii_case(tag))
        .copied()
        .unwrap_or("file")
}

/// A song's lines, tags from the library where given.
fn song(out: &mut String, track: &TrackRef, info: Option<&LibraryTrack>) {
    line(out, "file", uri(&track.id));
    match info {
        Some(info) => {
            for tag in TAGS {
                if let Some(value) = tag_value(info, &tag.to_lowercase()) {
                    line(out, tag, value);
                }
            }
        }
        None => {
            for (tag, value) in [
                ("Artist", &track.artist),
                ("Album", &track.album),
                ("Title", &track.title),
            ] {
                if let Some(value) = value {
                    line(out, tag, value);
                }
            }
        }
    }
    if let Some(ms) = track.duration_ms {
        line(out, "Time", ms / 1000);
        line(out, "duration", format!("{:.3}", ms as f64 / 1000.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_requests_into_words() {
        assert_eq!(split("status").unwrap(), ["status"]);
        assert_eq!(
            split(r#"find  artist "The \"Band\"" album "a\\b""#).unwrap(),
            ["find", "artist", r#"The "Band""#, "album", r"a\b"]
        );
        assert_eq!(split(r#"add """#).unwrap(), ["add", ""]);
        assert_eq!(split("  ").unwrap(), Vec::<String>::new());
        assert!(split(r#"add "unclosed"#).is_err());
        assert!(split(r#"add "a"b"#).is_err());
    }

    #[test]
    fn ranges_and_numbers() {
        assert_eq!(range("2", 5).unwrap(), 2..3);
        assert_eq!(range("1:3", 5).unwrap(), 1..3);
        assert_eq!(range("3:", 5).unwrap(), 3..5);
        assert!(range("4:9", 5).is_err());
        assert!(range("x", 5).is_err());
        assert_eq!(seconds("1.5").unwrap(), 1500);
    }

    #[test]
    fn uris_are_paths_without_the_root() {
        assert_eq!(uri("/music/a.flac"), "music/a.flac");
        if cfg!(unix) {
            assert_eq!(path("music/a.flac"), "/music/a.flac");
            assert_eq!(path("/music/a.flac"), "/music/a.flac");
        }
    }

    fn library_track(path: &str, artist: &str, album: &str, title: &str) -> LibraryTrack {
        LibraryTrack {
            id: 0,
            track: TrackRef {
                source: "local".into(),
                id: path.into(),
                title: Some(title.into()),
                artist: Some(artist.into()),
                album: Some(album.into()),
                duration_ms: Some(61_500),
                ..Default::default()
            },
            album_artist: None,
            track_no: Some(1),
            disc_no: None,
            year: Some(1999),
            original_year: None,
            genre: None,
            codec: None,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            bitrate: None,
            art: None,
            missing: false,
            problem: None,
        }
    }

    #[test]
    fn find_is_exact_and_search_ignores_case() {
        let track = library_track("/m/a.flac", "The Band", "First", "Song");
        let find = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            matches(&track, &filters(&args, true).unwrap())
        };
        let search = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            matches(&track, &filters(&args, false).unwrap())
        };
        assert!(find(&["artist", "The Band"]));
        assert!(!find(&["artist", "band"]));
        assert!(search(&["artist", "band"]));
        assert!(search(&["any", "firs", "date", "1999"]));
        assert!(find(&["base", "m"]));
        assert!(find(&["albumartist", "The Band", "sort", "Title"]));
        assert!(filters(&["artist".into()], true).is_err());

        // Expressions, as MPD 0.21 clients send them.
        assert!(find(&["(artist == 'The Band')"]));
        assert!(find(&[
            r#"((album == "First") AND (title starts_with 'So'))"#
        ]));
        assert!(!find(&["((album == 'First') AND (title != 'Song'))"]));
        assert!(find(&["(!(artist contains 'Other'))"]));
        assert!(search(&["(artist contains 'BAND')"]));
        assert!(find(&["(base 'm')"]));
        assert!(!find(&[r"(title == 'So\'ng')"]));
        for bad in [
            "(artist =~ 'x')",
            "(artist == 'x'",
            "(artist == x)",
            "(!((a == 'b') AND (c == 'd')))",
        ] {
            assert!(filters(&[bad.into()], true).is_err(), "{bad}");
        }

        let mut out = String::new();
        song(&mut out, &track.track, Some(&track));
        assert_eq!(
            out,
            "file: m/a.flac\nArtist: The Band\nAlbumArtist: The Band\nAlbum: First\n\
             Title: Song\nTrack: 1\nDate: 1999\nTime: 61\nduration: 61.500\n"
        );
    }

    /// A minute of silence (8 kHz, 8-bit mono WAV) named `name` in a
    /// folder of this test run's, so it plays without failing.
    fn queued(name: &str) -> TrackRef {
        let dir = std::env::temp_dir().join(format!("motif-mpd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{name}.wav"));
        let data = 8000 * 60u32;
        let mut wav = Vec::new();
        for chunk in [
            &b"RIFF"[..],
            &(36 + data).to_le_bytes(),
            b"WAVEfmt ",
            &16u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &1u16.to_le_bytes(),
            &8000u32.to_le_bytes(),
            &8000u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &8u16.to_le_bytes(),
            b"data",
            &data.to_le_bytes(),
        ] {
            wav.extend_from_slice(chunk);
        }
        wav.resize(wav.len() + data as usize, 128);
        std::fs::write(&file, wav).unwrap();
        TrackRef {
            source: "local".into(),
            id: file.to_string_lossy().into_owned(),
            title: Some(name.into()),
            ..Default::default()
        }
    }

    /// Runs `request` as a client would and returns the whole answer.
    async fn ask(client: &mut Client, request: &str) -> String {
        let mut out = String::new();
        let words = split(request).unwrap();
        match client.execute(&words, &mut out).await {
            Ok(()) => out.push_str("OK\n"),
            Err(ack) => respond_ack(&mut out, 0, &words[0], ack),
        }
        out
    }

    fn field<'a>(answer: &'a str, key: &str) -> Vec<&'a str> {
        answer
            .lines()
            .filter_map(|l| l.strip_prefix(&format!("{key}: ")))
            .collect()
    }

    #[tokio::test]
    async fn read_only_clients_get_an_ack() {
        let engine = Engine::spawn(engine::LocalFiles);
        let mut client = Client::new(engine.clone(), None, Some("secret".into()));
        assert_eq!(
            ask(&mut client, "play").await,
            "ACK [4@0] {play} you don't have permission for \"play\"\n"
        );
        assert!(
            ask(&mut client, "status")
                .await
                .ends_with("state: stop\nOK\n")
        );
        assert!(!field(&ask(&mut client, "commands").await, "command").contains(&"play"));
        assert_eq!(
            ask(&mut client, "password wrong").await,
            "ACK [3@0] {password} incorrect password\n"
        );
        assert_eq!(ask(&mut client, "password secret").await, "OK\n");
        assert_eq!(ask(&mut client, "setvol 0").await, "OK\n");
        assert_eq!(engine.state().volume, 0.0);
        assert_eq!(
            ask(&mut client, "frobnicate").await,
            "ACK [5@0] {frobnicate} unknown command \"frobnicate\"\n"
        );

        // No password set: nobody gets control.
        let mut client = Client::new(engine, None, None);
        assert!(ask(&mut client, "password ").await.starts_with("ACK [3@0]"));
        assert!(ask(&mut client, "stop").await.starts_with("ACK [4@0]"));
    }

    #[tokio::test]
    async fn commands_map_onto_the_engine() {
        let engine = Engine::spawn(engine::LocalFiles);
        engine
            .send(Command::SetVolume { volume: 0.0 })
            .await
            .unwrap();
        let mut client = Client::new(engine.clone(), None, Some("pw".into()));
        ask(&mut client, "password pw").await;
        engine
            .send(Command::SetContext {
                tracks: vec![queued("c1"), queued("c2"), queued("c3")],
                start: None,
                kind: protocol::ContextKind::Album,
                label: None,
            })
            .await
            .unwrap();
        client
            .send(Command::Enqueue {
                tracks: vec![queued("q1"), queued("q2")],
            })
            .await
            .unwrap();
        // Stopped, nothing played yet: the queue comes first.
        let titles = |a: &str| field(a, "Title").join(" ");
        assert_eq!(
            titles(&ask(&mut client, "playlistinfo").await),
            "q1 q2 c1 c2 c3"
        );

        // Playing c2 (index 3): the queue plays after it.
        assert_eq!(ask(&mut client, "play 3").await, "OK\n");
        let answer = ask(&mut client, "playlistinfo").await;
        assert_eq!(titles(&answer), "c1 c2 q1 q2 c3");
        let ids = field(&answer, "Id");
        let status = ask(&mut client, "status").await;
        assert_eq!(field(&status, "song"), ["1"]);
        assert_eq!(field(&status, "songid"), [ids[1]]);
        assert_eq!(field(&status, "playlistlength"), ["5"]);
        assert_eq!(titles(&ask(&mut client, "currentsong").await), "c2");

        assert_eq!(ask(&mut client, "move 3 2").await, "OK\n");
        assert_eq!(
            titles(&ask(&mut client, "playlistinfo").await),
            "c1 c2 q2 q1 c3"
        );
        assert!(ask(&mut client, "move 0 3").await.starts_with("ACK [2@0]"));
        assert_eq!(
            ask(&mut client, &format!("deleteid {}", ids[2])).await,
            "OK\n"
        );
        assert_eq!(
            titles(&ask(&mut client, "playlistinfo").await),
            "c1 c2 q2 c3"
        );

        assert_eq!(ask(&mut client, "single 1").await, "OK\n");
        assert_eq!(engine.state().repeat, RepeatMode::One);
        assert_eq!(ask(&mut client, "single 0").await, "OK\n");
        assert_eq!(engine.state().repeat, RepeatMode::All);
        assert_eq!(ask(&mut client, "repeat 0").await, "OK\n");
        assert_eq!(engine.state().repeat, RepeatMode::Off);
        assert!(ask(&mut client, "random 1").await.starts_with("ACK [2@0]"));

        assert_eq!(ask(&mut client, "pause 1").await, "OK\n");
        assert_eq!(engine.state().status, PlaybackStatus::Paused);
        assert_eq!(ask(&mut client, "clear").await, "OK\n");
        assert_eq!(
            field(&ask(&mut client, "status").await, "playlistlength"),
            ["0"]
        );
        assert!(
            ask(&mut client, "find artist x")
                .await
                .starts_with("ACK [50@0]")
        );
    }

    #[tokio::test]
    async fn idle_reports_changes_and_noidle_ends_it() {
        let engine = Engine::spawn(engine::LocalFiles);
        let mut client = Client::new(engine.clone(), None, None);
        let (tx, mut lines) = mpsc::channel(1);
        let mut out = String::new();
        engine
            .send(Command::SetVolume { volume: 0.5 })
            .await
            .unwrap();
        assert!(client.idle(&[], &mut lines, &mut out).await);
        assert_eq!(out, "changed: mixer\nOK\n");

        out.clear();
        tx.send("noidle".into()).await.unwrap();
        assert!(client.idle(&["player".into()], &mut lines, &mut out).await);
        assert_eq!(out, "OK\n");
    }

    #[tokio::test]
    async fn command_lists_are_capped() {
        let (tx, mut lines) = mpsc::channel(16);
        tokio::spawn(async move {
            for request in ["status", "ping", "command_list_end"] {
                tx.send(request.to_owned()).await.unwrap();
            }
            let request = "x".repeat(60_000);
            while tx.send(request.clone()).await.is_ok() {}
        });
        assert_eq!(
            read_list(&mut lines).await.unwrap().unwrap(),
            ["status", "ping"]
        );
        assert_eq!(
            read_list(&mut lines).await.unwrap().unwrap_err(),
            Ack(ACK_SYSTEM, "command list is too long".into())
        );
    }

    #[tokio::test]
    async fn connections_are_capped() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let pairing = Pairing::in_memory();
        pairing.set_mpd_port(port);
        pairing.enable_mpd(true);
        let mut listening = pairing.subscribe();
        tokio::spawn(serve(
            Engine::spawn(engine::LocalFiles),
            None,
            pairing.clone(),
        ));
        listening.wait_for(|s| s.mpd.listening).await.unwrap();
        let greeting = |mut stream: TcpStream| async move {
            let mut buf = vec![0; GREETING.len()];
            stream.read_exact(&mut buf).await.map(|_| stream)
        };
        let mut open = Vec::new();
        for _ in 0..MAX_CLIENTS {
            let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            open.push(greeting(stream).await.unwrap());
        }
        let extra = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert!(greeting(extra).await.is_err(), "turned away");
        drop(open.pop());
        // The freed slot is taken back once its task notices the close.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let again = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert!(greeting(again).await.is_ok());
    }

    #[tokio::test]
    async fn command_lists_stop_at_the_first_error() {
        let engine = Engine::spawn(engine::LocalFiles);
        let mut client = Client::new(engine, None, None);
        let mut out = String::new();
        let list = ["ping".to_owned(), "status".to_owned(), "play".to_owned()];
        client.list(&list, true, &mut out).await;
        assert!(out.starts_with("list_OK\nvolume: "));
        assert!(
            out.ends_with("list_OK\nACK [4@2] {play} you don't have permission for \"play\"\n")
        );
    }
}
