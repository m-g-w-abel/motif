//! Remote control away from home: a linked engine's app opens
//! [`net::Alpn::Remote`] connections to this one and sends plain HTTP over
//! each stream, so the same `/ws`, artwork and tokens work as on the LAN
//! ([`serve`]). Tunnelled requests show as from [`PEER`], which no real
//! socket can be, and carry the [`Dialler`]'s key: they always need the
//! token of the device paired with that engine's app, whether the LAN is
//! open or not.

use std::net::{Ipv4Addr, SocketAddr};

use axum::{Router, extract::connect_info::MockConnectInfo, serve::ListenerExt};
use net::{Connection, RecvStream, SendStream};
use tokio::{
    io::{AsyncRead, AsyncWrite, Join},
    sync::mpsc,
};

/// Where tunnelled requests seem to come from.
pub(crate) const PEER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

/// Whether a request came through the tunnel.
pub(crate) fn tunnelled(peer: SocketAddr) -> bool {
    peer == PEER
}

/// The key (hex) of the engine that opened a tunnelled request's stream.
#[derive(Debug, Clone)]
pub(crate) struct Dialler(pub(crate) String);

/// Serves `router` over every stream sent to the returned sender, each
/// with the key of the engine that opened it.
pub(crate) fn serve<Io>(router: Router) -> (mpsc::Sender<(Io, String)>, impl Future<Output = ()>)
where
    Io: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (tx, rx) = mpsc::channel(8);
    // Tapping is a no-op; axum knows the peer address of tapped listeners.
    let streams = Streams(rx).tap_io(|_| {});
    let app = router
        .layer(MockConnectInfo(PEER))
        .into_make_service_with_connect_info::<Dialler>();
    (tx, async move {
        let _ = axum::serve(streams, app).await;
    })
}

pub type Stream = Join<RecvStream, SendStream>;

/// Hands every stream the app at the other end of `conn` opens to
/// `streams`, until it closes.
pub(crate) async fn accept(conn: Connection, streams: mpsc::Sender<(Stream, String)>) {
    let key = net::key(&conn);
    while let Ok((send, recv)) = conn.accept_bi().await {
        if streams
            .send((tokio::io::join(recv, send), key.clone()))
            .await
            .is_err()
        {
            return;
        }
    }
}

struct Streams<Io>(mpsc::Receiver<(Io, String)>);

impl<Io> axum::serve::Listener for Streams<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Io = Io;
    type Addr = Dialler;

    async fn accept(&mut self) -> (Io, Dialler) {
        match self.0.recv().await {
            Some((io, key)) => (io, Dialler(key)),
            // No more tunnels: axum::serve waits for its accept forever.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Dialler> {
        Ok(Dialler(String::new()))
    }
}

/// A connection for controlling a linked engine from away: each
/// [`Tunnel::open`] is a stream to its HTTP server, as a TCP connection is
/// on the LAN. Dialled with [`crate::Pairing::tunnel`].
#[derive(Clone)]
pub struct Tunnel(Connection);

impl Tunnel {
    pub async fn open(&self) -> Result<Stream, String> {
        let (send, recv) = self.0.open_bi().await.map_err(|e| e.to_string())?;
        Ok(tokio::io::join(recv, send))
    }

    /// How it reaches the engine now.
    pub fn reach(&self) -> protocol::Reach {
        net::reach(&self.0)
    }

    pub fn is_closed(&self) -> bool {
        self.0.close_reason().is_some()
    }
}

impl crate::Pairing {
    /// Dials the linked engine with `key` to control it from away.
    pub async fn tunnel(&self, key: &str) -> Result<Tunnel, String> {
        let net = self.net().ok_or("not linked with any engine")?;
        let linked = self.linked().into_iter().find(|(k, _)| k == key);
        let (_, addr) = linked.ok_or("not linked with that engine")?;
        let conn = net.connect(key, &addr, net::Alpn::Remote).await?;
        Ok(Tunnel(conn))
    }
}
