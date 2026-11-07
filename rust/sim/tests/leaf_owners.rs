//! Instance isolation for the production collision, movement and pose owners.
//! Equal local ids are deliberate: a later call must not select another bank.
use bfe_content::bvh::{build, Positions};
use bfe_sim::{collision::{CollisionOwner, Corners, Tree}, movement::MovementOwner, pose};

use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell};
thread_local! { static TRACK: Cell<bool> = const { Cell::new(false) }; static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
struct Tracking;
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|active| if active.get() { ALLOCATIONS.with(|n| n.set(n.get() + 1)); });
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|active| if active.get() { ALLOCATIONS.with(|n| n.set(n.get() + 1)); });
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|active| if active.get() { ALLOCATIONS.with(|n| n.set(n.get() + 1)); });
        System.realloc(pointer, layout, size)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) { System.dealloc(pointer, layout); }
}
#[global_allocator]
static ALLOCATOR: Tracking = Tracking;
fn without_allocations(run: impl FnOnce()) {
    ALLOCATIONS.with(|n| n.set(0)); TRACK.with(|active| active.set(true));
    run();
    TRACK.with(|active| active.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

fn floor(owner: &mut CollisionOwner, height: f32) -> u32 {
    let positions = vec![-4., height, -4., 4., height, -4., 4., height, 4., -4., height, 4.];
    let index = vec![0, 2, 1, 0, 3, 2];
    let built = build(Positions::F32(&positions), &index);
    owner.state.positions.push(Corners::F32(positions));
    owner.state.trees.push(Tree { roots: built.roots, positions: 0, index, indexed: true, indirect: built.indirect, polygons: vec![17, 18] });
    let world = owner.world_new(0, -1, -1);
    let set = owner.set_new(0, 2, 0.);
    // The loader's immutable bounds and polygon arrays live in this owner.
    unsafe {
        std::slice::from_raw_parts_mut(owner.set_bounds(set), 12).copy_from_slice(&[-4., height, -4., 4., height, 4., -4., height, -4., 4., height, 4.]);
        std::slice::from_raw_parts_mut(owner.set_polygons(set), 2).copy_from_slice(&[17, 18]);
    }
    owner.world_statics(world, set);
    world
}
fn cast(owner: &mut CollisionOwner, world: u32) -> f64 {
    unsafe {
        let io = std::slice::from_raw_parts_mut(owner.io_ptr(), 64);
        io[..9].copy_from_slice(&[0., 20., 0., -0., -1., 0., 50., 0., 0.]);
    }
    assert_eq!(owner.world_candidates(world, -1, 0, 0), 0);
    assert_eq!(owner.world_cast(world, 0), -1);
    unsafe { *owner.io_ptr().add(10) }
}
fn gather(owner: &mut CollisionOwner, world: u32, cache: u32) -> u32 {
    unsafe { std::slice::from_raw_parts_mut(owner.io_ptr(), 4).copy_from_slice(&[0., 0., 0., 10.]); }
    let count = owner.gather_begin(world, cache as i32, -1, -1);
    owner.gather_end(world, count)
}

#[test]
fn geometry_queries_and_two_call_gathers_are_instance_local() {
    let mut a = CollisionOwner::new();
    let mut b = CollisionOwner::new();
    assert_eq!(floor(&mut a, 0.), 0);
    assert_eq!(floor(&mut b, 3.), 0);
    assert_eq!(a.cache_new(0.75), 0);
    assert_eq!(b.cache_new(0.75), 0);
    assert_eq!(cast(&mut a, 0), 20.);
    assert_eq!(cast(&mut b, 0), 17.);
    // Growth in the second owner cannot invalidate the first query's cache.
    for _ in 0..1024 { b.cache_new(0.75); }
    assert_eq!(gather(&mut a, 0, 0), 2);
    assert_eq!(gather(&mut b, 0, 0), 2);
    drop(b);
    assert_eq!(cast(&mut a, 0), 20.);
    assert_eq!(gather(&mut a, 0, 0), 2);
}

#[test]
fn pose_and_movement_local_ids_survive_another_owner_drop() {
    let mut a = pose::State::default();
    let mut b = pose::State::default();
    assert_eq!(a.pose_new(1, 0), 0);
    assert_eq!(b.pose_new(2, 0), 0);
    assert_eq!(a.clip_new(2, 1, 30., 0), 0);
    assert_eq!(b.clip_new(3, 2, 60., 1), 0);
    assert_eq!(a.clips[0].frames, 2);
    assert_eq!(b.clips[0].frames, 3);
    b.pose_clear(0);
    drop(b);
    assert_eq!(a.poses.len(), 1);
    assert_eq!(a.clips[0].frames, 2);
    let mut ma = MovementOwner::default();
    let mut mb = MovementOwner::default();
    assert_eq!(ma.new_mover(0, |m| m.max_speed = 2.), 0);
    assert_eq!(mb.new_mover(0, |m| m.max_speed = 9.), 0);
    ma.reserve_mover(0, 64, 128, 256);
    assert_eq!(ma.mover(0).max_speed, 2.);
    assert_eq!(mb.mover(0).max_speed, 9.);
    drop(mb);
    assert_eq!(ma.mover(0).max_speed, 2.);
}

#[test]
fn exchange_banks_keep_full_capacity_payloads_and_float_bits_independent() {
    use bfe_sim::store::{block::HEADER, calls::{ImportExchange, EXCHANGE}};
    let mut a = ImportExchange::new();
    let mut b = ImportExchange::new();
    let nan = f64::from_bits(0x7ff8_0000_0000_0042);
    a.out(2, |row, heap| {
        row.copy_from_slice(&[-0., nan]);
        heap.resize(EXCHANGE - HEADER - 2, 19.);
    });
    b.out(1, |row, heap| { row[0] = 7.; heap.extend_from_slice(&[3., 4.]); });
    // A nested exchange has its own scratch. The outer full-size record can
    // still be consumed after the nested owner grows and is dropped.
    for size in [8, 64, EXCHANGE - HEADER - 1] {
        b.out(1, |row, heap| { row[0] = 8.; heap.resize(size, 1.); });
    }
    drop(b);
    a.read(|row, heap| {
        assert_eq!(row[0].to_bits(), (-0.0f64).to_bits());
        assert_eq!(row[1].to_bits(), nan.to_bits());
        assert_eq!(heap.len(), EXCHANGE - HEADER - 2);
        assert!(heap.iter().all(|&value| value == 19.));
    });
}

#[test]
fn allocation_diagnostics_bound_overflow_and_restore_nested_stages() {
    use bfe_sim::{allocations::{Counts, Diagnostics}, gen::{F3Allocation, F3AllocationStage}};
    let mut a = Diagnostics::new();
    let mut b = Diagnostics::new();
    a.prepare(); b.prepare();
    a.begin(); b.begin();
    let outer = a.stage(F3AllocationStage::Other);
    a.record(13, false);
    let inner = a.stage(F3AllocationStage::Input);
    a.record(21, true);
    a.stage(inner);
    a.record(34, false);
    a.stage(outer);
    for _ in 0..65536 { b.record(1, false); }
    a.end(); b.end();
    assert_eq!(a.snapshot(), Counts { calls: 2, reallocs: 1, bytes: 68 });
    assert_eq!(b.snapshot().calls, 65536);
    unsafe {
        let words = std::slice::from_raw_parts(a.publish(), 5 + 3 * F3Allocation::WORDS);
        assert_eq!(words[0], 3.);
        assert_eq!(F3Allocation::take(&words[5..], &[]).stage, F3AllocationStage::Other);
        assert_eq!(F3Allocation::take(&words[5 + F3Allocation::WORDS..], &[]).stage, F3AllocationStage::Input);
        let overflow = b.publish();
        assert_eq!(*overflow, 128.);
    }
    // Ending the trace leaves totals live, as the production allocator does.
    a.record(5, false);
    assert_eq!(a.snapshot().bytes, 73);
    assert_eq!(unsafe { *a.publish() }, 3.);
    without_allocations(|| { for _ in 0..100 { a.publish(); b.publish(); } });
}

#[test]
fn push_wall_scratch_reuses_reserved_probes_and_lends_repeated_caches_sequentially() {
    let mut owner = CollisionOwner::new();
    let world = floor(&mut owner, 0.);
    assert_eq!(owner.cache_new(0.75), 0);
    let probes = 1024;
    unsafe {
        let input = std::slice::from_raw_parts_mut(owner.input_reserve(22 + probes * 4), 22 + probes as usize * 4);
        input[3] = 1.; input[13] = 10.; input[18] = 0.2; input[20] = -1.;
        // The encoded count can exceed its input span. Missing probes are
        // ignored, and equal cache ids are borrowed one gather at a time.
        input[21] = 1e10;
    }
    owner.push_walls_ray(world);
    without_allocations(|| { for _ in 0..10 { owner.push_walls_ray(world); } });
    assert_eq!(unsafe { *owner.io_ptr().add(1) }, 10.);
}
