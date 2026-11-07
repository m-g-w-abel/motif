//! Android has no OS keyring for `keyring`, so this makes `SecretsPlugin.kt`
//! its store: secrets encrypted with a key held in the Android Keystore.
//! [`integrations::settings::Secrets`] then uses it as on the desktop.

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use tauri::{
    Runtime,
    plugin::{PluginHandle, TauriPlugin},
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("secrets")
        .setup(|_app, api| {
            let kotlin = api.register_android_plugin("io.github.mgwabel.motif", "SecretsPlugin")?;
            keyring::set_default_credential_builder(Box::new(Keystore(kotlin)));
            Ok(())
        })
        .build()
}

struct Keystore<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> CredentialBuilderApi for Keystore<R> {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(Secret {
            kotlin: self.0.clone(),
            key: format!("{service}/{user}"),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct Secret<R: Runtime> {
    kotlin: PluginHandle<R>,
    key: String,
}

impl<R: Runtime> Secret<R> {
    fn run(&self, command: &str, value: Option<&str>) -> keyring::Result<serde_json::Value> {
        let args = serde_json::json!({ "key": self.key, "value": value });
        self.kotlin
            .run_mobile_plugin(command, args)
            .map_err(|err| keyring::Error::PlatformFailure(err.into()))
    }
}

impl<R: Runtime> CredentialApi for Secret<R> {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        let secret = std::str::from_utf8(secret)
            .map_err(|_| keyring::Error::BadEncoding(secret.to_vec()))?;
        self.run("set", Some(secret)).map(drop)
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        match self.run("get", None)?["value"].as_str() {
            Some(secret) => Ok(secret.into()),
            None => Err(keyring::Error::NoEntry),
        }
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        self.run("set", None).map(drop)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
