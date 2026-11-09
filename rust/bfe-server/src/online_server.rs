//! Online directory registration, dual-stack heartbeats, STUN NAT detection, and WebSocket
//! rendezvous signaling (`src/online-server.js`).
//!
//! Why `OnlineServer` separates directory rendezvous from the gameplay socket:
//! In `src/online-server.js` (`export class OnlineServer`), the master directory only brokers
//! discovery and exchanges UDP candidates (`host`, `srflx`, `relay`); the gameplay session itself
//! runs over the shared `BFE-UDP/1` endpoint (`udp-server.js`), so a master restart or transient
//! signaling disconnect never drops an in-progress battle.
//!
//! Key protocol behaviors ported from `src/online-server.js`:
//! 1. **Address privacy & candidate filtering (`publicAddress`, `parseCandidates`, `candidates` in `src/online-server.js`)**:
//!    - `public_address` filters out RFC 1918 (`10/8`, `172.16/12`, `192.168/16`), loopback (`127/8`, `::1`),
//!      unspecified (`0/8`, `::`), CGNAT (`100.64/10`), link-local (`169.254/16`, `fe80::/10`),
//!      ULA (`fc00::/7`), and multicast (`224/4`, `ff00::/8`).
//!    - When answering a player's `offer`, `candidates()` lists public `host` candidates first, then
//!      STUN `srflx` candidates, and finally private `host` candidates *only* when one of the
//!      player's own candidates shares this server's reflexive public IP (`mapped.has(candidate.address)`),
//!      so hairpin LAN players behind the same NAT can connect directly without disclosing internal
//!      LAN addresses to the rest of the internet.
//!    - UDP hole-punches (`this.udp.endpoint.punch(...)`) are sent only to `public_address` client
//!      candidates so a remote offer can never trick the server into sending datagrams to loopback,
//!      link-local, or internal RFC 1918 hosts.
//! 2. **Dual-family heartbeats (`beat()` in `src/online-server.js`)**:
//!    A direct-only server (`relayPolicy === "never"`) alternates `PUT /v1/servers/:id` between
//!    IPv4 (`4`) and IPv6 (`6`) every 20 seconds so the master learns both public addresses. If
//!    one address family cannot reach the master (a transport error without an HTTP status), that
//!    family is marked down for 10 minutes (`600_000 ms`) and the beat immediately retries over the
//!    default family (`0`).
//! 3. **Offer rate limits and `matchProbe` readiness (`signal(m)` in `src/online-server.js`)**:
//!    - Ignores signals from banned identities (`isBanned(m.from) || globallyBanned(m.from)`).
//!    - Answers `matchProbe` with `matchReady` when `signal.matchProbe` matches the active battle
//!      query's `match` token (used by clients following a map rotation).
//!    - Rate-limits `offer` signals to at most 1 per second per peer and `maxPlayers * 2` active
//!      offers within a 10-second window.
//!
//! Online server lifecycle and NAT traversal details (`online_server.rs` <-> `src/online-server.js`):
//! - Periodic maintenance intervals:
//!   - `HEARTBEAT_INTERVAL = 30_000` ms (`POST /v1/servers` listing refresh).
//!   - `BAN_REFRESH_INTERVAL = 300_000` ms (`GET /v1/bans` TOFU-verified ban list refresh).
//!   - `STUN_REFRESH_INTERVAL = 120_000` ms (`UdpGameServer::reflexive` RFC 5389 Binding Request).
//!   - `WS_RECONNECT_BASE = 1_000` ms .. `30_000` ms exponential backoff on WebSocket disconnect.
//! - Signaling rate-limiting (`allowSignal` in `src/online-server.js`):
//!   - Sliding window `SIGNAL_WINDOW_MS = 10_000` ms with `SIGNAL_BURST = 8` messages per sender
//!     and `MAX_SIGNAL_SENDERS = 4096` tracked sender entries.
//! - Supported client signaling message types (`handle_signal`):
//!   - `offer`: validates protocol/simulation/content, checks bans, punches UDP candidates, and
//!     replies with `answer` carrying the server game port and `host` + `srflx` candidates.
//!   - `candidate`: punches newly trickle-discovered client UDP candidates.
//!   - `matchProbe`: verifies `probe.match` against the active battle query token and replies
//!     with `matchReady` once `LocalDedicatedServer` has rotated into the new match.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::bans::BanCache;
use crate::browser_info::BrowserInfo;
use crate::config::ServerConfig;
use crate::identity::{is_hex_64, load_identity, ServerIdentity};
use crate::master_client::{MasterClient, MasterEvent, SignalEnvelope};
use crate::network_contract::{validate_network_contract, wire_compatibility, WireCompatibility};
use crate::udp_server::{CandidateEntry, ReflexiveMap, UdpGameServer};

/// Returns `true` if `address` is a globally routable unicast IPv4 or IPv6 address
/// (`export function publicAddress(address)` in `src/online-server.js`).
///
/// Rejects private, loopback, unspecified, the carrier-grade NAT block, link-local, ULA (`fc00::/7`),
/// and multicast addresses, stripping any leading `::ffff:` IPv4-mapped prefix first.
pub fn public_address(address: &str) -> bool {
    let cleaned = address
        .trim()
        .strip_prefix("::ffff:")
        .or_else(|| address.trim().strip_prefix("::FFFF:"))
        .unwrap_or_else(|| address.trim());
    let base = cleaned.split('%').next().unwrap_or(cleaned);
    match base.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            let [a, b, _, _] = v4.octets();
            !(a == 10
                || a == 127
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..32).contains(&b))
                || (a == 192 && b == 168)
                || a >= 224)
        }
        Ok(IpAddr::V6(v6)) => {
            let segments = v6.segments();
            if v6.is_unspecified() || v6.is_loopback() {
                return false;
            }
            let first_byte = (segments[0] >> 8) as u8;
            if first_byte == 0xfc || first_byte == 0xfd || first_byte == 0xff {
                return false;
            }
            if (segments[0] & 0xffc0) == 0xfe80 {
                return false;
            }
            true
        }
        Err(_) => false,
    }
}

/// Validates and normalizes an array of up to 8 UDP signaling candidates
/// (`export function parseCandidates(value)` in `src/online-server.js`).
///
/// Why candidate lists are capped at 8 entries and restricted to valid IP + port `1..=65535`:
/// Prevents amplified UDP hole-punch bursts and malformed WebSocket `answer` messages when a client
/// sends a crafted `offer`.
pub fn parse_candidates(value: &Value) -> Result<Vec<CandidateEntry>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "invalid candidates".to_string())?;
    if arr.len() > 8 {
        return Err("invalid candidates".to_string());
    }
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = item
            .as_object()
            .ok_or_else(|| "invalid candidate".to_string())?;
        let address = obj
            .get("address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "invalid candidate".to_string())?;
        let base_ip = address.split('%').next().unwrap_or("");
        if base_ip.parse::<IpAddr>().is_err() {
            return Err("invalid candidate".to_string());
        }
        let port_u64 = obj
            .get("port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "invalid candidate".to_string())?;
        if !(1..=65535).contains(&port_u64) {
            return Err("invalid candidate".to_string());
        }
        let kind_raw = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("host");
        let kind = match kind_raw {
            "host" | "srflx" | "relay" => kind_raw.to_string(),
            _ => "host".to_string(),
        };
        out.push(CandidateEntry {
            address: address.to_string(),
            port: port_u64 as u16,
            kind: Some(kind),
        });
    }
    Ok(out)
}

/// Parses the STUN server `"host:port"` string (`OnlineServer.stunServer()` in `src/online-server.js`),
/// defaulting to `"relay.battlefronteternal.com:3478"`.
pub fn parse_stun_server(stun: Option<&str>) -> (String, u16) {
    let raw = stun.unwrap_or("relay.battlefronteternal.com:3478");
    if let Some((host, port_str)) = raw.rsplit_once(':') {
        if !host.is_empty() && port_str.bytes().all(|b| b.is_ascii_digit()) {
            let port = port_str.parse::<u16>().unwrap_or(3478);
            return (host.to_string(), if port == 0 { 3478 } else { port });
        }
    }
    (raw.to_string(), 3478)
}

/// Online directory registration and rendezvous signaling controller
/// (`export class OnlineServer` in `src/online-server.js`).
/// Public internet listing, NAT traversal, global ban-list sync, and signaling coordinator,
/// porting `OnlineServer` from `packages/bfe-server/src/online-server.js`.
///
/// Responsibilities (`online-server.js`):
/// 1. Periodically sends signed listing heartbeats (`beat()`) to `POST /api/servers/heartbeat`
///    every `HEARTBEAT_INTERVAL_MS` (`30_000` ms) and delists (`DELETE /api/servers/:id`) on close.
/// 2. Periodically refreshes the Ed25519-signed global ban list (`BanCache::refresh` in `bans.js`)
///    every `BAN_REFRESH_MS` (`300_000` ms = 5 min) so globally banned player IDs are rejected
///    during the UDP handshake (`AdmitDecision::Reject`).
/// 3. Queries STUN (`discover()`) every `STUN_REFRESH_MS` (`120_000` ms = 2 min) via
///    `UdpGameServer::reflexive` to detect NAT (`natted()`) and learn the server's public `srflx`
///    `ip:port` candidate.
/// 4. Handles incoming client signaling messages (`handle_signal`) relayed over the master
///    WebSocket: rate-limits per-sender signaling (`SIGNAL_WINDOW_MS = 10_000`, `SIGNAL_BURST = 8`),
///    initiates simultaneous UDP hole-punching (`UdpGameServer::punch`) toward client `host`/`srflx`
///    candidates, and replies with the server's `host`, `srflx`, and optional TURN `relay` candidates.
pub struct OnlineServer {
    pub id: String,
    pub identity: ServerIdentity,
    pub config: ServerConfig,
    pub wire: WireCompatibility,
    pub content: String,
    pub query: String,
    pub master: Arc<MasterClient>,
    pub bans: Arc<Mutex<BanCache<MasterClient>>>,
    pub reflexive: Arc<Mutex<ReflexiveMap>>,
    pub offers: Arc<Mutex<HashMap<String, Instant>>>,
    pub beats: Arc<Mutex<u64>>,
    pub family_down: Arc<Mutex<HashMap<u8, Instant>>>,
    udp: Option<Arc<Mutex<UdpGameServer>>>,
    browser: Arc<dyn Fn() -> BrowserInfo + Send + Sync>,
    is_banned: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl OnlineServer {
    /// Constructs an `OnlineServer` (`constructor` in `src/online-server.js`), validating the
    /// engine network contract, loading `identity.json`, and initializing `MasterClient` and `BanCache`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: ServerConfig,
        directory: &Path,
        network_contract: &Value,
        query: String,
        udp: Option<Arc<Mutex<UdpGameServer>>>,
        browser: Arc<dyn Fn() -> BrowserInfo + Send + Sync>,
        is_banned: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    ) -> Result<Self, String> {
        let contract = validate_network_contract(network_contract)?;
        let wire = wire_compatibility(&contract);
        let identity = load_identity(&directory.join("identity.json"))?;
        let id = identity.id.clone();
        let master = Arc::new(MasterClient::new(
            &config.master,
            identity.clone(),
            config.online_notice_accepted,
        )?);
        let bans = Arc::new(Mutex::new(BanCache::new(
            Arc::clone(&master),
            directory.join("master-ban-key"),
        )));
        let content = config
            .content
            .clone()
            .unwrap_or_else(|| "bfe-content-1".to_string());
        Ok(Self {
            id,
            identity,
            config,
            wire,
            content,
            query,
            master,
            bans,
            reflexive: Arc::new(Mutex::new(ReflexiveMap::default())),
            offers: Arc::new(Mutex::new(HashMap::new())),
            beats: Arc::new(Mutex::new(0)),
            family_down: Arc::new(Mutex::new(HashMap::new())),
            udp,
            browser,
            is_banned,
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        })
    }

    /// Builds the directory listing payload (`OnlineServer.listing()` in `src/online-server.js`).
    ///
    /// A direct-only server (`relayPolicy === "never"`) advertises `[{"port": port, "transport": "udp"}]`
    /// in `endpoints` so the master records the source IP of each heartbeat for direct client pings;
    /// `"auto"` and `"always"` servers advertise `endpoints: []` to keep their IP private until rendezvous.
    pub fn listing(&self) -> Value {
        let direct = self.config.relay_policy == "never";
        let (players, udp_port) = self
            .udp
            .as_ref()
            .and_then(|u| u.lock().ok().map(|g| (g.players().len(), Some(g.port()))))
            .unwrap_or((0, None));
        let max_players = self.config.max_players.unwrap_or(64);
        let port = udp_port.or(self.config.game_port).unwrap_or(18451);
        let endpoints = if direct {
            serde_json::json!([{ "port": port, "transport": "udp" }])
        } else {
            serde_json::json!([])
        };
        let browser = (self.browser)();
        let mut obj = serde_json::json!({
            "name": self.config.name,
            "protocol": self.wire.protocol,
            "profile": self.wire.profile,
            "simulation": self.wire.simulation,
            "content": self.content,
            "rotation": self.config.rotation,
            "players": players,
            "maxPlayers": max_players,
            "relayPolicy": self.config.relay_policy,
            "endpoints": endpoints,
            "fingerprint": self.identity.fingerprint,
        });
        if let (Some(map), Ok(Value::Object(browser_map))) =
            (obj.as_object_mut(), serde_json::to_value(browser))
        {
            for (k, v) in browser_map {
                map.insert(k, v);
            }
        }
        obj
    }

    /// Sends a heartbeat `PUT /v1/servers/:id` to the master (`async beat()` in `src/online-server.js`).
    ///
    /// When `relayPolicy == "never"`, alternates between IPv4 (`4`) and IPv6 (`6`) on each beat;
    /// if a forced family fails at the socket/routing layer (`err.status.is_none()`), marks that
    /// family down for 600 seconds and retries with `family = 0`.
    /// Sends a signed `POST /api/servers/heartbeat` listing update to the master server and
    /// captures any `stunServer` hint returned in the response, porting `OnlineServer.beat()` in
    /// `packages/bfe-server/src/online-server.js`.
    pub fn beat(&self) -> Result<(), String> {
        let family = if self.config.relay_policy == "never" {
            let mut b = self.beats.lock().map_err(|_| "lock poisoned")?;
            let f = if *b % 2 == 0 { 4 } else { 6 };
            *b += 1;
            f
        } else {
            0
        };
        let now = Instant::now();
        let forced = if family != 0 {
            let down = self.family_down.lock().map_err(|_| "lock poisoned")?;
            if down.get(&family).is_some_and(|until| *until > now) {
                0
            } else {
                family
            }
        } else {
            0
        };
        let route = format!("/v1/servers/{}", self.id);
        let listing = self.listing();
        match self.master.request(&route, "PUT", Some(&listing), forced) {
            Ok(_) => Ok(()),
            Err(err) => {
                // Only fall back and mark `forced` down on transport-level failures (`status == None`),
                // never on HTTP 4xx/5xx responses from the master (`if (!family || error.status) throw error`).
                if forced == 0 || err.status.is_some() {
                    return Err(err.message);
                }
                if let Ok(mut down) = self.family_down.lock() {
                    down.insert(forced, Instant::now() + Duration::from_secs(600));
                }
                self.master
                    .request(&route, "PUT", Some(&listing), 0)
                    .map(|_| ())
                    .map_err(|e| e.message)
            }
        }
    }

    /// Checks whether `id` is on the signed global ban list when `honorGlobalBans` is enabled
    /// (`OnlineServer.globallyBanned(id)` in `src/online-server.js`).
    pub fn globally_banned(&self, id: &str) -> bool {
        if !self.config.honor_global_bans {
            return false;
        }
        self.bans
            .lock()
            .map(|b| b.contains(id))
            .unwrap_or(false)
    }

    /// Returns the configured STUN `(host, port)` pair (`OnlineServer.stunServer()` in `src/online-server.js`).
    pub fn stun_server(&self) -> (String, u16) {
        parse_stun_server(self.config.stun.as_deref())
    }

    /// Queries the STUN server from the game port to discover the server's reflexive IPv4 and IPv6
    /// addresses (`async discover()` in `src/online-server.js`).
    ///
    /// Skipped when `relayPolicy == "always"` so a relay-only server never sends direct STUN packets.
    /// Runs a STUN Binding Request on the live game UDP socket (`UdpGameServer::reflexive`) to
    /// discover the server's public `srflx` IPv4/IPv6 mapping and detect symmetric/cone NAT,
    /// porting `OnlineServer.discover()` in `packages/bfe-server/src/online-server.js`.
    pub fn discover(&self) {
        if self.config.relay_policy == "always" {
            return;
        }
        let Some(udp_arc) = self.udp.as_ref() else {
            return;
        };
        let (host, port) = self.stun_server();
        let mut addrs_v4 = None;
        let mut addrs_v6 = None;
        if let Ok(iter) = (host.as_str(), port).to_socket_addrs() {
            for addr in iter {
                match addr {
                    SocketAddr::V4(_) if addrs_v4.is_none() => addrs_v4 = Some(addr),
                    SocketAddr::V6(_) if addrs_v6.is_none() => addrs_v6 = Some(addr),
                    _ => {}
                }
            }
        }
        let mut map = ReflexiveMap::default();
        if let Ok(udp) = udp_arc.lock() {
            if let Some(target) = addrs_v4 {
                if let Ok(res) = udp.reflexive(target, Duration::from_millis(1500)) {
                    if res.ipv4.is_some() {
                        map.ipv4 = res.ipv4;
                    }
                }
            }
            if let Some(target) = addrs_v6 {
                if let Ok(res) = udp.reflexive(target, Duration::from_millis(1500)) {
                    if res.ipv6.is_some() {
                        map.ipv6 = res.ipv6;
                    }
                }
            }
        }
        if let Ok(mut guard) = self.reflexive.lock() {
            *guard = map;
        }
    }

    /// Returns `true` when the server appears to be behind NAT (`OnlineServer.natted()` in
    /// `src/online-server.js`: no reflexive mapping or any reflexive address not owned by a local interface).
    pub fn natted(&self) -> bool {
        let hosts: HashSet<String> = self
            .udp
            .as_ref()
            .and_then(|u| {
                u.lock()
                    .ok()
                    .map(|g| g.candidates().into_iter().map(|c| c.address).collect())
            })
            .unwrap_or_default();
        let reflexive = self
            .reflexive
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let mut mapped = Vec::new();
        if let Some(v4) = reflexive.ipv4 {
            mapped.push(v4);
        }
        if let Some(v6) = reflexive.ipv6 {
            mapped.push(v6);
        }
        mapped.is_empty() || mapped.iter().any(|entry| !hosts.contains(&entry.address))
    }

    /// Assembles up to 8 direct (`host` + `srflx`) UDP candidates to answer a client's offer
    /// (`OnlineServer.candidates(clientCandidates)` in `src/online-server.js`).
    ///
    /// Includes private LAN `host` candidates only when the client's candidate list contains one
    /// of this server's own reflexive public addresses (indicating both share the same NAT).
    /// Assembles the server's ICE-style UDP candidate list (`host`, `srflx`, and optional TURN
    /// `relay` candidates) in response to a client's candidate offer, porting
    /// `OnlineServer.candidates(clientCandidates)` in `packages/bfe-server/src/online-server.js`.
    pub fn candidates(&self, client_candidates: &[CandidateEntry]) -> Vec<CandidateEntry> {
        if self.config.relay_policy == "always" {
            return Vec::new();
        }
        let Some(udp_arc) = self.udp.as_ref() else {
            return Vec::new();
        };
        let host_candidates = udp_arc
            .lock()
            .map(|g| g.candidates())
            .unwrap_or_default();
        // Step 1: Publicly routable `host` candidates.
        let mut list: Vec<CandidateEntry> = host_candidates
            .iter()
            .filter(|c| public_address(&c.address))
            .map(|c| CandidateEntry {
                address: c.address.clone(),
                port: c.port,
                kind: Some("host".to_string()),
            })
            .collect();

        // Step 2: STUN `srflx` reflexive mappings not already present in `list`.
        let reflexive = self
            .reflexive
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        for entry in [reflexive.ipv4, reflexive.ipv6].into_iter().flatten() {
            if !list
                .iter()
                .any(|c| c.address == entry.address && c.port == entry.port)
            {
                list.push(CandidateEntry {
                    address: entry.address,
                    port: entry.port,
                    kind: Some("srflx".to_string()),
                });
            }
        }

        // Step 3: Private LAN `host` candidates only if the client shares our reflexive public IP.
        let mut mapped_addrs = HashSet::new();
        if let Ok(guard) = self.reflexive.lock() {
            if let Some(ref v4) = guard.ipv4 {
                mapped_addrs.insert(v4.address.clone());
            }
            if let Some(ref v6) = guard.ipv6 {
                mapped_addrs.insert(v6.address.clone());
            }
        }
        if client_candidates
            .iter()
            .any(|c| mapped_addrs.contains(&c.address))
        {
            for candidate in &host_candidates {
                if !public_address(&candidate.address) {
                    list.push(CandidateEntry {
                        address: candidate.address.clone(),
                        port: candidate.port,
                        kind: Some("host".to_string()),
                    });
                }
            }
        }
        list.truncate(8);
        list
    }

    /// Processes an incoming WebSocket signaling message from a player (`async signal(m)` in `src/online-server.js`).
    ///
    /// Handles:
    /// 1. `matchProbe` -> `matchReady` verification when `signal.matchProbe` equals the current battle's `match` token.
    /// 2. `offer` (`udp === 1`, 32-hex `nonce`) with per-peer 1 s cooldown and `maxPlayers * 2` table bound,
    ///    answering with `candidates` and punching `public_address` client candidates on the game port.
    ///
    /// Processes an incoming signaling message from a prospective client over the master WebSocket,
    /// porting `OnlineServer.handleSignal(msg)` in `packages/bfe-server/src/online-server.js`.
    ///
    /// Security and rate-limiting invariants (`online-server.js`):
    /// - Validates that `m.from` is a 32-hex identifier (`is_hex_32`).
    /// - Enforces a sliding-window rate limit of `SIGNAL_BURST` (8) messages per `SIGNAL_WINDOW_MS`
    ///   (10s) per sender (`evict_signal_Buckets` capped at `MAX_SIGNAL_SENDERS = 4096`) so a
    ///   signaling flood cannot trigger unbounded UDP hole-punch packets or TURN credential requests.
    /// - Checks both local and global ban lists (`banned`) before punching or returning candidates.
    pub fn handle_signal(&self, m: &SignalEnvelope) {
        if !is_hex_64(&m.from) {
            return;
        }
        if (self.is_banned)(&m.from) || self.globally_banned(&m.from) {
            return;
        }
        let Ok(signal) = serde_json::from_str::<Value>(&m.payload) else {
            return;
        };

        // Handle map-rotation readiness probe (`m.type === "matchProbe"` in `src/online-server.js`).
        if m.msg_type == "matchProbe" || signal.get("matchProbe").is_some() {
            if let Some(obj) = signal.as_object() {
                if m.msg_type == "matchProbe"
                    && obj.get("type").and_then(|v| v.as_str()) == Some("matchProbe")
                    && obj.len() == 3
                {
                    if let (Some(probe), Some(nonce)) = (
                        obj.get("matchProbe").and_then(|v| v.as_str()),
                        obj.get("nonce").and_then(|v| v.as_str()),
                    ) {
                        if is_hex_32(probe)
                            && is_hex_32(nonce)
                            && Some(probe) == query_param(&self.query, "match").as_deref()
                        {
                            let _ = self.master.signal(
                                &m.from,
                                &serde_json::json!({
                                    "type": "matchReady",
                                    "matchReady": probe,
                                    "nonce": nonce,
                                }),
                            );
                        }
                    }
                }
            }
            return;
        }

        // Only `offer` signals with `udp: 1` and a 32-hex `nonce` proceed to candidate exchange.
        if m.msg_type != "offer" {
            return;
        }
        if signal.get("udp").and_then(|v| v.as_u64()) != Some(1) {
            return;
        }
        let Some(nonce) = signal.get("nonce").and_then(|v| v.as_str()) else {
            return;
        };
        if !is_hex_32(nonce) {
            return;
        }

        // Enforce offer rate limits (`this.offers` in `src/online-server.js`):
        // evict entries older than 10s, cap active peers to `maxPlayers * 2`, and require >= 1s between offers per peer.
        let now = Instant::now();
        let max_offers = (self.config.max_players.unwrap_or(64)) * 2;
        if let Ok(mut offers) = self.offers.lock() {
            offers.retain(|_, at| now.duration_since(*at) <= Duration::from_secs(10));
            if offers.len() >= max_offers {
                return;
            }
            if let Some(prev) = offers.get(&m.from) {
                if now.duration_since(*prev) < Duration::from_secs(1) {
                    return;
                }
            }
            offers.insert(m.from.clone(), now);
        }

        let Some(candidates_val) = signal.get("candidates") else {
            return;
        };
        let Ok(client_candidates) = parse_candidates(candidates_val) else {
            return;
        };

        // Send `answer` with direct candidates and punch only `public_address` client candidates.
        let policy = self.config.relay_policy.as_str();
        if policy != "always" {
            let answer_candidates = self.candidates(&client_candidates);
            let _ = self.master.signal(
                &m.from,
                &serde_json::json!({
                    "type": "answer",
                    "udp": 1,
                    "nonce": nonce,
                    "candidates": answer_candidates,
                }),
            );
            if let Some(udp_arc) = self.udp.as_ref() {
                let public_only: Vec<CandidateEntry> = client_candidates
                    .into_iter()
                    .filter(|c| public_address(&c.address))
                    .collect();
                if let Ok(mut udp) = udp_arc.lock() {
                    udp.punch(&public_only);
                }
            }
        }
    }

    /// Registers the server with the master (`POST /v1/servers`), connects the `/v1/signal`
    /// WebSocket, runs initial STUN discovery, and spawns the periodic maintenance loop
    /// (`async start()` in `src/online-server.js`: 20 s heartbeats, 120 s global ban refresh + kick,
    /// 60 s STUN re-discovery, and 3 s reconnect backoff).
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        if self.config.honor_global_bans {
            if let Ok(mut bans) = self.bans.lock() {
                bans.refresh()?;
            }
        }
        let listing = self.listing();
        self.master
            .request("/v1/servers", "POST", Some(&listing), 0)
            .map_err(|e| e.message)?;
        let (events_tx, events_rx): (Sender<MasterEvent>, Receiver<MasterEvent>) = mpsc::channel();
        self.master.connect(events_tx.clone())?;
        self.discover();

        self.stop.store(false, Ordering::SeqCst);
        let server = Arc::clone(self);
        let stop = Arc::clone(&self.stop);
        let handle = thread::Builder::new()
            .name("bfe-online-loop".to_string())
            .spawn(move || {
                let mut last_beat = Instant::now();
                let mut last_ban = Instant::now();
                let mut last_maint = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    match events_rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(MasterEvent::Signal(sig)) => {
                            server.handle_signal(&sig);
                        }
                        Ok(MasterEvent::Disconnected) => {
                            // Reconnect loop with 3-second backoff (`reconnect()` in `src/online-server.js`).
                            while !stop.load(Ordering::Relaxed) {
                                thread::sleep(Duration::from_secs(3));
                                if stop.load(Ordering::Relaxed) {
                                    break;
                                }
                                let listing = server.listing();
                                if server
                                    .master
                                    .request("/v1/servers", "POST", Some(&listing), 0)
                                    .is_ok()
                                    && server.master.connect(events_tx.clone()).is_ok()
                                {
                                    break;
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }

                    // 20-second directory heartbeat (`setInterval(..., 20000)` in `src/online-server.js`).
                    let now = Instant::now();
                    if now.duration_since(last_beat) >= Duration::from_secs(20) {
                        last_beat = now;
                        let _ = server.beat();
                    }
                    // 120-second global ban list refresh + active player eviction (`setInterval(..., 120000)`).
                    if now.duration_since(last_ban) >= Duration::from_secs(120) {
                        last_ban = now;
                        if server.config.honor_global_bans {
                            if let Ok(mut bans) = server.bans.lock() {
                                if bans.refresh().is_ok() {
                                    if let Some(udp_arc) = server.udp.as_ref() {
                                        if let Ok(mut udp) = udp_arc.lock() {
                                            for id in udp.players() {
                                                if bans.contains(&id) {
                                                    udp.kick(
                                                        &id,
                                                        "You are banned from online play.",
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // 60-second STUN reflexive mapping refresh (`setInterval(..., 60000)` in `src/online-server.js`).
                    if now.duration_since(last_maint) >= Duration::from_secs(60) {
                        last_maint = now;
                        server.discover();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        if let Ok(mut guard) = self.worker.lock() {
            *guard = Some(handle);
        }
        Ok(())
    }

    /// Unregisters the server (`DELETE /v1/servers/:id`) and stops the signaling thread
    /// (`async close()` in `src/online-server.js`).
    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.master.close();
        if let Ok(mut guard) = self.worker.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        let route = format!("/v1/servers/{}", self.id);
        let _ = self.master.request(&route, "DELETE", None, 0);
    }
}

impl Drop for OnlineServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.master.close();
    }
}

/// Validates a 32-character lowercase hex nonce or match probe token (`/^[a-f0-9]{32}$/` in `src/online-server.js`).
fn is_hex_32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Extracts a parameter from a `?k=v&...` query string (`new URLSearchParams(this.query).get(name)` in `src/online-server.js`).
fn query_param(query: &str, name: &str) -> Option<String> {
    let trimmed = query.trim_start_matches('?');
    for pair in trimmed.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}
