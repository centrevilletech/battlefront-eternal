// Master server HTTPS/HTTP client (`GET /v1/servers`, `POST /v1/relay-credentials`,
// `POST /v1/signal-ticket`) with Ed25519 request signing and `rustls` TLS.
//
// Ports `requestMaster`, `fetchJson`, `unreachableError`, `masterHost`,
// `listServers`, `serverInfo`, `relayCredentials`, and `signalTicket` from
// `electron/network.js`.

use std::env;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ureq::config::{Config, IpFamily};
use ureq::Agent;

use crate::browser::ServerEntry;
use crate::identity::{is_lower_hex_64, PlayerIdentity};
use crate::ping::{with_latency, PING_TIMEOUT};

// Production master server URL (`DEFAULT_MASTER` in `electron/network.js`).
pub const DEFAULT_MASTER_URL: &str = "https://online.battlefronteternal.com";

// Default per-request master HTTP timeout (10s, matching `MASTER_TIMEOUT_MS`
// in `electron/network.js`).
pub const MASTER_TIMEOUT: Duration = Duration::from_secs(10);

// Delay before retrying a failed idempotent `GET` request (750 ms, matching
// `MASTER_RETRY_DELAY_MS` in `electron/network.js`).
pub const MASTER_RETRY_DELAY: Duration = Duration::from_millis(750);

// Maximum response body size accepted from the master server (128 KiB, matching
// `fetchJson` in `electron/network.js`).
const MAX_MASTER_RESPONSE_BYTES: u64 = 128 * 1024;

// IP address family preference for outbound master HTTP/HTTPS connections.
// Defaults to dual-stack (`Any`), while allowing CLI diagnostics (`--ipv4` /
// `--ipv6`) and loopback unit tests to pin `Ipv4Only` or `Ipv6Only`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MasterIpFamily {
    #[default]
    Any,
    Ipv4Only,
    Ipv6Only,
}

impl MasterIpFamily {
    // Converts this crate's family selector into `ureq::config::IpFamily`.
    fn into_ureq(self) -> IpFamily {
        match self {
            Self::Any => IpFamily::Any,
            Self::Ipv4Only => IpFamily::Ipv4Only,
            Self::Ipv6Only => IpFamily::Ipv6Only,
        }
    }
}

// Short-lived TURN relay credentials returned by `POST /v1/relay-credentials`
// (`dispatch('credentials')` in `electron/network.js`).
// - `urls`: `turn:` / `turns:` server URIs accepted by the ICE/TURN client.
// - `username` and `credential`: time-limited HMAC TURN credentials scoped to
//   the player's identity and the target `serverId`.
// - `ttl`: credential validity window in seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayCredentials {
    // TURN/STUN server URI list (`["turn:turn.battlefronteternal.com:3478?transport=udp", ...]`).
    #[serde(default)]
    pub urls: Vec<String>,
    // Ephemeral coturn REST API username (`"<expiry_unix>:<peer_id>"`).
    pub username: String,
    // Base64 HMAC-SHA1 TURN credential paired with `username`.
    pub credential: String,
    // Time-to-live of the credentials in seconds.
    pub ttl: u64,
}

// One-time WebSocket signaling ticket returned by `POST /v1/signal-ticket`
// (`dispatch('connect')` in `electron/network.js`), redeemed on
// `wss://<master>/v1/signal?ticket=<ticket>` to exchange WebRTC/UDP offers,
// answers, ICE candidates, and match readiness probes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalTicket {
    // Hex/opaque single-use ticket passed as `?ticket=` on `/v1/signal`.
    pub ticket: String,
    // Expiration timestamp in milliseconds since `UNIX_EPOCH`.
    pub expires: u64,
}

// JSON envelope returned by `GET /v1/servers` (`{ "servers": [...] }`).
#[derive(Debug, Deserialize)]
struct ServersEnvelope {
    #[serde(default)]
    servers: Vec<ServerEntry>,
}

// JSON envelope returned by `GET /v1/servers/:id` (`{ "server": { ... } }`).
#[derive(Debug, Deserialize)]
struct SingleServerEnvelope {
    server: ServerEntry,
}

// Error JSON body returned by the master on HTTP 4xx/5xx (`{ "error": "..." }`
// in `electron/network.js`: `result.error || ...`).
#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: Option<String>,
}

// Resolves the active master server URL from `BFE_MASTER_URL` or falls back to
// `DEFAULT_MASTER_URL`, stripping trailing slashes.
// Resolves the active master server URL from `BFE_MASTER_URL` when set to a
// non-empty string, or falls back to `DEFAULT_MASTER_URL`
// (`https://online.battlefronteternal.com`), stripping any trailing `/`.
pub fn resolve_master_url() -> String {
    let raw = env::var("BFE_MASTER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MASTER_URL.to_string());
    normalize_base_url(&raw)
}

// Trims surrounding whitespace and trailing `/` characters from a master base
// URL so concatenating routes like `"/v1/servers"` never produces `"//v1/servers"`.
pub fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

// Extracts the `host[:port]` authority from a master URL for user-facing error
// messages, matching `masterHost(baseUrl)` in `electron/network.js`.
pub fn master_host(base_url: &str) -> &str {
    let trimmed = base_url.trim();
    let after_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    after_scheme
        .split(['/', '?', '#'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed)
}

// Formats a transport/connection failure into the exact user-facing message
// produced by `unreachableError(baseUrl, err)` in `electron/network.js`.
// Formats a socket/DNS/TLS connection failure into a concise user-facing error
// message naming the target master `host[:port]` authority and distinguishing
// request timeouts (`"timed out"`) from connection errors (`unreachable` in
// `electron/network.js`).
pub fn unreachable_error(base_url: &str, detail: &str) -> String {
    let host = master_host(base_url);
    let lower = detail.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        return format!("Master server at {host} timed out");
    }
    let clean = detail
        .trim()
        .strip_prefix("io: ")
        .unwrap_or_else(|| detail.trim());
    if clean.is_empty() {
        format!("Master server at {host} is unreachable")
    } else {
        format!("Master server at {host} is unreachable ({clean})")
    }
}

// Signed HTTP/HTTPS client for the Battlefront Eternal master service
// (`request(route, method, value)` in `electron/network.js`).
//
// Signs every request with the player's Ed25519 `PlayerIdentity` (`x-bfe-key`,
// `x-bfe-time`, `x-bfe-nonce`, `x-bfe-signature`, `x-bfe-notice`), enforces a
// 128 KiB response body cap, and retries idempotent `GET` requests once with a
// freshly generated timestamp and nonce if the first attempt fails at the
// transport layer (`if (method === 'GET' && attempt === 0)` in `electron/network.js`).
pub struct MasterClient {
    // Normalized base URL of the master service (without trailing slash).
    base_url: String,
    // Persistent Ed25519 player identity used to sign every outbound request.
    identity: PlayerIdentity,
    // Configured `ureq::Agent` connection pool and TLS client.
    agent: Agent,
    // Delay between the 1st and 2nd attempt of a failed idempotent `GET` request.
    retry_delay: Duration,
    // Per-endpoint TCP handshake timeout used by `list_servers`.
    ping_timeout: Duration,
}

impl MasterClient {
    // Creates a `MasterClient` with standard 10s request timeout, 750ms GET
    // retry delay, and dual-stack IP resolution.
    pub fn new(base_url: impl Into<String>, identity: PlayerIdentity) -> Self {
        Self::with_options(
            base_url,
            identity,
            MASTER_TIMEOUT,
            MASTER_RETRY_DELAY,
            MasterIpFamily::Any,
        )
    }

    // Creates a `MasterClient` targeting `BFE_MASTER_URL` (or `DEFAULT_MASTER_URL`).
    pub fn from_env(identity: PlayerIdentity) -> Self {
        Self::new(resolve_master_url(), identity)
    }

    // Configures the underlying `ureq::Agent` with `http_status_as_error(false)`
    // so HTTP 4xx/5xx responses can be inspected for structured `{ "error": "..." }`
    // JSON bodies instead of being conflated with transport-level failures.
    pub fn with_options(
        base_url: impl Into<String>,
        identity: PlayerIdentity,
        timeout: Duration,
        retry_delay: Duration,
        ip_family: MasterIpFamily,
    ) -> Self {
        let config = Config::builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .ip_family(ip_family.into_ureq())
            .build();
        let agent = Agent::new_with_config(config);
        Self {
            base_url: normalize_base_url(&base_url.into()),
            identity,
            agent,
            retry_delay,
            ping_timeout: PING_TIMEOUT,
        }
    }

    // Overrides the per-endpoint TCP connect timeout used by `list_servers`.
    pub fn set_ping_timeout(&mut self, timeout: Duration) {
        self.ping_timeout = timeout;
    }

    // Returns the normalized master server base URL (without trailing slash).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // Returns a reference to the player's Ed25519 signing identity.
    pub fn identity(&self) -> &PlayerIdentity {
        &self.identity
    }

    // Fetches `GET /v1/servers` without probing TCP latency.
    pub fn list_servers_raw(&self) -> Result<Vec<ServerEntry>, String> {
        let data: ServersEnvelope = self.request_master("GET", "/v1/servers", None)?;
        Ok(data.servers)
    }

    // Fetches `GET /v1/servers` and measures TCP handshake latency to each
    // listed server (`listServers` in `electron/network.js`).
    pub fn list_servers(&self) -> Result<Vec<ServerEntry>, String> {
        let servers = self.list_servers_raw()?;
        Ok(with_latency(servers, self.ping_timeout))
    }

    // Fetches `GET /v1/servers/:id` (`serverInfo` in `electron/network.js`).
    pub fn server_info(&self, server_id: &str) -> Result<ServerEntry, String> {
        if !is_lower_hex_64(server_id) {
            return Err("Invalid server ID".to_string());
        }
        let route = format!("/v1/servers/{server_id}");
        let env: SingleServerEnvelope = self.request_master("GET", &route, None)?;
        Ok(env.server)
    }

    // Requests short-lived TURN relay credentials (`POST /v1/relay-credentials`),
    // optionally authorizing the peer->server TURN route for `server_id`.
    // Ports `relayCredentials(serverId)` in `electron/network.js`.
    pub fn relay_credentials(&self, server_id: Option<&str>) -> Result<RelayCredentials, String> {
        let payload = match server_id {
            Some(id) => {
                if !is_lower_hex_64(id) {
                    return Err("Invalid server ID".to_string());
                }
                serde_json::json!({ "serverId": id }).to_string()
            }
            None => "{}".to_string(),
        };
        self.request_master("POST", "/v1/relay-credentials", Some(&payload))
    }

    // Requests a one-time WebSocket signaling ticket (`POST /v1/signal-ticket`)
    // used when joining `relayPolicy: "auto"` or `"always"` servers.
    // Ports `signalTicket()` in `electron/network.js`.
    pub fn signal_ticket(&self) -> Result<SignalTicket, String> {
        self.request_master("POST", "/v1/signal-ticket", Some("{}"))
    }

    // Executes a signed master HTTP request, retrying idempotent `GET` requests
    // once after `self.retry_delay` (750ms) when a transport error occurs.
    // Non-idempotent `POST` requests and explicit HTTP 4xx/5xx responses fail
    // immediately without retrying, matching `request()` in `electron/network.js`.
    fn request_master<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        route: &str,
        body: Option<&str>,
    ) -> Result<T, String> {
        let attempts = if method.eq_ignore_ascii_case("GET") {
            2
        } else {
            1
        };
        let mut last_err = String::new();
        for attempt in 0..attempts {
            match self.send_once(method, route, body) {
                Ok(parsed) => return Ok(parsed),
                Err(RequestOutcome::HttpError(msg)) => return Err(msg),
                Err(RequestOutcome::TransportError(msg)) => {
                    last_err = msg;
                    if attempt + 1 < attempts && !self.retry_delay.is_zero() {
                        std::thread::sleep(self.retry_delay);
                    }
                }
            }
        }
        Err(last_err)
    }

    // Sends a single HTTP attempt with a freshly generated timestamp and 16-byte
    // random nonce (`sign_master_request`), ensuring a retried `GET` never trips
    // the master server's replay-protection nonce cache.
    fn send_once<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        route: &str,
        body: Option<&str>,
    ) -> Result<T, RequestOutcome> {
        let payload_bytes = body.map(str::as_bytes).unwrap_or(b"");
        let signed = self
            .identity
            .sign_master_request(method, route, payload_bytes)
            .map_err(RequestOutcome::HttpError)?;
        let url = format!("{}{}", self.base_url, route);

        // Attach the 5 signed `x-bfe-*` headers (`x-bfe-key`, `x-bfe-time`,
        // `x-bfe-nonce`, `x-bfe-signature`, `x-bfe-notice`) and dispatch either
        // a bodyless `GET` or a JSON `POST` (`electron/network.js::request`).
        let mut response = if method.eq_ignore_ascii_case("GET") {
            let mut req = self.agent.get(&url).header("accept", "application/json");
            for (k, v) in signed.as_pairs() {
                req = req.header(k, v);
            }
            req.call()
                .map_err(|e| RequestOutcome::TransportError(unreachable_error(&self.base_url, &e.to_string())))?
        } else {
            let mut req = self
                .agent
                .post(&url)
                .header("accept", "application/json")
                .header("content-type", "application/json");
            for (k, v) in signed.as_pairs() {
                req = req.header(k, v);
            }
            req.send(payload_bytes)
                .map_err(|e| RequestOutcome::TransportError(unreachable_error(&self.base_url, &e.to_string())))?
        };

        // Read the response body with a strict 128 KiB cap (`MAX_MASTER_RESPONSE_BYTES`)
        // so an oversized or runaway upstream response is rejected immediately
        // as a non-retryable `HttpError("Master response was too large")`.
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_MASTER_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|e| {
                if e.to_string().to_ascii_lowercase().contains("limit") {
                    RequestOutcome::HttpError("Master response was too large".to_string())
                } else {
                    RequestOutcome::TransportError(unreachable_error(&self.base_url, &e.to_string()))
                }
            })?;

        // On HTTP 4xx/5xx, prefer the master's structured `{ "error": "..." }`
        // message (`result.error || ...` in `electron/network.js`), falling back
        // to `"Request failed (<status>)"` when the body is not a JSON error object.
        if status >= 400 {
            if let Ok(env) = serde_json::from_str::<ErrorEnvelope>(&text) {
                if let Some(msg) = env.error.filter(|s| !s.is_empty()) {
                    return Err(RequestOutcome::HttpError(msg));
                }
            }
            return Err(RequestOutcome::HttpError(format!(
                "Request failed ({status})"
            )));
        }

        // Parse the 2xx response body into the caller's envelope type `T`.
        serde_json::from_str::<T>(&text)
            .map_err(|_| RequestOutcome::HttpError("Master returned an invalid response".to_string()))
    }
}

// Distinguishes authoritative HTTP/application errors (which must not be retried)
// from transient socket/TLS transport failures (which `GET` retries once).
enum RequestOutcome {
    HttpError(String),
    TransportError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{ED25519_SPKI_PREFIX, ONLINE_NOTICE_VERSION};
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    // Minimal HTTP/1.1 request reader for loopback mock master tests: reads
    // until `\r\n\r\n`, parses the request line and lowercase header map, and
    // reads the exact `Content-Length` body bytes.
    type ParsedHttpRequest = (String, String, HashMap<String, String>, Vec<u8>);

    fn read_http_request(stream: &mut std::net::TcpStream) -> Result<ParsedHttpRequest, String> {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut raw = Vec::new();
        let mut buf = [0u8; 1024];
        let header_end = loop {
            if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
            let n = stream
                .read(&mut buf)
                .map_err(|e| format!("read request: {e}"))?;
            assert!(n > 0, "unexpected EOF reading headers");
            raw.extend_from_slice(&buf[..n]);
        };
        // Parse the HTTP/1.1 request line (`<METHOD> <route> HTTP/1.1`) and
        // normalize header names to lowercase so `verify_master_headers` can
        // look up `x-bfe-*` and `content-length` case-insensitively.
        let header_str =
            std::str::from_utf8(&raw[..header_end]).map_err(|e| format!("header utf8: {e}"))?;
        let mut lines = header_str.split("\r\n");
        let req_line = lines.next().ok_or("missing request line")?;
        let mut parts = req_line.split_whitespace();
        let method = parts.next().ok_or("missing method")?.to_string();
        let route = parts.next().ok_or("missing route")?.to_string();
        let mut headers = HashMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        // Read the remaining request body bytes advertised by `Content-Length`
        // so `POST /v1/relay-credentials` and `POST /v1/signal-ticket` payloads
        // can be hash-checked by `verify_master_headers`.
        let content_len: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let body_start = header_end + 4;
        while raw.len() - body_start < content_len {
            let n = stream
                .read(&mut buf)
                .map_err(|e| format!("read body: {e}"))?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
        }
        let body = raw[body_start..body_start + content_len].to_vec();
        Ok((method, route, headers, body))
    }

    // Reconstructs the master server's authentication check from
    // `battlefront-eternal-master/src/auth.js`:
    // 1. Verifies `x-bfe-notice == ONLINE_NOTICE_VERSION`.
    // 2. Decodes the 44-byte SPKI DER `x-bfe-key` and checks `ED25519_SPKI_PREFIX`.
    // 3. Recomputes `{method}\n{route}\n{time}\n{nonce}\n{sha256_hex(body)}` and
    //    verifies the Ed25519 `x-bfe-signature` against the extracted public key.
    fn verify_master_headers(
        method: &str,
        route: &str,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<(), String> {
        assert_eq!(
            headers.get("x-bfe-notice").map(String::as_str),
            Some(ONLINE_NOTICE_VERSION)
        );
        let key_b64 = headers.get("x-bfe-key").ok_or("missing x-bfe-key")?;
        let time = headers.get("x-bfe-time").ok_or("missing x-bfe-time")?;
        let nonce = headers.get("x-bfe-nonce").ok_or("missing x-bfe-nonce")?;
        let sig_b64 = headers
            .get("x-bfe-signature")
            .ok_or("missing x-bfe-signature")?;
        assert_eq!(nonce.len(), 32);

        let spki = BASE64
            .decode(key_b64)
            .map_err(|e| format!("decode spki: {e}"))?;
        assert_eq!(spki.len(), 44);
        assert_eq!(&spki[..12], &ED25519_SPKI_PREFIX);
        let mut pub_bytes = [0u8; 32];
        pub_bytes.copy_from_slice(&spki[12..]);
        let verifying = VerifyingKey::from_bytes(&pub_bytes)
            .map_err(|e| format!("valid verifying key: {e}"))?;

        // Reconstruct the canonical newline-delimited signing preimage
        // (`{method}\n{route}\n{time}\n{nonce}\n{sha256_hex(body)}`) and verify
        // the Base64 Ed25519 signature against the SPKI public key.
        let body_hash = crate::identity::hex_lower(&Sha256::digest(body));
        let msg = format!("{method}\n{route}\n{time}\n{nonce}\n{body_hash}");
        let sig_bytes = BASE64
            .decode(sig_b64)
            .map_err(|e| format!("decode signature: {e}"))?;
        let sig = Signature::from_slice(&sig_bytes)
            .map_err(|e| format!("signature length: {e}"))?;
        verifying
            .verify(msg.as_bytes(), &sig)
            .map_err(|e| format!("master signature must verify: {e}"))?;
        Ok(())
    }

    // End-to-end test against a local TCP mock master verifying signed
    // `GET /v1/servers`, `POST /v1/relay-credentials`, and `POST /v1/signal-ticket`
    // requests, SPKI + Ed25519 signature verification, and JSON response parsing.
    #[test]
    fn signs_and_executes_list_servers_relay_credentials_and_signal_ticket(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let server_id = "cd".repeat(32);
        let expected_server_id = server_id.clone();

        // Spawn a sequential 3-request mock master server on `127.0.0.1:0`
        // handling `GET /v1/servers`, `POST /v1/relay-credentials`, and
        // `POST /v1/signal-ticket` in order while verifying Ed25519 signatures.
        let handle = std::thread::spawn(move || -> Result<(), String> {
            // 1. GET /v1/servers: verify signed headers over an empty body and
            // return a single-server directory listing.
            {
                let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
                let (method, route, headers, body) = read_http_request(&mut stream)?;
                assert_eq!(method, "GET");
                assert_eq!(route, "/v1/servers");
                verify_master_headers(&method, &route, &headers, &body)?;
                let json = serde_json::json!({
                    "servers": [{
                        "id": expected_server_id,
                        "name": "Official Cloud City",
                        "players": 12,
                        "maxPlayers": 32,
                        "location": "US-East",
                        "country": "US",
                        "relayPolicy": "auto",
                        "endpoints": [],
                        "battle": {
                            "map": "bes2",
                            "mapName": "Bespin: Cloud City",
                            "era": "gcw",
                            "mode": "conquest"
                        }
                    }]
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json.len(),
                    json
                );
                stream
                    .write_all(resp.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            // 2. POST /v1/relay-credentials: verify signed headers over the JSON
            // `{"serverId": ...}` body and return coturn HMAC credentials.
            {
                let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
                let (method, route, headers, body) = read_http_request(&mut stream)?;
                assert_eq!(method, "POST");
                assert_eq!(route, "/v1/relay-credentials");
                verify_master_headers(&method, &route, &headers, &body)?;
                let parsed: serde_json::Value =
                    serde_json::from_slice(&body).map_err(|e| e.to_string())?;
                assert_eq!(
                    parsed.get("serverId").and_then(|v| v.as_str()),
                    Some(expected_server_id.as_str())
                );
                let json = serde_json::json!({
                    "urls": ["turn:turn.battlefronteternal.com:3478?transport=udp"],
                    "username": "1760003600:peer1",
                    "credential": "hmac-secret",
                    "ttl": 3600
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json.len(),
                    json
                );
                stream
                    .write_all(resp.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            // 3. POST /v1/signal-ticket: verify signed headers over `{}` and
            // return a short-lived WebSocket signaling ticket.
            {
                let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
                let (method, route, headers, body) = read_http_request(&mut stream)?;
                assert_eq!(method, "POST");
                assert_eq!(route, "/v1/signal-ticket");
                verify_master_headers(&method, &route, &headers, &body)?;
                let json = serde_json::json!({
                    "ticket": "ef".repeat(24),
                    "expires": 1_760_000_060_000u64
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json.len(),
                    json
                );
                stream
                    .write_all(resp.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        });

        let id = PlayerIdentity::from_seed([7u8; 32]);
        let client = MasterClient::with_options(
            format!("http://{addr}"),
            id,
            Duration::from_secs(2),
            Duration::from_millis(10),
            MasterIpFamily::Ipv4Only,
        );

        // 1. Verify `list_servers` returns the mock server and leaves `latency_ms`
        // as `None` because `endpoints` is empty (relay-capable server).
        let servers = client.list_servers()?;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, server_id);
        assert_eq!(servers[0].name, "Official Cloud City");
        assert_eq!(servers[0].latency_ms, None);

        // 2. Verify `relay_credentials` posts `{"serverId": ...}` and parses the
        // returned TURN URLs, username, HMAC credential, and TTL.
        let creds = client.relay_credentials(Some(&server_id))?;
        assert_eq!(
            creds.urls,
            vec!["turn:turn.battlefronteternal.com:3478?transport=udp"]
        );
        assert_eq!(creds.username, "1760003600:peer1");
        assert_eq!(creds.credential, "hmac-secret");
        assert_eq!(creds.ttl, 3600);

        // 3. Verify `signal_ticket` posts `{}` and parses the one-time ticket.
        let ticket = client.signal_ticket()?;
        assert_eq!(ticket.ticket, "ef".repeat(24));

        // Ensure the background mock server thread completed all 3 assertions cleanly.
        handle
            .join()
            .map_err(|_| "mock master thread panicked")??;
        Ok(())
    }

    // Verifies that when the first `GET /v1/servers` connection drops before
    // sending an HTTP response, `MasterClient` waits `retry_delay` and issues a
    // second `GET` signed with a fresh `x-bfe-nonce`.
    #[test]
    fn retries_get_once_with_fresh_nonce_after_connection_drop(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;

        let handle = std::thread::spawn(move || -> Result<(), String> {
            // 1st attempt: accept the TCP connection, read the signed `GET`
            // headers to capture the first `x-bfe-nonce`, and drop the socket
            // without writing any HTTP status line so `ureq` sees an EOF error.
            let first_nonce = {
                let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
                let (_, _, headers, _) = read_http_request(&mut stream)?;
                headers
                    .get("x-bfe-nonce")
                    .ok_or("missing x-bfe-nonce")?
                    .clone()
            };
            // 2nd attempt: accept the retried `GET`, verify the Ed25519 signature,
            // and assert that `MasterClient` generated a fresh `x-bfe-nonce`
            // rather than re-sending the first attempt's nonce.
            {
                let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
                let (method, route, headers, body) = read_http_request(&mut stream)?;
                verify_master_headers(&method, &route, &headers, &body)?;
                let second_nonce = headers.get("x-bfe-nonce").ok_or("missing x-bfe-nonce")?;
                assert_ne!(
                    &first_nonce, second_nonce,
                    "retry must generate a fresh nonce"
                );
                let json = r#"{"servers":[]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json.len(),
                    json
                );
                stream
                    .write_all(resp.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        });

        // Configure a short 20ms `retry_delay` so the GET retry completes quickly
        // after the 1st connection is dropped by the mock listener.
        let id = PlayerIdentity::from_seed([9u8; 32]);
        let client = MasterClient::with_options(
            format!("http://{addr}"),
            id,
            Duration::from_secs(2),
            Duration::from_millis(20),
            MasterIpFamily::Ipv4Only,
        );
        let servers = client.list_servers()?;
        assert!(servers.is_empty());
        handle
            .join()
            .map_err(|_| "mock master thread panicked")??;
        Ok(())
    }
}
