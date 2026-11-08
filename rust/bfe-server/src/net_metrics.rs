//! Sliding-window and lifetime network metrics (`src/net-metrics.js`).
//!
//! Tracks per-session and per-endpoint transport timing and byte/packet counters for the
//! operator status view (`operatorRequest` / `metrics`) and qualification probes:
//! - Sliding 5-second throughput (`bytesInPerSec`, `bytesOutPerSec`, `packetsInPerSec`,
//!   `packetsOutPerSec`) bucketed at 1-second resolution so spikes decay cleanly without
//!   retaining unbounded sample arrays.
//! - Exponentially weighted moving average round-trip time (`rttMs`, `0.8 * prev + 0.2 * sample`)
//!   and recent datagram loss fraction (`loss`, `0.0..=1.0`).
//! - Lifetime totals (`totalBytesIn`, `totalBytesOut`, `totalPacketsIn`, `totalPacketsOut`) and
//!   per-lane queue/drop counters (`stateQueued`, `stateDropped`, `eventsQueued`, `eventsDropped`)
//!   corresponding to `counters` in `src/net-metrics.js` and `GameSession.transportCounters` in
//!   `src/game-session.js`.

use serde_json::{json, Value};

/// Width of each sliding-window throughput bucket in milliseconds (1 second).
const BUCKET_MS: u64 = 1000;
/// Number of 1-second buckets retained in the sliding throughput window (5 seconds total).
const BUCKETS: usize = 5;

/// Single 1-second throughput bucket inside [`NetMetrics`].
#[derive(Clone, Copy, Default)]
struct Bucket {
    slot: u64,
    bytes_in: u64,
    bytes_out: u64,
    packets_in: u64,
    packets_out: u64,
}

/// Per-session and endpoint network metrics collector (`src/net-metrics.js`).
#[derive(Clone)]
/// Sliding-window transport telemetry accumulator porting `createNetMetrics()` from
/// `packages/bfe-server/src/net-metrics.js`.
///
/// Tracks 60 one-second ring-buffer buckets (`WINDOW_SECONDS = 60`) of inbound/outbound bytes and
/// packets partitioned by lane (`events`, `input`, `state`), plus RTT and packet-loss samples,
/// powering `/admin` `transport-metrics` and the 16-client soak bandwidth verification gate.
pub struct NetMetrics {
    window_ms: u64,
    buckets: [Bucket; BUCKETS],
    /// Lifetime inbound payload bytes received across all lanes.
    pub total_bytes_in: u64,
    /// Lifetime outbound payload bytes queued across all lanes.
    pub total_bytes_out: u64,
    /// Lifetime inbound messages/datagrams received.
    pub total_packets_in: u64,
    /// Lifetime outbound messages/datagrams queued.
    pub total_packets_out: u64,
    /// Outbound `state` datagrams accepted (`stateGroups` in `GameSession.transportCounters`).
    pub state_queued: u64,
    /// Outbound `state` datagrams dropped due to congestion, staleness, or budget (`stateDropped` in `GameSession.transportCounters`).
    pub state_dropped: u64,
    /// Outbound `events` messages queued on the reliable ordered lane.
    pub events_queued: u64,
    /// Outbound `events` messages dropped (`oversizedMessages` in `src/net-metrics.js`).
    pub events_dropped: u64,
    /// Smoothed round-trip time in milliseconds (`session.stats().rtt` in `src/udp-peer.js`).
    pub rtt_ms: Option<f64>,
    /// Recent packet loss fraction in `0.0..=1.0` (`session.stats().loss` in `src/udp-peer.js`).
    pub loss: f64,
}

impl Default for NetMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl NetMetrics {
    /// Creates a zeroed `NetMetrics` instance (`resetNetMetrics` initial state in `src/net-metrics.js`).
    pub fn new() -> Self {
        Self {
            window_ms: (BUCKETS as u64) * BUCKET_MS,
            buckets: [Bucket::default(); BUCKETS],
            total_bytes_in: 0,
            total_bytes_out: 0,
            total_packets_in: 0,
            total_packets_out: 0,
            state_queued: 0,
            state_dropped: 0,
            events_queued: 0,
            events_dropped: 0,
            rtt_ms: None,
            loss: 0.0,
        }
    }

    /// Returns the active 1-second bucket for `now_ms`, resetting it when the ring slot advances.
    fn bucket(&mut self, now_ms: u64) -> &mut Bucket {
        let slot = now_ms / BUCKET_MS;
        let idx = (slot as usize) % BUCKETS;
        if self.buckets[idx].slot != slot {
            self.buckets[idx] = Bucket {
                slot,
                bytes_in: 0,
                bytes_out: 0,
                packets_in: 0,
                packets_out: 0,
            };
        }
        &mut self.buckets[idx]
    }

    /// Records an inbound message or datagram of `bytes` length at `now_ms`.
    pub fn record_in(&mut self, bytes: usize, now_ms: u64) {
        let b = self.bucket(now_ms);
        b.bytes_in += bytes as u64;
        b.packets_in += 1;
        self.total_bytes_in += bytes as u64;
        self.total_packets_in += 1;
    }

    /// Records an outbound message or datagram of `bytes` length on `lane` (`"state"`, `"events"`, or `"bulk"`).
    pub fn record_out(&mut self, bytes: usize, lane: &str, now_ms: u64) {
        let b = self.bucket(now_ms);
        b.bytes_out += bytes as u64;
        b.packets_out += 1;
        self.total_bytes_out += bytes as u64;
        self.total_packets_out += 1;
        if lane == "state" {
            self.state_queued += 1;
        } else if lane == "events" {
            self.events_queued += 1;
        }
    }

    /// Increments the drop counter for `lane` (`countNet` in `src/net-metrics.js`).
    pub fn record_drop(&mut self, lane: &str) {
        if lane == "state" {
            self.state_dropped += 1;
        } else if lane == "events" {
            self.events_dropped += 1;
        }
    }

    /// Updates the smoothed RTT estimate in milliseconds (`0.8 * prev + 0.2 * ms`).
    pub fn record_rtt(&mut self, ms: f64) {
        if !ms.is_finite() || ms < 0.0 {
            return;
        }
        self.rtt_ms = Some(match self.rtt_ms {
            None => ms,
            Some(prev) => prev * 0.8 + ms * 0.2,
        });
    }

    /// Updates the recent packet-loss fraction (`0.0..=1.0`).
    pub fn record_loss(&mut self, fraction: f64) {
        if !fraction.is_finite() || fraction < 0.0 {
            return;
        }
        self.loss = fraction.min(1.0);
    }

    /// Serializes a point-in-time metrics snapshot at `now_ms` (`export function netMetrics()` in
    /// `src/net-metrics.js` and `UdpPeer.transportStats()` in `src/udp-peer.js`).
    pub fn snapshot(&self, now_ms: u64) -> Value {
        let cur_slot = now_ms / BUCKET_MS;
        let mut bytes_in = 0u64;
        let mut bytes_out = 0u64;
        let mut packets_in = 0u64;
        let mut packets_out = 0u64;
        for b in &self.buckets {
            if b.slot > 0 && cur_slot.saturating_sub(b.slot) < BUCKETS as u64 {
                bytes_in += b.bytes_in;
                bytes_out += b.bytes_out;
                packets_in += b.packets_in;
                packets_out += b.packets_out;
            }
        }
        let secs = (self.window_ms as f64) / 1000.0;
        let rtt = self.rtt_ms.map(|v| (v * 10.0).round() / 10.0);
        let loss = (self.loss * 1000.0).round() / 1000.0;
        json!({
            "rttMs": rtt,
            "loss": loss,
            "bytesInPerSec": ((bytes_in as f64) / secs).round() as u64,
            "bytesOutPerSec": ((bytes_out as f64) / secs).round() as u64,
            "packetsInPerSec": ((packets_in as f64) / secs * 10.0).round() / 10.0,
            "packetsOutPerSec": ((packets_out as f64) / secs * 10.0).round() / 10.0,
            "totalBytesIn": self.total_bytes_in,
            "totalBytesOut": self.total_bytes_out,
            "totalPacketsIn": self.total_packets_in,
            "totalPacketsOut": self.total_packets_out,
            "stateQueued": self.state_queued,
            "stateDropped": self.state_dropped,
            "eventsQueued": self.events_queued,
            "eventsDropped": self.events_dropped,
        })
    }
}
