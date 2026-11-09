//! Socket modes that the JavaScript listeners requested explicitly. OS defaults
//! differ: a Windows IPv6 listener does not also accept IPv4 unless asked.
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{SocketAddr, TcpListener, UdpSocket};

fn bound(addr: SocketAddr, kind: Type, protocol: Protocol, only_v6: bool) -> io::Result<Socket> {
    let socket = Socket::new(Domain::for_address(addr), kind, Some(protocol))?;
    // libuv's Unix TCP listeners allow a new round to bind while completed
    // /info connections remain in TIME_WAIT. Windows has different reuse
    // semantics, so retain its default, as libuv's Windows backend does.
    #[cfg(unix)]
    if kind == Type::STREAM {
        socket.set_reuse_address(true)?;
    }
    // Configure before bind: some kernels reject changing this on a bound
    // socket. Failure must reach the caller's IPv4 fallback, never be ignored.
    if addr.is_ipv6() {
        socket.set_only_v6(only_v6)?;
    }
    socket.bind(&addr.into())?;
    Ok(socket)
}

pub(crate) fn tcp(addr: SocketAddr) -> io::Result<TcpListener> {
    let socket = bound(addr, Type::STREAM, Protocol::TCP, false)?;
    socket.listen(128)?;
    Ok(socket.into())
}

pub(crate) fn udp(addr: SocketAddr, only_v6: bool) -> io::Result<UdpSocket> {
    Ok(bound(addr, Type::DGRAM, Protocol::UDP, only_v6)?.into())
}

fn datagram_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionRefused
    )
}

/// Winsock can report an earlier peer's ICMP port-unreachable as WSAECONNRESET
/// on recv_from. Drop that notification and keep draining unrelated peers;
/// WouldBlock and real socket failures still reach the caller unchanged.
pub(crate) fn recv_from(socket: &UdpSocket, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
    loop {
        match socket.recv_from(buf) {
            Err(error)
                if !matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                eprintln!(
                    "server UDP receive local={:?} capacity={} raw_os_error={:?}: {error}",
                    socket.local_addr(),
                    buf.len(),
                    error.raw_os_error()
                );
                if datagram_error(&error) {
                    continue;
                }
                return Err(error);
            }
            result => return result,
        }
    }
}

/// Error diagnostics stay off the successful packet path.
pub(crate) fn send_to(socket: &UdpSocket, bytes: &[u8], to: SocketAddr) {
    // ServerEndpoint normalizes mapped sources for identity and path checks.
    // Winsock still requires sockaddr_in6 when replying through an IPv6 socket;
    // Linux also accepts sockaddr_in here, masking the missing conversion.
    let destination = match to {
        SocketAddr::V4(addr) if socket.local_addr().is_ok_and(|local| local.is_ipv6()) => {
            SocketAddr::new(addr.ip().to_ipv6_mapped().into(), addr.port())
        }
        _ => to,
    };
    if let Err(error) = socket.send_to(bytes, destination) {
        eprintln!(
            "server UDP send local={:?} to={to} bytes={} raw_os_error={:?}: {error}",
            socket.local_addr(),
            bytes.len(),
            error.raw_os_error()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    #[test]
    fn tcp_listener_rebinds_after_a_completed_info_connection() {
        let listener = tcp("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (mut accepted, _) = listener.accept().unwrap();
        accepted.write_all(b"info").unwrap();
        accepted.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert_eq!(response, b"info");
        drop(client);
        drop(accepted);
        drop(listener);
        // A round transition can reuse TIME_WAIT, but cannot steal a live
        // listener's port. Both sides of that distinction matter here.
        let reopened = tcp(address).expect("reopen the next round's /info listener");
        assert!(tcp(address).is_err());
        drop(reopened);
    }

    #[test]
    fn ipv6_game_listeners_also_accept_ipv4() {
        // IPv6 can be absent on a host. Once available, neither family may
        // silently disappear from a successfully bound dual-stack listener.
        if TcpListener::bind("[::1]:0").is_err() {
            return;
        }
        let listener = tcp("[::]:0".parse().unwrap()).unwrap();
        assert!(!socket2::SockRef::from(&listener).only_v6().unwrap());
        let port = listener.local_addr().unwrap().port();
        for ip in ["127.0.0.1", "::1"] {
            let addr = SocketAddr::new(ip.parse().unwrap(), port);
            let mut client = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
            client.write_all(b"x").unwrap();
            let (mut accepted, _) = listener.accept().unwrap();
            accepted
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut byte = [0];
            accepted.read_exact(&mut byte).unwrap();
            assert_eq!(byte, *b"x");
        }
        let receiver = udp("[::]:0".parse().unwrap(), false).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();
        for (bind, ip) in [("127.0.0.1:0", "127.0.0.1"), ("[::1]:0", "::1")] {
            let sender = UdpSocket::bind(bind).unwrap();
            sender
                .send_to(b"x", SocketAddr::new(ip.parse().unwrap(), port))
                .unwrap();
            let mut byte = [0];
            let (length, source) = recv_from(&receiver, &mut byte).unwrap();
            assert_eq!(length, 1);
            assert_eq!(byte, *b"x");
            // The transport hands the sink a canonical IPv4 address even
            // though recv_from returned an IPv6-mapped source. Prove replies,
            // not only reception, work through that same dual-stack socket.
            let normalized = bfe_transport::server::normalize_socket_addr(source);
            sender
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            send_to(&receiver, b"y", normalized);
            assert_eq!(sender.recv_from(&mut byte).unwrap().0, 1);
            assert_eq!(byte, *b"y");
        }
    }

    #[test]
    fn ipv4_listener_replies_without_mapping_the_destination() {
        let receiver = udp("127.0.0.1:0".parse().unwrap(), false).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        send_to(&receiver, b"x", sender.local_addr().unwrap());
        let mut byte = [0];
        assert_eq!(sender.recv_from(&mut byte).unwrap().0, 1);
        assert_eq!(byte, *b"x");
    }

    #[test]
    fn lan_ipv6_listener_keeps_the_ipv4_port_available() {
        if UdpSocket::bind("[::1]:0").is_err() {
            return;
        }
        let ipv4 = UdpSocket::bind("0.0.0.0:0").unwrap();
        let ipv6 = udp(
            SocketAddr::new("::".parse().unwrap(), ipv4.local_addr().unwrap().port()),
            true,
        )
        .unwrap();
        assert!(socket2::SockRef::from(&ipv6).only_v6().unwrap());
    }

    #[test]
    fn peer_errors_do_not_end_udp_drain() {
        for kind in [
            io::ErrorKind::Interrupted,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionRefused,
        ] {
            assert!(datagram_error(&io::Error::from(kind)));
        }
        assert!(!datagram_error(&io::Error::from(io::ErrorKind::WouldBlock)));
        assert!(!datagram_error(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        #[cfg(windows)]
        assert!(datagram_error(&io::Error::from_raw_os_error(10054)));
    }
}
