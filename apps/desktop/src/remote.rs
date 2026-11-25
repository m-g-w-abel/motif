//! Finding other engines on the LAN and pairing with them, for the UI's "Add
//! an engine", and reaching linked ones away from home through loopback
//! proxies over iroh ([`tunnel`]). Client-side, so it lives here and not in
//! the engine or server.

use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};
use protocol::{PairRequest, Paired, Reach};

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

/// This app's own engine's pairing; `None` when another process's engine
/// serves the window (the headless one owns the library).
pub struct OwnEngine(pub Option<server::Pairing>);

/// Redeems `code` at `address`'s `POST /pair`, sending this app's engine key
/// so the host can link the two; the answer waits for the host's approval
/// (up to 5 minutes). Errors are texts for the user.
#[tauri::command]
pub async fn pair_engine(
    own: tauri::State<'_, OwnEngine>,
    address: String,
    code: String,
    name: String,
) -> Result<Paired, String> {
    let key = own.0.as_ref().map(|p| p.engine_key().to_owned());
    pair(address, code, name, key).await
}

/// This app's own engine's key, to send to the engine it controls
/// (`PairingCommand::SetEngineKey`) before [`link_engine`].
#[tauri::command]
pub fn own_engine_key(own: tauri::State<'_, OwnEngine>) -> Result<String, String> {
    let pairing = own.0.as_ref().ok_or("this app's engine runs elsewhere")?;
    Ok(pairing.engine_key().to_owned())
}

/// Records the engine with `key`, which this app controls with Sync and
/// which has this app's engine key, as linked to its own engine.
/// `addresses` are where this app reached it, which its own engine
/// remembers. Safe to repeat.
#[tauri::command]
pub fn link_engine(
    own: tauri::State<'_, OwnEngine>,
    key: String,
    name: String,
    addresses: Vec<String>,
) -> Result<(), String> {
    let pairing = own.0.as_ref().ok_or("this app's engine runs elsewhere")?;
    pairing.link(&name, &key, &addresses)
}

/// Unlinks this app's engine from the engine with `key`, which this app
/// forgot; that engine is told when it next calls. Safe to repeat.
#[tauri::command]
pub fn unlink_engine(own: tauri::State<'_, OwnEngine>, key: String) -> Result<(), String> {
    let pairing = own.0.as_ref().ok_or("this app's engine runs elsewhere")?;
    pairing.unlink(&key);
    Ok(())
}

/// The loopback proxies to linked engines, by engine key.
#[derive(Default)]
pub struct Tunnels(Mutex<HashMap<String, Proxy>>);

struct Proxy {
    port: u16,
    /// The connection the proxy's streams go over; dialled again once lost.
    tunnel: Arc<tokio::sync::Mutex<Option<server::Tunnel>>>,
}

/// Where (`127.0.0.1:port`) the UI reaches the linked engine with `key`
/// away from home: plain HTTP and WebSocket there go to its server over
/// iroh (motif/remote/1), with the same token as on the LAN. Safe to
/// repeat: the same proxy, for as long as the app runs.
#[tauri::command]
pub async fn tunnel(
    own: tauri::State<'_, OwnEngine>,
    tunnels: tauri::State<'_, Tunnels>,
    key: String,
) -> Result<String, String> {
    let pairing = own.0.clone().ok_or("this app's engine runs elsewhere")?;
    proxy_to(&tunnels, pairing, key).await
}

/// How the tunnel to the engine with `key` reaches it now.
#[tauri::command]
pub async fn tunnel_reach(
    tunnels: tauri::State<'_, Tunnels>,
    key: String,
) -> Result<Reach, String> {
    let tunnel = tunnels
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .map(|p| p.tunnel.clone());
    let Some(tunnel) = tunnel else {
        return Ok(Reach::Unreachable);
    };
    let tunnel = tunnel.lock().await;
    Ok(tunnel
        .as_ref()
        .filter(|t| !t.is_closed())
        .map_or(Reach::Unreachable, server::Tunnel::reach))
}

async fn proxy_to(
    tunnels: &Tunnels,
    pairing: server::Pairing,
    key: String,
) -> Result<String, String> {
    if let Some(proxy) = tunnels
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return Ok(format!("127.0.0.1:{}", proxy.port));
    }
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let tunnel = Arc::new(tokio::sync::Mutex::new(None));
    let mut proxies = tunnels.0.lock().unwrap_or_else(|e| e.into_inner());
    // Another call may have started one meanwhile.
    if let Some(proxy) = proxies.get(&key) {
        return Ok(format!("127.0.0.1:{}", proxy.port));
    }
    proxies.insert(
        key.clone(),
        Proxy {
            port,
            tunnel: tunnel.clone(),
        },
    );
    tauri::async_runtime::spawn(proxy(listener, pairing, key, tunnel));
    Ok(format!("127.0.0.1:{port}"))
}

/// Pipes every connection to `listener` over its own stream to the engine
/// with `key`.
async fn proxy(
    listener: tokio::net::TcpListener,
    pairing: server::Pairing,
    key: String,
    tunnel: Arc<tokio::sync::Mutex<Option<server::Tunnel>>>,
) {
    while let Ok((mut tcp, _)) = listener.accept().await {
        let (pairing, key, tunnel) = (pairing.clone(), key.clone(), tunnel.clone());
        tauri::async_runtime::spawn(async move {
            let stream = async {
                // One dial at a time: every stream shares the connection.
                let mut kept = tunnel.lock().await;
                let live = match kept.as_ref().filter(|t| !t.is_closed()) {
                    Some(live) => live.clone(),
                    None => kept.insert(pairing.tunnel(&key).await?).clone(),
                };
                drop(kept);
                live.open().await
            };
            match stream.await {
                Ok(mut stream) => {
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
                }
                Err(err) => tracing::debug!(%key, %err, "tunnel: can't reach"),
            }
        });
    }
}

async fn pair(
    address: String,
    code: String,
    name: String,
    public_key: Option<String>,
) -> Result<Paired, String> {
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
        public_key,
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
        engine_with("pairing", r#""auto_approve":"view_only""#).await
    }

    /// A server like [`local_engine`]'s in its own folder `name`, with
    /// `settings` and no internet (linked engines meet on loopback).
    async fn engine_with(
        name: &str,
        settings: &str,
    ) -> (server::Pairing, String, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("motif-remote-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pairing.json"),
            format!(r#"{{{settings},"devices":[],"log":[],"lan":false,"internet":false}}"#),
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
        let own = server::Pairing::in_memory().engine_key().to_owned();

        let wrong = pair(address.clone(), "WRONG".into(), "Phone".into(), None).await;
        assert_eq!(wrong.unwrap_err(), "unknown code");
        // The code has a space in the middle on screen.
        let spaced = format!("{} {}", &code[..4], &code[4..]);
        let paired = pair(address.clone(), spaced, "Phone".into(), Some(own.clone()))
            .await
            .unwrap();
        assert_eq!(paired.engine_key, key);
        let devices = pairing.subscribe().borrow().devices.clone();
        assert_eq!(
            devices[0].public_key,
            Some(own),
            "the host knows this app's engine"
        );
        // A retried request finds the code used up and adds no second device.
        let again = pair(address, code, "Phone".into(), None).await;
        assert_eq!(again.unwrap_err(), "unknown code");
        assert_eq!(pairing.subscribe().borrow().devices.len(), 1);

        let gone = pair("127.0.0.1:9".into(), "X".into(), "Phone".into(), None).await;
        assert_eq!(gone.unwrap_err(), "can't reach 127.0.0.1:9");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The answer's status line to `GET path` at `address`.
    async fn status(address: &str, path: &str) -> std::io::Result<String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut tcp = tokio::net::TcpStream::connect(address).await?;
        let request = format!("GET {path} HTTP/1.1\r\nHost: motif\r\nConnection: close\r\n\r\n");
        tcp.write_all(request.as_bytes()).await?;
        let mut response = String::new();
        tcp.read_to_string(&mut response).await?;
        Ok(response.lines().next().unwrap_or_default().to_owned())
    }

    /// The phone's app controls the desktop's engine through its own
    /// engine's tunnel, with the token pairing gave it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_linked_engine_is_controlled_through_the_tunnel() {
        let (desk, desk_address, desk_dir) = engine_with("desk", r#""auto_approve":"full""#).await;
        let (phone, _, phone_dir) = engine_with("phone", r#""auto_approve":null"#).await;
        let code = desk.start().code;
        let mut states = desk.subscribe();
        let open = states.wait_for(|s| s.lan.open);
        tokio::time::timeout(Duration::from_secs(5), open)
            .await
            .unwrap()
            .unwrap();
        let phone_key = phone.engine_key().to_owned();
        let paired = pair(desk_address.clone(), code, "Phone".into(), Some(phone_key))
            .await
            .unwrap();
        phone
            .link("Desk", &paired.engine_key, &[desk_address])
            .unwrap();

        let tunnels = Tunnels::default();
        let proxy = proxy_to(&tunnels, phone.clone(), paired.engine_key.clone())
            .await
            .unwrap();
        let again = proxy_to(&tunnels, phone, paired.engine_key).await.unwrap();
        assert_eq!(proxy, again, "one proxy per engine");
        let with_token = format!("/art/probe?token={}", paired.token);
        // The phone's endpoint starts once it has a link.
        let reached = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match status(&proxy, &with_token).await {
                    Ok(line) if !line.is_empty() => return line,
                    _ => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            }
        });
        // Past the auth check: this engine has no library.
        assert_eq!(reached.await.unwrap(), "HTTP/1.1 404 Not Found");
        assert_eq!(
            status(&proxy, "/art/probe").await.unwrap(),
            "HTTP/1.1 401 Unauthorized"
        );
        for dir in [desk_dir, phone_dir] {
            let _ = std::fs::remove_dir_all(dir);
        }
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
