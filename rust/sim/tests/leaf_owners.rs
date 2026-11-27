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
fn push_wall_scratch_reuses_reserved_probes_and_lends_repeated_caches_sequentially() {
    let mut owner = CollisionOwner::new();
    let world = floor(&mut owner, 0.);
    assert_eq!(owner.cache_new(0.75), 0);
    let probes = 1024;
    unsafe {
        let input = std::slice::from_raw_parts_mut(owner.reserve_input(22 + probes * 4).as_mut_ptr(), 22 + probes as usize * 4);
        input[3] = 1.; input[13] = 10.; input[18] = 0.2; input[20] = -1.;
        // The encoded count can exceed its input span. Missing probes are
        // ignored, and equal cache ids are borrowed one gather at a time.
        input[21] = 1e10;
    }
    owner.push_walls_ray(world);
    without_allocations(|| { for _ in 0..10 { owner.push_walls_ray(world); } });
    assert_eq!(unsafe { *owner.io_ptr().add(1) }, 10.);
}

#[test]
fn rejected_infantry_steps_allocate_no_slot_or_heap_storage() {
    use bfe_family::GameFamily;
    use bfe_sim::{gen::{MoveInput, Soldier}, movement::{soldier::swbf2::*, ProfileError},
        units::soldier::tick_soldier_in};

    let controls = ControlSpeed { forward: 1.0, strafe: 0.8, turn: 1.0 };
    let class = Tuning { stand: controls, crouch: controls, sprint: controls, jump: controls,
        jet: controls, jetpack: None, roll: controls, tumble: controls, sprint_accelerate_time: 0.2,
        energy_capacity: 48., energy_floor: -3., energy_restore: 4., energy_restore_idle: 5.,
        energy_drain_sprint: 7., energy_min_sprint: 6., energy_cost_jump: 2., energy_cost_jump_sprint: None,
        energy_cost_roll: 3., jump_height: 2., jump_forward_speed_factor: 2., jump_strafe_speed_factor: 1.,
        roll_speed_factor: 1., roll_frames: 11, recover_from_tumble: false,
        getup_front_seconds: DEFAULT_GETUP_FRONT_SECONDS, getup_back_seconds: DEFAULT_GETUP_BACK_SECONDS };
    let mut collision = CollisionOwner::new();
    let world = floor(&mut collision, 0.) as usize;
    let mut owner = MovementOwner::new();
    owner.select_family(GameFamily::Swbf2).unwrap();
    let mut life = State::new(&class);
    let mut body = Soldier { mover: -1., grounded: true, normal: [0., 1., 0.], ..Default::default() };
    for (dt, turn, speed, height, error) in [
        (-1., 0., 0., 1., ProfileError::InvalidDelta),
        (f64::NAN, 0., 0., 1., ProfileError::InvalidDelta),
        (0.01, f64::NAN, 0., 1., ProfileError::InvalidState),
        (0.01, 0., 0., f64::MAX, ProfileError::InvalidTuning),
        (0.01, 0., f64::MAX, 1., ProfileError::InvalidState),
    ] {
        let tuning = Tuning { jump_height: height, ..class };
        body.velocity = [0., 1., speed];
        let input = MoveInput { jump: true, turn, ..Default::default() };
        let step = Step { actions: Actions::default(), tuning: &tuning, state: &mut life, override_: None };
        without_allocations(|| {
            assert_eq!(tick_soldier_in(&mut owner, &mut collision.state, &mut body, &input, dt,
                world, -1, Some(step)), Err(error));
        });
        assert_eq!(body.mover, -1.);
        assert_eq!(body.position, [0.; 3]);
        assert_eq!(life.energy, class.energy_capacity);
    }
    owner.select_family(GameFamily::Swbf1).unwrap();
    owner.select_family(GameFamily::Swbf2).unwrap();
    body.velocity = [0.; 3];
    let step = Step { actions: Actions::default(), tuning: &class, state: &mut life, override_: None };
    assert!(tick_soldier_in(&mut owner, &mut collision.state, &mut body, &MoveInput::default(), 0.01,
        world, -1, Some(step)).is_ok());
    assert_eq!(body.mover, 0.);
}
