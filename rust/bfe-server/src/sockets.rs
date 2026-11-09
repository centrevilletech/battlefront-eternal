//! Socket modes that the JavaScript listeners requested explicitly. OS defaults
//! differ: a Windows IPv6 listener does not also accept IPv4 unless asked.
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{SocketAddr, TcpListener, UdpSocket};

fn bound(addr: SocketAddr, kind: Type, protocol: Protocol, only_v6: bool) -> io::Result<Socket> {
    let socket = Socket::new(Domain::for_address(addr), kind, Some(protocol))?;
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
            Err(error) if datagram_error(&error) => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

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
            assert_eq!(recv_from(&receiver, &mut byte).unwrap().0, 1);
            assert_eq!(byte, *b"x");
        }
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
