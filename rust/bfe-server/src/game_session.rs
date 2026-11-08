//! Server-side game session framing, rate limits, liveness timeouts, and RPC dispatch
//! (`src/game-session.js` and `src/udp-peer.js`).
//!
//! Why `GameSessionState` exists separately from `UdpPeerState`:
//! In the Node dedicated server (`export class GameSession` and `class WorldRouter` in
//! `src/game-session.js`), `UdpPeer` owns the transport channel while `GameSession` owns the
//! per-player protocol rules between the transport and the authoritative world:
//! 1. **Input token bucket (`INPUT_RATE_PER_SECOND = 75`, `INPUT_BURST = 360`, `admitInput` in `src/game-session.js`)**:
//!    A 60 Hz client normally sends one `BNI1` datagram per tick (60 Hz), with brief bursts when
//!    the browser tab or OS scheduler catches up (`INPUT_BURST = 360`, ~6 seconds of headroom,
//!    refilling at `INPUT_RATE_PER_SECOND = 75` per second). Datagrams exceeding this rate are
//!    dropped at the session boundary before any allocation or world-router copy, and counted in
//!    `dropped_inputs` (`inputsDropped` in `GameSession.transportCounters`).
//! 2. **Two-tier silence timeouts (`SESSION_JOIN_SILENT_MS = 180_000`, `SESSION_SILENT_MS = 30_000`,
//!    `WorldRouter.checkLiveness` in `src/game-session.js`)**:
//!    - Before the client has sent its `start` RPC (`started == false`), it may still be loading
//!      map assets or sitting at the spawn/team-selection screen, so it is allowed up to
//!      `SESSION_JOIN_SILENT_MS` (180 s / 3 minutes) of silence.
//!    - Once the client has started (`started == true`), it sends 60 Hz inputs or 10-byte `BNA1`
//!      keepalives; if no message arrives for `SESSION_SILENT_MS` (30 s), the session is closed
//!      with `"session silent"` (`peer.close('idle', 'No messages from the client.')`).
//! 3. **Congestion recovery resync (`check_congestion_transition` / `resync` in `src/game-session.js`)**:
//!    When `UdpPeerState.state_congested` clears (`true -> false`), the client missed intermediate
//!    delta snapshots while congested, so `GameSessionState` triggers a `ResyncState` action to
//!    reset the client's delta baseline.
//! 4. **Control message framing and dispatch (`GameSession.message` in `src/game-session.js`)**:
//!    Validates incoming JSON messages on the `events` lane (`<= 65,536` bytes), answers application
//!    `ping` / `pong` probes (`PING_INTERVAL_MS = 2,000 ms`), forwards `edge-ack` / `entity-edge-ack`,
//!    and dispatches `call` / `rpc` methods (`start`, `chat`, etc.).

use serde_json::{json, Value};

/// Sustained client input datagram refill rate (`export const INPUT_RATE_PER_SECOND = 75` in `src/game-session.js`).
pub const INPUT_RATE_PER_SECOND: f64 = 75.0;
/// Maximum client input token burst capacity (`export const INPUT_BURST = 360` in `src/game-session.js`).
pub const INPUT_BURST: f64 = 360.0;
/// Maximum client silence once started in the battle (`SESSION_SILENT_MS = 30000` in `src/game-session.js`).
pub const SESSION_SILENT_MS: f64 = 30_000.0;
/// Maximum client silence while loading / before sending `start` (`SESSION_JOIN_SILENT_MS = 180000` in `src/game-session.js`).
pub const SESSION_JOIN_SILENT_MS: f64 = 180_000.0;
/// Maximum size in bytes of an incoming JSON control frame on the `events` lane.
const CONTROL_MAX_BYTES: usize = 65_536;
/// Interval in milliseconds between server-initiated application `ping` messages on `events` (`src/udp-peer.js`).
const PING_INTERVAL_MS: f64 = 2_000.0;

/// Action emitted by [`GameSessionState`] for [`crate::udp_server::UdpGameServer`] and the world loop to execute.
#[derive(Clone, Debug)]
pub enum SessionAction {
    /// Queue a serialized JSON text message on the reliable `events` lane (`peer.send('events', ...)`).
    SendEvents(String),
    /// Close the peer's transport session with `code` and `reason` (`peer.close(...)`).
    Close { code: u16, reason: String },
    /// Client invoked the `start` RPC (`m.method === 'start'` in `GameSession.message`).
    ClientStart { call_id: Option<Value>, args: Value },
    /// Client sent a chat message (`m.type === 'chat'` or `method === 'chat'` in `GameSession.message`).
    ClientChat { call_id: Option<Value>, text: String },
    /// Client acknowledged reliable entity-edge outcomes (`entity-edge-ack` in `GameSession.message`).
    EdgeAck(Value),
    /// Request a full snapshot baseline reset for this client (`resync` in `GameSession.message`).
    ResyncState,
    /// Forward a validated, rate-limited `BNI1` or `BNA1` binary datagram to the world host (`WorldRouter.input`).
    ForwardInput(Vec<u8>),
}

/// Per-player session state machine (`export class GameSession` in `src/game-session.js`).
pub struct GameSessionState {
    /// 64-character lowercase hex SHA-256 client identity (`this.id` in `src/game-session.js`).
    pub id: String,
    /// Available input datagram tokens (`this.inputTokens`, initialized to `INPUT_BURST = 360`).
    pub tokens: f64,
    /// Monotonic timestamp (`now_ms`) when `tokens` was last refilled (`this.inputRefilledAt`).
    pub last_refill_ms: f64,
    /// Monotonic timestamp (`now_ms`) when the last valid frame arrived from this client (`this.heardAt`).
    pub last_heard_ms: f64,
    /// Monotonic timestamp (`now_ms`) when the last `ping` was sent on `events`.
    pub last_ping_ms: f64,
    /// Whether the client has sent the `start` RPC to enter active gameplay (`this.started`).
    pub started: bool,
    /// Whether this session has been closed (`this.closed`).
    pub closed: bool,
    /// Smoothed application-layer RTT in milliseconds measured from `ping`/`pong`.
    pub rtt: Option<f64>,
    /// Total `state` input datagrams dropped by `take_input_token` (`transportCounters.inputsDropped`).
    pub dropped_inputs: u64,
    /// Previous congestion state used to detect `congested -> clear` transitions for baseline resync.
    pub was_congested: bool,
}

impl GameSessionState {
    /// Creates a new `GameSessionState` at `now_ms` with a full `INPUT_BURST` (360) token bucket
    /// (`new GameSession` in `src/game-session.js`).
    pub fn new(id: String, now_ms: f64) -> Self {
        Self {
            id,
            tokens: INPUT_BURST,
            last_refill_ms: now_ms,
            last_heard_ms: now_ms,
            last_ping_ms: now_ms,
            started: false,
            closed: false,
            rtt: None,
            dropped_inputs: 0,
            was_congested: false,
        }
    }

    /// Refills and consumes one token from the `75/s` (`burst 360`) input rate limiter
    /// (`GameSession.admitInput(now)` in `src/game-session.js`).
    ///
    /// Returns `false` and increments `dropped_inputs` when the client exceeds the token budget.
    pub fn take_input_token(&mut self, now_ms: f64) -> bool {
        let dt = ((now_ms - self.last_refill_ms).max(0.0)) / 1000.0;
        self.last_refill_ms = now_ms;
        self.tokens = (self.tokens + dt * INPUT_RATE_PER_SECOND).min(INPUT_BURST);
        if self.tokens < 1.0 {
            self.dropped_inputs += 1;
            return false;
        }
        self.tokens -= 1.0;
        true
    }

    /// Detects when a peer exits congestion (`was_congested && !congested`) so the world host can
    /// force a full snapshot resync after skipped delta frames.
    pub fn check_congestion_transition(&mut self, congested: bool) -> bool {
        if self.was_congested && !congested {
            self.was_congested = false;
            return true;
        }
        self.was_congested = congested;
        false
    }

    /// Evaluates periodic session timers at `now_ms` (`WorldRouter.checkLiveness` in `src/game-session.js`
    /// and `this.timer` in `src/udp-peer.js`):
    /// - Evicts the session if silent for longer than `SESSION_SILENT_MS` (30 s when `started`) or
    ///   `SESSION_JOIN_SILENT_MS` (180 s before `started`).
    /// - Emits a `{"type": "ping", "time": ...}` message every `PING_INTERVAL_MS` (2,000 ms).
    pub fn poll_timer(&mut self, now_ms: f64) -> Vec<SessionAction> {
        if self.closed {
            return Vec::new();
        }
        let silent_limit = if self.started {
            SESSION_SILENT_MS
        } else {
            SESSION_JOIN_SILENT_MS
        };
        if now_ms - self.last_heard_ms > silent_limit {
            self.closed = true;
            return vec![SessionAction::Close {
                code: 1,
                reason: "session silent".to_string(),
            }];
        }
        if now_ms - self.last_ping_ms >= PING_INTERVAL_MS {
            self.last_ping_ms = now_ms;
            let ping = json!({
                "type": "ping",
                "time": now_ms.round() as i64,
            });
            return vec![SessionAction::SendEvents(ping.to_string())];
        }
        Vec::new()
    }

    /// Handles a validated binary datagram on the `state` lane (`GameSession.binary` in `src/game-session.js`).
    ///
    /// Applies `take_input_token(now_ms)` before copying `bytes` into `SessionAction::ForwardInput`.
    pub fn on_state_datagram(&mut self, bytes: &[u8], now_ms: f64) -> Option<SessionAction> {
        if self.closed {
            return None;
        }
        if !self.take_input_token(now_ms) {
            return None;
        }
        self.last_heard_ms = now_ms;
        Some(SessionAction::ForwardInput(bytes.to_vec()))
    }

    /// Parses and dispatches a JSON message on the reliable `events` lane
    /// (`GameSession.message` in `src/game-session.js` and `UdpPeer.receive` in `src/udp-peer.js`).
    ///
    /// Oversized frames (`> 65,536` bytes) or malformed JSON close the session immediately with
    /// protocol close code `2` (`"invalid-message"` in `src/udp-peer.js`).
    pub fn on_events_message(&mut self, raw: &[u8], now_ms: f64) -> Vec<SessionAction> {
        if self.closed {
            return Vec::new();
        }
        if raw.len() > CONTROL_MAX_BYTES {
            self.closed = true;
            return vec![SessionAction::Close {
                code: 2,
                reason: "oversized control frame".to_string(),
            }];
        }
        let Ok(msg) = serde_json::from_slice::<Value>(raw) else {
            self.closed = true;
            return vec![SessionAction::Close {
                code: 2,
                reason: "invalid json".to_string(),
            }];
        };
        let Some(obj) = msg.as_object() else {
            self.closed = true;
            return vec![SessionAction::Close {
                code: 2,
                reason: "invalid json".to_string(),
            }];
        };
        self.last_heard_ms = now_ms;
        let msg_type = obj.get("type").and_then(Value::as_str).unwrap_or_default();
        match msg_type {
            "ping" => {
                let time = obj.get("time").cloned().unwrap_or(Value::Null);
                vec![SessionAction::SendEvents(
                    json!({"type": "pong", "time": time}).to_string(),
                )]
            }
            "pong" => {
                if let Some(t) = obj.get("time").and_then(Value::as_f64) {
                    if t.is_finite() {
                        let d = now_ms - t;
                        if (0.0..60_000.0).contains(&d) {
                            self.rtt = Some(match self.rtt {
                                None => d,
                                Some(prev) => prev * 0.8 + d * 0.2,
                            });
                        }
                    }
                }
                Vec::new()
            }
            "edge-ack" | "entity-edge-ack" => vec![SessionAction::EdgeAck(msg)],
            "call" | "rpc" => {
                let call_id = obj.get("id").cloned();
                let method = obj.get("method").and_then(Value::as_str).unwrap_or_default();
                let args = obj
                    .get("args")
                    .or_else(|| obj.get("fields"))
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Default::default()));
                match method {
                    "start" => {
                        self.started = true;
                        vec![SessionAction::ClientStart { call_id, args }]
                    }
                    "chat" => {
                        let text = args
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .chars()
                            .take(240)
                            .collect::<String>();
                        vec![SessionAction::ClientChat { call_id, text }]
                    }
                    _ => vec![SessionAction::SendEvents(
                        json!({
                            "type": "reply",
                            "id": call_id,
                            "ok": false,
                            "error": "unknown method",
                        })
                        .to_string(),
                    )],
                }
            }
            _ => Vec::new(),
        }
    }
}
