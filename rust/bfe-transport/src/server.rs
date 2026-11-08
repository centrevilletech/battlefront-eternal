//! Server-side listening UDP endpoint state machine (`src/net/endpoint.js`
//! `UdpEndpoint` in server mode). Demultiplexes `INITIATION`, `CONFIRM`,
//! `DATA`/`DATA_PHASE`, `PUNCH`, and `STUN` datagrams, enforces per-prefix
//! handshake rate limits and session caps, and drives established sessions
//! without heap allocation on the steady-state packet path.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};

use crate::connection::{Closed, Connection, Lane, Sink, Stats as ConnectionStats};
use crate::crypto::{Identity, RetryTokens};
use crate::handshake::ServerHandshakes;
use crate::layout::{
    close_code as close, confirm as conf, data as d, limits as lim, packet,
};
use crate::stun;
use crate::Result;

pub const PUNCH_SCHEDULE_MS: [u64; 7] = [0, 40, 100, 200, 400, 800, 1500];
const MAX_DRAINING: usize = 256;
const DRAIN_MS: f64 = 2000.0;
const MAX_BUCKETS: usize = 4096;

/// `src/net/codec.js` `normalizeAddress`: unwraps IPv4-mapped IPv6 addresses
/// (`::ffff:a.b.c.d`) into canonical IPv4 so IPv4 peers on dual-stack sockets
/// share the same rate-limit and ban keys.
pub fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                IpAddr::V4(v4)
            } else {
                IpAddr::V6(v6)
            }
        }
    }
}

pub fn normalize_socket_addr(addr: SocketAddr) -> SocketAddr {
    SocketAddr::new(normalize_ip(addr.ip()), addr.port())
}

/// `src/net/endpoint.js` `validPeerAddress`: rejects unspecified, multicast,
/// and IPv4 broadcast addresses.
pub fn valid_peer_ip(ip: IpAddr) -> bool {
    match normalize_ip(ip) {
        IpAddr::V4(v4) => {
            let [a, _, _, d] = v4.octets();
            a != 0 && a < 224 && !(a == 255 && d == 255)
        }
        IpAddr::V6(v6) => !v6.is_unspecified() && !v6.is_multicast(),
    }
}

/// `src/net/endpoint.js` `loopbackAddress`: true (`127/8` and `::1`).
pub fn loopback_ip(ip: IpAddr) -> bool {
    normalize_ip(ip).is_loopback()
}

/// `src/net/endpoint.js` `globalAddress`: true for routable internet addresses
/// (excludes loopback, RFC 1918 private, CGNAT, link-local,
/// ULA `fc00::/7`, and multicast).
pub fn global_ip(ip: IpAddr) -> bool {
    match normalize_ip(ip) {
        IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            !(a == 10
                || a == 127
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..32).contains(&b))
                || (a == 192 && b == 168)
                || a >= 224)
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() || v6.is_loopback() || v6.is_multicast() {
                return false;
            }
            let first = v6.segments()[0];
            // fc00::/7 (ULA) and fe80::/10 (link-local)
            (first & 0xfe00) != 0xfc00 && (first & 0xffc0) != 0xfe80
        }
    }
}

/// `src/net/endpoint.js` `addressPrefix`: one IPv4 address or one IPv6 `/64`
/// prefix (`a:b:c:d::/64`).
pub fn address_prefix(ip: IpAddr) -> String {
    match normalize_ip(ip) {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let segs = v6.segments();
            format!(
                "{:x}:{:x}:{:x}:{:x}::/64",
                segs[0], segs[1], segs[2], segs[3]
            )
        }
    }
}

/// Source key matching `directPath` in `src/net/endpoint.js`.
pub fn direct_source_key(local_port: u16, remote: SocketAddr) -> String {
    let norm = normalize_socket_addr(remote);
    format!("direct|{local_port}|{}|{}", norm.ip(), norm.port())
}

#[derive(Clone, Debug)]
pub struct ClientInfo {
    pub id: [u8; 32],
    pub id_hex: String,
    pub public_key: String,
    pub hello: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct AdmitDecision {
    pub accept: bool,
    pub code: u64,
    pub reason: String,
}

impl AdmitDecision {
    pub fn accept() -> Self {
        Self {
            accept: true,
            code: close::NORMAL as u64,
            reason: String::new(),
        }
    }

    pub fn reject(code: u64, reason: impl Into<String>) -> Self {
        Self {
            accept: false,
            code,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EndpointCounters {
    pub received: u64,
    pub sent: u64,
    pub unknown_session: u64,
    pub stray_replies: u64,
    pub rate_limited: u64,
    pub malformed: u64,
    pub send_errors: u64,
    pub initiations: u64,
}

pub struct ServerSession {
    pub local_session: u32,
    pub remote_session: u32,
    pub client: ClientInfo,
    pub prefix: String,
    pub connection: Connection,
    pub confirm_bytes: Vec<u8>,
    pub draining_until: Option<f64>,
    pub notified_close: bool,
}

impl ServerSession {
    pub fn is_closed(&self) -> bool {
        self.connection.closed.is_some()
    }
}

struct TokenBucket {
    key: String,
    tokens: f64,
    at: f64,
}

struct PendingPunch {
    at: f64,
    target: SocketAddr,
}

/// Callbacks borrowed during `ServerEndpoint::receive`, `poll`, `send`, and
/// `close_session`. Keeping transmit and message delivery on a stack-borrowed
/// trait avoids heap allocations on the steady-state packet path.
pub trait ServerSink {
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr);
    fn on_hello(&mut self) -> serde_json::Value;
    fn on_admit(&mut self, client: &ClientInfo, remote: SocketAddr) -> AdmitDecision;
    fn on_session(&mut self, session_id: u32, client: &ClientInfo, remote: SocketAddr, rtt: f64);
    fn on_message(&mut self, session_id: u32, lane: Lane, text: bool, bytes: &[u8]);
    fn on_path(&mut self, session_id: u32, new_path: SocketAddr, old_path: SocketAddr);
    fn on_close(&mut self, session_id: u32, closed: Closed);
    fn on_stun_response(&mut self, msg: &stun::StunMessage<'_>, from: SocketAddr) {
        let _ = (msg, from);
    }
}

struct ConnSink<'a, S: ServerSink> {
    session_id: u32,
    counters: &'a mut EndpointCounters,
    inner: &'a mut S,
}

impl<S: ServerSink> Sink for ConnSink<'_, S> {
    fn packet(&mut self, bytes: &[u8], path: SocketAddr) {
        self.counters.sent += 1;
        self.inner.send_packet(bytes, path);
    }

    fn message(&mut self, lane: Lane, text: bool, bytes: &[u8]) {
        self.inner.on_message(self.session_id, lane, text, bytes);
    }
}

pub struct ServerEndpoint {
    pub local_port: u16,
    pub max_sessions: usize,
    pub max_sessions_per_prefix: Option<usize>,
    pub handshake_rate: f64,
    pub handshakes: ServerHandshakes,
    pub counters: EndpointCounters,
    sessions: HashMap<u32, ServerSession>,
    session_ids: Vec<u32>,
    buckets: VecDeque<TokenBucket>,
    initiation_window_at: f64,
    initiation_window_count: usize,
    live: usize,
    draining: usize,
    punches: VecDeque<PendingPunch>,
}

impl ServerEndpoint {
    pub fn new(
        identity: Identity,
        retry: RetryTokens,
        local_port: u16,
        max_sessions: usize,
        max_sessions_per_prefix: Option<usize>,
        handshake_rate: f64,
    ) -> Self {
        Self {
            local_port,
            max_sessions,
            max_sessions_per_prefix,
            handshake_rate,
            handshakes: ServerHandshakes::new(identity, retry),
            counters: EndpointCounters::default(),
            sessions: HashMap::with_capacity(max_sessions.min(1024)),
            session_ids: Vec::with_capacity(max_sessions.min(1024)),
            buckets: VecDeque::with_capacity(256),
            initiation_window_at: 0.0,
            initiation_window_count: 0,
            live: 0,
            draining: 0,
            punches: VecDeque::new(),
        }
    }

    pub fn live_sessions(&self) -> usize {
        self.live
    }

    pub fn total_sessions(&self) -> usize {
        self.sessions.len()
    }

    pub fn session(&self, session_id: u32) -> Option<&ServerSession> {
        self.sessions.get(&session_id)
    }

    pub fn session_mut(&mut self, session_id: u32) -> Option<&mut ServerSession> {
        self.sessions.get_mut(&session_id)
    }

    pub fn sessions_iter(&self) -> impl Iterator<Item = &ServerSession> {
        self.sessions
            .values()
            .filter(|s| s.connection.closed.is_none())
    }

    fn allow_handshake(&mut self, addr: SocketAddr, now: f64) -> bool {
        let key = address_prefix(addr.ip());
        let rate = self.handshake_rate;
        let burst = rate * 2.0;
        let mut bucket = if let Some(pos) = self.buckets.iter().position(|b| b.key == key) {
            self.buckets.remove(pos).unwrap()
        } else {
            if self.buckets.len() >= MAX_BUCKETS {
                self.buckets.pop_front();
            }
            TokenBucket {
                key,
                tokens: burst,
                at: now,
            }
        };
        bucket.tokens = burst.min(bucket.tokens + (now - bucket.at).max(0.0) * rate / 1000.0);
        bucket.at = now;
        if bucket.tokens < 1.0 {
            self.buckets.push_back(bucket);
            self.counters.rate_limited += 1;
            return false;
        }
        bucket.tokens -= 1.0;
        self.buckets.push_back(bucket);
        true
    }

    fn under_load(&mut self, now: f64) -> bool {
        if now - self.initiation_window_at > 1000.0 {
            self.initiation_window_at = now;
            self.initiation_window_count = 0;
        }
        self.initiation_window_count += 1;
        self.initiation_window_count > 200 || self.handshakes.pending_count() > 64
    }

    fn crowded(&self, addr: SocketAddr, client_id: &[u8; 32]) -> bool {
        let Some(limit) = self.max_sessions_per_prefix else {
            return false;
        };
        if loopback_ip(addr.ip()) {
            return false;
        }
        let prefix = address_prefix(addr.ip());
        let mut count = 0usize;
        for session in self.sessions.values() {
            if session.connection.closed.is_none()
                && &session.client.id != client_id
                && session.prefix == prefix
            {
                count += 1;
                if count >= limit {
                    return true;
                }
            }
        }
        false
    }

    /// Processes a single incoming UDP datagram. Steady-state `DATA` and
    /// `DATA_PHASE` packets run with zero heap allocations.
    pub fn receive(
        &mut self,
        packet: &[u8],
        from: SocketAddr,
        now: f64,
        sink: &mut impl ServerSink,
    ) {
        if packet.is_empty() || packet.len() > 1500 {
            return;
        }
        let from = normalize_socket_addr(from);
        self.counters.received += 1;
        if stun::is_stun(packet) {
            if let Some(msg) = stun::decode_message(packet) {
                if msg.klass != stun::class::REQUEST {
                    sink.on_stun_response(&msg, from);
                }
            } else {
                self.counters.malformed += 1;
            }
            return;
        }
        let kind = packet[0] as usize;
        if kind == packet::DATA || kind == packet::DATA_PHASE {
            if packet.len() < d::BYTES + lim::TAG + 1 {
                return;
            }
            let session_id =
                u32::from_le_bytes(packet[d::RECEIVER..d::RECEIVER + 4].try_into().unwrap());
            let Some(session) = self.sessions.get_mut(&session_id) else {
                self.counters.unknown_session += 1;
                return;
            };
            let prev_path = session.connection.path;
            let was_closed = session.connection.closed.is_some();
            {
                let mut conn_sink = ConnSink {
                    session_id,
                    counters: &mut self.counters,
                    inner: sink,
                };
                session.connection.receive(packet, from, now, &mut conn_sink);
            }
            let new_path = session.connection.path;
            if new_path != prev_path {
                session.prefix = address_prefix(new_path.ip());
                sink.on_path(session_id, new_path, prev_path);
            }
            if !was_closed {
                if let Some(closed) = session.connection.closed {
                    self.transition_to_draining(session_id, closed, now, sink);
                }
            }
            return;
        }
        if kind == packet::INITIATION {
            if packet.len() < lim::INITIATION_BYTES {
                self.counters.malformed += 1;
                return;
            }
            self.handle_initiation(packet, from, now, sink);
            return;
        }
        if kind == packet::CONFIRM {
            if packet.len() < conf::BYTES + crate::layout::proof::BYTES + lim::TAG {
                self.counters.malformed += 1;
                return;
            }
            self.handle_confirm(packet, from, now, sink);
            return;
        }
        if kind == packet::PUNCH {
            return;
        }
        self.counters.malformed += 1;
    }

    fn handle_initiation(
        &mut self,
        packet: &[u8],
        from: SocketAddr,
        now: f64,
        sink: &mut impl ServerSink,
    ) {
        if !self.allow_handshake(from, now) {
            return;
        }
        self.counters.initiations += 1;
        let require_retry = self.under_load(now);
        let source = direct_source_key(self.local_port, from);
        let prefix = address_prefix(from.ip());
        let mut hello = sink.on_hello();
        if let Some(obj) = hello.as_object_mut() {
            obj.insert("transport".into(), serde_json::json!({ "probing": 1 }));
        }
        let existing_sessions = &self.sessions;
        if let Some(reply) = self.handshakes.initiation(
            packet,
            &source,
            &prefix,
            require_retry,
            &hello,
            now,
            |id| existing_sessions.contains_key(&id),
        ) {
            self.counters.sent += 1;
            sink.send_packet(&reply, from);
        }
    }

    fn handle_confirm(
        &mut self,
        packet: &[u8],
        from: SocketAddr,
        now: f64,
        sink: &mut impl ServerSink,
    ) {
        if !self.allow_handshake(from, now) {
            return;
        }
        let source = direct_source_key(self.local_port, from);
        let Some(completed) = self.handshakes.confirm(packet, &source, now) else {
            let session_id =
                u32::from_le_bytes(packet[conf::SESSION..conf::SESSION + 4].try_into().unwrap());
            if let Some(session) = self.sessions.get_mut(&session_id) {
                if session.connection.closed.is_none() && session.confirm_bytes == packet {
                    session.connection.confirm_handshake();
                    let mut conn_sink = ConnSink {
                        session_id,
                        counters: &mut self.counters,
                        inner: sink,
                    };
                    let _ = session.connection.flush(now, &mut conn_sink);
                }
            }
            return;
        };

        let client = ClientInfo {
            id: completed.client_id,
            id_hex: completed.client_id_hex,
            public_key: completed.client_public_key,
            hello: completed.client_hello,
        };
        let decision = if self.live >= self.max_sessions {
            AdmitDecision::reject(close::REJECTED as u64, "The server is full.")
        } else if self.crowded(from, &client.id) {
            AdmitDecision::reject(
                close::REJECTED as u64,
                "Too many players are connected from your network.",
            )
        } else {
            sink.on_admit(&client, from)
        };

        let session_id = completed.keys.local;
        let remote_session = completed.keys.remote;
        let initial_rtt = completed.rtt.max(1.0);
        let mut connection = Connection::new(completed.keys, from, now, initial_rtt);

        if !decision.accept {
            connection.close(decision.code, decision.reason.as_bytes());
            {
                let mut conn_sink = ConnSink {
                    session_id,
                    counters: &mut self.counters,
                    inner: sink,
                };
                let _ = connection.flush(now, &mut conn_sink);
            }
            if self.draining < MAX_DRAINING {
                self.draining += 1;
                self.session_ids.push(session_id);
                self.sessions.insert(
                    session_id,
                    ServerSession {
                        local_session: session_id,
                        remote_session,
                        client,
                        prefix: address_prefix(from.ip()),
                        connection,
                        confirm_bytes: completed.confirm,
                        draining_until: Some(now + DRAIN_MS),
                        notified_close: true,
                    },
                );
            }
            return;
        }

        connection.confirm_handshake();
        {
            let mut conn_sink = ConnSink {
                session_id,
                counters: &mut self.counters,
                inner: sink,
            };
            let _ = connection.flush(now, &mut conn_sink);
        }
        let session = ServerSession {
            local_session: session_id,
            remote_session,
            client: client.clone(),
            prefix: address_prefix(from.ip()),
            connection,
            confirm_bytes: completed.confirm,
            draining_until: None,
            notified_close: false,
        };
        self.live += 1;
        self.session_ids.push(session_id);
        self.sessions.insert(session_id, session);
        sink.on_session(session_id, &client, from, initial_rtt);
    }

    fn transition_to_draining(
        &mut self,
        session_id: u32,
        closed: Closed,
        now: f64,
        sink: &mut impl ServerSink,
    ) {
        let remove_immediately = if let Some(session) = self.sessions.get_mut(&session_id) {
            if session.notified_close {
                return;
            }
            session.notified_close = true;
            self.live = self.live.saturating_sub(1);
            if self.draining >= MAX_DRAINING {
                true
            } else {
                self.draining += 1;
                session.draining_until = Some(now + DRAIN_MS);
                false
            }
        } else {
            return;
        };
        if remove_immediately {
            self.sessions.remove(&session_id);
            self.session_ids.retain(|&id| id != session_id);
        }
        sink.on_close(session_id, closed);
    }

    /// Queues a message on a session's lane without flushing yet.
    pub fn queue(
        &mut self,
        session_id: u32,
        lane: Lane,
        text: bool,
        bytes: &[u8],
    ) -> Result<bool> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Ok(false);
        };
        session.connection.queue(lane, text, bytes)
    }

    /// Flushes a single session's pending frames immediately.
    pub fn flush_session(
        &mut self,
        session_id: u32,
        now: f64,
        sink: &mut impl ServerSink,
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Ok(());
        };
        let was_closed = session.connection.closed.is_some();
        let res = {
            let mut conn_sink = ConnSink {
                session_id,
                counters: &mut self.counters,
                inner: sink,
            };
            session.connection.flush(now, &mut conn_sink)
        };
        if !was_closed {
            if let Some(closed) = session.connection.closed {
                self.transition_to_draining(session_id, closed, now, sink);
            }
        }
        res
    }

    /// Closes a session with the given close code and reason and flushes the
    /// `CLOSE` frame onto the wire.
    pub fn close_session(
        &mut self,
        session_id: u32,
        code: u64,
        reason: &[u8],
        now: f64,
        sink: &mut impl ServerSink,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        if session.connection.closed.is_some() {
            return;
        }
        session.connection.close(code, reason);
        let mut conn_sink = ConnSink {
            session_id,
            counters: &mut self.counters,
            inner: sink,
        };
        let _ = session.connection.flush(now, &mut conn_sink);
        if let Some(closed) = session.connection.closed {
            self.transition_to_draining(session_id, closed, now, sink);
        }
    }

    /// Schedules hole-punch packets (`PACKET::PUNCH`, 9 bytes) toward up to 8
    /// valid candidate addresses across `PUNCH_SCHEDULE_MS`.
    pub fn punch(&mut self, candidates: &[SocketAddr], now: f64) {
        let mut accepted = 0usize;
        for &cand in candidates {
            if accepted >= 8 {
                break;
            }
            let norm = normalize_socket_addr(cand);
            if !valid_peer_ip(norm.ip()) || norm.port() == 0 {
                continue;
            }
            accepted += 1;
            for &delay in &PUNCH_SCHEDULE_MS {
                self.punches.push_back(PendingPunch {
                    at: now + delay as f64,
                    target: norm,
                });
            }
        }
    }

    /// Advances all session timers, flushes due packets, emits scheduled hole
    /// punches, and reaps expired handshakes and draining sessions.
    pub fn poll(&mut self, now: f64, sink: &mut impl ServerSink) {
        self.handshakes.expire(now);
        let punch_pkt = [packet::PUNCH as u8, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut idx = 0;
        while idx < self.punches.len() {
            if self.punches[idx].at <= now {
                let item = self.punches.remove(idx).unwrap();
                self.counters.sent += 1;
                sink.send_packet(&punch_pkt, item.target);
            } else {
                idx += 1;
            }
        }

        let mut i = 0;
        while i < self.session_ids.len() {
            let session_id = self.session_ids[i];
            let Some(session) = self.sessions.get_mut(&session_id) else {
                self.session_ids.swap_remove(i);
                continue;
            };
            if let Some(until) = session.draining_until {
                if now >= until {
                    self.sessions.remove(&session_id);
                    self.session_ids.swap_remove(i);
                    self.draining = self.draining.saturating_sub(1);
                    continue;
                }
                i += 1;
                continue;
            }
            let was_closed = session.connection.closed.is_some();
            {
                let mut conn_sink = ConnSink {
                    session_id,
                    counters: &mut self.counters,
                    inner: sink,
                };
                let _ = session.connection.poll(now, &mut conn_sink);
            }
            if !was_closed {
                if let Some(closed) = session.connection.closed {
                    self.transition_to_draining(session_id, closed, now, sink);
                }
            }
            i += 1;
        }
    }

    pub fn close_all(&mut self, now: f64, sink: &mut impl ServerSink) {
        let ids = self.session_ids.clone();
        for session_id in ids {
            self.close_session(
                session_id,
                close::SHUTDOWN as u64,
                b"Server shutting down",
                now,
                sink,
            );
        }
    }

    pub fn session_stats(&self, session_id: u32, now: f64) -> Option<(ConnectionStats, f64, f64)> {
        let session = self.sessions.get(&session_id)?;
        Some((
            session.connection.stats,
            session.connection.rtt,
            session.connection.recent_loss(now),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::Endpoint as ClientEndpoint;
    use crate::handshake::{ClientHandshake, Event};
    use std::net::{Ipv4Addr, UdpSocket};

    struct TestServerSink {
        outbox: Vec<(Vec<u8>, SocketAddr)>,
        sessions: Vec<u32>,
        messages: Vec<(u32, Lane, bool, Vec<u8>)>,
        closed: Vec<(u32, u64)>,
        admit: AdmitDecision,
    }

    impl TestServerSink {
        fn new() -> Self {
            Self {
                outbox: Vec::new(),
                sessions: Vec::new(),
                messages: Vec::new(),
                closed: Vec::new(),
                admit: AdmitDecision::accept(),
            }
        }
    }

    impl ServerSink for TestServerSink {
        fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
            self.outbox.push((bytes.to_vec(), to));
        }

        fn on_hello(&mut self) -> serde_json::Value {
            serde_json::json!({
                "protocol": 3,
                "profile": "snapshot-delta-v1",
                "simulation": "bfe-sim-28",
                "content": "test"
            })
        }

        fn on_admit(&mut self, _client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
            self.admit.clone()
        }

        fn on_session(
            &mut self,
            session_id: u32,
            _client: &ClientInfo,
            _remote: SocketAddr,
            _rtt: f64,
        ) {
            self.sessions.push(session_id);
        }

        fn on_message(&mut self, session_id: u32, lane: Lane, text: bool, bytes: &[u8]) {
            self.messages
                .push((session_id, lane, text, bytes.to_vec()));
        }

        fn on_path(&mut self, _session_id: u32, _new_path: SocketAddr, _old_path: SocketAddr) {}

        fn on_close(&mut self, session_id: u32, closed: Closed) {
            self.closed.push((session_id, closed.code));
        }
    }

    struct ClientTestSink {
        outbox: Vec<(Vec<u8>, SocketAddr)>,
        messages: Vec<(Lane, bool, Vec<u8>)>,
    }

    impl Sink for ClientTestSink {
        fn packet(&mut self, bytes: &[u8], path: SocketAddr) {
            self.outbox.push((bytes.to_vec(), path));
        }

        fn message(&mut self, lane: Lane, text: bool, bytes: &[u8]) {
            self.messages.push((lane, text, bytes.to_vec()));
        }
    }

    #[test]
    fn client_handshake_and_connection_round_trip_with_server_endpoint() {
        let server_id = Identity::from_seed([5u8; 32]);
        let client_id = Identity::from_seed([6u8; 32]);
        let retry = RetryTokens::from_secret([7u8; 32], 10_000.0);
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18451);
        let client_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49000);

        let mut server = ServerEndpoint::new(server_id.clone(), retry, 18451, 32, Some(4), 10.0);
        let mut server_sink = TestServerSink::new();

        let mut hs = ClientHandshake::new(
            client_id,
            server_id.id,
            br#"{"protocol":3,"profile":"snapshot-delta-v1","simulation":"bfe-sim-28","content":"test","name":"Tester"}"#,
        )
        .unwrap();
        let mut init_pkt = [0u8; lim::INITIATION_BYTES];
        hs.initiation(&[], &mut init_pkt).unwrap();

        server.receive(&init_pkt, client_addr, 5.0, &mut server_sink);
        assert_eq!(server_sink.outbox.len(), 1);
        let (resp_pkt, _) = server_sink.outbox.remove(0);
        assert_eq!(hs.receive(&resp_pkt), Event::Established);

        let keys = hs.take_keys().unwrap();
        let mut client_conn = Connection::new(keys, server_addr, 10.0, 10.0);
        let mut client_sink = ClientTestSink {
            outbox: Vec::new(),
            messages: Vec::new(),
        };

        server.receive(hs.confirm(), client_addr, 15.0, &mut server_sink);
        assert_eq!(server_sink.sessions.len(), 1);
        let session_id = server_sink.sessions[0];

        // Server sent HANDSHAKE_DONE immediately on confirm
        for (pkt, _) in server_sink.outbox.drain(..) {
            assert!(client_conn.receive(&pkt, server_addr, 16.0, &mut client_sink));
        }
        assert!(client_conn.handshake_confirmed);

        // Server queues welcome on Events lane and state snapshot on State lane
        server
            .queue(session_id, Lane::Events, true, br#"{"type":"welcome"}"#)
            .unwrap();
        server
            .queue(session_id, Lane::State, false, &[0x42, 1, 2, 3])
            .unwrap();
        server
            .flush_session(session_id, 20.0, &mut server_sink)
            .unwrap();

        for (pkt, _) in server_sink.outbox.drain(..) {
            assert!(client_conn.receive(&pkt, server_addr, 22.0, &mut client_sink));
        }
        assert_eq!(client_sink.messages.len(), 2);

        // Client sends input on State lane
        client_conn
            .queue(Lane::State, false, &[9, 8, 7])
            .unwrap();
        client_conn.flush(25.0, &mut client_sink).unwrap();
        for (pkt, _) in client_sink.outbox.drain(..) {
            server.receive(&pkt, client_addr, 30.0, &mut server_sink);
        }
        assert_eq!(server_sink.messages.len(), 1);
        assert_eq!(server_sink.messages[0].3, vec![9, 8, 7]);
    }

    #[test]
    fn live_udp_socket_client_endpoint_joins_server_endpoint() {
        let server_socket =
            UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
        server_socket.set_nonblocking(true).unwrap();
        let server_addr = server_socket.local_addr().unwrap();

        let server_id = Identity::from_seed([11u8; 32]);
        let client_id = Identity::from_seed([12u8; 32]);
        let retry = RetryTokens::from_secret([13u8; 32], 10_000.0);
        let mut server =
            ServerEndpoint::new(server_id.clone(), retry, server_addr.port(), 16, None, 10.0);
        let mut server_sink = TestServerSink::new();

        let mut client = ClientEndpoint::connect(
            &server_addr.to_string(),
            client_id,
            server_id.id,
            br#"{"protocol":3,"profile":"snapshot-delta-v1","simulation":"bfe-sim-28","content":"test","name":"UdpClient"}"#,
        )
        .unwrap();

        let mut buf = [0u8; 1500];
        let mut now = 0.0;
        let mut client_msgs = Vec::new();
        for _ in 0..50 {
            now += 10.0;
            client
                .pump(&mut |lane, text, bytes, _| {
                    client_msgs.push((lane, text, bytes.to_vec()));
                })
                .unwrap();
            while let Ok((n, from)) = server_socket.recv_from(&mut buf) {
                server.receive(&buf[..n], from, now, &mut server_sink);
            }
            server.poll(now, &mut server_sink);
            for (pkt, to) in server_sink.outbox.drain(..) {
                let _ = server_socket.send_to(&pkt, to);
            }
            if client.ready() {
                break;
            }
        }
        assert!(client.ready());
        assert_eq!(server.live_sessions(), 1);
    }
}
