//! A battle owns its publication banks and the protocol resources they name.
//! Facade numbers are checked local identities, never sibling addresses.
//! Empty banks allocate nothing: page codecs and unused server roles stay cold.
//! Pointer-shaped facade arguments preserve the existing WASM ABI only. Their
//! numeric values name these slots and must never be dereferenced as addresses.
use crate::store::handles::Pool;
pub struct Owner {
    pub runtimes:Pool<super::Runtime>,pub hosts:Pool<super::host::Host>,
    pub staggers:Pool<super::stagger::Stagger>,pub shots:Pool<super::shots::Counters>,
    pub edges:Pool<super::edges::Host>,pub combat:Pool<super::combat::Capture>,
    pub budgets:Pool<super::host::EntityBudget>,pub semantic:Pool<super::semantic::Semantic>,
    pub presentation:Pool<super::presentation::Presentation>,pub cosmetics:Pool<super::cosmetics::Session>,
    pub attached_shots:Vec<usize>,
    #[cfg(target_arch="wasm32")]
    pub sources:Pool<super::store_source::Source>,
    #[cfg(target_arch="wasm32")]
    pub publications:Pool<super::publish_module::Session>,
    #[cfg(target_arch="wasm32")]
    pub census:super::census::Census,
}
impl Default for Owner {fn default()->Self {Self::new()}}
impl Owner {
    pub const fn new()->Self {Self {runtimes:Pool::new(1),hosts:Pool::new(2),staggers:Pool::new(3),shots:Pool::new(4),
        edges:Pool::new(5),combat:Pool::new(6),budgets:Pool::new(7),semantic:Pool::new(8),presentation:Pool::new(9),
        cosmetics:Pool::new(10),attached_shots:Vec::new(),
        #[cfg(target_arch="wasm32")] sources:Pool::new(11),
        #[cfg(target_arch="wasm32")] publications:Pool::new(12),
        #[cfg(target_arch="wasm32")] census:super::census::Census::new()}}
    /// Canonical fired notices update only this battle's admitted counters.
    /// Capture itself never dispatches a gameplay or presentation callback.
    pub fn observe_shot(&mut self,id:f64,generation:f64,live:f64) {
        for &key in &self.attached_shots {if let Some(counter)=self.shots.get_mut(key) {counter.observe(id,generation,live);}}
    }
}

/// The instance facade detaches the existing box before canonical source
/// readers borrow sibling simulation fields. Publication cannot call JS or
/// recurse into the bank; every inner operation receives explicit borrows.
pub(crate) fn with<R>(run:impl FnOnce(&mut Owner)->R)->R {
    let mut owner=crate::abi::world().wire.take().expect("wire operation is not reentrant");
    let result=run(&mut owner);crate::abi::world().wire=Some(owner);result
}
