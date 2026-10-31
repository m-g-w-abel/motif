//! ListenBrainz, signed in with the user token from the account's settings
//! page. API: https://listenbrainz.readthedocs.io/en/latest/users/api/core.html

use protocol::{IntegrationSettings, ScrobbleAccount};
use reqwest::StatusCode;
use serde_json::{Value, json};

use super::{Error, Listen, Scrobbler, is_uuid};

const API: &str = "https://api.listenbrainz.org";

#[derive(Clone)]
pub struct ListenBrainz {
    http: reqwest::Client,
    base: String,
}

impl ListenBrainz {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            base: API.into(),
        }
    }

    async fn call(&self, token: &str, path: &str, body: Option<Value>) -> Result<Value, Error> {
        let url = format!("{}/1/{path}", self.base);
        let request = match body {
            Some(body) => self.http.post(url).json(&body),
            None => self.http.get(url),
        };
        let response = request
            .header("Authorization", format!("Token {token}"))
            .send()
            .await
            .map_err(|e| Error::Retry(format!("Can't reach ListenBrainz: {e}")))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_default();
        if status.is_success() {
            return Ok(body);
        }
        let message = match body["error"].as_str() {
            Some(error) => format!("ListenBrainz: {error}"),
            None => format!("ListenBrainz: {status}"),
        };
        let retry = status == StatusCode::UNAUTHORIZED
            || status == StatusCode::TOO_MANY_REQUESTS
            || status.is_server_error();
        Err(if retry {
            Error::Retry(message)
        } else {
            Error::Rejected(message)
        })
    }

    async fn submit(&self, token: &str, kind: &str, listens: Vec<Value>) -> Result<(), Error> {
        let body = json!({ "listen_type": kind, "payload": listens });
        self.call(token, "submit-listens", Some(body)).await?;
        Ok(())
    }
}

impl Scrobbler for ListenBrainz {
    const NAME: &str = "listenbrainz";
    const BATCH: usize = 100;

    fn account(settings: &mut IntegrationSettings) -> &mut ScrobbleAccount {
        &mut settings.listenbrainz
    }

    async fn sign_in(
        &self,
        token: &str,
        _: &(dyn Fn(&str) + Sync),
    ) -> Result<(String, String), Error> {
        let body = self.call(token, "validate-token", None).await;
        match body
            .as_ref()
            .map(|b| (&b["valid"], b["user_name"].as_str()))
        {
            Ok((Value::Bool(true), Some(user))) => Ok((user.into(), token.into())),
            Ok(_) | Err(Error::Rejected(_)) => Err(Error::Rejected(
                "ListenBrainz doesn't know that token".into(),
            )),
            Err(Error::Retry(err) | Error::SignedOut(err)) => Err(Error::Retry(err.clone())),
        }
    }

    async fn now_playing(&self, token: &str, listen: &Listen) -> Result<(), Error> {
        self.submit(token, "playing_now", vec![payload(listen, false)])
            .await
    }

    async fn scrobble(&self, token: &str, listens: &[Listen]) -> Result<(), Error> {
        let kind = if listens.len() == 1 {
            "single"
        } else {
            "import"
        };
        let listens = listens.iter().map(|l| payload(l, true)).collect();
        self.submit(token, kind, listens).await
    }
}

fn payload(listen: &Listen, timed: bool) -> Value {
    let mut info = json!({
        "media_player": "Motif",
        "submission_client": "Motif",
        "submission_client_version": env!("CARGO_PKG_VERSION"),
    });
    if let Some(ms) = listen.duration_ms {
        info["duration_ms"] = ms.into();
    }
    // ListenBrainz refuses the whole listen over a malformed ID.
    if let Some(mbid) = listen.recording_mbid.as_deref().filter(|id| is_uuid(id)) {
        info["recording_mbid"] = mbid.into();
    }
    if let Some(isrc) = &listen.isrc {
        info["isrc"] = isrc.as_str().into();
    }
    let mut metadata = json!({
        "artist_name": listen.artist,
        "track_name": listen.title,
        "additional_info": info,
    });
    if let Some(album) = &listen.album {
        metadata["release_name"] = album.as_str().into();
    }
    let mut listen_json = json!({ "track_metadata": metadata });
    if timed {
        listen_json["listened_at"] = listen.listened_at.into();
    }
    listen_json
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{Json, Router, extract::State, http::HeaderMap, routing};

    use super::*;

    type Seen = Arc<Mutex<Vec<(String, Value)>>>;

    /// A stand-in ListenBrainz that knows the token "good" and records submissions.
    async fn mock() -> (ListenBrainz, Seen) {
        let seen = Seen::default();
        let auth = |headers: &HeaderMap| headers["authorization"] == "Token good";
        let app =
            Router::new()
                .route(
                    "/1/validate-token",
                    routing::get(move |headers: HeaderMap| async move {
                        Json(match auth(&headers) {
                            true => json!({"code": 200, "valid": true, "user_name": "rob"}),
                            false => json!({"code": 200, "valid": false}),
                        })
                    }),
                )
                .route(
                    "/1/submit-listens",
                    routing::post(
                        move |State(seen): State<Seen>,
                              headers: HeaderMap,
                              Json(body): Json<Value>| async move {
                            if !auth(&headers) {
                                return (
                                    StatusCode::UNAUTHORIZED,
                                    Json(json!({"error": "Invalid token"})),
                                );
                            }
                            let token = headers["authorization"].to_str().unwrap().to_owned();
                            seen.lock().unwrap().push((token, body));
                            (StatusCode::OK, Json(json!({"status": "ok"})))
                        },
                    ),
                )
                .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        let service = ListenBrainz {
            http: super::super::client().unwrap(),
            base,
        };
        (service, seen)
    }

    fn listen() -> Listen {
        Listen {
            listened_at: 1_700_000_000,
            artist: "Band".into(),
            title: "Song".into(),
            album: Some("Album".into()),
            duration_ms: Some(200_000),
            recording_mbid: Some("6f9a2a5e-7e1e-4e2b-9b0a-2c5d1f3e4a5b".into()),
            isrc: Some("GBAYE0601498".into()),
        }
    }

    #[tokio::test]
    async fn signs_in_with_a_valid_token_only() {
        let (lb, _) = mock().await;
        let (user, secret) = lb.sign_in("good", &|_| {}).await.unwrap();
        assert_eq!((user.as_str(), secret.as_str()), ("rob", "good"));
        assert!(matches!(
            lb.sign_in("bad", &|_| {}).await,
            Err(Error::Rejected(_))
        ));
    }

    #[tokio::test]
    async fn submits_listens() {
        let (lb, seen) = mock().await;
        lb.now_playing("good", &listen()).await.unwrap();
        lb.scrobble("good", &[listen()]).await.unwrap();
        let mut bad_id = listen();
        bad_id.recording_mbid = Some("not-a-uuid".into());
        lb.scrobble("good", &[listen(), bad_id]).await.unwrap();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].0, "Token good");
        let (now, single, import) = (&seen[0].1, &seen[1].1, &seen[2].1);
        assert_eq!(now["listen_type"], "playing_now");
        assert!(now["payload"][0].get("listened_at").is_none());
        assert_eq!(single["listen_type"], "single");
        assert_eq!(
            single["payload"][0],
            json!({
                "listened_at": 1_700_000_000,
                "track_metadata": {
                    "artist_name": "Band",
                    "track_name": "Song",
                    "release_name": "Album",
                    "additional_info": {
                        "media_player": "Motif",
                        "submission_client": "Motif",
                        "submission_client_version": env!("CARGO_PKG_VERSION"),
                        "duration_ms": 200_000,
                        "recording_mbid": "6f9a2a5e-7e1e-4e2b-9b0a-2c5d1f3e4a5b",
                        "isrc": "GBAYE0601498",
                    },
                },
            })
        );
        assert_eq!(import["listen_type"], "import");
        let info = &import["payload"][1]["track_metadata"]["additional_info"];
        assert!(
            info.get("recording_mbid").is_none(),
            "malformed IDs left out"
        );
    }

    #[tokio::test]
    async fn a_revoked_token_keeps_the_listens() {
        let (lb, _) = mock().await;
        assert!(matches!(
            lb.scrobble("revoked", &[listen()]).await,
            Err(Error::Retry(_))
        ));
        let offline = ListenBrainz {
            http: super::super::client().unwrap(),
            base: "http://127.0.0.1:9".into(),
        };
        assert!(matches!(
            offline.scrobble("good", &[listen()]).await,
            Err(Error::Retry(_))
        ));
    }
}
