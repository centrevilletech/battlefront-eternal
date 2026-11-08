//! Server-side peer wrapper over `bfe_transport` (`src/udp-peer.js` and `src/game-session.js`).
//!
//! Why `UdpPeerState` wraps each `bfe_transport::connection::Connection`:
//! In `src/udp-peer.js` (`export class UdpPeer`) and `src/game-session.js` (`export class GameSession`),
//! the transport layer owns packet encryption, sequence numbers, and ACKs, while the server peer
//! layer enforces gameplay-specific framing, message size bounds, congestion hysteresis, and
//! per-client bandwidth budgets:
//! 1. **Wire framing & magic validation (`acceptStatePayload` in `src/udp-peer.js` and `binary()` in `src/game-session.js`)**:
//!    Binary payloads on the `state` lane from a client must fit in a single MTU datagram
//!    (`10..=1100` bytes) and start with a little-endian 32-bit magic:
//!    - `0x31494e42` (`'BNI1'`, `INPUT_MAGIC`): client entity input datagram (`>= 16` bytes).
//!    - `0x31414e42` (`'BNA1'`, `ACK_MAGIC`): 10-byte client snapshot-ack keepalive (`=== 10` bytes).
//!
//!    Outgoing server `state` datagrams (`entityPacket` in `src/game-session.js`) carry either:
//!    - `0x31534e42` (`'BNS1'`, `SNAPSHOT_MAGIC`): shared entity delta snapshot part, or
//!    - `0x314f4e42` (`'BNO1'`, `OWN_MAGIC`): per-client own-player correction datagram.
//! 2. **Lane size limits (`EVENTS_MESSAGE_BYTES` and `STATE_MESSAGE_BYTES` in `src/udp-peer.js`)**:
//!    - `EVENTS_MESSAGE_BYTES = 262_144` (256 KiB): the reliable `events` lane must carry the
//!      battle's `entity-registry` descriptor and `welcome` at join time, plus split batches of
//!      `gameplay-events`. If a peer's unacknowledged `events` backlog exceeds `EVENTS_MESSAGE_BYTES`,
//!      the peer is closed (`close_code::PROTOCOL`).
//!    - `STATE_MESSAGE_BYTES = 32_768` (32 KiB): bound on JSON messages (`input-ack`,
//!      `entity-edge-results`) on the unordered `state` lane.
//! 3. **Congestion hysteresis (`CONGESTED_MS = 120`, `CLEAR_MS = 50` in `src/udp-peer.js`, and
//!    `STATE_BACKLOG_BYTES = 32_768`, `STATE_STALE_MS = 100` in `src/game-session.js`)**:
//!    When a client's unacknowledged queue or path delay shows sustained queueing (`CONGESTED_MS = 120 ms`
//!    above `STATE_BACKLOG_BYTES = 32 KiB`), `state_congested` transitions to `true` and non-essential
//!    state datagrams are skipped until the backlog stays clear for `CLEAR_MS = 50 ms`, preventing
//!    bufferbloat oscillation. Snapshots older than `STATE_STALE_MS = 100 ms` (`Date.now() - capturedAtUnixMs > 100`)
//!    are dropped before transmission so stale state never queues behind fresh state.
//! 4. **Per-peer outgoing state token bucket (`StateBudget`, `stateBytesPerSecond` in `src/world-worker.js`)**:
//!    Caps outgoing `state` datagram bytes per client (`rate = stateBytesPerSecond`,
//!    `burst = max(4096, rate * 0.25)`) so a 64-player battle stays within the operator's bandwidth budget.

use crate::net_metrics::NetMetrics;
use bfe_transport::connection::{Connection, Lane};
use bfe_transport::layout::close_code;
use bfe_transport::server::address_prefix;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;

/// Little-endian u32 magic `'BNI1'` (`0x31494e42`) for client entity-input datagrams
/// (`STATE_MAGICS` in `src/udp-peer.js` and `binary()` in `src/game-session.js`).
pub const INPUT_MAGIC: u32 = 0x3149_4e42; // 'BNI1'
/// Little-endian u32 magic `'BNA1'` (`0x31414e42`) for 10-byte client snapshot-ack keepalive datagrams
/// (`STATE_MAGICS` in `src/udp-peer.js` and `binary()` in `src/game-session.js`).
pub const ACK_MAGIC: u32 = 0x3141_4e42; // 'BNA1'
/// Little-endian u32 magic `'BNS1'` (`0x31534e42`) for server shared entity-snapshot delta datagrams
/// (`entityPacket` in `src/game-session.js`).
pub const SNAPSHOT_MAGIC: u32 = 0x3153_4e42; // 'BNS1'
/// Little-endian u32 magic `'BNO1'` (`0x314f4e42`) for server own-player correction datagrams
/// (`entityPacket` in `src/game-session.js`).
pub const OWN_MAGIC: u32 = 0x314f_4e42; // 'BNO1'

/// Maximum reliable `events` message size and unacknowledged buffer cap (`EVENTS_MESSAGE_BYTES = 262144` in `src/udp-peer.js`).
pub const EVENTS_MESSAGE_BYTES: usize = 262_144;
/// Maximum `state` lane JSON message size (`STATE_MESSAGE_BYTES = 32768` in `src/udp-peer.js`).
pub const STATE_MESSAGE_BYTES: usize = 32_768;

/// Congestion entry threshold in milliseconds (`CONGESTED_MS = 120` in `src/udp-peer.js`).
const CONGESTED_MS: f64 = 120.0;
/// Congestion recovery hysteresis threshold in milliseconds (`CLEAR_MS = 50` in `src/udp-peer.js`).
const CLEAR_MS: f64 = 50.0;
/// Unacknowledged byte backlog above which snapshots are skipped (`STATE_BACKLOG_BYTES = 32768` in `src/game-session.js`).
const STATE_BACKLOG_BYTES: usize = 32_768;
/// Maximum age in milliseconds for an outgoing snapshot publication (`STATE_STALE_MS = 100` in `src/game-session.js`).
const STATE_STALE_MS: u64 = 100;

/// Selected transport path metadata for a connected peer (`UdpPeer.path()` in `src/udp-peer.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerPath {
    /// Path kind (`"direct"` or `"relay"`).
    pub kind: &'static str,
    /// Address family (`"IPv4"` or `"IPv6"`).
    pub family: &'static str,
    /// Remote socket address on the active path.
    pub address: SocketAddr,
}

impl PeerPath {
    /// Constructs a direct UDP path descriptor for `address` (`UdpPeer.path()` in `src/udp-peer.js`).
    pub fn direct(address: SocketAddr) -> Self {
        let family = if address.is_ipv6() { "IPv6" } else { "IPv4" };
        Self {
            kind: "direct",
            family,
            address,
        }
    }
}

/// Per-peer outgoing `state` lane token bucket (`stateBytesPerSecond` budget in `src/world-worker.js`).
///
/// Refills continuously at `rate` bytes/sec up to `burst = max(4096, rate * 0.25)` bytes.
#[derive(Clone, Debug)]
pub struct StateBudget {
    pub rate: f64,
    pub burst: f64,
    pub tokens: f64,
    pub at: f64,
    pub offered_bytes: u64,
    pub denied: u64,
}

impl StateBudget {
    /// Initializes a token bucket with `bytes_per_second` refill rate and a 250 ms burst window
    /// (at least 4096 bytes so a full-baseline multi-part snapshot is never starved on an idle link).
    pub fn new(bytes_per_second: usize, now_ms: f64) -> Self {
        let rate = bytes_per_second as f64;
        let burst = (rate * 0.25).max(4096.0);
        Self {
            rate,
            burst,
            tokens: burst,
            at: now_ms,
            offered_bytes: 0,
            denied: 0,
        }
    }

    /// Refills the bucket to `now_ms` and returns the currently available byte tokens.
    pub fn available(&mut self, now_ms: f64) -> f64 {
        let elapsed = ((now_ms - self.at).max(0.0)) / 1000.0;
        self.at = now_ms;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        self.tokens
    }

    /// Attempts to spend `bytes` (`1..=1100` MTU bound) from the token bucket at `now_ms`.
    pub fn take(&mut self, bytes: usize, now_ms: f64) -> bool {
        if !(1..=1100).contains(&bytes) {
            return false;
        }
        self.offered_bytes += bytes as u64;
        if self.available(now_ms) < bytes as f64 {
            self.denied += 1;
            return false;
        }
        self.tokens -= bytes as f64;
        true
    }
}

/// Validates incoming client `state` datagram magic and length bounds
/// (`receive("state", payload)` in `src/udp-peer.js` and `GameSession.binary` in `src/game-session.js`).
///
/// Checks `10 <= bytes.len() <= 1100` and requires either:
/// - `ACK_MAGIC` (`0x31414e42`, `'BNA1'`) with exact length `10`, or
/// - `INPUT_MAGIC` (`0x31494e42`, `'BNI1'`) with length `>= 16`.
pub fn valid_incoming_state_payload(bytes: &[u8]) -> bool {
    if bytes.len() < 10 || bytes.len() > 1100 {
        return false;
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    match magic {
        ACK_MAGIC => bytes.len() == 10,
        INPUT_MAGIC => bytes.len() >= 16,
        _ => false,
    }
}

/// Per-session server transport state (`export class UdpPeer` in `src/udp-peer.js`).
pub struct UdpPeerState {
    /// 64-character lowercase hex SHA-256 client identity (`session.id` in `src/udp-peer.js`).
    pub id: String,
    /// Sanitized player display name from the handshake hello (`playerName` in `src/game-session.js`).
    pub name: String,
    /// Local numeric session handle inside `bfe_transport::server::ServerEndpoint`.
    pub session_id: u32,
    /// Normalized remote IP string for operator status display.
    pub address: String,
    /// Network prefix key (IPv4 `/32` or IPv6 `/64`) for `maxPlayersPerAddress` accounting (`src/udp-server.js`).
    pub prefix: String,
    /// Active network path (`direct` or `relay`, `IPv4` or `IPv6`).
    pub path: PeerPath,
    /// Outgoing `state` lane bandwidth token bucket.
    pub state_budget: StateBudget,
    /// Sliding-window and lifetime byte/packet/drop metrics (`src/net-metrics.js`).
    pub metrics: NetMetrics,
    /// Timestamp (`now_ms`) when the connection backlog first exceeded `STATE_BACKLOG_BYTES`.
    pub congested_since: Option<f64>,
    /// Timestamp (`now_ms`) when a congested connection's backlog dropped back below `STATE_BACKLOG_BYTES`.
    pub clear_since: Option<f64>,
    /// Whether the peer is currently in the congested hysteresis state (`this.congested` in `src/udp-peer.js`).
    pub state_congested: bool,
    /// Count of outgoing snapshots dropped because `unix_now_ms - captured_at_unix_ms > STATE_STALE_MS`.
    pub state_stale_drops: u64,
    /// Count of outgoing snapshots dropped because `state_budget` had insufficient tokens.
    pub state_budget_drops: u64,
    /// Count of incoming `state` datagrams rejected by [`valid_incoming_state_payload`].
    pub state_invalid_drops: u64,
    /// Last RTT sample recorded from `Connection.rtt`.
    pub last_rtt: Option<f64>,
}

impl UdpPeerState {
    /// Constructs a new `UdpPeerState` for an admitted `BFE-UDP/1` session (`new UdpPeer` in `src/udp-server.js`).
    pub fn new(
        id: String,
        name: String,
        session_id: u32,
        address: String,
        path: PeerPath,
        state_bytes_per_second: usize,
        now_ms: f64,
    ) -> Self {
        let prefix = address_prefix(path.address.ip());
        Self {
            id,
            name,
            session_id,
            address,
            prefix,
            path,
            state_budget: StateBudget::new(state_bytes_per_second, now_ms),
            metrics: NetMetrics::new(),
            congested_since: None,
            clear_since: None,
            state_congested: false,
            state_stale_drops: 0,
            state_budget_drops: 0,
            state_invalid_drops: 0,
            last_rtt: None,
        }
    }

    /// Updates RTT/loss metrics and evaluates the `CONGESTED_MS` (120 ms) / `CLEAR_MS` (50 ms)
    /// congestion hysteresis state machine (`UdpPeer.stateBacklog` in `src/udp-peer.js`).
    pub fn update_congestion(&mut self, conn: &mut Connection, now_ms: f64) {
        if conn.rtt.is_finite() && self.last_rtt != Some(conn.rtt) {
            self.last_rtt = Some(conn.rtt);
            self.metrics.record_rtt(conn.rtt);
        }
        self.metrics.record_loss(conn.recent_loss(now_ms));
        let backlog = conn.backlog();
        if backlog > STATE_BACKLOG_BYTES {
            self.clear_since = None;
            let since = *self.congested_since.get_or_insert(now_ms);
            if now_ms - since >= CONGESTED_MS {
                self.state_congested = true;
            }
        } else {
            self.congested_since = None;
            if self.state_congested {
                let since = *self.clear_since.get_or_insert(now_ms);
                if now_ms - since >= CLEAR_MS {
                    self.state_congested = false;
                    self.clear_since = None;
                }
            }
        }
    }

    /// Queues a message or binary datagram on `lane` (`UdpPeer.sendEncoded`, `UdpPeer.sendBinary`,
    /// and `GameSession.entityPacket` in `src/udp-peer.js` and `src/game-session.js`).
    ///
    /// Enforces:
    /// - `EVENTS_MESSAGE_BYTES` (256 KiB) and `STATE_MESSAGE_BYTES` (32 KiB) payload limits.
    /// - `STATE_STALE_MS` (100 ms) staleness drop on `Lane::State`.
    /// - Congestion hysteresis skip (`state_congested`) and `StateBudget` token bucket on `Lane::State`.
    /// - Connection close (`close_code::PROTOCOL`) if the reliable `Lane::Events` backlog exceeds `EVENTS_MESSAGE_BYTES`.
    #[allow(clippy::too_many_arguments)]
    pub fn send_lane(
        &mut self,
        conn: &mut Connection,
        lane: Lane,
        text: bool,
        payload: &[u8],
        captured_at_unix_ms: Option<u64>,
        unix_now_ms: u64,
        now_ms: f64,
    ) -> bool {
        let lane_name = match lane {
            Lane::State => "state",
            Lane::Events => "events",
            Lane::Bulk => "bulk",
        };
        let limit = if lane == Lane::Events {
            EVENTS_MESSAGE_BYTES
        } else {
            STATE_MESSAGE_BYTES
        };
        if payload.is_empty() || payload.len() > limit {
            self.metrics.record_drop(lane_name);
            return false;
        }
        if lane == Lane::State {
            if let Some(captured) = captured_at_unix_ms {
                if unix_now_ms.saturating_sub(captured) > STATE_STALE_MS {
                    self.state_stale_drops += 1;
                    self.metrics.record_drop("state");
                    return false;
                }
            }
            self.update_congestion(conn, now_ms);
            if self.state_congested {
                self.metrics.record_drop("state");
                return false;
            }
            if !self.state_budget.take(payload.len(), now_ms) {
                self.state_budget_drops += 1;
                self.metrics.record_drop("state");
                return false;
            }
        } else if lane == Lane::Events && conn.backlog() > EVENTS_MESSAGE_BYTES {
            self.metrics.record_drop("events");
            conn.close(close_code::PROTOCOL as u64, b"events backlog");
            return false;
        }
        match conn.queue(lane, text, payload) {
            Ok(true) => {
                self.metrics
                    .record_out(payload.len(), lane_name, now_ms as u64);
                true
            }
            _ => {
                self.metrics.record_drop(lane_name);
                false
            }
        }
    }

    /// Builds the per-peer transport diagnostics JSON object (`UdpPeer.transportStats` in
    /// `src/udp-peer.js` and `GameSession.transportDiagnostics` in `src/game-session.js`).
    pub fn stats(&mut self, conn: &mut Connection, now_ms: f64) -> Value {
        self.update_congestion(conn, now_ms);
        let snap = self.metrics.snapshot(now_ms as u64);
        let mut obj = snap.as_object().cloned().unwrap_or_default();
        obj.insert("transport".to_string(), Value::String("udp".to_string()));
        obj.insert(
            "path".to_string(),
            json!({
                "kind": self.path.kind,
                "family": self.path.family,
                "address": self.path.address.ip().to_string(),
                "port": self.path.address.port(),
            }),
        );
        obj.insert("stateCongested".to_string(), Value::Bool(self.state_congested));
        obj.insert("stateStaleDrops".to_string(), json!(self.state_stale_drops));
        obj.insert(
            "stateBudgetDrops".to_string(),
            json!(self.state_budget_drops),
        );
        obj.insert(
            "stateInvalidDrops".to_string(),
            json!(self.state_invalid_drops),
        );
        obj.insert(
            "stateBudget".to_string(),
            json!({
                "bytesPerSecond": self.state_budget.rate as u64,
                "burstBytes": self.state_budget.burst as u64,
                "availableBytes": self.state_budget.available(now_ms).floor() as i64,
                "offeredBytes": self.state_budget.offered_bytes,
                "denied": self.state_budget.denied,
            }),
        );
        obj.insert(
            "datagramsAcked".to_string(),
            json!(conn.stats.datagram_packets_acked),
        );
        obj.insert(
            "datagramsLost".to_string(),
            json!(conn.stats.datagram_packets_lost),
        );
        Value::Object(obj)
    }
}
