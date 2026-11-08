// Direct Connect IPv4/IPv6 address and port validation.
//
// Ports `DEFAULT_DIRECT_PORT`, `parseDirectTarget`, `validIpAddress`,
// `validIpv4`, and `validIpv6` from `src/shell/direct-connect.js`, plus the
// `directInfo` validation in `electron/network.js` and `HOST:PORT` CLI target
// parsing for `online-tool ping`.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

// Default UDP/HTTP game port for Direct Connect and dedicated servers,
// matching `DEFAULT_DIRECT_PORT` in `src/shell/direct-connect.js`.
pub const DEFAULT_DIRECT_PORT: u16 = 18451;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectTarget {
    pub address: String,
    pub ip: IpAddr,
    pub port: u16,
}

impl DirectTarget {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }

    // Formats the host for URLs and `HOST:PORT` strings, wrapping IPv6
    // addresses in brackets (`[fe80::1]:18451`).
    pub fn host_port_string(&self) -> String {
        match self.ip {
            IpAddr::V4(_) => format!("{}:{}", self.address, self.port),
            IpAddr::V6(_) => format!("[{}]:{}", self.address, self.port),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectConnectError {
    EmptyAddress,
    InvalidIpAddress,
    InvalidPort,
}

impl DirectConnectError {
    // Uppercase UI status text matching `parseDirectTarget` in
    // `src/shell/direct-connect.js`.
    pub fn ui_text(self) -> &'static str {
        match self {
            Self::EmptyAddress => "ENTER AN IP ADDRESS",
            Self::InvalidIpAddress => "INVALID IP ADDRESS",
            Self::InvalidPort => "PORT MUST BE 1 TO 65535",
        }
    }

    // Error text matching `directInfo` in `electron/network.js`.
    pub fn bridge_text(self) -> &'static str {
        match self {
            Self::EmptyAddress | Self::InvalidIpAddress => {
                "Direct Connect requires an IPv4 or IPv6 address"
            }
            Self::InvalidPort => "Port must be between 1 and 65535",
        }
    }
}

impl std::fmt::Display for DirectConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.ui_text())
    }
}

// Strips surrounding whitespace and a single pair of outer `[` / `]` brackets,
// matching `String(address ?? '').trim().replace(/^\[|\]$/g, '')` in
// `src/shell/direct-connect.js` and `electron/network.js`.
pub fn strip_host_brackets(raw: &str) -> &str {
    let trimmed = raw.trim();
    let without_open = trimmed.strip_prefix('[').unwrap_or(trimmed);
    without_open.strip_suffix(']').unwrap_or(without_open)
}

// Validates a Direct Connect dialog `(address, port)` pair exactly as
// `parseDirectTarget` in `src/shell/direct-connect.js` does.
pub fn parse_direct_target(address: &str, port: &str) -> Result<DirectTarget, DirectConnectError> {
    let host = strip_host_brackets(address);
    if host.is_empty() {
        return Err(DirectConnectError::EmptyAddress);
    }
    let ip = parse_strict_ip(host).ok_or(DirectConnectError::InvalidIpAddress)?;
    let port_trimmed = port.trim();
    if port_trimmed.is_empty() {
        return Err(DirectConnectError::InvalidPort);
    }
    let port_num: i64 = port_trimmed
        .parse()
        .map_err(|_| DirectConnectError::InvalidPort)?;
    if !(1..=65535).contains(&port_num) {
        return Err(DirectConnectError::InvalidPort);
    }
    Ok(DirectTarget {
        address: host.to_string(),
        ip,
        port: port_num as u16,
    })
}

// Validates a numeric `(address, port)` pair for the network bridge, matching
// `directInfo(address, port)` in `electron/network.js`.
pub fn validate_direct_endpoint(address: &str, port: i64) -> Result<DirectTarget, String> {
    let host = strip_host_brackets(address);
    let Some(ip) = parse_strict_ip(host) else {
        return Err(DirectConnectError::InvalidIpAddress
            .bridge_text()
            .to_string());
    };
    if !(1..=65535).contains(&port) {
        return Err(DirectConnectError::InvalidPort.bridge_text().to_string());
    }
    Ok(DirectTarget {
        address: host.to_string(),
        ip,
        port: port as u16,
    })
}

// Matches `validIpAddress` in `src/shell/direct-connect.js` (strict decimal
// IPv4 without leading zeros and standard IPv6).
pub fn parse_strict_ip(host: &str) -> Option<IpAddr> {
    if valid_ipv4(host) {
        return host.parse::<IpAddr>().ok();
    }
    if valid_ipv6(host) {
        return host.parse::<IpAddr>().ok();
    }
    None
}

fn valid_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.into_iter().all(|part| {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        if part.len() > 1 && part.starts_with('0') {
            return false;
        }
        matches!(part.parse::<u16>(), Ok(0..=255))
    })
}

fn valid_ipv6(host: &str) -> bool {
    if !host.contains(':') {
        return false;
    }
    host.parse::<std::net::Ipv6Addr>().is_ok()
}

// Parses a `HOST:PORT` or `[IPv6]:PORT` string for CLI tools (`online-tool ping`
// and `--join`). Accepts literal IPv4, bracketed IPv6, or DNS hostnames.
pub fn resolve_host_port(target: &str) -> Result<(String, SocketAddr), String> {
    let trimmed = target.trim();
    if let Ok(addr) = trimmed.parse::<SocketAddr>() {
        return Ok((addr.ip().to_string(), addr));
    }
    let (host_part, port_part) = trimmed
        .rsplit_once(':')
        .ok_or_else(|| format!("expected HOST:PORT, got {trimmed:?}"))?;
    let host = strip_host_brackets(host_part);
    if host.is_empty() {
        return Err(format!("missing host in {trimmed:?}"));
    }
    let port: u16 = port_part
        .parse()
        .map_err(|_| format!("invalid port in {trimmed:?}"))?;
    if port == 0 {
        return Err("Port must be between 1 and 65535".to_string());
    }
    if let Some(ip) = parse_strict_ip(host) {
        return Ok((host.to_string(), SocketAddr::new(ip, port)));
    }
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("{trimmed}: {e}"))?
        .next()
        .ok_or_else(|| format!("{trimmed}: no addresses resolved"))?;
    Ok((host.to_string(), addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_ipv4_and_ipv6_direct_targets() {
        let sample_v4 = format!("{}.{}.{}.{}", 192, 168, 1, 40);
        let padded_v4 = format!(" {sample_v4} ");
        let v4 = parse_direct_target(&padded_v4, "18451").expect("valid v4");
        assert_eq!(v4.address, sample_v4);
        assert_eq!(v4.port, 18451);
        assert_eq!(v4.host_port_string(), format!("{sample_v4}:18451"));

        let v6 = parse_direct_target("[2001:db8::1]", "18451").expect("valid bracketed v6");
        assert_eq!(v6.address, "2001:db8::1");
        assert_eq!(v6.port, 18451);
        assert_eq!(v6.host_port_string(), "[2001:db8::1]:18451");

        let loopback = parse_direct_target("::1", "65535").expect("valid ::1");
        assert_eq!(loopback.address, "::1");
        assert_eq!(loopback.port, 65535);
    }

    #[test]
    fn rejects_invalid_addresses_and_ports_with_exact_messages() {
        assert_eq!(
            parse_direct_target("   ", "18451"),
            Err(DirectConnectError::EmptyAddress)
        );
        assert_eq!(
            parse_direct_target("[]", "18451"),
            Err(DirectConnectError::EmptyAddress)
        );
        assert_eq!(
            parse_direct_target("example.com", "18451"),
            Err(DirectConnectError::InvalidIpAddress)
        );
        let leading_zero_v4 = format!("{}.{}.{}.{}", 192, 168, "01", 1);
        assert_eq!(
            parse_direct_target(&leading_zero_v4, "18451"),
            Err(DirectConnectError::InvalidIpAddress)
        );
        let out_of_range_v4 = format!("{}.{}.{}.{}", 256, 0, 0, 1);
        assert_eq!(
            parse_direct_target(&out_of_range_v4, "18451"),
            Err(DirectConnectError::InvalidIpAddress)
        );
        assert_eq!(
            parse_direct_target("127.0.0.1", "0"),
            Err(DirectConnectError::InvalidPort)
        );
        assert_eq!(
            parse_direct_target("127.0.0.1", "65536"),
            Err(DirectConnectError::InvalidPort)
        );
        assert_eq!(
            parse_direct_target("127.0.0.1", "abc"),
            Err(DirectConnectError::InvalidPort)
        );
    }
}
