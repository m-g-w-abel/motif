//! Exposes an [`Engine`] and the [`Library`] over WebSocket at `/ws`, serves
//! artwork at `/art/<id>` (a track's other images listed at `/images`) and
//! the web UI.
//!
//! Phase 0 has no authentication, so callers must bind to loopback only, and
//! browsers may only connect from pages served by this machine.
//! Pairing tokens arrive in phase 4 before the server is exposed on the LAN.

mod resolver;
pub mod settings;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use engine::Engine;
use futures_util::{SinkExt, StreamExt};
use library::Library;
use protocol::{
    ClientMessage, Event, IntegrationSettings, LibraryQuery, PROTOCOL_VERSION, QueryResult,
    ServerMessage,
};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, mpsc, watch},
};
use tower_http::services::{ServeDir, ServeFile};

pub use resolver::LibraryResolver;

pub struct Config {
    pub addr: SocketAddr,
    /// Built web UI (`apps/ui/dist`). `None` serves only `/ws`.
    pub ui_dir: Option<PathBuf>,
    pub library: Option<Library>,
    /// The host's integration settings, which clients read and replace.
    pub integrations: watch::Sender<IntegrationSettings>,
}

#[derive(Clone)]
struct AppState {
    engine: Engine,
    library: Option<Library>,
    integrations: watch::Sender<IntegrationSettings>,
    /// Limits artwork requests in flight. Each can hold a whole image or a
    /// decoded one (tens of MB), and a grid asks for hundreds at once.
    art_slots: Arc<Semaphore>,
}

/// Artwork requests served at once: enough to fill a screen of covers
/// without waiting, while capping the memory peak of a large grid.
const ART_SLOTS: usize = 50;

/// Binds the listener and returns its address plus a future that serves it.
/// Binding first lets callers use port 0 and learn the real port.
pub async fn bind(
    engine: Engine,
    config: Config,
) -> std::io::Result<(SocketAddr, impl Future<Output = std::io::Result<()>>)> {
    let listener = TcpListener::bind(config.addr).await?;
    let addr = listener.local_addr()?;
    let app = router(engine, config.library, config.integrations, config.ui_dir);
    Ok((addr, async move { axum::serve(listener, app).await }))
}

pub fn router(
    engine: Engine,
    library: Option<Library>,
    integrations: watch::Sender<IntegrationSettings>,
    ui_dir: Option<PathBuf>,
) -> Router {
    let router = Router::new()
        .route("/ws", get(upgrade))
        .route("/art/{key}", get(art))
        .route("/images", get(images))
        .with_state(AppState {
            engine,
            library,
            integrations,
            art_slots: Arc::new(Semaphore::new(ART_SLOTS)),
        });
    match ui_dir {
        Some(dir) => {
            // Unknown paths fall back to index.html so client-side routes work.
            let index = ServeFile::new(dir.join("index.html"));
            router.fallback_service(ServeDir::new(dir).fallback(index))
        }
        None => router,
    }
}

async fn upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    if !origin_allowed(
        headers
            .get(header::ORIGIN)
            .map(|v| v.to_str().unwrap_or("")),
    ) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| session(socket, state))
}

#[derive(serde::Deserialize)]
struct ImagesParams {
    /// The track's path in the library.
    track: String,
}

/// Artwork keys for all of a track's images (cover, back, booklet…), its
/// shown art first, for `/art/<key>`. Read by script from the UI's own
/// origin, so it answers the same pages `/ws` does.
async fn images(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ImagesParams>,
) -> Response {
    let origin = headers.get(header::ORIGIN).cloned();
    if !origin_allowed(origin.as_ref().map(|v| v.to_str().unwrap_or(""))) {
        return StatusCode::FORBIDDEN.into_response();
    }
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
}

/// Artwork by id. Ids are content hashes, so responses never change and the
/// browser may cache them forever.
async fn art(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Query(params): Query<ArtParams>,
) -> Response {
    let origin = headers.get(header::ORIGIN).cloned();
    let Some(library) = state.library else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let _slot = state.art_slots.acquire().await;
    let file = tokio::task::spawn_blocking(move || {
        let path = library.art(&key, params.size)?;
        Ok::<_, library::Error>((std::fs::read(&path)?, path))
    })
    .await;
    match file {
        Ok(Ok((bytes, path))) => {
            let png = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"));
            let headers = [
                (
                    header::CONTENT_TYPE,
                    if png { "image/png" } else { "image/jpeg" },
                ),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                (header::VARY, "origin"),
            ];
            let mut response = (headers, bytes).into_response();
            // The desktop window's page has another origin than this server,
            // and it reads the cover's pixels for art colours.
            if let Some(origin) = origin.filter(|o| origin_allowed(Some(o.to_str().unwrap_or(""))))
            {
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
        LibraryQuery::Play { .. } | LibraryQuery::PlayAlbum { .. }
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

/// Browsers let any website open a WebSocket to localhost, so a page must come
/// from this machine or be the Tauri window. Clients that send no `Origin`
/// are not browsers and can't be driven by a website.
// shortcut: trusts every local origin (other localhost apps too), replace with pairing tokens in phase 4.
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
async fn session(socket: WebSocket, state: AppState) {
    let engine = state.engine.clone();
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
    let event_tx = out_tx.clone();
    let publisher = tokio::spawn(async move {
        let _no_library = no_library;
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
                else => break,
            };
            if event_tx.send(ServerMessage::Event { event }).await.is_err() {
                break;
            }
        }
    });

    // Forwards visualiser bands while subscribed; dropping it unsubscribes.
    let mut bands: Option<tokio::task::JoinHandle<()>> = None;
    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        let reply = match serde_json::from_str::<ClientMessage>(&text) {
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
            Ok(ClientMessage::SetIntegrations {
                id,
                integrations: mut new,
            }) => {
                state.integrations.send_if_modified(|settings| {
                    new.secrets_unencrypted = settings.secrets_unencrypted;
                    new.listenbrainz.user = settings.listenbrainz.user.clone();
                    new.listenbrainz.error = settings.listenbrainz.error.clone();
                    let changed = *settings != new;
                    *settings = new;
                    changed
                });
                ServerMessage::Response { id, error: None }
            }
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

    #[tokio::test]
    async fn websocket_round_trip() {
        let engine = Engine::spawn(engine::LocalFiles);
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui_dir: None,
            library: None,
            integrations: watch::channel(Default::default()).0,
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
                    event: Event::Queue { .. } | Event::Context { .. } | Event::Integrations { .. },
                } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn subscribing_to_bands_is_acknowledged() {
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui_dir: None,
            library: None,
            integrations: watch::channel(Default::default()).0,
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
            ui_dir: None,
            library: None,
            integrations,
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
        let new = IntegrationSettings {
            lrclib: false,
            listenbrainz: protocol::ScrobbleAccount {
                user: Some("mallory".into()),
                ..Default::default()
            },
            secrets_unencrypted: true,
            discord: false,
        };
        let json = serde_json::to_string(&ClientMessage::SetIntegrations {
            id: 1,
            integrations: new,
        })
        .unwrap();
        a.send(WsMessage::Text(json.into())).await.unwrap();
        let got = settings(&mut b).await;
        assert!(!got.lrclib);
        assert!(
            !got.secrets_unencrypted,
            "only the host says where secrets are"
        );
        assert_eq!(got.listenbrainz.user, None, "only the host signs in");
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
