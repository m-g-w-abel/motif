//! Desktop (and later Android) shell.
//!
//! Runs the engine and its WebSocket server in-process on a loopback port.
//! The window's UI then connects to it exactly as a remote client would; the
//! only Tauri-specific call is [`engine_url`], which tells it where to connect.

#[cfg(target_os = "android")]
mod media;
#[cfg(any(test, target_os = "android"))]
mod volume;

use std::{net::SocketAddr, sync::Arc};

use engine::Engine;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

mod remote;

// glibc's allocator keeps freed artwork buffers as fragmented heap (#53).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

struct EngineUrl(String);

#[tauri::command]
fn engine_url(url: tauri::State<'_, EngineUrl>) -> String {
    url.0.clone()
}

/// Opens the file manager on the folder holding `path`, with the file selected where the OS can.
#[tauri::command]
fn show_in_folder(path: String) -> Result<(), String> {
    use std::process::Command;
    let path = std::path::Path::new(&path);
    let mut command = if cfg!(target_os = "windows") {
        let mut c = Command::new("explorer");
        c.arg(format!("/select,{}", path.display()));
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-R").arg(path);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(path.parent().unwrap_or(path));
        c
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the folder: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    keep_gpu_rendering_on_nvidia();
    // The Discord crate warns on every retry while Discord isn't running.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,discord_rich_presence=error".into()),
        )
        .init();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(media::init());
    builder
        .setup(|app| {
            #[cfg(target_os = "android")]
            let media_controls = {
                let kotlin = app.state::<media::Kotlin<tauri::Wry>>();
                move |engine, library| media::start(&kotlin, engine, library)
            };
            #[cfg(not(target_os = "android"))]
            let media_controls = {
                // The OS media controls need the window on Windows.
                #[cfg(windows)]
                let hwnd = app
                    .get_webview_window("main")
                    .and_then(|window| window.hwnd().ok())
                    .map(|hwnd| hwnd.0);
                #[cfg(not(windows))]
                let hwnd = None;
                move |engine, library| integrations::media_controls::start(engine, library, hwnd)
            };
            // Android has no user data directory; the library lives with the app.
            let data_dir = if cfg!(target_os = "android") {
                app.path().app_data_dir().ok()
            } else {
                library::data_dir()
            };
            let url = tauri::async_runtime::block_on(start_engine(
                media_controls,
                built_in_ui(app),
                data_dir,
            ))?;
            tracing::info!(%url, "engine");
            app.manage(EngineUrl(url));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engine_url,
            show_in_folder,
            remote::discover_engines,
            remote::pair_engine
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}

/// WebKitGTK turns GPU rendering off on NVIDIA's driver because its GBM
/// buffers fail there, which leaves every frame to the CPU: the album grid
/// drops from 60 to under 20 fps. Rendering on the GPU into shared memory
/// works, so restart with that set. A user's own setting of any of these
/// variables wins.
#[cfg(target_os = "linux")]
fn keep_gpu_rendering_on_nvidia() {
    use std::os::unix::process::CommandExt;

    const VARS: [&str; 2] = [
        "WEBKIT_FORCE_DMABUF_RENDERER",
        "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
    ];
    let nvidia = std::path::Path::new("/proc/driver/nvidia").exists();
    let user_set = VARS.iter().any(|v| std::env::var_os(v).is_some())
        || std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some();
    if !nvidia || user_set {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        // `exec` only returns if the restart failed; carry on without it.
        let err = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .envs(VARS.map(|v| (v, "1")))
            .exec();
        eprintln!("could not restart with GPU rendering: {err}");
    }
}

/// The UI bundled into the app, served to other devices (a phone opening a
/// pairing link). `tauri dev` loads the UI from Vite and bundles none.
fn built_in_ui(app: &tauri::App) -> Option<server::Ui> {
    let assets = app.asset_resolver();
    if assets.get("index.html".into()).is_none() {
        tracing::warn!("no built-in UI (tauri dev?): pairing links open no page");
        return None;
    }
    Some(server::Ui::Assets(Arc::new(move |path: &str| {
        assets
            .get(path.into())
            .map(|asset| (asset.bytes, asset.mime_type))
    })))
}

/// Starts the embedded engine and returns its address. If another engine
/// (e.g. the headless one) already owns this machine's library, the window
/// connects to that one instead. `media_controls` mirrors the engine in the
/// OS media controls.
async fn start_engine(
    media_controls: impl FnOnce(Engine, Option<library::Library>),
    ui: Option<server::Ui>,
    data_dir: Option<std::path::PathBuf>,
) -> Result<String, Box<dyn std::error::Error>> {
    let library = match &data_dir {
        Some(dir) => match library::Claim::take(dir)? {
            Ok(claim) => Some((library::Library::open(dir)?, claim)),
            Err(Some(owner)) => return Ok(owner),
            Err(None) => {
                return Err(format!("{} is locked by another engine", dir.display()).into());
            }
        },
        None => None,
    };
    let data_dir = data_dir.filter(|_| library.is_some());
    let engine = Engine::spawn(server::LibraryResolver {
        library: library.as_ref().map(|(library, _)| library.clone()),
    });
    if let Some(dir) = &data_dir {
        tauri::async_runtime::spawn(server::settings::keep(
            engine.clone(),
            dir.join("engine.json"),
        ));
    }
    media_controls(
        engine.clone(),
        library.as_ref().map(|(library, _)| library.clone()),
    );
    let integrations = match &data_dir {
        Some(dir) => {
            let settings = integrations::settings::keep(dir);
            integrations::scrobble::start(engine.clone(), settings.clone(), dir);
            settings
        }
        // No data directory: the settings last until the app quits, and no scrobbling.
        None => tokio::sync::watch::channel(Default::default()).0,
    };
    integrations::discord::start(
        engine.clone(),
        library.as_ref().map(|(library, _)| library.clone()),
        integrations.subscribe(),
    );
    let port_file = data_dir.as_ref().map(|dir| dir.join("desktop-port"));
    let config = server::Config {
        // Loopback; pairing opens the LAN on the same port (server::bind).
        addr: SocketAddr::from(([127, 0, 0, 1], stable_port(port_file.as_deref()))),
        ui,
        library: library.as_ref().map(|(library, _)| library.clone()),
        integrations,
        pairing: match &data_dir {
            Some(dir) => server::Pairing::open(dir)?,
            None => server::Pairing::in_memory(),
        },
    };
    let (addr, serve) = server::bind(engine, config).await?;
    if let Some(file) = &port_file {
        let _ = std::fs::write(file, addr.port().to_string());
    }
    let url = format!("ws://{addr}/ws");
    if let Some((_, mut claim)) = library {
        claim.publish(&url)?;
        // The claim lasts as long as the process.
        std::mem::forget(claim);
    }
    tauri::async_runtime::spawn(async move {
        if let Err(err) = serve.await {
            tracing::error!(%err, "engine server stopped");
        }
    });
    Ok(url)
}

const DEFAULT_PORT: u16 = 7700;

/// The loopback port to listen on: the one used last time (so paired browsers,
/// stored per origin, keep working), else 7700, else 0 (any free port).
fn stable_port(file: Option<&std::path::Path>) -> u16 {
    let last = file
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| s.trim().parse::<u16>().ok());
    let port = first_free(last.into_iter().chain([DEFAULT_PORT]));
    if let Some(last) = last.filter(|last| *last != port) {
        tracing::warn!(
            last,
            "last port is taken, using a new one; paired browsers must pair again"
        );
    }
    port
}

/// The first candidate that can be bound on loopback, or 0 if none can.
// shortcut: probe then rebind, so another process could take it in between; bind then fails, fine for a desktop app.
fn first_free(candidates: impl IntoIterator<Item = u16>) -> u16 {
    candidates
        .into_iter()
        .find(|port| std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], *port))).is_ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_taken_ports() {
        let taken = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let busy = taken.local_addr().unwrap().port();
        let free = {
            let l = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
            l.local_addr().unwrap().port()
        };
        assert_eq!(first_free([busy, free]), free);
        assert_eq!(first_free([busy]), 0);
    }
}
