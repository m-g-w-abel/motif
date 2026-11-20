//! The iroh endpoint ([`net`]) while this engine has linked engines: it keeps
//! a [`net::Alpn::Link`] connection to each one it can reach, where they swap
//! addresses for each other's address book ([`Pairing::met`]), and shows how
//! each is reached ([`Pairing::set_reach`]). It binds again when the relay
//! settings change. Connections for other purposes are refused until the
//! features that use them exist.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use net::{Alpn, Connection, Net};
use protocol::Reach;
use tokio::task::JoinSet;

use crate::{Pairing, pairing::relays};

// shortcut: a fixed retry; back off if unreachable engines cost battery.
const RETRY: Duration = Duration::from_secs(60);

/// Runs the endpoint on UDP `port` (another if taken), which LAN addresses
/// of this engine share with its HTTP server ([`Pairing::link`]). Never ends.
pub(crate) async fn run(pairing: Pairing, port: u16) {
    let mut states = pairing.subscribe();
    loop {
        let wanted = {
            let state = states.borrow_and_update();
            (!state.net.links.is_empty()).then(|| relays(&state.net))
        };
        let Some(wanted) = wanted else {
            if states.changed().await.is_err() {
                return;
            }
            continue;
        };
        let gate = pairing.clone();
        let linked: net::Linked = Arc::new(move |key| gate.is_linked(key));
        let net = match Net::bind(pairing.secret(), port, &wanted, linked).await {
            Ok(net) => net,
            Err(err) => {
                tracing::warn!(%err, "iroh: can't start");
                tokio::time::sleep(RETRY).await;
                continue;
            }
        };
        tracing::info!(relays = ?wanted, "iroh: started");
        let changed = states.wait_for(|s| s.net.links.is_empty() || relays(&s.net) != wanted);
        tokio::select! {
            // The endpoint closed under us: start again.
            () = keep_links(&net, &pairing) => {}
            _ = changed => {}
        }
        net.close().await;
        for (key, _) in pairing.linked() {
            pairing.set_reach(&key, Reach::Unreachable);
        }
    }
}

/// A live link and which end dialled it.
struct Live {
    conn: Connection,
    dialler: String,
}

#[derive(Default)]
struct Links {
    live: HashMap<String, Live>,
    dialling: HashSet<String>,
}

async fn keep_links(net: &Net, pairing: &Pairing) {
    let links = Arc::new(Mutex::new(Links::default()));
    // Dropped with this future, which ends every connection's task.
    let mut tasks = JoinSet::new();
    let mut states = pairing.subscribe();
    let mut retry = tokio::time::interval(RETRY);
    loop {
        tokio::select! {
            accepted = net.accept() => {
                let Some((alpn, conn)) = accepted else { return };
                if alpn == Alpn::Link {
                    let dialler = net::key(&conn);
                    tasks.spawn(keep(net.clone(), pairing.clone(), links.clone(), conn, dialler));
                } else {
                    conn.close(2u32.into(), b"not yet");
                }
            }
            _ = retry.tick() => dial(net, pairing, &links, &mut tasks),
            Ok(()) = states.changed() => {
                forget_unlinked(pairing, &links);
                dial(net, pairing, &links, &mut tasks);
            }
            Some(_) = tasks.join_next() => {}
        }
    }
}

/// Dials every linked engine without a link or a dial under way.
fn dial(net: &Net, pairing: &Pairing, links: &Arc<Mutex<Links>>, tasks: &mut JoinSet<()>) {
    for (key, addr) in pairing.linked() {
        let mut shared = lock(links);
        if addr == net::PeerAddr::default()
            || shared.live.contains_key(&key)
            || !shared.dialling.insert(key.clone())
        {
            continue;
        }
        drop(shared);
        let (net, pairing, links) = (net.clone(), pairing.clone(), links.clone());
        tasks.spawn(async move {
            let dialled = net.connect(&key, &addr, Alpn::Link).await;
            lock(&links).dialling.remove(&key);
            match dialled {
                Ok(conn) => keep(net.clone(), pairing, links, conn, net.key()).await,
                Err(err) => tracing::debug!(%key, %err, "iroh: can't reach"),
            }
        });
    }
}

/// Closes links to engines no longer linked.
fn forget_unlinked(pairing: &Pairing, links: &Arc<Mutex<Links>>) {
    lock(links).live.retain(|key, live| {
        let linked = pairing.is_linked(key);
        if !linked {
            live.conn.close(1u32.into(), b"not linked");
        }
        linked
    });
}

/// Swaps addresses on a new link and shows its reach until it closes. Of two
/// links between the same engines (both dialled at once), both ends keep the
/// one the smaller key dialled.
async fn keep(
    net: Net,
    pairing: Pairing,
    links: Arc<Mutex<Links>>,
    conn: Connection,
    dialler: String,
) {
    let key = net::key(&conn);
    {
        let mut shared = lock(&links);
        let preferred = net.key().min(key.clone());
        if let Some(old) = shared.live.get(&key) {
            if old.dialler != dialler && dialler != preferred {
                conn.close(0u32.into(), b"duplicate");
                return;
            }
            old.conn.close(0u32.into(), b"duplicate");
        }
        let live = Live {
            conn: conn.clone(),
            dialler,
        };
        shared.live.insert(key.clone(), live);
    }
    match net::hello(&conn, &net.addr()).await {
        Ok(addr) => pairing.met(&key, addr),
        Err(err) => tracing::debug!(%key, %err, "iroh: no hello"),
    }
    net::follow_reach(&conn, |reach| pairing.set_reach(&key, reach)).await;
    let mut shared = lock(&links);
    if shared
        .live
        .get(&key)
        .is_some_and(|l| l.conn.stable_id() == conn.stable_id())
    {
        shared.live.remove(&key);
        pairing.set_reach(&key, Reach::Unreachable);
    }
}

fn lock(links: &Mutex<Links>) -> std::sync::MutexGuard<'_, Links> {
    links.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::PairingState;

    async fn wait_for(pairing: &Pairing, done: impl Fn(&PairingState) -> bool) {
        let mut states = pairing.subscribe();
        tokio::time::timeout(Duration::from_secs(10), states.wait_for(done))
            .await
            .expect("timed out")
            .unwrap();
    }

    fn port() -> u16 {
        std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test]
    async fn linked_engines_meet_and_learn_each_others_address() {
        let (a, b) = (Pairing::in_memory(), Pairing::in_memory());
        for p in [&a, &b] {
            p.command(protocol::PairingCommand::SetInternet { allowed: false })
                .unwrap();
        }
        let b_port = port();
        // A met B on the LAN; B knows nothing of A yet.
        a.link("B", b.engine_key(), &[format!("127.0.0.1:{b_port}")])
            .unwrap();
        b.link("A", a.engine_key(), &[]).unwrap();
        tokio::spawn(run(a.clone(), 0));
        tokio::spawn(run(b.clone(), b_port));
        let direct = |s: &PairingState| s.net.links.iter().all(|l| l.reach == Reach::Direct);
        wait_for(&a, direct).await;
        wait_for(&b, direct).await;
        let (_, told) = &b.linked()[0];
        assert!(!told.direct.is_empty(), "B learned where A is");

        // Unlinking closes the link.
        let device = b.subscribe().borrow().devices[0].id.clone();
        b.command(protocol::PairingCommand::Revoke { device })
            .unwrap();
        wait_for(&a, |s| {
            s.net.links.iter().all(|l| l.reach == Reach::Unreachable)
        })
        .await;
    }
}
