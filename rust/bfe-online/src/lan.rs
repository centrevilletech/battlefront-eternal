// LAN server discovery over IPv4 broadcast and IPv6 link-local multicast
// (`BFE-LAN/1` UDP protocol on port 18452).
//
// Ports the desktop host, the desktop host, and
// `battlefront-eternal-server/src/lan.js`.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::browser::ServerEntry;
use crate::identity::{hex_lower, is_lower_hex_64};
use crate::ping::{with_latency, PING_TIMEOUT};

// UDP port for `BFE-LAN/1` discovery beacons (`LAN_PORT` in the desktop host).
pub const LAN_PORT: u16 = 18452;

// Protocol discriminator (`LAN_MAGIC` in the desktop host).
pub const LAN_MAGIC: &str = "BFE-LAN/1";

// IPv6 link-local multicast group `ff02::bf:1` (`LAN_MULTICAST_V6` in
// the desktop host).
pub const LAN_MULTICAST_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0x00bf, 0x0001);

// Default discovery window (`DEFAULT_TIMEOUT_MS = 750` in the desktop host).
pub const DEFAULT_LAN_TIMEOUT: Duration = Duration::from_millis(750);

// Maximum distinct servers collected in one scan (`MAX_SERVERS = 256`).
pub const MAX_LAN_SERVERS: usize = 256;

// Maximum accepted UDP datagram length (`MAX_PACKET = 2048`).
pub const MAX_LAN_PACKET: usize = 2048;

// Configuration for a single `discover_lan` UDP scan (`discoverLan({ port, timeout, targets })`
// in the desktop host).
// - `port`: destination UDP beacon port (`LAN_PORT = 18452` when `0` or default).
// - `timeout`: collection window clamped to `1..=5000` ms (`DEFAULT_LAN_TIMEOUT = 750` ms).
// - `targets`: explicit broadcast/multicast socket addresses (used by unit tests
//   to target loopback responders without OS broadcast privileges), or `None` to
//   enumerate active interfaces via `default_lan_targets(port)`.
// - `measure_ping`: when true, probes TCP connect latency on discovered servers
//   via `with_latency` before returning.
#[derive(Debug, Clone)]
pub struct LanDiscoveryOptions {
    // UDP beacon port (`LAN_PORT = 18452` when `0` or default).
    pub port: u16,
    // Discovery window clamped to `1..=5000` ms (`DEFAULT_LAN_TIMEOUT = 750` ms).
    pub timeout: Duration,
    // Optional explicit target socket addresses (`None` uses `default_lan_targets`).
    pub targets: Option<Vec<SocketAddr>>,
    // Whether to run `with_latency` TCP probes on discovered LAN servers.
    pub measure_ping: bool,
}

impl Default for LanDiscoveryOptions {
    // Defaults to `LAN_PORT` (18452), `DEFAULT_LAN_TIMEOUT` (750 ms), automatic
    // interface broadcast/multicast target enumeration, and TCP ping measurement.
    fn default() -> Self {
        Self {
            port: LAN_PORT,
            timeout: DEFAULT_LAN_TIMEOUT,
            targets: None,
            measure_ping: true,
        }
    }
}

// Wire format of the client -> LAN broadcast probe (`{ magic: "BFE-LAN/1", type: "discover", nonce }`
// in the desktop host).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LanDiscoverPacket<'a> {
    // Protocol discriminator (`"BFE-LAN/1"`).
    magic: &'a str,
    // Packet discriminator (`"discover"`).
    #[serde(rename = "type")]
    kind: &'a str,
    // 32-character lowercase hex challenge nonce echoed in `"server"` replies.
    nonce: &'a str,
}

// Wire format of the server -> client unicast reply (`{ magic: "BFE-LAN/1", type: "server", nonce, info }`
// in the desktop host).
#[derive(Debug, Clone, Deserialize)]
struct LanServerPacket {
    // Protocol discriminator (`"BFE-LAN/1"`).
    magic: String,
    // Packet discriminator (`"server"`).
    #[serde(rename = "type")]
    kind: String,
    // Echoed 32-character lowercase hex challenge nonce from the `"discover"` probe.
    nonce: String,
    // Deserialized server metadata payload.
    info: ServerEntry,
}

// Fallible encoder for a `BFE-LAN/1` discovery request datagram
// (`JSON.stringify({ magic: "BFE-LAN/1", type: "discover", nonce })` in
// the desktop host).
pub fn try_encode_discover_packet(nonce_hex: &str) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&LanDiscoverPacket {
        magic: LAN_MAGIC,
        kind: "discover",
        nonce: nonce_hex,
    })
    .map_err(|e| format!("LanDiscoverPacket.nonce: {e}"))
}

// Encodes a `BFE-LAN/1` discovery request datagram for the given 32-char hex
// nonce, using a direct JSON format fallback so callers never panic.
pub fn encode_discover_packet(nonce_hex: &str) -> Vec<u8> {
    match try_encode_discover_packet(nonce_hex) {
        Ok(bytes) => bytes,
        Err(_) => {
            format!("{{\"magic\":\"{LAN_MAGIC}\",\"type\":\"discover\",\"nonce\":\"{nonce_hex}\"}}")
                .into_bytes()
        }
    }
}

// Validates and decodes a `BFE-LAN/1` server response datagram against
// `expected_nonce` and attaches the responder's source IP address.
//
// Enforces every check from the `socket.on('message')` handler in the desktop host:
// - Datagram size is `1..=MAX_LAN_PACKET` (2048 bytes).
// - `magic == "BFE-LAN/1"`, `type == "server"`, and `nonce == expected_nonce`
//   (preventing off-path or replayed UDP spoofing).
// - `info.id` is a 64-character lowercase hex SHA-256 identity digest.
// - `info.name` is non-empty and at most 80 bytes.
// - `info.port` is a valid TCP/UDP port in `1..=65535`.
// - Overwrites `info.address` with the UDP datagram's source IP (`remote.address`).
pub fn parse_server_packet(
    datagram: &[u8],
    expected_nonce: &str,
    remote_ip: IpAddr,
) -> Option<ServerEntry> {
    if datagram.is_empty() || datagram.len() > MAX_LAN_PACKET {
        return None;
    }
    let msg: LanServerPacket = serde_json::from_slice(datagram).ok()?;
    if msg.magic != LAN_MAGIC || msg.kind != "server" || msg.nonce != expected_nonce {
        return None;
    }
    let mut info = msg.info;
    if !is_lower_hex_64(&info.id) {
        return None;
    }
    if info.name.is_empty() || info.name.len() > 80 {
        return None;
    }
    match info.port {
        Some(1..=u16::MAX) => {}
        _ => return None,
    }
    info.address = Some(remote_ip.to_string());
    Some(info)
}

// Enumerates IPv4 broadcast targets (`Ipv4Addr::BROADCAST` plus per-interface
// directed subnet broadcasts) and IPv6 link-local multicast targets
// (`[ff02::bf:1%iface]:port`). Ports `broadcastTargets` and `multicastTargets6`
// from the desktop host.
//
// On Unix (`cfg(unix)`), active non-loopback interfaces are enumerated via
// `libc::getifaddrs` to emit directed IPv4 subnet broadcasts and per-interface
// IPv6 scope IDs. On Windows and other non-Unix platforms (`cfg(not(unix))`),
// `default_lan_targets` emits the limited IPv4 broadcast (`Ipv4Addr::BROADCAST`)
// and unscoped IPv6 multicast (`[ff02::bf:1]:port`); Windows adapter
// enumeration (`GetAdaptersAddresses`) for directed subnet broadcasts and
// per-interface IPv6 scope IDs is left for the integration lane.
pub fn default_lan_targets(port: u16) -> Vec<SocketAddr> {
    let mut v4_addrs: Vec<Ipv4Addr> = vec![Ipv4Addr::BROADCAST];
    let mut v6_scopes: Vec<u32> = Vec::new();
    collect_lan_interfaces(&mut v4_addrs, &mut v6_scopes);

    let mut out = Vec::with_capacity(v4_addrs.len() + v6_scopes.len().max(1));
    for ip in v4_addrs {
        let addr = SocketAddr::V4(SocketAddrV4::new(ip, port));
        if !out.contains(&addr) {
            out.push(addr);
        }
    }
    if v6_scopes.is_empty() {
        out.push(SocketAddr::V6(SocketAddrV6::new(
            LAN_MULTICAST_V6,
            port,
            0,
            0,
        )));
    } else {
        for scope_id in v6_scopes {
            let addr = SocketAddr::V6(SocketAddrV6::new(LAN_MULTICAST_V6, port, 0, scope_id));
            if !out.contains(&addr) {
                out.push(addr);
            }
        }
    }
    out
}

#[cfg(unix)]
fn collect_lan_interfaces(v4_broadcasts: &mut Vec<Ipv4Addr>, v6_scopes: &mut Vec<u32>) {
    // SAFETY: `libc::getifaddrs` initializes `ifap` to a singly-linked list of OS interface
    // descriptors allocated by libc when it returns 0 and `ifap` is non-null. Every `cur`,
    // `ifa_addr`, `ifa_netmask`, and `ifa_name` pointer is checked for null and matching
    // `sa_family` (`AF_INET` / `AF_INET6`) before dereferencing or passing to `if_nametoindex`,
    // and the list is freed once via `libc::freeifaddrs(ifap)` before returning.
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 || ifap.is_null() {
            return;
        }
        let mut cur = ifap;
        while !cur.is_null() {
            let ifa = &*cur;
            let flags = ifa.ifa_flags as libc::c_int;
            let is_up = (flags & libc::IFF_UP) != 0;
            let is_loopback = (flags & libc::IFF_LOOPBACK) != 0;
            if is_up && !is_loopback && !ifa.ifa_addr.is_null() {
                let family = (*ifa.ifa_addr).sa_family as libc::c_int;
                if family == libc::AF_INET && !ifa.ifa_netmask.is_null() {
                    let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let mask_sin = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                    let ip = u32::from_be(sin.sin_addr.s_addr);
                    let mask = u32::from_be(mask_sin.sin_addr.s_addr);
                    if mask != 0 && mask != 0xffff_ffff {
                        let bcast = Ipv4Addr::from(ip | !mask);
                        if !v4_broadcasts.contains(&bcast) {
                            v4_broadcasts.push(bcast);
                        }
                    }
                } else if family == libc::AF_INET6 && !ifa.ifa_name.is_null() {
                    let scope = libc::if_nametoindex(ifa.ifa_name);
                    if scope != 0 && !v6_scopes.contains(&scope) {
                        v6_scopes.push(scope);
                    }
                }
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
}

#[cfg(not(unix))]
fn collect_lan_interfaces(_v4_broadcasts: &mut Vec<Ipv4Addr>, _v6_scopes: &mut Vec<u32>) {
    // Non-Unix builds (Windows) fall back to `Ipv4Addr::BROADCAST` and
    // unscoped `LAN_MULTICAST_V6` until `GetAdaptersAddresses` enumeration is
    // added.
}

// Broadcasts a `BFE-LAN/1` discovery probe over IPv4 and IPv6 and collects
// valid server responses within `options.timeout` (clamped to 1..=5000 ms).
// When a host answers on both IPv4 and IPv6, prefers the IPv4 address just as
// the desktop host does. When `options.measure_ping` is true, populates
// `latency_ms` via `with_latency`.
pub fn discover_lan(options: LanDiscoveryOptions) -> Result<Vec<ServerEntry>, String> {
    let port = if options.port == 0 {
        LAN_PORT
    } else {
        options.port
    };
    let timeout_ms = options.timeout.as_millis().clamp(1, 5_000) as u64;
    let timeout = Duration::from_millis(timeout_ms);

    let mut nonce_bytes = [0u8; 16];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| format!("random failed: {e}"))?;
    let nonce_hex = hex_lower(&nonce_bytes);
    let packet = try_encode_discover_packet(&nonce_hex)?;

    let targets = options
        .targets
        .unwrap_or_else(|| default_lan_targets(port));

    // Bind ephemeral IPv4 (`0.0.0.0:0` with `SO_BROADCAST`) and IPv6 (`[::]:0`)
    // UDP sockets in non-blocking mode so a single polling loop can drain replies
    // from both address families until `deadline`.
    let sock4 = UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))).ok();
    if let Some(ref s4) = sock4 {
        let _ = s4.set_broadcast(true);
        let _ = s4.set_nonblocking(true);
    }

    let sock6 = UdpSocket::bind(SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::UNSPECIFIED,
        0,
        0,
        0,
    )))
    .ok();
    if let Some(ref s6) = sock6 {
        let _ = s6.set_multicast_loop_v6(true);
        let _ = s6.set_nonblocking(true);
    }

    // Fan out the discovery probe to every configured IPv4 broadcast address
    // and IPv6 link-local multicast target (`for (const destination of destinations)
    // socket.send(request, port, destination)` in the desktop host).
    for target in &targets {
        match target {
            SocketAddr::V4(_) => {
                if let Some(ref s4) = sock4 {
                    let _ = s4.send_to(&packet, target);
                }
            }
            SocketAddr::V6(_) => {
                if let Some(ref s6) = sock6 {
                    let _ = s6.send_to(&packet, target);
                }
            }
        }
    }

    let deadline = Instant::now() + timeout;
    let mut order: Vec<String> = Vec::new();
    let mut found: HashMap<String, (ServerEntry, bool)> = HashMap::new();
    let mut buf = [0u8; MAX_LAN_PACKET + 64];

    while Instant::now() < deadline {
        let mut received_any = false;
        for sock in [sock4.as_ref(), sock6.as_ref()].into_iter().flatten() {
            while let Ok((n, remote)) = sock.recv_from(&mut buf) {
                received_any = true;
                if n > MAX_LAN_PACKET {
                    continue;
                }
                let Some(info) = parse_server_packet(&buf[..n], &nonce_hex, remote.ip()) else {
                    continue;
                };
                // Matches `if (!found.has(info.id) || remote.family === 'IPv4')`
                // in the desktop host: when a dual-stack LAN server answers on
                // both IPv6 multicast and IPv4 broadcast, prefer its IPv4 address
                // while preserving the initial discovery order across distinct servers.
                let is_v4 = remote.is_ipv4();
                match found.get_mut(&info.id) {
                    Some((existing, existing_v4)) => {
                        if is_v4 && !*existing_v4 {
                            *existing = info;
                            *existing_v4 = true;
                        }
                    }
                    None => {
                        if order.len() < MAX_LAN_SERVERS {
                            order.push(info.id.clone());
                            found.insert(info.id.clone(), (info, is_v4));
                        }
                    }
                }
            }
        }
        // When both non-blocking UDP sockets return `WouldBlock`, sleep briefly
        // (up to 5ms or the remaining window) before polling again so the scan
        // stays responsive without busy-spinning a CPU core.
        if !received_any {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(5)));
        }
    }

    // Collect discovered servers in the order their IDs were first seen,
    // matching `Map.prototype.values()` iteration order in the desktop host.
    let servers: Vec<ServerEntry> = order
        .into_iter()
        .filter_map(|id| found.remove(&id).map(|(entry, _)| entry))
        .collect();

    // Populate `latency_ms` via concurrent TCP handshake probes when requested
    // so LAN rows display signal bars and ping times in the server browser.
    if options.measure_ping {
        Ok(with_latency(servers, PING_TIMEOUT))
    } else {
        Ok(servers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{BattleSummary, ServerContract};

    // Recorded empty /info fields, wrapped in the LAN reply envelope. The
    // same ServerEntry decoder serves direct HTTP discovery and the browser.
    #[test]
    fn accepts_null_operator_metadata() {
        let reply = br#"{"magic":"BFE-LAN/1","type":"server","nonce":"00000000000000000000000000000000","info":{"id":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","name":"Native LAN paired engine","port":18451,"description":null,"location":null,"players":2,"maxPlayers":8}}"#;
        let server = parse_server_packet(reply, &"0".repeat(32), Ipv4Addr::LOCALHOST.into()).unwrap();
        assert!(server.description.is_empty());
        assert!(server.location.is_empty());
        assert_eq!(server.players, 2);
        let info: serde_json::Value = serde_json::from_slice(reply).unwrap();
        let direct: ServerEntry = serde_json::from_value(info["info"].clone()).unwrap();
        assert_eq!(server.name, direct.name);
    }

    // Spawns a single-shot mock `BFE-LAN/1` UDP responder thread that waits for
    // one `"discover"` datagram on `sock` and echoes a `"server"` reply carrying
    // the caller's `nonce` and `info`.
    fn spawn_mock_lan_responder(sock: UdpSocket, info: ServerEntry) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 2048];
            if let Ok((n, remote)) = sock.recv_from(&mut buf) {
                if let Ok(req) = serde_json::from_slice::<serde_json::Value>(&buf[..n]) {
                    if req.get("magic").and_then(|v| v.as_str()) == Some(LAN_MAGIC)
                        && req.get("type").and_then(|v| v.as_str()) == Some("discover")
                    {
                        if let Some(nonce) = req.get("nonce").and_then(|v| v.as_str()) {
                            let reply = serde_json::json!({
                                "magic": LAN_MAGIC,
                                "type": "server",
                                "nonce": nonce,
                                "info": info,
                            });
                            if let Ok(bytes) = serde_json::to_vec(&reply) {
                                let _ = sock.send_to(&bytes, remote);
                            }
                        }
                    }
                }
            }
        })
    }

    // Verifies end-to-end IPv4 loopback LAN discovery (`discover_lan` -> mock
    // responder -> `ServerEntry` with `address = "127.0.0.1"`) and confirms that
    // `parse_server_packet` rejects replies whose `nonce` does not match the probe.
    #[test]
    fn discovers_lan_server_over_ipv4_and_rejects_mismatched_nonce(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let responder_sock = UdpSocket::bind("127.0.0.1:0")?;
        let responder_addr = responder_sock.local_addr()?;
        let expected_id = "0123456789abcdef".repeat(4);
        let server_info = ServerEntry {
            id: expected_id.clone(),
            name: "Mos Eisley LAN".to_string(),
            port: Some(18451),
            players: 3,
            max_players: 32,
            battle: Some(BattleSummary {
                map: "tat2".to_string(),
                map_name: "Tatooine: Mos Eisley".to_string(),
                era: "gcw".to_string(),
                mode: "conquest".to_string(),
                ..Default::default()
            }),
            contract: Some(ServerContract::current()),
            ..Default::default()
        };

        let handle = spawn_mock_lan_responder(responder_sock, server_info.clone());
        let found = discover_lan(LanDiscoveryOptions {
            port: responder_addr.port(),
            timeout: Duration::from_millis(120),
            targets: Some(vec![responder_addr]),
            measure_ping: false,
        })?;
        let _ = handle.join();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, expected_id);
        assert_eq!(found[0].name, "Mos Eisley LAN");
        assert_eq!(found[0].address.as_deref(), Some("127.0.0.1"));
        assert_eq!(found[0].port, Some(18451));

        // Verify mismatched nonce is rejected by `parse_server_packet`.
        let forged = serde_json::json!({
            "magic": LAN_MAGIC,
            "type": "server",
            "nonce": "f".repeat(32),
            "info": server_info,
        });
        assert!(parse_server_packet(
            &serde_json::to_vec(&forged)?,
            &"0".repeat(32),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        )
        .is_none());
        Ok(())
    }

    // Verifies dual-stack deduplication: when the same server ID answers on both
    // `[::1]` (IPv6) and `127.0.0.1` (IPv4), `discover_lan` deduplicates by `id`
    // and prefers the IPv4 responder address just as the desktop host does.
    #[test]
    fn discovers_lan_server_over_ipv6_and_prefers_ipv4_when_dual_stack_answers(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Ok(sock6) = UdpSocket::bind("[::1]:0") else {
            return Ok(());
        };
        let addr6 = sock6.local_addr()?;
        let sock4 = UdpSocket::bind("127.0.0.1:0")?;
        let addr4 = sock4.local_addr()?;

        let shared_id = "fedcba9876543210".repeat(4);
        let info = ServerEntry {
            id: shared_id.clone(),
            name: "Dual Stack LAN".to_string(),
            port: Some(18451),
            players: 6,
            max_players: 32,
            ..Default::default()
        };

        let h6 = spawn_mock_lan_responder(sock6, info.clone());
        let h4 = spawn_mock_lan_responder(sock4, info);

        // Put IPv6 target first and IPv4 second to verify IPv4 replaces IPv6.
        let found = discover_lan(LanDiscoveryOptions {
            port: addr4.port(),
            timeout: Duration::from_millis(150),
            targets: Some(vec![addr6, addr4]),
            measure_ping: false,
        })?;

        let _ = h6.join();
        let _ = h4.join();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, shared_id);
        assert_eq!(found[0].address.as_deref(), Some("127.0.0.1"));
        Ok(())
    }
}
