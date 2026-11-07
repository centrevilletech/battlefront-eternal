//! Allocation measurements for snapshot-host.publish, including codecs and
//! generated exchanges. The caller brackets a synchronous publication only.
use crate::{allocations::{self, Counts}, gen::WireCensus};
// Reading diagnostics uses fixed storage and cannot create allocator traffic.
pub struct Census {before:Option<Counts>,result:[f64;WireCensus::WORDS]}
impl Census {
    pub const fn new()->Self {Self {before:None,result:[0.;WireCensus::WORDS]}}
    pub fn begin(&mut self,counts:Counts) {self.before=Some(counts);}
    pub fn end(&mut self,now:Counts)->*const f64 {
        let counts=now.since(self.before.take().unwrap_or(now));
        self.result[WireCensus::ALLOCATIONS]=counts.calls as f64;
        self.result[WireCensus::REALLOCATIONS]=counts.reallocs as f64;
        self.result[WireCensus::ALLOCATION_BYTES]=counts.bytes as f64;self.result.as_ptr()
    }
}
#[no_mangle]
pub extern "C" fn wire_census_begin() {super::owner::with(|owner|owner.census.begin(allocations::snapshot()));}
#[no_mangle]
pub extern "C" fn wire_census_end()->*const f64 {super::owner::with(|owner|owner.census.end(allocations::snapshot()))}
