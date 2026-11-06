//! Count allocation requests made during Vehicles.update. The counter is read
//! by phase tests; initialization and synchronous commands outside the phase
//! are excluded so a new hull or rider may allocate its persistent storage.
use std::ptr::addr_of_mut;

static mut ACTIVE: bool = false;
static mut COUNT: u32 = 0;
static mut GROUP: usize = 0;
static mut GROUPS: [u32; 3] = [0; 3];
static mut STAGE: usize = 0;
static mut STAGES: [u32; 10] = [0; 10];
pub enum Stage { Foreign, Latch, Hulls, Fire, Riders, Turrets, Publish, Finish, Polygons, Load }
pub fn stage(stage: Stage) { unsafe { *addr_of_mut!(STAGE) = stage as usize; } }
pub const CORE: usize = 0;
pub const ORDNANCE: usize = 1;
pub const LIFECYCLE: usize = 2;
pub struct StageScope(usize);
impl Drop for StageScope { fn drop(&mut self) { unsafe { *addr_of_mut!(STAGE) = self.0; } } }
pub fn nested(stage: Stage) -> StageScope { let previous = unsafe { *addr_of_mut!(STAGE) }; self::stage(stage); StageScope(previous) }
pub struct Scope(usize);
impl Drop for Scope { fn drop(&mut self) { unsafe { *addr_of_mut!(GROUP) = self.0; } } }
pub fn enter(group: usize) -> Scope {
    let previous = unsafe { *addr_of_mut!(GROUP) }; unsafe { *addr_of_mut!(GROUP) = group; } Scope(previous)
}
// The module allocator owns counting for every native phase. F2 only labels
// its active scheduler/group here, so a coordinated span needs no second allocator.
pub(crate) fn count() {
    if unsafe { *addr_of_mut!(ACTIVE) } { unsafe { *addr_of_mut!(COUNT) += 1; (*addr_of_mut!(GROUPS))[*addr_of_mut!(GROUP)] += 1; (*addr_of_mut!(STAGES))[*addr_of_mut!(STAGE)] += 1; } }
}
pub fn begin() { unsafe { *addr_of_mut!(STAGE) = 0; *addr_of_mut!(STAGES) = [0; 10]; *addr_of_mut!(GROUP) = CORE; *addr_of_mut!(GROUPS) = [0; 3]; *addr_of_mut!(COUNT) = 0; *addr_of_mut!(ACTIVE) = true; } }
pub fn end() { unsafe { *addr_of_mut!(ACTIVE) = false; } }
#[no_mangle]
pub extern "C" fn f2_allocations() -> u32 { unsafe { *addr_of_mut!(COUNT) } }

#[no_mangle]
pub extern "C" fn f2_core_allocations() -> u32 { unsafe { (*addr_of_mut!(GROUPS))[CORE] } }
#[no_mangle]
pub extern "C" fn f2_ordnance_allocations() -> u32 { unsafe { (*addr_of_mut!(GROUPS))[ORDNANCE] } }
#[no_mangle]
pub extern "C" fn f2_lifecycle_allocations() -> u32 { unsafe { (*addr_of_mut!(GROUPS))[LIFECYCLE] } }

/// Allocation census by the Vehicles.update scheduler slot.
#[no_mangle]
pub extern "C" fn f2_stage_allocations(stage: usize) -> u32 { unsafe { (*addr_of_mut!(STAGES)).get(stage).copied().unwrap_or(0) } }
