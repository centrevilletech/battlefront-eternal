//! UDP game server listener, client authentication, per-prefix player caps, and session routing
//! (`src/udp-server.js` and `WorldRouter` in `src/game-session.js`).
//!
//! Why `UdpGameServer` owns one dual-stack `BFE-UDP/1` endpoint for LAN, JOIN IP, and online players:
//! In `src/udp-server.js` (`export async function startUdpServer` and `export function hostCandidates`),
//! a single native UDP endpoint on the game port (default `18451`) handles all player traffic:
//! 1. **Handshake hello & admission policy (`hello` and `admit` callbacks in `startUdpServer` in `src/udp-server.js`)**:
//!    - `on_hello`: returns `{ protocol, profile, simulation, content, fingerprint }` so the
//!      handshake `RESPONSE` fits in a single UDP datagram.
//!    - `on_admit`: verifies that the client's `hello` matches the active battle's `protocol` (`3`),
//!      `profile` (`"snapshot-delta-v1"`), `simulation` (`"bfe-sim-..."`), and `content` / `fingerprint`.
//!      Incompatible clients are rejected with `CLOSE_CODE.VERSION` (`"Incompatible game or content version."`).
//!      Banned identities (`isBanned(client.id)`) are rejected with `CLOSE_CODE.REJECTED`
//!      (`"You are banned from this server."`), and when `peers.size >= maxPlayers` the client is
//!      rejected with `"The server is full."`.
//! 2. **Per-prefix session cap (`maxPlayersPerAddress` / `maxSessionsPerPrefix` in `src/udp-server.js`)**:
//!    Because Ed25519 identities are free to generate, `ServerEndpoint` caps concurrent sessions
//!    from a single network prefix (an IPv4 `/32` or IPv6 `/64`, with loopback exempt) to
//!    `maxPlayersPerAddress` (default `8`, enough for a household or LAN party behind one NAT).
//! 3. **Immediate reconnect replacement (`admitSession` in `src/udp-server.js`)**:
//!    When an already-connected identity completes a new handshake (`peers.get(session.id)`), the
//!    stale session is closed immediately (`"reconnected"` / `"replaced"`) and replaced by the new
//!    `UdpPeerState` and `GameSessionState`.
//! 4. **STUN reflexive discovery and hole-punching (`reflexive` and `punch` in `src/udp-server.js`)**:
//!    Uses the game port socket itself to send RFC 5389 STUN Binding Requests (`0x0001`) so the
//!    NAT mapping discovered by `OnlineServer.discover` is the exact mapping players will send
//!    `BFE-UDP/1` handshakes to, and sends `0xb8` punch datagrams only to validated `valid_peer_ip`
//!    addresses.

use crate::game_session::{GameSessionState, SessionAction};
use crate::udp_peer::{valid_incoming_state_payload, PeerPath, UdpPeerState};
use bfe_transport::connection::{Closed, Lane};
use bfe_transport::crypto::{Identity, RetryTokens};
use bfe_transport::layout::{close_code, network_contract};
use bfe_transport::server::{
    address_prefix, valid_peer_ip, AdmitDecision, ClientInfo, ServerEndpoint, ServerSink,
};
use bfe_transport::stun;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum UDP datagram buffer length read on the game port (`1500` bytes Ethernet MTU).
/// Any `BFE-UDP/1` datagram exceeding `PACKET_MAX_BYTES` (`1200` bytes) is rejected before parsing
/// (`src/udp-server.js` and `crates/bfe-transport/src/server.rs`).
const MAX_PACKET: usize = 1500;

/// Signaling candidate descriptor (`hostCandidates` in `src/udp-server.js` and `parseCandidates` in `src/online-server.js`).
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

/// Discovered STUN reflexive addresses per IP family (`this.reflexive` in `src/online-server.js`).
///
/// Tracks IPv4 and IPv6 public NAT mappings independently so dual-stack servers advertise both
/// `srflx` candidates to the master coordinator (`candidates()` in `src/online-server.js`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflexiveMap {
    /// Public IPv4 `srflx` mapping discovered via STUN (`0x0001`), if reachable.
    pub ipv4: Option<CandidateEntry>,
    /// Public IPv6 `srflx` mapping discovered via STUN (`0x0001`), if reachable.
    pub ipv6: Option<CandidateEntry>,
}

/// Paired transport peer state (`UdpPeer` in `src/udp-peer.js`) and game session state
/// (`GameSession` in `src/game-session.js`) for one connected player.
///
/// Keeping `UdpPeerState` and `GameSessionState` paired by 64-hex Ed25519 identity mirrors
/// `peers.set(id, peer)` + `sessions.set(id, session)` in `src/udp-server.js` and `src/game-session.js`.
pub struct ConnectedPeer {
    /// Per-peer `BFE-UDP/1` token bucket, congestion watermark, and `NetMetrics` tracker (`src/udp-peer.js`).
    pub peer: UdpPeerState,
    /// Per-peer input/control rate limiters, ping/pong RTT tracker, and 15 s silence timer (`src/game-session.js`).
    pub session: GameSessionState,
}

/// Events emitted by [`UdpGameServer::poll`] for the world loop (`WorldRouter` in `src/game-session.js`).
#[derive(Clone, Debug)]
pub enum WorldPeerEvent {
    /// A client completed the `BFE-UDP/1` handshake and was admitted (`admitSession` in `src/udp-server.js`).
    Joined {
        id: String,
        name: String,
        address: String,
        prefix: String,
        path: PeerPath,
    },
    /// A client session closed or was evicted (`peer.once("closed", ...)` in `src/udp-server.js`).
    Left {
        id: String,
    },
    /// A client called the `start` RPC (`m.method === "start"` in `src/game-session.js`).
    Start {
        id: String,
        call_id: Option<Value>,
        args: Value,
    },
    /// A client submitted a chat message (`m.type === "chat"` in `src/game-session.js`).
    Chat {
        id: String,
        text: String,
        team: bool,
    },
    /// A client acknowledged reliable entity-edge results (`entity-edge-ack` in `src/game-session.js`).
    EdgeAck {
        id: String,
        message: Value,
    },
    /// A client sent a validated, rate-limited `BNI1` or `BNA1` binary datagram (`GameSession.binary` in `src/game-session.js`).
    Input {
        id: String,
        bytes: Vec<u8>,
    },
    /// A client requested or requires a snapshot delta baseline reset (`resync` in `src/game-session.js`).
    Resync {
        id: String,
    },
}

/// `ServerSink` implementation used when flushing outbound packets or closing a session outside `poll()`.
///
/// Only `send_packet` is active during `flush()` or `close_peer()`; admission and message callbacks
/// are no-ops because no inbound datagrams are fed through `SendOnlySink`.
struct SendOnlySink<'a>(&'a UdpSocket);

impl ServerSink for SendOnlySink<'_> {
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
        crate::sockets::send_to(self.0, bytes, to);
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

    fn on_close(&mut self, _session_id: u32, _closed: Closed) {}
}

/// `ServerSink` implementation used during [`UdpGameServer::poll`] to collect newly admitted
/// sessions, incoming lane messages, path migrations, and closed sessions from `ServerEndpoint`.
///
/// Why callbacks buffer events into vectors (`admitted`, `messages`, `paths`, `closed`):
/// `ServerEndpoint::receive` and `ServerEndpoint::poll` borrow `self.endpoint` mutably while
/// invoking `ServerSink`. Buffering the callbacks allows `UdpGameServer::poll` to subsequently
/// mutate `self.peers`, `self.by_session`, and `self.endpoint` without borrow conflicts.
struct PollSink<'a, F: FnMut(&str) -> bool> {
    socket: &'a UdpSocket,
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

impl<F: FnMut(&str) -> bool> ServerSink for PollSink<'_, F> {
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
        crate::sockets::send_to(self.socket, bytes, to);
    }

    /// Builds the server handshake `hello` payload (`server.hello` in `startUdpServer` in `src/udp-server.js`).
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

    /// Evaluates handshake admission policy (`server.admit` in `startUdpServer` in `src/udp-server.js`):
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
            return AdmitDecision::reject(
                close_code::VERSION as u64,
                "Incompatible game or content version.",
            );
        }
        // Reject locally or globally banned Ed25519 player identities (`isBanned(client.id)` in `src/udp-server.js`).
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
}

/// Authoritative UDP game port server (`export async function startUdpServer` in `src/udp-server.js`).
/// Authoritative UDP game server porting `createUdpServer(options)` from
/// `packages/bfe-server/src/udp-server.js`.
///
/// Transport and security invariants (`udp-server.js` + `udp-peer.js`):
/// - Wraps `bfe_transport::Server` (`UdpEndpoint` server mode in `packages/bfe-net`) on a
///   non-blocking dual-stack UDP socket (`bind_dual_stack_udp`).
/// - Enforces `MAX_PEERS_PER_PREFIX` (`4` concurrent sessions per IPv4 `/24` or IPv6 `/48` prefix
///   via `ip_prefix_key` in `udp-peer.js`) inside `on_admit` so a single subnet cannot exhaust all
///   player slots (`max_players`).
/// - Rejects banned client Ed25519 fingerprints during the handshake (`AdmitDecision::Reject`)
///   before allocating session state or spawning world entities.
/// - Applies per-peer `PeerBuckets` token-bucket rate limits (`udp-peer.js`):
///   - `input`: 120 Hz sustained, 240 packet burst (Channel 1 `entity-input` + `snapshot-ack`)
///   - `events`: 30 Hz sustained, 60 message burst (Channel 0 reliable gameplay JSON)
///   - `bytes`: 64 KiB/s sustained, 128 KiB burst across all inbound lanes
///
///   Exceeding `MAX_VIOLATIONS` (`24`) disconnects the offending peer with reason `"rate-limit"`.
/// - Enforces `PEER_IDLE_TIMEOUT_MS` (`15_000` ms) so silent peers are reaped and their prefix
///   and player slots are reclaimed.
pub struct UdpGameServer {
    /// Non-blocking dual-stack UDP socket bound to the game port (`18451`).
    socket: UdpSocket,
    /// Actual bound UDP port (`bound.port` in `src/udp-server.js`).
    pub port: u16,
    /// Bound socket address family (`"IPv6"` dual-stack or `"IPv4"` fallback).
    pub family: &'static str,
    /// Underlying `BFE-UDP/1` transport server endpoint (`bfe_transport::server::ServerEndpoint`).
    pub endpoint: ServerEndpoint,
    /// Connected players keyed by 64-hex Ed25519 identity (`peers` map in `src/udp-server.js`).
    pub peers: HashMap<String, ConnectedPeer>,
    /// Reverse lookup from `ServerEndpoint` session ID (`u32`) to 64-hex player identity.
    pub by_session: HashMap<u32, String>,
    /// Active session count per IPv4 `/32` or IPv6 `/64` prefix (`byPrefix` in `src/udp-server.js`).
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
    /// Monotonic start instant for `now_ms()` (`performance.now()` in `src/game-session.js`).
    start_instant: Instant,
    /// Reusable 1500-byte UDP receive buffer.
    recv_buf: [u8; MAX_PACKET],
}

impl UdpGameServer {
    /// Binds the UDP game server socket (`startUdpServer` in `src/udp-server.js` and `attachNetwork` in `src/main.js`).
    ///
    /// Computes `maxSessions = (maxPlayers * 2 + 32).max(32)` matching
    /// `Math.max(32, (config.maxPlayers ?? 64) * 2 + 32)` in `src/main.js` so transport session
    /// slots have headroom for concurrent reconnects and handshakes while `maxPlayers` caps admitted
    /// players.
    #[allow(clippy::too_many_arguments)]
    /// Binds a non-blocking dual-stack UDP socket (`[::]:port` with `IPV6_V6ONLY = 0`, falling back
    /// to `0.0.0.0:port` when IPv6 is disabled on the host) and initializes `bfe_transport::Server`,
    /// porting `createUdpServer(options)` in `packages/bfe-server/src/udp-server.js`.
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
            start_instant: Instant::now(),
            recv_buf: [0u8; MAX_PACKET],
        })
    }

    /// Returns the bound UDP game port (`bound.port` in `src/udp-server.js`).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the list of currently connected 64-hex player identities (`players: () => [...peers.keys()]`
    /// in `src/udp-server.js`).
    pub fn players(&self) -> Vec<String> {
        self.peers.keys().cloned().collect()
    }

    /// Returns direct host candidates for this socket (`export function hostCandidates` in `src/udp-server.js`).
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

    /// Performs an RFC 5389 STUN Binding Request over the game port socket to discover the server's
    /// public reflexive address (`endpoint.reflexive` in `src/udp-server.js`).
    ///
    /// Why STUN runs on `self.socket` directly:
    /// Symmetric and port-restricted cone NATs assign an external `ip:port` mapping per local UDP
    /// socket. Sending the STUN Binding Request from `self.socket` discovers the exact external
    /// mapping that remote clients will reach when hole-punching to the game port.
    /// Sends a RFC 5389 STUN Binding Request directly from the live game UDP socket to `server`
    /// and waits up to `timeout` for the `XOR-MAPPED-ADDRESS` / `MAPPED-ADDRESS` response, porting
    /// `udp.reflexive(stunServer, timeoutMs)` in `packages/bfe-server/src/udp-server.js`.
    ///
    /// Why STUN must be sent from the game UDP socket itself rather than an ephemeral socket:
    /// NAT bindings are keyed by the local `(ip, port)` tuple; querying STUN from the actual game
    /// socket reveals the exact public `srflx` port that remote clients must hole-punch toward.
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

    /// Monotonic milliseconds elapsed since server bind (`performance.now()` in `src/game-session.js`).
    pub fn now_ms(&self) -> f64 {
        self.start_instant.elapsed().as_secs_f64() * 1000.0
    }

    /// Current Unix epoch time in milliseconds (`Date.now()` in `src/game-session.js`).
    pub fn unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Updates the active content fingerprint on map rotation (`setBattle` in `src/udp-server.js`).
    pub fn set_fingerprint(&mut self, fingerprint: String) {
        self.fingerprint = fingerprint;
    }

    /// Schedules UDP hole-punch datagrams (`0xb8`, `endpoint.punch` in `src/udp-server.js`).
    ///
    /// Rejects any candidate IP that fails `valid_peer_ip` so the server never punches unspecified
    /// or multicast addresses.
    /// Sends low-TTL / empty hole-punch datagrams from the game UDP socket toward each client
    /// `host` and `srflx` candidate (`MAX_PUNCH_TARGETS = 8`), porting `udp.punch(candidates)` in
    /// `packages/bfe-server/src/udp-server.js`.
    ///
    /// Opens outbound state-table mappings on the server's local firewall/NAT so the client's
    /// incoming `bfe_transport` ClientHello can traverse address-restricted cone NATs.
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

    /// Closes a player's session with `CLOSE_CODE.KICKED` (`kick(id, message)` in `src/udp-server.js`).
    pub fn kick(&mut self, peer_id: &str, reason: &str) {
        let now_ms = self.now_ms();
        self.close_peer(peer_id, close_code::KICKED as u16, reason, now_ms);
    }

    /// Drains pending UDP datagrams from the non-blocking socket, advances `ServerEndpoint`
    /// handshakes and sessions, runs `GameSessionState` input/control/liveness logic, and returns
    /// [`WorldPeerEvent`] items for the world tick thread (`startUdpServer` + `WorldRouter` in
    /// `src/udp-server.js` and `src/game-session.js`).
    /// Drains all available UDP datagrams from the socket, advances the `bfe_transport::Server`
    /// handshake/session state machine, enforces per-peer rate limits and idle timeouts, and
    /// returns the ordered list of `WorldPeerEvent`s for `NativeWorldHost`, porting the socket
    /// receive + timer callbacks in `packages/bfe-server/src/udp-server.js`.
    ///
    /// Wire framing handled per lane (`udp-server.js` `onMessage`):
    /// - `Lane::Control` / `Lane::Events` (`text == true`): UTF-8 JSON envelope (`{ type: "events", events: [...] }`
    ///   or `{ type: "hello" }`), validated against `PeerBuckets.events` and `PeerBuckets.bytes`.
    /// - `Lane::Events` (`text == false`): binary reliable messages on Channel 0.
    /// - `Lane::Input` (`0x01` `entity-input`): 46-byte `BFEI` v2 player command frame decoded via
    ///   `decode_entity_input` (`world-worker.js`) and rate-limited by `PeerBuckets.input`.
    /// - `Lane::Input` (`0x02` `snapshot-ack` / `0x03` `keepalive`): 9-byte client baseline ack
    ///   decoded via `decode_snapshot_ack_or_keepalive` (`world-worker.js`) so the server can
    ///   delta-compress subsequent `bni1` state snapshots against `ack_seq`.
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

            // Advance transport timers (handshake expirations, punch bursts, keepalives, retransmit timers).
            self.endpoint.poll(now_ms, &mut sink);

            admitted.append(&mut sink.admitted);
            messages.append(&mut sink.messages);
            paths.append(&mut sink.paths);
            closed.append(&mut sink.closed);
        }

        // Admit newly established sessions (`admitSession` in `src/udp-server.js`).
        for (sid, client, remote, rtt) in admitted {
            let peer_id = client.id_hex.clone();
            let name = clean_player_name(client.hello.get("name").and_then(Value::as_str));
            let path = PeerPath::direct(remote);
            let address = remote.ip().to_string();
            let prefix = address_prefix(remote.ip());

            // A reconnect replaces a stale session of the same identity at once (`peers.get(session.id)?.close("replaced")`).
            if let Some(old) = self.peers.remove(&peer_id) {
                self.by_session.remove(&old.peer.session_id);
                self.dec_prefix(&old.peer.prefix);
                let mut sink = SendOnlySink(&self.socket);
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

        // Update peer path on NAT rebinding (`session.on("path", ...)` in `src/udp-peer.js`).
        for (sid, new_addr) in paths {
            if let Some(peer_id) = self.by_session.get(&sid) {
                if let Some(connected) = self.peers.get_mut(peer_id) {
                    connected.peer.path = PeerPath::direct(new_addr);
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
                    // Enforce the 180 packets/sec input rate limit (`GameSession.binary` in `src/game-session.js`).
                    if let Some(SessionAction::ForwardInput(bytes)) =
                        connected.session.on_state_datagram(&payload, now_ms)
                    {
                        events.push(WorldPeerEvent::Input {
                            id: peer_id.clone(),
                            bytes,
                        });
                    }
                } else if text && lane == Lane::Events {
                    // Enforce the 32 messages/sec control rate limit (`GameSession.control` in `src/game-session.js`).
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

        // Clean up transport sessions closed by the remote peer or transport idle timer.
        for sid in closed {
            if let Some(peer_id) = self.by_session.remove(&sid) {
                if let Some(removed) = self.peers.remove(&peer_id) {
                    self.dec_prefix(&removed.peer.prefix);
                    events.push(WorldPeerEvent::Left { id: peer_id });
                }
            }
        }

        self.flush();
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
    /// (`peer.send("events", value)` in `src/udp-peer.js`).
    pub fn send_events(&mut self, peer_id: &str, payload: &Value) -> bool {
        let Ok(bytes) = serde_json::to_vec(payload) else {
            return false;
        };
        self.send_events_bytes(peer_id, &bytes)
    }

    /// Queues pre-serialized JSON `bytes` on `peer_id`'s reliable `events` lane
    /// (`peer.sendEncoded("events", data)` in `src/udp-peer.js`).
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
            .send_lane(&mut ts.connection, Lane::Events, true, bytes, None, unix_ms, now_ms)
    }

    /// Queues a binary `BNS1` or `BNO1` snapshot datagram on `peer_id`'s unordered `state` lane
    /// (`GameSession.entityPacket` in `src/game-session.js`).
    /// Sends an unreliable sequenced `bni1` state snapshot (or baseline fragment) to `peer_id`
    /// on `Lane::State` (Channel 2) if its size is within `state_budget_bytes` (default 1200 B),
    /// porting `sendState(peerId, bytes)` in `packages/bfe-server/src/udp-server.js`.
    pub fn send_state(
        &mut self,
        peer_id: &str,
        bytes: &[u8],
        captured_at_unix_ms: Option<u64>,
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
        )
    }

    /// Serializes `payload` once and sends it to all connected peers on the `events` lane
    /// (`WorldRouter.broadcastEvents` in `src/game-session.js` and `match(value)` in `src/udp-server.js`).
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
    /// (`UdpPeer.flush()` in `src/udp-peer.js`).
    pub fn flush(&mut self) {
        let now_ms = self.now_ms();
        let mut sink = SendOnlySink(&self.socket);
        let sids: Vec<u32> = self.by_session.keys().copied().collect();
        for sid in sids {
            let _ = self.endpoint.flush_session(sid, now_ms, &mut sink);
        }
        self.endpoint.poll(now_ms, &mut sink);
    }

    /// Sends a `CLOSE` frame and removes `peer_id` from the server (`peer.close` in `src/udp-peer.js`).
    pub fn close_peer(&mut self, peer_id: &str, code: u16, reason: &str, now_ms: f64) {
        let Some(removed) = self.peers.remove(peer_id) else {
            return;
        };
        self.by_session.remove(&removed.peer.session_id);
        self.dec_prefix(&removed.peer.prefix);
        let mut sink = SendOnlySink(&self.socket);
        self.endpoint.close_session(
            removed.peer.session_id,
            code as u64,
            reason.as_bytes(),
            now_ms,
            &mut sink,
        );
    }

    /// Closes all connected peers with `CLOSE_CODE.SHUTDOWN` (`clearBattle` / `close` in `src/udp-server.js`).
    pub fn close_all(&mut self) {
        let now_ms = self.now_ms();
        let ids: Vec<String> = self.peers.keys().cloned().collect();
        for id in ids {
            self.close_peer(&id, close_code::SHUTDOWN as u16, "server stopping", now_ms);
        }
    }

    /// Returns per-player status and transport metrics for the operator `status` action
    /// (`diagnostics` in `src/udp-server.js`).
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
                    "net": net,
                }));
            }
        }
        out
    }

    /// Returns endpoint-wide packet and rate-limit counters (`endpoint.stats()` in `src/udp-server.js`).
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
/// in `src/game-session.js`), stripping control characters and bounding length, defaulting to `"Player"`.
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
/// (`endpoint.listen({ port, host })` in `src/udp-server.js`).
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
