//! Scrobbles waiting to be sent, saved as a JSON list so they survive
//! restarts and going offline.

use std::path::PathBuf;

use super::{Error, Listen, Scrobbler};

pub struct Queue {
    file: PathBuf,
    listens: Vec<Listen>,
}

impl Queue {
    pub fn open(file: PathBuf) -> Self {
        let listens = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                // Keep the unreadable file rather than overwrite it.
                tracing::warn!(%err, "unreadable scrobble queue {}", file.display());
                let _ = std::fs::rename(&file, file.with_extension("json.bad"));
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Self { file, listens }
    }

    pub fn len(&self) -> usize {
        self.listens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.listens.is_empty()
    }

    pub fn push(&mut self, listen: Listen) -> std::io::Result<()> {
        self.listens.push(listen);
        self.save()
    }

    /// Submits everything, oldest first. Listens the service rejects are
    /// dropped; anything else stops the flush and leaves the rest queued.
    pub async fn flush<S: Scrobbler>(&mut self, service: &S, secret: &str) -> Result<(), Error> {
        // Listens left to send singly, after a batch with a bad one was rejected.
        let mut singly = 0;
        while !self.listens.is_empty() {
            let n = if singly > 0 { 1 } else { S::BATCH }.min(self.listens.len());
            match service.scrobble(secret, &self.listens[..n]).await {
                Ok(()) => {}
                // Find the bad one by sending that batch singly.
                Err(Error::Rejected(_)) if n > 1 => {
                    singly = n;
                    continue;
                }
                Err(Error::Rejected(err)) => {
                    tracing::warn!(%err, listen = ?self.listens[0], "{} rejected a scrobble", service.name());
                }
                Err(err) => return Err(err),
            }
            self.listens.drain(..n);
            singly = singly.saturating_sub(n);
            if let Err(err) = self.save() {
                tracing::warn!(%err, "can't save the scrobble queue");
            }
        }
        Ok(())
    }

    fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_vec(&self.listens).map_err(std::io::Error::other)?;
        crate::settings::replace(&self.file, &json)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use protocol::{IntegrationSettings, ScrobbleAccount};

    use super::*;

    /// Takes batches of two; offline until told otherwise; rejects "bad".
    #[derive(Clone, Default)]
    struct Fake {
        online: Arc<Mutex<bool>>,
        got: Arc<Mutex<Vec<String>>>,
        /// The size of every batch sent while online.
        sizes: Arc<Mutex<Vec<usize>>>,
    }

    impl Scrobbler for Fake {
        const BATCH: usize = 2;
        fn name(&self) -> &'static str {
            "fake"
        }
        fn account<'a>(&self, settings: &'a mut IntegrationSettings) -> &'a mut ScrobbleAccount {
            &mut settings.listenbrainz
        }
        async fn sign_in(
            &self,
            _: &str,
            _: &(dyn Fn(&str) + Sync),
        ) -> Result<(String, String), Error> {
            unreachable!()
        }
        async fn now_playing(&self, _: &str, _: &Listen) -> Result<(), Error> {
            unreachable!()
        }
        async fn scrobble(&self, _: &str, listens: &[Listen]) -> Result<(), Error> {
            if !*self.online.lock().unwrap() {
                return Err(Error::Retry("offline".into()));
            }
            self.sizes.lock().unwrap().push(listens.len());
            if listens.iter().any(|l| l.title == "bad") {
                return Err(Error::Rejected("bad".into()));
            }
            let mut got = self.got.lock().unwrap();
            got.extend(
                listens
                    .iter()
                    .map(|l| format!("{}@{}", l.title, l.listened_at)),
            );
            Ok(())
        }
    }

    fn listen(title: &str, listened_at: u64) -> Listen {
        Listen {
            listened_at,
            artist: "Band".into(),
            title: title.into(),
            album: None,
            duration_ms: None,
            recording_mbid: None,
            isrc: None,
        }
    }

    #[tokio::test]
    async fn offline_scrobbles_survive_a_restart_and_flush_in_order() {
        let dir = std::env::temp_dir().join(format!("motif-queue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("scrobbles-fake.json");
        let fake = Fake::default();

        let mut queue = Queue::open(file.clone());
        for (i, title) in ["a", "b", "bad", "c", "d", "e"].into_iter().enumerate() {
            queue.push(listen(title, 100 + i as u64)).unwrap();
        }
        assert!(queue.flush(&fake, "secret").await.is_err());
        assert_eq!(queue.len(), 6, "nothing lost offline");
        drop(queue);

        let mut queue = Queue::open(file.clone());
        assert_eq!(queue.len(), 6, "kept across a restart");
        *fake.online.lock().unwrap() = true;
        queue.flush(&fake, "secret").await.unwrap();
        assert!(queue.is_empty());
        assert_eq!(
            *fake.got.lock().unwrap(),
            ["a@100", "b@101", "c@103", "d@104", "e@105"],
            "in order with their own times, the rejected one dropped"
        );
        assert_eq!(
            *fake.sizes.lock().unwrap(),
            [2, 2, 1, 1, 2],
            "only the rejected batch goes singly"
        );
        assert!(Queue::open(file).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
