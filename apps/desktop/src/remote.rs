//! Finding other engines on the LAN and pairing with them, for the UI's "Add
//! an engine". Client-side, so it lives here and not in the engine or server.

use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};
use protocol::{PairRequest, Paired};

/// How long one look around the LAN takes; the UI repeats it while its
/// screen is open, so nothing is browsed in the background.
const BROWSE: Duration = Duration::from_millis(2500);

/// An engine seen on the LAN (`_motif._tcp`).
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct Found {
    /// The engine key from its TXT record.
    id: String,
    name: String,
    /// `host:port`, IPv4 only like the host's own list.
    addresses: Vec<String>,
}

fn found(service: &ResolvedService) -> Option<Found> {
    let id = service.get_property_val_str("id")?.to_owned();
    let mut addresses: Vec<Ipv4Addr> = service
        .get_addresses()
        .iter()
        .filter_map(|a| match a.to_ip_addr() {
            IpAddr::V4(ip) if !ip.is_link_local() => Some(ip),
            _ => None,
        })
        .collect();
    addresses.sort();
    (!addresses.is_empty()).then(|| Found {
        name: service
            .get_fullname()
            .strip_suffix(&format!(".{}", service.ty_domain))
            .unwrap_or(&id)
            .to_owned(),
        addresses: addresses
            .into_iter()
            .map(|ip| format!("{ip}:{}", service.get_port()))
            .collect(),
        id,
    })
}

fn browse() -> Result<Vec<Found>, String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let events = daemon
        .browse("_motif._tcp.local.")
        .map_err(|e| e.to_string())?;
    let end = Instant::now() + BROWSE;
    let mut engines = BTreeMap::new();
    while let Ok(event) = events.recv_deadline(end) {
        if let ServiceEvent::ServiceResolved(service) = event
            && let Some(engine) = found(&service)
        {
            engines.insert(engine.id.clone(), engine);
        }
    }
    let _ = daemon.shutdown();
    Ok(engines.into_values().collect())
}

/// The engines answering on the LAN right now.
#[tauri::command]
pub async fn discover_engines() -> Result<Vec<Found>, String> {
    tauri::async_runtime::spawn_blocking(browse)
        .await
        .map_err(|e| e.to_string())?
}

/// Redeems `code` at `address`'s `POST /pair`; the answer waits for the
/// host's approval (up to 5 minutes). Errors are texts for the user.
#[tauri::command]
pub async fn pair_engine(address: String, code: String, name: String) -> Result<Paired, String> {
    // The other crates build reqwest without a TLS provider; it panics without one even for http.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let request = PairRequest {
        // Shown as "ABCD EFGH"; the host compares the bare code.
        code: code.split_whitespace().collect(),
        name: name.trim().to_owned(),
        public_key: None,
    };
    let response = client
        .post(format!("http://{address}/pair"))
        .json(&request)
        .send()
        .await
        .map_err(|_| format!("can't reach {address}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(if text.is_empty() {
            format!("the engine answered {status}")
        } else {
            text
        });
    }
    response.json().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdns_sd::ServiceInfo;

    fn resolved(addresses: &str, id: Option<&str>) -> ResolvedService {
        let txt: Vec<(&str, &str)> = id.map(|id| ("id", id)).into_iter().collect();
        ServiceInfo::new(
            "_motif._tcp.local.",
            "Motif abcd1234",
            "motif-abcd1234.local.",
            addresses,
            7700,
            &txt[..],
        )
        .unwrap()
        .as_resolved_service()
    }

    #[test]
    fn an_engine_is_its_key_name_and_ipv4_addresses() {
        let service = resolved("192.168.1.20,169.254.3.3,fe80::1", Some("abcd1234ef"));
        assert_eq!(
            found(&service),
            Some(Found {
                id: "abcd1234ef".into(),
                name: "Motif abcd1234".into(),
                addresses: vec!["192.168.1.20:7700".into()],
            })
        );
    }

    #[test]
    fn services_without_a_key_or_an_address_are_skipped() {
        assert_eq!(found(&resolved("192.168.1.20", None)), None);
        assert_eq!(found(&resolved("169.254.3.3", Some("k"))), None);
    }

    /// A server on a free loopback port that allows every device by itself,
    /// as a host does that has chosen so; the LAN stays closed.
    async fn local_engine() -> (server::Pairing, String, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("motif-remote-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pairing.json"),
            r#"{"auto_approve":"view_only","devices":[],"log":[],"lan":false}"#,
        )
        .unwrap();
        let pairing = server::Pairing::open(&dir).unwrap();
        let config = server::Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: tokio::sync::watch::channel(Default::default()).0,
            pairing: pairing.clone(),
        };
        let (addr, serve) = server::bind(engine::Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        (pairing, addr.to_string(), dir)
    }

    #[tokio::test]
    async fn pairing_by_code_and_its_errors() {
        let (pairing, address, dir) = local_engine().await;
        let code = pairing.start().code;
        // Connecting while the listener moves can fail once (#141).
        let mut states = pairing.subscribe();
        let open = states.wait_for(|s| s.lan.open);
        tokio::time::timeout(Duration::from_secs(5), open)
            .await
            .unwrap()
            .unwrap();
        let key = pairing.engine_key().to_owned();

        let wrong = pair_engine(address.clone(), "WRONG".into(), "Phone".into()).await;
        assert_eq!(wrong.unwrap_err(), "unknown code");
        // The code has a space in the middle on screen.
        let spaced = format!("{} {}", &code[..4], &code[4..]);
        let paired = pair_engine(address.clone(), spaced, "Phone".into())
            .await
            .unwrap();
        assert_eq!(paired.engine_key, key);
        // A retried request finds the code used up and adds no second device.
        let again = pair_engine(address, code, "Phone".into()).await;
        assert_eq!(again.unwrap_err(), "unknown code");
        assert_eq!(pairing.subscribe().borrow().devices.len(), 1);

        let gone = pair_engine("127.0.0.1:9".into(), "X".into(), "Phone".into()).await;
        assert_eq!(gone.unwrap_err(), "can't reach 127.0.0.1:9");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Multicast on the real network, and the engine listens on every interface meanwhile.
    #[tokio::test]
    #[ignore = "needs LAN multicast"]
    async fn browse_finds_an_engine_on_the_lan() {
        let pairing = server::Pairing::in_memory();
        let key = pairing.engine_key().to_owned();
        let config = server::Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            ui: None,
            library: None,
            integrations: tokio::sync::watch::channel(Default::default()).0,
            pairing: pairing.clone(),
        };
        let (_, serve) = server::bind(engine::Engine::spawn(engine::LocalFiles), config)
            .await
            .unwrap();
        tokio::spawn(serve);
        // A live code opens the LAN and starts the advert.
        pairing.start();
        let mut engine = None;
        for _ in 0..4 {
            let seen = tauri::async_runtime::spawn_blocking(browse)
                .await
                .unwrap()
                .unwrap();
            engine = seen.into_iter().find(|e| e.id == key);
            if engine.is_some() {
                break;
            }
        }
        let engine = engine.expect("the engine shows up in the browse");
        assert!(!engine.addresses.is_empty());
    }
}
