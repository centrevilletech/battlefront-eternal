//! Allocation measurements for snapshot-host.publish, including codecs and
//! generated exchanges. The caller brackets a synchronous publication only.
use crate::gen::WireCensus;
// Reading diagnostics uses fixed storage and cannot create allocator traffic.
pub struct Census {result:[f64;WireCensus::WORDS]}
impl Census {
    pub const fn new()->Self {Self {result:[0.;WireCensus::WORDS]}}
    /// The last measurement, for a native host reading by name.
    pub fn result(&self)->&[f64] {&self.result}
}

