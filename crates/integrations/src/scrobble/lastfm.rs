//! Last.fm and Libre.fm (which speaks the same API), signed in through the browser with desktop auth: Motif gets a
//! token, the user allows it on the site, and the token buys a session key.
//! API: https://www.last.fm/api

use std::time::Duration;

use md5::{Digest, Md5};
use protocol::{IntegrationSettings, ScrobbleAccount};
use serde_json::Value;

use super::{Error, Listen, Scrobbler, is_uuid};

const API: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH: &str = "https://www.last.fm/api/auth/";
/// Libre.fm takes any key and secret.
const LIBRE_API: &str = "https://libre.fm/2.0/";
const LIBRE_AUTH: &str = "https://libre.fm/api/auth/";
/// Motif's API account, in the source so every build (self-built, distro
/// packages) can scrobble. `MOTIF_LASTFM_API_KEY` and
/// `MOTIF_LASTFM_SHARED_SECRET` override it.
const API_KEY: &str = "d581da46dbeab20728d42f58f836a7f4";
const SHARED_SECRET: &str = "503db78ee2c8b7ab16f54cc50a2eacd9";
/// How often to ask whether the user has allowed Motif yet.
const POLL: Duration = Duration::from_millis(if cfg!(test) { 10 } else { 3000 });

type Params = Vec<(String, String)>;

#[derive(Clone)]
pub struct LastFm {
    http: reqwest::Client,
    /// "lastfm" or "librefm": names the secret, queue and settings.
    name: &'static str,
    /// For messages.
    label: &'static str,
    base: String,
    auth: String,
    key: String,
    secret: String,
}

impl LastFm {
    pub fn new(http: reqwest::Client) -> Self {
        let env = |name, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
        Self {
            http,
            name: "lastfm",
            label: "Last.fm",
            base: API.into(),
            auth: AUTH.into(),
            key: env("MOTIF_LASTFM_API_KEY", API_KEY),
            secret: env("MOTIF_LASTFM_SHARED_SECRET", SHARED_SECRET),
        }
    }

    pub fn libre_fm(http: reqwest::Client) -> Self {
        Self {
            http,
            name: "librefm",
            label: "Libre.fm",
            base: LIBRE_API.into(),
            auth: LIBRE_AUTH.into(),
            key: "motif".into(),
            secret: "motif".into(),
        }
    }

    /// Calls a signed method. Fails only if the service can't be reached; its
    /// own errors come back in the body for [`check`].
    async fn send(&self, method: &str, mut params: Params) -> Result<Value, Error> {
        params.push(("method".into(), method.into()));
        params.push(("api_key".into(), self.key.clone()));
        params.push(("api_sig".into(), signature(&params, &self.secret)));
        params.push(("format".into(), "json".into()));
        let response = self
            .http
            .post(&self.base)
            .form(&params)
            .send()
            .await
            .map_err(|e| Error::Retry(format!("Can't reach {}: {e}", self.label)))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_default();
        if body.get("error").is_none() && !status.is_success() {
            return Err(Error::Retry(format!("{}: {status}", self.label)));
        }
        Ok(body)
    }

    async fn call(&self, method: &str, params: Params) -> Result<Value, Error> {
        self.check(self.send(method, params).await?)
    }
}

impl LastFm {
    /// Turns the error in a reply into ours.
    fn check(&self, body: Value) -> Result<Value, Error> {
        let Some(code) = body["error"].as_u64() else {
            return Ok(body);
        };
        let label = self.label;
        let message = match body["message"].as_str() {
            Some(message) => format!("{label}: {message}"),
            None => format!("{label}: error {code}"),
        };
        // https://www.last.fm/api/errorcodes
        Err(match code {
            9 => Error::SignedOut(message),
            // Down, rate limited or the API key suspended: the listens may go later.
            8 | 10 | 11 | 16 | 26 | 29 => Error::Retry(message),
            _ => Error::Rejected(message),
        })
    }
}

/// `api_sig`: the MD5 of every parameter's name and value, sorted by name,
/// then the shared secret.
fn signature(params: &[(String, String)], secret: &str) -> String {
    let mut sorted: Vec<_> = params.iter().collect();
    sorted.sort();
    let mut md5 = Md5::new();
    for (name, value) in sorted {
        md5.update(name);
        md5.update(value);
    }
    md5.update(secret);
    format!("{:x}", md5.finalize())
}

/// The track fields of a now playing or scrobble, suffixed `[i]` in batches.
fn track(params: &mut Params, listen: &Listen, suffix: &str) {
    let mut add = |name: &str, value: String| params.push((format!("{name}{suffix}"), value));
    add("artist", listen.artist.clone());
    add("track", listen.title.clone());
    if let Some(album) = &listen.album {
        add("album", album.clone());
    }
    if let Some(ms) = listen.duration_ms {
        add("duration", (ms / 1000).to_string());
    }
    if let Some(mbid) = listen.recording_mbid.as_deref().filter(|id| is_uuid(id)) {
        add("mbid", mbid.into());
    }
}

impl Scrobbler for LastFm {
    const BATCH: usize = 50;

    fn name(&self) -> &'static str {
        self.name
    }

    fn account<'a>(&self, settings: &'a mut IntegrationSettings) -> &'a mut ScrobbleAccount {
        match self.name {
            "librefm" => &mut settings.librefm,
            _ => &mut settings.lastfm,
        }
    }

    async fn sign_in(
        &self,
        _: &str,
        show: &(dyn Fn(&str) + Sync),
    ) -> Result<(String, String), Error> {
        let body = self.call("auth.getToken", Vec::new()).await?;
        let Some(token) = body["token"].as_str() else {
            return Err(Error::Retry(format!(
                "{} sent no sign-in token",
                self.label
            )));
        };
        show(&format!("{}?api_key={}&token={token}", self.auth, self.key));
        // Until the user allows Motif, or the token expires after an hour.
        loop {
            tokio::time::sleep(POLL).await;
            let params = vec![("token".into(), token.into())];
            let body = self.send("auth.getSession", params).await?;
            // Not allowed yet.
            if body["error"] == 14 {
                continue;
            }
            let session = &self.check(body)?["session"];
            return match (session["name"].as_str(), session["key"].as_str()) {
                (Some(user), Some(key)) => Ok((user.into(), key.into())),
                _ => Err(Error::Retry(format!("{} sent no session", self.label))),
            };
        }
    }

    async fn now_playing(&self, session: &str, listen: &Listen) -> Result<(), Error> {
        let mut params = vec![("sk".into(), session.into())];
        track(&mut params, listen, "");
        self.call("track.updateNowPlaying", params).await?;
        Ok(())
    }

    async fn scrobble(&self, session: &str, listens: &[Listen]) -> Result<(), Error> {
        let mut params = vec![("sk".into(), session.into())];
        for (i, listen) in listens.iter().enumerate() {
            let suffix = format!("[{i}]");
            track(&mut params, listen, &suffix);
            params.push((format!("timestamp{suffix}"), listen.listened_at.to_string()));
        }
        self.call("track.scrobble", params).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use axum::{Form, Json, Router, extract::State, routing};
    use serde_json::json;

    use super::*;

    type Seen = Arc<Mutex<Vec<HashMap<String, String>>>>;

    #[test]
    fn signs_like_the_last_fm_example() {
        // From https://www.last.fm/api/desktopauth, hashed with `md5sum`.
        let params = [
            ("token", "yyyyyy"),
            ("method", "auth.getSession"),
            ("api_key", "xxxxxxxx"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        assert_eq!(
            signature(&params, "ilovecher"),
            "1333ebf6f7dec747486b6ce965cca66b"
        );
    }

    /// A stand-in Last.fm that checks signatures, allows the token on the
    /// second ask, knows the session "good" and records submissions.
    async fn mock() -> (LastFm, Seen) {
        let seen = Seen::default();
        let asks = Arc::new(Mutex::new(0));
        let app = Router::new()
            .route(
                "/2.0/",
                routing::post(
                    move |State(seen): State<Seen>, Form(form): Form<Vec<(String, String)>>| async move {
                        let params: Vec<_> = form
                            .iter()
                            .filter(|(k, _)| k != "api_sig" && k != "format")
                            .cloned()
                            .collect();
                        let map: HashMap<_, _> = form.into_iter().collect();
                        if map["api_sig"] != signature(&params, "secret") {
                            return Json(json!({"error": 13, "message": "Invalid method signature supplied"}));
                        }
                        Json(match map["method"].as_str() {
                            "auth.getToken" => json!({"token": "t0ken"}),
                            "auth.getSession" => {
                                let mut asks = asks.lock().unwrap();
                                *asks += 1;
                                match *asks {
                                    1 => json!({"error": 14, "message": "Unauthorized Token"}),
                                    _ => json!({"session": {"name": "rj", "key": "good", "subscriber": 0}}),
                                }
                            }
                            _ if map["sk"] != "good" => {
                                json!({"error": 9, "message": "Invalid session key - Please re-authenticate"})
                            }
                            _ => {
                                seen.lock().unwrap().push(map);
                                json!({"scrobbles": {}})
                            }
                        })
                    },
                ),
            )
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        let service = LastFm {
            http: super::super::client().unwrap(),
            name: "lastfm",
            label: "Last.fm",
            base: format!("{base}/2.0/"),
            auth: format!("{base}/auth/"),
            key: "key".into(),
            secret: "secret".into(),
        };
        (service, seen)
    }

    fn listen() -> Listen {
        Listen {
            listened_at: 1_700_000_000,
            artist: "Band".into(),
            title: "Song".into(),
            album: Some("Album".into()),
            duration_ms: Some(200_500),
            recording_mbid: Some("6f9a2a5e-7e1e-4e2b-9b0a-2c5d1f3e4a5b".into()),
            isrc: None,
        }
    }

    #[tokio::test]
    async fn signs_in_once_allowed_in_the_browser() {
        let (lastfm, _) = mock().await;
        let shown = Mutex::new(None);
        let signed_in = lastfm
            .sign_in("", &|url| *shown.lock().unwrap() = Some(url.to_owned()))
            .await
            .unwrap();
        assert_eq!(signed_in, ("rj".into(), "good".into()));
        let shown = shown.into_inner().unwrap().unwrap();
        assert!(shown.ends_with("/auth/?api_key=key&token=t0ken"), "{shown}");
    }

    #[tokio::test]
    async fn submits_listens() {
        let (lastfm, seen) = mock().await;
        lastfm.now_playing("good", &listen()).await.unwrap();
        let mut bad_id = listen();
        bad_id.recording_mbid = Some("not-a-uuid".into());
        lastfm.scrobble("good", &[listen(), bad_id]).await.unwrap();

        let seen = seen.lock().unwrap();
        let (now, batch) = (&seen[0], &seen[1]);
        assert_eq!(now["method"], "track.updateNowPlaying");
        assert_eq!(
            (
                now["artist"].as_str(),
                now["track"].as_str(),
                now["album"].as_str()
            ),
            ("Band", "Song", "Album")
        );
        assert_eq!(now["duration"], "200");
        assert!(!now.contains_key("timestamp"));
        assert_eq!(batch["method"], "track.scrobble");
        assert_eq!(batch["timestamp[1]"], "1700000000");
        assert_eq!(batch["mbid[0]"], "6f9a2a5e-7e1e-4e2b-9b0a-2c5d1f3e4a5b");
        assert!(!batch.contains_key("mbid[1]"), "malformed IDs left out");
    }

    #[test]
    fn libre_fm_is_its_own_account() {
        let http = super::super::client().unwrap();
        let (last, libre) = (LastFm::new(http.clone()), LastFm::libre_fm(http));
        assert_ne!(last.name(), libre.name(), "separate secrets and queues");
        assert_eq!(libre.base, "https://libre.fm/2.0/");
        let mut settings = IntegrationSettings::default();
        libre.account(&mut settings).enabled = true;
        assert!(settings.librefm.enabled && !settings.lastfm.enabled);
        last.account(&mut settings).user = Some("rj".into());
        assert!(settings.lastfm.user.is_some() && settings.librefm.user.is_none());
    }

    #[tokio::test]
    async fn a_revoked_session_signs_out() {
        let (lastfm, _) = mock().await;
        assert!(matches!(
            lastfm.scrobble("revoked", &[listen()]).await,
            Err(Error::SignedOut(_))
        ));
        let offline = LastFm {
            base: "http://127.0.0.1:9/2.0/".into(),
            ..lastfm
        };
        assert!(matches!(
            offline.scrobble("good", &[listen()]).await,
            Err(Error::Retry(_))
        ));
    }
}
