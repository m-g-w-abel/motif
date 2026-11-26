//! The iroh transport between linked engines: an endpoint on the engine's
//! identity key that lets in only the keys it is told are linked, one ALPN
//! per purpose ([`Alpn`]), and no lookup service: a peer is dialled at the
//! addresses it gave last time they met ([`PeerAddr`], swapped by [`hello`]).

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use iroh::endpoint::{ConnectError, ConnectionError};
pub use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey, TransportAddr,
    endpoint::{AfterHandshakeOutcome, BindOpts, EndpointHooks, presets},
};
use protocol::Reach;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// What a connection is for; each has its own ALPN, so later features plug
/// in without a protocol of their own to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alpn {
    /// Kept open between linked engines: swaps addresses, shows reach.
    Link,
    Remote,
    Sync,
    Catalog,
    Files,
}

impl Alpn {
    pub const ALL: [Alpn; 5] = [
        Alpn::Link,
        Alpn::Remote,
        Alpn::Sync,
        Alpn::Catalog,
        Alpn::Files,
    ];

    pub fn bytes(self) -> &'static [u8] {
        match self {
            Alpn::Link => b"motif/link/1",
            Alpn::Remote => b"motif/remote/1",
            Alpn::Sync => b"motif/sync/1",
            Alpn::Catalog => b"motif/catalog/1",
            Alpn::Files => b"motif/files/1",
        }
    }

    fn of(bytes: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.bytes() == bytes)
    }
}

/// Where a peer said it can be reached: the address book's entry.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PeerAddr {
    pub direct: Vec<SocketAddr>,
    /// Its home relay.
    pub relay: Option<String>,
}

/// Which relays the endpoint uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relays {
    Public,
    Own(String),
    /// No internet: LAN addresses only, directly.
    Off,
}

/// Whether `url` can be a relay.
pub fn check_relay(url: &str) -> Result<(), String> {
    url.parse::<RelayUrl>()
        .map(|_| ())
        .map_err(|_| format!("{url} is not a relay URL"))
}

/// Says whether a key (hex) is linked; asked at every handshake.
pub type Linked = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct Net {
    endpoint: Endpoint,
    internet: bool,
    /// Connections whose handshake is done, from [`handshakes`].
    accepted: Arc<tokio::sync::Mutex<mpsc::Receiver<(Alpn, Connection)>>>,
}

/// How long an incoming handshake may take.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// Incoming handshakes at once.
const HANDSHAKES: usize = 16;

impl Net {
    /// Binds UDP `port` (another if it is taken; 0 picks one) on every
    /// interface, with the engine's identity key.
    pub async fn bind(
        secret: [u8; 32],
        port: u16,
        relays: &Relays,
        linked: Linked,
    ) -> Result<Self, String> {
        let endpoint = match endpoint(secret, port, relays, linked.clone()).await {
            Err(err) if port != 0 => {
                tracing::info!(port, %err, "iroh: port taken, using another");
                endpoint(secret, 0, relays, linked).await
            }
            bound => bound,
        }?;
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(handshakes(endpoint.clone(), tx));
        Ok(Self {
            endpoint,
            internet: *relays != Relays::Off,
            accepted: Arc::new(tokio::sync::Mutex::new(rx)),
        })
    }

    /// This engine's key (hex): its iroh endpoint id.
    pub fn key(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// The UDP port it listens on (IPv4).
    pub fn port(&self) -> Option<u16> {
        let sockets = self.endpoint.bound_sockets();
        sockets.iter().find(|a| a.is_ipv4()).map(|a| a.port())
    }

    /// Where this engine can be reached now, for peers' address books.
    pub fn addr(&self) -> PeerAddr {
        let addr = self.endpoint.addr();
        PeerAddr {
            direct: addr.ip_addrs().copied().collect(),
            relay: addr.relay_urls().next().map(|u| u.to_string()),
        }
    }

    /// Dials the linked engine with `key` at `addr`.
    pub async fn connect(
        &self,
        key: &str,
        addr: &PeerAddr,
        alpn: Alpn,
    ) -> Result<Connection, Failed> {
        let id: EndpointId = key.parse().map_err(|_| Failed {
            refused: false,
            why: "bad engine key".into(),
        })?;
        let addr = EndpointAddr::from_parts(id, dialled(addr, self.internet));
        self.endpoint
            .connect(addr, alpn.bytes())
            .await
            .map_err(|err| Failed {
                refused: matches!(&err, ConnectError::Connection { source, .. } if not_linked(source)),
                why: err.to_string(),
            })
    }

    /// The next connection from a linked engine; `None` once closed. Safe
    /// to race in a `select!`: handshakes run on their own.
    pub async fn accept(&self) -> Option<(Alpn, Connection)> {
        self.accepted.lock().await.recv().await
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

/// Why a dial failed.
#[derive(Debug)]
pub struct Failed {
    /// The engine dialled isn't linked with this one ([`refused`]).
    pub refused: bool,
    why: String,
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why)
    }
}

impl From<Failed> for String {
    fn from(failed: Failed) -> String {
        failed.why
    }
}

/// Closes `conn` as the engine at the other end is no longer linked: it
/// can tell ([`refused`]) and drop its side of the link.
pub fn unlinked(conn: &Connection) {
    conn.close(NOT_LINKED.into(), b"not linked");
}

/// Whether the engine at the other end closed `conn` because it isn't
/// linked with this one (refused at the handshake, or unlinked since).
/// Only the peer's key can send that close (it comes in the encrypted
/// space; one in the handshake's shows as a transport error), so dropping
/// the link on it is safe.
pub fn refused(conn: &Connection) -> bool {
    conn.close_reason().is_some_and(|e| not_linked(&e))
}

fn not_linked(err: &ConnectionError) -> bool {
    matches!(err, ConnectionError::ApplicationClosed(c)
        if c.error_code == NOT_LINKED.into() && &c.reason[..] == b"not linked")
}

/// The close code of [`unlinked`] and the gate's refusal.
const NOT_LINKED: u32 = 1;

/// The key (hex) of the engine at the other end.
pub fn key(conn: &Connection) -> String {
    conn.remote_id().to_string()
}

/// How `conn` reaches its peer now.
pub fn reach(conn: &Connection) -> Reach {
    match conn.paths().iter().find(|p| p.is_selected()) {
        Some(path) if path.is_relay() => Reach::Relayed,
        Some(_) => Reach::Direct,
        None => Reach::Unreachable,
    }
}

/// Calls `f` with `conn`'s reach whenever it changes, until it closes.
pub async fn follow_reach(conn: &Connection, mut f: impl FnMut(Reach)) {
    let mut events = conn.path_events();
    let mut last = None;
    loop {
        let now = reach(conn);
        if last != Some(now) {
            f(now);
            last = Some(now);
        }
        tokio::select! {
            event = n0_future::StreamExt::next(&mut events) => if event.is_none() { break },
            _ = conn.closed() => break,
        }
    }
}

/// Tells the peer on a [`Alpn::Link`] connection where we are (a
/// [`PeerAddr`], perhaps with more), and hears where it is.
pub async fn hello<T>(conn: &Connection, mine: &T) -> Result<T, String>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let tell = async {
        let mut send = conn.open_uni().await.map_err(|e| e.to_string())?;
        let json = serde_json::to_vec(mine).map_err(|e| e.to_string())?;
        send.write_all(&json).await.map_err(|e| e.to_string())?;
        send.finish().map_err(|e| e.to_string())
    };
    let hear = async {
        let mut recv = conn.accept_uni().await.map_err(|e| e.to_string())?;
        let json = recv
            .read_to_end(HELLO_CAP)
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&json).map_err(|e| e.to_string())
    };
    let (told, heard) = tokio::join!(tell, hear);
    told?;
    heard
}

/// Accepts every incoming connection on `endpoint` and hands it to `tx`
/// once its handshake is done, each handshake on its own task (a slow one
/// holds up no other), until the endpoint closes.
async fn handshakes(endpoint: Endpoint, tx: mpsc::Sender<(Alpn, Connection)>) {
    // Anyone can call through a public relay: more at once are refused.
    let slots = Arc::new(tokio::sync::Semaphore::new(HANDSHAKES));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(slot) = slots.clone().try_acquire_owned() else {
            incoming.refuse();
            continue;
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let _slot = slot;
            match tokio::time::timeout(HANDSHAKE, incoming).await {
                Ok(Ok(conn)) => match Alpn::of(conn.alpn()) {
                    Some(alpn) => {
                        let _ = tx.send((alpn, conn)).await;
                    }
                    None => conn.close(0u32.into(), b"unknown ALPN"),
                },
                // Refused keys end here too, closed by the gate.
                Ok(Err(err)) => tracing::debug!(%err, "iroh: incoming failed"),
                Err(_) => tracing::debug!("iroh: handshake timed out"),
            }
        });
    }
}

/// A hello is a few addresses and a URL.
const HELLO_CAP: usize = 4096;

async fn endpoint(
    secret: [u8; 32],
    port: u16,
    relays: &Relays,
    linked: Linked,
) -> Result<Endpoint, String> {
    let any4 = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port);
    let any6 = SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), port);
    let builder = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&secret))
        .alpns(Alpn::ALL.map(|a| a.bytes().to_vec()).into())
        .hooks(Gate(linked))
        .clear_ip_transports()
        .bind_addr(any4)
        .and_then(|b| b.bind_addr_with_opts(any6, BindOpts::default().set_is_required(false)))
        .map_err(|e| e.to_string())?;
    let builder = match relays {
        Relays::Public => builder.relay_mode(RelayMode::Default),
        Relays::Own(url) => {
            let url: RelayUrl = url
                .parse()
                .map_err(|_| format!("{url} is not a relay URL"))?;
            builder.relay_mode(RelayMode::custom([url]))
        }
        Relays::Off => builder
            .relay_mode(RelayMode::Disabled)
            .clear_relay_transports(),
    };
    builder.bind().await.map_err(|e| e.to_string())
}

/// Refuses every key that isn't linked, right after the handshake: before
/// any stream, either way.
struct Gate(Linked);

impl std::fmt::Debug for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Gate")
    }
}

impl EndpointHooks for Gate {
    async fn after_handshake<'a>(&'a self, conn: &'a Connection) -> AfterHandshakeOutcome {
        if (self.0)(&key(conn)) {
            return AfterHandshakeOutcome::Accept;
        }
        tracing::info!(key = %key(conn), "iroh: refused an engine that isn't linked");
        AfterHandshakeOutcome::Reject {
            error_code: NOT_LINKED.into(),
            reason: b"not linked".to_vec(),
        }
    }
}

/// The addresses to try: all of them, or with no internet the LAN ones.
fn dialled(addr: &PeerAddr, internet: bool) -> impl Iterator<Item = TransportAddr> {
    let direct = addr.direct.iter().copied();
    let relay = addr.relay.as_deref().and_then(|u| u.parse().ok());
    direct
        .filter(move |a| internet || on_lan(a.ip()))
        .map(TransportAddr::Ip)
        .chain(relay.filter(|_| internet).map(TransportAddr::Relay))
}

fn on_lan(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => ip.is_private() || ip.is_link_local() || ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_unique_local() || ip.is_unicast_link_local() || ip.is_loopback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An engine with key `secret` that is linked with the one with `peer`.
    async fn net(secret: u8, peer: Option<u8>) -> Net {
        let peer = peer.map(key_of);
        let linked: Linked = Arc::new(move |key| peer.as_deref() == Some(key));
        Net::bind([secret; 32], 0, &Relays::Off, linked)
            .await
            .unwrap()
    }

    fn key_of(secret: u8) -> String {
        SecretKey::from_bytes(&[secret; 32]).public().to_string()
    }

    /// Only loopback: tests don't depend on the machine's interfaces.
    fn loopback(net: &Net) -> PeerAddr {
        let port = net.addr().direct[0].port();
        PeerAddr {
            direct: vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)],
            relay: None,
        }
    }

    #[tokio::test]
    async fn linked_engines_connect_by_key_and_swap_addresses() {
        let (a, b) = (net(1, Some(2)).await, net(2, Some(1)).await);
        assert_eq!(a.key(), key_of(1));
        let (at_a, at_b, b_key) = (loopback(&a), loopback(&b), b.key());
        let (dialled, accepted) = tokio::join!(a.connect(&b_key, &at_b, Alpn::Link), b.accept());
        let (dialled, (alpn, accepted)) = (dialled.unwrap(), accepted.unwrap());
        assert_eq!(
            (alpn, key(&accepted), key(&dialled)),
            (Alpn::Link, a.key(), b.key())
        );
        let (heard_by_a, heard_by_b) =
            tokio::join!(hello(&dialled, &at_a), hello(&accepted, &at_b));
        assert_eq!(heard_by_a.unwrap(), at_b);
        assert_eq!(heard_by_b.unwrap(), at_a);
        assert_eq!(reach(&dialled), Reach::Direct);
    }

    /// A select loop races accepting with other events: an event firing
    /// mid-handshake must not drop the connection being accepted.
    #[tokio::test]
    async fn a_handshake_survives_accepting_being_raced() {
        let (a, b) = (net(1, Some(2)).await, net(2, Some(1)).await);
        let (at_b, b_key) = (loopback(&b), b.key());
        let dialled = a.connect(&b_key, &at_b, Alpn::Files);
        let accepting = async {
            loop {
                tokio::select! {
                    accepted = b.accept() => return accepted,
                    () = tokio::time::sleep(Duration::from_micros(200)) => {}
                }
            }
        };
        let (dialled, accepted) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(dialled, accepting)
        })
        .await
        .expect("timed out");
        let conn = dialled.expect("the handshake was dropped");
        assert_eq!(accepted.unwrap().0, Alpn::Files);
        assert!(conn.close_reason().is_none());
    }

    #[tokio::test]
    async fn an_unlinked_key_is_refused() {
        let (a, b) = (net(1, Some(2)).await, net(2, None).await);
        let (at_b, b_key) = (loopback(&b), b.key());
        let refused = tokio::time::timeout(Duration::from_secs(5), async {
            match a.connect(&b_key, &at_b, Alpn::Sync).await {
                Err(_) => true,
                // Refused just after the handshake: no stream gets through.
                Ok(conn) => conn.accept_uni().await.is_err(),
            }
        });
        let accepted = tokio::time::timeout(Duration::from_millis(500), b.accept());
        let (refused, accepted) = tokio::join!(refused, accepted);
        assert!(refused.unwrap());
        assert!(accepted.is_err(), "b let an unlinked engine in");
    }

    /// An engine that is refused, or unlinked while connected, can tell it
    /// from any other loss, and so drop its side of the link.
    #[tokio::test]
    async fn an_engine_can_tell_it_was_refused_or_unlinked() {
        let (a, b) = (net(1, Some(2)).await, net(2, None).await);
        let (at_b, b_key) = (loopback(&b), b.key());
        let seen = tokio::time::timeout(Duration::from_secs(5), async {
            match a.connect(&b_key, &at_b, Alpn::Link).await {
                Err(err) => err.refused,
                Ok(conn) => {
                    conn.closed().await;
                    refused(&conn)
                }
            }
        });
        assert!(seen.await.unwrap(), "refusal not seen");

        let (a, b) = (net(1, Some(2)).await, net(2, Some(1)).await);
        let (at_b, b_key) = (loopback(&b), b.key());
        let (dialled, accepted) = tokio::join!(a.connect(&b_key, &at_b, Alpn::Link), b.accept());
        let (dialled, (_, accepted)) = (dialled.unwrap(), accepted.unwrap());
        unlinked(&accepted);
        dialled.closed().await;
        assert!(refused(&dialled));
        // Any other close isn't.
        let (dialled, accepted) = tokio::join!(a.connect(&b_key, &at_b, Alpn::Link), b.accept());
        let (dialled, (_, accepted)) = (dialled.unwrap(), accepted.unwrap());
        accepted.close(0u32.into(), b"duplicate");
        dialled.closed().await;
        assert!(!refused(&dialled));
    }

    #[test]
    fn without_internet_only_lan_addresses_are_dialled() {
        let addr = PeerAddr {
            direct: [
                "192.168.1.5:7700",
                "8.8.8.8:7700",
                "[fe80::1]:7700",
                "127.0.0.1:1",
            ]
            .map(|a| a.parse().unwrap())
            .into(),
            relay: Some("https://relay.example.com".into()),
        };
        let lan: Vec<_> = dialled(&addr, false).collect();
        assert_eq!(lan.len(), 3);
        assert!(lan.iter().all(|a| matches!(a, TransportAddr::Ip(_))));
        assert_eq!(dialled(&addr, true).count(), 5);
    }

    #[test]
    fn relay_urls_are_checked() {
        assert!(check_relay("https://relay.example.com").is_ok());
        assert!(check_relay("not a url").is_err());
    }
}
