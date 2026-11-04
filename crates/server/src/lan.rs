//! Opening the engine to the LAN while [`Pairing::lan_wanted`] says so: the
//! listener moves from the configured address (loopback) to every interface
//! on the same port, which still serves loopback (Linux won't bind both), and
//! `_motif._tcp` is advertised over mDNS.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
};

use crate::Pairing;

pub(crate) struct LanListener {
    /// `None` only while no bind worked; retried every second.
    listener: Option<TcpListener>,
    bound: SocketAddr,
    /// The configured address, used while the LAN is closed.
    local: SocketAddr,
    pairing: Pairing,
    states: watch::Receiver<protocol::PairingState>,
    advert: Option<Advert>,
}

impl LanListener {
    /// Binds `addr` (port 0 picks one, kept for the LAN) and opens the LAN
    /// at once if pairing wants it.
    pub(crate) async fn bind(addr: SocketAddr, pairing: Pairing) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        let mut me = Self {
            listener: Some(listener),
            bound: local,
            local,
            states: pairing.subscribe(),
            pairing,
            advert: None,
        };
        me.update().await;
        Ok(me)
    }

    pub(crate) fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Moves the listener and the advert to where pairing wants them; says
    /// how long to wait before asking again if nothing changes.
    async fn update(&mut self) -> Option<Duration> {
        let (wanted, expires) = self.pairing.lan_wanted();
        let target = if wanted {
            let any: IpAddr = match self.local {
                SocketAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
                SocketAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
            };
            SocketAddr::new(any, self.local.port())
        } else {
            self.local
        };
        if target != self.bound || self.listener.is_none() {
            // Frees the port first: the new socket can't share it.
            self.listener = None;
            match TcpListener::bind(target).await {
                Ok(listener) => {
                    tracing::info!(%target, "listening");
                    (self.listener, self.bound) = (Some(listener), target);
                }
                Err(err) => {
                    tracing::warn!(%target, %err, "can't listen");
                    if let Ok(listener) = TcpListener::bind(self.bound).await {
                        self.listener = Some(listener);
                    }
                }
            }
        }
        let open = self.listener.is_some() && self.bound.ip().is_unspecified();
        let addresses = open.then(|| lan_addresses(self.local.port()));
        if open != self.advert.is_some() {
            self.advert = open
                .then(|| Advert::start("_motif._tcp", &self.pairing, self.local.port()))
                .flatten();
        }
        let stale = {
            let shown = &self.states.borrow().lan;
            shown.open != open || shown.addresses != addresses_text(addresses.as_deref())
        };
        if stale {
            self.pairing.set_lan(addresses);
        }
        let now = engine::wall_clock_ms();
        match self.listener {
            None => Some(Duration::from_secs(1)),
            Some(_) => expires.map(|at| Duration::from_millis(at.saturating_sub(now) + 1)),
        }
    }
}

fn addresses_text(addresses: Option<&[SocketAddr]>) -> Vec<String> {
    addresses
        .unwrap_or_default()
        .iter()
        .map(|a| a.to_string())
        .collect()
}

impl axum::serve::Listener for LanListener {
    type Io = TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (TcpStream, SocketAddr) {
        let mut wait = None;
        loop {
            let sleep = tokio::time::sleep(wait.unwrap_or(Duration::MAX));
            let listener = &self.listener;
            tokio::select! {
                Some(accepted) = async { Some(listener.as_ref()?.accept().await) } => {
                    match accepted {
                        Ok(accepted) => return accepted,
                        Err(err) => {
                            // As axum's own listener does: connection errors are
                            // the peer's, others (out of files) need a moment.
                            tracing::debug!(%err, "accept failed");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                Ok(()) = self.states.changed() => {}
                () = sleep, if wait.is_some() => {}
            }
            wait = self.update().await;
        }
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok(self.local)
    }
}

/// A service (`_motif._tcp`, `_mpd._tcp`) with the engine key in TXT,
/// withdrawn when dropped.
pub(crate) struct Advert {
    daemon: ServiceDaemon,
    name: String,
}

impl Advert {
    pub(crate) fn start(service: &str, pairing: &Pairing, port: u16) -> Option<Self> {
        let key = pairing.engine_key();
        let short = &key[..8];
        let service = ServiceInfo::new(
            &format!("{service}.local."),
            &format!("Motif {short}"),
            &format!("motif-{short}.local."),
            (),
            port,
            &[("id", key)][..],
        )
        .map(ServiceInfo::enable_addr_auto);
        let advert = service.and_then(|service| {
            let daemon = ServiceDaemon::new()?;
            let name = service.get_fullname().to_owned();
            daemon.register(service)?;
            Ok(Self { daemon, name })
        });
        advert
            .inspect_err(|err| tracing::warn!(%err, "can't advertise over mDNS"))
            .ok()
    }
}

impl Drop for Advert {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.name);
        let _ = self.daemon.shutdown();
    }
}

/// This machine's IPv4 LAN addresses with `port`, the likeliest first.
// shortcut: IPv4 only, so an IPv6-only LAN gets no address (mDNS still works); add IPv6 if someone has one.
fn lan_addresses(port: u16) -> Vec<SocketAddr> {
    let mut found: Vec<(String, Ipv4Addr)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) => Some((i.name, ip)),
            IpAddr::V6(_) => None,
        })
        .filter(|(_, ip)| !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified())
        .collect();
    found.sort_by_key(|(name, ip)| likeliness(name, *ip));
    found.dedup_by_key(|(_, ip)| *ip);
    found
        .into_iter()
        .map(|(_, ip)| SocketAddr::new(ip.into(), port))
        .collect()
}

/// Sort key, smallest likeliest: real interfaces before virtual ones
/// (containers, VMs, VPNs), then home networks' usual ranges.
fn likeliness(name: &str, ip: Ipv4Addr) -> (bool, u8) {
    const VIRTUAL: &[&str] = &[
        "docker",
        "br-",
        "veth",
        "virbr",
        "vmnet",
        "vboxnet",
        "lxc",
        "lxd",
        "podman",
        "cni",
        "flannel",
        "tun",
        "tap",
        "wg",
        "tailscale",
        "zt",
        "utun",
    ];
    let virtual_ = VIRTUAL.iter().any(|v| name.starts_with(v));
    let [a, b, ..] = ip.octets();
    let range = match (a, b) {
        (192, 168) => 0,
        (10, _) => 1,
        (172, 16..=31) => 2,
        _ => 3,
    };
    (virtual_, range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_networks_come_before_containers_and_vpns() {
        let mut found = [
            ("docker0", Ipv4Addr::new(172, 17, 0, 1)),
            ("wg0", Ipv4Addr::new(10, 8, 0, 2)),
            ("eth1", Ipv4Addr::new(10, 0, 0, 5)),
            ("wlan0", Ipv4Addr::new(192, 168, 1, 20)),
            ("eth2", Ipv4Addr::new(100, 64, 3, 1)),
        ];
        found.sort_by_key(|(name, ip)| likeliness(name, *ip));
        let names: Vec<_> = found.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["wlan0", "eth1", "eth2", "wg0", "docker0"]);
    }
}
