//! Engine settings that outlive a restart (decoder order, volume, repeat,
//! ReplayGain),
//! kept in a small JSON file next to the library.

use std::path::PathBuf;

use engine::Engine;
use protocol::{Command, PlayerState, RepeatMode, ReplayGainSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    decoders: Vec<String>,
    volume: f32,
    repeat: RepeatMode,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    replay_gain: ReplayGainSettings,
}

impl Saved {
    fn of(state: &PlayerState) -> Self {
        Self {
            decoders: state.decoders.clone(),
            volume: state.volume,
            repeat: state.repeat,
            output: state.output.clone(),
            replay_gain: state.replay_gain,
        }
    }
}

/// Restores the settings saved in `file` and saves them whenever they change.
pub async fn keep(engine: Engine, file: PathBuf) {
    if let Some(saved) = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
    {
        for command in [
            Command::SetDecoders {
                order: saved.decoders,
            },
            Command::SetVolume {
                volume: saved.volume,
            },
            Command::SetRepeat { mode: saved.repeat },
            Command::SetOutput {
                device: saved.output,
            },
            Command::SetReplayGain {
                settings: saved.replay_gain,
            },
        ] {
            if let Err(err) = engine.send(command).await {
                tracing::warn!(%err, "ignoring a saved setting");
            }
        }
    }
    let mut states = engine.subscribe();
    let mut last = Saved::of(&states.borrow_and_update());
    while states.changed().await.is_ok() {
        let now = Saved::of(&states.borrow_and_update());
        if now != last {
            match serde_json::to_vec_pretty(&now) {
                Ok(json) => {
                    if let Err(err) = std::fs::write(&file, json) {
                        tracing::warn!(%err, "can't save engine settings");
                    }
                }
                Err(err) => tracing::warn!(%err, "can't save engine settings"),
            }
            last = now;
        }
    }
}
