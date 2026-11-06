//! Android's media session: the notification, lock screen and headset
//! buttons. `MediaPlugin.kt` shows what [`integrations::media_controls`]
//! follows from the engine, and its buttons come back as ordinary commands.
//! While something is queued its foreground service keeps Android from
//! freezing the app, so playback goes on in the background. The engine's
//! volume is the system media volume ([`crate::volume`]). Without that
//! service, the library holds its background work while the app is out of
//! sight, or Android kills it for using the CPU. Audio focus and unplugged
//! headphones pause, resume and duck it ([`crate::focus`]).

use crate::{
    focus::{Change, Focus},
    volume::System,
};
use engine::Engine;
use library::Library;
use protocol::{Command, PlaybackStatus};
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
    start_volume(kotlin.clone(), engine.clone());
    let background = library.clone().map(|library| {
        Channel::<serde_json::Value>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                match serde_json::from_str::<serde_json::Value>(&json).map(|v| v["held"].as_bool())
                {
                    Ok(Some(held)) => library.hold_background(held),
                    _ => tracing::warn!(json, "unknown background change"),
                }
            }
            Ok(())
        })
    });
    let runtime = tokio::runtime::Handle::current();
    let focus = focus_changes(engine.clone(), runtime.clone());
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
            let listen = serde_json::json!({ "channel": focus });
            if let Err(err) = kotlin.run_mobile_plugin::<serde_json::Value>("listenFocus", listen) {
                tracing::warn!(%err, "no audio focus");
            }
            if let Some(channel) = background {
                let listen = serde_json::json!({ "channel": channel });
                if let Err(err) =
                    kotlin.run_mobile_plugin::<serde_json::Value>("listenBackground", listen)
                {
                    tracing::warn!(%err, "library work won't pause in the background");
                }
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

/// Where `MediaPlugin.kt` reports audio focus and unplugged headphones.
fn focus_changes(engine: Engine, runtime: tokio::runtime::Handle) -> Channel<serde_json::Value> {
    let focus = std::sync::Mutex::new(Focus::default());
    Channel::new(move |body| {
        let InvokeResponseBody::Json(json) = body else {
            return Ok(());
        };
        match serde_json::from_str::<Change>(&json) {
            Ok(change) => {
                let playing = engine.state().status == PlaybackStatus::Playing;
                let (command, ducked) = {
                    let mut focus = focus.lock().expect("focus state");
                    (focus.on(change, playing), focus.ducked())
                };
                let engine = engine.clone();
                runtime.spawn(async move {
                    if let Err(err) = engine.duck(ducked).await {
                        tracing::warn!(%err, "can't duck");
                    }
                    if let Some(command) = command
                        && let Err(err) = engine.send(command).await
                    {
                        tracing::warn!(%err, "audio focus command rejected");
                    }
                });
            }
            Err(err) => tracing::warn!(%err, json, "unknown audio focus change"),
        }
        Ok(())
    })
}

/// Keeps the engine's volume and Android's media volume the same.
fn start_volume<R: Runtime>(kotlin: PluginHandle<R>, engine: Engine) {
    let (system_tx, system_rx) = tokio::sync::mpsc::unbounded_channel();
    let changes: Channel<serde_json::Value> = Channel::new(move |body| {
        if let InvokeResponseBody::Json(json) = body {
            match serde_json::from_str::<System>(&json) {
                Ok(system) => _ = system_tx.send(system),
                Err(err) => tracing::warn!(%err, json, "unknown volume change"),
            }
        }
        Ok(())
    });
    let (steps, rx) = std::sync::mpsc::channel::<i32>();
    // Its own thread, so steps are set in order and never wait on the media session.
    let spawned = std::thread::Builder::new()
        .name("media-volume".into())
        .spawn(move || {
            let listen = serde_json::json!({ "channel": changes });
            if let Err(err) = kotlin.run_mobile_plugin::<serde_json::Value>("listenVolume", listen)
            {
                tracing::warn!(%err, "volume keys won't reach the engine");
            }
            for index in rx {
                let set = serde_json::json!({ "index": index });
                if let Err(err) = kotlin.run_mobile_plugin::<serde_json::Value>("setVolume", set) {
                    tracing::warn!(%err, "can't set the media volume");
                }
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "can't follow the media volume");
        return;
    }
    tokio::spawn(follow_volume(engine, system_rx, steps));
}

async fn follow_volume(
    engine: Engine,
    mut changes: tokio::sync::mpsc::UnboundedReceiver<System>,
    steps: std::sync::mpsc::Sender<i32>,
) {
    let mut states = engine.subscribe();
    // Unknown until Kotlin reports it, so the engine's default never sets it.
    let mut system: Option<System> = None;
    loop {
        tokio::select! {
            Some(now) = changes.recv() => {
                system = Some(now);
                if let Some(volume) = now.to_follow(engine.state().volume)
                    && let Err(err) = engine.send(Command::SetVolume { volume }).await
                {
                    tracing::warn!(%err, "can't follow the media volume");
                }
            }
            changed = states.changed() => {
                if changed.is_err() {
                    return;
                }
                let volume = states.borrow_and_update().volume;
                if let Some(now) = &mut system
                    && let Some(index) = now.to_set(volume)
                {
                    now.index = index;
                    _ = steps.send(index);
                }
            }
        }
    }
}
