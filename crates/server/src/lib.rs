//! Exposes an [`Engine`] over WebSocket at `/ws` and serves the web UI.
//!
//! Phase 0 has no authentication, so callers must bind to loopback only, and
//! browsers may only connect from pages served by this machine.
//! Pairing tokens arrive in phase 4 before the server is exposed on the LAN.

use std::{net::SocketAddr, path::PathBuf};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, Uri, header::ORIGIN},
    response::{IntoResponse, Response},
    routing::get,
};
use engine::Engine;
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, Event, PROTOCOL_VERSION, ServerMessage};
use tokio::{net::TcpListener, sync::mpsc};
use tower_http::services::{ServeDir, ServeFile};

pub struct Config {
    pub addr: SocketAddr,
    /// Built web UI (`apps/ui/dist`). `None` serves only `/ws`.
    pub ui_dir: Option<PathBuf>,
}

/// Binds the listener and returns its address plus a future that serves it.
/// Binding first lets callers use port 0 and learn the real port.
pub async fn bind(
    engine: Engine,
    config: Config,
) -> std::io::Result<(SocketAddr, impl Future<Output = std::io::Result<()>>)> {
    let listener = TcpListener::bind(config.addr).await?;
    let addr = listener.local_addr()?;
    let app = router(engine, config.ui_dir);
    Ok((addr, async move { axum::serve(listener, app).await }))
}

pub fn router(engine: Engine, ui_dir: Option<PathBuf>) -> Router {
    let router = Router::new().route("/ws", get(upgrade)).with_state(engine);
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
    State(engine): State<Engine>,
) -> Response {
    if !origin_allowed(headers.get(ORIGIN).map(|v| v.to_str().unwrap_or(""))) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| session(socket, engine))
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
async fn session(socket: WebSocket, engine: Engine) {
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
    };
    let _ = out_tx.send(hello).await;

    let mut states = engine.subscribe();
    let mut queue = engine.subscribe_queue();
    states.mark_changed();
    queue.mark_changed();
    let event_tx = out_tx.clone();
    let publisher = tokio::spawn(async move {
        loop {
            // Biased towards the queue: the engine updates it before the
            // state that indexes into it.
            let event = tokio::select! {
                biased;
                Ok(()) = queue.changed() => Event::Queue { items: queue.borrow_and_update().clone() },
                Ok(()) = states.changed() => Event::State { state: states.borrow_and_update().clone() },
                else => break,
            };
            if event_tx.send(ServerMessage::Event { event }).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        let reply = match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage { id, command }) => ServerMessage::Response {
                id,
                error: engine.send(command).await.err().map(|e| e.to_string()),
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

    publisher.abort();
    drop(out_tx);
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Command, PlaybackStatus, TrackInfo};
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
        let engine = Engine::spawn();
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui_dir: None,
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
                event: Event::Queue { .. }
            }
        ));

        for (id, command) in [
            (
                1,
                Command::Enqueue {
                    tracks: vec![TrackInfo {
                        uri: "file:///a.flac".into(),
                        ..Default::default()
                    }],
                },
            ),
            (2, Command::Play),
        ] {
            let json = serde_json::to_string(&ClientMessage { id, command }).unwrap();
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
                    event: Event::Queue { .. },
                } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
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
