// Joining a listed server that names no direct address (relay policy "auto"
// or "always"). The player and the server exchange UDP candidates through
// the master's signalling socket; both then send at once, which opens most
// home routers (hole punching). A server behind a router that cannot be
// opened adds a relayed address on the official relay as a later candidate.
//
// Wire shapes (as the 2025 desktop client and bfeserver use them):
//   to the master:   {"to": <server id>, "type": "offer", "payload": <json text>}
//   offer payload:   {"udp": 1, "nonce": <32 hex>, "candidates": [{address, port, kind}]}
//   from the master: {"from": <server id>, "type": "answer" | "candidate", "payload": <json text>}

use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::identity::is_lower_hex_64;
use crate::master::MasterClient;

// Default STUN server for the player's public address when the master
// issues no relay URL (the official relay answers STUN on 3478).
pub const DEFAULT_STUN: &str = "relay.battlefronteternal.com:3478";

// At most eight candidates cross the master in either direction.
const MAX_CANDIDATES: usize = 8;

// A signalling message larger than the master allows is not ours.
const MAX_SIGNAL_BYTES: usize = 32 * 1024;

/// One UDP address offered to the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub address: SocketAddr,
    /// "host" (a local interface), "srflx" (as the STUN server saw us) or
    /// "relay" (an address on the relay).
    pub kind: &'static str,
}

/// The JSON text of an offer carrying `candidates`.
pub fn offer_payload(nonce: &str, candidates: &[Candidate]) -> String {
    let list: Vec<Value> = candidates
        .iter()
        .take(MAX_CANDIDATES)
        .map(|c| {
            json!({
                "address": c.address.ip().to_string(),
                "port": c.address.port(),
                "kind": c.kind,
            })
        })
        .collect();
    json!({ "udp": 1, "nonce": nonce, "candidates": list }).to_string()
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    from: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    payload: String,
}

#[derive(Deserialize)]
struct WireCandidate {
    address: String,
    port: u16,
}

#[derive(Deserialize)]
struct Answer {
    udp: u8,
    nonce: String,
    #[serde(default)]
    candidates: Vec<WireCandidate>,
}

/// The candidates of a signalling message from the master when it is the
/// server's answer (or later candidate) to one of our offers; `None` for
/// anything else. Addresses that are not plain IPs or have port 0 are dropped.
pub fn parse_answer(text: &str, server_id: &str, nonces: &[String]) -> Option<Vec<SocketAddr>> {
    if text.len() > MAX_SIGNAL_BYTES {
        return None;
    }
    let envelope: Envelope = serde_json::from_str(text).ok()?;
    if !envelope.from.eq_ignore_ascii_case(server_id)
        || !matches!(envelope.kind.as_str(), "answer" | "candidate")
    {
        return None;
    }
    let answer: Answer = serde_json::from_str(&envelope.payload).ok()?;
    if answer.udp != 1 || !nonces.contains(&answer.nonce) {
        return None;
    }
    Some(
        answer
            .candidates
            .iter()
            .take(MAX_CANDIDATES)
            .filter(|c| c.port != 0)
            .filter_map(|c| {
                let ip: IpAddr = c.address.split('%').next()?.parse().ok()?;
                (!ip.is_unspecified() && !ip.is_multicast()).then_some(SocketAddr::new(ip, c.port))
            })
            .collect(),
    )
}

/// A fresh 32-hex offer nonce.
pub fn new_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `host:port` of the first `turn:` URL the master issued, for STUN.
pub fn stun_from_turn_urls(urls: &[String]) -> Option<String> {
    urls.iter().find_map(|url| {
        let rest = url.strip_prefix("turn:")?;
        let authority = rest.split('?').next()?;
        authority.rsplit_once(':')?.1.parse::<u16>().ok()?;
        Some(authority.to_string())
    })
}

/// This player's public address as the STUN server at `stun` sees `socket`.
/// The socket must be blocking or have a read timeout; replies from anyone
/// else are skipped.
pub fn reflexive(socket: &UdpSocket, stun: SocketAddr, timeout: Duration) -> Option<SocketAddr> {
    let transaction = bfe_transport::stun::new_transaction().ok()?;
    let request = bfe_transport::stun::encode_binding_request(&transaction);
    let deadline = Instant::now() + timeout;
    let mut buffer = [0u8; 1500];
    let mut next_send = Instant::now();
    while Instant::now() < deadline {
        if Instant::now() >= next_send {
            let _ = socket.send_to(&request, stun);
            next_send = Instant::now() + Duration::from_millis(500);
        }
        let _ = socket.set_read_timeout(Some(Duration::from_millis(100)));
        let Ok((n, from)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        if from != stun {
            continue;
        }
        let Some(message) = bfe_transport::stun::decode_message(&buffer[..n]) else {
            continue;
        };
        if message.klass == bfe_transport::stun::class::SUCCESS && message.transaction == transaction {
            return message.mapped_address();
        }
    }
    None
}

/// The local interface address the system would use to reach `toward`
/// (no packet is sent: connecting a UDP socket only picks a route).
pub fn local_address(toward: SocketAddr) -> Option<IpAddr> {
    let probe = UdpSocket::bind(if toward.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" }).ok()?;
    probe.connect(toward).ok()?;
    let ip = probe.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

/// The master's signalling socket for one join.
pub struct Signalling {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
}

impl Signalling {
    /// Takes a one-time ticket from the master and opens `/v1/signal`.
    pub fn open(master: &MasterClient) -> Result<Self, String> {
        let ticket = master.signal_ticket()?;
        let url = signal_url(master.base_url(), &ticket.ticket)?;
        let (mut socket, _) =
            tungstenite::connect(url.as_str()).map_err(|e| format!("signalling connect failed: {e}"))?;
        set_read_timeout(&mut socket, Some(Duration::from_millis(50)));
        Ok(Self { socket })
    }

    /// Sends one message to `to` (a server id).
    pub fn send(&mut self, to: &str, kind: &str, payload: &str) -> Result<(), String> {
        if !is_lower_hex_64(to) {
            return Err("Invalid server identity".to_string());
        }
        let text = json!({ "to": to, "type": kind, "payload": payload }).to_string();
        self.socket
            .send(Message::Text(text.into()))
            .map_err(|e| format!("signalling send failed: {e}"))
    }

    /// The next text message, waiting at most about `wait`. `Ok(None)` when
    /// nothing arrived; an error when the master closed the socket.
    pub fn receive(&mut self, wait: Duration) -> Result<Option<String>, String> {
        let deadline = Instant::now() + wait;
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => return Ok(Some(text.as_str().to_string())),
                Ok(Message::Binary(bytes)) => {
                    return Ok(String::from_utf8(bytes.to_vec()).ok());
                }
                Ok(Message::Close(_)) => return Err("the online service closed signalling".to_string()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(format!("signalling failed: {e}")),
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    pub fn close(mut self) {
        let _ = self.socket.close(None);
        let _ = self.socket.flush();
    }
}

fn set_read_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, timeout: Option<Duration>) {
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

/// `https://host` -> `wss://host/v1/signal?ticket=...` (`http` -> `ws`).
pub fn signal_url(base_url: &str, ticket: &str) -> Result<String, String> {
    let (scheme, rest) = base_url
        .split_once("://")
        .ok_or_else(|| "invalid master URL".to_string())?;
    let ws = if scheme == "https" { "wss" } else { "ws" };
    let mut encoded = String::with_capacity(ticket.len());
    for byte in ticket.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(format!("{ws}://{}/v1/signal?ticket={encoded}", rest.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

    #[test]
    fn answers_are_matched_to_the_server_and_our_offer() {
        let nonce = "0123456789abcdef0123456789abcdef".to_string();
        let payload = json!({"udp": 1, "nonce": nonce, "candidates": [
            {"address": "203.0.113.7", "port": 18451, "kind": "srflx"},
            {"address": "2001:db8::7", "port": 18451, "kind": "host"},
            {"address": "0.0.0.0", "port": 18451, "kind": "host"},
            {"address": "203.0.113.8", "port": 0, "kind": "host"},
            {"address": "not-an-ip", "port": 18451, "kind": "host"}
        ]})
        .to_string();
        let message = json!({"from": SERVER, "type": "answer", "payload": payload}).to_string();
        let got = parse_answer(&message, SERVER, std::slice::from_ref(&nonce)).expect("answer");
        assert_eq!(
            got,
            vec![
                "203.0.113.7:18451".parse::<SocketAddr>().unwrap(),
                "[2001:db8::7]:18451".parse::<SocketAddr>().unwrap()
            ]
        );
        // A later relayed candidate arrives as type "candidate".
        let late = json!({"from": SERVER, "type": "candidate", "payload": json!({"udp": 1, "nonce": nonce,
            "candidates": [{"address": "198.51.100.2", "port": 50000, "kind": "relay"}]}).to_string()})
        .to_string();
        assert_eq!(parse_answer(&late, SERVER, std::slice::from_ref(&nonce)).map(|c| c.len()), Some(1));
        // Another server, another nonce, or an offer are not answers to us.
        let other = message.replace(SERVER, &"cd".repeat(32));
        assert!(parse_answer(&other, SERVER, std::slice::from_ref(&nonce)).is_none());
        assert!(parse_answer(&message, SERVER, &["ff".repeat(16)]).is_none());
        let offer = message.replace("\"answer\"", "\"offer\"");
        assert!(parse_answer(&offer, SERVER, std::slice::from_ref(&nonce)).is_none());
    }

    #[test]
    fn offers_carry_at_most_eight_candidates_and_the_wire_kinds() {
        let one = Candidate { address: "192.0.2.1:5000".parse().unwrap(), kind: "srflx" };
        let text = offer_payload("aa", &[one; 12]);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["udp"], 1);
        assert_eq!(value["candidates"].as_array().map(Vec::len), Some(8));
        assert_eq!(value["candidates"][0]["address"], "192.0.2.1");
        assert_eq!(value["candidates"][0]["kind"], "srflx");
    }

    #[test]
    fn stun_server_comes_from_the_relay_url_and_signal_url_follows_the_scheme() {
        let urls = vec![
            "turns:relay.example:5349?transport=tcp".to_string(),
            "turn:relay.example:3478?transport=udp".to_string(),
        ];
        assert_eq!(stun_from_turn_urls(&urls).as_deref(), Some("relay.example:3478"));
        assert_eq!(
            signal_url("https://online.example", "a+b").unwrap(),
            "wss://online.example/v1/signal?ticket=a%2Bb"
        );
        assert_eq!(
            signal_url("http://127.0.0.1:8080", "t").unwrap(),
            "ws://127.0.0.1:8080/v1/signal?ticket=t"
        );
    }

    #[test]
    fn reflexive_address_comes_from_a_binding_reply() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let stun = server.local_addr().unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mapped: SocketAddr = "203.0.113.9:40000".parse().unwrap();
        let worker = std::thread::spawn(move || {
            let mut buffer = [0u8; 1500];
            let (n, from) = server.recv_from(&mut buffer).unwrap();
            let request = bfe_transport::stun::decode_message(&buffer[..n]).unwrap();
            let reply = bfe_transport::stun::encode_binding_response(&request.transaction, mapped);
            server.send_to(&reply, from).unwrap();
        });
        assert_eq!(reflexive(&client, stun, Duration::from_secs(3)), Some(mapped));
        worker.join().unwrap();
    }
}
