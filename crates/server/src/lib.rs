//! Exposes an [`Engine`] and the [`Library`] over WebSocket at `/ws`, serves
//! artwork at `/art/<id>` (a track's other images listed at `/images`) and
//! the web UI.
//!
//! Clients on this machine (loopback peers, from pages served by this
//! machine) need no token; anyone else needs a paired device's token, got by
//! redeeming a one-time code at `POST /pair` (see [`Pairing`]). Other
//! machines reach the engine only while the LAN is open (see [`Pairing`]'s
//! [`protocol::LanState`]): the server then listens on every interface.
//!
//! MPD clients have a server of their own (see `mpd.rs`), off by default.

mod lan;
mod mpd;
mod pairing;
pub mod playback;
mod resolver;
pub mod settings;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use engine::Engine;
use futures_util::{SinkExt, StreamExt};
use library::Library;
use protocol::{
    ClientMessage, Command, Event, IntegrationSettings, LibraryQuery, PROTOCOL_VERSION,
    PairRequest, QueryResult, Scope, ServerMessage,
};
use tokio::sync::{Semaphore, mpsc, watch};
use tower_http::services::{ServeDir, ServeFile};

use pairing::Who;
pub use pairing::{Pairing, pair_url};
pub use resolver::LibraryResolver;

pub struct Config {
    pub addr: SocketAddr,
    /// The web UI to serve, for browsers and for phones opening a pairing
    /// link. `None` serves only `/ws`, artwork and `POST /pair`.
    pub ui: Option<Ui>,
    pub library: Option<Library>,
    /// The host's integration settings, which clients read and replace.
    pub integrations: watch::Sender<IntegrationSettings>,
    pub pairing: Pairing,
}

/// Where the web UI comes from.
pub enum Ui {
    /// A built UI folder (`apps/ui/dist`).
    Dir(PathBuf),
    /// A lookup of path (no leading `/`) to file bytes and MIME type, for a
    /// UI built into the app (the desktop's Tauri assets). It answers unknown
    /// paths with `index.html` itself so client-side routes work.
    Assets(AssetLookup),
}

/// Path to file bytes and MIME type; see [`Ui::Assets`].
pub type AssetLookup = Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

#[derive(Clone)]
struct AppState {
    engine: Engine,
    library: Option<Library>,
    integrations: watch::Sender<IntegrationSettings>,
    pairing: Pairing,
    /// Limits artwork requests in flight. Each can hold a whole image or a
    /// decoded one (tens of MB), and a grid asks for hundreds at once.
    art_slots: Arc<Semaphore>,
}

/// Artwork requests served at once: enough to fill a screen of covers
/// without waiting, while capping the memory peak of a large grid.
const ART_SLOTS: usize = 50;

/// Binds the listener and returns its address plus a future that serves it.
/// Binding first lets callers use port 0 and learn the real port. While the
/// LAN is open the server listens on every interface on that port. The
/// future also serves MPD while [`Pairing`] says so.
pub async fn bind(
    engine: Engine,
    config: Config,
) -> std::io::Result<(SocketAddr, impl Future<Output = std::io::Result<()>>)> {
    let listener = lan::LanListener::bind(config.addr, config.pairing.clone()).await?;
    let addr = listener.local_addr();
    // Tapping is a no-op; axum knows the peer address of tapped listeners.
    let listener = axum::serve::ListenerExt::tap_io(listener, |_| {});
    let mpd = mpd::serve(
        engine.clone(),
        config.library.clone(),
        config.pairing.clone(),
    );
    let app = router(engine, config).into_make_service_with_connect_info::<SocketAddr>();
    Ok((addr, async move {
        tokio::select! {
            result = axum::serve(listener, app).into_future() => result,
            () = mpd => Ok(()),
        }
    }))
}

/// The routes; serve them with `ConnectInfo<SocketAddr>`, which says who is
/// local. `config.addr` is unused.
pub fn router(engine: Engine, config: Config) -> Router {
    // A phone opening a pairing QR code GETs /pair: the UI answers it.
    let pair_route = match &config.ui {
        Some(Ui::Dir(dir)) => post(pair).get_service(ServeFile::new(dir.join("index.html"))),
        Some(Ui::Assets(lookup)) => {
            let lookup = lookup.clone();
            post(pair).get(move |uri: Uri| asset(lookup, uri))
        }
        None => post(pair),
    };
    let router = Router::new()
        .route("/ws", get(upgrade))
        .route("/art/{key}", get(art))
        .route("/images", get(images))
        .route("/pair", pair_route)
        .with_state(AppState {
            engine,
            library: config.library,
            integrations: config.integrations,
            pairing: config.pairing,
            art_slots: Arc::new(Semaphore::new(ART_SLOTS)),
        });
    match config.ui {
        Some(Ui::Dir(dir)) => {
            // Unknown paths fall back to index.html so client-side routes work.
            let index = ServeFile::new(dir.join("index.html"));
            router.fallback_service(ServeDir::new(dir).fallback(index))
        }
        Some(Ui::Assets(lookup)) => router.fallback(move |uri: Uri| asset(lookup, uri)),
        None => router,
    }
}

/// A file of a built-in UI ([`Ui::Assets`]).
async fn asset(lookup: AssetLookup, uri: Uri) -> Response {
    match lookup(uri.path().trim_start_matches('/')) {
        Some((bytes, mime)) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(serde::Deserialize)]
struct TokenParam {
    token: Option<String>,
}

async fn upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(params): Query<TokenParam>,
    State(state): State<AppState>,
) -> Response {
    let Some(who) = authorise(&state, peer, &headers, params.token.as_deref()) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    ws.on_upgrade(move |socket| session(socket, state, who, peer))
}

/// Who a request comes from: the device whose token it carries (in
/// `Authorization: Bearer` or, for browsers' `<img>` and WebSocket, a
/// `token` parameter), else this machine if the peer is loopback and the
/// page (if any) is local. `None` is refused.
fn authorise(
    state: &AppState,
    peer: SocketAddr,
    headers: &HeaderMap,
    token: Option<&str>,
) -> Option<Who> {
    if !reachable(state, peer) {
        return None;
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if let Some(token) = bearer.or(token) {
        return state.pairing.authenticate(token).map(Who::Device);
    }
    let origin = headers
        .get(header::ORIGIN)
        .map(|v| v.to_str().unwrap_or(""));
    (peer.ip().to_canonical().is_loopback() && origin_allowed(origin)).then_some(Who::Local)
}

/// Whether `peer` may talk to this engine at all: other machines only while
/// the LAN is open, which also cuts off their kept-alive connections.
fn reachable(state: &AppState, peer: SocketAddr) -> bool {
    peer.ip().to_canonical().is_loopback() || state.pairing.lan_open()
}

/// A new device redeems a pairing code; the answer may wait for the host's
/// approval.
async fn pair(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(request): Json<PairRequest>,
) -> Response {
    if !reachable(&state, peer) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state.pairing.redeem(request, peer).await {
        Ok(paired) => Json(paired).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ImagesParams {
    /// The track's path in the library.
    track: String,
    token: Option<String>,
}

/// Artwork keys for all of a track's images (cover, back, booklet…), its
/// shown art first, for `/art/<key>`. Read by script from the UI's own
/// origin, so it answers the same pages `/ws` does.
async fn images(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(params): Query<ImagesParams>,
) -> Response {
    if authorise(&state, peer, &headers, params.token.as_deref()).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let origin = headers.get(header::ORIGIN).cloned();
    let Some(library) = state.library else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let track = params.track.clone();
    match tokio::task::spawn_blocking(move || library.images(&params.track)).await {
        Ok(Err(err)) => {
            tracing::warn!(track, %err, "can't list images");
            StatusCode::NOT_FOUND.into_response()
        }
        Ok(Ok(keys)) => {
            let mut response = axum::Json(keys).into_response();
            if let Some(origin) = origin {
                response
                    .headers_mut()
                    .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
            }
            response
        }
        Err(err) => {
            tracing::warn!(%err, "listing images failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(serde::Deserialize)]
struct ArtParams {
    /// Fit within this many pixels; full size if absent.
    size: Option<u32>,
    token: Option<String>,
}

/// Artwork by id. Ids are content hashes, so responses never change and the
/// browser may cache them forever.
async fn art(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(key): Path<String>,
    Query(params): Query<ArtParams>,
) -> Response {
    if authorise(&state, peer, &headers, params.token.as_deref()).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let origin = headers.get(header::ORIGIN).cloned();
    let Some(library) = state.library else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let _slot = state.art_slots.acquire().await;
    let image = tokio::task::spawn_blocking(move || library.art_image(&key, params.size)).await;
    match image {
        Ok(Ok((bytes, media_type))) => {
            let headers = [
                (header::CONTENT_TYPE, media_type),
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                ),
                (header::VARY, "origin"),
            ];
            let mut response = (headers, bytes).into_response();
            // The desktop window's page has another origin than this server,
            // and it reads the cover's pixels for art colours. `authorise`
            // has already refused local pages from other origins.
            if let Some(origin) = origin {
                response
                    .headers_mut()
                    .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
            }
            response
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Answers a library query. Play queries set the engine's context.
async fn query(state: &AppState, query: LibraryQuery) -> Result<QueryResult, String> {
    let library = state.library.clone().ok_or("this engine has no library")?;
    if matches!(
        query,
        LibraryQuery::Play { .. }
            | LibraryQuery::PlayAlbum { .. }
            | LibraryQuery::PlayPlaylist { .. }
    ) {
        let command = tokio::task::spawn_blocking(move || library.selection(&query))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        state
            .engine
            .send(command)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(QueryResult::Done);
    }
    let lrclib = state
        .integrations
        .borrow()
        .lrclib
        .then_some(library::LRCLIB);
    tokio::task::spawn_blocking(move || match query {
        LibraryQuery::Lyrics { path } => library
            .lyrics(&path, lrclib)
            .map(|lyrics| QueryResult::Lyrics { lyrics }),
        LibraryQuery::SaveLyrics { path, offset_ms } => library
            .save_lyrics(&path, offset_ms, lrclib)
            .map(|()| QueryResult::Done),
        query => library.query(query),
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// The scope a message needs; `None` if any client may send it.
fn required_scope(message: &ClientMessage) -> Option<Scope> {
    match message {
        ClientMessage::Command { command, .. } => Some(match command {
            Command::Play
            | Command::Pause
            | Command::TogglePlayback
            | Command::Stop
            | Command::Next
            | Command::Previous
            | Command::Seek { .. }
            | Command::SetVolume { .. }
            | Command::SetRepeat { .. }
            | Command::SetContext { .. }
            | Command::PlayItem { .. } => Scope::Playback,
            Command::Enqueue { .. }
            | Command::EnqueueNext { .. }
            | Command::RemoveItem { .. }
            | Command::MoveItem { .. }
            | Command::ClearQueue => Scope::Queue,
            Command::SetDecoders { .. }
            | Command::SetOutput { .. }
            | Command::ListOutputs
            | Command::SetReplayGain { .. } => Scope::Settings,
        }),
        ClientMessage::Query { query, .. } => match query {
            LibraryQuery::Status
            | LibraryQuery::Tracks { .. }
            | LibraryQuery::Groups { .. }
            | LibraryQuery::Albums { .. }
            | LibraryQuery::RecentlyPlayed { .. }
            | LibraryQuery::RecentlyPlayedAlbums { .. }
            | LibraryQuery::AlbumPosition { .. }
            | LibraryQuery::Lookup { .. }
            | LibraryQuery::AlbumTracks { .. }
            | LibraryQuery::Waveform { .. }
            | LibraryQuery::Lyrics { .. }
            | LibraryQuery::Playlists
            | LibraryQuery::PlaylistTracks { .. } => None,
            LibraryQuery::Play { .. }
            | LibraryQuery::PlayAlbum { .. }
            | LibraryQuery::PlayPlaylist { .. } => Some(Scope::Playback),
            LibraryQuery::AddRoot { .. }
            | LibraryQuery::RemoveRoot { .. }
            | LibraryQuery::Rescan
            | LibraryQuery::SetWatching { .. }
            | LibraryQuery::SaveLyrics { .. }
            | LibraryQuery::CreatePlaylist { .. }
            | LibraryQuery::RenamePlaylist { .. }
            | LibraryQuery::DeletePlaylist { .. }
            | LibraryQuery::AddToPlaylist { .. }
            | LibraryQuery::RemoveFromPlaylist { .. }
            | LibraryQuery::MovePlaylistItem { .. } => Some(Scope::Library),
        },
        ClientMessage::Subscribe { .. } => None,
        ClientMessage::SetIntegrations { .. } => Some(Scope::Settings),
        ClientMessage::Pairing { .. } => Some(Scope::Pairing),
    }
}

/// Browsers let any website open a WebSocket to localhost, so a page must come
/// from this machine or be the Tauri window. Clients that send no `Origin`
/// are not browsers and can't be driven by a website. Only asked for
/// loopback peers: anyone else could fake the header.
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
    let Ok(uri) = origin.parse::<Uri>() else {
        return false;
    };
    uri.scheme_str() == Some("tauri")
        || matches!(
            uri.host(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost")
        )
}

/// One connected client: forwards state changes out and commands in.
async fn session(socket: WebSocket, state: AppState, who: Who, peer: SocketAddr) {
    let engine = state.engine.clone();
    let (connection, close) = state.pairing.connected(&who, peer);
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<ServerMessage>(32);

    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let json = serde_json::to_string(&msg).expect("protocol types serialise");
            if sink.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    let hello = ServerMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        engine_version: env!("CARGO_PKG_VERSION").into(),
        now_ms: engine::wall_clock_ms(),
    };
    let _ = out_tx.send(hello).await;

    let mut states = engine.subscribe();
    let mut queue = engine.subscribe_queue();
    let mut context = engine.subscribe_context();
    states.mark_changed();
    queue.mark_changed();
    context.mark_changed();
    let mut integrations = state.integrations.subscribe();
    integrations.mark_changed();
    // Without a library, a channel that never changes stands in.
    let (no_library, mut library) = tokio::sync::watch::channel(Default::default());
    if let Some(lib) = &state.library {
        library = lib.subscribe();
        library.mark_changed();
    }
    let mut pairing = state.pairing.subscribe();
    pairing.mark_changed();
    let (pairing_handle, publisher_who) = (state.pairing.clone(), who.clone());
    let event_tx = out_tx.clone();
    let publisher = tokio::spawn(async move {
        let _no_library = no_library;
        // What this connection may do, as last sent.
        let mut scopes: Option<Vec<Scope>> = None;
        loop {
            // Biased towards the entries: the engine updates them before the
            // state that refers to them.
            let event = tokio::select! {
                biased;
                Ok(()) = context.changed() => {
                    let window = context.borrow_and_update().clone();
                    Event::Context { offset: window.offset, items: window.items }
                }
                Ok(()) = queue.changed() => Event::Queue { items: queue.borrow_and_update().clone() },
                Ok(()) = states.changed() => Event::State { state: states.borrow_and_update().clone() },
                Ok(()) = library.changed() => Event::Library { status: library.borrow_and_update().clone() },
                Ok(()) = integrations.changed() => Event::Integrations { settings: integrations.borrow_and_update().clone() },
                Ok(()) = pairing.changed() => {
                    let state = pairing.borrow_and_update().clone();
                    let now = pairing_handle.scopes(&publisher_who);
                    if scopes.as_ref() != Some(&now) {
                        scopes = Some(now.clone());
                        let event = Event::Access { scopes: now.clone() };
                        if event_tx.send(ServerMessage::Event { event }).await.is_err() {
                            break;
                        }
                    }
                    if !now.contains(&Scope::Pairing) {
                        continue;
                    }
                    Event::Pairing { state }
                }
                else => break,
            };
            if event_tx.send(ServerMessage::Event { event }).await.is_err() {
                break;
            }
        }
    });

    // Forwards visualiser bands while subscribed; dropping it unsubscribes.
    let mut bands: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let msg = tokio::select! {
            msg = stream.next() => msg,
            () = close.notified() => break,
        };
        let text = match msg {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            _ => continue,
        };
        let message = serde_json::from_str::<ClientMessage>(&text);
        if let Ok(message) = &message
            && let Some(scope) = required_scope(message)
            && !state.pairing.allows(&who, scope)
        {
            let error = Some(format!("not allowed: needs the {scope:?} permission"));
            let reply = match *message {
                ClientMessage::Query { id, .. } => ServerMessage::QueryResult {
                    id,
                    error,
                    result: None,
                },
                ClientMessage::Command { id, .. }
                | ClientMessage::Subscribe { id, .. }
                | ClientMessage::SetIntegrations { id, .. }
                | ClientMessage::Pairing { id, .. } => ServerMessage::Response { id, error },
            };
            if out_tx.send(reply).await.is_err() {
                break;
            }
            continue;
        }
        let reply = match message {
            Ok(ClientMessage::Command { id, command }) => ServerMessage::Response {
                id,
                error: engine.send(command).await.err().map(|e| e.to_string()),
            },
            Ok(ClientMessage::Query { id, query: q }) => {
                // Queries can take a moment; don't hold up commands behind them.
                let (state, out_tx) = (state.clone(), out_tx.clone());
                tokio::spawn(async move {
                    let (result, error) = match query(&state, q).await {
                        Ok(result) => (Some(result), None),
                        Err(error) => (None, Some(error)),
                    };
                    let _ = out_tx
                        .send(ServerMessage::QueryResult { id, error, result })
                        .await;
                });
                continue;
            }
            Ok(ClientMessage::Subscribe { id, bands: on }) => {
                if let Some(task) = bands.take() {
                    task.abort();
                }
                if on {
                    let mut rx = engine.subscribe_bands();
                    let out_tx = out_tx.clone();
                    bands = Some(tokio::spawn(async move {
                        while rx.changed().await.is_ok() {
                            let event = Event::Bands {
                                bands: rx.borrow_and_update().clone(),
                            };
                            if out_tx.send(ServerMessage::Event { event }).await.is_err() {
                                break;
                            }
                        }
                    }));
                }
                ServerMessage::Response { id, error: None }
            }
            Ok(ClientMessage::SetIntegrations { id, changes }) => {
                state.integrations.send_if_modified(|settings| {
                    let before = settings.clone();
                    if let Some(on) = changes.lrclib {
                        settings.lrclib = on;
                    }
                    if let Some(on) = changes.discord {
                        settings.discord = on;
                    }
                    for (new, old) in [
                        (changes.listenbrainz, &mut settings.listenbrainz),
                        (changes.lastfm, &mut settings.lastfm),
                        (changes.librefm, &mut settings.librefm),
                    ] {
                        if let Some(new) = new {
                            old.enabled = new.enabled;
                            old.sign_in = new.sign_in;
                        }
                    }
                    *settings != before
                });
                ServerMessage::Response { id, error: None }
            }
            Ok(ClientMessage::Pairing { id, pairing }) => ServerMessage::Response {
                id,
                error: state.pairing.command(pairing).err(),
            },
            Err(err) => {
                tracing::debug!(%err, "malformed client message");
                ServerMessage::Response {
                    id: 0,
                    error: Some(format!("malformed message: {err}")),
                }
            }
        };
        if out_tx.send(reply).await.is_err() {
            break;
        }
    }

    state.pairing.disconnected(connection);
    publisher.abort();
    if let Some(task) = bands {
        task.abort();
    }
    drop(out_tx);
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Command, PlaybackStatus, TrackRef};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

    async fn next_message(
        ws: &mut (
                 impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin
             ),
    ) -> ServerMessage {
        loop {
            if let WsMessage::Text(text) = ws.next().await.unwrap().unwrap() {
                return serde_json::from_str(&text).unwrap();
            }
        }
    }

    #[test]
    fn viewing_playlists_is_a_read_and_editing_needs_the_library_scope() {
        let query = |query| required_scope(&ClientMessage::Query { id: 1, query });
        assert_eq!(query(LibraryQuery::Playlists), None);
        assert_eq!(query(LibraryQuery::PlaylistTracks { id: "p".into() }), None);
        assert_eq!(
            query(LibraryQuery::PlayPlaylist {
                id: "p".into(),
                start: 0
            }),
            Some(Scope::Playback)
        );
        assert_eq!(
            query(LibraryQuery::DeletePlaylist { id: "p".into() }),
            Some(Scope::Library)
        );
    }

    #[tokio::test]
    async fn websocket_round_trip() {
        let engine = Engine::spawn(engine::LocalFiles);
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: watch::channel(Default::default()).0,
            pairing: Pairing::in_memory(),
        };
        let (addr, serve) = bind(engine, config).await.unwrap();
        tokio::spawn(serve);

        let (mut ws, _) = connect_async(format!("ws://{addr}/ws")).await.unwrap();
        assert!(matches!(
            next_message(&mut ws).await,
            ServerMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                ..
            }
        ));
        assert!(matches!(
            next_message(&mut ws).await,
            ServerMessage::Event {
                event: Event::Context { .. }
            }
        ));
        assert!(matches!(
            next_message(&mut ws).await,
            ServerMessage::Event {
                event: Event::Queue { .. }
            }
        ));

        for (id, command) in [
            (
                1,
                Command::Enqueue {
                    tracks: vec![TrackRef {
                        source: "demo".into(),
                        id: "/a.flac".into(),
                        ..Default::default()
                    }],
                },
            ),
            (2, Command::Play),
        ] {
            let json = serde_json::to_string(&ClientMessage::Command { id, command }).unwrap();
            ws.send(WsMessage::Text(json.into())).await.unwrap();
        }

        let mut responses = 0;
        let mut playing = false;
        while responses < 2 || !playing {
            match next_message(&mut ws).await {
                ServerMessage::Response { error, .. } => {
                    assert_eq!(error, None);
                    responses += 1;
                }
                ServerMessage::Event {
                    event: Event::State { state },
                } => playing = state.status == PlaybackStatus::Playing,
                ServerMessage::Event {
                    event:
                        Event::Queue { .. }
                        | Event::Context { .. }
                        | Event::Integrations { .. }
                        | Event::Access { .. }
                        | Event::Pairing { .. },
                } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn subscribing_to_bands_is_acknowledged() {
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: watch::channel(Default::default()).0,
            pairing: Pairing::in_memory(),
        };
        let (addr, serve) = bind(Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        let (mut ws, _) = connect_async(format!("ws://{addr}/ws")).await.unwrap();
        for (id, on) in [(1, true), (2, false)] {
            let json = serde_json::to_string(&ClientMessage::Subscribe { id, bands: on }).unwrap();
            assert_eq!(json, format!(r#"{{"id":{id},"bands":{on}}}"#));
            ws.send(WsMessage::Text(json.into())).await.unwrap();
            loop {
                if let ServerMessage::Response { id: got, error } = next_message(&mut ws).await {
                    assert_eq!((got, error), (id, None));
                    break;
                }
            }
        }
    }

    #[tokio::test]
    async fn integration_settings_reach_every_client() {
        let (integrations, _) = watch::channel(IntegrationSettings::default());
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: integrations.clone(),
            pairing: Pairing::in_memory(),
        };
        let (addr, serve) = bind(Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        let url = format!("ws://{addr}/ws");
        let (mut a, _) = connect_async(&url).await.unwrap();
        let (mut b, _) = connect_async(&url).await.unwrap();
        async fn settings(
            ws: &mut (
                     impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>>
                     + Unpin
                 ),
        ) -> IntegrationSettings {
            loop {
                if let ServerMessage::Event {
                    event: Event::Integrations { settings },
                } = next_message(ws).await
                {
                    return settings;
                }
            }
        }
        assert!(settings(&mut b).await.lrclib);
        let send = |json: &str| WsMessage::Text(json.to_owned().into());
        a.send(send(
            r#"{"id":1,"changes":{"lrclib":false,"secrets_unencrypted":true,
                "listenbrainz":{"enabled":true,"user":"mallory"}}}"#,
        ))
        .await
        .unwrap();
        let got = settings(&mut b).await;
        assert!(!got.lrclib);
        assert!(
            !got.secrets_unencrypted,
            "only the host says where secrets are"
        );
        assert_eq!(got.listenbrainz.user, None, "only the host signs in");
        assert!(got.listenbrainz.enabled);

        // The host signs in to Last.fm while a's copy still says it is off:
        // a's next change mustn't turn it off again.
        integrations.send_modify(|s| s.lastfm.enabled = true);
        assert!(settings(&mut b).await.lastfm.enabled);
        a.send(send(r#"{"id":2,"changes":{"discord":true}}"#))
            .await
            .unwrap();
        let got = settings(&mut b).await;
        assert!(got.discord);
        assert!(got.lastfm.enabled && !got.lrclib, "the rest is kept");
    }

    /// Serves to clients that all appear to come from another machine, with
    /// the LAN open.
    async fn lan_server(pairing: Pairing) -> SocketAddr {
        pairing.set_lan(Some(Vec::new()));
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: watch::channel(Default::default()).0,
            pairing,
        };
        let app = router(Engine::spawn(engine::LocalFiles), config).layer(
            axum::extract::connect_info::MockConnectInfo(SocketAddr::from((
                [192, 168, 1, 20],
                5000,
            ))),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        addr
    }

    async fn token(pairing: &Pairing, preset: protocol::Preset) -> String {
        pairing
            .command(protocol::PairingCommand::SetAutoApprove {
                preset: Some(preset),
            })
            .unwrap();
        let request = PairRequest {
            code: pairing.start().code,
            name: "Phone".into(),
            public_key: None,
        };
        let peer = "192.168.1.20:5000".parse().unwrap();
        pairing.redeem(request, peer).await.unwrap().token
    }

    async fn http_status(addr: SocketAddr, path: &str) -> u16 {
        http(addr, "GET", path, "").await[9..12].parse().unwrap()
    }

    /// The whole response, headers and body.
    async fn http(addr: SocketAddr, method: &str, path: &str, body: &str) -> String {
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: motif\r\nConnection: close\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        tokio::task::spawn_blocking(move || {
            use std::io::{Read, Write};
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        })
        .await
        .unwrap()
    }

    /// Waits until the LAN is `open` or not.
    async fn lan_open(pairing: &Pairing, open: bool) -> protocol::LanState {
        let mut states = pairing.subscribe();
        let state = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            states.wait_for(|s| s.lan.open == open),
        );
        state.await.unwrap().unwrap().lan.clone()
    }

    // 127.0.0.2 is loopback too, but a socket bound to 127.0.0.1 doesn't
    // answer it: only one on every interface does (on Linux).
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_lan_opens_and_closes_at_runtime() {
        let pairing = Pairing::in_memory();
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: watch::channel(Default::default()).0,
            pairing: pairing.clone(),
        };
        let (addr, serve) = bind(Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        let other = SocketAddr::from(([127, 0, 0, 2], addr.port()));
        let reachable = || async move { tokio::net::TcpStream::connect(other).await.is_ok() };
        assert!(!reachable().await, "loopback only at first");

        let code = pairing.start().code;
        let lan = lan_open(&pairing, true).await;
        assert!(reachable().await);
        assert!(lan.allowed);
        for (address, url) in lan.addresses.iter().zip(&lan.pair_urls) {
            assert!(url.starts_with(&format!("http://{address}/pair?code={code}&id=")));
        }

        // Nothing left to pair: the LAN closes, the switch stays on.
        pairing.command(protocol::PairingCommand::Cancel).unwrap();
        assert!(lan_open(&pairing, false).await.allowed);
        assert!(!reachable().await);
        let (mut ws, _) = connect_async(format!("ws://{addr}/ws")).await.unwrap();
        assert!(matches!(
            next_message(&mut ws).await,
            ServerMessage::Hello { .. }
        ));

        // A paired device keeps it open until the switch closes it.
        token(&pairing, protocol::Preset::Full).await;
        lan_open(&pairing, true).await;
        pairing.command(protocol::PairingCommand::Cancel).unwrap();
        assert!(reachable().await, "a device is paired");
        pairing.allow_lan(false);
        let lan = lan_open(&pairing, false).await;
        assert!(!lan.allowed && lan.addresses.is_empty());
        assert!(!reachable().await);
        assert_eq!(
            pairing.subscribe().borrow().devices.len(),
            1,
            "nobody is revoked"
        );
    }

    /// Moving the listener mustn't reset connections waiting to be accepted
    /// (#141), such as the desktop UI's own reconnecting WebSocket.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn moving_the_listener_resets_no_connection() {
        let pairing = Pairing::in_memory();
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: watch::channel(Default::default()).0,
            pairing: pairing.clone(),
        };
        let (addr, serve) = bind(Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let client = tokio::task::spawn_blocking({
            let done = done.clone();
            move || {
                use std::io::{ErrorKind, Read, Write};
                let (mut served, mut refused, mut reset) = (0, 0, Vec::new());
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    // A handshake under way as the old socket closes, or one in
                    // the moment before the new one binds, fails at once
                    // (refused or reset) and the client retries: unavoidable
                    // without SO_REUSEPORT. A reset after any response byte
                    // still fails the test.
                    let mut stream = match std::net::TcpStream::connect(addr) {
                        Ok(stream) => stream,
                        Err(err)
                            if matches!(
                                err.kind(),
                                ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset
                            ) =>
                        {
                            refused += 1;
                            continue;
                        }
                        Err(err) => panic!("{err}"),
                    };
                    let limit = Some(std::time::Duration::from_secs(5));
                    stream.set_read_timeout(limit).unwrap();
                    let mut response = Vec::new();
                    let got = stream
                        .write_all(
                            b"GET /nothing HTTP/1.1\r\nHost: motif\r\nConnection: close\r\n\r\n",
                        )
                        .and_then(|()| stream.read_to_end(&mut response));
                    match got {
                        Ok(_) if response.starts_with(b"HTTP/1.1") => served += 1,
                        Ok(_) => reset.push("closed without a response".to_owned()),
                        // Completed on the old socket after its backlog was
                        // drained, before it closed (#141's gap): the server
                        // never read it, so the client retries as if refused.
                        Err(err)
                            if response.is_empty()
                                && matches!(
                                    err.kind(),
                                    ErrorKind::ConnectionReset | ErrorKind::BrokenPipe
                                ) =>
                        {
                            refused += 1
                        }
                        Err(err) => reset.push(err.to_string()),
                    }
                }
                (served, refused, reset)
            }
        });
        for _ in 0..100 {
            pairing.start();
            lan_open(&pairing, true).await;
            pairing.command(protocol::PairingCommand::Cancel).unwrap();
            lan_open(&pairing, false).await;
        }
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        let client = tokio::time::timeout(std::time::Duration::from_secs(10), client);
        let (served, refused, reset) = client.await.expect("the client hangs").unwrap();
        eprintln!(
            "served {served}, failed to connect {refused}, reset {}",
            reset.len()
        );
        assert!(served > 0);
        assert_eq!(reset, Vec::<String>::new());
    }

    #[tokio::test]
    async fn closing_the_lan_cuts_other_machines_off() {
        let pairing = Pairing::in_memory();
        let addr = lan_server(pairing.clone()).await;
        let token = token(&pairing, protocol::Preset::Full).await;
        pairing.set_lan(None);
        assert_eq!(
            http_status(addr, &format!("/images?track=a&token={token}")).await,
            401
        );
    }

    #[tokio::test]
    async fn a_pairing_link_opens_the_ui() {
        let dir = std::env::temp_dir().join(format!("motif-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<p>Motif</p>").unwrap();
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: Some(Ui::Dir(dir.clone())),
            library: None,
            integrations: watch::channel(Default::default()).0,
            pairing: Pairing::in_memory(),
        };
        let (addr, serve) = bind(Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        assert_eq!(http_status(addr, "/pair?code=ABCD&id=00").await, 200);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_built_in_ui_answers_pairing_links() {
        let files = |path: &str| -> Option<(Vec<u8>, String)> {
            match path {
                "assets/app.js" => Some((b"run()".to_vec(), "text/javascript".into())),
                _ => Some((b"<p>Motif</p>".to_vec(), "text/html".into())),
            }
        };
        let pairing = Pairing::in_memory();
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: Some(Ui::Assets(Arc::new(files))),
            library: None,
            integrations: watch::channel(Default::default()).0,
            pairing: pairing.clone(),
        };
        let (addr, serve) = bind(Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        for (path, mime, body) in [
            ("/pair?code=ABCD&id=00", "text/html", "<p>Motif</p>"),
            ("/assets/app.js", "text/javascript", "run()"),
            ("/some/route", "text/html", "<p>Motif</p>"),
        ] {
            let response = http(addr, "GET", path, "").await.to_lowercase();
            assert!(response.starts_with("http/1.1 200"), "{path}: {response}");
            assert!(
                response.contains(&format!("content-type: {mime}")),
                "{path}"
            );
            assert!(response.ends_with(&body.to_lowercase()), "{path}");
        }
        pairing
            .command(protocol::PairingCommand::SetAutoApprove {
                preset: Some(protocol::Preset::Full),
            })
            .unwrap();
        let code = pairing.start().code;
        // The code opens the LAN: don't connect while the listener moves.
        lan_open(&pairing, true).await;
        let response = http(
            addr,
            "POST",
            "/pair",
            &format!(r#"{{"code":"{code}","name":"Phone"}}"#),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains(r#""token":"#));
    }

    #[tokio::test]
    async fn other_machines_need_a_token() {
        let pairing = Pairing::in_memory();
        let addr = lan_server(pairing.clone()).await;
        assert!(connect_async(format!("ws://{addr}/ws")).await.is_err());
        assert!(
            connect_async(format!("ws://{addr}/ws?token=guess"))
                .await
                .is_err()
        );
        assert_eq!(http_status(addr, "/art/abc").await, 401);
        assert_eq!(http_status(addr, "/images?track=a").await, 401);

        let token = token(&pairing, protocol::Preset::ViewOnly).await;
        let (mut ws, _) = connect_async(format!("ws://{addr}/ws?token={token}"))
            .await
            .unwrap();
        assert!(matches!(
            next_message(&mut ws).await,
            ServerMessage::Hello { .. }
        ));
        // Past the auth check: this engine has no library.
        assert_eq!(
            http_status(addr, &format!("/art/abc?token={token}")).await,
            404
        );
    }

    #[tokio::test]
    async fn view_only_devices_cant_control_playback() {
        let pairing = Pairing::in_memory();
        let addr = lan_server(pairing.clone()).await;
        let token = token(&pairing, protocol::Preset::ViewOnly).await;
        let (mut ws, _) = connect_async(format!("ws://{addr}/ws?token={token}"))
            .await
            .unwrap();
        for (id, json) in [
            (1, r#"{"id":1,"command":{"type":"play"}}"#),
            (2, r#"{"id":2,"pairing":{"type":"start"}}"#),
        ] {
            ws.send(WsMessage::Text(json.into())).await.unwrap();
            loop {
                match next_message(&mut ws).await {
                    ServerMessage::Response { id: got, error } => {
                        assert_eq!(got, id);
                        assert!(error.unwrap().starts_with("not allowed"));
                        break;
                    }
                    ServerMessage::Event {
                        event: Event::Pairing { .. },
                    } => panic!("pairing state is for Full devices"),
                    _ => {}
                }
            }
        }
        assert_eq!(pairing.subscribe().borrow().code, None);
    }

    #[tokio::test]
    async fn devices_hear_what_they_may_do() {
        let pairing = Pairing::in_memory();
        let addr = lan_server(pairing.clone()).await;
        let token = token(&pairing, protocol::Preset::ViewOnly).await;
        let (mut ws, _) = connect_async(format!("ws://{addr}/ws?token={token}"))
            .await
            .unwrap();
        let mut access = async || loop {
            if let ServerMessage::Event {
                event: Event::Access { scopes },
            } = next_message(&mut ws).await
            {
                break scopes;
            }
        };
        assert_eq!(access().await, vec![]);
        let device = pairing.subscribe().borrow().devices[0].id.clone();
        pairing
            .command(protocol::PairingCommand::SetPreset {
                device,
                preset: protocol::Preset::ControlPlayback,
            })
            .unwrap();
        assert_eq!(access().await, protocol::Preset::ControlPlayback.scopes());
    }

    #[tokio::test]
    async fn revoking_a_device_closes_its_connection() {
        let pairing = Pairing::in_memory();
        let addr = lan_server(pairing.clone()).await;
        let token = token(&pairing, protocol::Preset::Full).await;
        let (mut ws, _) = connect_async(format!("ws://{addr}/ws?token={token}"))
            .await
            .unwrap();
        let device = loop {
            if let ServerMessage::Event {
                event: Event::Pairing { state },
            } = next_message(&mut ws).await
                && let Some(connection) = state.connected.first()
            {
                break connection.device.clone().unwrap();
            }
        };
        pairing
            .command(protocol::PairingCommand::Revoke { device })
            .unwrap();
        let closed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(Ok(_)) = ws.next().await {}
        })
        .await;
        assert!(closed.is_ok(), "the socket closes");
        assert!(
            connect_async(format!("ws://{addr}/ws?token={token}"))
                .await
                .is_err()
        );
        let state = pairing.subscribe();
        let state = state.borrow();
        assert!(state.devices.is_empty() && state.connected.is_empty());
    }

    #[test]
    fn only_local_pages_may_connect() {
        for origin in [
            None,
            Some("http://localhost:5173"),
            Some("http://127.0.0.1:7700"),
            Some("http://[::1]:7700"),
            Some("tauri://localhost"),
            Some("http://tauri.localhost"),
        ] {
            assert!(origin_allowed(origin), "{origin:?}");
        }
        for origin in [
            "https://evil.example",
            "http://localhost.evil.example",
            "http://evil.example:7700",
            "null",
        ] {
            assert!(!origin_allowed(Some(origin)), "{origin}");
        }
    }
}
