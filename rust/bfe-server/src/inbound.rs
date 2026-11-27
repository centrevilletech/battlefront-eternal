//! The inbound limit of a server hosting a 2005 battle: what one source
//! address may send it, checked before anything of a datagram is decoded.
//!
//! A seated pilot sends a datagram every tick, at most about 21.5 kB a second
//! on a lossy link (the measured worst second); a soldier on foot sends less.
//! Each source gets 32,000 bytes a second with an 8,000-byte burst and 120
//! datagrams a second (twice the 60 Hz send) with a burst of 30. A datagram
//! over either is dropped undecoded and counted; a source that was over in five
//! seconds in a row is closed. A 2004 battle's server keeps the released path:
//! the limit applies only while a 2005 match is hosted.
use std::collections::HashMap;
use std::net::SocketAddr;

/// Sustained bytes a second one source may send.
pub const RATE: f64 = 32_000.;
/// Bytes one source may send at once above the sustained rate.
pub const BURST: f64 = 8_000.;
/// Sustained datagrams a second one source may send.
pub const DATAGRAMS: f64 = 120.;
/// Datagrams one source may send at once above the sustained rate.
pub const DATAGRAM_BURST: f64 = 30.;
/// Seconds in a row with a dropped datagram after which the source is closed.
pub const OVER_SECONDS: u32 = 5;
/// The most sources the limit remembers; past it the quietest are forgotten.
pub const MAX_SOURCES: usize = 4096;

#[derive(Clone, Copy, Debug)]
struct Source {
    bytes: f64,
    datagrams: f64,
    at: f64,
    /// The start of the second being counted, and whether a datagram of it was dropped.
    second: f64,
    over: bool,
    /// Whole seconds in a row with a dropped datagram.
    over_seconds: u32,
}

/// What becomes of one datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Within the limit: decode it.
    Take,
    /// Over the limit: drop it undecoded.
    Drop,
    /// Over the limit for [`OVER_SECONDS`] seconds in a row: drop it and close the source.
    Close,
}

/// Every source's allowance, and what was dropped.
#[derive(Default)]
pub struct Inbound {
    sources: HashMap<SocketAddr, Source>,
    /// Datagrams dropped undecoded since the server started.
    pub dropped: u32,
    /// Their bytes.
    pub dropped_bytes: u32,
}

impl Inbound {
    /// The verdict on a datagram of `len` bytes from `from` at `now_ms`.
    pub fn check(&mut self, from: SocketAddr, len: usize, now_ms: f64) -> Verdict {
        if !self.sources.contains_key(&from) && self.sources.len() >= MAX_SOURCES {
            // Forget sources silent for a second; a flood of new addresses
            // cannot grow the table.
            self.sources.retain(|_, s| now_ms - s.at < 1000.);
            if self.sources.len() >= MAX_SOURCES {
                self.sources.clear();
            }
        }
        let source = self.sources.entry(from).or_insert(Source {
            bytes: BURST, datagrams: DATAGRAM_BURST, at: now_ms, second: now_ms, over: false, over_seconds: 0,
        });
        let elapsed = (now_ms - source.at).max(0.) / 1000.;
        source.at = source.at.max(now_ms);
        source.bytes = (source.bytes + elapsed * RATE).min(BURST);
        source.datagrams = (source.datagrams + elapsed * DATAGRAMS).min(DATAGRAM_BURST);
        // Close the seconds that have passed: the next second after one with a
        // drop counts on, any second without one (silent seconds too) resets.
        let passed = ((now_ms - source.second) / 1000.).floor();
        if passed >= 1. {
            source.over_seconds = if source.over && passed < 2. { source.over_seconds.saturating_add(1) } else { 0 };
            source.over = false;
            source.second += passed * 1000.;
        }
        let size = len as f64;
        if source.bytes >= size && source.datagrams >= 1. {
            source.bytes -= size;
            source.datagrams -= 1.;
            return Verdict::Take;
        }
        source.over = true;
        self.dropped = self.dropped.saturating_add(1);
        self.dropped_bytes = self.dropped_bytes.saturating_add(u32::try_from(len).unwrap_or(u32::MAX));
        if source.over_seconds + 1 >= OVER_SECONDS {
            Verdict::Close
        } else {
            Verdict::Drop
        }
    }

    /// Forgets a source once its session is closed.
    pub fn forget(&mut self, from: SocketAddr) {
        self.sources.remove(&from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> SocketAddr {
        SocketAddr::from(([10, 0, 0, n], 40_000))
    }

    // A pilot at the measured worst rate (60 datagrams a second of about 360
    // bytes) is never dropped, on its own or beside a hostile source.
    #[test]
    fn a_seated_pilot_at_the_worst_measured_rate_is_never_dropped() {
        let mut inbound = Inbound::default();
        for tick in 0..60 * 30 {
            let now = f64::from(tick) * 1000. / 60.;
            assert_eq!(inbound.check(addr(1), 360, now), Verdict::Take, "tick {tick}");
            for k in 0..17 {
                let _ = inbound.check(addr(2), 1200, now + f64::from(k) * 0.9);
            }
        }
        assert!(inbound.dropped > 0);
    }

    // A hostile source sending 1,200-byte datagrams a thousand times a second
    // gets no more than the limit through, and is closed after five seconds
    // over it in a row.
    #[test]
    fn a_flood_gets_the_limit_through_and_is_closed_after_five_seconds() {
        let mut inbound = Inbound::default();
        let (mut taken, mut taken_bytes, mut closed_at) = (0u32, 0u32, None);
        for n in 0..6_000u32 {
            let now = f64::from(n);
            match inbound.check(addr(3), 1200, now) {
                Verdict::Take => { taken += 1; taken_bytes += 1200; }
                Verdict::Drop => {}
                Verdict::Close => { closed_at.get_or_insert(now); }
            }
        }
        // Six seconds: the burst and six seconds of rate, whichever binds.
        assert!(f64::from(taken_bytes) <= BURST + 6. * RATE, "{taken_bytes}");
        assert!(f64::from(taken) <= DATAGRAM_BURST + 6. * DATAGRAMS, "{taken}");
        let closed_at = closed_at.expect("closed");
        assert!((4000. ..5001.).contains(&closed_at), "closed at {closed_at} ms");
    }

    // A burst that stops is forgiven: a quiet second resets the count.
    #[test]
    fn a_short_burst_is_dropped_but_not_closed() {
        let mut inbound = Inbound::default();
        for round in 0..10u32 {
            let start = f64::from(round) * 2000.;
            let verdicts: Vec<_> = (0..100).map(|k| inbound.check(addr(4), 1200, start + f64::from(k))).collect();
            assert!(verdicts.contains(&Verdict::Drop) && !verdicts.contains(&Verdict::Close), "round {round}");
        }
    }
}
