//! Android's media session: the notification, lock screen and headset
//! buttons. `MediaPlugin.kt` shows what [`integrations::media_controls`]
//! follows from the engine, and its buttons come back as ordinary commands.
//! While something is queued its foreground service keeps Android from
//! freezing the app, so playback goes on in the background.

use engine::Engine;
use library::Library;
use protocol::Command;
use tauri::{
    Manager, Runtime,
    ipc::{Channel, InvokeResponseBody},
    plugin::{PluginHandle, TauriPlugin},
};

/// The Kotlin side, for [`start`].
pub struct Kotlin<R: Runtime>(PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("media")
        .setup(|app, api| {
            app.manage(Kotlin(api.register_android_plugin(
                "io.github.mgwabel.motif",
                "MediaPlugin",
            )?));
            Ok(())
        })
        .build()
}

/// Mirrors `engine` in the media session and sends its buttons to `engine`.
pub fn start<R: Runtime>(kotlin: &Kotlin<R>, engine: Engine, library: Option<Library>) {
    let kotlin = kotlin.0.clone();
    let runtime = tokio::runtime::Handle::current();
    let commands = engine.clone();
    let buttons: Channel<serde_json::Value> = Channel::new(move |body| {
        let InvokeResponseBody::Json(json) = body else {
            return Ok(());
        };
        match serde_json::from_str::<Command>(&json) {
            Ok(command) => {
                let engine = commands.clone();
                runtime.spawn(async move {
                    if let Err(err) = engine.send(command).await {
                        tracing::debug!(%err, "media button command rejected");
                    }
                });
            }
            Err(err) => tracing::warn!(%err, json, "unknown media button"),
        }
        Ok(())
    });
    let (updates, rx) = std::sync::mpsc::channel();
    // Plugin calls block until Kotlin answers, so they get a thread of their own.
    let spawned = std::thread::Builder::new()
        .name("media-session".into())
        .spawn(move || {
            let listen = serde_json::json!({ "channel": buttons });
            if let Err(err) = kotlin.run_mobile_plugin::<serde_json::Value>("listen", listen) {
                tracing::warn!(%err, "no media session buttons");
            }
            for update in rx {
                if let Err(err) = kotlin.run_mobile_plugin::<serde_json::Value>("show", update) {
                    tracing::warn!(%err, "media session update failed");
                }
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "can't start the media session");
        return;
    }
    tokio::spawn(integrations::media_controls::follow(
        engine, library, updates,
    ));
}
