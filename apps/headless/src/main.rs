//! Runs the engine and server without a window.
//!
//! Environment:
//! - `MOTIF_ADDR`: listen address, default `127.0.0.1:7700`
//! - `MOTIF_UI_DIR`: built web UI to serve, default `apps/ui/dist` if present
//! - `MOTIF_DATA_DIR`: library database and caches, default the OS data
//!   directory (shared with the desktop app; only one engine may own it)
//! - `MOTIF_LAN`: `on` or `off` sets the LAN switch, which is kept across
//!   restarts. While on, other machines reach the engine (on every interface,
//!   same port, advertised as `_motif._tcp` over mDNS) when a pairing code is
//!   live or a device is paired; `off` closes it without unpairing anyone.
//!   Starting a pairing turns it on.
//! - `MOTIF_MPD`: `on` or `off` sets the MPD server switch, kept across
//!   restarts (off by default). It serves MPD clients on this machine, or the
//!   LAN while the LAN switch is on; see the `server` crate's `mpd.rs`.
//! - `MOTIF_MPD_PORT`: the MPD port, default 6600.
//! - `MOTIF_PAIR`: `1` starts pairing a new device. The QR code, code and URL
//!   are printed here, also when a client starts a pairing.

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
    tokio::spawn(server::playback::keep(
        engine.clone(),
        data_dir.join("playback.json"),
    ));
    // shortcut: no window handle, so no media controls on Windows (SMTC needs one); fine for a NAS or Pi.
    integrations::media_controls::start(engine.clone(), Some(library.clone()), None);
    let integrations = integrations::settings::keep(&data_dir);
    integrations::scrobble::start(
        engine.clone(),
        Some(library.clone()),
        integrations.clone(),
        &data_dir,
    );
    integrations::discord::start(
        engine.clone(),
        Some(library.clone()),
        integrations.subscribe(),
    );
    let pairing = server::Pairing::open(&data_dir)?;
    match std::env::var("MOTIF_LAN").as_deref() {
        Ok("on") => pairing.allow_lan(true),
        Ok("off") => pairing.allow_lan(false),
        Ok(other) => return Err(format!("MOTIF_LAN is on or off, not {other:?}").into()),
        Err(_) => {}
    }
    match std::env::var("MOTIF_MPD").as_deref() {
        Ok("on") => pairing.enable_mpd(true),
        Ok("off") => pairing.enable_mpd(false),
        Ok(other) => return Err(format!("MOTIF_MPD is on or off, not {other:?}").into()),
        Err(_) => {}
    }
    if let Ok(port) = std::env::var("MOTIF_MPD_PORT") {
        pairing.set_mpd_port(port.parse()?);
    }
    let mut states = pairing.subscribe();
    tokio::spawn(async move {
        let mut shown = None;
        while states.changed().await.is_ok() {
            let state = states.borrow_and_update().clone();
            let (Some(code), Some(url)) = (state.code, state.lan.pair_urls.first()) else {
                continue;
            };
            if shown.replace(code.code.clone()).as_ref() != Some(&code.code) {
                print_pairing(url, &code.code, &state.lan.pair_urls[1..]);
            }
        }
    });
    let config = server::Config {
        addr,
        ui: ui_dir.clone().map(server::Ui::Dir),
        library: Some(library),
        integrations,
        pairing: pairing.clone(),
    };
    let (addr, serve) = server::bind(engine, config).await?;
    if std::env::var("MOTIF_PAIR").is_ok_and(|v| v == "1") {
        pairing.start();
    }
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

/// A pairing code for the terminal: a QR code to scan with a phone, and the
/// code and URL to type.
fn print_pairing(url: &str, code: &str, others: &[String]) {
    use qrcode::render::unicode::Dense1x2;
    let qr = match qrcode::QrCode::new(url) {
        // Light on dark: terminals are mostly dark, and scanners need the
        // quiet zone around the code to be light.
        Ok(qr) => qr
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build(),
        Err(err) => format!("(no QR code: {err})"),
    };
    println!("\n{qr}\nPair a device: scan this, or open {url}\nCode: {code}");
    for other in others {
        println!("Or on another network: {other}");
    }
    println!();
}
