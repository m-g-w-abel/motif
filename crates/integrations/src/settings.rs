//! Integration settings, kept by the host in their own JSON file in the data
//! directory (not with the engine's), and the secrets integrations sign in
//! with, kept in the OS keyring.

use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

use protocol::IntegrationSettings;
use tokio::sync::watch;

const FILE: &str = "integrations.json";
/// Where secrets go when there is no OS keyring.
const SECRETS_FILE: &str = "secrets.json";
const KEYRING_SERVICE: &str = "motif";

/// Loads the settings saved in `dir` and saves them whenever they change.
/// Integrations follow the returned channel; the server changes it.
pub fn keep(dir: &Path) -> watch::Sender<IntegrationSettings> {
    let file = dir.join(FILE);
    let mut settings: IntegrationSettings = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    settings.secrets_unencrypted = !Secrets::new(dir).keyring_available();
    let (tx, mut rx) = watch::channel(settings);
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let now = rx.borrow_and_update().clone();
            let saved = serde_json::to_vec_pretty(&now)
                .map_err(std::io::Error::other)
                .and_then(|json| replace(&file, &json));
            if let Err(err) = saved {
                tracing::warn!(%err, "can't save integration settings");
            }
        }
    });
    tx
}

/// Tokens and session keys, by name. In the OS keyring where there is one,
/// else in a file in the data directory that only this user may read.
pub struct Secrets {
    service: String,
    file: PathBuf,
}

impl Secrets {
    pub fn new(dir: &Path) -> Self {
        Self {
            // Tests keep out of the real sign-ins.
            service: if cfg!(test) {
                "motif-test"
            } else {
                KEYRING_SERVICE
            }
            .into(),
            file: dir.join(SECRETS_FILE),
        }
    }

    pub fn get(&self, name: &str) -> Option<String> {
        if let Some(Ok(secret)) = self.entry(name).map(|e| e.get_password()) {
            return Some(secret);
        }
        let secret = self.read_file().remove(name)?;
        // Moves a secret kept before there was a keyring (Android before #132) into it.
        if let Err(err) = self.set(name, Some(&secret)) {
            tracing::warn!(%err, "can't move a secret to the keyring");
        }
        Some(secret)
    }

    /// Stores `secret` under `name`, or forgets it if `None`.
    pub fn set(&self, name: &str, secret: Option<&str>) -> std::io::Result<()> {
        let mut file = self.read_file();
        let in_keyring = match (self.entry(name), secret) {
            (Some(entry), Some(secret)) => entry.set_password(secret).is_ok(),
            (Some(entry), None) => {
                let _ = entry.delete_credential();
                false
            }
            (None, _) => false,
        };
        let changed = match secret {
            Some(secret) if !in_keyring => file
                .insert(name.into(), secret.into())
                .is_none_or(|old| old != secret),
            _ => file.remove(name).is_some(),
        };
        if changed {
            self.write_file(&file)
        } else {
            Ok(())
        }
    }

    /// Whether secrets go to an OS keyring, rather than the file.
    pub fn keyring_available(&self) -> bool {
        self.entry("probe").is_some_and(|entry| {
            matches!(entry.get_password(), Ok(_) | Err(keyring::Error::NoEntry))
        })
    }

    /// On Android the app shell makes the Keystore `keyring`'s store. Its
    /// fallback mock forgets everything on quit, so it never counts.
    fn entry(&self, name: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(&self.service, name)
            .ok()
            .filter(|e| !e.get_credential().is::<keyring::mock::MockCredential>())
    }

    fn read_file(&self) -> BTreeMap<String, String> {
        std::fs::read(&self.file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write_file(&self, secrets: &BTreeMap<String, String>) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(secrets).map_err(std::io::Error::other)?;
        replace(&self.file, &json)
    }
}

/// Replaces `file` whole, so a crash or power cut leaves the old or the new
/// contents, never a cut-off file. Only this user may read it.
pub(crate) fn replace(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = file.with_extension("partial");
    // `mode` only applies to new files.
    let _ = std::fs::remove_file(&partial);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(&partial)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    std::fs::rename(partial, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motif-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn secrets_stay_out_of_the_settings_file() {
        let dir = temp_dir("secrets");
        let secrets = Secrets {
            service: "motif-test".into(),
            file: dir.join(SECRETS_FILE),
        };
        let name = format!("token-{}", std::process::id());
        secrets.set(&name, Some("hunter2")).unwrap();
        assert_eq!(secrets.get(&name).as_deref(), Some("hunter2"));
        let in_file = std::fs::read_to_string(&secrets.file).unwrap_or_default();
        assert_eq!(in_file.contains("hunter2"), !secrets.keyring_available());
        #[cfg(unix)]
        if in_file.contains("hunter2") {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&secrets.file)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        secrets.set(&name, None).unwrap();
        assert_eq!(secrets.get(&name), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_secrets_move_to_the_keyring() {
        let dir = temp_dir("migrate");
        let secrets = Secrets {
            service: "motif-test".into(),
            file: dir.join(SECRETS_FILE),
        };
        let name = format!("old-token-{}", std::process::id());
        let mut kept = BTreeMap::new();
        kept.insert(name.clone(), "hunter2".to_string());
        secrets.write_file(&kept).unwrap();
        assert_eq!(secrets.get(&name).as_deref(), Some("hunter2"));
        let in_file = std::fs::read_to_string(&secrets.file).unwrap();
        assert_eq!(in_file.contains("hunter2"), !secrets.keyring_available());
        assert_eq!(
            secrets.get(&name).as_deref(),
            Some("hunter2"),
            "after moving"
        );
        secrets.set(&name, None).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn settings_survive_a_restart() {
        let dir = temp_dir("settings");
        let tx = keep(&dir);
        assert!(tx.borrow().lrclib, "on by default");
        tx.send_modify(|s| s.lrclib = false);
        // Saving runs on its own task.
        for _ in 0..100 {
            if dir.join(FILE).exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        drop(tx);
        assert!(!keep(&dir).borrow().lrclib);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
