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
    #[cfg(target_os = "linux")]
    keep_gpu_rendering_on_nvidia();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // The OS media controls need the window on Windows.
            #[cfg(windows)]
            let hwnd = app
                .get_webview_window("main")
                .and_then(|window| window.hwnd().ok())
                .map(|hwnd| hwnd.0);
            #[cfg(not(windows))]
            let hwnd = None;
            let url = tauri::async_runtime::block_on(start_engine(hwnd))?;
            tracing::info!(%url, "engine");
            app.manage(EngineUrl(url));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![engine_url])
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

/// Starts the embedded engine and returns its address. If another engine
/// (e.g. the headless one) already owns this machine's library, the window
/// connects to that one instead.
async fn start_engine(
    hwnd: Option<*mut std::ffi::c_void>,
) -> Result<String, Box<dyn std::error::Error>> {
    let library = match library::data_dir() {
        Some(dir) => match library::Claim::take(&dir)? {
            Ok(claim) => Some((library::Library::open(&dir)?, claim)),
            Err(Some(owner)) => return Ok(owner),
            Err(None) => {
                return Err(format!("{} is locked by another engine", dir.display()).into());
            }
        },
        // shortcut: no data directory (Android) means no library, give Android its app data dir in phase 5.
        None => None,
    };
    let engine = Engine::spawn(server::LibraryResolver {
        library: library.as_ref().map(|(library, _)| library.clone()),
    });
    if let Some(dir) = library.is_some().then(library::data_dir).flatten() {
        tauri::async_runtime::spawn(server::settings::keep(
            engine.clone(),
            dir.join("engine.json"),
        ));
    }
    integrations::media_controls::start(
        engine.clone(),
        library.as_ref().map(|(library, _)| library.clone()),
        hwnd,
    );
    let config = server::Config {
        // Loopback only until pairing exists (phase 4).
        addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        ui_dir: None,
        library: library.as_ref().map(|(library, _)| library.clone()),
    };
    let (addr, serve) = server::bind(engine, config).await?;
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
