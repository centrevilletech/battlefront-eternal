//! Allocation measurements for snapshot-host.publish, including codecs and
//! generated exchanges. The caller brackets a synchronous publication only.
use crate::{allocations::{self, Counts}, gen::WireCensus};
use std::ptr::addr_of_mut;

// Use fixed storage so reading the counters cannot add allocator requests.
// Each module is battle-local and single-threaded, like the wire host itself.
static mut BEFORE: Option<Counts> = None;
static mut RESULT: [f64; WireCensus::WORDS] = [0.; WireCensus::WORDS];

#[no_mangle]
pub extern "C" fn wire_census_begin() {
    unsafe { *addr_of_mut!(BEFORE) = Some(allocations::snapshot()); }
}

#[no_mangle]
pub extern "C" fn wire_census_end() -> *const f64 {
    let now = allocations::snapshot();
    unsafe {
        let before = (*addr_of_mut!(BEFORE)).take().unwrap_or(now);
        let counts = now.since(before);
        let row = &mut *addr_of_mut!(RESULT);
        row[WireCensus::ALLOCATIONS] = counts.calls as f64;
        row[WireCensus::REALLOCATIONS] = counts.reallocs as f64;
        row[WireCensus::ALLOCATION_BYTES] = counts.bytes as f64;
        row.as_ptr()
    }
}
