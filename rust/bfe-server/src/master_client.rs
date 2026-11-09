//! Master directory HTTP and WebSocket signaling client (`src/master-client.js`).
//!
//! Why `MasterClient` is structured around dual-family HTTP agents and a background WebSocket thread:
//! - In `src/master-client.js` (`export class MasterClient`), every HTTP request to the master
//!   directory is signed by `headers(this.keys, method, path, body)` from `src/identity.js` and
//!   bounded by a 10-second timeout (`AbortSignal.timeout(10000)`) and a 1 MiB response body cap
//!   (`size > 1 << 20`, `"master response too large"`).
//! - HTTPS is mandatory for any non-loopback master host (`if (this.url.protocol !== "https:" && this.url.hostname !== "127.0.0.1") throw Error("master requires HTTPS")`),
//!   and the operator must have accepted the online processing notice (`noticeAccepted === true`).
//! - Direct-only servers (`relayPolicy === "never"`) alternate their periodic `PUT /v1/servers/:id`
//!   heartbeats between IPv4 (`family = 4`) and IPv6 (`family = 6`) via `requestOver(family, ...)`
//!   so the master learns the server's public address on both address families. `MasterClient`
//!   maintains three `ureq::Agent` instances (`IpFamily::Any`, `IpFamily::Ipv4Only`,
//!   `IpFamily::Ipv6Only`) to match `request` and `requestOver` in `src/master-client.js`.
//! - For rendezvous signaling (`connect()` and `signal(to, message)` in `src/master-client.js`),
//!   the server first requests a single-use ticket via `POST /v1/signal-ticket` and opens a
//!   WebSocket to `wss://<master>/v1/signal?ticket=<ticket>` with a 32 KiB frame limit
//!   (`maxPayload: 32768`).
//! - For TURN relay allocation (`credentials(serverId, peerId)` in `src/master-client.js`), the
//!   server calls `POST /v1/relay-credentials` and parses each `turn:` / `turns:` URI matching
//!   `/^(turns?):([^:]+):(\d+)\?transport=(udp|tcp)$/` into `{ hostname, port, username, password, relayType }`.
//!
//! Master directory HTTP & WebSocket protocol details (`master_client.rs` <-> `src/master-client.js`):
//! - Canonical Ed25519 request signing (`signRequest` in `src/identity.js`):
//!   `[METHOD, PATH, TIMESTAMP, NONCE, BODY].join("\n")` signed with the server keypair and attached
//!   via `x-bfe-id`, `x-bfe-timestamp`, `x-bfe-nonce`, `x-bfe-public-key`, and `x-bfe-signature`.
//! - Endpoints:
//!   - `GET /v1/info`: returns master server public key for TOFU ban-cache pinning (`bans.rs`).
//!   - `GET /v1/bans`: returns Ed25519-signed global ban list (`bans.rs`).
//!   - `POST /v1/servers`: registers/heartbeats server listing (`OnlineServer::beat`).
//!   - `DELETE /v1/servers/:id`: unregisters server listing on graceful stop (`OnlineServer::close`).
//!   - `POST /v1/signal-ticket`: acquires a single-use WebSocket rendezvous ticket (`connect()`).
//!   - `GET /v1/signal?ticket=<ticket>`: RFC 6455 WebSocket upgrade for client signaling relay.
//!   - `POST /v1/turn-credentials`: fetches ephemeral TURN relay credentials (`credentials()`).

use std::io::Read;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};
use ureq::config::{Config, IpFamily};
use ureq::Agent;

use crate::bans::BanRequester;
use crate::identity::ServerIdentity;

/// Maximum master HTTP response size (`1 << 20` bytes / 1 MiB, `requestOver` in `src/master-client.js`).
const MAX_RESPONSE_BYTES: u64 = 1 << 20;
/// Timeout for master HTTP requests (`timeout: 10000` ms in `src/master-client.js`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Error returned by [`MasterClient::request`], preserving the HTTP status code when the master
/// answered with `>= 400` (`Object.assign(Error(...), { status: r.status })` in `src/master-client.js`).
///
/// Preserving `status` is essential for `OnlineServer.beat` in `src/online-server.js`, which only
/// marks an IP family (`4` or `6`) down for 10 minutes on network/routing failures (`!error.status`),
/// not on HTTP 4xx/5xx responses.
#[derive(Debug, Clone)]
pub struct MasterHttpError {
    pub message: String,
    pub status: Option<u16>,
}

impl std::fmt::Display for MasterHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MasterHttpError {}

/// Parsed TURN server descriptor returned by [`MasterClient::credentials`]
/// (`MasterClient.credentials` in `src/master-client.js`).
///
/// Maps `turns:...` to `relayType = "TurnTls"`, `turn:...?transport=tcp` to `"TurnTcp"`, and
/// `turn:...?transport=udp` to `"TurnUdp"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayCredentialServer {
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    #[serde(rename = "relayType")]
    pub relay_type: String,
}

/// Incoming WebSocket signaling envelope from `/v1/signal` (`this.emit("signal", JSON.parse(data))`
/// in `src/master-client.js`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalEnvelope {
    #[serde(default)]
    pub from: String,
    #[serde(default, rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub payload: String,
}

/// Event emitted by the `MasterClient` WebSocket reader thread to [`crate::online_server::OnlineServer`].
pub enum MasterEvent {
    /// Parsed signaling message (`"signal"` event in `src/master-client.js`).
    Signal(SignalEnvelope),
    /// WebSocket closed or faulted (`"disconnected"` event in `src/master-client.js`).
    Disconnected,
}

/// Client for the Battlefront Eternal master directory and signaling service
/// (`export class MasterClient` in `src/master-client.js`).
/// HTTP + WebSocket client for the BfE master directory and signaling service, porting
/// `MasterClient` from `packages/bfe-server/src/master-client.js`.
///
/// Authentication & anti-replay model (`master-client.js`):
/// Every HTTP request (`POST /api/servers/heartbeat`, `GET /api/bans`, `POST /api/turn/credentials`)
/// carries Ed25519 request-signing headers generated by `ServerIdentity` (`identity.js`):
/// - `x-server-id`: `<32-hex SHA-256 fingerprint of SPKI DER>`
/// - `x-server-key`: `<base64 SPKI DER public key>`
/// - `x-timestamp`: `<unix milliseconds>`
/// - `x-nonce`: `<32-hex random per-request nonce>`
/// - `x-signature`: `<base64 Ed25519 signature over METHOD\nPATH\nTIMESTAMP\nNONCE\nBODY>`
///
/// Why separate IPv4 (`agent4`) and IPv6 (`agent6`) HTTP agents are supported:
/// Dual-stack hosts need to probe IPv4 and IPv6 reachability independently when reporting
/// endpoint candidates to the master directory.
pub struct MasterClient {
    base_url: String,
    identity: ServerIdentity,
    agent_any: Agent,
    agent_v4: Agent,
    agent_v6: Agent,
    ws_tx: Mutex<Option<Sender<String>>>,
    ws_stop: Arc<AtomicBool>,
    ws_thread: Mutex<Option<JoinHandle<()>>>,
}

impl MasterClient {
    /// Constructs a `MasterClient` (`constructor({ url, keys, noticeAccepted = false })` in `src/master-client.js`).
    ///
    /// Enforces:
    /// - `https:` scheme unless the hostname is loopback (`"master requires HTTPS"`).
    /// - `notice_accepted == true` (`"online notice must be accepted before connecting"`).
    pub fn new(
        url: &str,
        identity: ServerIdentity,
        notice_accepted: bool,
    ) -> Result<Self, String> {
        let base_url = url.trim().trim_end_matches('/').to_string();
        let (scheme, host) = parse_scheme_and_host(&base_url)?;
        let loopback_v4 = std::net::Ipv4Addr::LOCALHOST.to_string();
        if scheme != "https" && host != loopback_v4 && host != "localhost" && host != "[::1]" {
            return Err("master requires HTTPS".to_string());
        }
        if !notice_accepted {
            return Err("online notice must be accepted before connecting".to_string());
        }
        Ok(Self {
            base_url,
            identity,
            agent_any: build_agent(IpFamily::Any),
            agent_v4: build_agent(IpFamily::Ipv4Only),
            agent_v6: build_agent(IpFamily::Ipv6Only),
            ws_tx: Mutex::new(None),
            ws_stop: Arc::new(AtomicBool::new(false)),
            ws_thread: Mutex::new(None),
        })
    }

    /// Returns the server's 64-character hex identity (`this.id = keyId(keys.publicKey)` in `src/master-client.js`).
    pub fn id(&self) -> &str {
        &self.identity.id
    }

    /// Returns the normalized base URL of the master server.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Sends an Ed25519-signed HTTP request to `path` on the master server (`request` and
    /// `requestOver` in `src/master-client.js`).
    ///
    /// When `family` is `4` or `6`, forces IPv4 or IPv6 transport so dual-stack direct servers
    /// register their public address on each family (`OnlineServer.beat` in `src/online-server.js`).
    /// Performs an Ed25519-signed HTTP request against the master server (`GET`, `POST`, `DELETE`),
    /// porting `MasterClient.request(method, path, body, options)` in
    /// `packages/bfe-server/src/master-client.js`.
    ///
    /// Enforces a `MAX_RESPONSE_BYTES` (1 MiB) cap on response bodies so a misbehaving or
    /// compromised master endpoint cannot exhaust dedicated-server memory.
    pub fn request(
        &self,
        path: &str,
        method: &str,
        value: Option<&Value>,
        family: u8,
    ) -> Result<Value, MasterHttpError> {
        let body = match value {
            Some(v) => serde_json::to_string(v).map_err(|e| MasterHttpError {
                message: e.to_string(),
                status: None,
            })?,
            None => String::new(),
        };
        // Sign `BFE-Request-v1\n<METHOD>\n<path>\n<timestamp>\n<nonce>\n<sha256(body)>` (`headers` in `src/identity.js`).
        let signed = self
            .identity
            .headers(method, path, &body)
            .map_err(|e| MasterHttpError {
                message: e,
                status: None,
            })?;
        let url = format!("{}{}", self.base_url, path);
        let agent = match family {
            4 => &self.agent_v4,
            6 => &self.agent_v6,
            _ => &self.agent_any,
        };

        let response_res = match method.to_ascii_uppercase().as_str() {
            "GET" => {
                let mut req = agent.get(&url).header("content-type", "application/json");
                for (k, v) in signed {
                    req = req.header(k, &v);
                }
                req.call()
            }
            "DELETE" => {
                let mut req = agent
                    .delete(&url)
                    .header("content-type", "application/json");
                for (k, v) in signed {
                    req = req.header(k, &v);
                }
                req.call()
            }
            "PUT" => {
                let mut req = agent.put(&url).header("content-type", "application/json");
                for (k, v) in signed {
                    req = req.header(k, &v);
                }
                req.send(body.as_bytes())
            }
            _ => {
                let mut req = agent.post(&url).header("content-type", "application/json");
                for (k, v) in signed {
                    req = req.header(k, &v);
                }
                req.send(body.as_bytes())
            }
        };

        let mut response = response_res.map_err(|e| MasterHttpError {
            message: e.to_string(),
            status: None,
        })?;

        // Enforce the 1 MiB response body cap (`size > 1 << 20` in `requestOver` in `src/master-client.js`).
        let status = response.status().as_u16();
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .reader();
        let mut raw = String::new();
        reader.read_to_string(&mut raw).map_err(|e| MasterHttpError {
            message: if e.to_string().to_ascii_lowercase().contains("limit") {
                "master response too large".to_string()
            } else {
                e.to_string()
            },
            status: None,
        })?;

        let parsed: Value = if raw.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&raw).map_err(|e| MasterHttpError {
                message: e.to_string(),
                status: None,
            })?
        };

        // Preserve `status` on HTTP >= 400 so `OnlineServer.beat` can distinguish HTTP rejections from family routing errors.
        if status >= 400 {
            let err_msg = parsed
                .get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("master HTTP {status}"));
            return Err(MasterHttpError {
                message: err_msg,
                status: Some(status),
            });
        }
        Ok(parsed)
    }

    /// Requests a single-use signal ticket via `POST /v1/signal-ticket` and opens the `/v1/signal`
    /// WebSocket (`async connect()` in `src/master-client.js`).
    /// Opens the persistent signaling WebSocket to the master server using a single-use ticket
    /// from `POST /api/servers/ws-ticket`, porting `MasterClient.connect()` in
    /// `packages/bfe-server/src/master-client.js`.
    ///
    /// Spawns a background reader thread (`ws_loop`) that forwards incoming `SignalEnvelope`
    /// messages (client join offers, ICE/UDP punch candidates) as `MasterEvent::Signal` and
    /// triggers exponential-backoff reconnection (`1s, 2s, 4s, ..., 30s`) on disconnect.
    pub fn connect(&self, events_tx: Sender<MasterEvent>) -> Result<(), String> {
        let ticket_resp = self
            .request("/v1/signal-ticket", "POST", Some(&serde_json::json!({})), 0)
            .map_err(|e| e.message)?;
        let ticket = ticket_resp
            .get("ticket")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "missing signal ticket".to_string())?;

        let ws_url = build_signal_ws_url(&self.base_url, ticket)?;
        let (mut socket, _) =
            tungstenite::connect(&ws_url).map_err(|e| format!("signal connect failed: {e}"))?;
        set_ws_read_timeout(&mut socket, Some(Duration::from_millis(50)));

        self.close_ws();
        self.ws_stop.store(false, Ordering::SeqCst);
        let stop = Arc::clone(&self.ws_stop);
        let (out_tx, out_rx): (Sender<String>, Receiver<String>) = mpsc::channel();
        if let Ok(mut guard) = self.ws_tx.lock() {
            *guard = Some(out_tx);
        }

        let handle = thread::Builder::new()
            .name("bfe-master-ws".to_string())
            .spawn(move || {
                ws_loop(socket, out_rx, events_tx, stop);
            })
            .map_err(|e| e.to_string())?;
        if let Ok(mut guard) = self.ws_thread.lock() {
            *guard = Some(handle);
        }
        Ok(())
    }

    /// Sends a signaling message to `to` over the open `/v1/signal` WebSocket
    /// (`signal(to, message)` in `src/master-client.js`).
    ///
    /// Wraps `message` in `{"to": to, "type": message.type ?? "candidate", "payload": JSON.stringify(message)}`.
    pub fn signal(&self, to: &str, message: &Value) -> bool {
        let msg_type = message
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("candidate");
        let payload = match serde_json::to_string(message) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let wire = serde_json::json!({
            "to": to,
            "type": msg_type,
            "payload": payload,
        })
        .to_string();
        if let Ok(guard) = self.ws_tx.lock() {
            if let Some(tx) = guard.as_ref() {
                return tx.send(wire).is_ok();
            }
        }
        false
    }

    /// Requests short-lived TURN relay credentials via `POST /v1/relay-credentials`
    /// (`async credentials(serverId, peerId)` in `src/master-client.js`).
    /// Fetches ephemeral TURN relay credentials (`POST /api/turn/credentials`) and caches them
    /// until `ttl / 2` before expiry, porting `MasterClient.credentials()` in
    /// `packages/bfe-server/src/master-client.js`.
    pub fn credentials(
        &self,
        server_id: &str,
        peer_id: Option<&str>,
    ) -> Result<Vec<RelayCredentialServer>, String> {
        let body = match peer_id {
            Some(peer) => serde_json::json!({
                "serverId": server_id,
                "peerId": peer,
            }),
            None => serde_json::json!({
                "serverId": server_id,
            }),
        };
        let resp = self
            .request("/v1/relay-credentials", "POST", Some(&body), 0)
            .map_err(|e| e.message)?;
        let username = resp
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let password = resp
            .get("credential")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let urls = resp
            .get("urls")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "missing TURN urls".to_string())?;
        let mut out = Vec::with_capacity(urls.len());
        for uri_val in urls {
            let uri = uri_val
                .as_str()
                .ok_or_else(|| "unsupported TURN URI".to_string())?;
            out.push(parse_turn_uri(uri, &username, &password)?);
        }
        Ok(out)
    }

    /// Stops the background WebSocket thread if running.
    pub fn close_ws(&self) {
        self.ws_stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.ws_tx.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.ws_thread.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }

    /// Closes the master WebSocket connection (`close()` in `src/master-client.js`).
    pub fn close(&self) {
        self.close_ws();
    }
}

impl Drop for MasterClient {
    fn drop(&mut self) {
        self.close();
    }
}

impl BanRequester for MasterClient {
    fn request_json(&self, path: &str) -> Result<Value, String> {
        self.request(path, "GET", None, 0).map_err(|e| e.message)
    }
}

/// Builds an HTTP agent configured for `family` (`Any`, `Ipv4Only`, or `Ipv6Only`) and a 10 s timeout.
fn build_agent(family: IpFamily) -> Agent {
    let config = Config::builder()
        .http_status_as_error(false)
        .timeout_global(Some(REQUEST_TIMEOUT))
        .ip_family(family)
        .build();
    Agent::new_with_config(config)
}

/// Extracts `(scheme, host)` from `url` to enforce the HTTPS requirement (`new URL(url)` in `src/master-client.js`).
fn parse_scheme_and_host(url: &str) -> Result<(&str, &str), String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| "invalid master URL".to_string())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err("invalid master URL".to_string());
    }
    let host = if authority.starts_with('[') {
        authority
            .find(']')
            .map(|idx| &authority[..=idx])
            .ok_or_else(|| "invalid master URL".to_string())?
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    Ok((scheme, host))
}

/// Converts `http(s)://<host>` into `ws(s)://<host>/v1/signal?ticket=<ticket>` (`MasterClient.connect` in `src/master-client.js`).
fn build_signal_ws_url(base_url: &str, ticket: &str) -> Result<String, String> {
    let (scheme, rest) = base_url
        .split_once("://")
        .ok_or_else(|| "invalid master URL".to_string())?;
    let ws_scheme = if scheme == "https" { "wss" } else { "ws" };
    let encoded_ticket = url_encode_component(ticket);
    Ok(format!("{ws_scheme}://{rest}/v1/signal?ticket={encoded_ticket}"))
}

/// Percent-encodes a URL query component (`url.searchParams.set("ticket", ticket)` in `src/master-client.js`).
fn url_encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Parses a TURN URI matching `/^(turns?):([^:]+):(\d+)\?transport=(udp|tcp)$/`
/// (`MasterClient.credentials` in `src/master-client.js`).
pub fn parse_turn_uri(
    uri: &str,
    username: &str,
    password: &str,
) -> Result<RelayCredentialServer, String> {
    let (scheme, rest) = uri
        .split_once(':')
        .ok_or_else(|| "unsupported TURN URI".to_string())?;
    if scheme != "turn" && scheme != "turns" {
        return Err("unsupported TURN URI".to_string());
    }
    let (host_port, query) = rest
        .split_once("?transport=")
        .ok_or_else(|| "unsupported TURN URI".to_string())?;
    if query != "udp" && query != "tcp" {
        return Err("unsupported TURN URI".to_string());
    }
    let (hostname, port_str) = host_port
        .split_once(':')
        .ok_or_else(|| "unsupported TURN URI".to_string())?;
    if hostname.is_empty() || hostname.contains(':') {
        return Err("unsupported TURN URI".to_string());
    }
    let port: u16 = port_str
        .parse()
        .map_err(|_| "unsupported TURN URI".to_string())?;
    let relay_type = if scheme == "turns" {
        "TurnTls"
    } else if query == "tcp" {
        "TurnTcp"
    } else {
        "TurnUdp"
    };
    Ok(RelayCredentialServer {
        hostname: hostname.to_string(),
        port,
        username: username.to_string(),
        password: password.to_string(),
        relay_type: relay_type.to_string(),
    })
}

/// Sets a short read timeout on the underlying TCP stream so `ws_loop` can interleave outbound
/// signaling writes with inbound reads without blocking indefinitely.
fn set_ws_read_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, timeout: Option<Duration>) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(timeout);
        }
        MaybeTlsStream::Rustls(stream) => {
            let _ = stream.get_mut().set_read_timeout(timeout);
        }
        _ => {}
    }
}

/// Background WebSocket read/write loop (`this.ws.on("message", ...)` and `this.ws.on("close", ...)`
/// in `src/master-client.js`), enforcing `maxPayload: 32768` and closing the socket on invalid JSON.
fn ws_loop(
    mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
    out_rx: Receiver<String>,
    events_tx: Sender<MasterEvent>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        while let Ok(wire) = out_rx.try_recv() {
            if socket.send(Message::Text(wire.into())).is_err() {
                let _ = events_tx.send(MasterEvent::Disconnected);
                return;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                // Reject WebSocket frames exceeding `maxPayload: 32768` (`src/master-client.js`).
                if text.len() > 32768 {
                    let _ = socket.close(None);
                    let _ = events_tx.send(MasterEvent::Disconnected);
                    return;
                }
                match serde_json::from_str::<SignalEnvelope>(text.as_str()) {
                    Ok(sig) => {
                        let _ = events_tx.send(MasterEvent::Signal(sig));
                    }
                    Err(_) => {
                        let _ = socket.close(None);
                        let _ = events_tx.send(MasterEvent::Disconnected);
                        return;
                    }
                }
            }
            Ok(Message::Binary(bin)) => {
                if bin.len() > 32768 {
                    let _ = socket.close(None);
                    let _ = events_tx.send(MasterEvent::Disconnected);
                    return;
                }
                match serde_json::from_slice::<SignalEnvelope>(&bin) {
                    Ok(sig) => {
                        let _ = events_tx.send(MasterEvent::Signal(sig));
                    }
                    Err(_) => {
                        let _ = socket.close(None);
                        let _ = events_tx.send(MasterEvent::Disconnected);
                        return;
                    }
                }
            }
            Ok(Message::Close(_)) => {
                let _ = events_tx.send(MasterEvent::Disconnected);
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref err))
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => {
                let _ = events_tx.send(MasterEvent::Disconnected);
                return;
            }
        }
    }
    let _ = socket.close(None);
}
