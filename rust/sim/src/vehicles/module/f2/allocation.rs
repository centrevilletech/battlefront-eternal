//! Count allocation requests made during Vehicles.update. The counter is read
//! by phase tests; initialization and synchronous commands outside the phase
//! are excluded so a new hull or rider may allocate its persistent storage.
use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr::addr_of_mut;

struct Counted;
static mut ACTIVE: bool = false;
static mut COUNT: u32 = 0;
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if unsafe { *addr_of_mut!(ACTIVE) } { unsafe { *addr_of_mut!(COUNT) += 1; } }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if unsafe { *addr_of_mut!(ACTIVE) } { unsafe { *addr_of_mut!(COUNT) += 1; } }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout); } }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if unsafe { *addr_of_mut!(ACTIVE) } { unsafe { *addr_of_mut!(COUNT) += 1; } }
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
pub fn begin() { unsafe { *addr_of_mut!(COUNT) = 0; *addr_of_mut!(ACTIVE) = true; } }
pub fn end() { unsafe { *addr_of_mut!(ACTIVE) = false; } }
#[no_mangle]
pub extern "C" fn f2_allocations() -> u32 { unsafe { *addr_of_mut!(COUNT) } }
