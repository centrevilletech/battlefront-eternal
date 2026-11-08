//! Dual-stack connection attempt timeout configuration for slow or high-latency links.
//!
//! Mirrors `electron/slow-links.js` (`CONNECT_ATTEMPT`, `allowSlowLinks`,
//! `SLOW_LINKS_FLAG`).

use std::time::Duration;

/// Per-address-family connection attempt timeout in milliseconds (`2500` ms).
///
/// Standard Happy Eyeballs implementations give each address family (IPv6, IPv4)
/// 250 ms before falling back. Over a congested or satellite link with >500 ms
/// round-trip times, a 250 ms budget drops the IPv4 attempt before it can
/// answer. Raising the per-family attempt timeout to 2500 ms allows high-latency
/// dual-stack connections to complete while still failing fast when no route exists.
///
/// Mirrors `CONNECT_ATTEMPT` in `electron/slow-links.js`.
pub const CONNECT_ATTEMPT: u64 = 2500;

/// Command-line flag passed to child processes (such as a hosted server) so
/// their dual-stack connection attempts use [`CONNECT_ATTEMPT`] milliseconds.
///
/// Mirrors `SLOW_LINKS_FLAG` in `electron/slow-links.js`.
pub const SLOW_LINKS_FLAG: &str = "--network-family-autoselection-attempt-timeout=2500";

/// Returns the [`Duration`] (`2500` ms) configured by `allowSlowLinks` for
/// dual-stack Happy Eyeballs connection attempts.
///
/// Mirrors `allowSlowLinks` in `electron/slow-links.js`.
pub const fn allow_slow_links() -> Duration {
    Duration::from_millis(CONNECT_ATTEMPT)
}
