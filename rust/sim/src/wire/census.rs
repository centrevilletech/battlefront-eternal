//! Allocation measurements for snapshot-host.publish, including codecs and
//! generated exchanges. The caller brackets a synchronous publication only.
use crate::{allocations::Counts, gen::WireCensus};
// Reading diagnostics uses fixed storage and cannot create allocator traffic.
pub struct Census {before:Option<Counts>,result:[f64;WireCensus::WORDS]}
impl Census {
    pub const fn new()->Self {Self {before:None,result:[0.;WireCensus::WORDS]}}
    pub fn begin(&mut self,counts:Counts) {self.before=Some(counts);}
    /// The last measurement, for a native host reading by name.
    pub fn result(&self)->&[f64] {&self.result}
    pub fn end(&mut self,now:Counts)->*const f64 {
        let counts=now.since(self.before.take().unwrap_or(now));
        self.result[WireCensus::ALLOCATIONS]=counts.calls as f64;
        self.result[WireCensus::REALLOCATIONS]=counts.reallocs as f64;
        self.result[WireCensus::ALLOCATION_BYTES]=counts.bytes as f64;self.result.as_ptr()
    }
}
pub(crate) fn wire_census_begin_in(w: &mut crate::world::owner::World, counts: Counts) {super::owner::with_in(w, |owner|owner.census.begin(counts));}
pub(crate) fn wire_census_end_in(w: &mut crate::world::owner::World, counts: Counts) -> Option<&[f64]> {super::owner::with_in(w, |owner| { owner.census.end(counts); }); Some(&w.wire.as_deref().expect("admitted wire owner").census.result)}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn wire_census_begin() { wire_census_begin_in(crate::abi::facade::world(), crate::allocations::snapshot()) }
    #[no_mangle]
    pub extern "C" fn wire_census_end() ->*const f64 { wire_census_end_in(crate::abi::facade::world(), crate::allocations::snapshot()).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
