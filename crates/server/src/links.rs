//! The iroh endpoint ([`net`]) while this engine has linked engines: it keeps
//! a [`net::Alpn::Link`] connection to each one it can reach, where they swap
//! addresses for each other's address book ([`Pairing::met`]), and shows how
//! each is reached ([`Pairing::set_reach`]). While the LAN switch is on it
//! also browses `_motif._tcp` over mDNS, whose adverts carry the iroh port,
//! so engines whose addresses changed find each other again without relays.
//! It binds again when the relay settings change. Over each link's life it
//! syncs playlists, plays and marks ([`net::Alpn::Sync`], sync.rs) and
//! swaps catalogs ([`net::Alpn::Catalog`], catalog.rs) when they meet and
//! after each change here; unlinking drops that engine's catalog. It serves
//! this engine's tracks to them ([`net::Alpn::Files`], files.rs).
//! Connections for other purposes are refused until the features that use
//! them exist.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use library::Library;
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};
use net::{Alpn, Connection, Net};
use protocol::{LibraryStatus, Reach};
use tokio::task::JoinSet;

use crate::{Pairing, catalog, files, pairing::relays, sync};

// shortcut: a fixed retry; back off if unreachable engines cost battery.
const RETRY: Duration = Duration::from_secs(60);
/// How long changes may gather before they are synced: a burst of edits
/// (a playlist being reordered) goes in one session.
const SETTLE: Duration = Duration::from_secs(1);
/// How soon a failed sync or catalog session is tried again.
const AGAIN: Duration = Duration::from_secs(5);

/// Runs the endpoint on UDP `port` (another if taken), which LAN addresses
/// of this engine share with its HTTP server ([`Pairing::link`]), syncing
/// `library` if there is one. Never ends.
pub(crate) async fn run(pairing: Pairing, port: u16, library: Option<Library>) {
    if let Some(library) = &library {
        tokio::spawn(drop_unlinked_catalogs(pairing.clone(), library.clone()));
    }
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
        pairing.set_net(Some(&net));
        let changed = states.wait_for(|s| s.net.links.is_empty() || relays(&s.net) != wanted);
        tokio::select! {
            // The endpoint closed under us: start again.
            () = keep_links(&net, &pairing, library.as_ref()) => {}
            _ = changed => {}
        }
        net.close().await;
        pairing.set_net(None);
        for (key, _) in pairing.linked() {
            pairing.set_reach(&key, Reach::Unreachable);
        }
    }
}

/// Drops the catalogs of engines no longer linked, whenever the links
/// change (and at start, for ones unlinked while it was off).
async fn drop_unlinked_catalogs(pairing: Pairing, library: Library) {
    let mut states = pairing.subscribe();
    let mut kept = None;
    loop {
        let mut linked: Vec<String> = pairing.linked().into_iter().map(|(key, _)| key).collect();
        linked.sort();
        if kept.as_ref() != Some(&linked) {
            let (library, keep) = (library.clone(), linked.clone());
            match tokio::task::spawn_blocking(move || library.keep_catalogs(&keep)).await {
                Ok(Ok(())) => kept = Some(linked),
                Ok(Err(err)) => tracing::warn!(%err, "can't drop unlinked catalogs"),
                Err(err) => tracing::warn!(%err, "can't drop unlinked catalogs"),
            }
        }
        if states.changed().await.is_err() {
            return;
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

async fn keep_links(net: &Net, pairing: &Pairing, library: Option<&Library>) {
    let links = Arc::new(Mutex::new(Links::default()));
    // Dropped with this future, which ends every connection's task.
    let mut tasks = JoinSet::new();
    let mut states = pairing.subscribe();
    let mut retry = tokio::time::interval(RETRY);
    let browse = browse_lan(pairing.clone(), net.key());
    tokio::pin!(browse);
    loop {
        tokio::select! {
            accepted = net.accept() => {
                let Some((alpn, conn)) = accepted else { return };
                match (alpn, library) {
                    (Alpn::Link, _) => {
                        let dialler = net::key(&conn);
                        let library = library.cloned();
                        tasks.spawn(keep(net.clone(), pairing.clone(), links.clone(), library, conn, dialler));
                    }
                    (Alpn::Sync | Alpn::Catalog, Some(library)) => {
                        let library = library.clone();
                        tasks.spawn(async move {
                            let done = match alpn {
                                Alpn::Catalog => catalog::session(&library, &conn, false).await,
                                _ => sync::session(&library, &conn, false).await,
                            };
                            if let Err(err) = done {
                                tracing::debug!(key = %net::key(&conn), %err, ?alpn, "can't keep in step");
                            }
                        });
                    }
                    (Alpn::Files, Some(library)) => {
                        let gate = pairing.clone();
                        let linked: net::Linked = Arc::new(move |key| gate.is_linked(key));
                        tasks.spawn(files::serve(library.clone(), conn, linked, None));
                    }
                    _ => conn.close(2u32.into(), b"not yet"),
                }
            }
            _ = retry.tick() => dial(net, pairing, library, &links, &mut tasks),
            Ok(()) = states.changed() => {
                forget_unlinked(pairing, &links);
                dial(net, pairing, library, &links, &mut tasks);
            }
            Some(_) = tasks.join_next() => {}
            () = &mut browse => {}
        }
    }
}

/// Browses `_motif._tcp` while the LAN switch is on, for where linked
/// engines are ([`found_on_lan`]). Never ends.
// mDNS costs wakeups: a query now and then (backing off) and every answer
// on the LAN, but only on engines with links.
async fn browse_lan(pairing: Pairing, me: String) {
    let mut states = pairing.subscribe();
    loop {
        if states.wait_for(|s| s.lan.allowed).await.is_err() {
            return std::future::pending().await;
        }
        let browsing = ServiceDaemon::new().and_then(|d| Ok((d.browse("_motif._tcp.local.")?, d)));
        let (events, daemon) = match browsing {
            Ok(browsing) => browsing,
            Err(err) => {
                tracing::warn!(%err, "can't browse the LAN");
                tokio::time::sleep(RETRY).await;
                continue;
            }
        };
        let off = states.wait_for(|s| !s.lan.allowed);
        tokio::pin!(off);
        loop {
            tokio::select! {
                event = events.recv_async() => match event {
                    Ok(ServiceEvent::ServiceResolved(found)) => found_on_lan(&pairing, &me, &found),
                    Ok(_) => {}
                    Err(_) => break,
                },
                _ = &mut off => break,
            }
        }
        let _ = daemon.shutdown();
    }
}

/// Puts where a linked engine advertises itself on the LAN (its addresses
/// with the iroh port) first in its address book entry.
fn found_on_lan(pairing: &Pairing, me: &str, found: &ResolvedService) {
    let key = found.get_property_val_str("id");
    let port = found
        .get_property_val_str("iroh")
        .and_then(|p| p.parse().ok());
    let (Some(key), Some(port)) = (key, port) else {
        return;
    };
    if key == me {
        return;
    }
    let mut lan: Vec<std::net::SocketAddr> = found
        .get_addresses()
        .iter()
        .map(|ip| (ip.to_ip_addr(), port).into())
        .collect();
    lan.sort();
    pairing.seen_on_lan(key, lan);
}

/// Dials every linked engine without a link or a dial under way.
fn dial(
    net: &Net,
    pairing: &Pairing,
    library: Option<&Library>,
    links: &Arc<Mutex<Links>>,
    tasks: &mut JoinSet<()>,
) {
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
        let library = library.cloned();
        tasks.spawn(async move {
            let dialled = net.connect(&key, &addr, Alpn::Link).await;
            lock(&links).dialling.remove(&key);
            match dialled {
                Ok(conn) => keep(net.clone(), pairing, links, library, conn, net.key()).await,
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

/// Swaps addresses on a new link, then shows its reach and keeps `library`
/// in sync until it closes. Of two links between the same engines (both
/// dialled at once), both ends keep the one the smaller key dialled.
async fn keep(
    net: Net,
    pairing: Pairing,
    links: Arc<Mutex<Links>>,
    library: Option<Library>,
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
    tokio::select! {
        () = net::follow_reach(&conn, |reach| pairing.set_reach(&key, reach)) => {}
        () = keep_in_step(&net, &pairing, library.as_ref(), &key, Alpn::Sync) => {}
        () = keep_in_step(&net, &pairing, library.as_ref(), &key, Alpn::Catalog) => {}
    }
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

/// Runs a [`Alpn::Sync`] or [`Alpn::Catalog`] session with the linked
/// engine `key` now and after every change here to what it carries
/// (the engine at the other end does the same). Never ends.
async fn keep_in_step(
    net: &Net,
    pairing: &Pairing,
    library: Option<&Library>,
    key: &str,
    alpn: Alpn,
) {
    let Some(library) = library else {
        return std::future::pending().await;
    };
    let synced = |s: &LibraryStatus| match alpn {
        Alpn::Catalog => (s.revision, 0, 0),
        _ => (s.playlists, s.plays, s.ratings),
    };
    let mut changes = library.subscribe();
    loop {
        let before = synced(&changes.borrow_and_update());
        let addr = pairing.linked().into_iter().find(|(k, _)| k == key);
        let mut failed = false;
        if let Some((_, addr)) = addr {
            let synced = match net.connect(key, &addr, alpn).await {
                Ok(conn) if alpn == Alpn::Catalog => catalog::session(library, &conn, true).await,
                Ok(conn) => sync::session(library, &conn, true).await,
                Err(err) => Err(err),
            };
            if let Err(err) = synced {
                tracing::debug!(%key, %err, ?alpn, "can't keep in step");
                failed = true;
            }
        }
        // A failed session (a lost connection) is tried again soon.
        let retry = async {
            match failed {
                true => tokio::time::sleep(AGAIN).await,
                false => std::future::pending().await,
            }
        };
        let closed = async { changes.wait_for(|s| synced(s) != before).await.is_err() };
        tokio::select! {
            closed = closed => if closed {
                return std::future::pending().await;
            },
            () = retry => {}
        }
        tokio::time::sleep(SETTLE).await;
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

    /// The UDP port `pairing`'s endpoint bound, once it has. Picking a free
    /// port up front races other tests binding port 0.
    async fn bound(pairing: &Pairing) -> u16 {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(port) = pairing.iroh_port() {
                    return port;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("never bound")
    }

    fn advert(key: &str, ip: &str, iroh: Option<&str>) -> ResolvedService {
        let txt: Vec<(&str, &str)> = [("id", key)]
            .into_iter()
            .chain(iroh.map(|p| ("iroh", p)))
            .collect();
        mdns_sd::ServiceInfo::new(
            "_motif._tcp.local.",
            "Motif x",
            "x.local.",
            ip,
            7700,
            &txt[..],
        )
        .unwrap()
        .as_resolved_service()
    }

    #[test]
    fn a_linked_engine_found_on_the_lan_goes_first_in_the_address_book() {
        let (pairing, other) = (Pairing::in_memory(), Pairing::in_memory());
        let key = other.engine_key();
        pairing
            .link("Other", key, &["10.0.0.2:41000".into()])
            .unwrap();
        let me = pairing.engine_key();
        found_on_lan(&pairing, me, &advert(key, "192.168.1.7", Some("7700")));
        // Older engines don't say where iroh is; strangers aren't recorded.
        found_on_lan(&pairing, me, &advert(key, "192.168.1.8", None));
        found_on_lan(
            &pairing,
            me,
            &advert(
                Pairing::in_memory().engine_key(),
                "192.168.1.9",
                Some("7700"),
            ),
        );
        let direct: Vec<_> = ["192.168.1.7:7700", "10.0.0.2:41000"]
            .map(|a| a.parse().unwrap())
            .into();
        assert_eq!(pairing.linked()[0].1.direct, direct);
    }

    fn library(name: &str) -> (Library, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("motif-links-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (Library::open(&dir).unwrap(), dir)
    }

    /// A tenth of a second of silence, 16-bit mono at 8 kHz.
    fn wav() -> Vec<u8> {
        let data = vec![0u8; 1600];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for field in [
            16u32.to_le_bytes(),
            [1, 0, 1, 0],
            8000u32.to_le_bytes(),
            16000u32.to_le_bytes(),
        ] {
            wav.extend_from_slice(&field);
        }
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    fn playlists(library: &Library) -> Vec<String> {
        let protocol::QueryResult::Playlists { playlists } =
            library.query(protocol::LibraryQuery::Playlists).unwrap()
        else {
            panic!()
        };
        playlists.into_iter().map(|p| p.name).collect()
    }

    async fn until(what: &str, library: &Library, done: impl Fn(&Library) -> bool) {
        let mut changes = library.subscribe();
        tokio::time::timeout(Duration::from_secs(20), async {
            while !done(library) {
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out: {what}"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn linked_engines_sync_and_share_catalogs_when_they_meet_and_on_change() {
        let (a, b) = (Pairing::in_memory(), Pairing::in_memory());
        for p in [&a, &b] {
            p.command(protocol::PairingCommand::SetInternet { allowed: false })
                .unwrap();
        }
        let ((library_a, dir_a), (library_b, dir_b)) = (library("a"), library("b"));
        library_a.set_device(a.engine_key()).unwrap();
        library_b.set_device(b.engine_key()).unwrap();
        // Made before they meet.
        library_a
            .query(protocol::LibraryQuery::CreatePlaylist {
                id: "p".into(),
                name: "Before".into(),
                paths: vec![],
            })
            .unwrap();
        b.link("A", a.engine_key(), &[]).unwrap();
        tokio::spawn(run(b.clone(), 0, Some(library_b.clone())));
        let b_port = bound(&b).await;
        a.link("B", b.engine_key(), &[format!("127.0.0.1:{b_port}")])
            .unwrap();
        tokio::spawn(run(a.clone(), 0, Some(library_a.clone())));
        until("made before", &library_b, |l| playlists(l) == ["Before"]).await;
        // A change on the end that was dialled goes the other way.
        library_b
            .query(protocol::LibraryQuery::RenamePlaylist {
                id: "p".into(),
                name: "After".into(),
            })
            .unwrap();
        until("renamed", &library_a, |l| playlists(l) == ["After"]).await;
        // A's files show on B, as A's.
        let music = dir_a.join("music");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::write(music.join("tone.wav"), wav()).unwrap();
        let path = music.to_string_lossy().into_owned();
        library_a
            .query(protocol::LibraryQuery::AddRoot { path })
            .unwrap();
        let on_b = |l: &Library| {
            let protocol::QueryResult::Tracks { tracks, .. } = l
                .query(protocol::LibraryQuery::Tracks {
                    search: None,
                    sort: vec![],
                    filter: Default::default(),
                    offset: 0,
                    limit: 10,
                })
                .unwrap()
            else {
                panic!()
            };
            tracks.into_iter().map(|t| t.devices).collect::<Vec<_>>()
        };
        until("catalog", &library_b, |l| {
            on_b(l) == [vec![a.engine_key().to_string()]]
        })
        .await;
        for dir in [dir_a, dir_b] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[tokio::test]
    async fn linked_engines_meet_and_learn_each_others_address() {
        let (a, b) = (Pairing::in_memory(), Pairing::in_memory());
        for p in [&a, &b] {
            p.command(protocol::PairingCommand::SetInternet { allowed: false })
                .unwrap();
        }
        // A met B on the LAN; B knows nothing of A yet.
        b.link("A", a.engine_key(), &[]).unwrap();
        tokio::spawn(run(b.clone(), 0, None));
        let b_port = bound(&b).await;
        a.link("B", b.engine_key(), &[format!("127.0.0.1:{b_port}")])
            .unwrap();
        tokio::spawn(run(a.clone(), 0, None));
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
