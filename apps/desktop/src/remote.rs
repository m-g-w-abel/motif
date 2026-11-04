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
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let request = PairRequest {
        code: code.trim().to_owned(),
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
}
