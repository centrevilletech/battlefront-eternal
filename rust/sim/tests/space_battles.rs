//! Fixed-step flight, collision, presentation and allocation checks.

use bfe_wide::{Fnv, Wide};
use bfe_content::bvh::{build as build_bvh, Positions};
use bfe_content::install::CollisionKernel;
use bfe_content::world_anim::{
    WorldAnimBinding, WorldAnimClip, WorldAnimGroup, WorldAnimKey, WorldAnimations,
};
use bfe_content::Value;
use bfe_sim::collision::CollisionOwner;
use bfe_sim::space::bounds::{HangarForcefield, OrientedRegion, SpaceBounds, DEFAULT_WORLD_EXTENTS};
use bfe_sim::space::combat::{SpaceProjectile, MAX_SPACE_PROJECTILES};
use bfe_sim::space::flight::{
    install_bounds_box_tree, SpaceFighter, SpaceFlightProfile, SpaceFlightState,
    SpaceOrdnanceKind, SpacePilotId, SpacePilotInput, SpaceVehiclePad, SpaceWeaponSpec,
};
use bfe_sim::space::objects::SpaceDoor;
use bfe_sim::space::world_anim::{AnimatedObject, SpaceWorldAnims};
use bfe_sim::space::{SpaceBattlefield, SpaceEvent, SPACE_FIXED_DT};
use bfe_sim::world::owner::World;
use std::sync::Mutex;
use three::Matrix4;

static TEST_SERIAL: Mutex<()> = Mutex::new(());

#[global_allocator]
static GLOBAL: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

fn count_allocs<R>(f: impl FnOnce() -> R) -> (R, u32) {
    let (out, totals) = bfe_test_alloc::measure_all(f);
    (out, totals.allocations)
}

#[test]
#[ignore = "requires a prepared Space Assault map"]
fn first_space_tick_after_fighter_admission_allocates_nothing() {
    use bfe_content::{files::DirFiles, GameFamily};
    use bfe_sim::world::admit::{admit_for_family, Bots, Options};

    let Some(root) = std::env::var_os("BFE_SWBF2_CONTENT") else {
        return;
    };
    let _guard = TEST_SERIAL.lock().unwrap();
    let mut world = World::new();
    let options = Options {
        family: GameFamily::Swbf2,
        map: "spa6".into(),
        mode: "ass".into(),
        side: "rep".into(),
        bots: Bots::default(),
        seed: 41,
        ..Default::default()
    };
    admit_for_family(&mut world, &DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2), GameFamily::Swbf2, &options,
        Some("spa6c_ass"), false).unwrap();
    assert!(world.space().unwrap().fighters.iter().any(|f| f.kernel_body.is_some()));
    let (stepped, allocations) = count_allocs(|| world.space_step_tick(SPACE_FIXED_DT));
    assert!(stepped);
    assert_eq!(allocations, 0);
}

#[inline]
fn mix_word(mut acc: Fnv, val: Wide) -> Fnv {
    acc.word(val);
    acc
}

fn make_empty_world(coll: &mut CollisionOwner) -> usize {
    let empty_bvh = build_bvh(Positions::F32(&[]), &[]);
    let empty_pos = coll.positions(&[]);
    let empty_tree = coll.tree(empty_pos, &[], &empty_bvh);
    coll.world_new(empty_tree, -1, -1) as usize
}

fn add_static_box(
    coll: &mut CollisionOwner,
    world: usize,
    min: [f64; 3],
    max: [f64; 3],
    flags: &mut [u8],
    enabled: bool,
) -> u32 {
    let tree = install_bounds_box_tree(coll, min, max);
    let body = coll.body_new(tree, 0) as u32;
    coll.body_offset(body, (body as f64 + 1.0) * 16777216.0);
    let io = coll.io_mut();
    io[20..23].copy_from_slice(&min);
    io[23..26].copy_from_slice(&max);
    coll.body_set(body);
    coll.state.worlds[world].order.push(body);
    coll.state.worlds[world].version += 1;
    let order = coll.state.worlds[world].order.len() - 1;
    flags[order] = u8::from(enabled);
    coll.enable_body(body, enabled);
    body
}

fn add_moving_box(
    coll: &mut CollisionOwner,
    world: usize,
    local_min: [f64; 3],
    local_max: [f64; 3],
    pos: [f64; 3],
    flags: &mut [u8],
) -> u32 {
    let tree = install_bounds_box_tree(coll, local_min, local_max);
    let body = coll.body_new(tree, 1) as u32;
    coll.body_offset(body, (body as f64 + 1.0) * 16777216.0);
    let mut m = Matrix4::default();
    m.elements[12] = pos[0];
    m.elements[13] = pos[1];
    m.elements[14] = pos[2];
    let mut inv = m;
    inv.invert();
    let world_box = [
        local_min[0] + pos[0],
        local_min[1] + pos[1],
        local_min[2] + pos[2],
        local_max[0] + pos[0],
        local_max[1] + pos[1],
        local_max[2] + pos[2],
    ];
    let io = coll.io_mut();
    io[20..26].copy_from_slice(&world_box);
    io[26..42].copy_from_slice(&m.elements);
    io[42..58].copy_from_slice(&inv.elements);
    coll.body_set(body);
    coll.state.worlds[world].order.push(body);
    coll.state.worlds[world].version += 1;
    let order = coll.state.worlds[world].order.len() - 1;
    flags[order] = 1;
    coll.enable_body(body, true);
    body
}

fn steer_fighter_toward(
    fighter: &SpaceFighter,
    target: [f64; 3],
    boost: bool,
    fire_primary: bool,
    fire_secondary: bool,
    lock_target: bool,
) -> SpacePilotInput {
    let fwd = fighter.forward();
    let right = fighter.right();
    let up = fighter.up();
    let dx = target[0] - fighter.position[0];
    let dy = target[1] - fighter.position[1];
    let dz = target[2] - fighter.position[2];
    let dist = v8math::hypot3(dx, dy, dz);
    if dist < 1e-6 {
        return SpacePilotInput::default();
    }
    let dir = [dx / dist, dy / dist, dz / dist];
    let local_x = dir[0] * right[0] + dir[1] * right[1] + dir[2] * right[2];
    let local_y = dir[0] * up[0] + dir[1] * up[1] + dir[2] * up[2];
    let local_z = dir[0] * fwd[0] + dir[1] * fwd[1] + dir[2] * fwd[2];
    let yaw_err = v8math::atan2(local_x, local_z);
    let pitch_err = v8math::asin(v8math::min(1.0, v8math::max(-1.0, local_y)));
    let stick_x = v8math::min(1.0, v8math::max(-1.0, -yaw_err * 2.5));
    let stick_y = v8math::min(1.0, v8math::max(-1.0, pitch_err * 2.5));
    let aligned = local_z > 0.94;
    SpacePilotInput {
        weapon_index: None,
        aim: None,
        spread_scale: None,
        throttle: if dist > 120.0 { 1.0 } else { -0.4 },
        stick_x,
        stick_y,
        roll: 0.0,
        roll_lock: false,
        boost: boost && local_z > 0.85,
        takeoff_or_land: false,
        trick_roll: 0,
        trick_flip: 0,
        eject_held: false,
        fire_primary: fire_primary && aligned,
        fire_secondary: fire_secondary && aligned,
        lock_target,
    }
}

// ---- Synthetic Space World Tests (Purely Invented Fixtures, Zero Retail Data) ----

fn build_synthetic_space_world(fighter_count: usize, extra_speed: f64) -> Box<World> {
    let mut world = World::new();
    world.space_install_synthetic(|collision, flags| {
        let coll_world = make_empty_world(collision);

        // Hangar floor deck under north_bay at y = 0.0, z in [-540, -460].
        let _north_deck = add_static_box(
            collision,
            coll_world,
            [-60.0, -2.0, -540.0],
            [60.0, 0.0, -460.0],
            flags,
            true,
        );

        // Hangar forcefield slab at z = -450 (flyers pass through, infantry shots stop).
        let mut ff_m = Matrix4::default();
        ff_m.elements[12] = 0.0;
        ff_m.elements[13] = 8.0;
        ff_m.elements[14] = -450.0;
        let mut ff_inv = ff_m;
        ff_inv.invert();
        let ff_body = add_static_box(
            collision,
            coll_world,
            [-40.0, -2.0, -451.0],
            [40.0, 20.0, -449.0],
            flags,
            true,
        );

        // Orbiting escort frigate moving along X from 120 to 220 over 4 s.
        let frig_body = add_moving_box(
            collision,
            coll_world,
            [-12.0, -6.0, -25.0],
            [12.0, 6.0, 25.0],
            [120.0, 30.0, 0.0],
            flags,
        );
        let mut frig_base = Matrix4::default();
        frig_base.elements[12] = 120.0;
        frig_base.elements[13] = 30.0;
        let mut frig_inv = frig_base;
        frig_inv.invert();

        let world_anims_src = WorldAnimations {
            clips: vec![WorldAnimClip {
                name: "orbit_escort".into(),
                duration: 4.0,
                looping: true,
                local_translation: true,
                positions: vec![
                    WorldAnimKey {
                        time: 0.0,
                        value: [0.0, 0.0, 0.0],
                        kind: 1,
                        tangents: [0.0; 6],
                    },
                    WorldAnimKey {
                        time: 4.0,
                        value: [100.0, 0.0, 0.0],
                        kind: 1,
                        tangents: [0.0; 6],
                    },
                ],
                rotations: Vec::new(),
            }],
            groups: vec![WorldAnimGroup {
                name: "grp_escort".into(),
                default_on: true,
                stop_when_controlled: false,
                disable_hierarchies: false,
                bindings: vec![WorldAnimBinding {
                    clip: "orbit_escort".into(),
                    object: "escort_frigate_1".into(),
                }],
            }],
            hierarchies: Vec::new(),
        };
        let world_anims = SpaceWorldAnims::build(
            &world_anims_src,
            vec![AnimatedObject {
                name: "escort_frigate_1".into(),
                prop_instance: 0,
                base_matrix: frig_base.elements,
                base_inverse: frig_inv.elements,
                prev_matrix: frig_base.elements,
                live_matrix: frig_base.elements,
                live_inverse: frig_inv.elements,
                bodies: [Some((frig_body, [-12.0, -6.0, -25.0, 12.0, 6.0, 25.0])), None, None],
            }],
        );

        // Indestructible interior proximity door at (0, 0, 500).
        let door_body = add_static_box(
            collision,
            coll_world,
            [-3.0, 0.0, 499.5],
            [3.0, 4.0, 500.5],
            flags,
            true,
        );
        let door = SpaceDoor {
            id: 0,
            instance: 1,
            name: "south_airlock_1".into(),
            class: "synth_door".into(),
            body: Some(door_body),
            matrix: Matrix4::default().elements,
            position: [0.0, 0.0, 500.0],
            center: [0.0, 2.0, 500.0],
            trigger_position: [0.0, 0.0, 500.0],
            opening_clip: None,
            door_trigger_radius: 4.0,
            door_open: false,
            prev_door_open: false,
            open_fraction: 0.0, previous_fraction: 0.0,
            locked: false,
            collidable: true,
            visible: true,
        };

        let mut profile = SpaceFlightProfile {
            class_name: "synth_interceptor".into(),
            side: "north".into(),
            max_health: 2000.0,
            min_speed: 35.0,
            mid_speed: 75.0 + extra_speed,
            max_speed: 120.0 + extra_speed,
            boost_speed: 175.0 + extra_speed,
            acceleration: 60.0,
            boost_acceleration: 100.0,
            turn_rate: 2.0,
            pitch_rate: 2.0,
            takeoff_height: 3.0,
            takeoff_time: 0.4,
            takeoff_speed: 15.0,
            landing_time: 0.4,
            landing_speed: 15.0,
            bounds_min: [-3.0, -1.5, -4.0],
            bounds_max: [3.0, 1.5, 4.0],
            contact_bounds_min: [-3.0, -1.5, -4.0],
            contact_bounds_max: [3.0, 1.5, 4.0],
            weapons: vec![
                SpaceWeaponSpec {
                    name: "synth_cannon".into(),
                    channel: 0,
                    ordnance_kind: SpaceOrdnanceKind::Bolt,
                    shot_delay: 0.12,
                    velocity: 1000.0,
                    life_span: 1.5,
                    max_damage: 120.0,
                    final_damage: Some(60.0),
                    damage_transition_delay: 0.2,
                    damage_transition_period: 0.5,
                    pitch_spread: 0.0075,
                    yaw_spread: 0.015,
                    muzzles: vec![[-2.0, 0.0, 3.0], [2.0, 0.0, 3.0]],
                    ..Default::default()
                },
                SpaceWeaponSpec {
                    name: "synth_torpedo".into(),
                    channel: 1,
                    ordnance_kind: SpaceOrdnanceKind::Missile,
                    shot_delay: 0.5,
                    velocity: 320.0,
                    life_span: 3.0,
                    turn_rate: 3.5,
                    lock_on_range: 800.0,
                    lock_on_angle_deg: 20.0,
                    lock_off_angle_deg: 30.0,
                    lock_time: 0.15,
                    max_damage: 300.0,
                    muzzles: vec![[0.0, -1.0, 2.0]],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let fighter_tree = install_bounds_box_tree(collision, profile.bounds_min, profile.bounds_max);
        profile.collision_tree = Some(fighter_tree);

        let mut bounds = SpaceBounds::from_world_json(&Value::Null);
        bounds.set_world_extents(2000.0);
        let mut north_m = Matrix4::default();
        north_m.elements[12] = 0.0;
        north_m.elements[13] = 10.0;
        north_m.elements[14] = -500.0;
        bounds.add_landing_region(OrientedRegion::from_matrix(
            "north_bay".into(),
            1,
            north_m.elements,
            [55.0, 15.0, 45.0],
            0.0,
        ));
        bounds.forcefields.push(HangarForcefield {
            name: "north_shield_door".into(),
            team: 1,
            center: [0.0, 8.0, -450.0],
            matrix: ff_m.elements,
            inverse: ff_inv.elements,
            half_extents: [40.0, 12.0, 1.5],
            kernel_body: Some(ff_body),
            authored_opening: false,
        });

        let mut pads = Vec::new();
        let mut fighters = Vec::new();
        for i in 0..fighter_count {
            let mut pad_m = Matrix4::default();
            let x = -40.0 + (i % 8) as f64 * 12.0;
            let z = -520.0 + (i / 8) as f64 * 12.0;
            pad_m.elements[12] = x;
            pad_m.elements[14] = z;
            let pad = SpaceVehiclePad {
                name: format!("pad_{i}"),
                team: if i.is_multiple_of(2) { 1 } else { 2 },
                profile_index: 0,
                control_zone: String::new(),
                position: [x, 0.0, z],
                yaw: 0.0,
                matrix: pad_m.elements,
                spawn_count: 1,
                spawn_time: 8.0,
                expire_time_enemy: 20.0,
                expire_time_field: 40.0,
                decay_time: 10.0,
                respawn_timer: 8.0,
                floor_y: 0.0,
            };
            let fbody = collision.body_new(fighter_tree, 1) as u32;
            collision.body_offset(fbody, (fbody as f64 + 1.0) * 16777216.0);
            collision.state.worlds[coll_world].order.push(fbody);
            collision.state.worlds[coll_world].version += 1;
            let f = SpaceFighter::spawned_on_pad(
                i,
                1,
                i,
                &pad,
                0,
                &profile,
                0.0,
                Some(0),
                Some(fbody),
            );
            f.sync_collision_body(&profile, collision, coll_world, flags);
            pads.push(pad);
            fighters.push(f);
        }

        SpaceBattlefield::from_parts(
            bounds,
            world_anims,
            vec![profile],
            pads,
            fighters,
            Vec::new(),
            vec![door],
            coll_world,
            collision.state.bodies.len(),
        )
    });
    world
}

fn run_synthetic_sortie(extra_speed: f64) -> Wide {
    let mut world = build_synthetic_space_world(2, extra_speed);
    let pilot = SpacePilotId {
        owner: bfe_sim::store::Ref::new(1, 1),
        team: 1,
        is_human: true,
        repair_capable: true,
    };
    world.space_mut().unwrap().fighters[0].pilot = Some(pilot);

    // Infantry shot aimed across hangar forcefield at z = -450 is blocked,
    // while starfighter passes straight through `north_shield_door`.
    assert!(world
        .space_mut()
        .unwrap()
        .fire_infantry_shot(
            [0.0, 5.0, -460.0],
            [0.0, 0.0, 1.0],
            400.0,
            100.0,
            0.1,
            1.0,
            0.0,
            0.0,
            Some(bfe_sim::store::Ref::new(1, 1)),
            1,
            false,
        )
        .is_some());
    assert!(world.space_step_tick(SPACE_FIXED_DT));
    assert!(world.space_step_tick(SPACE_FIXED_DT));
    assert_eq!(
        world.space().unwrap().ordnance.active_count(),
        0,
        "hangar forcefield must block infantry shots"
    );

    // Takeoff and fly cleanly through the hangar forcefield into open space.
    let mut rolling = Fnv::new();
    world.space_mut().unwrap().fighters[0].pending_input = SpacePilotInput {
        takeoff_or_land: true,
        ..Default::default()
    };
    assert!(world.space_step_tick(SPACE_FIXED_DT));
    assert!(world
        .space()
        .unwrap()
        .events()
        .iter()
        .any(|e| matches!(e, SpaceEvent::FighterTakeoff { fighter: 0, .. })));
    rolling = mix_word(rolling, world.space_state_digest().unwrap());

    for _ in 0..160 {
        world.space_mut().unwrap().fighters[0].pending_input = SpacePilotInput {
            throttle: 1.0,
            boost: true,
            fire_primary: true,
            ..Default::default()
        };
        assert!(world.space_step_tick(SPACE_FIXED_DT));
        rolling = mix_word(rolling, world.space_state_digest().unwrap());
    }
    assert!(
        world.space().unwrap().fighters[0].position[2] > -420.0,
        "starfighter must pass cleanly through hangar forcefield into open space"
    );

    // Slow down inside landing zone and land cleanly.
    {
        let f = &mut world.space_mut().unwrap().fighters[0];
        f.position = [0.0, 8.0, -500.0];
        f.airspeed = 45.0;
    }
    let mut saw_landed = false;
    for step_idx in 0..90 {
        world.space_mut().unwrap().fighters[0].pending_input = SpacePilotInput {
            throttle: -1.0,
            takeoff_or_land: step_idx == 0,
            ..Default::default()
        };
        assert!(world.space_step_tick(SPACE_FIXED_DT));
        rolling = mix_word(rolling, world.space_state_digest().unwrap());
        if world
            .space()
            .unwrap()
            .events()
            .iter()
            .any(|e| matches!(e, SpaceEvent::FighterLanded { fighter: 0, zone: 0, .. }))
        {
            saw_landed = true;
            break;
        }
    }
    assert!(saw_landed);
    assert_eq!(world.space().unwrap().fighters[0].state, SpaceFlightState::Landed);
    rolling.finish()
}

#[test]
fn synthetic_space_battles_are_deterministic_sensitive_and_zero_alloc_under_full_load() {
    let _guard = TEST_SERIAL.lock().unwrap();

    let d1 = run_synthetic_sortie(0.0);
    let d2 = run_synthetic_sortie(0.0);
    assert_eq!(d1, d2, "synthetic sortie must be bit-identical across runs");
    let d_alt = run_synthetic_sortie(15.0);
    assert_ne!(d1, d_alt, "altering flight speed must change digest");

    // Full-load zero-alloc stress proof: 32 active airborne fighters +
    // 512 saturated ordnance slots stepping 60 ticks with ZERO heap allocations!
    let mut stress = build_synthetic_space_world(32, 0.0);
    {
        let sp = stress.space_mut().unwrap();
        for i in 0..32usize {
            let team = if i.is_multiple_of(2) { 1 } else { 2 };
            let pilot = SpacePilotId {
                owner: bfe_sim::store::Ref::new(i as u32 + 1, 1),
                team,
                is_human: false,
                repair_capable: true,
            };
            let f = &mut sp.fighters[i];
            f.pilot = Some(pilot);
            f.team = team;
            f.state = SpaceFlightState::Flying;
            f.position = [
                -300.0 + (i % 8) as f64 * 85.0,
                80.0 + (i / 8) as f64 * 45.0,
                if team == 1 { -200.0 } else { 200.0 },
            ];
            f.airspeed = 90.0;
            f.pending_input = SpacePilotInput {
                throttle: 0.8,
                stick_x: 0.2,
                boost: true,
                fire_primary: true,
                fire_secondary: true,
                lock_target: true,
                ..Default::default()
            };
        }
        // Saturate all 512 ordnance slots before measuring.
        for s in 0..MAX_SPACE_PROJECTILES {
            let team = if s.is_multiple_of(2) { 1 } else { 2 };
            let _ = sp.ordnance.spawn(SpaceProjectile {
                generation: 0, previous_position: [0.0; 3],
            active: true,
                kind: SpaceOrdnanceKind::Bolt,
                owner_team: team,
                position: [-400.0 + (s % 32) as f64 * 25.0, 120.0, -300.0],
                velocity: [0.0, 0.0, 500.0],
                life_span: 5.0,
                base_damage: 100.0,
                ..Default::default()
            });
        }
        assert_eq!(sp.ordnance.active_count(), MAX_SPACE_PROJECTILES);
    }

    // Warm up the collision broadphase once (built lazily on first raycast),
    // then re-saturate all 512 ordnance slots and verify ZERO allocations
    // across 60 full-load ticks.
    assert!(stress.space_step_tick(SPACE_FIXED_DT));
    {
        let sp = stress.space_mut().unwrap();
        while sp.ordnance.active_count() < MAX_SPACE_PROJECTILES {
            let s = sp.ordnance.active_count();
            let team = if s.is_multiple_of(2) { 1 } else { 2 };
            let _ = sp.ordnance.spawn(SpaceProjectile {
                generation: 0, previous_position: [0.0; 3],
            active: true,
                kind: SpaceOrdnanceKind::Bolt,
                owner_team: team,
                position: [-400.0 + (s % 32) as f64 * 25.0, 120.0, -300.0],
                velocity: [0.0, 0.0, 500.0],
                life_span: 5.0,
                base_damage: 100.0,
                ..Default::default()
            });
        }
        assert_eq!(sp.ordnance.active_count(), MAX_SPACE_PROJECTILES);
    }

    let ((), allocs) = count_allocs(|| {
        for _ in 0..60 {
            assert!(stress.space_step_tick(SPACE_FIXED_DT));
        }
    });
    assert_eq!(
        allocs, 0,
        "32 active fighters + 512 ordnance saturated must allocate 0 heap blocks per tick"
    );
}

#[test]
fn space_wire_bank_capture_and_pilot_decode_allocate_nothing() {
    use bfe_sim::space::local::PilotControls;
    use bfe_sim::wire::space::{
        read_pilot_entries_into, write_own_pilot, write_pilot_entries, Bank, PilotEntry,
        MAX_PILOT_ENTRIES,
    };
    use bfe_sim::wire::PositionBounds;

    let _guard = TEST_SERIAL.lock().unwrap();
    let mut stress = build_synthetic_space_world(32, 0.0);
    stress.enable_space_wire_observations();
    {
        let sp = stress.space_mut().unwrap();
        for i in 0..32usize {
            let team = if i.is_multiple_of(2) { 1 } else { 2 };
            let pilot = SpacePilotId {
                owner: bfe_sim::store::Ref::new(i as u32 + 1, 1),
                team,
                is_human: i == 0,
                repair_capable: true,
            };
            let f = &mut sp.fighters[i];
            f.pilot = Some(pilot);
            f.team = team;
            f.state = SpaceFlightState::Flying;
            f.position = [
                -300.0 + (i % 8) as f64 * 85.0,
                80.0 + (i / 8) as f64 * 45.0,
                if team == 1 { -200.0 } else { 200.0 },
            ];
            f.airspeed = 90.0;
            f.pending_input = SpacePilotInput {
                throttle: 0.8,
                stick_x: 0.2,
                boost: true,
                fire_primary: true,
                fire_secondary: true,
                lock_target: true,
                ..Default::default()
            };
        }
    }

    let wire_bounds = PositionBounds::from_map_extents([-2000.0; 3], [2000.0; 3]).unwrap();
    let mut bank = Bank::new(stress.space().unwrap(), wire_bounds);
    let mut own_buf = Vec::with_capacity(512);
    let mut pilot_pkt = Vec::new();
    write_pilot_entries(
        &[PilotEntry {
            sequence: 7,
            controls: PilotControls {
                flight: SpacePilotInput {
                    throttle: 1.0,
                    fire_primary: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        }],
        &mut pilot_pkt,
    )
    .unwrap();
    let mut pilot_scratch = [PilotEntry::default(); MAX_PILOT_ENTRIES];

    // Warm up 2 ticks + 1 capture.
    for t in 1..=2u32 {
        assert!(stress.space_step_tick(SPACE_FIXED_DT));
        bank.sample(&mut stress, t);
    }

    let ((), allocs) = count_allocs(|| {
        for tick in 3..63u32 {
            let count = read_pilot_entries_into(&pilot_pkt, &mut pilot_scratch).unwrap();
            assert_eq!(count, 1);
            assert!(stress.space_step_tick(SPACE_FIXED_DT));
            if tick % 3 == 0 {
                bank.sample(&mut stress, tick);
                let sp = stress.space().unwrap();
                own_buf.clear();
                write_own_pilot(
                    &sp.fighters[0],
                    &sp.profiles[0],
                    0,
                    Some(0),
                    [0.0, 0.0],
                    false,
                    [0.0, 0.0],
                    true,
                    &bfe_sim::space::local::DoubleTapDetector::default(),
                    &mut own_buf,
                )
                .unwrap();
                assert!(bank.offer_count() > 0);
            }
        }
    });
    assert_eq!(
        allocs, 0,
        "Space wire Bank::sample + write_own_pilot + read_pilot_entries_into + step must allocate 0 bytes"
    );
}
