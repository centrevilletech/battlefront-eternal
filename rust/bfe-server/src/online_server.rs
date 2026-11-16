//! Online directory registration, dual-stack heartbeats, STUN NAT detection, and WebSocket
//! rendezvous signaling.

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
use crate::udp_server::{CandidateEntry, ReflexiveMap, RelayEvent, UdpGameServer};

/// A relay allocation not taken up by a playing player is released after this long
/// (`sweep(30000)`): identities are free, so offers alone must not
/// hold allocations.
const RELAY_UNUSED: Duration = Duration::from_secs(30);

/// One player's relay allocation as signalling sees it (`this.relays`).
#[derive(Clone, Debug)]
struct RelayEntry {
    created: Instant,
    /// The nonce of the player's latest offer: the relayed candidate answers that one.
    nonce: String,
    relayed: Option<SocketAddr>,
}

/// Returns `true` if `address` is a globally routable unicast IPv4 or IPv6 address
/// (`export function publicAddress(address)`).
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
/// (`export function parseCandidates(value)`).
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

/// Parses the STUN server `"host:port"` string (`OnlineServer.stunServer()`),
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
/// (`export class OnlineServer`).
/// Public internet listing, NAT traversal, global ban-list sync, and signaling coordinator,
/// porting `OnlineServer`.
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
    relays: Mutex<HashMap<String, RelayEntry>>,
    /// The STUN host learned from the master's relay URL (see [`Self::stun_server`]).
    master_stun: Mutex<Option<(String, u16)>>,
    /// Local addresses the system sends from towards the STUN server (see [`Self::natted`]).
    route_addresses: Mutex<Vec<String>>,
    udp: Option<Arc<Mutex<UdpGameServer>>>,
    browser: Arc<dyn Fn() -> BrowserInfo + Send + Sync>,
    is_banned: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl OnlineServer {
    /// Constructs an `OnlineServer` (`constructor`), validating the
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
            relays: Mutex::new(HashMap::new()),
            master_stun: Mutex::new(None),
            route_addresses: Mutex::new(Vec::new()),
            udp,
            browser,
            is_banned,
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        })
    }

    /// Builds the directory listing payload (`OnlineServer.listing()`).
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

    /// Sends a heartbeat `PUT /v1/servers/:id` to the master (`async beat()`).
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
    /// (`OnlineServer.globallyBanned(id)`).
    pub fn globally_banned(&self, id: &str) -> bool {
        if !self.config.honor_global_bans {
            return false;
        }
        self.bans
            .lock()
            .map(|b| b.contains(id))
            .unwrap_or(false)
    }

    /// Returns the STUN `(host, port)` pair (`OnlineServer.stunServer()`): the configured one,
    /// else the relay host the master names (so a server registered with another master asks
    /// that master's relay), else the default.
    pub fn stun_server(&self) -> (String, u16) {
        if self.config.stun.is_none() {
            if let Some(stun) = self.master_stun.lock().ok().and_then(|s| s.clone()) {
                return stun;
            }
        }
        parse_stun_server(self.config.stun.as_deref())
    }

    /// Learns the master's relay host for STUN when the configuration names none. A server
    /// with relay policy never is refused relay credentials and keeps the default.
    fn learn_master_stun(&self) {
        if self.config.stun.is_some() {
            return;
        }
        let Ok(servers) = self.master.credentials(&self.id, None) else {
            return;
        };
        if let Some(server) = servers.iter().find(|s| s.relay_type == "TurnUdp") {
            if let Ok(mut stun) = self.master_stun.lock() {
                *stun = Some((server.hostname.clone(), server.port));
            }
        }
    }

    /// Queries the STUN server from the game port to discover the server's reflexive IPv4 and IPv6
    /// addresses (`async discover()`).
    ///
    /// The requests go out from the game socket and their answers come back through the game
    /// loop's poll, so no game packet is lost to the query and the game loop is never held up.
    /// A server whose game socket nobody polls yet reads the answers itself.
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
        let targets: Vec<SocketAddr> = [addrs_v4, addrs_v6].into_iter().flatten().collect();
        if let Ok(mut routes) = self.route_addresses.lock() {
            *routes = targets.iter().filter_map(|t| route_address(*t)).collect();
        }
        let polled = |udp: &Arc<Mutex<UdpGameServer>>| udp.lock().map(|u| u.polls()).unwrap_or(0);
        let before = polled(udp_arc);
        let sent: Vec<[u8; 12]> = match udp_arc.lock() {
            Ok(mut udp) => targets.iter().filter_map(|t| udp.send_binding(*t)).collect(),
            Err(_) => return,
        };
        let deadline = Instant::now() + Duration::from_millis(1500);
        let mut map = ReflexiveMap::default();
        let mut answered = 0;
        while Instant::now() < deadline && answered < sent.len() {
            thread::sleep(Duration::from_millis(20));
            let Ok(mut udp) = udp_arc.lock() else {
                return;
            };
            for tx in &sent {
                if let Some(mapped) = udp.binding_answer(tx) {
                    answered += 1;
                    let entry = CandidateEntry {
                        address: mapped.ip().to_string(),
                        port: mapped.port(),
                        kind: Some("srflx".to_string()),
                    };
                    if mapped.is_ipv4() {
                        map.ipv4 = Some(entry);
                    } else {
                        map.ipv6 = Some(entry);
                    }
                }
            }
        }
        if answered == 0 && polled(udp_arc) == before {
            map = self.discover_unpolled(addrs_v4, addrs_v6);
        }
        // Nothing answered: keep what the last discovery found rather than forget it.
        if map == ReflexiveMap::default() {
            return;
        }
        if let Ok(mut guard) = self.reflexive.lock() {
            *guard = map;
        }
    }

    /// Whether a reflexive address is known.
    fn discovered(&self) -> bool {
        self.reflexive.lock().is_ok_and(|r| *r != ReflexiveMap::default())
    }

    /// Reflexive discovery for a game socket nobody polls: reads the answers from it directly.
    fn discover_unpolled(&self, addrs_v4: Option<SocketAddr>, addrs_v6: Option<SocketAddr>) -> ReflexiveMap {
        let mut map = ReflexiveMap::default();
        let Some(udp_arc) = self.udp.as_ref() else {
            return map;
        };
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
        map
    }

    /// Returns `true` when the server appears to be behind NAT (`OnlineServer.natted()`: no
    /// reflexive mapping or any reflexive address not owned by a local interface).
    ///
    /// The host's own addresses are the game socket's bound address and the local address the
    /// system routes to the STUN server from (a socket bound to every interface names none).
    pub fn natted(&self) -> bool {
        let mut hosts: HashSet<String> = self
            .udp
            .as_ref()
            .and_then(|u| {
                u.lock()
                    .ok()
                    .map(|g| g.candidates().into_iter().map(|c| c.address).collect())
            })
            .unwrap_or_default();
        if let Ok(routes) = self.route_addresses.lock() {
            hosts.extend(routes.iter().cloned());
        }
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
    /// (`OnlineServer.candidates(clientCandidates)`).
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

    /// Processes an incoming WebSocket signaling message from a player (`async signal(m)`).
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

        // Handle map-rotation readiness probe (`m.type === "matchProbe"`).
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

        // Enforce offer rate limits (`this.offers`):
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
                    .iter()
                    .filter(|c| public_address(&c.address))
                    .cloned()
                    .collect();
                if let Ok(mut udp) = udp_arc.lock() {
                    udp.punch(&public_only);
                }
            }
        }

        // A relay follows when this server needs one: always, or behind NAT under "auto".
        self.sweep(RELAY_UNUSED);
        if policy == "always" || (policy == "auto" && self.natted()) {
            if let Err(error) = self.relay_for(&m.from, &client_candidates, nonce) {
                eprintln!("Online relay: {error}");
            }
        }
    }

    /// Most relay allocations at once (`relayLimit()`).
    pub fn relay_limit(&self) -> usize {
        self.config.max_players.unwrap_or(64) + 8
    }

    /// Relay allocations held now.
    pub fn relay_count(&self) -> usize {
        self.relays.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// A TURN allocation for one player, permitted for that player's public addresses only, with
    /// short-lived per-pair credentials from the master (`relayFor`).
    /// The relayed candidate is signalled when the allocation is ready ([`Self::pump_relays`]);
    /// a player who offers again gets the same allocation, answered with the new nonce.
    fn relay_for(&self, peer: &str, client: &[CandidateEntry], nonce: &str) -> Result<(), String> {
        let udp_arc = self.udp.as_ref().ok_or("no game port")?;
        let permit: Vec<IpAddr> = client
            .iter()
            .filter(|c| public_address(&c.address))
            .filter_map(|c| c.address.split('%').next()?.parse().ok())
            .collect();
        {
            let mut relays = self.relays.lock().map_err(|_| "lock poisoned")?;
            if let Some(entry) = relays.get_mut(peer) {
                entry.nonce = nonce.to_string();
                let relayed = entry.relayed;
                drop(relays);
                if let Ok(mut udp) = udp_arc.lock() {
                    udp.permit_relay(peer, &permit);
                }
                if let Some(relayed) = relayed {
                    self.signal_relay(peer, nonce, relayed);
                }
                return Ok(());
            }
            if relays.len() >= self.relay_limit() {
                return Err("limit reached".to_string());
            }
        }
        let servers = self.master.credentials(&self.id, Some(peer))?;
        let server = servers
            .iter()
            .find(|s| s.relay_type == "TurnUdp")
            .ok_or("no UDP relay offered")?;
        let address = resolve_relay(&server.hostname, server.port)?;
        udp_arc
            .lock()
            .map_err(|_| "lock poisoned")?
            .open_relay(peer, address, &server.username, &server.password, &permit)?;
        self.relays.lock().map_err(|_| "lock poisoned")?.insert(
            peer.to_string(),
            RelayEntry {
                created: Instant::now(),
                nonce: nonce.to_string(),
                relayed: None,
            },
        );
        Ok(())
    }

    /// Signals ready relays to their players and forgets failed ones.
    pub fn pump_relays(&self) {
        let Some(udp_arc) = self.udp.as_ref() else {
            return;
        };
        let events = match udp_arc.lock() {
            Ok(mut udp) => udp.relay_events(),
            Err(_) => return,
        };
        for event in events {
            match event {
                RelayEvent::Ready { peer_id, relayed } => {
                    let nonce = self.relays.lock().ok().and_then(|mut relays| {
                        let entry = relays.get_mut(&peer_id)?;
                        entry.relayed = Some(relayed);
                        Some(entry.nonce.clone())
                    });
                    if let Some(nonce) = nonce {
                        eprintln!("Online relay: {relayed} allocated for player {}", short(&peer_id));
                        self.signal_relay(&peer_id, &nonce, relayed);
                    }
                }
                RelayEvent::Failed { peer_id, reason } => {
                    eprintln!("Online relay: player {}: {reason}", short(&peer_id));
                    if let Ok(mut relays) = self.relays.lock() {
                        relays.remove(&peer_id);
                    }
                }
            }
        }
    }

    /// The relayed candidate: an answer of its own under "always", a later candidate otherwise.
    fn signal_relay(&self, peer: &str, nonce: &str, relayed: SocketAddr) {
        let kind = if self.config.relay_policy == "always" { "answer" } else { "candidate" };
        let _ = self.master.signal(
            peer,
            &serde_json::json!({
                "type": kind,
                "udp": 1,
                "nonce": nonce,
                "candidates": [{
                    "address": relayed.ip().to_string(),
                    "port": relayed.port(),
                    "kind": "relay",
                }],
            }),
        );
    }

    /// Releases relay allocations of players not playing after `max_age` (`sweep`).
    pub fn sweep(&self, max_age: Duration) {
        let Some(udp_arc) = self.udp.as_ref() else {
            return;
        };
        let Ok(mut relays) = self.relays.lock() else {
            return;
        };
        if relays.is_empty() {
            return;
        }
        let Ok(mut udp) = udp_arc.lock() else {
            return;
        };
        let playing: HashSet<String> = udp.players().into_iter().collect();
        relays.retain(|id, entry| {
            let keep = playing.contains(id) || entry.created.elapsed() <= max_age;
            if !keep {
                eprintln!("Online relay: released for player {} (not playing)", short(id));
                udp.close_relay(id);
            }
            keep
        });
    }

    /// Releases the relay allocations of players who are not playing. A playing player's
    /// allocation stays with the game socket, which the next battle keeps: that battle's
    /// controller adopts it (`adopt_relays`) and releases it once the player has left.
    fn close_relays(&self) {
        let Ok(mut relays) = self.relays.lock() else {
            return;
        };
        if let Some(Ok(mut udp)) = self.udp.as_ref().map(|u| u.lock()) {
            let playing: HashSet<String> = udp.players().into_iter().collect();
            for id in relays.keys().filter(|id| !playing.contains(*id)) {
                udp.close_relay(id);
            }
        }
        relays.clear();
    }

    /// Takes over the allocations a previous battle left open on this game socket.
    fn adopt_relays(&self) {
        let Some(Ok(udp)) = self.udp.as_ref().map(|u| u.lock()) else {
            return;
        };
        let Ok(mut relays) = self.relays.lock() else {
            return;
        };
        for (id, relayed) in udp.relayed_players() {
            relays.entry(id).or_insert_with(|| RelayEntry {
                created: Instant::now(),
                nonce: String::new(),
                relayed,
            });
        }
    }

    /// Registers the server with the master (`POST /v1/servers`), connects the `/v1/signal`
    /// WebSocket, runs initial STUN discovery, and spawns the periodic maintenance loop
    /// (`async start()`: 20 s heartbeats, 120 s global ban refresh + kick,
    /// 60 s STUN re-discovery, and 3 s reconnect backoff).
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        self.adopt_relays();
        if self.config.honor_global_bans {
            if let Ok(mut bans) = self.bans.lock() {
                bans.refresh()?;
            }
        }
        let listing = self.listing();
        self.master
            .request("/v1/servers", "POST", Some(&listing), 0)
            .map_err(|e| e.message)?;
        self.learn_master_stun();
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
                // Until a reflexive address is known, discovery is retried after 5, 10, 20 and
                // 40 s, then with the minute's maintenance.
                let mut retry = Duration::from_secs(5);
                while !stop.load(Ordering::Relaxed) {
                    match events_rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(MasterEvent::Signal(sig)) => {
                            server.handle_signal(&sig);
                        }
                        Ok(MasterEvent::Disconnected) => {
                            // Reconnect loop with 3-second backoff (`reconnect()`).
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
                    server.pump_relays();

                    // 20-second directory heartbeat (`setInterval(..., 20000)`).
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
                    // 60-second STUN reflexive mapping refresh (`setInterval(..., 60000)`).
                    if now.duration_since(last_maint) >= Duration::from_secs(60) {
                        last_maint = now;
                        server.discover();
                        server.sweep(RELAY_UNUSED);
                    } else if retry <= Duration::from_secs(40)
                        && now.duration_since(last_maint) >= retry
                        && server.config.relay_policy != "always"
                        && !server.discovered()
                    {
                        retry *= 2;
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
    /// (`async close()`).
    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.master.close();
        if let Ok(mut guard) = self.worker.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        self.close_relays();
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

/// The first characters of a player identity, for the log.
fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

/// The local address the system would send from to reach `target` (no packet is sent).
fn route_address(target: SocketAddr) -> Option<String> {
    let any = if target.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let socket = std::net::UdpSocket::bind(any).ok()?;
    socket.connect(target).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then(|| ip.to_string())
}

/// The relay server's address, IPv4 first.
fn resolve_relay(host: &str, port: u16) -> Result<SocketAddr, String> {
    let addresses: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("relay address {host}: {e}"))?
        .collect();
    addresses
        .iter()
        .find(|a| a.is_ipv4())
        .or_else(|| addresses.first())
        .copied()
        .ok_or_else(|| format!("relay address {host}: not found"))
}

/// Validates a 32-character lowercase hex nonce or match probe token (`/^[a-f0-9]{32}$/`).
fn is_hex_32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Extracts a parameter from a `?k=v&...` query string (`new URLSearchParams(this.query).get(name)`).
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
