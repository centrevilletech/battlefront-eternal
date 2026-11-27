//! Zero-allocation steady-state proof for 2005 AI navigation with multiple bots
//! routing simultaneously, executing jump/jet-jump arcs, running `route_distance`
//! queries, toggling `plnp` dynamic arc groups, and toggling barriers.

use bfe_content::{json, zero_hash};
use bfe_sim::ai::nav::goto::{GotoMode, GotoWorld, Resolved};
use bfe_sim::ai::nav::math::Vec3;
use bfe_sim::ai::nav::route::MapNavigation;
use bfe_sim::ai::nav::swbf2::{ProbeKind, Swbf2GotoWorld, Swbf2GroundGoto};
use bfe_sim::ai::nav::units::{kind, ClassDef, JetView, NavUnit, SizeType, SoldierView, UnitRef};
use bfe_sim::ai::nav::PlannerOptions;
use bfe_sim::ai::random::{ClassicRandom, Random};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

#[global_allocator]
static GLOBAL: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

struct GridWorld {
    time: f64,
    ra: ClassicRandom,
    rb: ClassicRandom,
    /// The tick has no work left for what can wait.
    busy: bool,
}

impl GridWorld {
    fn new(seed: u32) -> Self {
        Self {
            time: 0.0,
            ra: ClassicRandom::new(f64::from(seed)),
            rb: ClassicRandom::new(f64::from(seed ^ 0x9e37_79b9)),
            busy: false,
        }
    }

    /// Two invented interior building walls so bots must route around corners.
    fn segment_clear(&self, a: Vec3, b: Vec3) -> bool {
        let walls = [[15.0, -5.0, 15.0, 25.0], [35.0, 15.0, 35.0, 55.0]];
        for [x0, z0, x1, z1] in walls {
            let o1 = (b.x - a.x) * (z0 - a.z) - (b.z - a.z) * (x0 - a.x);
            let o2 = (b.x - a.x) * (z1 - a.z) - (b.z - a.z) * (x1 - a.x);
            let o3 = (x1 - x0) * (a.z - z0) - (z1 - z0) * (a.x - x0);
            let o4 = (x1 - x0) * (b.z - z0) - (z1 - z0) * (b.x - x0);
            if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
                return false;
            }
        }
        true
    }
}

impl GotoWorld for GridWorld {
    fn time(&self) -> f64 {
        self.time
    }
    fn random_a(&mut self) -> &mut dyn Random {
        &mut self.ra
    }
    fn random_b(&mut self) -> &mut dyn Random {
        &mut self.rb
    }
    fn resolve(&mut self, _r: UnitRef) -> Option<Resolved> {
        None
    }
    fn leader(&mut self, _r: UnitRef) -> Option<NavUnit> {
        None
    }
    fn remembered(&mut self, _target: u64) -> Option<Vec3> {
        None
    }
    fn clear_above(&mut self, _from: Vec3, _unit: &NavUnit) -> bool {
        true
    }
    fn follow_far(&mut self) -> f64 {
        15.0
    }
}

impl Swbf2GotoWorld for GridWorld {
    fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld {
        self
    }
    fn line_of_walk(&mut self, from: Vec3, to: Vec3, _unit: &NavUnit) -> bool {
        self.segment_clear(from, to)
    }
    fn try_crosses(
        &mut self,
        from: Vec3,
        to: Vec3,
        _unit: &NavUnit,
        _movement: u8,
        kind: ProbeKind,
    ) -> Option<bool> {
        let waits = matches!(kind, ProbeKind::Plan | ProbeKind::Step | ProbeKind::Leg);
        (!(self.busy && waits)).then(|| self.segment_clear(from, to))
    }
}

fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// Build an invented 4x4 grid (16 hubs, 24 arcs) with barriers, hints, and one
/// `plnp` dynamic group covering a central street arc.
fn build_invented_grid_nav() -> MapNavigation {
    let mut hubs_json = Vec::new();
    let mut arcs_json = Vec::new();
    let mut hub_slots: Vec<Vec<usize>> = vec![Vec::new(); 16];

    // Horizontal and vertical arcs across the 4x4 grid (20m spacing).
    let mut arc_id = 0usize;
    let mut central_arc_name = String::new();
    for row in 0..4usize {
        for col in 0..4usize {
            let u = row * 4 + col;
            if col + 1 < 4 {
                let v = row * 4 + col + 1;
                let name = format!("arc_h_{row}_{col}");
                if row == 1 && col == 1 {
                    central_arc_name = name.clone();
                }
                let jump = row == 2 && col == 1;
                let jet = row == 0 && col == 2;
                arcs_json.push(format!(
                    r#"{{"name":"{name}","start":{u},"end":{v},"filterFlags":31,"jump":{jump},"jetJump":{jet}}}"#
                ));
                hub_slots[u].push(arc_id);
                hub_slots[v].push(arc_id);
                arc_id += 1;
            }
            if row + 1 < 4 {
                let v = (row + 1) * 4 + col;
                let name = format!("arc_v_{row}_{col}");
                arcs_json.push(format!(
                    r#"{{"name":"{name}","start":{u},"end":{v},"filterFlags":31,"typeFlags":0}}"#
                ));
                hub_slots[u].push(arc_id);
                hub_slots[v].push(arc_id);
                arc_id += 1;
            }
        }
    }

    for (idx, slots) in hub_slots.iter().enumerate() {
        let row = idx / 4;
        let col = idx % 4;
        let x = (col as f64) * 20.0;
        let z = (row as f64) * 20.0;
        let mut s = [255usize; 8];
        for (k, &a) in slots.iter().take(8).enumerate() {
            s[k] = a;
        }
        hubs_json.push(format!(
            r#"{{"name":"hub_{idx}","position":[{x},0,{z}],"radius":4,"slots":[{},{},{},{},{},{},{},{}],"weightCounts":[0,0,0,0,0],"weights":[]}}"#,
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]
        ));
    }

    // Minimal valid plnp delta blocks (0 changes per hub in both states; graph
    // still toggles `blocked_by_group` and rebuilds APSP in-place).
    let raw_empty = vec![0u8; 16];
    let compressed = zlib_compress(&raw_empty);
    let mut bin = Vec::new();
    let mut blocks = Vec::new();
    for _ in 0..64 {
        blocks.push(format!(
            r#"{{"offset":{},"size":{},"unpackedSize":{}}}"#,
            bin.len(),
            compressed.len(),
            raw_empty.len()
        ));
        bin.extend_from_slice(&compressed);
    }
    let central_hash = format!("{:08x}", zero_hash::fnv1a(&central_arc_name));

    let payload = format!(
        r#"{{
            "schema": 2,
            "map": "invented_grid_32bots",
            "plan": {{
                "branchCount": 5,
                "hubs": [{}],
                "arcs": [{}]
            }},
            "planningGroups": {{
                "groupCounts": [0,0,0,0,0,1],
                "connectionHashes": ["{central_hash}"],
                "blocks": [{}]
            }},
            "barriers": [
                {{"sourceIndex":0,"enabled":true,"name":"mid_gate","xfrm":[1,0,0, 0,1,0, 0,0,1, 30,0,30],"size":[1,4,4],"flags":31}}
            ],
            "hints": [
                {{"sourceIndex":0,"name":"cover0","type":"2","xfrm":[1,0,0, 0,1,0, 0,0,1, 10,0,10],"properties":[]}},
                {{"sourceIndex":1,"name":"extra7","type":"7","xfrm":[1,0,0, 0,1,0, 0,0,1, 50,0,50],"properties":[]}}
            ]
        }}"#,
        hubs_json.join(","),
        arcs_json.join(","),
        blocks.join(","),
    );

    let parsed = json::parse_bytes(payload.as_bytes()).unwrap();
    MapNavigation::new_swbf2(&parsed, Some(&bin), None, PlannerOptions::default()).unwrap()
}

fn make_bot(idx: usize) -> NavUnit {
    let row = idx % 4;
    let col = (idx / 4) % 4;
    let pos = Vec3::new((col as f64) * 20.0 + 1.0, 0.0, (row as f64) * 20.0 + 1.0);
    let jet = idx % 4 == 0;
    let mut u = NavUnit::new((idx + 1) as u64, kind::SOLDIER, pos);
    u.generation = Some(1.0);
    u.nav_radius = Some(0.5);
    u.class_def = Some(ClassDef {
        ai_size_type: SizeType::Number(1.0),
        max_speed: Some(6.0),
        max_strafe_speed: Some(4.0),
        max_turn_speed: Some(6.0),
        max_pitch_speed: Some(4.0),
        jet_fuel_min_border: if jet { Some(0.1) } else { None },
        jet_fuel_recharge_rate: if jet { Some(0.2) } else { None },
        ..ClassDef::default()
    });
    u.soldier = Some(SoldierView {
        aim_yaw: Some(0.0),
        aim_pitch: Some(0.0),
        max_speed: Some(6.0),
        max_strafe: Some(4.0),
        max_turn: Some(6.0),
        max_pitch: Some(4.0),
        fuel: if jet { 1.0 } else { 0.0 },
        jet: if jet {
            Some(JetView {
                min_border: Some(0.1),
                recharge: Some(0.2),
            })
        } else {
            None
        },
        ..SoldierView::default()
    });
    u
}

#[test]
fn routing_and_large_group_toggles_do_not_allocate() {
    run::<32>();
    run::<64>();
}

fn run<const BOTS: usize>() {
    let mut nav = build_invented_grid_nav();
    nav.prepare_owned();
    nav.controls.reserve(BOTS);
    nav.store.reserve_paths(BOTS * 3 + 8);
    nav.swbf2.as_mut().unwrap().reserve_controls(BOTS);
    nav.swbf2.as_mut().unwrap().prepare_clearance();

    let mut helpers = Vec::with_capacity(BOTS);
    let mut units = Vec::with_capacity(BOTS);
    let mut goals = Vec::with_capacity(BOTS);
    for i in 0..BOTS {
        let ctrl = nav.new_control();
        helpers.push(Swbf2GroundGoto::new(ctrl));
        units.push(make_bot(i));
        let target_hub = (15 - (i % 16)) as usize;
        let trow = target_hub / 4;
        let tcol = target_hub % 4;
        goals.push(Vec3::new((tcol as f64) * 20.0, 0.0, (trow as f64) * 20.0));
    }

    let mut world = GridWorld::new(42);
    let mut hint_buf: Vec<usize> = Vec::with_capacity(16);
    let base_order: [u32; 4] = [0, 1, 2, 3];
    let base_flags: [u8; 4] = [1, 1, 1, 1];
    let mut live_order: [u32; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    let mut live_flags: [u8; 8] = [1, 1, 1, 1, 1, 1, 0, 1];
    assert!(nav.observe_collision_classified(
        (1, 1),
        &base_order,
        &base_flags,
        |b| b >= 4,
        |_| Some([0.0, -1.0, 0.0, 2.0, 2.0, 2.0]),
    ));

    // Issue initial routes for all 32 bots and warm up 1 tick.
    for i in 0..BOTS {
        nav.swbf2_ground_goto(
            &mut helpers[i],
            &mut units[i],
            &mut world,
            goals[i],
            GotoMode::Route,
        );
    }
    nav.service(1.0);

    // Now count allocations across 120 full simulation ticks with all 32 bots
    // routing, repathing on arrival, running route_distance queries, hint
    // queries, dynamic plnp group toggles, and barrier toggles.
    let before = bfe_test_alloc::thread_allocations();

    let dt = 1.0 / 60.0;
    let mut total_routes_completed = 0usize;
    let mut total_distance_queries = 0usize;
    let mut waits = 0usize;

    for tick in 0..120usize {
        nav.swbf2.as_mut().unwrap().begin_tick(tick as u32);
        world.time += dt;
        world.busy = tick % 3 == 0;
        if tick == 1 {
            // Appending moving bodies after admission must neither allocate nor reset floors.
            assert!(nav.observe_collision_classified(
                (2, 1),
                &live_order,
                &live_flags,
                |b| b >= 4,
                |i| Some([f64::from(i as u32) * 5.0, -1.0, 0.0, f64::from(i as u32) * 5.0 + 2.0, 2.0, 2.0]),
            ));
        } else if tick % 10 == 0 {
            let slot = 4 + (tick / 10) % 4;
            live_flags[slot] ^= 1;
            live_order[7] = 7;
            assert!(nav.observe_collision_classified(
                (2, 1),
                &live_order,
                &live_flags,
                |b| b >= 4,
                |i| Some([f64::from(i as u32) * 5.0, -1.0, 0.0, f64::from(i as u32) * 5.0 + 2.0, 2.0, 2.0]),
            ));
        }
        for i in 0..BOTS {
            nav.swbf2_ground_resume(&mut helpers[i], &mut units[i], &mut world);
        }
        if tick == 40 {
            nav.block_planning_arcs("arc_h_1_1");
            nav.toggle_barriers("mid_gate");
        } else if tick == 80 {
            nav.unblock_planning_arcs("arc_h_1_1");
            nav.toggle_barriers("mid_gate");
        }

        for i in 0..BOTS {
            nav.swbf2_ground_tick(&mut helpers[i], &mut units[i], &mut world, dt);
            let _intent = nav.swbf2_navigation_input(
                helpers[i].control,
                &mut units[i],
                dt,
                None,
                true,
                &mut world,
            );
            let req = nav.controls[helpers[i].control].request;
            let dx = req.steering_target.x - units[i].position.x;
            let dz = req.steering_target.z - units[i].position.z;
            let len = v8math::hypot(dx, dz);
            if len > 1e-4 {
                let step = (6.0 * dt).min(len);
                units[i].position.x += dx / len * step;
                units[i].position.z += dz / len * step;
            }
            // Every soldier gets a new order once in a tick without work.
            if tick == 30 || nav.swbf2_ground_arrived(&helpers[i]) || helpers[i].failed {
                total_routes_completed += usize::from(nav.swbf2_ground_arrived(&helpers[i]));
                let next_hub = (i + tick) % 16;
                let next_goal = Vec3::new(
                    ((next_hub % 4) as f64) * 20.0,
                    0.0,
                    ((next_hub / 4) as f64) * 20.0,
                );
                goals[i] = next_goal;
                nav.swbf2_ground_goto(
                    &mut helpers[i],
                    &mut units[i],
                    &mut world,
                    next_goal,
                    GotoMode::Route,
                );
                waits += usize::from(!helpers[i].failed && helpers[i].cursor.is_none());
            }
            // Exercise cross-family route_distance query per bot per tick.
            let q = nav.route_distance(units[i].position, goals[i], 0.5, 1, i % 4 == 0, |a, b| {
                world.segment_clear(a, b)
            });
            if i >= 4 {
                assert_eq!(q.status, bfe_sim::ai::nav::route::RouteStatus::Unknown);
            } else if q.reachable {
                total_distance_queries += 1;
            }
        }
        nav.service(1.0);
        nav.query_hints_into(
            0xff,
            Vec3::new(20.0, 0.0, 20.0),
            40.0,
            8.0,
            None,
            world.time,
            &mut hint_buf,
        );
    }

    let allocs = bfe_test_alloc::thread_allocations().wrapping_sub(before);
    assert!(
        total_distance_queries > 0,
        "expected reachable route_distance queries"
    );
    assert!(waits > 0, "expected orders that wait for work");
    assert_eq!(
        allocs, 0,
        "2005 navigation allocated {allocs} times during 120 ticks with {BOTS} routing bots (routes completed: {total_routes_completed})"
    );
}
