//! Desktop (and later Android) shell.
//!
//! Runs the engine and its WebSocket server in-process on a loopback port.
//! The window's UI then connects to it exactly as a remote client would; the
//! only Tauri-specific call is [`engine_url`], which tells it where to connect.

use std::net::SocketAddr;

use engine::Engine;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

struct EngineUrl(String);

#[tauri::command]
fn engine_url(url: tauri::State<'_, EngineUrl>) -> String {
    url.0.clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let addr = tauri::async_runtime::block_on(start_engine())?;
            tracing::info!(%addr, "engine listening");
            app.manage(EngineUrl(format!("ws://{addr}/ws")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![engine_url])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}

async fn start_engine() -> std::io::Result<SocketAddr> {
    let config = server::Config {
        // Loopback only until pairing exists (phase 4).
        addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        ui_dir: None,
    };
    let (addr, serve) = server::bind(Engine::spawn(), config).await?;
    tauri::async_runtime::spawn(async move {
        if let Err(err) = serve.await {
            tracing::error!(%err, "engine server stopped");
        }
    });
    Ok(addr)
}
