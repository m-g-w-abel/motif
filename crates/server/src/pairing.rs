//! Who may use this engine: its identity, the paired devices with their
//! scopes and token hashes, one-time pairing codes and approval, and the
//! clients connected now, and how linked engines are reached over iroh
//! (their address book, the relay settings). Kept in the data directory
//! (`identity.key`, `pairing.json`).

use std::{
    collections::HashMap,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use axum::http::StatusCode;
use ed25519_dalek::{SigningKey, VerifyingKey};
use protocol::{
    Connection, LanState, LinkState, MpdState, NetState, PairRequest, Paired, PairedDevice,
    PairingAction, PairingCode, PairingCommand, PairingLogEntry, PairingState, PendingDevice,
    Preset, Reach, Scope,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, oneshot, watch};

/// How long a pairing code works.
const CODE_TTL_MS: u64 = 5 * 60 * 1000;
/// How long a redeemed code waits for the host's approval.
const APPROVAL_WAIT: Duration = Duration::from_secs(5 * 60);
const LOG_CAP: usize = 100;
const NAME_CAP: usize = 64;
/// Direct addresses kept per linked engine; the newest first.
const ADDRESS_CAP: usize = 8;
/// No 0/O or 1/I to mistype; 8 of them make 40 bits, too many to guess in a
/// code's lifetime.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A client's identity: this machine, or a paired device by id.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Who {
    Local,
    Device(String),
}

#[derive(Clone)]
pub struct Pairing(Arc<Shared>);

struct Shared {
    /// `pairing.json`, or `None` to keep everything in memory.
    file: Option<PathBuf>,
    engine_key: String,
    /// The identity key's secret half, for the iroh endpoint.
    secret: [u8; 32],
    inner: Mutex<Inner>,
    state: watch::Sender<PairingState>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    auto_approve: Option<Preset>,
    devices: Vec<Stored>,
    log: Vec<PairingLogEntry>,
    /// The LAN switch, [`LanState::allowed`].
    #[serde(default)]
    lan: bool,
    #[serde(default)]
    mpd: SavedMpd,
    /// The internet switch, [`NetState::internet`].
    #[serde(default = "yes")]
    internet: bool,
    #[serde(default)]
    own_relay: Option<String>,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            auto_approve: None,
            devices: Vec::new(),
            log: Vec::new(),
            lan: false,
            mpd: SavedMpd::default(),
            internet: true,
            own_relay: None,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Default, Serialize, Deserialize)]
struct SavedMpd {
    enabled: bool,
    /// Plain text: MPD clients send it so anyway.
    password: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    device: PairedDevice,
    /// `None` for an engine this one paired with ([`Pairing::link`]): it
    /// connects by its key, not a token.
    token_hash: Option<String>,
    /// A linked engine's address book entry: where it was last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    address: Option<net::PeerAddr>,
}

struct Inner {
    saved: Saved,
    code: Option<PairingCode>,
    code_ttl_ms: u64,
    pending: Vec<(PendingDevice, oneshot::Sender<Option<Preset>>)>,
    connections: Vec<(Connection, Arc<Notify>)>,
    next_id: u32,
    /// Where the server listens on the LAN; `None` while it doesn't.
    lan: Option<Vec<SocketAddr>>,
    mpd_port: u16,
    mpd_listening: bool,
    /// How linked engines are reached now, by key; missing is unreachable.
    reach: HashMap<String, Reach>,
    iroh_port: Option<u16>,
}

impl Pairing {
    /// Loads the engine's identity and paired devices from `dir`, making a
    /// new identity on first run.
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        let key_file = dir.join("identity.key");
        let secret: [u8; 32] = match std::fs::read(&key_file) {
            Ok(bytes) => bytes.try_into().map_err(|_| {
                std::io::Error::other(format!("{} is not a key", key_file.display()))
            })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let secret = random();
                replace(&key_file, &secret)?;
                secret
            }
            Err(err) => return Err(err),
        };
        let file = dir.join("pairing.json");
        let saved = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                // Fails closed: nobody stays paired, the bad file is kept.
                tracing::warn!(%err, "can't read the paired devices, starting over");
                let _ = std::fs::rename(&file, file.with_extension("json.bad"));
                Saved::default()
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(err) => return Err(err),
        };
        Ok(Self::new(Some(file), secret, saved))
    }

    /// A new identity and no paired devices, forgotten on quitting.
    pub fn in_memory() -> Self {
        Self::new(None, random(), Saved::default())
    }

    fn new(file: Option<PathBuf>, secret: [u8; 32], mut saved: Saved) -> Self {
        // Full pairings from before Sync existed are Full still.
        for stored in &mut saved.devices {
            let scopes = &mut stored.device.scopes;
            if scopes.contains(&Scope::Pairing) && !scopes.contains(&Scope::Sync) {
                scopes.push(Scope::Sync);
            }
        }
        let engine_key = hex(SigningKey::from_bytes(&secret).verifying_key().as_bytes());
        let pairing = Self(Arc::new(Shared {
            file,
            engine_key,
            secret,
            inner: Mutex::new(Inner {
                saved,
                code: None,
                code_ttl_ms: CODE_TTL_MS,
                pending: Vec::new(),
                connections: Vec::new(),
                next_id: 1,
                lan: None,
                mpd_port: 6600,
                mpd_listening: false,
                reach: HashMap::new(),
                iroh_port: None,
            }),
            state: watch::Sender::new(PairingState::default()),
        }));
        pairing.publish(&pairing.lock());
        pairing
    }

    /// The engine's public key (ed25519, hex).
    pub fn engine_key(&self) -> &str {
        &self.0.engine_key
    }

    pub(crate) fn secret(&self) -> [u8; 32] {
        self.0.secret
    }

    /// The linked engines' keys and where they were last.
    pub(crate) fn linked(&self) -> Vec<(String, net::PeerAddr)> {
        self.lock()
            .saved
            .devices
            .iter()
            .filter_map(|d| {
                Some((
                    linked_key(d)?.to_owned(),
                    d.address.clone().unwrap_or_default(),
                ))
            })
            .collect()
    }

    pub(crate) fn is_linked(&self, key: &str) -> bool {
        self.lock()
            .saved
            .devices
            .iter()
            .any(|d| linked_key(d) == Some(key))
    }

    /// Records where the linked engine with `key` said it is, on meeting it.
    pub(crate) fn met(&self, key: &str, address: net::PeerAddr) {
        let mut inner = self.lock();
        let Some(stored) = inner
            .saved
            .devices
            .iter_mut()
            .find(|d| linked_key(d) == Some(key))
        else {
            return;
        };
        stored.device.last_seen_ms = Some(engine::wall_clock_ms());
        stored.address = Some(address);
        self.save(&inner);
        self.publish(&inner);
    }

    /// Puts `lan` first in the address book entry of the linked engine with
    /// `key`, found on the LAN (mDNS).
    pub(crate) fn seen_on_lan(&self, key: &str, lan: Vec<SocketAddr>) {
        let mut inner = self.lock();
        let Some(stored) = inner
            .saved
            .devices
            .iter_mut()
            .find(|d| linked_key(d) == Some(key))
        else {
            return;
        };
        if lan_first(stored.address.get_or_insert_default(), lan) {
            self.save(&inner);
            self.publish(&inner);
        }
    }

    /// The iroh endpoint's UDP port while it runs, for the LAN advert.
    pub(crate) fn iroh_port(&self) -> Option<u16> {
        self.lock().iroh_port
    }

    pub(crate) fn set_iroh_port(&self, port: Option<u16>) {
        let mut inner = self.lock();
        if inner.iroh_port != port {
            inner.iroh_port = port;
            self.publish(&inner);
        }
    }

    /// Says how the linked engine with `key` is reached now.
    pub(crate) fn set_reach(&self, key: &str, reach: Reach) {
        let mut inner = self.lock();
        if inner.reach.get(key).copied().unwrap_or(Reach::Unreachable) != reach {
            inner.reach.insert(key.to_owned(), reach);
            self.publish(&inner);
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<PairingState> {
        self.0.state.subscribe()
    }

    /// Makes a one-time code, replacing any earlier one, and opens the LAN.
    pub fn start(&self) -> PairingCode {
        let mut inner = self.lock();
        let code = new_code(&mut inner);
        self.save(&inner);
        self.publish(&inner);
        code
    }

    /// Sets the LAN switch ([`LanState::allowed`]).
    pub fn allow_lan(&self, allowed: bool) {
        self.command(PairingCommand::SetLan { allowed })
            .expect("SetLan can't fail");
    }

    /// Whether the server should listen on the LAN, and when to ask again
    /// (the live code's expiry) if nothing else changes.
    pub(crate) fn lan_wanted(&self) -> (bool, Option<u64>) {
        let inner = self.lock();
        let expires = inner
            .code
            .as_ref()
            .map(|c| c.expires_at_ms)
            .filter(|&at| at > engine::wall_clock_ms());
        let wanted = inner.saved.lan
            && (expires.is_some() || !inner.pending.is_empty() || !inner.saved.devices.is_empty());
        (wanted, expires)
    }

    /// Turns the MPD server on or off ([`MpdState::enabled`]).
    pub fn enable_mpd(&self, enabled: bool) {
        self.command(PairingCommand::SetMpd { enabled })
            .expect("SetMpd can't fail");
    }

    /// The MPD server's port (6600 unless set); not saved.
    pub fn set_mpd_port(&self, port: u16) {
        let mut inner = self.lock();
        inner.mpd_port = port;
        self.publish(&inner);
    }

    /// Where the MPD server should listen and its password; `None` while
    /// it is off.
    pub(crate) fn mpd_wanted(&self) -> Option<(SocketAddr, Option<String>)> {
        let inner = self.lock();
        let mpd = &inner.saved.mpd;
        let ip: std::net::IpAddr = if inner.saved.lan {
            std::net::Ipv4Addr::UNSPECIFIED.into()
        } else {
            std::net::Ipv4Addr::LOCALHOST.into()
        };
        mpd.enabled
            .then(|| (SocketAddr::new(ip, inner.mpd_port), mpd.password.clone()))
    }

    pub(crate) fn set_mpd_listening(&self, listening: bool) {
        let mut inner = self.lock();
        if inner.mpd_listening != listening {
            inner.mpd_listening = listening;
            self.publish(&inner);
        }
    }

    pub(crate) fn lan_open(&self) -> bool {
        self.lock().lan.is_some()
    }

    /// Says where the server listens on the LAN now (`None`: it doesn't),
    /// and closes the connections from other machines when it stops.
    pub(crate) fn set_lan(&self, addresses: Option<Vec<SocketAddr>>) {
        let mut inner = self.lock();
        if addresses.is_none() {
            for (connection, close) in &inner.connections {
                let remote = connection
                    .address
                    .parse::<SocketAddr>()
                    .is_ok_and(|a| !a.ip().to_canonical().is_loopback());
                if remote {
                    close.notify_one();
                }
            }
        }
        inner.lan = addresses;
        self.publish(&inner);
    }

    /// The paired device holding `token`.
    pub(crate) fn authenticate(&self, token: &str) -> Option<String> {
        let hash = hash(token);
        let inner = self.lock();
        let stored = inner
            .saved
            .devices
            .iter()
            .find(|d| d.token_hash.as_ref() == Some(&hash))?;
        Some(stored.device.id.clone())
    }

    /// Whether `who` may do what needs `scope`, now: a revoked device may
    /// do nothing.
    pub(crate) fn allows(&self, who: &Who, scope: Scope) -> bool {
        self.scopes(who).contains(&scope)
    }

    /// What `who` may do now: everything for this machine, nothing for a
    /// revoked device.
    pub(crate) fn scopes(&self, who: &Who) -> Vec<Scope> {
        match who {
            Who::Local => Preset::Full.scopes(),
            Who::Device(id) => self
                .lock()
                .saved
                .devices
                .iter()
                .find(|d| &d.device.id == id)
                .map(|d| d.device.scopes.clone())
                .unwrap_or_default(),
        }
    }

    /// Lists a new connection; the returned `Notify` fires when it should
    /// close. Call [`Pairing::disconnected`] when it does.
    pub(crate) fn connected(&self, who: &Who, peer: SocketAddr) -> (u32, Arc<Notify>) {
        let mut inner = self.lock();
        let id = inner.next_id();
        let now = engine::wall_clock_ms();
        let device = match who {
            Who::Local => None,
            Who::Device(device) => Some(device.clone()),
        };
        let close = Arc::new(Notify::new());
        // Last seen is saved on disconnecting only: connected devices show as
        // connected, and every save waits for the disk under this lock.
        inner.connections.push((
            Connection {
                id,
                device,
                address: peer.to_string(),
                connected_at_ms: now,
            },
            close.clone(),
        ));
        self.publish(&inner);
        (id, close)
    }

    pub(crate) fn disconnected(&self, id: u32) {
        let mut inner = self.lock();
        let Some(at) = inner.connections.iter().position(|(c, _)| c.id == id) else {
            return;
        };
        let (connection, _) = inner.connections.remove(at);
        if let Some(device) = connection.device {
            self.seen(&mut inner, &device, engine::wall_clock_ms());
        }
        self.publish(&inner);
    }

    /// Redeems a code for a token, waiting for the host's approval unless
    /// devices are let in automatically.
    pub(crate) async fn redeem(
        &self,
        request: PairRequest,
        peer: SocketAddr,
    ) -> Result<Paired, (StatusCode, &'static str)> {
        let public_key = match request.public_key {
            Some(key) => Some(parse_key(&key).ok_or((StatusCode::BAD_REQUEST, "bad public key"))?),
            None => None,
        };
        let name = clean_name(&request.name);
        let (id, approval) = {
            let mut inner = self.lock();
            let now = engine::wall_clock_ms();
            match &inner.code {
                // Only a match uses the code up, so wrong guesses can't
                // knock out the real one.
                Some(code) if code.code == request.code.trim().to_uppercase() => {
                    let fresh = code.expires_at_ms > now;
                    inner.code = None;
                    self.publish(&inner);
                    if !fresh {
                        return Err((StatusCode::FORBIDDEN, "this code has expired"));
                    }
                }
                _ => return Err((StatusCode::FORBIDDEN, "unknown code")),
            }
            if let Some(preset) = inner.saved.auto_approve {
                return Ok(self.add(&mut inner, name, public_key, preset, PairingAction::Paired));
            }
            let id = inner.next_id();
            let (tx, rx) = oneshot::channel();
            inner.pending.push((
                PendingDevice {
                    id,
                    name: name.clone(),
                    public_key: public_key.clone(),
                    address: peer.to_string(),
                    requested_at_ms: now,
                },
                tx,
            ));
            self.publish(&inner);
            (id, rx)
        };
        // Withdraws the request if this future is dropped (the device gave up).
        let _withdraw = Withdraw(self, id);
        match tokio::time::timeout(APPROVAL_WAIT, approval).await {
            Ok(Ok(Some(preset))) => {
                let mut inner = self.lock();
                Ok(self.add(
                    &mut inner,
                    name,
                    public_key,
                    preset,
                    PairingAction::Approved,
                ))
            }
            Ok(_) => Err((StatusCode::FORBIDDEN, "the host said no")),
            Err(_) => Err((StatusCode::REQUEST_TIMEOUT, "nobody approved this device")),
        }
    }

    /// Links the engine with `key`, which this machine paired with and may
    /// sync with: a device that also paired here gains [`Scope::Sync`],
    /// else it is listed with only that and no token. `addresses` are where
    /// this machine reached it on the LAN (`ip:port`), which its iroh
    /// endpoint shares; they go first in its address book. Safe to repeat.
    pub fn link(&self, name: &str, key: &str, addresses: &[String]) -> Result<(), String> {
        let key = parse_key(key).ok_or("bad engine key")?;
        let mut inner = self.lock();
        let inner = &mut *inner;
        let now = engine::wall_clock_ms();
        let known = inner
            .saved
            .devices
            .iter_mut()
            .find(|d| d.device.public_key.as_ref() == Some(&key));
        let lan: Vec<SocketAddr> = addresses.iter().filter_map(|a| a.parse().ok()).collect();
        let name = match known {
            Some(stored) if stored.device.scopes.contains(&Scope::Sync) => {
                if lan_first(stored.address.get_or_insert_default(), lan) {
                    self.save(inner);
                    self.publish(inner);
                }
                return Ok(());
            }
            Some(stored) => {
                stored.device.scopes.push(Scope::Sync);
                stored.address = Some(net::PeerAddr {
                    direct: lan,
                    relay: None,
                });
                stored.device.name.clone()
            }
            None => {
                let name = clean_name(name);
                inner.saved.devices.push(Stored {
                    device: PairedDevice {
                        id: hex(&random::<8>()),
                        name: name.clone(),
                        public_key: Some(key),
                        scopes: vec![Scope::Sync],
                        paired_at_ms: now,
                        last_seen_ms: None,
                    },
                    token_hash: None,
                    address: Some(net::PeerAddr {
                        direct: lan,
                        relay: None,
                    }),
                });
                name
            }
        };
        log(inner, now, PairingAction::Linked, name);
        self.save(inner);
        self.publish(inner);
        Ok(())
    }

    /// Carries out a [`PairingCommand`] from `who`.
    pub(crate) fn command_as(&self, who: &Who, command: PairingCommand) -> Result<(), String> {
        let PairingCommand::SetEngineKey { key } = command else {
            return self.command(command);
        };
        let Who::Device(id) = who else {
            return Err("only a paired device has an engine key to send".into());
        };
        if !self.allows(who, Scope::Sync) {
            return Err("linking engines needs the Full preset".into());
        }
        let key = parse_key(&key).ok_or("bad engine key")?;
        let mut inner = self.lock();
        let inner = &mut *inner;
        let stored = find(inner, id)?;
        if stored.device.public_key.as_ref() == Some(&key) {
            return Ok(());
        }
        stored.device.public_key = Some(key);
        let name = stored.device.name.clone();
        log(inner, engine::wall_clock_ms(), PairingAction::Linked, name);
        self.save(inner);
        self.publish(inner);
        Ok(())
    }

    /// Carries out a client's [`PairingCommand`].
    pub(crate) fn command(&self, command: PairingCommand) -> Result<(), String> {
        let mut inner = self.lock();
        let inner = &mut *inner;
        let now = engine::wall_clock_ms();
        match command {
            PairingCommand::Start => {
                new_code(inner);
            }
            PairingCommand::Cancel => inner.code = None,
            PairingCommand::Approve { request, preset } => {
                let (_, tx) = take_pending(inner, request)?;
                // The waiting request adds the device and logs it.
                tx.send(Some(preset))
                    .map_err(|_| "that device stopped waiting")?;
            }
            PairingCommand::Deny { request } => {
                let (pending, tx) = take_pending(inner, request)?;
                let _ = tx.send(None);
                log(inner, now, PairingAction::Denied, pending.name);
            }
            PairingCommand::SetAutoApprove { preset } => inner.saved.auto_approve = preset,
            PairingCommand::Rename { device, name } => {
                let stored = find(inner, &device)?;
                stored.device.name = clean_name(&name);
                let name = stored.device.name.clone();
                log(inner, now, PairingAction::Renamed, name);
            }
            PairingCommand::SetPreset { device, preset } => {
                let stored = find(inner, &device)?;
                stored.device.scopes = preset.scopes();
                let name = stored.device.name.clone();
                log(inner, now, PairingAction::PresetChanged, name);
            }
            PairingCommand::Revoke { device } => {
                let at = inner
                    .saved
                    .devices
                    .iter()
                    .position(|d| d.device.id == device)
                    .ok_or("no such device")?;
                let stored = inner.saved.devices.remove(at);
                for (connection, close) in &inner.connections {
                    if connection.device.as_ref() == Some(&device) {
                        close.notify_one();
                    }
                }
                log(inner, now, PairingAction::Revoked, stored.device.name);
            }
            PairingCommand::SetLan { allowed } => inner.saved.lan = allowed,
            PairingCommand::SetMpd { enabled } => inner.saved.mpd.enabled = enabled,
            PairingCommand::SetMpdPassword { password } => {
                inner.saved.mpd.password = Some(password).filter(|p| !p.is_empty());
            }
            PairingCommand::SetInternet { allowed } => inner.saved.internet = allowed,
            PairingCommand::SetRelay { url } => {
                let url = url.map(|u| u.trim().to_owned()).filter(|u| !u.is_empty());
                if let Some(url) = &url {
                    net::check_relay(url)?;
                }
                inner.saved.own_relay = url;
            }
            PairingCommand::SetEngineKey { .. } => {
                return Err("only a paired device has an engine key to send".into());
            }
            PairingCommand::Disconnect { connection } => {
                let (_, close) = inner
                    .connections
                    .iter()
                    .find(|(c, _)| c.id == connection)
                    .ok_or("no such connection")?;
                close.notify_one();
            }
        }
        self.save(inner);
        self.publish(inner);
        Ok(())
    }

    fn add(
        &self,
        inner: &mut Inner,
        name: String,
        public_key: Option<String>,
        preset: Preset,
        action: PairingAction,
    ) -> Paired {
        let token = hex(&random::<32>());
        let now = engine::wall_clock_ms();
        let device = PairedDevice {
            id: hex(&random::<8>()),
            name: name.clone(),
            public_key,
            scopes: preset.scopes(),
            paired_at_ms: now,
            last_seen_ms: None,
        };
        let id = device.id.clone();
        inner.saved.devices.push(Stored {
            device,
            token_hash: Some(hash(&token)),
            address: None,
        });
        log(inner, now, action, name);
        self.save(inner);
        self.publish(inner);
        Paired {
            device: id,
            token,
            engine_key: self.0.engine_key.clone(),
        }
    }

    fn seen(&self, inner: &mut Inner, device: &str, now: u64) {
        if let Ok(stored) = find(inner, device) {
            stored.device.last_seen_ms = Some(now);
            self.save(inner);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, inner: &Inner) {
        let Some(file) = &self.0.file else { return };
        let saved = serde_json::to_vec_pretty(&inner.saved).map_err(std::io::Error::other);
        if let Err(err) = saved.and_then(|json| replace(file, &json)) {
            tracing::warn!(%err, "can't save the paired devices");
        }
    }

    fn publish(&self, inner: &Inner) {
        let addresses = inner.lan.as_deref().unwrap_or_default();
        self.0.state.send_replace(PairingState {
            engine_key: self.0.engine_key.clone(),
            code: inner.code.clone(),
            auto_approve: inner.saved.auto_approve,
            pending: inner.pending.iter().map(|(p, _)| p.clone()).collect(),
            devices: inner
                .saved
                .devices
                .iter()
                .map(|d| d.device.clone())
                .collect(),
            connected: inner.connections.iter().map(|(c, _)| c.clone()).collect(),
            log: inner.saved.log.clone(),
            lan: LanState {
                allowed: inner.saved.lan,
                open: inner.lan.is_some(),
                addresses: addresses.iter().map(|a| a.to_string()).collect(),
                pair_urls: match &inner.code {
                    Some(code) => addresses
                        .iter()
                        .map(|a| pair_url(*a, &code.code, &self.0.engine_key))
                        .collect(),
                    None => Vec::new(),
                },
            },
            mpd: MpdState {
                enabled: inner.saved.mpd.enabled,
                password_set: inner.saved.mpd.password.is_some(),
                port: inner.mpd_port,
                listening: inner.mpd_listening,
            },
            net: NetState {
                internet: inner.saved.internet,
                own_relay: inner.saved.own_relay.clone(),
                links: inner
                    .saved
                    .devices
                    .iter()
                    .filter_map(|d| {
                        let reach = inner.reach.get(linked_key(d)?);
                        Some(LinkState {
                            device: d.device.id.clone(),
                            reach: reach.copied().unwrap_or(Reach::Unreachable),
                        })
                    })
                    .collect(),
            },
        });
    }
}

/// The relays [`NetState`] asks for.
pub(crate) fn relays(net: &NetState) -> net::Relays {
    match (net.internet, &net.own_relay) {
        (false, _) => net::Relays::Off,
        (true, Some(url)) => net::Relays::Own(url.clone()),
        (true, None) => net::Relays::Public,
    }
}

/// Puts `lan` first in `address`, keeping the rest; says if it changed.
fn lan_first(address: &mut net::PeerAddr, lan: Vec<SocketAddr>) -> bool {
    let mut direct = lan;
    let rest: Vec<_> = address
        .direct
        .iter()
        .filter(|a| !direct.contains(a))
        .collect();
    direct.extend(rest);
    direct.truncate(ADDRESS_CAP);
    let changed = direct != address.direct;
    address.direct = direct;
    changed
}

/// The key of a linked engine: one with [`Scope::Sync`].
fn linked_key(stored: &Stored) -> Option<&str> {
    let device = &stored.device;
    device
        .public_key
        .as_deref()
        .filter(|_| device.scopes.contains(&Scope::Sync))
}

/// What a pairing QR code holds: a phone's camera opens it in a browser, and
/// Motif apps read the code and engine key from it.
pub fn pair_url(address: SocketAddr, code: &str, engine_key: &str) -> String {
    format!("http://{address}/pair?code={code}&id={engine_key}")
}

/// Also opens the LAN: that is what a code is for.
fn new_code(inner: &mut Inner) -> PairingCode {
    let code = PairingCode {
        code: random::<8>()
            .iter()
            .map(|b| CODE_ALPHABET[*b as usize % CODE_ALPHABET.len()] as char)
            .collect(),
        expires_at_ms: engine::wall_clock_ms() + inner.code_ttl_ms,
    };
    inner.code = Some(code.clone());
    inner.saved.lan = true;
    code
}

impl Inner {
    fn next_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }
}

struct Withdraw<'a>(&'a Pairing, u32);

impl Drop for Withdraw<'_> {
    fn drop(&mut self) {
        let mut inner = self.0.lock();
        if take_pending(&mut inner, self.1).is_ok() {
            self.0.publish(&inner);
        }
    }
}

fn take_pending(
    inner: &mut Inner,
    id: u32,
) -> Result<(PendingDevice, oneshot::Sender<Option<Preset>>), &'static str> {
    let at = inner
        .pending
        .iter()
        .position(|(p, _)| p.id == id)
        .ok_or("no such request")?;
    Ok(inner.pending.remove(at))
}

fn find<'a>(inner: &'a mut Inner, device: &str) -> Result<&'a mut Stored, &'static str> {
    inner
        .saved
        .devices
        .iter_mut()
        .find(|d| d.device.id == device)
        .ok_or("no such device")
}

fn log(inner: &mut Inner, at_ms: u64, action: PairingAction, device: String) {
    let log = &mut inner.saved.log;
    log.push(PairingLogEntry {
        at_ms,
        action,
        device,
    });
    if log.len() > LOG_CAP {
        log.drain(..log.len() - LOG_CAP);
    }
}

fn clean_name(name: &str) -> String {
    let name: String = name.trim().chars().take(NAME_CAP).collect();
    if name.is_empty() {
        "Unnamed device".into()
    } else {
        name
    }
}

fn parse_key(key: &str) -> Option<String> {
    let bytes: Vec<u8> = (0..key.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(key.get(i..i + 2)?, 16).ok())
        .collect::<Option<_>>()?;
    let key = VerifyingKey::from_bytes(&bytes.try_into().ok()?).ok()?;
    Some(hex(key.as_bytes()))
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the OS has a random source");
    bytes
}

fn hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Replaces `file` whole, so a crash leaves the old or the new contents.
/// Only this user may read it.
// Same as integrations' settings file; server doesn't depend on that crate.
fn replace(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = file.with_extension("partial");
    // `mode` only applies to new files.
    let _ = std::fs::remove_file(&partial);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(&partial)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    std::fs::rename(partial, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(code: &str) -> PairRequest {
        PairRequest {
            code: code.into(),
            name: "Phone".into(),
            public_key: None,
        }
    }

    fn peer() -> SocketAddr {
        "192.168.1.20:5000".parse().unwrap()
    }

    #[tokio::test]
    async fn a_code_works_once_and_expires() {
        let pairing = Pairing::in_memory();
        pairing
            .command(PairingCommand::SetAutoApprove {
                preset: Some(Preset::ViewOnly),
            })
            .unwrap();
        let code = pairing.start().code;
        assert!(pairing.redeem(request("WRONG"), peer()).await.is_err());
        let paired = pairing
            .redeem(request(&code.to_lowercase()), peer())
            .await
            .unwrap();
        assert_eq!(pairing.authenticate(&paired.token), Some(paired.device));
        assert_eq!(paired.engine_key, pairing.engine_key());
        assert!(pairing.redeem(request(&code), peer()).await.is_err());

        pairing.lock().code_ttl_ms = 0;
        let code = pairing.start().code;
        let refused = pairing.redeem(request(&code), peer()).await;
        assert_eq!(refused.unwrap_err().1, "this code has expired");
        assert_eq!(pairing.authenticate("not a token"), None);
    }

    #[tokio::test]
    async fn approval_required_codes_wait_until_approved() {
        let pairing = Pairing::in_memory();
        let mut state = pairing.subscribe();
        let code = pairing.start().code;
        let waiting = tokio::spawn({
            let pairing = pairing.clone();
            async move { pairing.redeem(request(&code), peer()).await }
        });
        let pending = state
            .wait_for(|s| !s.pending.is_empty())
            .await
            .unwrap()
            .pending[0]
            .id;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiting.is_finished(), "waits for the host");
        pairing
            .command(PairingCommand::Approve {
                request: pending,
                preset: Preset::ControlPlayback,
            })
            .unwrap();
        let paired = waiting.await.unwrap().unwrap();
        let who = Who::Device(paired.device);
        assert!(pairing.allows(&who, Scope::Playback));
        assert!(!pairing.allows(&who, Scope::Pairing));
        let state = pairing.subscribe().borrow().clone();
        assert!(state.pending.is_empty());
        assert_eq!(state.log.last().unwrap().action, PairingAction::Approved);

        // Denied, and a request that gives up is withdrawn.
        let code = pairing.start().code;
        let waiting = tokio::spawn({
            let pairing = pairing.clone();
            async move { pairing.redeem(request(&code), peer()).await }
        });
        let mut state = pairing.subscribe();
        let id = state
            .wait_for(|s| !s.pending.is_empty())
            .await
            .unwrap()
            .pending[0]
            .id;
        pairing
            .command(PairingCommand::Deny { request: id })
            .unwrap();
        assert_eq!(waiting.await.unwrap().unwrap_err().0, StatusCode::FORBIDDEN);
        let code = pairing.start().code;
        let waiting = tokio::spawn({
            let pairing = pairing.clone();
            async move { pairing.redeem(request(&code), peer()).await }
        });
        state.wait_for(|s| !s.pending.is_empty()).await.unwrap();
        waiting.abort();
        state.wait_for(|s| s.pending.is_empty()).await.unwrap();
    }

    #[test]
    fn identity_and_devices_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("motif-pairing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pairing = Pairing::open(&dir).unwrap();
        let paired = {
            let mut inner = pairing.lock();
            pairing.add(
                &mut inner,
                "Tablet".into(),
                None,
                Preset::Full,
                PairingAction::Paired,
            )
        };
        let again = Pairing::open(&dir).unwrap();
        assert_eq!(again.engine_key(), pairing.engine_key());
        assert_eq!(again.authenticate(&paired.token), Some(paired.device));
        let saved = std::fs::read_to_string(dir.join("pairing.json")).unwrap();
        assert!(!saved.contains(&paired.token), "only the hash is kept");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_lan_switch_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("motif-lan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let allowed = |p: &Pairing| p.subscribe().borrow().lan.allowed;
        assert!(
            !allowed(&Pairing::open(&dir).unwrap()),
            "loopback only at first"
        );
        Pairing::open(&dir).unwrap().start();
        let pairing = Pairing::open(&dir).unwrap();
        assert!(allowed(&pairing), "starting a pairing opens it");
        // The code is gone after a restart, and nobody is paired.
        assert_eq!(pairing.lan_wanted(), (false, None));
        pairing.allow_lan(false);
        assert!(!allowed(&Pairing::open(&dir).unwrap()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn key(seed: u8) -> String {
        hex(SigningKey::from_bytes(&[seed; 32])
            .verifying_key()
            .as_bytes())
    }

    fn device(pairing: &Pairing, id: &str) -> PairedDevice {
        let state = pairing.subscribe().borrow().clone();
        state.devices.into_iter().find(|d| d.id == id).unwrap()
    }

    #[tokio::test]
    async fn pairing_with_an_engine_key_links_with_full_only() {
        assert!(Preset::Full.scopes().contains(&Scope::Sync));
        assert!(!Preset::ControlPlayback.scopes().contains(&Scope::Sync));
        assert!(!Preset::ViewOnly.scopes().contains(&Scope::Sync));
        let pairing = Pairing::in_memory();
        for (preset, linked) in [(Preset::Full, true), (Preset::ViewOnly, false)] {
            pairing
                .command(PairingCommand::SetAutoApprove {
                    preset: Some(preset),
                })
                .unwrap();
            let request = PairRequest {
                public_key: Some(key(1).to_uppercase()),
                ..request(&pairing.start().code)
            };
            let paired = pairing.redeem(request, peer()).await.unwrap();
            assert_eq!(device(&pairing, &paired.device).public_key, Some(key(1)));
            assert_eq!(
                pairing.allows(&Who::Device(paired.device), Scope::Sync),
                linked
            );
        }
    }

    #[test]
    fn old_full_pairings_gain_sync_and_send_their_key_once() {
        let dir = std::env::temp_dir().join(format!("motif-upgrade-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Saved before Sync and links.
        let old = format!(
            r#"{{"auto_approve":null,"log":[],"devices":[
                {{"id":"full","name":"Laptop","public_key":null,"scopes":["playback","queue","library","settings","pairing"],"paired_at_ms":1,"last_seen_ms":null,"token_hash":"{}"}},
                {{"id":"view","name":"Tablet","public_key":null,"scopes":[],"paired_at_ms":1,"last_seen_ms":null,"token_hash":"{}"}}]}}"#,
            hash("full token"),
            hash("view token")
        );
        std::fs::write(dir.join("pairing.json"), old).unwrap();
        let pairing = Pairing::open(&dir).unwrap();
        let full = Who::Device(pairing.authenticate("full token").unwrap());
        assert!(pairing.allows(&full, Scope::Sync));
        let view = Who::Device(pairing.authenticate("view token").unwrap());
        assert!(!pairing.allows(&view, Scope::Sync));

        let set = |who: &Who, key: &str| {
            pairing.command_as(who, PairingCommand::SetEngineKey { key: key.into() })
        };
        set(&full, &key(2)).unwrap();
        let logged = pairing.subscribe().borrow().log.len();
        set(&full, &key(2)).unwrap();
        assert_eq!(
            pairing.subscribe().borrow().log.len(),
            logged,
            "a repeat changes nothing"
        );
        assert_eq!(
            pairing.subscribe().borrow().log.last().unwrap().action,
            PairingAction::Linked
        );
        assert!(set(&view, &key(3)).is_err(), "needs Sync");
        assert!(
            set(&Who::Local, &key(3)).is_err(),
            "this machine has no other engine"
        );
        assert!(set(&full, "abcd").is_err());
        let again = Pairing::open(&dir).unwrap();
        assert_eq!(device(&again, "full").public_key, Some(key(2)));
        assert_eq!(device(&again, "view").public_key, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn the_engine_paired_with_is_linked_here_without_a_token() {
        let dir = std::env::temp_dir().join(format!("motif-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pairing = Pairing::open(&dir).unwrap();
        pairing.link("Desktop", &key(4), &[]).unwrap();
        pairing.link("Desktop again", &key(4), &[]).unwrap();
        let state = pairing.subscribe().borrow().clone();
        assert_eq!(state.devices.len(), 1, "a repeat adds no second link");
        let linked = &state.devices[0];
        assert_eq!(
            (linked.name.as_str(), linked.public_key.clone()),
            ("Desktop", Some(key(4)))
        );
        assert_eq!(
            linked.scopes,
            vec![Scope::Sync],
            "it gets our data but no control"
        );
        assert!(pairing.link("Bad", "abcd", &[]).is_err());

        // An engine that also paired here keeps its access and gains Sync.
        pairing
            .command(PairingCommand::SetAutoApprove {
                preset: Some(Preset::ControlPlayback),
            })
            .unwrap();
        let request = PairRequest {
            public_key: Some(key(5)),
            ..request(&pairing.start().code)
        };
        let paired = pairing.redeem(request, peer()).await.unwrap();
        pairing.link("Other", &key(5), &[]).unwrap();
        let both = device(&pairing, &paired.device);
        assert_eq!(
            both.scopes,
            vec![Scope::Playback, Scope::Queue, Scope::Sync]
        );
        assert_eq!(pairing.subscribe().borrow().devices.len(), 2);

        let again = Pairing::open(&dir).unwrap();
        assert_eq!(again.subscribe().borrow().devices.len(), 2);
        assert_eq!(again.authenticate(&paired.token), Some(paired.device));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_address_book_survives_a_restart_and_follows_meetings() {
        let dir = std::env::temp_dir().join(format!("motif-book-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let addr = |direct: &[&str], relay: Option<&str>| net::PeerAddr {
            direct: direct.iter().map(|a| a.parse().unwrap()).collect(),
            relay: relay.map(str::to_owned),
        };
        let book = |p: &Pairing| p.linked();
        let pairing = Pairing::open(&dir).unwrap();
        pairing
            .link(
                "Desktop",
                &key(4),
                &["192.168.1.5:7700".into(), "junk".into()],
            )
            .unwrap();
        assert_eq!(
            book(&pairing),
            [(key(4), addr(&["192.168.1.5:7700"], None))]
        );
        assert!(pairing.is_linked(&key(4)) && !pairing.is_linked(&key(5)));

        // Meeting over iroh: what it says replaces what we had.
        let told = addr(&["10.0.0.2:41000"], Some("https://relay.example.com./"));
        pairing.met(&key(4), told.clone());
        pairing.met(&key(5), told.clone());
        let again = Pairing::open(&dir).unwrap();
        assert_eq!(book(&again), [(key(4), told)]);

        // Meeting on the LAN: that address goes first, the rest stays.
        again
            .link("Desktop", &key(4), &["192.168.1.9:7700".into()])
            .unwrap();
        let after = addr(
            &["192.168.1.9:7700", "10.0.0.2:41000"],
            Some("https://relay.example.com./"),
        );
        assert_eq!(book(&Pairing::open(&dir).unwrap()), [(key(4), after)]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn linked_engines_show_how_they_are_reached() {
        let pairing = Pairing::in_memory();
        pairing.link("Desktop", &key(4), &[]).unwrap();
        let links = |p: &Pairing| p.subscribe().borrow().net.links.clone();
        let id = links(&pairing)[0].device.clone();
        assert_eq!(links(&pairing)[0].reach, Reach::Unreachable);
        pairing.set_reach(&key(4), Reach::Relayed);
        assert_eq!(
            links(&pairing),
            [LinkState {
                device: id,
                reach: Reach::Relayed
            }]
        );
        let paired = {
            let mut inner = pairing.lock();
            pairing.add(
                &mut inner,
                "Tablet".into(),
                None,
                Preset::Full,
                PairingAction::Paired,
            )
        };
        assert!(
            links(&pairing).iter().all(|l| l.device != paired.device),
            "a device without an engine isn't linked"
        );
    }

    #[test]
    fn the_relay_settings_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("motif-relay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let relays_of = |p: &Pairing| relays(&p.subscribe().borrow().net);
        let pairing = Pairing::open(&dir).unwrap();
        assert_eq!(relays_of(&pairing), net::Relays::Public);
        let set = |url: &str| {
            pairing.command(PairingCommand::SetRelay {
                url: Some(url.into()),
            })
        };
        assert!(set("not a url").is_err());
        set(" https://relay.example.com ").unwrap();
        let own = net::Relays::Own("https://relay.example.com".into());
        assert_eq!(relays_of(&Pairing::open(&dir).unwrap()), own);
        pairing
            .command(PairingCommand::SetInternet { allowed: false })
            .unwrap();
        let again = Pairing::open(&dir).unwrap();
        assert_eq!(relays_of(&again), net::Relays::Off);
        again
            .command(PairingCommand::SetInternet { allowed: true })
            .unwrap();
        assert_eq!(relays_of(&again), own, "the own relay is kept while off");
        again
            .command(PairingCommand::SetRelay {
                url: Some(String::new()),
            })
            .unwrap();
        assert_eq!(
            relays_of(&Pairing::open(&dir).unwrap()),
            net::Relays::Public
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn public_keys_must_be_ed25519() {
        let key = hex(SigningKey::from_bytes(&[7; 32]).verifying_key().as_bytes());
        assert_eq!(parse_key(&key.to_uppercase()), Some(key));
        assert_eq!(parse_key("abcd"), None);
        assert_eq!(parse_key("zz"), None);
    }
}
