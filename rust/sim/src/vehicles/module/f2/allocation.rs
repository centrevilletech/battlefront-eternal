//! Count allocation requests made during Vehicles.update. The counter is read
//! by phase tests; initialization and synchronous commands outside the phase
//! are excluded so a new hull or rider may allocate its persistent storage.
use crate::allocations::fleet;

/// Fixed Vehicles.update counters belong to diagnostics. Recording cannot
/// allocate or borrow Fleet: the allocator can enter while a hull bank grows.
pub(crate) struct Census {
    active: bool, count: u32, group: usize, groups: [u32; 3],
    stage: usize, stages: [u32; 10],
}
impl Census {
    pub(crate) const fn new() -> Self {
        Self { active: false, count: 0, group: 0, groups: [0; 3], stage: 0, stages: [0; 10] }
    }
    pub(crate) fn record(&mut self) {
        if !self.active { return; }
        self.count += 1; self.groups[self.group] += 1; self.stages[self.stage] += 1;
    }
    fn begin(&mut self) {
        self.stage = 0; self.stages = [0; 10]; self.group = CORE;
        self.groups = [0; 3]; self.count = 0; self.active = true;
    }
}
pub enum Stage { Foreign, Latch, Hulls, Fire, Riders, Turrets, Publish, Finish, Polygons, Load }
pub fn stage(stage: Stage) { fleet(|c| c.stage = stage as usize); }
pub const CORE: usize = 0;
pub const ORDNANCE: usize = 1;
pub const LIFECYCLE: usize = 2;
pub struct StageScope(usize);
impl Drop for StageScope { fn drop(&mut self) { fleet(|c| c.stage = self.0); } }
pub fn nested(stage: Stage) -> StageScope {
    StageScope(fleet(|c| std::mem::replace(&mut c.stage, stage as usize)))
}
pub struct Scope(usize);
impl Drop for Scope { fn drop(&mut self) { fleet(|c| c.group = self.0); } }
pub fn enter(group: usize) -> Scope {
    Scope(fleet(|c| std::mem::replace(&mut c.group, group)))
}
// The module allocator owns counting for every native phase. F2 only labels
// its active scheduler/group here, so a coordinated span needs no second allocator.
pub fn begin() { fleet(Census::begin); }
pub fn end() { fleet(|c| c.active = false); }
pub fn f2_allocations() -> u32 { fleet(|c| c.count) }

pub fn f2_core_allocations() -> u32 { fleet(|c| c.groups[CORE]) }

pub fn f2_ordnance_allocations() -> u32 { fleet(|c| c.groups[ORDNANCE]) }

pub fn f2_lifecycle_allocations() -> u32 { fleet(|c| c.groups[LIFECYCLE]) }

/// Allocation census by the Vehicles.update scheduler slot.
pub fn f2_stage_allocations(stage: usize) -> u32 {
    fleet(|c| c.stages.get(stage).copied().unwrap_or(0))
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    #[no_mangle]
    pub extern "C" fn f2_allocations() -> u32 { super::f2_allocations() }
    #[no_mangle]
    pub extern "C" fn f2_core_allocations() -> u32 { super::f2_core_allocations() }
    #[no_mangle]
    pub extern "C" fn f2_ordnance_allocations() -> u32 { super::f2_ordnance_allocations() }
    #[no_mangle]
    pub extern "C" fn f2_lifecycle_allocations() -> u32 { super::f2_lifecycle_allocations() }
    #[no_mangle]
    pub extern "C" fn f2_stage_allocations(stage: usize) -> u32 { super::f2_stage_allocations(stage) }
}
