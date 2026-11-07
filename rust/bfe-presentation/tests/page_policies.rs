//! Whole-frame policies exercise their published records, including long
//! pauses and repeated display ticks, without renderer or content fixtures.
use bfe_presentation::{leaving_warning::Warning, page_layout::warning as w};
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

#[test]
fn warning_enters_repeats_leaves_and_reenters() {
    let mut p = Warning::default();
    p.row[w::TICK] = 100.0;
    p.row[w::OUTSIDE] = 1.0;
    p.frame();
    assert_eq!(p.row[w::ALPHA], 254.0 / 255.0);
    assert_eq!(p.row[w::STARTED], 1.0);
    let rate = p.row[w::RATE];
    p.frame();
    assert_eq!(p.row[w::RATE], rate);
    assert_eq!(p.row[w::STARTED], 0.0);
    p.row[w::TICK] += 1.0;
    p.frame();
    assert_eq!(p.row[w::ALPHA], 236.0 / 255.0);
    p.row[w::OUTSIDE] = 0.0;
    p.frame();
    assert_eq!(p.row[w::ALPHA], 0.0);
    p.row[w::OUTSIDE] = 1.0;
    p.frame();
    assert_eq!(p.row[w::ALPHA], 254.0 / 255.0);
    assert_eq!(p.row[w::STARTED], 1.0);
}

#[test]
fn warning_pause_is_bounded_and_frames_allocate_nothing() {
    let mut paused = Warning::default();
    paused.row[w::OUTSIDE] = 1.0;
    paused.frame();
    let mut sixty = Warning { row: paused.row };
    sixty.row[w::TICK] = 60.0;
    sixty.frame();
    paused.row[w::TICK] = 1e20;
    paused.frame();
    assert_eq!(paused.row[w::VALUE], sixty.row[w::VALUE]);
    assert_eq!(paused.row[w::RATE], sixty.row[w::RATE]);
    TRACK.with(|t| t.set(true));
    for tick in 0..100_000 {
        paused.row[w::TICK] = tick as f64;
        paused.row[w::OUTSIDE] = (tick % 100 != 0) as u8 as f64;
        paused.frame();
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert!(paused.row[w::ALPHA].is_finite());
}

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
#[test]
fn lights_bound_paused_bursts_and_allocate_nothing() {
    let mut p = Lights::new(usize::MAX);
    for i in 0..lim::COMMANDS { command(&mut p, i, i as f64, (i % 9 + 1) as f64, true); }
    p.input[f::COMMANDS] = f64::INFINITY;
    p.input[f::START] = f64::INFINITY;
    p.input[f::DT] = 0.0;
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|t| t.set(true));
    p.frame();
    command(&mut p, 0, -5.0, 1000.0, true);
    p.commands[c::SERIAL] = (lim::COMMANDS + 1) as f64;
    p.input[f::COMMANDS] = 1.0; p.input[f::START] = 0.0;
    p.frame();
    // One additional flash forces a nonzero ring head. The strong new
    // flash leads; the oldest surviving equal-weight maximum stays next.
    assert_eq!(p.output[o::POSITION], -5.0);
    assert_eq!(p.output[o::STRIDE + o::POSITION], 57347.0);
    p.input[f::COMMANDS] = 0.0;
    for _ in 0..600 { p.input[f::DT] = 1.0 / 60.0; p.frame(); }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(p.header[h::REFUSED_TIMED], (lim::COMMANDS - lim::TIMED + 1) as f64);
    assert_eq!(p.header[h::TIMED], 0.0);
    assert_eq!(p.header[h::COUNT], 0.0);
}

use bfe_presentation::{interface_batch::Batch, interface_batch_layout::{interface_batch_frame as bf,
    interface_quad as quad, interface_batch_limits as bl}};
fn quad(b: &mut Batch, slot: usize, index: f64) {
    let a = &mut b.commands[slot * quad::STRIDE..(slot + 1) * quad::STRIDE];
    a.fill(0.0); a[quad::INDEX] = index;
    for i in [0, 5, 10, 15] { a[quad::MATRIX + i] = 1.0; }
    a[quad::RECT + 2] = 20.0; a[quad::RECT + 3] = 10.0;
    a[quad::UV + 3] = 1.0; a[quad::UV + 4] = 1.0; a[quad::UV + 6] = 1.0; a[quad::UV + 7] = 1.0;
    a[quad::MASK + 2] = 1.0; a[quad::MASK + 3] = 1.0;
    a[quad::TINT..quad::TINT + 4].fill(1.0);
}
#[test]
fn interface_quads_transform_publish_and_reuse_rounded_vertices() {
    let mut b = Batch::default(); quad(&mut b, 0, 0.0);
    b.commands[quad::MATRIX + 12] = 2.5;
    b.commands[quad::MATRIX + 13] = -3.0;
    b.commands[quad::MATRIX + 3] = 0.125;
    b.frame[bf::COUNT] = 1.0; b.frame[bf::COMMANDS] = 1.0; b.step();
    assert_eq!(&b.position[..3], &[2.5, -3.0, 0.0]);
    assert_eq!(b.position[6], (22.5f64 / 3.5) as f32);
    assert_eq!(&b.uv[..8], &[0.0,0.0,0.0,1.0,1.0,0.0,1.0,1.0]);
    assert_eq!(b.frame[bf::FIRST], 0.0); assert_eq!(b.frame[bf::LAST], 1.0);
    b.step(); assert_eq!(b.frame[bf::FIRST], 1.0); assert_eq!(b.frame[bf::LAST], 0.0);
    b.commands[quad::TINT + 3] = 0.5; b.step(); assert_eq!(b.tint[3], 0.5);
}
#[test]
fn interface_quads_bound_full_batches_and_foreign_dirty_spans_without_allocating() {
    let mut b = Batch::default();
    for i in 0..bl::QUADS { quad(&mut b, i, i as f64); }
    b.commands[quad::INDEX] = f64::INFINITY;
    b.frame[bf::COUNT] = f64::INFINITY; b.frame[bf::COMMANDS] = f64::INFINITY;
    ALLOCS.with(|n| n.set(0)); TRACK.with(|t| t.set(true));
    for _ in 0..100 { b.step(); }
    // A native HUD owner can overwrite vertices at its own copied indices.
    // No corner command is needed for that foreign geometry publication.
    b.position[3] = 9.0; b.dirty[0] = 1; b.frame[bf::COMMANDS] = 0.0; b.step();
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(b.position[3], 9.0);
    assert_eq!(b.frame[bf::FIRST], 0.0); assert_eq!(b.frame[bf::LAST], 1.0);
}

#[test]
fn shell_music_commands_allocate_nothing_after_content_registration() {
    use audio_control::{shell_music::Music, shell_music_layout::{shell_music_row as r, shell_music_op as op}};
    let mut m = Music::new(vec![vec![0, 1, 2]]);
    m.row[r::OP] = op::SET as f64; m.row[r::ENABLED] = 1.0; m.frame();
    ALLOCS.with(|n| n.set(0)); TRACK.with(|t| t.set(true));
    for _ in 0..100_000 {
        m.row[r::OP] = op::END as f64; m.frame();
        m.row[r::OP] = op::TICK as f64; m.row[r::DT] = 1e20; m.frame();
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
}
