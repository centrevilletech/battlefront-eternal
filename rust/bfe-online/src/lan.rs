// LAN server discovery over IPv4 broadcast and IPv6 link-local multicast
// (`BFE-LAN/1` UDP protocol on port 18452).
//
// Ports `electron/lan.js`, `electron/network.js::discoverLan`, and
// `battlefront-eternal-server/src/lan.js`.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::browser::ServerEntry;
use crate::identity::{hex_lower, is_lower_hex_64};
use crate::ping::{with_latency, PING_TIMEOUT};

// UDP port for `BFE-LAN/1` discovery beacons (`LAN_PORT` in `electron/lan.js`).
pub const LAN_PORT: u16 = 18452;

// Protocol discriminator (`LAN_MAGIC` in `electron/lan.js`).
pub const LAN_MAGIC: &str = "BFE-LAN/1";

// IPv6 link-local multicast group `ff02::bf:1` (`LAN_MULTICAST_V6` in
// `electron/lan.js`).
pub const LAN_MULTICAST_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0x00bf, 0x0001);

// Default discovery window (`DEFAULT_TIMEOUT_MS = 750` in `electron/lan.js`).
pub const DEFAULT_LAN_TIMEOUT: Duration = Duration::from_millis(750);

// Maximum distinct servers collected in one scan (`MAX_SERVERS = 256`).
pub const MAX_LAN_SERVERS: usize = 256;

// Maximum accepted UDP datagram length (`MAX_PACKET = 2048`).
pub const MAX_LAN_PACKET: usize = 2048;

#[derive(Debug, Clone)]
pub struct LanDiscoveryOptions {
    pub port: u16,
    pub timeout: Duration,
    pub targets: Option<Vec<SocketAddr>>,
    pub measure_ping: bool,
}

impl Default for LanDiscoveryOptions {
    fn default() -> Self {
        Self {
            port: LAN_PORT,
            timeout: DEFAULT_LAN_TIMEOUT,
            targets: None,
            measure_ping: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LanDiscoverPacket<'a> {
    magic: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    nonce: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
struct LanServerPacket {
    magic: String,
    #[serde(rename = "type")]
    kind: String,
    nonce: String,
    info: ServerEntry,
}

// Encodes a `BFE-LAN/1` discovery request datagram for the given 32-char hex
// nonce.
pub fn encode_discover_packet(nonce_hex: &str) -> Vec<u8> {
    serde_json::to_vec(&LanDiscoverPacket {
        magic: LAN_MAGIC,
        kind: "discover",
        nonce: nonce_hex,
    })
    .expect("serialize LanDiscoverPacket")
}

// Validates and decodes a `BFE-LAN/1` server response datagram against
// `expected_nonce` and attaches the responder's source IP address. Ports the
// message handler in `electron/lan.js`.
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
// from `electron/lan.js`.
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
// `electron/lan.js` does. When `options.measure_ping` is true, populates
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
    let packet = encode_discover_packet(&nonce_hex);

    let targets = options
        .targets
        .unwrap_or_else(|| default_lan_targets(port));

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
        if !received_any {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(5)));
        }
    }

    let servers: Vec<ServerEntry> = order
        .into_iter()
        .filter_map(|id| found.remove(&id).map(|(entry, _)| entry))
        .collect();

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
                            let bytes = serde_json::to_vec(&reply).unwrap();
                            let _ = sock.send_to(&bytes, remote);
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn discovers_lan_server_over_ipv4_and_rejects_mismatched_nonce() {
        let responder_sock = UdpSocket::bind("127.0.0.1:0").expect("bind v4 responder");
        let responder_addr = responder_sock.local_addr().unwrap();
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
        })
        .expect("discover_lan v4");
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
            &serde_json::to_vec(&forged).unwrap(),
            &"0".repeat(32),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        )
        .is_none());
    }

    #[test]
    fn discovers_lan_server_over_ipv6_and_prefers_ipv4_when_dual_stack_answers() {
        let Ok(sock6) = UdpSocket::bind("[::1]:0") else {
            return;
        };
        let addr6 = sock6.local_addr().unwrap();
        let sock4 = UdpSocket::bind("127.0.0.1:0").expect("bind v4");
        let addr4 = sock4.local_addr().unwrap();

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
        })
        .expect("discover dual stack");

        let _ = h6.join();
        let _ = h4.join();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, shared_id);
        assert_eq!(found[0].address.as_deref(), Some("127.0.0.1"));
    }
}
