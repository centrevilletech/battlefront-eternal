//! Whole-frame policies exercise their published records, including long
//! pauses and repeated display ticks, without renderer or content fixtures.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! { static TRACK: Cell<bool> = const { Cell::new(false) }; static ALLOCS: Cell<usize> = const { Cell::new(0) }; }
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) { ALLOCS.with(|n| n.set(n.get() + 1)); }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { System.dealloc(p, l); }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) { ALLOCS.with(|a| a.set(a.get() + 1)); }
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;



use bfe_presentation::{lights::Lights, page_layout::{light_command as c,
    light_frame as f, light_header as h, light_kind as kind, light_limits as lim,
    light_output as o, light_handle as token}};
fn command(p: &mut Lights, i: usize, x: f64, radius: f64, flash: bool) {
    let a = &mut p.commands[i * c::STRIDE..(i + 1) * c::STRIDE];
    a.fill(0.0);
    a[c::KIND] = if flash { kind::FLASH } else { kind::ADD } as f64;
    a[c::SERIAL] = (i + 1) as f64;
    a[c::POSITION] = x;
    a[c::RGB..c::RGB + 3].fill(1.0);
    a[c::SCALE] = 1.0;
    a[c::RADIUS] = radius;
    a[c::DURATION] = 1.0;
}
#[test]
fn lights_rank_stably_then_append_fading_flashes() {
    let mut p = Lights::new(3);
    for (i, radius) in [1.0, 3.0, 3.0, 1.0 / 3.0, 4.0, 1.0].iter().enumerate() {
        command(&mut p, i, (i + 1) as f64, *radius, false);
    }
    command(&mut p, 6, 7.0, 1.0, true);
    p.input[f::COMMANDS] = 7.0;
    p.input[f::DT] = 0.5;
    p.frame();
    assert_eq!([p.output[o::POSITION], p.output[o::STRIDE + o::POSITION],
        p.output[2 * o::STRIDE + o::POSITION]], [5.0, 2.0, 3.0]);
    assert_eq!(p.header[h::TIMED], 1.0);
    assert_eq!(p.handles[7 * token::STRIDE + token::LEFT], 0.5);
    p.input[f::COMMANDS] = 0.0;
    p.frame();
    assert_eq!(p.header[h::TIMED], 0.0);
    assert_eq!(p.header[h::COUNT], 0.0);
    assert_eq!(p.output[o::SCALE], 0.0);
}


