//! LAN discovery responder and scanner (and `startLanDiscovery`).

use bfe_transport::crypto::random;
use bfe_transport::server::normalize_ip;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Default UDP port for `BFE-LAN/1` discovery (`PORT = 18452`).
pub const LAN_PORT: u16 = 18452;
/// Protocol magic string for LAN discovery datagrams (`MAGIC = "BFE-LAN/1"`).
pub const LAN_MAGIC: &str = "BFE-LAN/1";
/// IPv6 link-local multicast group for LAN discovery (`MULTICAST_V6 = "ff02::bf:1"`).
pub const LAN_MULTICAST_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0x00bf, 0x0001);
/// Maximum UDP datagram payload size accepted or sent by LAN discovery (`MAX_PACKET = 2048`).
const MAX_PACKET: usize = 2048;

/// Encodes a byte slice as lowercase hexadecimal (`randomBytes(16).toString('hex')`).
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Validates a 32-character lowercase hex discovery nonce (`validNonce`:
/// `/^[a-f0-9]{32}$/.test(nonce)`).
pub fn valid_nonce(nonce: &str) -> bool {
    nonce.len() == 32 && nonce.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Enumerates local network interface indices for IPv6 link-local multicast membership
/// (`ipv6MulticastInterfaces`).
///
/// Reads `/sys/class/net/<iface>/ifindex` using safe standard-library filesystem I/O (no `unsafe`
/// FFI calls), always including index `0` (the kernel default multicast interface).
fn ipv6_multicast_interfaces() -> Vec<u32> {
    let mut indices = vec![0u32];
    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            if let Ok(raw) = fs::read_to_string(entry.path().join("ifindex")) {
                if let Ok(idx) = raw.trim().parse::<u32>() {
                    if idx != 0 && !indices.contains(&idx) {
                        indices.push(idx);
                    }
                }
            }
        }
    }
    indices
}

/// Validates and sanitizes a `BFE-LAN/1` server response packet (`export function cleanServer(m, rinfo, nonce)`).
pub fn clean_lan_server(msg: &Value, rinfo: SocketAddr, nonce: &str) -> Option<Value> {
    let obj = msg.as_object()?;
    if obj.get("magic").and_then(Value::as_str) != Some(LAN_MAGIC) {
        return None;
    }
    if obj.get("type").and_then(Value::as_str) != Some("server") {
        return None;
    }
    if obj.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return None;
    }
    let info = obj.get("info")?.as_object()?;
    let id = info.get("id")?.as_str()?;
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return None;
    }
    let port = info.get("port")?.as_u64()?;
    if !(1..=65535).contains(&port) {
        return None;
    }
    let protocol = info.get("protocol")?.as_u64()?;
    let max_players = info.get("maxPlayers")?.as_u64()?;
    if !(1..=256).contains(&max_players) {
        return None;
    }
    let players = info
        .get("players")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(max_players);
    let rotation_arr = info.get("rotation")?.as_array()?;
    if rotation_arr.is_empty() {
        return None;
    }
    let mut rotation = Vec::with_capacity(rotation_arr.len().min(32));
    for r in rotation_arr.iter().take(32) {
        rotation.push(Value::String(r.as_str()?.chars().take(64).collect()));
    }
    let name: String = info
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("LAN Server")
        .chars()
        .take(80)
        .collect();
    let profile: String = info
        .get("profile")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(64)
        .collect();
    let simulation: String = info
        .get("simulation")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(64)
        .collect();
    let content: String = info
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(64)
        .collect();
    let fingerprint: String = info
        .get("fingerprint")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(128)
        .collect();
    let query: String = info
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(128)
        .collect();
    let ip = normalize_ip(rinfo.ip());
    let address = match ip {
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
        IpAddr::V4(v4) => format!("{v4}:{port}"),
    };
    let mut out = Map::new();
    out.insert("id".to_string(), Value::String(id.to_string()));
    out.insert("name".to_string(), Value::String(name));
    out.insert("protocol".to_string(), json!(protocol));
    out.insert("profile".to_string(), Value::String(profile));
    out.insert("simulation".to_string(), Value::String(simulation));
    out.insert("content".to_string(), Value::String(content));
    out.insert("fingerprint".to_string(), Value::String(fingerprint));
    out.insert("query".to_string(), Value::String(query));
    out.insert("rotation".to_string(), Value::Array(rotation));
    out.insert("players".to_string(), json!(players));
    out.insert("maxPlayers".to_string(), json!(max_players));
    out.insert("relayPolicy".to_string(), Value::String("never".to_string()));
    out.insert("port".to_string(), json!(port));
    out.insert("transport".to_string(), Value::String("udp".to_string()));
    out.insert("address".to_string(), Value::String(address));
    out.insert("lan".to_string(), Value::Bool(true));
    if let Some(loc) = info.get("location").and_then(Value::as_str) {
        let cleaned: String = loc
            .chars()
            .map(|c| {
                let code = c as u32;
                if code <= 0x1f || (0x7f..=0x9f).contains(&code) {
                    ' '
                } else {
                    c
                }
            })
            .collect::<String>()
            .trim()
            .chars()
            .take(64)
            .collect();
        if !cleaned.is_empty() {
            out.insert("location".to_string(), Value::String(cleaned));
        }
    }
    Some(Value::Object(out))
}

/// Strips `description` and `sponsor` from `/info` for LAN responses (`getInfo` in `startLocalServer`).
///
/// A LAN discovery reply must fit in a single UDP datagram (`MAX_PACKET = 2048`), so the longer
/// `description` and `sponsor` fields remain on HTTP `GET /info` only.
pub fn lan_info(info: &Value) -> Value {
    let Some(obj) = info.as_object() else {
        return info.clone();
    };
    let mut clean = obj.clone();
    clean.remove("description");
    clean.remove("sponsor");
    Value::Object(clean)
}

/// Dual-stack `BFE-LAN/1` UDP discovery responder (`export async function startLanDiscovery`).
/// Dual-stack LAN discovery responder porting `startLanResponder(port, infoFn)`.
pub struct LanDiscoveryResponder {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl LanDiscoveryResponder {
    /// Starts the dual-stack LAN discovery responder on `port` (default `18452`, `startLanDiscovery`).
    ///
    /// Binds an IPv4 broadcast socket (`0.0.0.0:port`, `SO_BROADCAST`) and an IPv6-only socket
    /// (`[::]:port`, `IPV6_V6ONLY = 1`) joined to `ff02::bf:1` on every local interface.
    pub fn start<F>(port: u16, info_fn: F) -> Self
    where
        F: Fn() -> Value + Send + Sync + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let info_fn = Arc::new(info_fn);
        let recent: Arc<Mutex<HashMap<IpAddr, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
        let mut threads = Vec::new();

        // IPv4 socket (`createSocket({ type: 'udp4', reuseAddr: true })`).
        if let Ok(sock4) = UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            port,
        ))) {
            let _ = sock4.set_broadcast(true);
            let _ = sock4.set_read_timeout(Some(Duration::from_millis(100)));
            let stop_c = Arc::clone(&stop);
            let info_c = Arc::clone(&info_fn);
            let recent_c = Arc::clone(&recent);
            threads.push(thread::spawn(move || {
                responder_loop(sock4, stop_c, info_c, recent_c);
            }));
        }

        // IPv6 socket (`createSocket({ type: 'udp6', reuseAddr: true, ipv6Only: true })`).
        if let Ok(sock6) = bind_ipv6_only(port) {
            let _ = sock6.set_read_timeout(Some(Duration::from_millis(100)));
            for idx in ipv6_multicast_interfaces() {
                let _ = sock6.join_multicast_v6(&LAN_MULTICAST_V6, idx);
            }
            let stop_c = Arc::clone(&stop);
            let info_c = Arc::clone(&info_fn);
            let recent_c = Arc::clone(&recent);
            threads.push(thread::spawn(move || {
                responder_loop(sock6, stop_c, info_c, recent_c);
            }));
        }

        Self { stop, threads }
    }

    /// Stops both responder threads and releases the UDP discovery sockets (`close()`).
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for LanDiscoveryResponder {
    fn drop(&mut self) {
        self.close();
    }
}

/// Binds a UDPv6 socket on `[::]:port` and sets `IPV6_V6ONLY = 1` so it can coexist with the
/// IPv4 responder bound on `0.0.0.0:port` (`ipv6Only: true`).
fn bind_ipv6_only(port: u16) -> std::io::Result<UdpSocket> {
    crate::sockets::udp(
        SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0)),
        true,
    )
}

/// Per-socket receive loop for `LanDiscoveryResponder` (`onMessage` in `startLanDiscovery`).
///
/// Enforces:
/// - `1..=MAX_PACKET` byte length check.
/// - Per-IP 250 ms cooldown (`now - prev < 250 ms`) and 1024-entry table cap (`10 s` expiry).
/// - `magic == "BFE-LAN/1"`, `type == "discover"`, and `valid_nonce(nonce)`.
fn responder_loop<F>(
    socket: UdpSocket,
    stop: Arc<AtomicBool>,
    info_fn: Arc<F>,
    recent: Arc<Mutex<HashMap<IpAddr, Instant>>>,
) where
    F: Fn() -> Value + Send + Sync + 'static,
{
    let mut buf = [0u8; MAX_PACKET + 1];
    while !stop.load(Ordering::Relaxed) {
        let (len, rinfo) = match crate::sockets::recv_from(&socket, &mut buf) {
            Ok(pair) => pair,
            Err(e)
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => break,
        };
        // Ignore empty or oversize datagrams (`msg.length > MAX_PACKET`).
        if len == 0 || len > MAX_PACKET {
            continue;
        }
        let now = Instant::now();
        let ip = normalize_ip(rinfo.ip());
        // Enforce 250 ms per-source-IP rate limit and 1024-entry recent map bound.
        {
            let mut guard = recent.lock().unwrap();
            if let Some(&prev) = guard.get(&ip) {
                if now.duration_since(prev) < Duration::from_millis(250) {
                    continue;
                }
            }
            guard.insert(ip, now);
            if guard.len() > 1024 {
                guard.retain(|_, &mut t| now.duration_since(t) < Duration::from_secs(10));
            }
        }
        let Ok(msg) = serde_json::from_slice::<Value>(&buf[..len]) else {
            continue;
        };
        let Some(obj) = msg.as_object() else {
            continue;
        };
        if obj.get("magic").and_then(Value::as_str) != Some(LAN_MAGIC)
            || obj.get("type").and_then(Value::as_str) != Some("discover")
        {
            continue;
        }
        let Some(nonce) = obj.get("nonce").and_then(Value::as_str) else {
            continue;
        };
        if !valid_nonce(nonce) {
            continue;
        }
        // Reply with `lan_info(&info_fn())` echoing the caller's 32-hex `nonce`.
        let packet = json!({
            "magic": LAN_MAGIC,
            "type": "server",
            "nonce": nonce,
            "info": lan_info(&info_fn()),
        });
        if let Ok(bytes) = serde_json::to_vec(&packet) {
            if bytes.len() <= MAX_PACKET {
                crate::sockets::send_to(&socket, &bytes, rinfo);
            }
        }
    }
}

/// Discovers LAN servers via IPv4 broadcast + loopback and IPv6 multicast + loopback
/// (`export async function discoverLan`).
pub fn discover_lan(timeout_ms: u64, port: u16) -> Vec<Value> {
    let nonce_bytes = random::<16>().unwrap_or([0x42; 16]);
    let nonce = hex_encode(&nonce_bytes);
    let packet = serde_json::to_vec(&json!({
        "magic": LAN_MAGIC,
        "type": "discover",
        "nonce": nonce,
    }))
    .unwrap_or_default();

    // Send IPv4 discovery probes to `255.255.255.255:port` and `127.0.0.1:port`.
    let sock4 = UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))).ok();
    if let Some(s4) = &sock4 {
        let _ = s4.set_broadcast(true);
        let _ = s4.set_nonblocking(true);
        crate::sockets::send_to(
            s4,
            &packet,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, port)),
        );
        crate::sockets::send_to(
            s4,
            &packet,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
        );
    }

    // Send IPv6 discovery probes to `[::1]:port` and `[ff02::bf:1]:port` on each scope index.
    let sock6 = UdpSocket::bind(SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::UNSPECIFIED,
        0,
        0,
        0,
    )))
    .ok();
    if let Some(s6) = &sock6 {
        let _ = s6.set_nonblocking(true);
        crate::sockets::send_to(
            s6,
            &packet,
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, port, 0, 0)),
        );
        for scope in ipv6_multicast_interfaces() {
            crate::sockets::send_to(
                s6,
                &packet,
                SocketAddr::V6(SocketAddrV6::new(LAN_MULTICAST_V6, port, 0, scope)),
            );
        }
    }

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut found: HashMap<String, Value> = HashMap::new();
    let mut buf = [0u8; MAX_PACKET + 1];

    while Instant::now() < deadline {
        let mut received_any = false;
        for sock in [sock4.as_ref(), sock6.as_ref()].into_iter().flatten() {
            while let Ok((len, rinfo)) = crate::sockets::recv_from(sock, &mut buf) {
                received_any = true;
                if len == 0 || len > MAX_PACKET {
                    continue;
                }
                let Ok(msg) = serde_json::from_slice::<Value>(&buf[..len]) else {
                    continue;
                };
                let Some(server) = clean_lan_server(&msg, rinfo, &nonce) else {
                    continue;
                };
                let Some(id) = server.get("id").and_then(Value::as_str) else {
                    continue;
                };
                // Prefer IPv4 address (`prev.address.startsWith("[")`) when a dual-stack server answers on both sockets.
                let is_v4 = !rinfo.ip().is_ipv6()
                    || matches!(rinfo.ip(), IpAddr::V6(v6) if v6.to_ipv4_mapped().is_some());
                let replace = match found.get(id) {
                    None => true,
                    Some(prev) => {
                        let prev_addr = prev
                            .get("address")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        is_v4 && prev_addr.starts_with('[')
                    }
                };
                if replace {
                    found.insert(id.to_string(), server);
                }
            }
        }
        if !received_any {
            thread::sleep(Duration::from_millis(10));
        }
    }

    found.into_values().collect()
}
