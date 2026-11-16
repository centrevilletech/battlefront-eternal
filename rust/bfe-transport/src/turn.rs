//! TURN (RFC 8656) UDP allocation client as a state machine without sockets.
//!
//! The owner of the socket feeds every datagram from the TURN server to
//! [`TurnClient::handle`], calls [`TurnClient::poll`] regularly, and sends what
//! [`TurnClient::transmit`] yields to the TURN server. Data for a peer goes
//! through [`TurnClient::send_to`]: a Send indication at first, ChannelData
//! (4 bytes of overhead) once a channel is bound. Long-term credentials work as
//! in RFC 8489: the first request goes out without them, the 401 answer names
//! the realm and nonce, and every later request carries USERNAME, REALM, NONCE
//! and a MESSAGE-INTEGRITY keyed by MD5(username:realm:password). A stale nonce
//! (438) is renewed once per request.
//!
//! Everything here reads network input, so nothing may panic and nothing grows
//! without bound: permissions, channels and open transactions are capped, and a
//! response that matches no open transaction is ignored.

use std::net::{IpAddr, SocketAddr};

use hmac::{Hmac, Mac};
use sha1::Sha1;

use crate::server::{normalize_ip, normalize_socket_addr, valid_peer_ip};
use crate::stun::{self, attr, class, method, StunMessage, MAGIC};

/// First retransmission timeout; each retry doubles it.
const RTO_MS: f64 = 250.0;
/// Transmissions of one request before it fails (about 7.75 s in all).
const ATTEMPTS: u8 = 5;
const FIRST_CHANNEL: u16 = 0x4000;
const LAST_CHANNEL: u16 = 0x4fff;
/// Permissions last 300 s and channel bindings 600 s: refresh both well before.
pub const PERMISSION_REFRESH_MS: f64 = 240_000.0;
pub const CHANNEL_REFRESH_MS: f64 = 540_000.0;
/// Allocation lifetime asked for, in seconds.
pub const LIFETIME_S: u32 = 600;
/// Peer addresses one allocation may permit, and channels it may bind.
pub const MAX_PERMISSIONS: usize = 16;
pub const MAX_CHANNELS: usize = 16;
/// Addresses carried by one CreatePermission request.
const PERMISSIONS_PER_REQUEST: usize = 8;
/// Open transactions at once; the client itself never needs more.
const MAX_TRANSACTIONS: usize = 32;

/// Where the allocation stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnState {
    /// Allocate sent (or its first permissions), no relayed address usable yet.
    Allocating,
    /// Relayed address allocated and the first permissions installed.
    Ready,
    /// The allocation failed or was lost; nothing more is sent.
    Failed,
    /// Closed by its owner.
    Closed,
}

/// What happened, for the owner of the allocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnEvent {
    /// The relayed address is usable: peers named before this point are permitted.
    Ready { relayed: SocketAddr, mapped: Option<SocketAddr>, lifetime: u32 },
    /// The allocation failed or expired (Allocate, Refresh or a permission refused or unanswered).
    Failed(String),
}

#[derive(Clone, Debug)]
enum Purpose {
    Allocate,
    Refresh(u32),
    /// `initial` marks the permissions that must land before the allocation is Ready.
    Permission { peers: Vec<IpAddr>, initial: bool },
    ChannelBind { number: u16, peer: SocketAddr },
}

struct Transaction {
    id: [u8; 12],
    purpose: Purpose,
    packet: Vec<u8>,
    sent: u8,
    next_at: f64,
    /// Credentials were attached (the response must then pass integrity).
    authenticated: bool,
    /// Already resent once after a 401 or 438.
    retried: bool,
}

struct Channel {
    number: u16,
    peer: SocketAddr,
    bound: bool,
    refresh_at: f64,
}

/// One TURN allocation over UDP (see the module comment).
pub struct TurnClient {
    server: SocketAddr,
    username: String,
    password: String,
    realm: Vec<u8>,
    nonce: Vec<u8>,
    key: Option<[u8; 16]>,
    state: TurnState,
    relayed: Option<SocketAddr>,
    mapped: Option<SocketAddr>,
    lifetime: u32,
    refresh_at: f64,
    permission_at: Option<f64>,
    permissions: Vec<IpAddr>,
    /// Peers named before the allocation succeeded.
    waiting: Vec<IpAddr>,
    channels: Vec<Channel>,
    next_channel: u16,
    transactions: Vec<Transaction>,
    outbox: Vec<Vec<u8>>,
    events: Vec<TurnEvent>,
    /// Deterministic transaction ids (tests and recordings); random when None.
    seed: Option<u64>,
    counter: u32,
}

impl TurnClient {
    /// A client for the TURN server at `server`. Nothing is sent before [`TurnClient::allocate`].
    pub fn new(server: SocketAddr, username: &str, password: &str) -> Self {
        Self {
            server: normalize_socket_addr(server),
            username: username.to_string(),
            password: password.to_string(),
            realm: Vec::new(),
            nonce: Vec::new(),
            key: None,
            state: TurnState::Allocating,
            relayed: None,
            mapped: None,
            lifetime: LIFETIME_S,
            refresh_at: f64::INFINITY,
            permission_at: None,
            permissions: Vec::new(),
            waiting: Vec::new(),
            channels: Vec::new(),
            next_channel: FIRST_CHANNEL,
            transactions: Vec::new(),
            outbox: Vec::new(),
            events: Vec::new(),
            seed: None,
            counter: 0,
        }
    }

    /// Transaction ids become `seed` followed by a counter (for replaying a recorded exchange).
    pub fn set_transaction_seed(&mut self, seed: u64) {
        self.seed = Some(seed);
    }

    /// The TURN server this allocation talks to.
    pub fn server(&self) -> SocketAddr {
        self.server
    }
    pub fn state(&self) -> TurnState {
        self.state
    }
    pub fn relayed(&self) -> Option<SocketAddr> {
        self.relayed
    }
    /// Peer addresses currently permitted.
    pub fn permissions(&self) -> &[IpAddr] {
        &self.permissions
    }

    /// Sends the Allocate request (UDP transport, the server's address family).
    pub fn allocate(&mut self, now: f64) {
        if self.state == TurnState::Allocating && self.relayed.is_none() && self.transactions.is_empty() {
            self.request(Purpose::Allocate, now, false);
        }
    }

    /// Lets packets from these peer addresses through the relay (ports do not matter) and keeps
    /// them permitted from then on. Addresses of the other family, invalid ones and any beyond
    /// [`MAX_PERMISSIONS`] are left out: one bad address would fail the whole request (RFC 6156).
    pub fn permit(&mut self, addresses: &[IpAddr], now: f64) {
        let mut fresh = Vec::new();
        for &raw in addresses {
            let ip = normalize_ip(raw);
            if !valid_peer_ip(ip) || ip.is_ipv6() != self.family_v6() {
                continue;
            }
            if self.permissions.contains(&ip) || self.waiting.contains(&ip) || fresh.contains(&ip) {
                continue;
            }
            if self.permissions.len() + self.waiting.len() + fresh.len() >= MAX_PERMISSIONS {
                break;
            }
            fresh.push(ip);
        }
        if fresh.is_empty() {
            return;
        }
        match self.state {
            TurnState::Ready => {
                self.permissions.extend_from_slice(&fresh);
                for group in fresh.chunks(PERMISSIONS_PER_REQUEST) {
                    self.request(Purpose::Permission { peers: group.to_vec(), initial: false }, now, false);
                }
                if self.permission_at.is_none() {
                    self.permission_at = Some(now + PERMISSION_REFRESH_MS);
                }
            }
            TurnState::Allocating => self.waiting.extend_from_slice(&fresh),
            TurnState::Failed | TurnState::Closed => {}
        }
    }

    /// Wraps `data` for `peer` into `out` (ChannelData once a channel is bound, a Send indication
    /// before; the first send also starts binding a channel). Returns false when the allocation
    /// is not ready, and `out` must then not be sent.
    pub fn send_to(&mut self, peer: SocketAddr, data: &[u8], out: &mut Vec<u8>, now: f64) -> bool {
        if self.state != TurnState::Ready || data.len() > u16::MAX as usize - 64 {
            return false;
        }
        let peer = normalize_socket_addr(peer);
        out.clear();
        match self.channels.iter().position(|c| c.peer == peer) {
            Some(i) if self.channels[i].bound => {
                out.extend_from_slice(&self.channels[i].number.to_be_bytes());
                out.extend_from_slice(&(data.len() as u16).to_be_bytes());
                out.extend_from_slice(data);
                return true;
            }
            Some(_) => {}
            None => {
                if self.channels.len() < MAX_CHANNELS && self.next_channel <= LAST_CHANNEL {
                    let number = self.next_channel;
                    self.next_channel += 1;
                    self.channels.push(Channel { number, peer, bound: false, refresh_at: f64::INFINITY });
                    self.request(Purpose::ChannelBind { number, peer }, now, false);
                }
            }
        }
        let transaction = self.transaction_id();
        let peer_attr = stun::encode_address(peer, &transaction, true);
        *out = encode(method::SEND, class::INDICATION, &transaction, &[(attr::XOR_PEER_ADDRESS, &peer_attr), (attr::DATA, data)], None);
        true
    }

    /// A datagram from the TURN server. Returns the peer and payload of relayed data (Data
    /// indication or ChannelData); responses are consumed here and return None. Anything that is
    /// not a well-formed TURN message is ignored.
    pub fn handle<'p>(&mut self, packet: &'p [u8], now: f64) -> Option<(SocketAddr, &'p [u8])> {
        if matches!(self.state, TurnState::Closed | TurnState::Failed) {
            return None;
        }
        if stun::is_channel_data(packet) {
            let number = u16::from_be_bytes([packet[0], packet[1]]);
            let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
            let channel = self.channels.iter().find(|c| c.number == number)?;
            let data = packet.get(4..4 + length)?;
            return Some((channel.peer, data));
        }
        let message = stun::decode_message(packet)?;
        if message.method == method::DATA && message.klass == class::INDICATION {
            let peer = message.address(attr::XOR_PEER_ADDRESS)?;
            let data = message.attr(attr::DATA)?;
            return Some((normalize_socket_addr(peer), data));
        }
        if message.klass != class::SUCCESS && message.klass != class::ERROR {
            return None;
        }
        let index = self.transactions.iter().position(|t| t.id == message.transaction)?;
        let transaction = self.transactions.swap_remove(index);
        self.respond(transaction, &message, now);
        None
    }

    /// Retransmits unanswered requests and refreshes the allocation, permissions and channels
    /// when due. A request unanswered after every retry fails like an error response.
    pub fn poll(&mut self, now: f64) {
        if matches!(self.state, TurnState::Closed | TurnState::Failed) {
            return;
        }
        let mut expired = Vec::new();
        for (i, t) in self.transactions.iter_mut().enumerate() {
            if now < t.next_at {
                continue;
            }
            if t.sent >= ATTEMPTS {
                expired.push(i);
                continue;
            }
            self.outbox.push(t.packet.clone());
            t.next_at = now + RTO_MS * f64::from(1u32 << t.sent);
            t.sent += 1;
        }
        for i in expired.into_iter().rev() {
            let t = self.transactions.swap_remove(i);
            self.fail_request(&t.purpose, "TURN server did not answer".to_string());
        }
        if self.state != TurnState::Ready {
            return;
        }
        if now >= self.refresh_at {
            self.refresh_at = f64::INFINITY;
            self.request(Purpose::Refresh(self.lifetime), now, false);
        }
        if self.permission_at.is_some_and(|at| now >= at) {
            self.permission_at = Some(now + PERMISSION_REFRESH_MS);
            let peers = self.permissions.clone();
            for group in peers.chunks(PERMISSIONS_PER_REQUEST) {
                self.request(Purpose::Permission { peers: group.to_vec(), initial: false }, now, false);
            }
        }
        let due: Vec<(u16, SocketAddr)> = self
            .channels
            .iter_mut()
            .filter(|c| c.bound && now >= c.refresh_at)
            .map(|c| {
                c.refresh_at = f64::INFINITY;
                (c.number, c.peer)
            })
            .collect();
        for (number, peer) in due {
            self.request(Purpose::ChannelBind { number, peer }, now, false);
        }
    }

    /// Releases the allocation (a Refresh with lifetime 0, sent once) and stops everything.
    pub fn close(&mut self) {
        if self.state == TurnState::Closed {
            return;
        }
        if self.relayed.is_some() && self.state != TurnState::Failed {
            let transaction = self.transaction_id();
            let lifetime = 0u32.to_be_bytes();
            let packet = self.encode_request(method::REFRESH, &transaction, &[(attr::LIFETIME, lifetime.to_vec())]);
            self.outbox.push(packet);
        }
        self.state = TurnState::Closed;
        self.transactions.clear();
    }

    /// Datagrams for the TURN server, in order.
    pub fn transmit(&mut self) -> std::vec::Drain<'_, Vec<u8>> {
        self.outbox.drain(..)
    }

    /// Events since the last call.
    pub fn events(&mut self) -> std::vec::Drain<'_, TurnEvent> {
        self.events.drain(..)
    }

    fn family_v6(&self) -> bool {
        self.relayed.map_or(self.server.is_ipv6(), |r| r.is_ipv6())
    }

    fn transaction_id(&mut self) -> [u8; 12] {
        self.counter = self.counter.wrapping_add(1);
        match self.seed {
            Some(seed) => {
                let mut id = [0u8; 12];
                id[..8].copy_from_slice(&seed.to_be_bytes());
                id[8..].copy_from_slice(&self.counter.to_be_bytes());
                id
            }
            // Randomness failing leaves a counter-based id: still unique per allocation.
            None => stun::new_transaction().unwrap_or_else(|_| {
                let mut id = [0u8; 12];
                id[8..].copy_from_slice(&self.counter.to_be_bytes());
                id
            }),
        }
    }

    fn attributes(&self, purpose: &Purpose, transaction: &[u8; 12]) -> Vec<(u16, Vec<u8>)> {
        match purpose {
            Purpose::Allocate => {
                let mut list = vec![(attr::REQUESTED_TRANSPORT, vec![17, 0, 0, 0]), (attr::LIFETIME, LIFETIME_S.to_be_bytes().to_vec())];
                if self.server.is_ipv6() {
                    list.push((attr::REQUESTED_ADDRESS_FAMILY, vec![2, 0, 0, 0]));
                }
                list
            }
            Purpose::Refresh(lifetime) => vec![(attr::LIFETIME, lifetime.to_be_bytes().to_vec())],
            Purpose::Permission { peers, .. } => peers
                .iter()
                .map(|&ip| (attr::XOR_PEER_ADDRESS, stun::encode_address(SocketAddr::new(ip, 0), transaction, true)))
                .collect(),
            Purpose::ChannelBind { number, peer } => {
                let mut channel = vec![0u8; 4];
                channel[..2].copy_from_slice(&number.to_be_bytes());
                vec![(attr::CHANNEL_NUMBER, channel), (attr::XOR_PEER_ADDRESS, stun::encode_address(*peer, transaction, true))]
            }
        }
    }

    fn encode_request(&self, method: u16, transaction: &[u8; 12], fields: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let mut list: Vec<(u16, &[u8])> = fields.iter().map(|(t, v)| (*t, v.as_slice())).collect();
        if self.key.is_some() {
            list.push((attr::USERNAME, self.username.as_bytes()));
            list.push((attr::REALM, &self.realm));
            list.push((attr::NONCE, &self.nonce));
        }
        encode(method, class::REQUEST, transaction, &list, self.key.as_ref())
    }

    fn request(&mut self, purpose: Purpose, now: f64, retried: bool) {
        if self.transactions.len() >= MAX_TRANSACTIONS {
            self.fail_request(&purpose, "TURN client has too many open requests".to_string());
            return;
        }
        let id = self.transaction_id();
        let fields = self.attributes(&purpose, &id);
        let method = match purpose {
            Purpose::Allocate => method::ALLOCATE,
            Purpose::Refresh(_) => method::REFRESH,
            Purpose::Permission { .. } => method::CREATE_PERMISSION,
            Purpose::ChannelBind { .. } => method::CHANNEL_BIND,
        };
        let packet = self.encode_request(method, &id, &fields);
        self.outbox.push(packet.clone());
        self.transactions.push(Transaction {
            id,
            purpose,
            packet,
            sent: 1,
            next_at: now + RTO_MS,
            authenticated: self.key.is_some(),
            retried,
        });
    }

    fn respond(&mut self, t: Transaction, message: &StunMessage<'_>, now: f64) {
        if message.klass == class::SUCCESS {
            if t.authenticated && !self.key.is_some_and(|key| check_integrity(message, &key)) {
                self.fail_request(&t.purpose, "TURN response failed integrity".to_string());
                return;
            }
            self.succeed(t.purpose, message, now);
            return;
        }
        let (code, reason) = message.error_code().unwrap_or((0, String::new()));
        let nonce = message.attr(attr::NONCE);
        if (code == 401 || code == 438) && !t.retried {
            if let Some(nonce) = nonce.filter(|n| !n.is_empty() && n.len() <= 763) {
                if let Some(realm) = message.attr(attr::REALM).filter(|r| r.len() <= 763) {
                    self.realm = realm.to_vec();
                }
                self.nonce = nonce.to_vec();
                self.key = Some(long_term_key(&self.username, &self.realm, &self.password));
                self.request(t.purpose, now, true);
                return;
            }
        }
        let reason: String = reason.chars().filter(|c| !c.is_control()).take(80).collect();
        self.fail_request(&t.purpose, format!("TURN {code} {reason}").trim_end().to_string());
    }

    fn succeed(&mut self, purpose: Purpose, message: &StunMessage<'_>, now: f64) {
        match purpose {
            Purpose::Allocate => {
                let Some(relayed) = message.address(attr::XOR_RELAYED_ADDRESS) else {
                    self.fail("TURN allocation has no relayed address".to_string());
                    return;
                };
                self.relayed = Some(normalize_socket_addr(relayed));
                self.mapped = message.address(attr::XOR_MAPPED_ADDRESS).map(normalize_socket_addr);
                self.lifetime = message
                    .attr(attr::LIFETIME)
                    .and_then(|v| v.get(..4))
                    .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
                    .unwrap_or(LIFETIME_S)
                    .clamp(60, 3600);
                self.schedule_refresh(now);
                // Peers of the other family than the relayed address cannot be permitted.
                let family_v6 = self.family_v6();
                let waiting: Vec<IpAddr> = std::mem::take(&mut self.waiting).into_iter().filter(|ip| ip.is_ipv6() == family_v6).collect();
                if waiting.is_empty() {
                    self.ready();
                } else {
                    self.permissions.extend_from_slice(&waiting);
                    self.permission_at = Some(now + PERMISSION_REFRESH_MS);
                    for group in waiting.chunks(PERMISSIONS_PER_REQUEST) {
                        self.request(Purpose::Permission { peers: group.to_vec(), initial: true }, now, false);
                    }
                }
            }
            Purpose::Refresh(_) => self.schedule_refresh(now),
            Purpose::Permission { initial, .. } => {
                let pending = self.transactions.iter().any(|t| matches!(t.purpose, Purpose::Permission { initial: true, .. }));
                if initial && !pending && self.state == TurnState::Allocating {
                    self.ready();
                }
            }
            Purpose::ChannelBind { number, .. } => {
                if let Some(channel) = self.channels.iter_mut().find(|c| c.number == number) {
                    channel.bound = true;
                    channel.refresh_at = now + CHANNEL_REFRESH_MS;
                }
            }
        }
    }

    fn schedule_refresh(&mut self, now: f64) {
        self.refresh_at = now + (f64::from(self.lifetime) * 500.0).max(30_000.0);
    }

    fn ready(&mut self) {
        if let Some(relayed) = self.relayed {
            self.state = TurnState::Ready;
            self.events.push(TurnEvent::Ready { relayed, mapped: self.mapped, lifetime: self.lifetime });
        }
    }

    fn fail_request(&mut self, purpose: &Purpose, reason: String) {
        match purpose {
            // A channel that cannot be bound leaves that peer on Send indications.
            Purpose::ChannelBind { number, .. } => {
                if let Some(i) = self.channels.iter().position(|c| c.number == *number && !c.bound) {
                    self.channels.swap_remove(i);
                } else if let Some(c) = self.channels.iter_mut().find(|c| c.number == *number) {
                    // A refresh failed: try again at the next poll after one more period.
                    c.refresh_at = f64::INFINITY;
                    c.bound = false;
                }
            }
            Purpose::Allocate | Purpose::Refresh(_) | Purpose::Permission { .. } => self.fail(reason),
        }
    }

    fn fail(&mut self, reason: String) {
        if matches!(self.state, TurnState::Failed | TurnState::Closed) {
            return;
        }
        self.state = TurnState::Failed;
        self.transactions.clear();
        self.events.push(TurnEvent::Failed(reason));
    }
}

/// MD5(username ":" realm ":" password), the long-term credential key (RFC 8489 9.2.2).
pub fn long_term_key(username: &str, realm: &[u8], password: &str) -> [u8; 16] {
    let mut input = Vec::with_capacity(username.len() + realm.len() + password.len() + 2);
    input.extend_from_slice(username.as_bytes());
    input.push(b':');
    input.extend_from_slice(realm);
    input.push(b':');
    input.extend_from_slice(password.as_bytes());
    md5(&input)
}

/// Encodes a STUN message with padded attributes, an optional MESSAGE-INTEGRITY (HMAC-SHA1 under
/// `key`) and the trailing FINGERPRINT.
pub fn encode(method: u16, klass: u8, transaction: &[u8; 12], attributes: &[(u16, &[u8])], key: Option<&[u8; 16]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(&stun::message_type(method, klass).to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&MAGIC.to_be_bytes());
    out.extend_from_slice(transaction);
    for &(kind, value) in attributes {
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&(value.len() as u16).to_be_bytes());
        out.extend_from_slice(value);
        out.resize(out.len() + (4 - value.len() % 4) % 4, 0);
    }
    let set_length = |out: &mut Vec<u8>, length: usize| {
        let bytes = (length as u16).to_be_bytes();
        out[2] = bytes[0];
        out[3] = bytes[1];
    };
    if let Some(key) = key {
        let length = out.len() - 20 + 24;
        set_length(&mut out, length);
        let integrity = hmac_sha1(key, &out);
        out.extend_from_slice(&attr::MESSAGE_INTEGRITY.to_be_bytes());
        out.extend_from_slice(&20u16.to_be_bytes());
        out.extend_from_slice(&integrity);
    }
    let length = out.len() - 20 + 8;
    set_length(&mut out, length);
    let crc = crc32fast::hash(&out) ^ 0x5354_554e;
    out.extend_from_slice(&attr::FINGERPRINT.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

/// True when `message` carries a MESSAGE-INTEGRITY valid under `key` (compared in constant time).
pub fn check_integrity(message: &StunMessage<'_>, key: &[u8]) -> bool {
    let Some(at) = message.integrity_at else {
        return false;
    };
    let (Some(covered), Some(given)) = (message.packet.get(..at), message.packet.get(at + 4..at + 24)) else {
        return false;
    };
    let mut header = covered.to_vec();
    let length = ((at - 20 + 24) as u16).to_be_bytes();
    header[2] = length[0];
    header[3] = length[1];
    let Ok(mut mac) = <Hmac<Sha1> as Mac>::new_from_slice(key) else {
        return false;
    };
    mac.update(&header);
    mac.verify_slice(given).is_ok()
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    match <Hmac<Sha1> as Mac>::new_from_slice(key) {
        Ok(mut mac) => {
            mac.update(data);
            mac.finalize().into_bytes().into()
        }
        // HMAC takes keys of any length; this arm cannot be reached.
        Err(_) => [0; 20],
    }
}

/// MD5 (RFC 1321), only for the TURN long-term credential key.
pub fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16,
        23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee, 0xf57c_0faf, 0x4787_c62a, 0xa830_4613, 0xfd46_9501, 0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1,
        0x895c_d7be, 0x6b90_1122, 0xfd98_7193, 0xa679_438e, 0x49b4_0821, 0xf61e_2562, 0xc040_b340, 0x265e_5a51, 0xe9b6_c7aa, 0xd62f_105d, 0x0244_1453,
        0xd8a1_e681, 0xe7d3_fbc8, 0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed, 0xa9e3_e905, 0xfcef_a3f8, 0x676f_02d9, 0x8d2a_4c8a, 0xfffa_3942,
        0x8771_f681, 0x6d9d_6122, 0xfde5_380c, 0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70, 0x289b_7ec6, 0xeaa1_27fa, 0xd4ef_3085, 0x0488_1d05,
        0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665, 0xf429_2244, 0x432a_ff97, 0xab94_23a7, 0xfc93_a039, 0x655b_59c3, 0x8f0c_cc92, 0xffef_f47d,
        0x8584_5dd1, 0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1, 0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb, 0xeb86_d391,
    ];
    let mut state: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((input.len() as u64).wrapping_mul(8)).to_le_bytes());
    for block in message.chunks_exact(64) {
        let m: [u32; 16] = std::array::from_fn(|i| u32::from_le_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]));
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]).rotate_left(S[i]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(rotated);
        }
        state = [state[0].wrapping_add(a), state[1].wrapping_add(b), state[2].wrapping_add(c), state[3].wrapping_add(d)];
    }
    let mut out = [0u8; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests;
