//! A count of every entry into the goal bots on this thread.
//!
//! Every public entry of the goal bots (installation, each tick's pass,
//! retirement, the button reader) adds one. A battle that never reaches the
//! goal bots leaves the count unchanged, which tests and measuring harnesses
//! read with [`entries`]. [`scans`] counts the enemies the bots looked at
//! while choosing whom to hunt, the only search over the roster they make,
//! and [`resolves`] the lookups of the goals' arguments.
//!
//! The counts are `u32` and wrap: a reader takes a difference with `wrapping_sub`, never an
//! absolute value.

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(test)]
use std::time::Duration;

static ALL_ENTRIES: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static ENTRIES: Cell<u32> = const { Cell::new(0) };
    static SCANS: Cell<u32> = const { Cell::new(0) };
    static RESOLVES: Cell<u32> = const { Cell::new(0) };
    #[cfg(test)]
    static PASS_TIME: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// Time spent in the goal bots' passes on this thread.
#[cfg(test)]
pub(crate) fn pass_time() -> Duration { PASS_TIME.with(Cell::get) }
#[cfg(test)]
pub(crate) fn add_pass_time(spent: Duration) { PASS_TIME.with(|n| n.set(n.get() + spent)); }

pub(crate) fn note() {
    ENTRIES.with(|n| n.set(n.get().wrapping_add(1)));
    ALL_ENTRIES.fetch_add(1, Ordering::Relaxed);
}
pub(crate) fn scan() { SCANS.with(|n| n.set(n.get().wrapping_add(1))); }
pub(crate) fn resolve() { RESOLVES.with(|n| n.set(n.get().wrapping_add(1))); }

/// Entries into the goal bots on this thread since it started.
pub fn entries() -> u32 { ENTRIES.with(Cell::get) }
/// Entries into the goal bots on every thread of the process.
pub fn process_entries() -> u32 { ALL_ENTRIES.load(Ordering::Relaxed) }
/// Enemies the goal bots looked at while choosing a target, on this thread.
pub fn scans() -> u32 { SCANS.with(Cell::get) }
/// Lookups of the goals' arguments, on this thread.
pub fn resolves() -> u32 { RESOLVES.with(Cell::get) }

#[cfg(test)]
thread_local! { static BUILDINGS: Cell<u32> = const { Cell::new(0) }; }
#[cfg(test)]
pub(crate) fn building_lookup() { BUILDINGS.with(|n| n.set(n.get().wrapping_add(1))); }
#[cfg(test)]
pub(crate) fn building_lookups() -> u32 { BUILDINGS.with(Cell::get) }
