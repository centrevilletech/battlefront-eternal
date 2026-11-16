//! UDP game server listener, client authentication, per-prefix player caps, and session routing
//! (and `WorldRouter`).

use crate::game_session::{GameSessionState, SessionAction};
use crate::udp_peer::{valid_incoming_state_payload, PeerPath, UdpPeerState};
use bfe_transport::connection::{Closed, Lane};
use bfe_transport::crypto::{Identity, RetryTokens};
use bfe_transport::layout::{close_code, network_contract};
use bfe_transport::server::{
    address_prefix, normalize_socket_addr, valid_peer_ip, AdmitDecision, ClientInfo,
    ServerEndpoint, ServerSink,
};
use bfe_transport::stun;
use bfe_transport::turn::{TurnClient, TurnEvent, TurnState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum UDP datagram buffer length read on the game port (`1500` bytes Ethernet MTU).
/// Any `BFE-UDP/1` datagram exceeding `PACKET_MAX_BYTES` (`1200` bytes) is rejected before parsing
/// (and `crates/bfe-transport/src/server.rs`).
const MAX_PACKET: usize = 1500;

/// Signaling candidate descriptor (`hostCandidates` and `parseCandidates`).
///
/// Why `kind` is optional:
/// Direct LAN / configured host entries carry `kind: Some("host")`, STUN bindings carry
/// `kind: Some("srflx")`, and raw operator or client candidates omit `kind` when unspecified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateEntry {
    /// IPv4 or IPv6 literal address (`"203.0.113.10"` or `"2001:db8::1"`).
    pub address: String,
    /// UDP port in `1..=65535`.
    pub port: u16,
    /// Candidate type (`"host"`, `"srflx"`, or `"relay"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Discovered STUN reflexive addresses per IP family (`this.reflexive`).
///
/// Tracks IPv4 and IPv6 public NAT mappings independently so dual-stack servers advertise both
/// `srflx` candidates to the master coordinator (`candidates()`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflexiveMap {
    /// Public IPv4 `srflx` mapping discovered via STUN (`0x0001`), if reachable.
    pub ipv4: Option<CandidateEntry>,
    /// Public IPv6 `srflx` mapping discovered via STUN (`0x0001`), if reachable.
    pub ipv6: Option<CandidateEntry>,
}

/// Paired transport peer state (`UdpPeer`) and game session state
/// (`GameSession`) for one connected player.
///
/// Keeping `UdpPeerState` and `GameSessionState` paired by 64-hex Ed25519 identity mirrors
/// `peers.set(id, peer)` + `sessions.set(id, session)` and `src/game-session.js`.
pub struct ConnectedPeer {
    /// Per-peer `BFE-UDP/1` token bucket, congestion watermark, and `NetMetrics` tracker.
    pub peer: UdpPeerState,
    /// Per-peer input/control rate limiters, ping/pong RTT tracker, and 15 s silence timer.
    pub session: GameSessionState,
    /// The last edge-results message sent and when, so a repeat of it is not queued again at once.
    last_edges: Option<(Vec<u8>, f64)>,
    /// Superseding messages not queued because the reliable lane was backed up, or repeated.
    pub superseded: u64,
}

/// Reliable bytes pending for a player above which superseding messages wait (see
/// [`UdpGameServer::send_superseding`]).
const SUPERSEDING_BACKLOG_BYTES: usize = 8_192;

/// Events emitted by [`UdpGameServer::poll`] for the world loop (`WorldRouter`).
#[derive(Clone, Debug)]
pub enum WorldPeerEvent {
    /// A client completed the `BFE-UDP/1` handshake and was admitted (`admitSession`).
    Joined {
        id: String,
        name: String,
        address: String,
        prefix: String,
        path: PeerPath,
    },
    /// A client session closed or was evicted (`peer.once("closed", ...)`).
    Left {
        id: String,
    },
    /// A client called the `start` RPC (`m.method === "start"`).
    Start {
        id: String,
        call_id: Option<Value>,
        args: Value,
    },
    /// A client submitted a chat message (`m.type === "chat"`).
    Chat {
        id: String,
        text: String,
        team: bool,
    },
    /// A client acknowledged reliable entity-edge results (`entity-edge-ack`).
    EdgeAck {
        id: String,
        message: Value,
    },
    /// A client sent a validated, rate-limited `BNI1` or `BNA1` binary datagram (`GameSession.binary`).
    Input {
        id: String,
        bytes: Vec<u8>,
    },
    /// A client requested or requires a snapshot delta baseline reset (`resync`).
    Resync {
        id: String,
    },
}

/// What became of a relay allocation opened with [`UdpGameServer::open_relay`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayEvent {
    /// The relayed address is allocated and the player's addresses are permitted.
    Ready { peer_id: String, relayed: SocketAddr },
    /// The allocation failed or expired; it is gone.
    Failed { peer_id: String, reason: String },
}

/// Peer addresses one relay allocation may carry at once (see `Relays::route`).
const ROUTES_PER_RELAY: usize = 8;

/// One player's TURN allocation. Each has its own UDP socket: a TURN server tells allocations
/// apart by the client's address and port, so one game socket could hold only one.
struct RelayAllocation {
    peer_id: String,
    socket: UdpSocket,
    turn: TurnClient,
}

/// The relay allocations, and which peer addresses currently reach the game through which one.
/// A peer address is routed through a relay from the moment a packet arrives from it through
/// that relay until one arrives from it directly (the last path heard from wins, as for direct
/// path changes).
#[derive(Default)]
struct Relays {
    allocations: HashMap<u32, RelayAllocation>,
    next_id: u32,
    routes: HashMap<SocketAddr, u32>,
    events: Vec<RelayEvent>,
    buf: Vec<u8>,
}

impl Relays {
    /// Sends a game datagram to `to`: through its relay when it is routed through one.
    fn send(&mut self, socket: &UdpSocket, bytes: &[u8], to: SocketAddr, now_ms: f64) {
        if !self.routes.is_empty() {
            if let Some(id) = self.routes.get(&normalize_socket_addr(to)) {
                if let Some(relay) = self.allocations.get_mut(id) {
                    if relay.turn.send_to(to, bytes, &mut self.buf, now_ms) {
                        crate::sockets::send_to(&relay.socket, &self.buf, relay.turn.server());
                    }
                    return;
                }
            }
        }
        crate::sockets::send_to(socket, bytes, to);
    }

    /// Routes `peer` through allocation `id`. A player's packets arrive from a few addresses at
    /// most; one allocation keeps at most [`ROUTES_PER_RELAY`], dropping its others for a new one.
    fn route(&mut self, peer: SocketAddr, id: u32) {
        if self.routes.get(&peer) == Some(&id) {
            return;
        }
        if self.routes.values().filter(|routed| **routed == id).count() >= ROUTES_PER_RELAY {
            self.routes.retain(|_, routed| *routed != id);
        }
        self.routes.insert(peer, id);
    }

    fn find(&self, peer_id: &str) -> Option<u32> {
        self.allocations.iter().find(|(_, r)| r.peer_id == peer_id).map(|(id, _)| *id)
    }

    /// Sends what each allocation has queued and collects its events; failed ones are removed.
    fn pump(&mut self, now_ms: f64) {
        let mut failed = Vec::new();
        for (id, relay) in &mut self.allocations {
            relay.turn.poll(now_ms);
            let server = relay.turn.server();
            for packet in relay.turn.transmit() {
                crate::sockets::send_to(&relay.socket, &packet, server);
            }
            for event in relay.turn.events() {
                self.events.push(match event {
                    TurnEvent::Ready { relayed, .. } => RelayEvent::Ready { peer_id: relay.peer_id.clone(), relayed },
                    TurnEvent::Failed(reason) => RelayEvent::Failed { peer_id: relay.peer_id.clone(), reason },
                });
            }
            if relay.turn.state() == TurnState::Failed {
                failed.push(*id);
            }
        }
        for id in failed {
            self.remove(id);
        }
    }

    /// Releases an allocation (a Refresh with lifetime 0) and forgets its routes.
    fn remove(&mut self, id: u32) {
        if let Some(mut relay) = self.allocations.remove(&id) {
            relay.turn.close();
            let server = relay.turn.server();
            for packet in relay.turn.transmit() {
                crate::sockets::send_to(&relay.socket, &packet, server);
            }
        }
        self.routes.retain(|_, routed| *routed != id);
    }
}

/// `ServerSink` implementation used when flushing outbound packets or closing a session outside `poll()`.
///
/// Admission and message callbacks are no-ops because no inbound datagrams are fed through
/// `SendOnlySink`. Closes are kept: the transport's idle timeout can fire inside `flush()`, and it
/// reports each close once, so `poll()` retires those players later.
struct SendOnlySink<'a> {
    socket: &'a UdpSocket,
    relays: &'a mut Relays,
    closed: &'a mut Vec<u32>,
    now_ms: f64,
}

impl ServerSink for SendOnlySink<'_> {
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
        self.relays.send(self.socket, bytes, to, self.now_ms);
    }

    fn on_hello(&mut self) -> Value {
        Value::Null
    }

    fn on_admit(&mut self, _client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
        AdmitDecision::reject(close_code::REJECTED as u64, "unavailable")
    }

    fn on_session(&mut self, _session_id: u32, _client: &ClientInfo, _remote: SocketAddr, _rtt: f64) {}

    fn on_message(&mut self, _session_id: u32, _lane: Lane, _text: bool, _bytes: &[u8]) {}

    fn on_path(&mut self, _session_id: u32, _new_path: SocketAddr, _old_path: SocketAddr) {}

    fn on_close(&mut self, session_id: u32, _closed: Closed) {
        self.closed.push(session_id);
    }
}

/// `ServerSink` implementation used during [`UdpGameServer::poll`] to collect newly admitted
/// sessions, incoming lane messages, path migrations, and closed sessions from `ServerEndpoint`.
struct PollSink<'a, F: FnMut(&str) -> bool> {
    socket: &'a UdpSocket,
    relays: &'a mut Relays,
    bindings: &'a mut Bindings,
    now_ms: f64,
    content: &'a str,
    fingerprint: &'a str,
    max_players: usize,
    active_players: usize,
    is_banned: &'a mut F,
    admitted: Vec<(u32, ClientInfo, SocketAddr, f64)>,
    messages: Vec<(u32, Lane, bool, Vec<u8>)>,
    paths: Vec<(u32, SocketAddr)>,
    closed: Vec<u32>,
}

/// The refusal a player sees when their game and this server differ: which side is out of date and
/// what to do. A client of the same version whose game data differs is told so.
pub fn version_refusal(protocol: usize, profile: &str, simulation: &str) -> String {
    let number = |sim: &str| sim.strip_prefix("bfe-sim-").and_then(|n| n.parse::<u64>().ok());
    let ours = number(network_contract::SIMULATION).unwrap_or(0);
    let theirs = number(simulation);
    let older = if protocol != network_contract::PROTOCOL {
        Some(protocol < network_contract::PROTOCOL)
    } else if simulation != network_contract::SIMULATION {
        theirs.map(|n| n < ours)
    } else if profile != network_contract::PROFILE {
        None
    } else {
        return "Your game data differs from this server's. Update the game; if it is up to date, \
                prepare its game data again."
            .to_string();
    };
    let versions = match theirs {
        Some(n) if n != ours => format!(" (your game: simulation {n}, this server: {ours})"),
        _ => String::new(),
    };
    match older {
        Some(true) => format!(
            "Your game is older than this server{versions}. Update Battlefront Eternal to join."
        ),
        Some(false) => format!(
            "This server runs an older version of Battlefront Eternal{versions}. \
             It can be joined once the server updates."
        ),
        None => format!("Your game and this server are different versions{versions}."),
    }
}

impl<F: FnMut(&str) -> bool> ServerSink for PollSink<'_, F> {
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
        self.relays.send(self.socket, bytes, to, self.now_ms);
    }

    /// Builds the server handshake `hello` payload (`server.hello` in `startUdpServer`).
    ///
    /// Includes only the compact version/digest fields (`protocol`, `profile`, `simulation`, `content`,
    /// `fingerprint`) so the encrypted `RESPONSE` packet stays well below the 1200-byte UDP MTU.
    fn on_hello(&mut self) -> Value {
        json!({
            "protocol": network_contract::PROTOCOL,
            "profile": network_contract::PROFILE,
            "simulation": network_contract::SIMULATION,
            "content": self.content,
            "fingerprint": self.fingerprint,
        })
    }

    /// Evaluates handshake admission policy (`server.admit` in `startUdpServer`):
    /// 1. Wire compatibility (`protocol`, `profile`, `simulation`) and content digest check (`CLOSE_CODE.VERSION`).
    /// 2. Local and global ban check (`CLOSE_CODE.REJECTED`, `"You are banned from this server."`).
    /// 3. Player capacity check (`CLOSE_CODE.REJECTED`, `"The server is full."`).
    fn on_admit(&mut self, client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
        let hello = &client.hello;
        let hello_protocol = hello.get("protocol").and_then(Value::as_u64).unwrap_or(0) as usize;
        let hello_profile = hello
            .get("profile")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let hello_sim = hello
            .get("simulation")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let hello_content = hello
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();

        // Reject clients running a mismatched wire protocol, snapshot profile, simulation build, or content pack.
        if hello_protocol != network_contract::PROTOCOL
            || hello_profile != network_contract::PROFILE
            || hello_sim != network_contract::SIMULATION
            || (hello_content != self.fingerprint && hello_content != self.content)
        {
            // Earlier games show this text as it is, so it says which side is out of date.
            let reason = version_refusal(hello_protocol, hello_profile, hello_sim);
            return AdmitDecision::reject(close_code::VERSION as u64, &reason);
        }
        // Reject locally or globally banned Ed25519 player identities (`isBanned(client.id)`).
        if (self.is_banned)(&client.id_hex) {
            return AdmitDecision::reject(
                close_code::REJECTED as u64,
                "You are banned from this server.",
            );
        }
        // Reject new joins once active player count reaches `maxPlayers` (`peers.size >= maxPlayers`).
        if self.active_players >= self.max_players {
            return AdmitDecision::reject(close_code::REJECTED as u64, "The server is full.");
        }
        AdmitDecision::accept()
    }

    fn on_session(&mut self, session_id: u32, client: &ClientInfo, remote: SocketAddr, rtt: f64) {
        self.active_players += 1;
        self.admitted
            .push((session_id, client.clone(), remote, rtt));
    }

    fn on_message(&mut self, session_id: u32, lane: Lane, text: bool, bytes: &[u8]) {
        self.messages
            .push((session_id, lane, text, bytes.to_vec()));
    }

    fn on_path(&mut self, session_id: u32, new_path: SocketAddr, _old_path: SocketAddr) {
        self.paths.push((session_id, new_path));
    }

    fn on_close(&mut self, session_id: u32, _closed: Closed) {
        self.closed.push(session_id);
    }

    fn on_stun_response(&mut self, msg: &stun::StunMessage<'_>, _from: SocketAddr) {
        self.bindings.answer(msg);
    }
}

/// STUN Binding requests sent from the game socket and their answers, which arrive through
/// [`UdpGameServer::poll`] like every other datagram (nothing else may read the game socket).
#[derive(Default)]
struct Bindings {
    pending: Vec<[u8; 12]>,
    answers: Vec<([u8; 12], SocketAddr)>,
}

impl Bindings {
    fn answer(&mut self, msg: &stun::StunMessage<'_>) {
        if msg.klass != stun::class::SUCCESS {
            return;
        }
        if let Some(i) = self.pending.iter().position(|tx| *tx == msg.transaction) {
            self.pending.swap_remove(i);
            if let Some(mapped) = msg.mapped_address() {
                self.answers.push((msg.transaction, normalize_socket_addr(mapped)));
            }
        }
    }
}

/// Authoritative UDP game port server (`export async function startUdpServer`).
/// Authoritative UDP game server porting `createUdpServer(options)`.
pub struct UdpGameServer {
    /// Non-blocking dual-stack UDP socket bound to the game port (`18451`).
    socket: UdpSocket,
    /// Actual bound UDP port (`bound.port`).
    pub port: u16,
    /// Bound socket address family (`"IPv6"` dual-stack or `"IPv4"` fallback).
    pub family: &'static str,
    /// Underlying `BFE-UDP/1` transport server endpoint (`bfe_transport::server::ServerEndpoint`).
    pub endpoint: ServerEndpoint,
    /// Connected players keyed by 64-hex Ed25519 identity (`peers` map).
    pub peers: HashMap<String, ConnectedPeer>,
    /// Reverse lookup from `ServerEndpoint` session ID (`u32`) to 64-hex player identity.
    pub by_session: HashMap<u32, String>,
    /// Active session count per IPv4 `/32` or IPv6 `/64` prefix (`byPrefix`).
    pub by_prefix: HashMap<String, usize>,
    /// Maximum simultaneous players allowed on the server (`maxPlayers`).
    pub max_players: usize,
    /// Maximum simultaneous sessions allowed from one network prefix (`maxPlayersPerAddress`).
    pub max_players_per_address: usize,
    /// Per-client outbound `state` lane token-bucket rate in bytes/sec (`stateBytesPerSecond`).
    pub state_bytes_per_second: usize,
    /// Content identifier string advertised in the handshake `hello`.
    pub content: String,
    /// `bfe-content-sha256-...` digest verified during `on_admit`.
    pub fingerprint: String,
    /// The announced next battle (`query`, deadline in `now_ms` time) once this battle has
    /// ended: players who join during the end table are told it too, so they follow it.
    pub pending_match: Option<(String, f64)>,
    /// Sessions the transport closed during a flush or an explicit close (see `SendOnlySink`).
    closed_outside_poll: Vec<u32>,
    /// Monotonic start instant for `now_ms()` (`performance.now()`).
    start_instant: Instant,
    /// Reusable 1500-byte UDP receive buffer.
    recv_buf: [u8; MAX_PACKET],
    /// TURN allocations for players who reach this server through a relay.
    relays: Relays,
    /// Reflexive address queries in flight on the game socket.
    bindings: Bindings,
    /// Completed `poll` calls, so a waiting caller knows answers can arrive.
    polls: u64,
}

impl UdpGameServer {
    /// Binds the UDP game server socket (`startUdpServer` and `attachNetwork`).
    ///
    /// Computes `maxSessions = (maxPlayers * 2 + 32).max(32)` matching
    /// `Math.max(32, (config.maxPlayers ?? 64) * 2 + 32)` in `src/main.js` so transport session
    /// slots have headroom for concurrent reconnects and handshakes while `maxPlayers` caps admitted
    /// players.
    #[allow(clippy::too_many_arguments)]
    /// Binds a non-blocking dual-stack UDP socket (`[::]:port` with `IPV6_V6ONLY = 0`, falling back
    /// to `0.0.0.0:port` when IPv6 is disabled on the host) and initializes `bfe_transport::Server`,
    /// porting `createUdpServer(options)`.
    pub fn bind(
        port: u16,
        bind_host: Option<&str>,
        identity: Identity,
        max_players: usize,
        max_players_per_address: usize,
        state_bytes_per_second: usize,
        content: String,
        fingerprint: String,
    ) -> Result<Self, String> {
        let (socket, family) = bind_dual_stack_udp(port, bind_host)?;
        let actual_port = socket.local_addr().map(|a| a.port()).unwrap_or(port);
        eprintln!("server UDP bound local={:?} family={family}", socket.local_addr());
        let _ = socket.set_nonblocking(true);
        let retry = RetryTokens::new().map_err(|e| format!("failed to init retry tokens: {e}"))?;
        let max_sessions = (max_players * 2 + 32).max(32);
        let endpoint = ServerEndpoint::new(
            identity,
            retry,
            actual_port,
            max_sessions,
            Some(max_players_per_address),
            8.0,
        );
        Ok(Self {
            socket,
            port: actual_port,
            family,
            endpoint,
            peers: HashMap::new(),
            by_session: HashMap::new(),
            by_prefix: HashMap::new(),
            max_players,
            max_players_per_address,
            state_bytes_per_second,
            content,
            fingerprint,
            pending_match: None,
            closed_outside_poll: Vec::new(),
            start_instant: Instant::now(),
            recv_buf: [0u8; MAX_PACKET],
            relays: Relays::default(),
            bindings: Bindings::default(),
            polls: 0,
        })
    }

    /// Returns the bound UDP game port (`bound.port`).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the list of currently connected 64-hex player identities (`players: () => [...peers.keys()]`).
    pub fn players(&self) -> Vec<String> {
        self.peers.keys().cloned().collect()
    }

    /// Returns direct host candidates for this socket (`export function hostCandidates`).
    pub fn candidates(&self) -> Vec<CandidateEntry> {
        let mut out = Vec::new();
        if let Ok(local) = self.socket.local_addr() {
            let ip = local.ip();
            if !ip.is_unspecified() {
                out.push(CandidateEntry {
                    address: ip.to_string(),
                    port: self.port,
                    kind: Some("host".to_string()),
                });
            }
        }
        out
    }

    /// Sends a STUN Binding request to `server` from the game socket; its answer arrives through
    /// [`Self::poll`] and is read with [`Self::binding_answer`]. At most four are in flight.
    pub fn send_binding(&mut self, server: SocketAddr) -> Option<[u8; 12]> {
        let tx = stun::new_transaction().ok()?;
        if self.bindings.pending.len() >= 4 {
            self.bindings.pending.remove(0);
        }
        self.bindings.pending.push(tx);
        self.bindings.answers.retain(|(t, _)| self.bindings.pending.contains(t));
        crate::sockets::send_to(&self.socket, &stun::encode_binding_request(&tx), server);
        Some(tx)
    }

    /// The reflexive address answered to a [`Self::send_binding`] request, once it has arrived.
    pub fn binding_answer(&mut self, tx: &[u8; 12]) -> Option<SocketAddr> {
        let i = self.bindings.answers.iter().position(|(t, _)| t == tx)?;
        Some(self.bindings.answers.swap_remove(i).1)
    }

    /// Completed [`Self::poll`] calls.
    pub fn polls(&self) -> u64 {
        self.polls
    }

    /// Performs an RFC 5389 STUN Binding Request over the game port socket to discover the server's
    /// public reflexive address (`endpoint.reflexive`), reading the socket itself: only for a
    /// server whose game socket nobody polls (it drops anything else that arrives).
    pub fn reflexive(&self, server: SocketAddr, timeout: Duration) -> Result<ReflexiveMap, String> {
        let tx = stun::new_transaction().map_err(|e| e.to_string())?;
        let req = stun::encode_binding_request(&tx);
        crate::sockets::send_to(&self.socket, &req, server);
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 512];
        while Instant::now() < deadline {
            match crate::sockets::recv_from(&self.socket, &mut buf) {
                Ok((n, _from)) => {
                    if let Some(msg) = stun::decode_message(&buf[..n]) {
                        if msg.klass == stun::class::SUCCESS && msg.transaction == tx {
                            if let Some(mapped) = msg.mapped_address() {
                                let mut out = ReflexiveMap::default();
                                let entry = CandidateEntry {
                                    address: mapped.ip().to_string(),
                                    port: mapped.port(),
                                    kind: Some("srflx".to_string()),
                                };
                                if mapped.is_ipv4() {
                                    out.ipv4 = Some(entry);
                                } else {
                                    out.ipv6 = Some(entry);
                                }
                                return Ok(out);
                            }
                        }
                    }
                }
                Err(e)
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(ReflexiveMap::default())
    }

    /// Monotonic milliseconds elapsed since server bind (`performance.now()`).
    pub fn now_ms(&self) -> f64 {
        self.start_instant.elapsed().as_secs_f64() * 1000.0
    }

    /// Current Unix epoch time in milliseconds (`Date.now()`).
    pub fn unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Updates the active content fingerprint on map rotation (`setBattle`).
    pub fn set_fingerprint(&mut self, fingerprint: String) {
        self.fingerprint = fingerprint;
    }

    /// Schedules UDP hole-punch datagrams (`0xb8`, `endpoint.punch`).
    pub fn punch(&mut self, candidates: &[CandidateEntry]) {
        let mut addrs = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let Ok(ip) = candidate.address.parse::<IpAddr>() else {
                continue;
            };
            if !valid_peer_ip(ip) {
                continue;
            }
            addrs.push(SocketAddr::new(ip, candidate.port));
        }
        let now_ms = self.now_ms();
        self.endpoint.punch(&addrs, now_ms);
    }

    /// Schedules a single UDP hole-punch datagram to `host:port`.
    pub fn punch_one(&mut self, host: &str, port: u16) -> bool {
        let Ok(ip) = host.parse::<IpAddr>() else {
            return false;
        };
        if !valid_peer_ip(ip) {
            return false;
        }
        let target = SocketAddr::new(ip, port);
        let now_ms = self.now_ms();
        self.endpoint.punch(&[target], now_ms);
        true
    }

    /// Closes a player's session with `CLOSE_CODE.KICKED` (`kick(id, message)`).
    pub fn kick(&mut self, peer_id: &str, reason: &str) {
        let now_ms = self.now_ms();
        self.close_peer(peer_id, close_code::KICKED as u16, reason, now_ms);
    }

    /// Drains pending UDP datagrams from the non-blocking socket, advances `ServerEndpoint`
    /// handshakes and sessions, runs `GameSessionState` input/control/liveness logic, and returns
    /// [`WorldPeerEvent`] items for the world tick thread (`startUdpServer` + `WorldRouter`).
    /// Drains all available UDP datagrams from the socket, advances the `bfe_transport::Server`
    /// handshake/session state machine, enforces per-peer rate limits and idle timeouts, and
    /// returns the ordered list of `WorldPeerEvent`s for `NativeWorldHost`, porting the socket
    /// receive + timer callbacks.
    pub fn poll<F>(&mut self, mut is_banned: F) -> Vec<WorldPeerEvent>
    where
        F: FnMut(&str) -> bool,
    {
        let now_ms = self.now_ms();
        let mut events = Vec::new();

        let mut admitted = Vec::new();
        let mut messages = Vec::new();
        let mut paths = Vec::new();
        let mut closed = Vec::new();

        {
            let mut sink = PollSink {
                socket: &self.socket,
                relays: &mut self.relays,
                bindings: &mut self.bindings,
                now_ms,
                content: &self.content,
                fingerprint: &self.fingerprint,
                max_players: self.max_players,
                active_players: self.peers.len(),
                is_banned: &mut is_banned,
                admitted: Vec::new(),
                messages: Vec::new(),
                paths: Vec::new(),
                closed: Vec::new(),
            };

            loop {
                match crate::sockets::recv_from(&self.socket, &mut self.recv_buf) {
                    Ok((len, rinfo)) => {
                        // `BFE-UDP/1` datagrams are capped at 1200 bytes (`PACKET_MAX_BYTES`).
                        // Oversize or zero-length datagrams are dropped before touching the crypto or session tables.
                        if len == 0 || len > 1200 {
                            continue;
                        }
                        let packet = &self.recv_buf[..len];
                        // A packet straight from a relayed peer's address moves it back to the direct path.
                        if !sink.relays.routes.is_empty() {
                            sink.relays.routes.remove(&normalize_socket_addr(rinfo));
                        }
                        self.endpoint.receive(packet, rinfo, now_ms, &mut sink);
                    }
                    Err(e)
                        if e.kind() == ErrorKind::WouldBlock
                            || e.kind() == ErrorKind::TimedOut =>
                    {
                        break;
                    }
                    Err(_) => break,
                }
            }

            // Datagrams from the TURN servers: responses go to their allocation, relayed game
            // packets to the transport with the peer's address as their path.
            let ids: Vec<u32> = sink.relays.allocations.keys().copied().collect();
            for id in ids {
                while let Some(relay) = sink.relays.allocations.get_mut(&id) {
                    let Ok((len, from)) = crate::sockets::recv_from(&relay.socket, &mut self.recv_buf) else {
                        break;
                    };
                    if normalize_socket_addr(from) != relay.turn.server() {
                        continue;
                    }
                    let Some((peer, data)) = relay.turn.handle(&self.recv_buf[..len], now_ms) else {
                        continue;
                    };
                    if data.is_empty() || data.len() > 1200 {
                        continue;
                    }
                    sink.relays.route(peer, id);
                    self.endpoint.receive(data, peer, now_ms, &mut sink);
                }
            }
            sink.relays.pump(now_ms);

            // Advance transport timers (handshake expirations, punch bursts, keepalives, retransmit timers).
            self.endpoint.poll(now_ms, &mut sink);

            admitted.append(&mut sink.admitted);
            messages.append(&mut sink.messages);
            paths.append(&mut sink.paths);
            closed.append(&mut sink.closed);
        }

        // Admit newly established sessions (`admitSession`).
        for (sid, client, remote, rtt) in admitted {
            let peer_id = client.id_hex.clone();
            let name = clean_player_name(client.hello.get("name").and_then(Value::as_str));
            let path = self.path_to(remote);
            let address = remote.ip().to_string();
            let prefix = address_prefix(remote.ip());

            // A reconnect replaces a stale session of the same identity at once (`peers.get(session.id)?.close("replaced")`).
            if let Some(old) = self.peers.remove(&peer_id) {
                self.by_session.remove(&old.peer.session_id);
                self.dec_prefix(&old.peer.prefix);
                let mut sink = SendOnlySink {
                    socket: &self.socket,
                    relays: &mut self.relays,
                    closed: &mut self.closed_outside_poll,
                    now_ms,
                };
                self.endpoint.close_session(
                    old.peer.session_id,
                    close_code::REJECTED as u64,
                    b"reconnected",
                    now_ms,
                    &mut sink,
                );
                events.push(WorldPeerEvent::Left {
                    id: peer_id.clone(),
                });
            }

            *self.by_prefix.entry(prefix.clone()).or_insert(0) += 1;
            self.by_session.insert(sid, peer_id.clone());
            let mut udp_peer = UdpPeerState::new(
                peer_id.clone(),
                name.clone(),
                sid,
                address.clone(),
                path,
                self.state_bytes_per_second,
                now_ms,
            );
            udp_peer.metrics.record_rtt(rtt);
            let game_session = GameSessionState::new(peer_id.clone(), now_ms);
            self.peers.insert(
                peer_id.clone(),
                ConnectedPeer {
                    peer: udp_peer,
                    session: game_session,
                    last_edges: None,
                    superseded: 0,
                },
            );
            events.push(WorldPeerEvent::Joined {
                id: peer_id,
                name,
                address,
                prefix,
                path,
            });
        }

        // Update peer path on NAT rebinding (`session.on("path", ...)`).
        for (sid, new_addr) in paths {
            let path = self.path_to(new_addr);
            if let Some(peer_id) = self.by_session.get(&sid) {
                if let Some(connected) = self.peers.get_mut(peer_id) {
                    connected.peer.path = path;
                    connected.peer.address = new_addr.ip().to_string();
                }
            }
        }

        // Route incoming `state` and `events` lane messages through `UdpPeerState` and `GameSessionState`.
        for (sid, lane, text, payload) in messages {
            let Some(peer_id) = self.by_session.get(&sid).cloned() else {
                continue;
            };
            let mut close_req: Option<(u16, String)> = None;
            if let Some(connected) = self.peers.get_mut(&peer_id) {
                connected.peer.metrics.record_in(payload.len(), now_ms as u64);
                if let Some(ts) = self.endpoint.session_mut(sid) {
                    connected.peer.update_congestion(&mut ts.connection, now_ms);
                }
                if !text && lane == Lane::State {
                    // Reject any binary state payload that is not a valid `BNI1` input or `BNA1` ack keepalive.
                    if !valid_incoming_state_payload(&payload) {
                        connected.peer.state_invalid_drops += 1;
                        connected.peer.metrics.record_drop("state");
                        continue;
                    }
                    // Enforce the 180 packets/sec input rate limit (`GameSession.binary`).
                    if let Some(SessionAction::ForwardInput(bytes)) =
                        connected.session.on_state_datagram(&payload, now_ms)
                    {
                        events.push(WorldPeerEvent::Input {
                            id: peer_id.clone(),
                            bytes,
                        });
                    }
                } else if text && lane == Lane::Events {
                    // Enforce the 32 messages/sec control rate limit (`GameSession.control`).
                    let actions = connected.session.on_events_message(&payload, now_ms);
                    for action in actions {
                        match action {
                            SessionAction::SendEvents(reply) => {
                                if let Some(ts) = self.endpoint.session_mut(sid) {
                                    connected.peer.send_lane(
                                        &mut ts.connection,
                                        Lane::Events,
                                        true,
                                        reply.as_bytes(),
                                        None,
                                        Self::unix_ms(),
                                        now_ms,
                                        false,
                                    );
                                }
                            }
                            SessionAction::Close { code, reason } => {
                                close_req = Some((code, reason));
                                break;
                            }
                            SessionAction::ClientStart { call_id, args } => {
                                events.push(WorldPeerEvent::Start {
                                    id: peer_id.clone(),
                                    call_id,
                                    args,
                                });
                            }
                            SessionAction::ClientChat { text, team } => {
                                events.push(WorldPeerEvent::Chat {
                                    id: peer_id.clone(),
                                    text,
                                    team,
                                });
                            }
                            SessionAction::EdgeAck(message) => {
                                events.push(WorldPeerEvent::EdgeAck {
                                    id: peer_id.clone(),
                                    message,
                                });
                            }
                            SessionAction::ResyncState => {
                                events.push(WorldPeerEvent::Resync {
                                    id: peer_id.clone(),
                                });
                            }
                            SessionAction::ForwardInput(bytes) => {
                                events.push(WorldPeerEvent::Input {
                                    id: peer_id.clone(),
                                    bytes,
                                });
                            }
                        }
                    }
                }
            }
            if let Some((code, reason)) = close_req {
                self.close_peer(&peer_id, code, &reason, now_ms);
                events.push(WorldPeerEvent::Left { id: peer_id });
            }
        }

        // Poll session silence timeouts (`SESSION_TIMEOUT_MS`), application ping timers (`PING_INTERVAL_MS`),
        // and congestion recovery transitions (`wasCongested && !congested` -> `resync`).
        let session_ids: Vec<u32> = self.by_session.keys().copied().collect();
        for sid in session_ids {
            let Some(peer_id) = self.by_session.get(&sid).cloned() else {
                continue;
            };
            let mut close_req: Option<(u16, String)> = None;
            if let Some(connected) = self.peers.get_mut(&peer_id) {
                if let Some(ts) = self.endpoint.session_mut(sid) {
                    connected.peer.update_congestion(&mut ts.connection, now_ms);
                    if connected
                        .session
                        .check_congestion_transition(connected.peer.state_congested)
                    {
                        events.push(WorldPeerEvent::Resync {
                            id: peer_id.clone(),
                        });
                    }
                    for action in connected.session.poll_timer(now_ms) {
                        match action {
                            SessionAction::SendEvents(text) => {
                                connected.peer.send_lane(
                                    &mut ts.connection,
                                    Lane::Events,
                                    true,
                                    text.as_bytes(),
                                    None,
                                    Self::unix_ms(),
                                    now_ms,
                                    false,
                                );
                            }
                            SessionAction::Close { code, reason } => {
                                close_req = Some((code, reason));
                            }
                            _ => {}
                        }
                    }
                }
            }
            if let Some((code, reason)) = close_req {
                self.close_peer(&peer_id, code, &reason, now_ms);
                events.push(WorldPeerEvent::Left { id: peer_id });
            }
        }

        // Clean up transport sessions closed by the remote peer or transport idle timer, wherever
        // the transport noticed it, and any player whose transport session no longer exists.
        closed.append(&mut self.closed_outside_poll);
        closed.extend(
            self.by_session
                .keys()
                .copied()
                .filter(|sid| self.endpoint.session_mut(*sid).is_none()),
        );
        for sid in closed {
            if let Some(peer_id) = self.by_session.remove(&sid) {
                if let Some(removed) = self.peers.remove(&peer_id) {
                    self.dec_prefix(&removed.peer.prefix);
                    events.push(WorldPeerEvent::Left { id: peer_id });
                }
            }
        }

        self.flush();
        self.polls += 1;
        events
    }

    /// Decrements the active session count for `prefix` (`by_prefix`).
    fn dec_prefix(&mut self, prefix: &str) {
        if let Some(count) = self.by_prefix.get_mut(prefix) {
            if *count <= 1 {
                self.by_prefix.remove(prefix);
            } else {
                *count -= 1;
            }
        }
    }

    /// Serializes `payload` as JSON and queues it on `peer_id`'s reliable `events` lane
    /// (`peer.send("events", value)`).
    /// Queues a message that replaces the previous one of its kind (an input acknowledgement,
    /// the edges received so far): the next one carries the same news, so such a message is not
    /// queued while the player's reliable lane is backed up (a slow or lossy link then gets
    /// fewer of them instead of an ever longer queue), and an edge-results message equal to the
    /// one queued less than 100 ms ago is not queued again.
    pub fn send_superseding(&mut self, peer_id: &str, payload: &Value) -> bool {
        let now_ms = self.now_ms();
        let Some(connected) = self.peers.get_mut(peer_id) else {
            return false;
        };
        let backlog = self
            .endpoint
            .session_mut(connected.peer.session_id)
            .map_or(0, |s| s.connection.backlog());
        if backlog > SUPERSEDING_BACKLOG_BYTES {
            connected.superseded += 1;
            return false;
        }
        let Ok(bytes) = serde_json::to_vec(payload) else {
            return false;
        };
        if payload.get("type").and_then(Value::as_str) == Some("entity-edge-results") {
            if connected.last_edges.as_ref().is_some_and(|(last, at)| *last == bytes && now_ms - at < 100.) {
                connected.superseded += 1;
                return false;
            }
            connected.last_edges = Some((bytes.clone(), now_ms));
        }
        self.send_events_bytes(peer_id, &bytes)
    }

    pub fn send_events(&mut self, peer_id: &str, payload: &Value) -> bool {
        let Ok(bytes) = serde_json::to_vec(payload) else {
            return false;
        };
        self.send_events_bytes(peer_id, &bytes)
    }

    /// Queues pre-serialized JSON `bytes` on `peer_id`'s reliable `events` lane
    /// (`peer.sendEncoded("events", data)`).
    pub fn send_events_bytes(&mut self, peer_id: &str, bytes: &[u8]) -> bool {
        let now_ms = self.now_ms();
        let unix_ms = Self::unix_ms();
        let Some(connected) = self.peers.get_mut(peer_id) else {
            return false;
        };
        let sid = connected.peer.session_id;
        let Some(ts) = self.endpoint.session_mut(sid) else {
            return false;
        };
        connected
            .peer
            .send_lane(&mut ts.connection, Lane::Events, true, bytes, None, unix_ms, now_ms, false)
    }

    /// Queues a binary `BNS1` or `BNO1` snapshot datagram on `peer_id`'s unordered `state` lane
    /// (`GameSession.entityPacket`).
    /// Sends an unreliable sequenced `bni1` state snapshot (or baseline fragment) to `peer_id`
    /// on `Lane::State` (Channel 2) if its size is within `state_budget_bytes` (default 1200 B),
    /// porting `sendState(peerId, bytes)`.
    /// `own`: the player's own datagram, which congestion and staleness never hold back.
    pub fn send_state(
        &mut self,
        peer_id: &str,
        bytes: &[u8],
        captured_at_unix_ms: Option<u64>,
        own: bool,
    ) -> bool {
        let now_ms = self.now_ms();
        let unix_ms = Self::unix_ms();
        let Some(connected) = self.peers.get_mut(peer_id) else {
            return false;
        };
        let sid = connected.peer.session_id;
        let Some(ts) = self.endpoint.session_mut(sid) else {
            return false;
        };
        connected.peer.send_lane(
            &mut ts.connection,
            Lane::State,
            false,
            bytes,
            captured_at_unix_ms,
            unix_ms,
            now_ms,
            own,
        )
    }

    /// Serializes `payload` once and sends it to all connected peers on the `events` lane
    /// (`WorldRouter.broadcastEvents` and `match(value)`).
    /// The pending rotation as the `match` event a newly welcomed player needs, with the
    /// delay that is left of it.
    pub fn pending_match_event(&self) -> Option<Value> {
        let (query, deadline) = self.pending_match.as_ref()?;
        let left = (deadline - self.now_ms()).max(0.).round() as u64;
        Some(json!({ "type": "match", "query": query, "delayMs": left }))
    }

    pub fn broadcast_events(&mut self, payload: &Value) {
        let Ok(bytes) = serde_json::to_vec(payload) else {
            return;
        };
        let ids: Vec<String> = self.peers.keys().cloned().collect();
        for id in ids {
            self.send_events_bytes(&id, &bytes);
        }
    }

    /// Flushes all pending encrypted packets across all active sessions to the UDP socket
    /// (`UdpPeer.flush()`).
    pub fn flush(&mut self) {
        let now_ms = self.now_ms();
        let mut sink = SendOnlySink {
            socket: &self.socket,
            relays: &mut self.relays,
            closed: &mut self.closed_outside_poll,
            now_ms,
        };
        let sids: Vec<u32> = self.by_session.keys().copied().collect();
        for sid in sids {
            let _ = self.endpoint.flush_session(sid, now_ms, &mut sink);
        }
        self.endpoint.poll(now_ms, &mut sink);
    }

    /// Sends a `CLOSE` frame and removes `peer_id` from the server (`peer.close`).
    pub fn close_peer(&mut self, peer_id: &str, code: u16, reason: &str, now_ms: f64) {
        let Some(removed) = self.peers.remove(peer_id) else {
            return;
        };
        self.by_session.remove(&removed.peer.session_id);
        self.dec_prefix(&removed.peer.prefix);
        let mut sink = SendOnlySink {
            socket: &self.socket,
            relays: &mut self.relays,
            closed: &mut self.closed_outside_poll,
            now_ms,
        };
        self.endpoint.close_session(
            removed.peer.session_id,
            code as u64,
            reason.as_bytes(),
            now_ms,
            &mut sink,
        );
    }

    /// Closes all connected peers with `CLOSE_CODE.SHUTDOWN` (`clearBattle` / `close`), then
    /// releases every relay allocation.
    pub fn close_all(&mut self) {
        let now_ms = self.now_ms();
        let ids: Vec<String> = self.peers.keys().cloned().collect();
        for id in ids {
            self.close_peer(&id, close_code::SHUTDOWN as u16, "server stopping", now_ms);
        }
        let relays: Vec<u32> = self.relays.allocations.keys().copied().collect();
        for id in relays {
            self.relays.remove(id);
        }
    }

    /// The path descriptor for a peer address: relayed when it currently arrives through a relay.
    fn path_to(&self, address: SocketAddr) -> PeerPath {
        let mut path = PeerPath::direct(address);
        if self.relays.routes.contains_key(&normalize_socket_addr(address)) {
            path.kind = "relay";
        }
        path
    }

    /// Opens a TURN allocation for `peer_id` on the relay server `server` with the given
    /// credentials (the Node server's `allocateRelay`), permitting `permit`. Its outcome arrives
    /// as a [`RelayEvent`] from [`UdpGameServer::relay_events`].
    pub fn open_relay(
        &mut self,
        peer_id: &str,
        server: SocketAddr,
        username: &str,
        password: &str,
        permit: &[IpAddr],
    ) -> Result<(), String> {
        if self.relays.find(peer_id).is_some() {
            return Err("a relay is already open for this player".to_string());
        }
        let local = if server.is_ipv6() {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
        } else {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
        };
        let socket = crate::sockets::udp(local, server.is_ipv6())
            .map_err(|e| format!("relay socket: {e}"))?;
        socket
            .set_nonblocking(true)
            .map_err(|e| format!("relay socket: {e}"))?;
        let now_ms = self.now_ms();
        let mut turn = TurnClient::new(server, username, password);
        turn.allocate(now_ms);
        turn.permit(permit, now_ms);
        for packet in turn.transmit() {
            crate::sockets::send_to(&socket, &packet, server);
        }
        let id = self.relays.next_id;
        self.relays.next_id = self.relays.next_id.wrapping_add(1);
        self.relays.allocations.insert(
            id,
            RelayAllocation {
                peer_id: peer_id.to_string(),
                socket,
                turn,
            },
        );
        Ok(())
    }

    /// Permits more of a player's addresses on its relay (ports do not matter).
    pub fn permit_relay(&mut self, peer_id: &str, addresses: &[IpAddr]) {
        let now_ms = self.now_ms();
        if let Some(id) = self.relays.find(peer_id) {
            if let Some(relay) = self.relays.allocations.get_mut(&id) {
                relay.turn.permit(addresses, now_ms);
            }
        }
    }

    /// Releases a player's relay allocation, if it has one.
    pub fn close_relay(&mut self, peer_id: &str) {
        if let Some(id) = self.relays.find(peer_id) {
            self.relays.remove(id);
        }
    }

    /// The players that have a relay allocation open now, with its relayed address once allocated.
    pub fn relayed_players(&self) -> Vec<(String, Option<SocketAddr>)> {
        self.relays
            .allocations
            .values()
            .map(|a| (a.peer_id.clone(), a.turn.relayed()))
            .collect()
    }

    /// Relay allocations open now.
    pub fn relay_count(&self) -> usize {
        self.relays.allocations.len()
    }

    /// Relay outcomes since the last call.
    pub fn relay_events(&mut self) -> Vec<RelayEvent> {
        std::mem::take(&mut self.relays.events)
    }

    /// Returns per-player status and transport metrics for the operator `status` action
    /// (`diagnostics`).
    pub fn stats(&mut self) -> Vec<Value> {
        let now_ms = self.now_ms();
        let mut out = Vec::with_capacity(self.peers.len());
        for (id, connected) in &mut self.peers {
            let sid = connected.peer.session_id;
            if let Some(ts) = self.endpoint.session_mut(sid) {
                let net = connected.peer.stats(&mut ts.connection, now_ms);
                out.push(json!({
                    "id": id,
                    "name": connected.peer.name,
                    "address": connected.peer.address,
                    "rtt": connected.session.rtt,
                    "droppedInputs": connected.session.dropped_inputs,
                    "supersededEvents": connected.superseded,
                    "net": net,
                }));
            }
        }
        out
    }

    /// Returns endpoint-wide packet and rate-limit counters (`endpoint.stats()`).
    pub fn transport_metrics(&mut self, reset: bool) -> Value {
        let c = &mut self.endpoint.counters;
        let out = json!({
            "received": c.received,
            "sent": c.sent,
            "unknownSession": c.unknown_session,
            "strayReplies": c.stray_replies,
            "rateLimited": c.rate_limited,
            "malformed": c.malformed,
            "sendErrors": c.send_errors,
            "initiations": c.initiations,
            "sessionsActive": self.peers.len(),
        });
        if reset {
            *c = Default::default();
        }
        out
    }
}

/// Sanitizes the player name offered in the client handshake `hello` (`export function playerName(value)`
///), stripping control characters and bounding length, defaulting to `"Player"`.
fn clean_player_name(raw: Option<&str>) -> String {
    let text = raw.unwrap_or("Player");
    let cleaned: String = text
        .chars()
        .filter(|c| {
            let code = *c as u32;
            code > 0x1f && !(0x7f..=0x9f).contains(&code)
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(24)
        .collect();
    if cleaned.is_empty() {
        "Player".to_string()
    } else {
        cleaned
    }
}

/// Binds a dual-stack UDP socket on `[::]:port` (`IPV6_V6ONLY = 0`) with fallback to `0.0.0.0:port`
/// (`endpoint.listen({ port, host })`).
fn bind_dual_stack_udp(
    port: u16,
    bind_host: Option<&str>,
) -> Result<(UdpSocket, &'static str), String> {
    if let Some(host) = bind_host {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .or_else(|_| format!("[{host}]:{port}").parse())
            .map_err(|e| format!("invalid bind address {host}:{port}: {e}"))?;
        let family = if addr.is_ipv6() { "IPv6" } else { "IPv4" };
        let sock = crate::sockets::udp(addr, false)
            .map_err(|e| format!("failed to bind UDP {addr}: {e}"))?;
        return Ok((sock, family));
    }
    if let Ok(sock6) = crate::sockets::udp(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
        false,
    ) {
        return Ok((sock6, "IPv6"));
    }
    let sock4 = crate::sockets::udp(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port), false)
        .map_err(|e| format!("failed to bind UDP port {port}: {e}"))?;
    Ok((sock4, "IPv4"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_refusal_says_which_side_is_out_of_date() {
        let ours: u64 = network_contract::SIMULATION.trim_start_matches("bfe-sim-").parse().unwrap();
        let p = network_contract::PROTOCOL;
        let older = version_refusal(p, network_contract::PROFILE, &format!("bfe-sim-{}", ours - 1));
        assert!(older.starts_with("Your game is older than this server"), "{older}");
        assert!(older.contains(&format!("simulation {}, this server: {ours}", ours - 1)), "{older}");
        let newer = version_refusal(p, network_contract::PROFILE, &format!("bfe-sim-{}", ours + 1));
        assert!(newer.starts_with("This server runs an older version"), "{newer}");
        assert!(version_refusal(p - 1, "old", "").starts_with("Your game is older"));
        let data = version_refusal(p, network_contract::PROFILE, network_contract::SIMULATION);
        assert!(data.starts_with("Your game data differs"), "{data}");
        assert!(older.len() < 200 && newer.len() < 200);
    }
}
