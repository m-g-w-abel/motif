//! Runs the engine and server without a window.
//!
//! Environment:
//! - `MOTIF_ADDR`: listen address, default `127.0.0.1:7700`
//! - `MOTIF_UI_DIR`: built web UI to serve, default `apps/ui/dist` if present
//! - `MOTIF_DATA_DIR`: library database and caches, default the OS data
//!   directory (shared with the desktop app; only one engine may own it)

use std::{net::SocketAddr, path::PathBuf};

use engine::Engine;
use tracing_subscriber::EnvFilter;

// glibc's allocator keeps freed artwork buffers as fragmented heap (#53).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const DEFAULT_ADDR: &str = "127.0.0.1:7700";
const DEFAULT_UI_DIR: &str = "apps/ui/dist";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Discord crate warns on every retry while Discord isn't running.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,discord_rich_presence=error".into()),
        )
        .init();

    let addr: SocketAddr = std::env::var("MOTIF_ADDR")
        .unwrap_or_else(|_| DEFAULT_ADDR.into())
        .parse()?;
    if !addr.ip().is_loopback() {
        tracing::warn!(%addr, "no authentication yet: anyone who can reach this address can control playback");
    }
    let ui_dir = std::env::var_os("MOTIF_UI_DIR")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(DEFAULT_UI_DIR)).filter(|p| p.is_dir()));

    let data_dir = library::data_dir().ok_or("no data directory; set MOTIF_DATA_DIR")?;
    let mut claim = match library::Claim::take(&data_dir)? {
        Ok(claim) => claim,
        Err(owner) => {
            let owner = owner.unwrap_or_else(|| "an unknown address".into());
            return Err(format!(
                "another Motif engine already owns {} ({owner})",
                data_dir.display()
            )
            .into());
        }
    };
    let library = library::Library::open(&data_dir)?;
    let engine = Engine::spawn(server::LibraryResolver {
        library: Some(library.clone()),
    });
    tokio::spawn(server::settings::keep(
        engine.clone(),
        data_dir.join("engine.json"),
    ));
    // shortcut: no window handle, so no media controls on Windows (SMTC needs one); fine for a NAS or Pi.
    integrations::media_controls::start(engine.clone(), Some(library.clone()), None);
    let integrations = integrations::settings::keep(&data_dir);
    integrations::scrobble::start(engine.clone(), integrations.clone(), &data_dir);
    integrations::discord::start(
        engine.clone(),
        Some(library.clone()),
        integrations.subscribe(),
    );
    let config = server::Config {
        addr,
        ui_dir: ui_dir.clone(),
        library: Some(library),
        integrations,
    };
    let (addr, serve) = server::bind(engine, config).await?;
    claim.publish(&format!("ws://{addr}/ws"))?;
    match ui_dir {
        Some(dir) => tracing::info!("serving {} at http://{addr}", dir.display()),
        None => tracing::info!("listening on ws://{addr}/ws (no web UI found)"),
    }

    tokio::select! {
        result = serve => result?,
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
    }
    Ok(())
}
