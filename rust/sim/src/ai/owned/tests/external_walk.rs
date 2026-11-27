//! Invented-geometry tests for the 2005 external ground-walk seam
//! (`World::{nav_walk_begin, nav_walk_step, nav_walk_stop}`).
use super::phase::{ALLOCATIONS, COUNTING};
use crate::{
    abi::context::Context,
    ai::owned::{
        entry::{ai_world_begin_in, ai_world_begin_swbf2},
        walk::test_cast_count,
    },
    gen::{
        AiCapabilities, AiCapacity, AiColdSettings, AiNavPools, AiNavSetup, AiWorldSetup, Soldier,
        Unit, UnitBody,
    },
    movement::soldier::{Input, Mover, V3},
    store::Ref,
    world::owner::{NavWalkStatus, World},
};
use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision},
};

struct Mesh {
    vertices: Vec<f32>,
    triangles: Vec<u32>,
}

impl Mesh {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
        }
    }

    /// Add an upward-facing horizontal or ramped quad.
    fn quad(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) {
        let base = (self.vertices.len() / 3) as u32;
        self.vertices.extend(a);
        self.vertices.extend(b);
        self.vertices.extend(c);
        self.vertices.extend(d);
        self.triangles
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }

    /// Add a solid axis-aligned box with outward-facing triangles.
    fn solid_box(&mut self, x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32) {
        // Top (+y)
        self.quad([x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]);
        // Bottom (-y)
        self.quad([x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]);
        // -x face
        self.quad([x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]);
        // +x face
        self.quad([x1, y0, z1], [x1, y1, z1], [x1, y1, z0], [x1, y0, z0]);
        // -z face
        self.quad([x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y0, z0]);
        // +z face
        self.quad([x0, y0, z1], [x0, y1, z1], [x1, y1, z1], [x1, y0, z1]);
    }

    fn into_collision(self) -> MapCollision {
        let polygons = (0..self.triangles.len() as u32 / 3).collect::<Vec<_>>();
        MapCollision {
            tri_bounds: triangle_bounds(&self.vertices, &self.triangles),
            positions: self.vertices.into(),
            indices: self.triangles.into(),
            polygons: polygons.into(),
            masks: Vec::new().into(),
            grid: None,
            terrain_positions: Vec::new().into(),
            terrain_indices: Vec::new().into(),
            has_terrain: false,
            kill_y: -40.,
        }
    }
}

fn swbf2_plan_json(hubs: &[([f64; 3], f64)], arcs: &[(usize, usize)]) -> Vec<u8> {
    let mut hub_slots: Vec<Vec<usize>> = vec![Vec::new(); hubs.len()];
    let mut arcs_json = Vec::with_capacity(arcs.len());
    for (idx, &(a, b)) in arcs.iter().enumerate() {
        hub_slots[a].push(idx);
        hub_slots[b].push(idx);
        arcs_json.push(format!(
            r#"{{"name":"arc_{idx}","start":{a},"end":{b},"filterFlags":31,"typeFlags":0}}"#
        ));
    }
    let mut hubs_json = Vec::with_capacity(hubs.len());
    for (idx, (&(pos, radius), slots)) in hubs.iter().zip(&hub_slots).enumerate() {
        let mut s = [255usize; 8];
        for (k, &a) in slots.iter().take(8).enumerate() {
            s[k] = a;
        }
        hubs_json.push(format!(
            r#"{{"name":"hub_{idx}","position":[{},{},{}],"radius":{radius},"slots":[{},{},{},{},{},{},{},{}],"weightCounts":[0,0,0,0,0],"weights":[]}}"#,
            pos[0], pos[1], pos[2], s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]
        ));
    }
    format!(
        r#"{{"schema":2,"map":"invented_external","plan":{{"branchCount":5,"hubs":[{}],"arcs":[{}]}},"barriers":[],"hints":[]}}"#,
        hubs_json.join(","),
        arcs_json.join(","),
    )
    .into_bytes()
}

fn setup_args(world: &mut World, nav_json: Vec<u8>) {
    let setup = AiWorldSetup {
        settings: AiColdSettings {
            seed: 7,
            capacity: AiCapacity {
                registry: 16,
                characters: 8,
                posts: 4,
                hulls: 0,
                buildings: 4,
                controls: 4,
                scopes: 4,
                cameras: 1,
                support_keys: 4,
                repair_jobs: 4,
                grenades: 4,
            },
            capabilities: AiCapabilities {
                navigation: true,
                combat: false,
                combat_agent: false,
                population: true,
                vehicles: false,
                classes: true,
                random_b: true,
                mine_ground: false,
                vehicle_adapter: false,
            },
            ..Default::default()
        },
        navigation: AiNavSetup {
            json: nav_json,
            pools: AiNavPools {
                paths: 256.0,
                path_nodes: 2048.0,
                requests: 32.0,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    world
        .sim
        .calls
        .result(AiWorldSetup::WORDS, |row, heap| setup.put(row, heap));
}

fn install_world(
    scene: MapCollision,
    door_mesh: Option<(MapCollision, [f64; 6])>,
    nav_json: Vec<u8>,
    swbf2: bool,
) -> (Context, usize, Option<usize>) {
    let ctx = Context::new();
    let tree = build(Positions::F32(&scene.positions), &scene.indices);
    let installed = ctx.collision_install(&scene, &tree, None).unwrap();
    let door_installed = door_mesh.map(|(mesh, bounds)| {
        let dtree = build(Positions::F32(&mesh.positions), &mesh.indices);
        let d = ctx.collision_install(&mesh, &dtree, None).unwrap();
        (d.world, bounds)
    });
    let mut door_place = None;
    ctx.world(|world| {
        crate::units::f3::f3_init_in(world, installed.world, -40., 0).unwrap();
        let mut doors = Vec::new();
        if let Some((door_world, bounds)) = door_installed {
            let door_tree = world.collision.state.worlds[door_world as usize].tree as u32;
            let body = world.collision.body_new(door_tree, 0) as u32;
            world.collision.io_mut()[20..26].copy_from_slice(&bounds);
            world.collision.body_set(body);
            world.collision.enable_body(body, true);
            world
                .collision
                .world_order_slots(installed.world as u32, 1)
                .copy_from_slice(&[body]);
            world.movement.flags_mut()[0] = 1;
            doors.push(body as usize);
            door_place = Some(0usize);
        }
        setup_args(world, nav_json);
        if swbf2 {
            assert_eq!(
                ai_world_begin_swbf2(world, None, &doors, &[]).unwrap(),
                1
            );
        } else {
            assert_eq!(ai_world_begin_in(world), 1);
        }
    })
    .unwrap();
    (ctx, installed.world as usize, door_place)
}

fn spawn_soldier(world: &mut World, collision_world: usize, pos: [f64; 3]) -> (Ref<Unit>, u32) {
    let unit = world.sim.store.unit.insert(Unit {
        alive: true,
        generation: 1.0,
        kind: 0.0,
        nav_radius: Some(0.45),
        soldier: UnitBody::Soldier(Soldier {
            position: pos,
            settled: pos,
            velocity: [0.0; 3],
            max_speed: 6.5,
            max_strafe: 4.5,
            max_jump: 7.5,
            max_turn: 6.0,
            max_pitch: 4.0,
            accel: 70.0,
            grounded: true,
            ..Default::default()
        }),
        ..Default::default()
    });
    let mover = world.movement.new_mover(collision_world, |m| {
        *m = Mover::new(collision_world);
        m.max_speed = 6.5;
        m.max_strafe = 4.5;
        m.max_jump = 7.5;
        m.max_turn = 6.0;
        m.max_pitch = 4.0;
        m.accel = 70.0;
        m.position = V3::new(pos[0], pos[1], pos[2]);
        m.settled = m.position;
        m.grounded = true;
    });
    world.movement.reserve_mover(mover, 256, 16, 512);
    world
        .movement
        .prepare_world_gathers(&world.collision.state, collision_world);
    (unit, mover)
}

fn step_soldier_physics(world: &mut World, unit_ref: Ref<Unit>, mover_id: u32, dt: f64) {
    let input = {
        let row = world.sim.store.unit.get(unit_ref).unwrap();
        let i = &row.intent;
        Input {
            forward: i.forward,
            right: i.right,
            turn: i.turn,
            pitch: i.pitch,
            jump: i.jump,
            crouch: i.crouch,
            prone: i.prone,
            jet_direction: i.jet_direction.map(|v| V3::new(v[0], v[1], v[2])),
        }
    };
    world
        .movement
        .advance(&mut world.collision.state, mover_id, &input, dt);
    let m = world.movement.mover(mover_id);
    let pos = [m.position.x, m.position.y, m.position.z];
    let settled = [m.settled.x, m.settled.y, m.settled.z];
    let vel = [m.velocity.x, m.velocity.y, m.velocity.z];
    let yaw = m.aim_yaw;
    let pitch = m.aim_pitch;
    let grounded = m.grounded;
    let row = world.sim.store.unit.get_mut(unit_ref).unwrap();
    if let UnitBody::Soldier(s) = &mut row.soldier {
        s.position = pos;
        s.settled = settled;
        s.velocity = vel;
        s.aim_yaw = yaw;
        s.aim_pitch = pitch;
        s.grounded = grounded;
    }
}

fn wall_and_corner_mesh() -> (MapCollision, Vec<u8>) {
    let mut mesh = Mesh::new();
    // Main flat deck y = 0.
    mesh.quad([-4., 0., -10.], [28., 0., -10.], [28., 0., 12.], [-4., 0., 12.]);
    // Solid wall at x = 12 blocking direct path from (4, 0, -4) to (20, 0, -4).
    mesh.solid_box(11.8, 0.0, -10.0, 12.2, 4.0, 4.0);
    // Disconnected upper island at (60, 10, 0) with no arcs to hubs 0..3.
    mesh.quad([54., 10., -6.], [66., 10., -6.], [66., 10., 6.], [54., 10., 6.]);
    let hubs = [
        ([4.0, 0.0, -4.0], 4.0),
        ([4.0, 0.0, 8.0], 4.0),
        ([20.0, 0.0, 8.0], 4.0),
        ([20.0, 0.0, -4.0], 4.0),
        ([60.0, 10.0, 0.0], 4.0),
    ];
    // Include authored blocked shortcut (0, 3) through the wall to prove the
    // survey rejects it and routes around via 0 -> 1 -> 2 -> 3.
    let arcs = [(0, 1), (1, 2), (2, 3), (0, 3)];
    (mesh.into_collision(), swbf2_plan_json(&hubs, &arcs))
}

#[test]
fn external_walk_routes_around_wall_with_zero_allocations() {
    let (scene, nav_json) = wall_and_corner_mesh();
    let (ctx, cw, _) = install_world(scene, None, nav_json, true);
    ctx.world(|world| {
        let (unit, mover) = spawn_soldier(world, cw, [4.0, 0.0, -4.0]);
        let target = [20.0, 0.4, -4.0]; // Slightly above floor to exercise support snap.
        world.ai.begin_nav_tick(1);
        let start = world.nav_walk_begin(unit, target);
        assert!(
            matches!(start, NavWalkStatus::Moving | NavWalkStatus::Waiting),
            "expected active route, got {start:?}"
        );

        let dt = 1.0 / 60.0;
        let mut arrived = false;
        let mut max_z = -4.0f64;
        ALLOCATIONS.with(|n| n.set(0));
        COUNTING.with(|on| on.set(true));
        for tick in 2..=600 {
            world.ai.begin_nav_tick(tick);
            // Simulate Core/GoalController writing conflicting intent earlier in the tick.
            {
                let row = world.sim.store.unit.get_mut(unit).unwrap();
                row.intent.forward = -1.0;
                row.intent.crouch = true;
                row.intent.prone = true;
            }
            let status = world.nav_walk_step(unit, dt);
            assert_ne!(status, NavWalkStatus::Failed, "unexpected failure at tick {tick}");
            assert!(
                !world.sim.store.unit.get(unit).unwrap().intent.crouch,
                "external walk must overwrite Core posture"
            );
            assert!(!world.sim.store.unit.get(unit).unwrap().intent.prone);
            if status == NavWalkStatus::Arrived {
                arrived = true;
                break;
            }
            step_soldier_physics(world, unit, mover, dt);
            let p = world.sim.store.unit.get(unit).unwrap().position();
            max_z = max_z.max(p.z);
        }
        COUNTING.with(|on| on.set(false));
        let allocs = ALLOCATIONS.with(|n| n.get());

        assert!(arrived, "soldier should route around wall and arrive");
        assert!(max_z > 4.5, "soldier must detour round the wall corner, max_z={max_z}");
        let end = world.sim.store.unit.get(unit).unwrap().position();
        let dist = v8math::hypot(end.x - 20.0, end.z + 4.0);
        assert!(dist < 1.2, "expected arrival near goal, dist={dist}");
        assert_eq!(allocs, 0, "external walk steady-state stepping must allocate 0 times");
    })
    .unwrap();
}

#[test]
fn external_walk_waits_for_closed_door_round_the_corner() {
    let (scene, nav_json) = wall_and_corner_mesh();
    let mut door = Mesh::new();
    door.solid_box(11.8, 0.0, 4.0, 12.2, 4.0, 12.0);
    let door_bounds = [11.8, 0.0, 4.0, 12.2, 4.0, 12.0];
    let (ctx, cw, door_place) = install_world(
        scene,
        Some((door.into_collision(), door_bounds)),
        nav_json,
        true,
    );
    let door_place = door_place.unwrap();
    ctx.world(|world| {
        let (unit, mover) = spawn_soldier(world, cw, [4.0, 0.0, -4.0]);
        world.ai.begin_nav_tick(1);
        let start = world.nav_walk_begin(unit, [20.0, 0.0, -4.0]);
        assert!(
            matches!(start, NavWalkStatus::Moving | NavWalkStatus::Waiting),
            "surveyed proximity door corridor should plan while shutter is closed, got {start:?}"
        );

        let dt = 1.0 / 60.0;
        let mut waited_ticks = 0usize;
        let mut opened = false;
        let mut arrived = false;
        for tick in 2..=900 {
            world.ai.begin_nav_tick(tick);
            // Simulate conflicting Core intent every tick, including while waiting.
            {
                let row = world.sim.store.unit.get_mut(unit).unwrap();
                row.intent.forward = -1.0;
                row.intent.right = 1.0;
            }
            let status = world.nav_walk_step(unit, dt);
            assert_ne!(status, NavWalkStatus::Failed, "door wait failed at tick {tick}");
            if status == NavWalkStatus::Waiting && !opened {
                let intent = &world.sim.store.unit.get(unit).unwrap().intent;
                assert_eq!(intent.forward, 0.0, "waiting at closed door must zero Core intent");
                assert_eq!(intent.right, 0.0);
                waited_ticks += 1;
                let p = world.sim.store.unit.get(unit).unwrap().position();
                assert!(
                    p.x < 11.8 && p.z > 4.0,
                    "soldier should wait in front of the corner door, pos=({}, {}, {})",
                    p.x, p.y, p.z
                );
                if waited_ticks == 20 {
                    // Open the proximity door shutter after 20 ticks of waiting.
                    world.movement.flags_mut()[door_place] = 0;
                    opened = true;
                }
            } else if status == NavWalkStatus::Arrived {
                arrived = true;
                break;
            }
            step_soldier_physics(world, unit, mover, dt);
        }
        assert!(opened, "soldier should reach and wait at the closed corner door");
        assert!(arrived, "soldier should cross the opened corner door and arrive");
        let end = world.sim.store.unit.get(unit).unwrap().position();
        assert!(end.x > 18.5 && (end.z + 4.0).abs() < 1.5, "ended at {:?}", end);
    })
    .unwrap();
}

#[test]
fn external_walk_reaches_goal_on_another_floor() {
    let mut mesh = Mesh::new();
    // Lower deck y = 0.
    mesh.quad([-4., 0., -6.], [26., 0., -6.], [26., 0., 4.], [-4., 0., 4.]);
    // Walkable ramp from z = 2 (y = 0) up to z = 12 (y = 3.0) at x in [16, 24].
    mesh.quad([16., 0., 2.], [24., 0., 2.], [24., 3., 12.], [16., 3., 12.]);
    // Upper floor y = 3.0.
    mesh.quad([14., 3., 12.], [34., 3., 12.], [34., 3., 20.], [14., 3., 20.]);
    let hubs = [
        ([4.0, 0.0, 0.0], 4.0),
        ([20.0, 0.0, 0.0], 4.0),
        ([20.0, 3.0, 14.0], 4.0),
        ([30.0, 3.0, 14.0], 4.0),
    ];
    let arcs = [(0, 1), (1, 2), (2, 3)];
    let nav_json = swbf2_plan_json(&hubs, &arcs);
    let (ctx, cw, _) = install_world(mesh.into_collision(), None, nav_json, true);
    ctx.world(|world| {
        let (unit, mover) = spawn_soldier(world, cw, [4.0, 0.0, 0.0]);
        world.ai.begin_nav_tick(1);
        let start = world.nav_walk_begin(unit, [30.0, 3.0, 14.0]);
        assert!(
            matches!(start, NavWalkStatus::Moving | NavWalkStatus::Waiting),
            "expected multi-floor route, got {start:?}"
        );
        let dt = 1.0 / 60.0;
        let mut arrived = false;
        for tick in 2..=800 {
            world.ai.begin_nav_tick(tick);
            let status = world.nav_walk_step(unit, dt);
            assert_ne!(status, NavWalkStatus::Failed, "multi-floor walk failed at tick {tick}");
            if status == NavWalkStatus::Arrived {
                arrived = true;
                break;
            }
            step_soldier_physics(world, unit, mover, dt);
        }
        assert!(arrived, "soldier should climb ramp to upper floor and arrive");
        let end = world.sim.store.unit.get(unit).unwrap().position();
        assert!(end.y >= 2.7, "expected soldier on upper floor y=3, got y={}", end.y);
        assert!(v8math::hypot(end.x - 30.0, end.z - 14.0) < 1.2);
    })
    .unwrap();
}

#[test]
fn external_walk_unreachable_fails_with_no_physical_check() {
    let (scene, nav_json) = wall_and_corner_mesh();
    let (ctx, cw, _) = install_world(scene, None, nav_json, true);
    ctx.world(|world| {
        let (u0, _) = spawn_soldier(world, cw, [4.0, 0.0, -4.0]);
        let (u1, _) = spawn_soldier(world, cw, [5.0, 0.0, -4.0]);
        world.ai.begin_nav_tick(1);
        let casts_before = test_cast_count();
        let work_before = world.ai.ground_probe.as_ref().unwrap().work_used();

        // Goal on disconnected island (hub 4): must fail with zero physical checks
        // and without spending a per-tick start credit.
        assert_eq!(
            world.nav_walk_begin(u0, [60.0, 10.0, 0.0]),
            NavWalkStatus::Failed
        );
        assert_eq!(
            test_cast_count(),
            casts_before,
            "unreachable goal must perform 0 raycasts"
        );
        assert_eq!(
            world.ai.ground_probe.as_ref().unwrap().work_used(),
            work_before,
            "unreachable goal must spend 0 GroundProbe work"
        );

        // Both per-tick start credits remain available for reachable targets.
        assert!(matches!(
            world.nav_walk_begin(u0, [20.0, 0.0, -4.0]),
            NavWalkStatus::Moving | NavWalkStatus::Waiting
        ));
        assert!(matches!(
            world.nav_walk_begin(u1, [20.0, 0.0, -4.0]),
            NavWalkStatus::Moving | NavWalkStatus::Waiting
        ));
    })
    .unwrap();
}

#[test]
fn external_walk_per_tick_cap_and_lifecycle_release() {
    let (scene, nav_json) = wall_and_corner_mesh();
    let (ctx, cw, _) = install_world(scene, None, nav_json, true);
    ctx.world(|world| {
        let (u0, _) = spawn_soldier(world, cw, [4.0, 0.0, -4.0]);
        let (u1, _) = spawn_soldier(world, cw, [5.0, 0.0, -4.0]);
        let (u2, _) = spawn_soldier(world, cw, [6.0, 0.0, -4.0]);
        let goal = [20.0, 0.0, -4.0];

        world.ai.begin_nav_tick(1);
        assert!(matches!(
            world.nav_walk_begin(u0, goal),
            NavWalkStatus::Moving | NavWalkStatus::Waiting
        ));
        // Re-requesting the same target within 0.1 m keeps the walk without spending a start.
        assert!(matches!(
            world.nav_walk_begin(u0, [20.05, 0.0, -4.02]),
            NavWalkStatus::Moving | NavWalkStatus::Waiting
        ));
        assert!(matches!(
            world.nav_walk_begin(u1, goal),
            NavWalkStatus::Moving | NavWalkStatus::Waiting
        ));
        // Third distinct walk on the same tick hits the per-tick cap with 0 physical checks.
        let casts_before = test_cast_count();
        let work_before = world.ai.ground_probe.as_ref().unwrap().work_used();
        assert_eq!(world.nav_walk_begin(u2, goal), NavWalkStatus::Failed);
        assert_eq!(test_cast_count(), casts_before);
        assert_eq!(
            world.ai.ground_probe.as_ref().unwrap().work_used(),
            work_before
        );

        // Next tick refreshes the start allowance.
        world.ai.begin_nav_tick(2);
        assert!(matches!(
            world.nav_walk_begin(u2, goal),
            NavWalkStatus::Moving | NavWalkStatus::Waiting
        ));

        // Explicit stop is idempotent and releases the slot.
        world.nav_walk_stop(u0);
        world.nav_walk_stop(u0);
        assert_eq!(world.nav_walk_step(u0, 1.0 / 60.0), NavWalkStatus::Failed);

        // Death auto-releases.
        world.sim.store.unit.get_mut(u1).unwrap().alive = false;
        assert_eq!(world.nav_walk_step(u1, 1.0 / 60.0), NavWalkStatus::Failed);

        // Seating auto-releases.
        world.sim.store.unit.get_mut(u2).unwrap().seat = Some(Ref::new(0, 1));
        assert_eq!(world.nav_walk_step(u2, 1.0 / 60.0), NavWalkStatus::Failed);

        // Generation bump auto-releases.
        world.ai.begin_nav_tick(3);
        assert!(matches!(
            world.nav_walk_begin(u0, goal),
            NavWalkStatus::Moving | NavWalkStatus::Waiting
        ));
        world.sim.store.unit.get_mut(u0).unwrap().generation += 1.0;
        assert_eq!(world.nav_walk_step(u0, 1.0 / 60.0), NavWalkStatus::Failed);
    })
    .unwrap();
}

#[test]
fn external_walk_unreserved_and_unreachable_in_2004() {
    let mut mesh = Mesh::new();
    mesh.quad([-4., 0., -6.], [24., 0., -6.], [24., 0., 6.], [-4., 0., 6.]);
    let (ctx, cw, _) = install_world(mesh.into_collision(), None, Vec::new(), false);
    ctx.world(|world| {
        assert_eq!(
            world
                .ai
                .battle()
                .unwrap()
                .state
                .navigation
                .external_walk_capacity(),
            0,
            "2004 battles must reserve zero external walk slots"
        );
        let (unit, _) = spawn_soldier(world, cw, [4.0, 0.0, 0.0]);
        assert_eq!(
            world.nav_walk_begin(unit, [16.0, 0.0, 0.0]),
            NavWalkStatus::Failed
        );
        assert_eq!(
            world.nav_walk_step(unit, 1.0 / 60.0),
            NavWalkStatus::Failed
        );
        world.nav_walk_stop(unit);
    })
    .unwrap();
}

#[test]
fn swbf2_observation_marks_failed_walk_arrived() {
    let mut mesh = Mesh::new();
    mesh.quad([-4., 0., -6.], [24., 0., -6.], [24., 0., 6.], [-4., 0., 6.]);
    let nav_json = swbf2_plan_json(&[([4.0, 0.0, 0.0], 2.0)], &[]);
    let (ctx, cw, _) = install_world(mesh.into_collision(), None, nav_json, true);
    ctx.world(|world| {
        let (unit, _) = spawn_soldier(world, cw, [4.0, 0.0, 0.0]);
        let handle = crate::gen::AiLifeHandle {
            kind: crate::gen::KIND_UNIT.into(),
            slot: unit.slot,
            generation: unit.gen,
            life: 1.0,
        };
        struct Env;
        impl crate::ai::owned::navigation::Environment for Env {
            fn time(&self) -> f64 { 1.0 }
            fn remembered(&mut self, _: crate::ai::owned::navigation::Actor) -> Option<crate::ai::nav::math::Vec3> { None }
            fn clear_above(&mut self, _: &mut World, _: crate::ai::nav::math::Vec3, _: &crate::gen::AiLifeHandle) -> bool { true }
            fn follow_far(&mut self) -> f64 { 15.0 }
            fn seat_frame(&mut self, _: &mut World, _: crate::vehicles::f2_seats::HullLife, _: usize) -> Option<(crate::ai::nav::math::Vec3, crate::ai::nav::math::Vec3)> { None }
        }
        let mut random = crate::ai::phase::RandomState::new(7);
        let mut env = Env;
        world.ai.begin_nav_tick(1);
        let mut battle = world.ai.take_battle().expect("swbf2 battle");
        let class = battle.state.navigation.class(Default::default());
        let actor = battle
            .state
            .navigation
            .register(&world.sim.store, handle, class)
            .expect("register actor");
        let key = battle
            .state
            .navigation
            .helper_key(actor)
            .expect("helper key");
        let obs = battle
            .state
            .navigation
            .combat_approach(
                world,
                &mut random,
                &mut env,
                key,
                crate::ai::nav::math::Vec3::new(60.0, 10.0, 0.0),
            )
            .expect("failed walk observation");
        assert!(obs.failed, "unreachable route must report failed=true");
        assert!(
            obs.arrived,
            "failed SWBF2 route must report arrived=true so melee/snipe/mine retry instead of stalling"
        );
        world.ai.return_battle(battle);
    })
    .unwrap();
}
