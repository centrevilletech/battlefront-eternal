// Server TCP handshake latency measurement (`withLatency` / `measureLatency`)
// and direct `/info` HTTP query (`directInfo`).
//
// Ports `measureLatency`, `withLatency`, `directInfo`, and `fetchJson` from
// `electron/network.js`, plus HTTP `/info` framing from
// `rust/bfe-client/src/online/info.rs`.

use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::browser::{EndpointAddr, ServerEntry};
use crate::direct::{resolve_host_port, strip_host_brackets, validate_direct_endpoint};
use crate::identity::is_lower_hex_64;

// Default per-endpoint TCP connect timeout (1000 ms), matching
// `PING_TIMEOUT_MS` in `electron/network.js`.
pub const PING_TIMEOUT: Duration = Duration::from_millis(1_000);

// Maximum concurrent server latency probes (16), matching `PING_CONCURRENCY`
// in `electron/network.js`.
pub const PING_CONCURRENCY: usize = 16;

// Maximum endpoints probed per server (`endpoints.slice(0, 4)` in
// `electron/network.js`).
pub const MAX_PING_ENDPOINTS: usize = 4;

// Default direct `/info` HTTP timeout (5000 ms), matching
// `DIRECT_INFO_TIMEOUT_MS` in `electron/network.js`.
pub const DIRECT_INFO_TIMEOUT: Duration = Duration::from_millis(5_000);

// Maximum `/info` response body size (32 KiB, matching `electron/network.js`).
const MAX_DIRECT_INFO_BYTES: usize = 32 * 1024;

// Measures TCP connect handshake time to `(address, port)` in milliseconds,
// returning `Some(max(1, round(elapsed_ms)))` on success or `None` on timeout
// or connection error. Ports `measureLatency` in `electron/network.js`.
pub fn measure_latency(address: IpAddr, port: u16, timeout: Duration) -> Option<u32> {
    if port == 0 {
        return None;
    }
    let target = SocketAddr::new(address, port);
    let started = Instant::now();
    let stream = TcpStream::connect_timeout(&target, timeout).ok()?;
    let elapsed_ms = (started.elapsed().as_secs_f64() * 1000.0).round().max(1.0) as u32;
    let _ = stream.shutdown(Shutdown::Both);
    Some(elapsed_ms)
}

// Measures latency to an `EndpointAddr` by parsing its IP address and probing
// TCP connect time.
pub fn measure_endpoint_latency(endpoint: &EndpointAddr, timeout: Duration) -> Option<u32> {
    if endpoint.port == 0 {
        return None;
    }
    let clean = strip_host_brackets(&endpoint.address);
    let ip: IpAddr = clean.parse().ok()?;
    measure_latency(ip, endpoint.port, timeout)
}

// Measures latency for a single server across up to `MAX_PING_ENDPOINTS`
// advertised endpoints in parallel, returning the minimum answered latency or
// `None` if no endpoints are listed or none answer.
pub fn measure_server_latency(server: &ServerEntry, timeout: Duration) -> Option<u32> {
    let slice: &[EndpointAddr] = if server.endpoints.len() > MAX_PING_ENDPOINTS {
        &server.endpoints[..MAX_PING_ENDPOINTS]
    } else {
        &server.endpoints[..]
    };
    if slice.is_empty() {
        if let (Some(addr), Some(port)) = (server.address.as_deref(), server.port) {
            let ep = EndpointAddr {
                address: addr.to_string(),
                port,
                ..Default::default()
            };
            return measure_endpoint_latency(&ep, timeout);
        }
        return None;
    }
    if slice.len() == 1 {
        return measure_endpoint_latency(&slice[0], timeout);
    }
    std::thread::scope(|s| {
        let handles: Vec<_> = slice
            .iter()
            .map(|ep| s.spawn(move || measure_endpoint_latency(ep, timeout)))
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .min()
    })
}

// Populates `latency_ms` on every server in `servers` using up to
// `PING_CONCURRENCY` (16) concurrent workers. Ports `withLatency` in
// `electron/network.js`.
pub fn with_latency(mut servers: Vec<ServerEntry>, timeout: Duration) -> Vec<ServerEntry> {
    if servers.is_empty() {
        return servers;
    }
    let count = servers.len().min(PING_CONCURRENCY);
    let next_idx = AtomicUsize::new(0);
    let servers_ref = &servers;
    let next_ref = &next_idx;
    let results: Vec<Option<u32>> = std::thread::scope(|s| {
        let mut workers = Vec::with_capacity(count);
        for _ in 0..count {
            workers.push(s.spawn(move || {
                let mut local = Vec::new();
                loop {
                    let i = next_ref.fetch_add(1, Ordering::Relaxed);
                    if i >= servers_ref.len() {
                        break;
                    }
                    let ms = measure_server_latency(&servers_ref[i], timeout);
                    local.push((i, ms));
                }
                local
            }));
        }
        let mut out = vec![None; servers.len()];
        for worker in workers {
            if let Ok(pairs) = worker.join() {
                for (i, ms) in pairs {
                    out[i] = ms;
                }
            }
        }
        out
    });
    for (server, ms) in servers.iter_mut().zip(results) {
        server.latency_ms = ms;
    }
    servers
}

// Queries `GET http://{address}:{port}/info` directly over TCP, validates the
// server info payload, and measures TCP latency. Ports `directInfo` in
// `electron/network.js`.
pub fn direct_info(address: &str, port: i64, timeout: Duration) -> Result<ServerEntry, String> {
    let target = validate_direct_endpoint(address, port)?;
    let mut info = fetch_server_info(target.socket_addr(), &target.host_port_string(), timeout)?;
    info.address = Some(target.address);
    info.port = Some(target.port);
    info.latency_ms = measure_latency(target.ip, target.port, PING_TIMEOUT);
    Ok(info)
}

// Resolves `HOST:PORT`, measures TCP connect latency, and fetches `/info` for
// `online-tool ping HOST:PORT`.
pub fn ping_host_port(target: &str, timeout: Duration) -> Result<(ServerEntry, u32), String> {
    let (host, addr) = resolve_host_port(target)?;
    let latency = measure_latency(addr.ip(), addr.port(), timeout)
        .ok_or_else(|| format!("Server at {target} did not respond"))?;
    let host_header = match addr {
        SocketAddr::V4(_) => format!("{host}:{}", addr.port()),
        SocketAddr::V6(_) => format!("[{host}]:{}", addr.port()),
    };
    let mut info = fetch_server_info(addr, &host_header, timeout)?;
    info.address = Some(addr.ip().to_string());
    info.port = Some(addr.port());
    info.latency_ms = Some(latency);
    Ok((info, latency))
}

// Performs a minimal HTTP/1.1 `GET /info` request over a `TcpStream` with a
// bounded body limit, supporting both `Content-Length` and chunked transfer
// encoding.
pub fn fetch_server_info(
    addr: SocketAddr,
    host_header: &str,
    timeout: Duration,
) -> Result<ServerEntry, String> {
    let deadline = Instant::now() + timeout;
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|_| format!("Server at {host_header} did not respond"))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let request = format!(
        "GET /info HTTP/1.1\r\nHost: {host_header}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|_| format!("Server at {host_header} did not respond"))?;

    let mut raw = Vec::with_capacity(2048);
    let mut buf = [0u8; 2048];
    let header_end = loop {
        if let Some(pos) = find_subslice(&raw, b"\r\n\r\n") {
            break pos;
        }
        if raw.len() >= MAX_DIRECT_INFO_BYTES {
            return Err("Server response was too large".to_string());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("Server at {host_header} did not respond"));
        }
        let _ = stream.set_read_timeout(Some(remaining));
        let n = stream
            .read(&mut buf)
            .map_err(|_| format!("Server at {host_header} did not respond"))?;
        if n == 0 {
            return Err(format!("Server at {host_header} did not respond"));
        }
        raw.extend_from_slice(&buf[..n]);
    };

    let header_str = std::str::from_utf8(&raw[..header_end])
        .map_err(|_| "Server returned an invalid response".to_string())?;
    let mut lines = header_str.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "Server returned an invalid response".to_string())?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| "Server returned an invalid response".to_string())?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    for line in lines {
        if let Some((name, val)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                let len = val
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Server returned an invalid response".to_string())?;
                if len > MAX_DIRECT_INFO_BYTES {
                    return Err("Server response was too large".to_string());
                }
                content_length = Some(len);
            } else if name.eq_ignore_ascii_case("transfer-encoding")
                && val.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }

    let body_start = header_end + 4;
    loop {
        if let Some(expected) = content_length {
            if raw.len().saturating_sub(body_start) >= expected {
                break;
            }
        }
        if raw.len().saturating_sub(body_start) > MAX_DIRECT_INFO_BYTES {
            return Err("Server response was too large".to_string());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("Server at {host_header} did not respond"));
        }
        let _ = stream.set_read_timeout(Some(remaining));
        let n = stream
            .read(&mut buf)
            .map_err(|_| format!("Server at {host_header} did not respond"))?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }

    let body_bytes = if chunked {
        decode_chunked(&raw[body_start..])?
    } else if let Some(expected) = content_length {
        if raw.len().saturating_sub(body_start) < expected {
            return Err("Server returned an invalid response".to_string());
        }
        raw[body_start..body_start + expected].to_vec()
    } else {
        raw[body_start..].to_vec()
    };

    if status_code >= 400 {
        if let Ok(err_obj) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            if let Some(msg) = err_obj.get("error").and_then(|v| v.as_str()) {
                return Err(msg.to_string());
            }
        }
        return Err(format!("Request failed ({status_code})"));
    }

    let info: ServerEntry = serde_json::from_slice(&body_bytes)
        .map_err(|_| "Server returned an invalid response".to_string())?;
    if !is_lower_hex_64(&info.id) {
        return Err("Server returned an invalid info payload".to_string());
    }
    Ok(info)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn decode_chunked(mut input: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = find_subslice(input, b"\r\n")
            .ok_or_else(|| "Server returned an invalid response".to_string())?;
        let size_str = std::str::from_utf8(&input[..line_end])
            .map_err(|_| "Server returned an invalid response".to_string())?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let chunk_len = usize::from_str_radix(size_str, 16)
            .map_err(|_| "Server returned an invalid response".to_string())?;
        input = &input[line_end + 2..];
        if chunk_len == 0 {
            return Ok(out);
        }
        if out.len().saturating_add(chunk_len) > MAX_DIRECT_INFO_BYTES {
            return Err("Server response was too large".to_string());
        }
        if input.len() < chunk_len + 2 || &input[chunk_len..chunk_len + 2] != b"\r\n" {
            return Err("Server returned an invalid response".to_string());
        }
        out.extend_from_slice(&input[..chunk_len]);
        input = &input[chunk_len + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn measures_tcp_latency_and_populates_with_latency() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().unwrap();
        let accept_handle = std::thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((stream, _)) = listener.accept() {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
        });

        let ms = measure_latency(addr.ip(), addr.port(), Duration::from_millis(500));
        assert!(ms.is_some());
        assert!(ms.unwrap() >= 1);

        let servers = vec![
            ServerEntry {
                id: "a".repeat(64),
                name: "Reachable".to_string(),
                endpoints: vec![EndpointAddr {
                    address: addr.ip().to_string(),
                    port: addr.port(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ServerEntry {
                id: "b".repeat(64),
                name: "No Endpoints".to_string(),
                endpoints: Vec::new(),
                ..Default::default()
            },
        ];
        let updated = with_latency(servers, Duration::from_millis(500));
        assert!(updated[0].latency_ms.is_some());
        assert_eq!(updated[1].latency_ms, None);

        let _ = accept_handle.join();
    }

    #[test]
    fn direct_info_fetches_and_validates_server_info_over_ipv4() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().unwrap();
        let body = serde_json::json!({
            "id": "ab".repeat(32),
            "name": "Local Direct Server",
            "port": addr.port(),
            "players": 4,
            "maxPlayers": 32,
            "battle": {
                "map": "bes2",
                "mapName": "Bespin: Cloud City",
                "era": "gcw",
                "mode": "conquest"
            }
        })
        .to_string();

        let server_thread = std::thread::spawn(move || {
            // 1st connection: GET /info, 2nd connection: measure_latency TCP probe.
            if let Ok((mut stream, _)) = listener.accept() {
                let mut req = [0u8; 512];
                let _ = stream.read(&mut req);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
            if let Ok((stream, _)) = listener.accept() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        });

        let info = direct_info("127.0.0.1", i64::from(addr.port()), Duration::from_secs(2))
            .expect("direct_info should succeed");
        assert_eq!(info.id, "ab".repeat(32));
        assert_eq!(info.name, "Local Direct Server");
        assert_eq!(info.address.as_deref(), Some("127.0.0.1"));
        assert_eq!(info.port, Some(addr.port()));
        assert!(info.latency_ms.is_some());

        let _ = server_thread.join();
    }
}
