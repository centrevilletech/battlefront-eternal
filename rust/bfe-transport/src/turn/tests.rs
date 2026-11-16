use super::*;
use std::net::{Ipv4Addr, Ipv6Addr};

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn md5_matches_rfc_1321_test_suite() {
    let cases = [
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        ("abcdefghijklmnopqrstuvwxyz", "c3fcd3d76192e4007dfb496cca67e13b"),
        ("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", "d174ab98d277d9f5a5611c2c9f419d9f"),
        ("12345678901234567890123456789012345678901234567890123456789012345678901234567890", "57edf4a22be3c955ac49da2e2107b67a"),
    ];
    for (input, digest) in cases {
        assert_eq!(to_hex(&md5(input.as_bytes())), digest, "md5({input:?})");
    }
}

// RFC 5769 2.1: a request with short-term credentials (the key is the password itself).
const RFC5769_REQUEST: &str = "000100582112a442b7e7a701bc34d686fa87dfae802200105354554e20746573742063
6c69656e74002400046e0001ff80290008932ff9b151263b36000600096576746a3a683676592020200008001
49aeaa70cbfd8cb56781ef2b5b2d3f249c1b571a280280004e57a3bcf";
// RFC 5769 2.2 and 2.3: responses carrying XOR-MAPPED-ADDRESS.
const RFC5769_RESPONSE_V4: &str = "0101003c2112a442b7e7a701bc34d686fa87dfae8022000b7465737420766563746f7220
002000080001a147e112a643000800142b91f599fd9e90c38c7489f92af9ba53f06be7d780280004c07d4c96";
const RFC5769_RESPONSE_V6: &str = "010100482112a442b7e7a701bc34d686fa87dfae8022000b7465737420766563746f7220
00200014 0002a1470113a9faa5d3f179bc25f4b5bed2b9d9 00080014a382954e4be67bf11784c97c8292c275bfe3ed41
80280004c8fb0b4c";
// RFC 5769 2.4: a request with long-term credentials.
const RFC5769_LONG_TERM: &str = "000100602112a44278ad3433c6ad72c029da412e00060012e3839ee38388e383aae38383
e382afe382b900000015001c662f2f3439396b3935346436 4f4c33346f4c39465354767936347341 0014000b6578616d
706c652e6f726700 00080014f67024656dd64a3e02b8e0712e85c9a28ca89666";

#[test]
fn integrity_matches_rfc_5769_vectors() {
    let password = b"VOkJxbRl1RmTxUk/WvJxBt";
    let request = hex(RFC5769_REQUEST);
    let message = stun::decode_message(&request).expect("RFC 5769 request decodes");
    assert_eq!(message.method, method::BINDING);
    assert!(check_integrity(&message, password));
    assert!(!check_integrity(&message, b"wrong password"));
    // The trailing FINGERPRINT is the CRC-32 of everything before it, XOR "STUN".
    let crc = crc32fast::hash(&request[..request.len() - 8]) ^ 0x5354_554e;
    assert_eq!(request[request.len() - 4..], crc.to_be_bytes());

    let v4 = hex(RFC5769_RESPONSE_V4);
    let message = stun::decode_message(&v4).expect("RFC 5769 IPv4 response decodes");
    assert!(check_integrity(&message, password));
    assert_eq!(message.address(attr::XOR_MAPPED_ADDRESS), Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 32853)));

    let v6 = hex(RFC5769_RESPONSE_V6);
    let message = stun::decode_message(&v6).expect("RFC 5769 IPv6 response decodes");
    assert!(check_integrity(&message, password));
    let expected: Ipv6Addr = "2001:db8:1234:5678:11:2233:4455:6677".parse().expect("address");
    assert_eq!(message.address(attr::XOR_MAPPED_ADDRESS), Some(SocketAddr::new(IpAddr::V6(expected), 32853)));

    // Long-term: key = MD5(username:realm:password), password after SASLprep.
    let long = hex(RFC5769_LONG_TERM);
    let message = stun::decode_message(&long).expect("RFC 5769 long-term request decodes");
    assert_eq!(message.attr(attr::REALM), Some(&b"example.org"[..]));
    let key = long_term_key("\u{30DE}\u{30C8}\u{30EA}\u{30C3}\u{30AF}\u{30B9}", b"example.org", "TheMatrIX");
    assert!(check_integrity(&message, &key));
    assert!(!check_integrity(&message, &long_term_key("x", b"example.org", "TheMatrIX")));
}

#[test]
fn encoded_integrity_and_fingerprint_verify() {
    let key = long_term_key("user", b"realm", "pass");
    let tx = [7u8; 12];
    let packet = encode(method::ALLOCATE, class::REQUEST, &tx, &[(attr::LIFETIME, &600u32.to_be_bytes()), (attr::USERNAME, b"user")], Some(&key));
    let message = stun::decode_message(&packet).expect("decodes");
    assert!(check_integrity(&message, &key));
    let crc = crc32fast::hash(&packet[..packet.len() - 8]) ^ 0x5354_554e;
    assert_eq!(packet[packet.len() - 4..], crc.to_be_bytes());
    let mut tampered = packet.clone();
    tampered[25] ^= 1;
    let message = stun::decode_message(&tampered).expect("still decodes");
    assert!(!check_integrity(&message, &key));
}

// ---- A small TURN server, enough to drive the client through its states ----

const SERVER: &str = "11.0.0.1:3478";
const RELAYED: &str = "11.0.0.1:49200";
const REALM: &[u8] = b"relay.test";

struct FakeTurn {
    key: [u8; 16],
    nonce: Vec<u8>,
    stale_once: bool,
}

impl FakeTurn {
    fn new() -> Self {
        Self { key: long_term_key("1700000000:peer", REALM, "secret"), nonce: b"nonce-1".to_vec(), stale_once: false }
    }
    /// Answers one client request; None for indications.
    fn answer(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        let m = stun::decode_message(packet)?;
        if m.klass != class::REQUEST {
            return None;
        }
        if m.integrity_at.is_none() || m.attr(attr::NONCE) != Some(&self.nonce[..]) {
            return Some(self.error(&m, 401));
        }
        if self.stale_once {
            self.stale_once = false;
            self.nonce = b"nonce-2".to_vec();
            return Some(self.error(&m, 438));
        }
        assert!(check_integrity(&m, &self.key), "client request integrity");
        assert_eq!(m.attr(attr::USERNAME), Some(&b"1700000000:peer"[..]));
        let mut fields: Vec<(u16, Vec<u8>)> = Vec::new();
        if m.method == method::ALLOCATE {
            assert_eq!(m.attr(attr::REQUESTED_TRANSPORT), Some(&[17u8, 0, 0, 0][..]));
            fields.push((attr::XOR_RELAYED_ADDRESS, stun::encode_address(RELAYED.parse().expect("addr"), &m.transaction, true)));
            fields.push((attr::XOR_MAPPED_ADDRESS, stun::encode_address("11.0.0.2:40000".parse().expect("addr"), &m.transaction, true)));
            fields.push((attr::LIFETIME, 600u32.to_be_bytes().to_vec()));
        }
        let list: Vec<(u16, &[u8])> = fields.iter().map(|(t, v)| (*t, v.as_slice())).collect();
        Some(encode(m.method, class::SUCCESS, &m.transaction, &list, Some(&self.key)))
    }
    fn error(&self, m: &StunMessage<'_>, code: u16) -> Vec<u8> {
        let error = [0, 0, (code / 100) as u8, (code % 100) as u8, b'n', b'o'];
        encode(m.method, class::ERROR, &m.transaction, &[(attr::ERROR_CODE, &error), (attr::REALM, REALM), (attr::NONCE, &self.nonce)], None)
    }
}

/// Runs request/response rounds until the client sends nothing more.
fn exchange(client: &mut TurnClient, server: &mut FakeTurn, now: f64) -> Vec<Vec<u8>> {
    let mut indications = Vec::new();
    for _ in 0..16 {
        let sent: Vec<Vec<u8>> = client.transmit().collect();
        if sent.is_empty() {
            break;
        }
        for packet in sent {
            match server.answer(&packet) {
                Some(reply) => assert!(client.handle(&reply, now).is_none()),
                None => indications.push(packet),
            }
        }
    }
    indications
}

fn ready_client() -> (TurnClient, FakeTurn) {
    let mut client = TurnClient::new(SERVER.parse().expect("addr"), "1700000000:peer", "secret");
    let mut server = FakeTurn::new();
    client.allocate(0.0);
    client.permit(&["11.0.0.3".parse().expect("ip"), "192.0.2.9".parse().expect("ip")], 0.0);
    // Peers of the other family are left out: they would fail the whole request.
    client.permit(&["2001:db8::1".parse().expect("ip")], 0.0);
    exchange(&mut client, &mut server, 0.0);
    (client, server)
}

#[test]
fn allocates_with_long_term_credentials_then_permits() {
    let (mut client, _) = ready_client();
    assert_eq!(client.state(), TurnState::Ready);
    let events: Vec<TurnEvent> = client.events().collect();
    assert_eq!(
        events,
        vec![TurnEvent::Ready { relayed: RELAYED.parse().expect("addr"), mapped: Some("11.0.0.2:40000".parse().expect("addr")), lifetime: 600 }]
    );
    assert_eq!(client.permissions().len(), 2);
}

#[test]
fn sends_through_indication_then_channel_and_receives_both() {
    let (mut client, mut server) = ready_client();
    let peer: SocketAddr = "11.0.0.3:5000".parse().expect("addr");
    let mut out = Vec::new();
    assert!(client.send_to(peer, b"hello", &mut out, 10.0));
    // A Send indication now; a ChannelBind request is on its way.
    let m = stun::decode_message(&out).expect("send indication");
    assert_eq!((m.method, m.klass), (method::SEND, class::INDICATION));
    assert_eq!(m.address(attr::XOR_PEER_ADDRESS), Some(peer));
    assert_eq!(m.attr(attr::DATA), Some(&b"hello"[..]));
    exchange(&mut client, &mut server, 10.0);
    assert!(client.send_to(peer, b"hello", &mut out, 20.0));
    assert_eq!(out, [0x40, 0x00, 0x00, 0x05, b'h', b'e', b'l', b'l', b'o']);

    // Inbound ChannelData and Data indications reach the owner with the peer address.
    let inbound = [0x40, 0x00, 0x00, 0x02, 9, 8, 0, 0];
    assert_eq!(client.handle(&inbound, 30.0), Some((peer, &[9u8, 8][..])));
    let tx = [3u8; 12];
    let data = encode(method::DATA, class::INDICATION, &tx, &[(attr::XOR_PEER_ADDRESS, &stun::encode_address(peer, &tx, true)), (attr::DATA, b"abc")], None);
    assert_eq!(client.handle(&data, 30.0), Some((peer, &b"abc"[..])));
    // ChannelData on an unknown channel, or shorter than it says, is dropped.
    assert_eq!(client.handle(&[0x40, 0x07, 0x00, 0x01, 1], 30.0), None);
    assert_eq!(client.handle(&[0x40, 0x00, 0x00, 0x09, 1], 30.0), None);
}

#[test]
fn refreshes_allocation_permissions_and_channels_and_renews_a_stale_nonce() {
    let (mut client, mut server) = ready_client();
    let peer: SocketAddr = "11.0.0.3:5000".parse().expect("addr");
    let mut out = Vec::new();
    client.send_to(peer, b"x", &mut out, 0.0);
    exchange(&mut client, &mut server, 0.0);
    assert_eq!(client.events().count(), 1, "the allocation's Ready");
    let count = |packets: &[Vec<u8>], wanted: u16| packets.iter().filter(|p| stun::decode_message(p).is_some_and(|m| m.method == wanted)).count();

    // At 300 s the permissions (every 240 s) and the allocation (half its 600 s lifetime) are
    // due, with the nonce gone stale: each request is renewed once, then succeeds.
    let at = 300_000.0;
    server.stale_once = true;
    client.poll(at);
    let sent: Vec<Vec<u8>> = client.transmit().collect();
    assert_eq!(count(&sent, method::CREATE_PERMISSION), 1);
    assert_eq!(count(&sent, method::REFRESH), 1, "allocation refresh is due at half its lifetime");
    for packet in sent {
        if let Some(reply) = server.answer(&packet) {
            client.handle(&reply, at);
        }
    }
    exchange(&mut client, &mut server, at);
    assert_eq!(client.state(), TurnState::Ready);
    assert!(client.events().next().is_none());

    client.poll(CHANNEL_REFRESH_MS);
    let sent: Vec<Vec<u8>> = client.transmit().collect();
    assert_eq!(count(&sent, method::CHANNEL_BIND), 1);
}

#[test]
fn an_unanswered_request_fails_after_its_retries() {
    let mut client = TurnClient::new(SERVER.parse().expect("addr"), "u", "p");
    client.allocate(0.0);
    let mut sent = client.transmit().count();
    let mut now = 0.0;
    while client.state() == TurnState::Allocating && now < 60_000.0 {
        now += 50.0;
        client.poll(now);
        sent += client.transmit().count();
    }
    assert_eq!(sent, 5);
    assert!((7_750.0..=7_800.0).contains(&now), "gave up at {now}");
    assert_eq!(client.events().collect::<Vec<_>>(), vec![TurnEvent::Failed("TURN server did not answer".to_string())]);
}

#[test]
fn a_refused_allocation_fails_and_close_releases_a_live_one() {
    let mut client = TurnClient::new(SERVER.parse().expect("addr"), "u", "p");
    client.allocate(0.0);
    let request: Vec<Vec<u8>> = client.transmit().collect();
    let m = stun::decode_message(&request[0]).expect("allocate");
    let error = [0, 0, 4, 86];
    let reply = encode(method::ALLOCATE, class::ERROR, &m.transaction, &[(attr::ERROR_CODE, &error)], None);
    client.handle(&reply, 1.0);
    assert_eq!(client.state(), TurnState::Failed);
    assert_eq!(client.events().collect::<Vec<_>>(), vec![TurnEvent::Failed("TURN 486".to_string())]);

    let (mut live, _) = ready_client();
    live.close();
    let sent: Vec<Vec<u8>> = live.transmit().collect();
    let m = stun::decode_message(&sent[0]).expect("refresh");
    assert_eq!(m.method, method::REFRESH);
    assert_eq!(m.attr(attr::LIFETIME), Some(&[0u8, 0, 0, 0][..]));
    let mut out = Vec::new();
    assert!(!live.send_to("11.0.0.3:5000".parse().expect("addr"), b"x", &mut out, 0.0));
}

#[test]
fn a_success_without_valid_integrity_is_refused() {
    let mut client = TurnClient::new(SERVER.parse().expect("addr"), "u", "p");
    let mut server = FakeTurn::new();
    client.allocate(0.0);
    let first: Vec<Vec<u8>> = client.transmit().collect();
    let challenge = server.answer(&first[0]).expect("401");
    client.handle(&challenge, 0.0);
    let second: Vec<Vec<u8>> = client.transmit().collect();
    let m = stun::decode_message(&second[0]).expect("allocate");
    let forged = encode(method::ALLOCATE, class::SUCCESS, &m.transaction, &[(attr::XOR_RELAYED_ADDRESS, &stun::encode_address(RELAYED.parse().expect("addr"), &m.transaction, true))], Some(&[0u8; 16]));
    client.handle(&forged, 0.0);
    assert_eq!(client.state(), TurnState::Failed);
}

#[test]
fn hostile_datagrams_never_panic_or_grow_state() {
    let (mut client, _) = ready_client();
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let tx = [1u8; 12];
    let templates = [
        encode(method::DATA, class::INDICATION, &tx, &[(attr::XOR_PEER_ADDRESS, &stun::encode_address("11.0.0.3:5000".parse().expect("addr"), &tx, true)), (attr::DATA, b"abc")], None),
        encode(method::ALLOCATE, class::SUCCESS, &tx, &[(attr::XOR_RELAYED_ADDRESS, &[0, 1, 2])], None),
        encode(method::REFRESH, class::ERROR, &tx, &[(attr::ERROR_CODE, &[0, 0, 4, 38]), (attr::NONCE, b"n")], None),
        vec![0x40, 0x00, 0xff, 0xff, 1, 2, 3],
    ];
    for round in 0..200_000u32 {
        let length = (next() % 1500) as usize;
        let mut packet: Vec<u8> = if round % 2 == 0 {
            (0..length).map(|_| next() as u8).collect()
        } else {
            let mut p = templates[(next() % 4) as usize].clone();
            let flips = next() % 6;
            for _ in 0..flips {
                if !p.is_empty() {
                    let at = (next() as usize) % p.len();
                    p[at] = next() as u8;
                }
            }
            p.truncate((next() as usize) % (p.len() + 1));
            p
        };
        if round % 3 == 0 && packet.len() >= 8 {
            // Make it look like STUN so it gets past the first checks.
            packet[0] &= 0x03;
            packet[4..8].copy_from_slice(&MAGIC.to_be_bytes());
        }
        let _ = client.handle(&packet, f64::from(round));
        client.poll(f64::from(round));
        let _ = client.transmit().count();
    }
    assert!(client.transactions.len() <= MAX_TRANSACTIONS);
    assert!(client.permissions.len() <= MAX_PERMISSIONS);
    assert!(client.channels.len() <= MAX_CHANNELS);
    let _ = client.events().count();
}

#[test]
fn permissions_and_channels_are_capped() {
    let (mut client, _) = ready_client();
    let many: Vec<IpAddr> = (1..=200u8).map(|i| IpAddr::V4(Ipv4Addr::new(11, 1, 0, i))).collect();
    client.permit(&many, 0.0);
    assert_eq!(client.permissions().len(), MAX_PERMISSIONS);
    let mut out = Vec::new();
    for port in 1..=200u16 {
        client.send_to(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(11, 0, 0, 3)), port), b"x", &mut out, 0.0);
    }
    assert_eq!(client.channels.len(), MAX_CHANNELS);
    assert!(client.transactions.len() <= MAX_TRANSACTIONS);
}

// ---- An exchange recorded against coturn, replayed byte for byte ----

/// Where the scripted exchange's datagrams go: a live TURN server, or a recording of one.
trait Wire {
    fn out(&mut self, packet: &[u8]);
    fn input(&mut self) -> Vec<u8>;
    /// The peer sends `data` to the relayed address (after checking what it received).
    fn peer_sends(&mut self, relayed: SocketAddr, data: &[u8]);
}

/// Allocate (401, then with credentials), permit the peer, send through a Send indication, bind
/// a channel, send ChannelData, receive the peer's reply, release the allocation.
fn scripted_exchange(wire: &mut dyn Wire, server: SocketAddr, peer: SocketAddr) {
    let mut client = TurnClient::new(server, "bfe", "rehearsal");
    client.set_transaction_seed(0x6266_6574_7572_6e31);
    client.allocate(0.0);
    client.permit(&[peer.ip()], 0.0);
    loop {
        let sent: Vec<Vec<u8>> = client.transmit().collect();
        for packet in &sent {
            wire.out(packet);
        }
        if client.state() != TurnState::Allocating {
            break;
        }
        let reply = wire.input();
        assert!(client.handle(&reply, 0.0).is_none());
    }
    assert_eq!(client.state(), TurnState::Ready, "{:?}", client.events().collect::<Vec<_>>());
    let relayed = client.relayed().expect("relayed address");
    let mut buf = Vec::new();
    assert!(client.send_to(peer, b"bfe relay test 1", &mut buf, 0.0));
    wire.out(&buf);
    let bind: Vec<Vec<u8>> = client.transmit().collect();
    assert_eq!(bind.len(), 1);
    wire.out(&bind[0]);
    let reply = wire.input();
    assert!(client.handle(&reply, 0.0).is_none());
    assert!(client.send_to(peer, b"bfe relay test 2", &mut buf, 0.0));
    assert_eq!(buf[..2], [0x40, 0x00], "ChannelData once bound");
    wire.out(&buf);
    wire.peer_sends(relayed, b"bfe relay reply");
    let reply = wire.input();
    assert_eq!(client.handle(&reply, 0.0), Some((peer, &b"bfe relay reply"[..])));
    client.close();
    let release: Vec<Vec<u8>> = client.transmit().collect();
    wire.out(&release[0]);
    let reply = wire.input();
    let m = stun::decode_message(&reply).expect("refresh answer");
    assert_eq!((m.method, m.klass), (method::REFRESH, class::SUCCESS));
}

struct Replay {
    recorded: &'static [(&'static str, &'static str)],
    at: usize,
}

impl Wire for Replay {
    fn out(&mut self, packet: &[u8]) {
        let (kind, bytes) = self.recorded[self.at];
        self.at += 1;
        assert_eq!(kind, "out", "step {}", self.at);
        assert_eq!(to_hex(packet), bytes.replace(char::is_whitespace, ""), "step {}", self.at);
    }
    fn input(&mut self) -> Vec<u8> {
        let (kind, bytes) = self.recorded[self.at];
        self.at += 1;
        assert_eq!(kind, "in", "step {}", self.at);
        hex(bytes)
    }
    fn peer_sends(&mut self, _relayed: SocketAddr, _data: &[u8]) {}
}

#[test]
fn replays_an_exchange_recorded_with_coturn() {
    let mut wire = Replay { recorded: coturn::RECORDED, at: 0 };
    scripted_exchange(&mut wire, coturn::SERVER.parse().expect("addr"), coturn::PEER.parse().expect("addr"));
    assert_eq!(wire.at, coturn::RECORDED.len());
}

struct Live {
    socket: std::net::UdpSocket,
    peer: std::net::UdpSocket,
    server: SocketAddr,
}

impl Wire for Live {
    fn out(&mut self, packet: &[u8]) {
        println!("    (\"out\", \"{}\"),", to_hex(packet));
        self.socket.send_to(packet, self.server).expect("send");
    }
    fn input(&mut self) -> Vec<u8> {
        let mut buf = [0u8; 1500];
        let (n, _) = self.socket.recv_from(&mut buf).expect("TURN server answers");
        println!("    (\"in\", \"{}\"),", to_hex(&buf[..n]));
        buf[..n].to_vec()
    }
    fn peer_sends(&mut self, relayed: SocketAddr, data: &[u8]) {
        let mut buf = [0u8; 1500];
        for expected in [&b"bfe relay test 1"[..], b"bfe relay test 2"] {
            let (n, from) = self.peer.recv_from(&mut buf).expect("peer receives");
            assert_eq!((&buf[..n], from), (expected, relayed));
        }
        self.peer.send_to(data, relayed).expect("peer sends");
    }
}

/// Records the fixture in `coturn.rs`: `BFE_TURN_RECORD="<server ip:port> <local ip>"`, run where
/// the local ip reaches a coturn that has the static user bfe / rehearsal (see that file).
#[test]
#[ignore = "needs a coturn; records the fixture"]
fn record_an_exchange_with_coturn() {
    let Ok(setting) = std::env::var("BFE_TURN_RECORD") else {
        return;
    };
    let mut parts = setting.split_whitespace();
    let server: SocketAddr = parts.next().and_then(|s| s.parse().ok()).expect("server address");
    let local: IpAddr = parts.next().and_then(|s| s.parse().ok()).expect("local address");
    let bind = |ip| {
        let socket = std::net::UdpSocket::bind(SocketAddr::new(ip, 0)).expect("bind");
        socket.set_read_timeout(Some(std::time::Duration::from_secs(3))).expect("timeout");
        socket
    };
    let mut wire = Live { socket: bind(local), peer: bind(local), server };
    let peer = wire.peer.local_addr().expect("peer address");
    println!("pub const SERVER: &str = \"{server}\";\npub const PEER: &str = \"{peer}\";\npub const RECORDED: &[(&str, &str)] = &[");
    scripted_exchange(&mut wire, server, peer);
    println!("];");
}

#[cfg(test)]
mod coturn;
