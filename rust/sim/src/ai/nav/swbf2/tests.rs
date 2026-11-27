//! Unit tests for 2005 navigation loading, validation, dynamic `plnp` arc
//! groups, barrier toggles, 2005 hint types, boundary polygons, collision-aware
//! routing, jump/jet-jump arc execution, stuck recovery, and deterministic
//! reachability/distance queries. All fixtures in this file are invented.

use crate::ai::nav::route::MapNavigation;
use super::super::goto::{GotoMode, GotoWorld, Resolved};
use super::super::math::Vec3;
use super::super::units::{kind, Air, ClassDef, JetView, NavUnit, SizeType, SoldierView, UnitRef};
use super::super::{Nav, PlannerOptions};
use super::goto::{Swbf2GotoWorld, Swbf2GroundGoto};
use super::graph::{RouteQuery, ARC_JET_JUMP, ARC_JUMP, ARC_ONE_WAY};
use super::ProbeKind;
use super::load::{boundary_edges_from_swbf2, HINT_BIT_TYPE7, HINT_BIT_TYPE8};
use crate::ai::random::{ClassicRandom, Random};
use bfe_content::{json, zero_hash};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

/// Optional synthetic rooftop/street surface for local-segment drop tests.
#[derive(Clone, Copy)]
struct RooftopSpec {
    roof_half: f64,
    street_edge: f64,
    roof_y: f64,
    allow_drop: bool,
    allow_jump: bool,
}

/// Deterministic test harness world with optional horizontal wall segments
/// (blocking `line_of_walk`).
struct TestWorld {
    time: f64,
    ra: ClassicRandom,
    rb: ClassicRandom,
    /// Horizontal wall segments `[x0, z0, x1, z1]` that block walking.
    walls: Vec<[f64; 4]>,
    flat: bool,
    drives: usize,
    /// Kinds of checks that find no work left in this tick.
    no_work: Vec<ProbeKind>,
    rooftop: Option<RooftopSpec>,
    /// Minimum `from.y` required for `clear_above` to report open sky.
    min_clear_above_y: f64,
}

impl TestWorld {
    fn new(seed: u32) -> Self {
        Self {
            time: 0.0,
            ra: ClassicRandom::new(f64::from(seed)),
            rb: ClassicRandom::new(f64::from(seed ^ 0x9e37_79b9)),
            walls: Vec::new(),
            flat: false,
            drives: 0,
            no_work: Vec::new(),
            rooftop: None,
            min_clear_above_y: f64::NEG_INFINITY,
        }
    }

    fn with_rooftop(mut self, spec: RooftopSpec) -> Self {
        self.rooftop = Some(spec);
        self
    }

    fn with_wall(mut self, x0: f64, z0: f64, x1: f64, z1: f64) -> Self {
        self.walls.push([x0, z0, x1, z1]);
        self
    }

    fn segment_clear(&self, a: Vec3, b: Vec3) -> bool {
        for &[x0, z0, x1, z1] in &self.walls {
            if segments_intersect_2d(a.x, a.z, b.x, b.z, x0, z0, x1, z1) {
                return false;
            }
        }
        true
    }
}

fn orient_2d(ax: f64, az: f64, bx: f64, bz: f64, cx: f64, cz: f64) -> f64 {
    (bx - ax) * (cz - az) - (bz - az) * (cx - ax)
}

#[allow(clippy::too_many_arguments)]
fn segments_intersect_2d(
    ax: f64,
    az: f64,
    bx: f64,
    bz: f64,
    cx: f64,
    cz: f64,
    dx: f64,
    dz: f64,
) -> bool {
    let o1 = orient_2d(ax, az, bx, bz, cx, cz);
    let o2 = orient_2d(ax, az, bx, bz, dx, dz);
    let o3 = orient_2d(cx, cz, dx, dz, ax, az);
    let o4 = orient_2d(cx, cz, dx, dz, bx, bz);
    let overlaps = |a, b, c, d| v8math::min(a, b) <= v8math::max(c, d) && v8math::min(c, d) <= v8math::max(a, b);
    overlaps(ax,bx,cx,dx) && overlaps(az,bz,cz,dz) && o1 * o2 <= 0. && o3 * o4 <= 0.
}

impl GotoWorld for TestWorld {
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
    fn clear_above(&mut self, from: Vec3, _unit: &NavUnit) -> bool {
        from.y >= self.min_clear_above_y
    }
    fn follow_far(&mut self) -> f64 {
        15.0
    }
}

impl Swbf2GotoWorld for TestWorld {
    fn foot(&mut self, p: Vec3) -> Option<Vec3> {
        if let Some(r) = self.rooftop {
            let on_roof = p.x.abs() <= r.roof_half && p.z.abs() <= r.roof_half;
            let on_street = p.x.abs() > r.street_edge || p.z.abs() > r.street_edge;
            return if on_roof && (p.y - r.roof_y).abs() <= 1.25 {
                Some(Vec3::new(p.x, r.roof_y, p.z))
            } else if on_street && p.y.abs() <= 1.25 {
                Some(Vec3::new(p.x, 0., p.z))
            } else {
                None
            };
        }
        if self.flat {
            (-0.32..=1.25).contains(&p.y).then_some(Vec3::new(p.x, 0., p.z))
        } else { Some(p) }
    }
    fn step_floor(&mut self, p: Vec3) -> Option<Vec3> {
        if let Some(r) = self.rooftop {
            let on_roof = p.x.abs() <= r.roof_half && p.z.abs() <= r.roof_half;
            return (on_roof && (p.y - r.roof_y).abs() <= 0.75)
                .then_some(Vec3::new(p.x, r.roof_y, p.z));
        }
        self.foot(p)
    }
    fn combat_floor(&mut self, p: Vec3) -> Option<Vec3> {
        self.foot(p)
    }
    fn approach_floor(&mut self, p: Vec3) -> Option<Vec3> {
        if let Some(r) = self.rooftop {
            return self.foot(p).or_else(|| {
                ((p.x.abs() > r.street_edge || p.z.abs() > r.street_edge)
                    && (0. ..=8.0).contains(&p.y))
                .then_some(Vec3::new(p.x, 0., p.z))
            });
        }
        self.foot(p)
    }
    fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld {
        self
    }
    fn crosses(&mut self, from: Vec3, to: Vec3, unit: &NavUnit, movement: u8) -> bool {
        if let Some(r) = self.rooftop {
            let drop_h = from.y - to.y;
            if movement == super::walk::DROP {
                return r.allow_drop && (0. ..=super::walk::MAX_DROP).contains(&drop_h);
            }
            if movement & ARC_JUMP != 0 {
                return r.allow_jump && (0.75..=super::walk::MAX_DROP).contains(&drop_h);
            }
            return drop_h.abs() <= 0.75 && self.segment_clear(from, to);
        }
        self.line_of_walk(from, to, unit)
    }
    fn line_of_walk(&mut self, from: Vec3, to: Vec3, _unit: &NavUnit) -> bool {
        self.drives += 1;
        if self.rooftop.is_some() {
            return (from.y - to.y).abs() <= 0.75 && self.segment_clear(from, to);
        }
        self.segment_clear(from, to)
    }
    fn try_crosses(
        &mut self,
        from: Vec3,
        to: Vec3,
        unit: &NavUnit,
        movement: u8,
        kind: ProbeKind,
    ) -> Option<bool> {
        (!self.no_work.contains(&kind)).then(|| self.crosses(from, to, unit, movement))
    }
}

fn make_soldier(id: u64, pos: Vec3, jetpack: bool) -> NavUnit {
    let mut u = NavUnit::new(id, kind::SOLDIER, pos);
    u.generation = Some(1.0);
    u.nav_radius = Some(0.5);
    u.class_def = Some(ClassDef {
        ai_size_type: SizeType::Number(1.0),
        max_speed: Some(6.0),
        max_strafe_speed: Some(4.0),
        max_turn_speed: Some(6.0),
        max_pitch_speed: Some(4.0),
        jet_fuel_min_border: if jetpack { Some(0.1) } else { None },
        jet_fuel_recharge_rate: if jetpack { Some(0.2) } else { None },
        ..ClassDef::default()
    });
    u.soldier = Some(SoldierView {
        aim_yaw: Some(0.0),
        aim_pitch: Some(0.0),
        max_speed: Some(6.0),
        max_strafe: Some(4.0),
        max_turn: Some(6.0),
        max_pitch: Some(4.0),
        fuel: if jetpack { 1.0 } else { 0.0 },
        jet: if jetpack {
            Some(JetView { min_border: Some(0.1), recharge: Some(0.2) })
        } else {
            None
        },
        ..SoldierView::default()
    });
    u
}

fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

fn pack_byte(slot: u8, weight: u8) -> u8 {
    ((weight & 31) << 3) | (slot & 7)
}

/// Build an invented 4-hub diamond graph with a `plnp` group blocking the top
/// arc (`arc_top`: hub 0 <-> hub 1, shorter than bottom hub 0 <-> hub 2 <-> hub 3).
fn diamond_plnp_nav() -> MapNavigation {
    // Hub 0 at (0,0,0), Hub 1 at (20,0,10), Hub 2 at (20,0,-20), Hub 3 at (40,0,0).
    // Arcs:
    //  0: "arc_top_a" (0 <-> 1, ~22.36 m) -- in plnp group 0
    //  1: "arc_top_b" (1 <-> 3, ~22.36 m)
    //  2: "arc_bot_a" (0 <-> 2, ~28.28 m)
    //  3: "arc_bot_b" (2 <-> 3, ~28.28 m)
    let top_hash = format!("{:08x}", zero_hash::fnv1a("arc_top_a"));

    // Base weights for hub 0 (table 0, weightCount = 1, 4 destinations):
    // dest 0 -> 0, dest 1 -> slot 0 (arc 0, w=31), dest 2 -> slot 1 (arc 2, w=31), dest 3 -> slot 0 (arc 0, w=31).
    let base_h0_t0 = [0u8, pack_byte(0, 31), pack_byte(1, 31), pack_byte(0, 31)];
    // When group 0 is blocked, hub 0 -> dest 3 switches from slot 0 (arc 0) to slot 1 (arc 2),
    // and hub 0 -> dest 1 switches from slot 0 (arc 0) to slot 1 (arc 2).
    let blocked_h0_t0 = [0u8, pack_byte(1, 31), pack_byte(1, 31), pack_byte(1, 31)];
    let xor_h0_t0: Vec<u8> = base_h0_t0
        .iter()
        .zip(blocked_h0_t0.iter())
        .map(|(a, b)| a ^ b)
        .collect();

    // Block 0 = all zeros (no groups blocked), Block 1 = group 0 blocked.
    let block0_raw = vec![0u8, 0, 0, 0];
    let mut block1_raw = Vec::new();
    // Hub 0 changed = 1: 5 tables, table 0 has 1 row (4 bytes), tables 1..4 have 0 rows.
    block1_raw.push(1);
    block1_raw.push(1);
    block1_raw.extend_from_slice(&xor_h0_t0);
    block1_raw.extend_from_slice(&[0, 0, 0, 0]);
    // Hubs 1, 2, 3 unchanged.
    block1_raw.extend_from_slice(&[0, 0, 0]);

    let c0 = zlib_compress(&block0_raw);
    let c1 = zlib_compress(&block1_raw);
    let mut bin = Vec::new();
    let off0 = 0usize;
    bin.extend_from_slice(&c0);
    let off1 = bin.len();
    bin.extend_from_slice(&c1);

    let payload = format!(
        r#"{{
            "schema": 2,
            "map": "invented_diamond",
            "plan": {{
                "branchCount": 5,
                "hubs": [
                    {{"name":"h0","position":[0,0,0],"radius":4,"slots":[0,2,255,255,255,255,255,255],"weightCounts":[1,0,0,0,0],"weights":[{},{},{},{}]}},
                    {{"name":"h1","position":[20,0,10],"radius":4,"slots":[0,1,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]}},
                    {{"name":"h2","position":[20,0,-20],"radius":4,"slots":[2,3,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]}},
                    {{"name":"h3","position":[40,0,0],"radius":4,"slots":[1,3,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]}}
                ],
                "arcs": [
                    {{"name":"arc_top_a","start":0,"end":1,"filterFlags":1,"typeFlags":0}},
                    {{"name":"arc_top_b","start":1,"end":3,"filterFlags":1,"typeFlags":0}},
                    {{"name":"arc_bot_a","start":0,"end":2,"filterFlags":1,"typeFlags":0}},
                    {{"name":"arc_bot_b","start":2,"end":3,"filterFlags":1,"typeFlags":0}}
                ]
            }},
            "planningGroups": {{
                "groupCounts": [1],
                "connectionHashes": ["{top_hash}"],
                "blocks": [
                    {{"offset":{off0},"size":{},"unpackedSize":{}}},
                    {{"offset":{off1},"size":{},"unpackedSize":{}}}
                ]
            }}
        }}"#,
        base_h0_t0[0],
        base_h0_t0[1],
        base_h0_t0[2],
        base_h0_t0[3],
        c0.len(),
        block0_raw.len(),
        c1.len(),
        block1_raw.len(),
    );
    let parsed = json::parse_bytes(payload.as_bytes()).unwrap();
    MapNavigation::new_swbf2(&parsed, Some(&bin), None, PlannerOptions::default()).unwrap()
}

#[test]
fn validates_malformed_graph_without_panicking() {
    let payload = r#"{
        "schema": 2,
        "map": "invented_broken",
        "plan": {
            "branchCount": 5,
            "hubs": [
                {"name":"ok0","position":[0,0,0],"radius":3,"slots":[0,99,255,255,255,255,255,255],"weightCounts":[1,0,0,0,0],"weights":[1]},
                {"name":"ok1","position":[10,0,0],"radius":3,"slots":[0,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"nan_hub","position":[null,0,0],"radius":-2,"slots":[255,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"island","position":[100,0,0],"radius":3,"slots":[3,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"island2","position":[110,0,0],"radius":3,"slots":[3,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]}
            ],
            "arcs": [
                {"name":"a0","start":0,"end":1,"filterFlags":1,"typeFlags":0},
                {"name":"dangling","start":0,"end":99,"filterFlags":1,"typeFlags":0},
                {"name":"zero_len","start":0,"end":0,"filterFlags":1,"typeFlags":0},
                {"name":"island_arc","start":3,"end":4,"filterFlags":1,"typeFlags":0}
            ]
        },
        "planningGroups": {
            "groupCounts": [1],
            "connectionHashes": ["deadbeef"],
            "blocks": [
                {"offset":0,"size":10,"unpackedSize":10},
                {"offset":999,"size":10,"unpackedSize":10}
            ]
        }
    }"#;
    let parsed = json::parse_bytes(payload.as_bytes()).unwrap();
    let nav = MapNavigation::new_swbf2(&parsed, Some(b"not-valid-zlib"), None, PlannerOptions::default()).unwrap();
    let g = nav.swbf2.as_ref().unwrap().graph.as_ref().unwrap();
    assert_eq!(g.validation.dangling_arcs, vec![1]);
    assert_eq!(g.validation.zero_length_arcs, vec![2]);
    assert_eq!(g.validation.bad_hub_slots, vec![(0, 1)]);
    assert_eq!(g.validation.non_finite_hubs, vec![2]);
    assert_eq!(g.validation.weight_mismatches, vec![0]);
    assert_eq!(g.validation.bad_delta_blocks, vec![0, 1]);
    // Two separate soldier components: {0,1} and {3,4}.
    assert_eq!(g.validation.components_by_table[0].len(), 2);
    assert_eq!(g.validation.isolated_hubs_by_table[0], 2);
    assert!(!g.hub_route_distance(0, 3, 1, false).reachable);
    assert!(g.hub_route_distance(0, 1, 1, false).reachable);
}

#[test]
fn plnp_xor_delta_and_manual_arc_blocking_round_trip() {
    let mut nav = diamond_plnp_nav();
    let mut rng = ClassicRandom::new(7.0);

    // Initially, shortest and weighted route 0 -> 3 uses arc 0 (top, ~44.72 m).
    let d_open = nav.swbf2.as_ref().unwrap().graph.as_ref().unwrap().hub_route_distance(0, 3, 1, false);
    assert!(d_open.reachable);
    assert_eq!(d_open.hops, 2);
    assert!(d_open.distance < 46.0, "expected short top path, got {}", d_open.distance);
    let first_open = nav
        .swbf2
        .as_mut()
        .unwrap()
        .graph
        .as_mut()
        .unwrap()
        .next_arc(None, None, 0, 3, 1, false, false, &mut rng);
    assert_eq!(first_open, Some(0));

    // Block via plnp group member name (case-insensitive).
    assert!(nav.block_planning_arcs("ARC_TOP_A"));
    let d_blocked = nav.swbf2.as_ref().unwrap().graph.as_ref().unwrap().hub_route_distance(0, 3, 1, false);
    assert!(d_blocked.reachable);
    assert_eq!(d_blocked.hops, 2);
    assert!(d_blocked.distance > 55.0, "expected bottom detour ~56.57m, got {}", d_blocked.distance);
    let first_blocked = nav
        .swbf2
        .as_mut()
        .unwrap()
        .graph
        .as_mut()
        .unwrap()
        .next_arc(None, None, 0, 3, 1, false, false, &mut rng);
    assert_eq!(first_blocked, Some(2));

    // Unblock restores original XOR weights and shortest path.
    assert!(nav.unblock_planning_arcs("arc_top_a"));
    let d_restored = nav.swbf2.as_ref().unwrap().graph.as_ref().unwrap().hub_route_distance(0, 3, 1, false);
    assert_eq!(d_restored.distance.to_bits(), d_open.distance.to_bits());
    let first_restored = nav
        .swbf2
        .as_mut()
        .unwrap()
        .graph
        .as_mut()
        .unwrap()
        .next_arc(None, None, 0, 3, 1, false, false, &mut rng);
    assert_eq!(first_restored, Some(0));

    // Toggle blocks again; blocking manual bottom arc makes 0 -> 3 unreachable.
    assert!(nav.toggle_planning_arcs("arc_top_a"));
    assert!(nav.block_planning_arcs("arc_bot_a"));
    assert!(!nav.swbf2.as_ref().unwrap().graph.as_ref().unwrap().hub_route_distance(0, 3, 1, false).reachable);
    assert!(nav.unblock_planning_arcs("arc_bot_a"));
    assert!(nav.swbf2.as_ref().unwrap().graph.as_ref().unwrap().hub_route_distance(0, 3, 1, false).reachable);
}

#[test]
fn arc_flags_one_way_jump_jet_jump_and_filter_mask() {
    let payload = r#"{
        "schema": 2,
        "map": "invented_flags",
        "plan": {
            "branchCount": 5,
            "hubs": [
                {"name":"h0","position":[0,-73,0],"radius":4,"slots":[0,1,2,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"h1","position":[15,-73,0],"radius":4,"slots":[0,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"h2","position":[0,-73,15],"radius":4,"slots":[1,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"h3","position":[0,-73,-15],"radius":4,"slots":[2,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]}
            ],
            "arcs": [
                {"name":"oneway_jump","start":0,"end":1,"filterFlags":1,"oneWay":true,"jump":true},
                {"name":"jet_bridge","start":0,"end":2,"filterFlags":1,"jetJump":true},
                {"name":"hover_only","start":0,"end":3,"filterFlags":4,"typeFlags":0}
            ]
        }
    }"#;
    let parsed = json::parse_bytes(payload.as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&parsed, None, None, PlannerOptions::default()).unwrap();
    // The planning plane and the supported deck have independent heights.
    struct Deck;
    impl super::walk::Survey for Deck {
        fn floors(&mut self, h: &super::Swbf2Hub, out: &mut Vec<Vec3>) {
            out.push(Vec3::new(h.x, 0., h.z));
        }
        fn floor(&mut self, p: Vec3) -> Option<Vec3> { Some(Vec3::new(p.x, 0., p.z)) }
        fn crosses(&mut self, _: Vec3, _: Vec3, _: u8) -> bool { true }
    }
    let state = nav.swbf2.as_mut().unwrap();
    state.floors = super::walk::Floors::build(state.graph.as_ref().unwrap(), &mut nav.common.barriers, &mut Deck).unwrap();
    let g = nav.swbf2.as_ref().unwrap().graph.as_ref().unwrap();

    // 2D XZ containment succeeds even when unit Y is 0 and hub Y is -73.
    assert!(g.contains_hub_2d(0, Vec3::new(1.0, 0.0, 1.0)));

    // One-way jump: 0 -> 1 allowed, 1 -> 0 blocked.
    assert_eq!(g.arcs[0].type_flags, ARC_ONE_WAY | ARC_JUMP);
    assert!(g.hub_route_distance(0, 1, 1, false).reachable);
    assert!(!g.hub_route_distance(1, 0, 1, false).reachable);

    // Jet-jump bridge: blocked without jetpack, allowed with jetpack.
    assert_eq!(g.arcs[1].type_flags, ARC_JET_JUMP);
    assert!(!g.hub_route_distance(0, 2, 1, false).reachable);
    assert!(g.hub_route_distance(0, 2, 1, true).reachable);

    // Filter flags: soldier (mask=1) cannot use hover_only (mask=4), hover (mask=4) can.
    assert!(!g.hub_route_distance(0, 3, 1, false).reachable);
    assert!(g.hub_route_distance(0, 3, 4, false).reachable);

    // Execute a jump arc with a soldier and confirm intent.jump fires at the mid node.
    let ctrl = nav.new_control();
    let mut helper = Swbf2GroundGoto::new(ctrl);
    let mut world = TestWorld::new(11).with_wall(7.5, -5.0, 7.5, 5.0);
    // Wall separates direct walk (0,0,0) -> (15,0,0), forcing the jump arc 0->1.
    let mut unit = make_soldier(1, Vec3::new(0.0, 0.0, 0.0), false);
    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, Vec3::new(15.0, 0.0, 0.0), GotoMode::Route);
    nav.service(1.0);
    let mut jumped = false;
    for _ in 0..180 {
        world.time += 1.0 / 60.0;
        nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1.0 / 60.0);
        nav.service(1.0);
        let intent = nav.swbf2_navigation_input(ctrl, &mut unit, 1.0 / 60.0, None, true, &mut world);
        if intent.jump {
            jumped = true;
            break;
        }
        // Advance unit along its steering request.
        let req = nav.controls[ctrl].request;
        let dx = req.steering_target.x - unit.position.x;
        let dz = req.steering_target.z - unit.position.z;
        let len = v8math::hypot(dx, dz);
        if len > 1e-4 {
            let step = v8math::min(6.0f64 / 60.0, len);
            unit.position.x += dx / len * step;
            unit.position.z += dz / len * step;
        }
    }
    assert!(jumped, "expected 2005 jump arc to trigger intent.jump");

    // Execute a jet-jump arc with a jetpack soldier and confirm jet intent fires.
    let ctrl2 = nav.new_control();
    let mut helper2 = Swbf2GroundGoto::new(ctrl2);
    let mut world2 = TestWorld::new(13).with_wall(-5.0, 7.5, 5.0, 7.5);
    let mut jet_unit = make_soldier(2, Vec3::new(0.0, 0.0, 0.0), true);
    nav.swbf2_ground_goto(&mut helper2, &mut jet_unit, &mut world2, Vec3::new(0.0, 0.0, 15.0), GotoMode::Route);
    nav.service(1.0);
    let mut jetted = false;
    for _ in 0..180 {
        world2.time += 1.0 / 60.0;
        nav.swbf2_ground_tick(&mut helper2, &mut jet_unit, &mut world2, 1.0 / 60.0);
        nav.service(1.0);
        let intent = nav.swbf2_navigation_input(ctrl2, &mut jet_unit, 1.0 / 60.0, None, true, &mut world2);
        if intent.jump {
            // Mirror soldier physics: jumping puts the soldier into Air::Jump
            // so the next tick lights the jetpack with jet_direction.
            jet_unit.soldier.as_mut().unwrap().air = Air::Jump;
        }
        if intent.jet_direction.flatten().is_some() {
            jetted = true;
            break;
        }
        let req = nav.controls[ctrl2].request;
        let dx = req.steering_target.x - jet_unit.position.x;
        let dz = req.steering_target.z - jet_unit.position.z;
        let len = v8math::hypot(dx, dz);
        if len > 1e-4 {
            let step = v8math::min(6.0f64 / 60.0, len);
            jet_unit.position.x += dx / len * step;
            jet_unit.position.z += dz / len * step;
        }
    }
    assert!(jetted, "expected 2005 jetJump arc to trigger jet_direction");
}

#[test]
fn dynamic_barriers_2005_hints_and_boundary_paths() {
    let payload = r#"{
        "schema": 2,
        "map": "invented_extras",
        "barriers": [
            {"sourceIndex":0,"enabled":true,"name":"GateWall","xfrm":[1,0,0, 0,1,0, 0,0,1, 10,0,0],"size":[1,4,10],"flags":31}
        ],
        "hints": [
            {"sourceIndex":0,"name":"snipe0","type":"0","xfrm":[1,0,0, 0,1,0, 0,0,1, 2,0,0],"properties":[]},
            {"sourceIndex":1,"name":"extra7","type":"7","xfrm":[1,0,0, 0,1,0, 0,0,1, 4,0,0],"properties":[]},
            {"sourceIndex":2,"name":"extra8","type":"8","xfrm":[1,0,0, 0,1,0, 0,0,1, 6,0,0],"properties":[]}
        ],
        "paths": [
            {"name":"boundary1","nodes":[
                {"position":[-50,0,-50]},
                {"position":[-50,0,50]},
                {"position":[50,0,50]},
                {"position":[50,0,-50]}
            ]}
        ],
        "boundaryPaths": ["BOUNDARY1"]
    }"#;
    let parsed = json::parse_bytes(payload.as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&parsed, None, None, PlannerOptions::default()).unwrap();

    // Barrier initially blocks segment across x = 10.
    assert!(nav.barriers.segment_blocked(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0), 0.5, 1, None));
    assert!(nav.set_barriers_enabled("gatewall", false));
    assert!(!nav.barriers.segment_blocked(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0), 0.5, 1, None));
    assert!(nav.toggle_barriers("GATEWALL"));
    assert!(nav.barriers.segment_blocked(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0), 0.5, 1, None));

    // Hints include standard snipe (bit 1) plus 2005 types 7 and 8.
    let h7 = nav.query_hints(HINT_BIT_TYPE7, Vec3::new(4.0, 0.0, 0.0), 5.0, 4.0, None, 0.0);
    let h8 = nav.query_hints(HINT_BIT_TYPE8, Vec3::new(6.0, 0.0, 0.0), 5.0, 4.0, None, 0.0);
    assert_eq!(h7.len(), 1);
    assert_eq!(h8.len(), 1);

    // Boundary polygon check and F4Tail boundary edge extraction.
    assert!(!nav.outside(Vec3::new(0.0, 0.0, 0.0)));
    assert!(nav.outside(Vec3::new(80.0, 0.0, 0.0)));
    let paths = parsed.get("paths").unwrap().as_array().unwrap();
    let edges = boundary_edges_from_swbf2(parsed.get("boundaryPaths"), paths);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].len(), 4);
}

#[test]
fn detours_around_a_wall_and_cancels_a_stalled_leg() {
    // Wall along x = 10 from z = -20 to z = 12 blocks direct walk (0,0,0) -> (20,0,0) (only 20m < 50m!).
    // Street hubs route around the north end of the wall: h0(0,0,0) -> h1(0,0,16) -> h2(20,0,16) -> h3(20,0,0).
    let payload = r#"{
        "schema": 2,
        "map": "invented_wall_detour",
        "plan": {
            "branchCount": 5,
            "hubs": [
                {"name":"h0","position":[0,0,0],"radius":3,"slots":[0,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"h1","position":[0,0,16],"radius":3,"slots":[0,1,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"h2","position":[20,0,16],"radius":3,"slots":[1,2,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"h3","position":[20,0,0],"radius":3,"slots":[2,255,255,255,255,255,255,255],"weightCounts":[0,0,0,0,0],"weights":[]}
            ],
            "arcs": [
                {"name":"a0","start":0,"end":1,"filterFlags":1,"typeFlags":0},
                {"name":"a1","start":1,"end":2,"filterFlags":1,"typeFlags":0},
                {"name":"a2","start":2,"end":3,"filterFlags":1,"typeFlags":0}
            ]
        }
    }"#;
    let parsed = json::parse_bytes(payload.as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&parsed, None, None, PlannerOptions::default()).unwrap();
    let ctrl = nav.new_control();
    let mut helper = Swbf2GroundGoto::new(ctrl);
    let mut world = TestWorld::new(23).with_wall(10.0, -20.0, 10.0, 12.0);
    let mut unit = make_soldier(1, Vec3::new(0.0, 0.0, 0.0), false);

    // 20 m is below the 50 m long-range threshold, yet line_of_walk is blocked,
    // so 2005 routing must attach a graph cursor and detour around the wall.
    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, Vec3::new(20.0, 0.0, 0.0), GotoMode::Route);
    assert!(helper.cursor.is_some(), "expected short-range blocked route to use graph cursor");
    nav.service(1.0);

    let mut max_z = 0.0f64;
    for _ in 0..900 {
        world.time += 1.0 / 60.0;
        nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1.0 / 60.0);
        nav.service(1.0);
        let _ = nav.swbf2_navigation_input(ctrl, &mut unit, 1.0 / 60.0, None, true, &mut world);
        if nav.swbf2_ground_arrived(&helper) {
            break;
        }
        let req = nav.controls[ctrl].request;
        let dx = req.steering_target.x - unit.position.x;
        let dz = req.steering_target.z - unit.position.z;
        let len = v8math::hypot(dx, dz);
        if len > 1e-4 {
            let step = v8math::min(6.0f64 / 60.0, len);
            let next = Vec3::new(unit.position.x + dx / len * step, 0.0, unit.position.z + dz / len * step);
            assert!(world.segment_clear(unit.position, next), "bot tried to walk through wall!");
            unit.position = next;
            max_z = v8math::max(max_z, unit.position.z);
        }
    }
    assert!(nav.swbf2_ground_arrived(&helper), "bot should arrive around the wall");
    assert!(max_z > 12.0, "expected route to go north of wall (z > 12), max_z = {max_z}");

    // A stalled mover must release its steering request.
    let mut stuck_unit = make_soldier(2, Vec3::new(0.0, 0.0, 0.0), false);
    nav.swbf2_ground_goto(&mut helper, &mut stuck_unit, &mut world, Vec3::new(20.0, 0.0, 0.0), GotoMode::Route);
    nav.service(1.0);
    for _ in 0..600 {
        world.time += 1.0 / 60.0;
        nav.swbf2_ground_tick(&mut helper, &mut stuck_unit, &mut world, 1.0 / 60.0);
        nav.service(1.0);
        let _ = nav.swbf2_navigation_input(ctrl, &mut stuck_unit, 1.0 / 60.0, None, true, &mut world);
        if helper.failed {
            break;
        }
    }
    assert!(helper.failed);
    assert!(nav.controls[ctrl].executor.is_none());
    assert!(!nav.controls[ctrl].request.valid);
}

#[test]
fn route_distance_works_across_2005_2004_and_graphless_arenas() {
    // 1. 2005 graph with separating building wall + sealed off pocket.
    let mut swbf2_nav = diamond_plnp_nav();
    let wall_world = TestWorld::new(1).with_wall(20.0, -5.0, 20.0, 5.0);
    // Direct (0,0,0) -> (40,0,0) hits wall at x=20, so route_distance routes through h0 -> h1 -> h3 (~44.72 m).
    let q_open = swbf2_nav.route_distance(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(40.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |a, b| wall_world.segment_clear(a, b),
    );
    assert!(q_open.reachable);
    assert_eq!(q_open.hops, 2);
    assert!(q_open.distance > 44.0 && q_open.distance < 46.0);
    // Deterministic repeat returns bit-identical result.
    let q_repeat = swbf2_nav.route_distance(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(40.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |a, b| wall_world.segment_clear(a, b),
    );
    assert_eq!(q_open.distance.to_bits(), q_repeat.distance.to_bits());
    assert_eq!(q_open.hops, q_repeat.hops);

    // Block top arc via plnp: route_distance switches to ~56.57 m immediately.
    assert!(swbf2_nav.block_planning_arcs("arc_top_a"));
    let q_detour = swbf2_nav.route_distance(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(40.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |a, b| wall_world.segment_clear(a, b),
    );
    assert!(q_detour.reachable);
    assert!(q_detour.distance > 55.0);

    // Enemy sealed inside a 4-wall box with no walkable hub is UNREACHABLE.
    let sealed_world = TestWorld::new(1)
        .with_wall(95.0, -5.0, 95.0, 5.0)
        .with_wall(105.0, -5.0, 105.0, 5.0)
        .with_wall(95.0, -5.0, 105.0, -5.0)
        .with_wall(95.0, 5.0, 105.0, 5.0);
    let q_sealed = swbf2_nav.route_distance(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(100.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |a, b| sealed_world.segment_clear(a, b),
    );
    assert_eq!(q_sealed, RouteQuery::UNKNOWN);

    // 2. Graphless arena with a barrier wall.
    let barrier_payload = r#"{
        "schema": 2,
        "map": "invented_graphless",
        "barriers": [
            {"sourceIndex":0,"enabled":true,"name":"MidWall","xfrm":[1,0,0, 0,1,0, 0,0,1, 10,0,0],"size":[1,4,20],"flags":31}
        ]
    }"#;
    let parsed_gl = json::parse_bytes(barrier_payload.as_bytes()).unwrap();
    let mut nav_gl = MapNavigation::new_swbf2(&parsed_gl, None, None, PlannerOptions::default()).unwrap();
    let q_clear = nav_gl.route_distance(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(5.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |_, _| true,
    );
    assert!(q_clear.reachable);
    assert_eq!(q_clear.hops, 0);
    let q_wall = nav_gl.route_distance(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(20.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |_, _| true,
    );
    assert_eq!(q_wall, RouteQuery::UNKNOWN);

    // 3. 2004 graph (schema 1) read-only route_distance.
    let w01 = u32::from(pack_byte(0, 31));
    let w10 = u32::from(pack_byte(0, 31));
    let mut words = vec![0u32; 2 * 5 * 2];
    words[1] = w01; // hub 0 -> hub 1, table 0
    words[5 * 2] = w10; // hub 1 -> hub 0, table 0
    let mut bin_2004 = Vec::with_capacity(words.len() * 4);
    for w in &words {
        bin_2004.extend_from_slice(&w.to_le_bytes());
    }
    let payload_2004 = r#"{
        "schema": 1,
        "map": "invented_2004",
        "graph": {
            "info": [2, 1, 5],
            "hubs": [
                {"name":"h0","position":[0,0,0],"radius":4,"arcs":[0,255,255,255,255,255,255,255]},
                {"name":"h1","position":[60,0,0],"radius":4,"arcs":[0,255,255,255,255,255,255,255]}
            ],
            "arcs": [
                {"name":"a0","hubs":[0,1],"mask":1}
            ],
            "routes": {"byteOffset":0,"count":20,"tables":5}
        }
    }"#;
    let parsed_2004 = json::parse_bytes(payload_2004.as_bytes()).unwrap();
    let nav_2004 = Nav::new(&parsed_2004, Some(&bin_2004), None, None, PlannerOptions::default()).unwrap();
    let q_2004 = MapNavigation::from(nav_2004).route_distance(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(60.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |_, _| true,
    );
    assert!(q_2004.reachable);
    assert_eq!(q_2004.hops, 1);
    assert!((q_2004.distance - 60.0).abs() < 0.01);
}

#[test]
fn disconnected_destinations_stop_before_publishing_a_leg() {
    let payload = r#"{
        "schema": 2,
        "map": "invented_two_islands",
        "plan": {
            "hubs": [
                {"name":"deck_a","position":[0,-200,0],"radius":4,"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"bay_exit","position":[40,-200,0],"radius":4,"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"other_ship","position":[400,-200,0],"radius":4,"weightCounts":[0,0,0,0,0],"weights":[]},
                {"name":"other_core","position":[440,-200,0],"radius":4,"weightCounts":[0,0,0,0,0],"weights":[]}
            ],
            "arcs": [
                {"name":"corridor_a","start":0,"end":1,"filterFlags":1,"typeFlags":0},
                {"name":"corridor_b","start":2,"end":3,"filterFlags":1,"typeFlags":0}
            ]
        }
    }"#;
    let parsed = json::parse_bytes(payload.as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&parsed, None, None, PlannerOptions::default()).unwrap();
    nav.prepare_owned();

    // Reachability query across disconnected islands must still report UNREACHABLE.
    let q = nav.route_distance(
        Vec3::new(0.0, -100.0, 0.0),
        Vec3::new(400.0, 0.0, 0.0),
        0.5,
        1,
        false,
        |_, _| true,
    );
    assert_eq!(q, RouteQuery::UNREACHABLE);

    let mut world = TestWorld::new(77);
    let mut unit = make_soldier(1, Vec3::new(0.0, -100.0, 0.0), false);
    let ctrl = nav.new_control();
    let mut helper = Swbf2GroundGoto::new(ctrl);
    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, Vec3::new(400.0, 0.0, 0.0), GotoMode::Route);
    assert!(helper.failed);
    assert!(nav.controls[ctrl].executor.is_none());
    assert!(!nav.controls[ctrl].request.valid);
}

#[test]
fn barrier_enable_is_separate_from_traversal_and_hint_order_precedes_expansion() {
    let mut barriers = Vec::new();
    for bit in [1, 2, 4, 8, 16, 31] {
        barriers.push(serde_json::json!({"sourceIndex":barriers.len(),"name":format!("panel{bit}"),"flags":bit|32,
            "enabled":false,"xfrm":[1,0,0,0,1,0,0,0,-1,4,1,2],"size":[1,2,3]}));
    }
    let frame = [1,0,0,0,1,0,0,0,1,7,4,2];
    let payload = serde_json::json!({"schema":2, "barriers":barriers, "hints":[
        {"sourceIndex":0,"type":"unknown","xfrm":frame},{"sourceIndex":1,"type":"8","xfrm":frame,"properties":[["bf0f1cd1",null,"6"]]},
        {"sourceIndex":2,"type":"7","xfrm":frame},{"sourceIndex":3,"type":"2","xfrm":frame}
    ]});
    let parsed = json::parse_bytes(payload.to_string().as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&parsed, None, Some(2.), PlannerOptions::default()).unwrap();
    for (index, mask) in [1,2,4,8,16,31].into_iter().enumerate() {
        let name = format!("panel{mask}");
        assert_eq!(nav.barriers.list[index].flags, 0);
        assert!(nav.toggle_barriers(&name));
        assert_eq!(nav.barriers.list[index].flags, mask);
    }
    assert_eq!(nav.hints.list.len(), 2);
    assert_eq!((nav.hints.list[0].source, nav.hints.list[0].bit, nav.hints.list[0].primary_mask), (1,128,6));
    assert_eq!((nav.hints.list[1].source, nav.hints.list[1].bit), (2,64));
}

#[test]
fn directed_chain_finishes_every_leg_and_revokes_active_steering() {
    let n = 193;
    let hubs = (0..n).map(|i| format!(r#"{{"position":[{},0,0],"radius":1}}"#, i*3))
        .collect::<Vec<_>>().join(",");
    let arcs = (1..n).map(|i| format!(r#"{{"start":{},"end":{i},"name":"gate{i}","oneWay":true,"filterFlags":1}}"#, i-1))
        .collect::<Vec<_>>().join(",");
    let data = format!(r#"{{"schema":2,"plan":{{"hubs":[{hubs}],"arcs":[{arcs}]}}}}"#);
    let json = json::parse_bytes(data.as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&json, None, None, Default::default()).unwrap();
    let start = Vec3::default();
    let end = Vec3::new(3. * (n-1) as f64, 0., 0.);
    let first = Vec3::new(3., 0., 0.);
    let mut world = TestWorld::new(2);
    let mut unit = make_soldier(1, start, false);
    let ctrl = nav.new_control();
    let mut helper = Swbf2GroundGoto::new(ctrl);
    let distance = nav.route_distance(start, end, 0.5, 1, false, |_, _| true);
    assert_eq!(distance.hops, n-1);
    assert_eq!(distance.distance, end.x);
    assert!(!nav.route_distance(first, start, 0.5, 1, false, |_, _| true).reachable);
    assert!(!nav.route_distance(start, first, 0.5, 4, false, |_, _| true).reachable);

    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, end, GotoMode::Route);
    for _ in 0..8 {
        nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1./60.);
        nav.swbf2_navigation_input(ctrl, &mut unit, 1./60., None, true, &mut world);
    }
    assert!(nav.controls[ctrl].request.valid);
    assert!(nav.controls[ctrl].request.steering_target.x > 0.);
    assert!(nav.block_planning_arcs("gate2"));
    assert!(nav.controls[ctrl].request.valid);
    assert!(nav.unblock_planning_arcs("gate2"));
    assert!(nav.block_planning_arcs("gate1"));
    assert!(!nav.controls[ctrl].request.valid);
    assert!(nav.controls[ctrl].executor.is_none());
    nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1./60.);
    assert!(helper.failed);
    assert!(nav.unblock_planning_arcs("gate1"));
    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, end, GotoMode::Route);
    for tick in 1..=n * 4 {
        // Each tick brings its own search allowance.
        nav.swbf2.as_mut().unwrap().begin_tick(tick as _);
        nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1./60.);
        nav.swbf2_navigation_input(ctrl, &mut unit, 1./60., None, true, &mut world);
        let request = nav.controls[ctrl].request;
        if request.valid { unit.position = request.waypoint; }
        if nav.swbf2_ground_arrived(&helper) { break; }
    }
    assert!(nav.swbf2_ground_arrived(&helper), "at {:?} with {helper:?}", unit.position);
    assert_eq!(unit.position.x, end.x);
}

#[test]
fn stacked_decks_share_hub_discs_and_keep_independent_routes() {
    struct Decks;
    impl super::walk::Survey for Decks {
        fn floors(&mut self, h:&super::Swbf2Hub, out:&mut Vec<Vec3>) {
            for y in [3.0,12.0] { out.push(Vec3::new(h.x,y,h.z)); }
        }
        fn floor(&mut self, p:Vec3)->Option<Vec3> {
            [3.,12.].into_iter().find(|y| (p.y-y).abs()<0.5).map(|y| Vec3::new(p.x,y,p.z))
        }
        fn crosses(&mut self,a:Vec3,b:Vec3,_:u8)->bool {(a.y-b.y).abs()<0.5}
    }
    let payload=br#"{"schema":2,"plan":{"hubs":[{"position":[0,-27,0],"radius":4},{"position":[12,-27,0],"radius":4}],"arcs":[{"start":0,"end":1,"filterFlags":1,"oneWay":true}]}}"#;
    let json=json::parse_bytes(payload).unwrap();
    let mut nav=MapNavigation::new_swbf2(&json,None,None,Default::default()).unwrap();
    let state=nav.swbf2.as_mut().unwrap();
    state.floors=super::walk::Floors::build(state.graph.as_ref().unwrap(),&mut nav.common.barriers,&mut Decks).unwrap();
    let query=|nav:&mut MapNavigation,a,b|nav.route_distance(a,b,0.5,1,false,|p,q|(p.y-q.y).abs()<0.5);
    let lower=Vec3::new(0.,3.,0.);
    let upper=Vec3::new(0.,12.,0.);
    let far=Vec3::new(12.,12.,0.);
    assert!(query(&mut nav,upper,far).reachable);
    assert!(!query(&mut nav,far,upper).reachable);
    assert!(!query(&mut nav,lower,upper).reachable);
    assert!(!query(&mut nav,lower,far).reachable);
}

#[test]
fn changing_collision_flags_revokes_a_corridor() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"name":"birch","position":[2,0,1],"radius":2},{"name":"cedar","position":[16,0,1],"radius":2}],
        "arcs":[{"name":"passage","start":0,"end":1,"filterFlags":1}]}}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data,None,None,Default::default()).unwrap();
    let (from,to)=(Vec3::new(2.,0.,1.),Vec3::new(16.,0.,1.));
    let control=nav.new_control();
    nav.swbf2.as_mut().unwrap().clearance.prepare();
    assert!(nav.observe_collision((1,1), &[0]));
    assert!(nav.route_distance(from,to,0.5,1,false,|_,_|true).reachable);
    nav.controls[control].request.valid=true;
    assert!(nav.observe_collision((1,1), &[1]));
    assert!(nav.controls[control].request.valid, "an independent control keeps its request");
    let world=TestWorld::new(3).with_wall(9.,-20.,9.,20.);
    assert!(!nav.route_distance(from,to,0.5,1,false,|a,b|world.segment_clear(a,b)).reachable);
    assert!(nav.observe_collision((1,1), &[0]));
    assert!(nav.route_distance(from,to,0.5,1,false,|_,_|true).reachable);
}

#[test]
fn oversized_barrier_grid_is_refused_at_load() {
    let data=json::parse_bytes(br#"{"schema":2,"barriers":[{"sourceIndex":0,"enabled":true,"xfrm":[1,0,0,0,1,0,0,0,1,0,0,0],"size":[100000,2,100000],"flags":1}]}"#).unwrap();
    assert!(MapNavigation::new_swbf2(&data,None,None,Default::default()).is_err());
}

#[test]
fn short_orders_inside_an_ordinary_corridor_keep_its_direction() {
    let data=json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,3,0],"radius":4},{"position":[23,3,0],"radius":4}],
        "arcs":[{"start":0,"end":1,"filterFlags":1,"oneWay":true}]}}"#).unwrap();
    let mut nav=MapNavigation::new_swbf2(&data,None,None,Default::default()).unwrap();
    let a=Vec3::new(4.,3.,0.5);
    let b=Vec3::new(19.,3.,0.5);
    let forward=nav.route_distance(a,b,0.5,1,false,|_,_|true);
    assert_eq!((forward.distance,forward.hops),(15.,1));
    assert!(!nav.route_distance(b,a,0.5,1,false,|_,_|true).reachable);
}

#[test]
fn a_point_outside_its_disc_can_enter_a_nearer_directed_route() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,0,0],"radius":1},{"position":[9,0,0],"radius":1},
                {"position":[9,0,14],"radius":1}],
        "arcs":[{"name":"gallery","start":0,"end":1,"filterFlags":1},
                {"name":"bend","start":1,"end":2,"filterFlags":1,"oneWay":true}]
    }}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let point = Vec3::new(4.,0.,3.);
    let goal = Vec3::new(9.,0.,14.);
    let clear = |a: Vec3,b: Vec3| (a.z <= 4. && b.z <= 4.) || (a.x == 9. && b.x == 9.);
    let route = nav.route_distance(point,goal,0.5,1,false,clear);
    assert_eq!(route.hops,1);
    assert!((route.distance - (v8math::sqrt(34.) + 14.)).abs() < 1e-9);
    assert!(!nav.route_distance(goal,point,0.5,1,false,clear).reachable);
    assert!(nav.block_planning_arcs("bend"));
    assert!(!nav.route_distance(point,goal,0.5,1,false,clear).reachable);
}

#[test]
fn a_near_waypoint_hands_off_only_to_a_walkable_successor() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[2,3,1],"radius":2},{"position":[13,3,1],"radius":2},
                {"position":[13,3,15],"radius":2}],
        "arcs":[{"start":0,"end":1,"filterFlags":1},{"start":1,"end":2,"filterFlags":1}]
    }}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut helper = Swbf2GroundGoto::new(nav.new_control());
    let mut unit = make_soldier(3, Vec3::new(2., 3., 1.), false);
    let mut world = TestWorld::new(8);
    let corner = Vec3::new(13., 3., 1.);
    let exit = Vec3::new(13., 3., 15.);
    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, exit, GotoMode::Route);
    assert_eq!(helper.cursor.as_ref().unwrap().waypoint, corner);

    unit.position = Vec3::new(12.25, 3., 1.);
    nav.swbf2_ground_hold_path(&helper, true);
    nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1. / 60.);
    assert_eq!(helper.cursor.as_ref().unwrap().waypoint, corner);
    nav.swbf2_ground_hold_path(&helper, false);

    world.walls.push([12., 3., 12.9, 3.]);
    nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1. / 60.);
    assert_eq!(helper.cursor.as_ref().unwrap().waypoint, corner);
    world.walls.clear();
    helper.cursor.as_mut().unwrap().in_passage = true;
    unit.velocity = Vec3::new(0., 0., -4.);
    nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1. / 60.);
    assert_eq!(helper.cursor.as_ref().unwrap().waypoint, corner);
    unit.velocity = Vec3::default();
    nav.swbf2_ground_tick(&mut helper, &mut unit, &mut world, 1. / 60.);
    assert_eq!(helper.cursor.as_ref().unwrap().waypoint, exit);
    assert_eq!(nav.controls[helper.control].executor.as_ref().unwrap().waypoint, exit);
}

#[test]
fn an_order_at_the_entrance_starts_its_first_corridor_immediately() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,2,0],"radius":2},{"position":[8,2,0],"radius":2},
                {"position":[8,2,9],"radius":2}],
        "arcs":[{"start":0,"end":1,"filterFlags":1},{"start":1,"end":2,"filterFlags":1}]
    }}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let control = nav.new_control();
    let mut helper = Swbf2GroundGoto::new(control);
    let start = Vec3::new(0., 2., 0.);
    let corner = Vec3::new(8., 2., 0.);
    let goal = Vec3::new(8., 2., 9.);
    let mut unit = make_soldier(2, start, false);
    let mut world = TestWorld::new(31);

    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, goal, GotoMode::Route);
    assert_eq!(helper.cursor.as_ref().unwrap().waypoint, corner);

    unit.position = Vec3::new(-0.3, 2., 0.);
    nav.swbf2_ground_stop(&mut helper, &mut unit, &mut world);
    nav.swbf2_ground_goto(&mut helper, &mut unit, &mut world, goal, GotoMode::Route);
    assert_eq!(helper.cursor.as_ref().unwrap().waypoint, start);
}

#[test]
fn local_orders_follow_the_floor_under_an_airborne_target() {
    let parsed = json::parse_bytes(br#"{"schema":2,"plan":{"hubs":[],"arcs":[]}}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&parsed, None, None, Default::default()).unwrap();
    let mut w = TestWorld::new(7).with_wall(6., -16., 6., 16.);
    w.flat = true;
    let mut unit = make_soldier(5, Vec3::default(), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    let target = Vec3::new(12., 8., 0.);
    for mode in [GotoMode::Straight, GotoMode::Direct] {
        nav.swbf2_ground_goto(&mut h, &mut unit, &mut w, target, mode);
        assert!(!h.failed);
        assert!(h.destination.x > 4. && h.destination.x < 6.);
        assert_eq!(h.destination.y, 0.);
    }
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut w, target, GotoMode::Route);
    assert!(h.failed);
}


#[test]
fn a_partial_point_search_never_exceeds_its_probe_budget() {
    let hubs=(0..24).map(|i|serde_json::json!({"position":[i*2,0,0],"radius":3})).collect::<Vec<_>>();
    let arcs=(1..24).map(|i|serde_json::json!({"start":i-1,"end":i,"filterFlags":1})).collect::<Vec<_>>();
    let row=serde_json::json!({"schema":2,"plan":{"hubs":hubs,"arcs":arcs}});
    let value=json::parse_bytes(row.to_string().as_bytes()).unwrap();
    let mut nav=MapNavigation::new_swbf2(&value,None,None,Default::default()).unwrap();
    let mut calls=0;
    let q=nav.route_distance(Vec3::new(15.,0.,2.),Vec3::new(37.,0.,2.),0.45,1,false,|a,b| {
        calls+=1;
        a==b
    });
    assert!(calls<=16 && calls>2);
    assert_eq!(q,RouteQuery::UNKNOWN);
    assert!(nav.route_distance(Vec3::new(15.,0.,2.),Vec3::new(37.,0.,2.),0.45,1,false,|_,_|true).reachable);
}

#[test]
fn rechecking_a_descent_uses_the_bounded_drop_mover() {
    struct Steps;
    impl super::walk::Survey for Steps {
        fn floors(&mut self,h:&super::Swbf2Hub,out:&mut Vec<Vec3>) {out.push(h.pos());}
        fn floor(&mut self,_:Vec3)->Option<Vec3> {None}
        fn crosses(&mut self,a:Vec3,b:Vec3,mode:u8)->bool {a.y<=b.y || mode==super::walk::DROP}
    }
    let payload=br#"{"schema":2,"plan":{"hubs":[{"position":[0,0,0],"radius":1},{"position":[7,3,0],"radius":1}],"arcs":[{"start":0,"end":1,"filterFlags":1}]}}"#;
    let json=json::parse_bytes(payload).unwrap();
    let mut nav=MapNavigation::new_swbf2(&json,None,None,Default::default()).unwrap();
    let state=nav.swbf2.as_mut().unwrap();
    state.floors=super::walk::Floors::build(state.graph.as_ref().unwrap(),&mut nav.common.barriers,&mut Steps).unwrap();
    state.floors.collision_changed();
    let mut drop=false;
    let result=nav.distance_with_clearance(Vec3::new(7.,3.,0.),Vec3::default(),0.45,1,false,false,|a,b,m| {
        drop |= m==super::walk::DROP;
        a.y<=b.y || m==super::walk::DROP
    });
    assert!(result.reachable && drop);
}


#[test]
fn closing_a_destination_stops_only_the_order_that_uses_it() {
    let data=json::parse_bytes(br#"{"schema":2,"plan":{"hubs":[
        {"position":[0,0,0],"radius":1},{"position":[5,0,0],"radius":1},{"position":[10,0,0],"radius":1}],
        "arcs":[{"start":0,"end":1,"filterFlags":1},{"start":1,"end":2,"filterFlags":1}]},
        "barriers":[{"name":"shutter","sourceIndex":0,"enabled":false,"flags":1,"size":[1,2,1],
        "xfrm":[1,0,0,0,1,0,0,0,1,10,0,0]}]}"#).unwrap();
    let mut nav=MapNavigation::new_swbf2(&data,None,None,Default::default()).unwrap();
    let mut world=TestWorld::new(5);
    let mut short=Swbf2GroundGoto::new(nav.new_control());
    let mut long=Swbf2GroundGoto::new(nav.new_control());
    let mut unit=make_soldier(3,Vec3::default(),false);
    for (order,end) in [(&mut short,5.),(&mut long,10.)] {
        nav.swbf2_ground_goto(order,&mut unit,&mut world,Vec3::new(end,0.,0.),GotoMode::Route);
        nav.swbf2_control_update(order.control,1./60.,&mut unit,&mut world);
        assert!(nav.controls[order.control].request.valid);
    }
    assert!(nav.set_barriers_enabled("shutter",true));
    assert!(nav.controls[short.control].request.valid);
    assert!(!nav.controls[long.control].request.valid);
    nav.swbf2_ground_tick(&mut long,&mut unit,&mut world,1./60.);
    assert!(long.failed);
}

#[test]
fn a_combat_order_shares_its_search_budget_across_approaches() {
    let hubs = (0..24)
        .map(|i| serde_json::json!({"position":[i*2,0,0],"radius":3}))
        .collect::<Vec<_>>();
    let arcs = (1..24)
        .map(|i| serde_json::json!({"start":i-1,"end":i,"filterFlags":1}))
        .collect::<Vec<_>>();
    let data = serde_json::json!({"schema":2,"plan":{"hubs":hubs,"arcs":arcs}});
    let value = json::parse_bytes(data.to_string().as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&value, None, None, Default::default()).unwrap();
    let mut unit = make_soldier(1, Vec3::new(15., 0., 2.), false);
    let mut world = TestWorld::new(2).with_wall(-20., 1., 65., 1.);
    let mut helper = Swbf2GroundGoto::new(nav.new_control());
    nav.ground_approach(
        &mut helper,
        &mut unit,
        &mut world,
        Vec3::new(37., 0., 2.),
        GotoMode::Route,
        true,
    );
    assert!(helper.failed);
    assert!((3..=16).contains(&world.drives), "{} drives", world.drives);
}

#[test]
fn hunters_share_four_questions_until_the_simulation_tick_changes() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{"hubs":[{"position":[0,0,0],"radius":8},{"position":[5,0,0],"radius":3}],"arcs":[{"start":0,"end":1,"filterFlags":1}]}}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let ask = |nav: &mut MapNavigation, r| {
        nav.route_distance(
            Vec3::default(),
            Vec3::new(1., 0., 0.),
            r,
            1,
            false,
            |_, _| true,
        )
    };
    nav.swbf2.as_mut().unwrap().begin_tick(7);
    for radius in [0.31, 0.42, 0.49, 0.31] {
        assert!(ask(&mut nav, radius).reachable);
    }
    nav.swbf2.as_mut().unwrap().begin_tick(7);
    assert_eq!(ask(&mut nav, 0.49), RouteQuery::UNKNOWN);
    nav.swbf2.as_mut().unwrap().begin_tick(8);
    assert!(ask(&mut nav, 0.49).reachable);
}

#[test]
fn a_soldier_beside_a_barrier_is_an_uncertain_start() {
    let data = json::parse_bytes(br#"{"schema":2,"barriers":[{"sourceIndex":0,"name":"panel","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,1,0,0,0],"size":[1,3,2],"flags":1}]}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let near = Vec3::new(1.2, 0., 0.);
    let free = Vec3::new(4., 0., 0.);
    assert!(nav.common.barriers.point_clear(near, 0., 1).0);
    assert!(!nav.common.barriers.point_clear(near, 0.5, 1).0);
    let ask = |nav: &mut MapNavigation, a, b| nav.route_distance(a, b, 0.5, 1, false, |_, _| true);
    assert_eq!(ask(&mut nav, near, near), RouteQuery::UNKNOWN);
    assert_eq!(ask(&mut nav, near, free), RouteQuery::UNKNOWN);
    assert_eq!(
        ask(&mut nav, free, Vec3::default()),
        RouteQuery::UNREACHABLE
    );
}

#[test]
fn doors_in_one_weight_group_switch_independently_by_name() {
    let mut nav = diamond_plnp_nav();
    let graph = nav.swbf2.as_mut().unwrap().graph.as_mut().unwrap();
    graph.groups[0].arc_indices.push(2);
    graph.groups[0].arc_hashes.push(graph.arcs[2].name_hash);
    assert!(graph.set_arcs_blocked("arc_top_a", true));
    assert!(graph.arcs[0].is_blocked() && !graph.arcs[2].is_blocked());
    assert!(graph.toggle_arcs("arc_bot_a"));
    assert!(graph.set_arcs_blocked("arc_top_a", false));
    assert!(!graph.arcs[0].is_blocked() && graph.arcs[2].is_blocked());
    assert!(graph.toggle_arcs("arc_bot_a"));
    assert!(!graph.arcs[2].is_blocked());
}

#[test]
fn a_jump_can_touch_down_beyond_an_overhang_then_walk_back() {
    struct Ledge;
    impl super::walk::Survey for Ledge {
        fn floors(&mut self, h: &super::Swbf2Hub, out: &mut Vec<Vec3>) {
            out.push(h.pos());
        }
        fn floor(&mut self, p: Vec3) -> Option<Vec3> {
            Some(p)
        }
        fn open_landing(&mut self, _: Vec3, b: Vec3) -> bool {
            b.x >= 6.
        }
        fn crosses(&mut self, a: Vec3, b: Vec3, m: u8) -> bool {
            if a.y - b.y > 2. {
                m == super::ARC_JUMP && b.x >= 6.
            } else {
                a.y == b.y
            }
        }
    }
    let data=json::parse_bytes(br#"{"schema":2,"plan":{"hubs":[{"position":[0,4,0],"radius":1},{"position":[4,0,0],"radius":1}],"arcs":[{"start":0,"end":1,"jump":true,"oneWay":true,"filterFlags":1}]}}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let state = nav.swbf2.as_mut().unwrap();
    let graph = state.graph.as_ref().unwrap();
    state.floors = super::walk::Floors::build(graph, &mut nav.common.barriers, &mut Ledge).unwrap();
    let (_, bends, move_type, _, after) = state
        .floors
        .next(
            graph,
            &mut nav.common.barriers,
            0,
            1,
            0.45,
            1,
            false,
            &mut super::walk::SearchBudget::new(),
            &mut |_, _, _| Some(true),
        )
        .unwrap();
    assert_eq!((move_type, after), (super::ARC_JUMP, 1));
    assert!(bends.points()[0].x >= 6.);
    state.floors.changed_bodies(&[[-1., -2., -1., 8., 8., 1.]]);
    assert!(state
        .floors
        .next(
            graph,
            &mut nav.common.barriers,
            0,
            1,
            0.45,
            1,
            false,
            &mut super::walk::SearchBudget::new(),
            &mut |_, _, _| Some(true),
        )
        .is_none());
}

#[test]
fn a_question_without_work_left_is_unknown_and_leaves_no_answer_behind() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,0,0],"radius":4},{"position":[22,0,0],"radius":4}],
        "arcs":[{"start":0,"end":1,"filterFlags":1}]
    }}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    nav.swbf2.as_mut().unwrap().clearance.prepare();
    assert!(nav.observe_collision((1, 1), &[]));
    let (from, to) = (Vec3::default(), Vec3::new(22., 0., 0.));
    let mut ask = |tick, clear| {
        nav.swbf2.as_mut().unwrap().begin_tick(tick);
        nav.try_distance_with_clearance(from, to, 0.45, 1, false, true, |_, _, _| clear)
    };
    assert_eq!(ask(1, None), RouteQuery::UNKNOWN);
    assert!(ask(2, Some(true)).reachable);
}

#[test]
fn unconnected_decks_need_no_physical_check() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,0,0],"radius":4},{"position":[20,0,0],"radius":4},
                {"position":[700,90,0],"radius":4},{"position":[720,90,0],"radius":4}],
        "arcs":[{"start":0,"end":1,"filterFlags":1},{"start":2,"end":3,"filterFlags":1}]
    }}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let (here, there) = (Vec3::new(6., 0., 2.), Vec3::new(706., 90., 2.));
    // Beside the deck, in the air above it and far from every deck.
    for to in [there, Vec3::new(5., 60., 0.), Vec3::new(300., 0., 0.)] {
        assert_eq!(
            nav.route_distance(here, to, 0.45, 1, false, |_, _| panic!("a check across decks")),
            RouteQuery::UNKNOWN
        );
        let mut unit = make_soldier(1, here, false);
        let mut h = Swbf2GroundGoto::new(nav.new_control());
        let mut world = TestWorld::new(4);
        nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Route);
        assert!(h.failed && world.drives == 0);
        nav.swbf2.as_mut().unwrap().begin_tick(to.x as u32);
    }
    assert!(
        nav.route_distance(here, Vec3::new(17., 0., 1.), 0.45, 1, false, |_, _| true)
            .reachable
    );
}

#[test]
fn a_reachable_entrance_can_share_a_disc_with_a_dead_end() {
    struct SplitFloor;
    impl super::walk::Survey for SplitFloor {
        fn floors(&mut self, hub: &super::Swbf2Hub, points: &mut Vec<Vec3>) {
            points.push(hub.pos());
            if hub.index == 0 {
                points.push(Vec3::new(0., 0., 2.));
            }
        }

        fn floor(&mut self, point: Vec3) -> Option<Vec3> {
            Some(point)
        }

        fn crosses(&mut self, a: Vec3, b: Vec3, _: u8) -> bool {
            a.z >= 2. && b.z >= 2.
        }
    }

    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,0,0],"radius":3},{"position":[16,0,2],"radius":3}],
        "arcs":[{"start":0,"end":1,"filterFlags":1}]
    }}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let state = nav.swbf2.as_mut().unwrap();
    state.floors = super::walk::Floors::build(
        state.graph.as_ref().unwrap(),
        &mut nav.common.barriers,
        &mut SplitFloor,
    )
    .unwrap();
    let result = nav.route_distance(
        Vec3::new(0., 0., 0.5),
        Vec3::new(16., 0., 2.),
        0.45,
        1,
        false,
        |a, b| (a.x - b.x).abs() < 3. || (a.z >= 2. && b.z >= 2.),
    );
    assert!(result.reachable);
}

#[test]
fn cold_rows_share_the_tick_allowance_across_callers() {
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,0,0],"radius":3},{"position":[12,0,0],"radius":3}],
        "arcs":[{"start":0,"end":1,"filterFlags":1}]
    }}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let state = nav.swbf2.as_mut().unwrap();
    state.begin_tick(17);
    state.floors.invalidate();
    state.row_entries_left = 0;
    let ask = |nav: &mut MapNavigation| {
        nav.route_distance(
            Vec3::default(),
            Vec3::new(12., 0., 0.),
            0.45,
            1,
            false,
            |_, _| true,
        )
    };
    assert_eq!(ask(&mut nav), RouteQuery::UNKNOWN);
    nav.swbf2.as_mut().unwrap().begin_tick(17);
    assert_eq!(ask(&mut nav), RouteQuery::UNKNOWN);
    nav.swbf2.as_mut().unwrap().begin_tick(18);
    assert!(ask(&mut nav).reachable);
}

#[test]
fn spent_route_work_leaves_a_straight_recovery_move() {
    let data = json::parse_bytes(br#"{"schema":2}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let destination = Vec3::new(4., 0., 0.);
    let mut unit = make_soldier(1, Vec3::default(), false);
    let mut helper = Swbf2GroundGoto::new(nav.new_control());
    let mut world = TestWorld::new(2);
    world.no_work.push(ProbeKind::Plan);
    nav.swbf2_ground_goto(
        &mut helper,
        &mut unit,
        &mut world,
        destination,
        GotoMode::Straight,
    );
    assert!(!helper.failed);
    assert_eq!(helper.destination.x, destination.x);
}

#[test]
fn refused_order_from_rest_steps_then_requests_recovery() {
    check_recovery_after_refusal(false);
}

#[test]
fn refused_order_in_motion_finishes_its_leg_then_requests_recovery() {
    check_recovery_after_refusal(true);
}

fn check_recovery_after_refusal(moving: bool) {
    let data = json::parse_bytes(br#"{"schema":2}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut unit = make_soldier(1, Vec3::default(), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    let mut world = TestWorld::new(2);
    world.flat = true;
    let end = Vec3::new(24., 0., 0.);
    let target = Vec3::new(0., 0., 15.);
    let path = if moving {
        nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, end, GotoMode::Straight);
        Some(nav.controls[h.control].executor.as_ref().unwrap().path)
    } else {
        None
    };
    // A plan that finds no work for a second is refused.
    world.no_work.push(ProbeKind::Plan);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, target, GotoMode::Route);
    for tick in 1..=60 {
        assert!(!h.failed && !nav.controls[h.control].stranded, "tick {tick}");
        nav.swbf2.as_mut().unwrap().begin_tick(tick);
        nav.swbf2_ground_tick(&mut h, &mut unit, &mut world, 1. / 60.);
    }
    assert!(h.failed && !nav.swbf2_ground_arrived(&h));
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    if let Some(path) = path {
        assert_eq!(nav.controls[h.control].executor.as_ref().unwrap().path, path);
        assert_eq!(leg.end, end);
    } else {
        assert!(leg.local && leg.end.z > 0. && leg.end.z < target.z);
    }

    let dt = 1. / 60.;
    let mut walking = 0;
    while nav.control_following(h.control) {
        assert!(walking < 600);
        assert_eq!(nav.control_status(h.control, Some(&unit)), 0.);
        assert_eq!(nav.controls[h.control].stranded_for, 0.);
        unit.position.x = (unit.position.x + 0.06).min(leg.end.x);
        unit.position.z = (unit.position.z + 0.06).min(leg.end.z);
        nav.swbf2_control_update(h.control, dt, &mut unit, &mut world);
        walking += 1;
    }
    assert!((unit.position.x - leg.end.x).hypot(unit.position.z - leg.end.z) <= 1.);
    if moving {
        assert!(walking > 300);
    }
    let mut stopped = 0;
    while nav.control_status(h.control, Some(&unit)) == 0. && stopped < 301 {
        nav.swbf2_control_update(h.control, dt, &mut unit, &mut world);
        stopped += 1;
    }
    assert!((300..=301).contains(&stopped));
    let status = nav.control_status(h.control, Some(&unit));
    assert!(crate::ai::owned::tests::soldier_requests_recovery(status),
            "recovery after {stopped} ticks without an executor (kept leg: {moving})");
}

#[test]
fn an_unchecked_changed_leg_waits_for_the_next_ticks_execution_allowance() {
    let data = json::parse_bytes(br#"{"schema":2}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut unit = make_soldier(1, Vec3::default(), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    let mut world = TestWorld::new(2);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(6., 0., 0.), GotoMode::Straight);
    let state = nav.swbf2.as_mut().unwrap();
    state.begin_tick(10);
    state.execution_checks_left = 0;
    state.legs[h.control].as_mut().unwrap().check_body = true;
    world.walls.push([3., -2., 3., 2.]);
    nav.swbf2_control_update(h.control, 1. / 60., &mut unit, &mut world);
    assert!(!nav.controls[h.control].request.valid);
    assert!(nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().check_body);
    nav.swbf2.as_mut().unwrap().begin_tick(11);
    nav.swbf2_control_update(h.control, 1. / 60., &mut unit, &mut world);
    assert!(nav.swbf2.as_ref().unwrap().legs[h.control].is_none());
    assert!(nav.controls[h.control].executor.is_none());
}

#[test]
fn a_certified_first_corridor_survives_eviction_of_its_planning_row() {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[2,0,1],"radius":2},{"position":[14,0,1],"radius":2},
                {"position":[14,0,13],"radius":2}],
        "arcs":[{"start":0,"end":1,"filterFlags":1},{"start":1,"end":2,"filterFlags":1}]
    }}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    let mut unit = make_soldier(1, Vec3::new(1.25, 0., 1.), false);
    let mut world = TestWorld::new(3);
    let entry = Vec3::new(2., 0., 1.);
    let corner = Vec3::new(14., 0., 1.);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(14., 0., 13.), GotoMode::Route);
    assert_eq!(h.cursor.as_ref().unwrap().waypoint, entry);
    let state = nav.swbf2.as_mut().unwrap();
    state.floors.invalidate();
    state.row_entries_left = 0;
    unit.position = entry;
    nav.swbf2_ground_tick(&mut h, &mut unit, &mut world, 1. / 60.);
    assert_eq!(nav.controls[h.control].executor.as_ref().unwrap().waypoint, corner);
    assert_eq!(nav.swbf2.as_ref().unwrap().row_entries_left, 0);
}

/// Two hubs in a row and an order along them that has to plan.
fn corridor_order() -> (MapNavigation, NavUnit, Swbf2GroundGoto, TestWorld, Vec3) {
    let data = json::parse_bytes(br#"{"schema":2,"plan":{
        "hubs":[{"position":[0,0,0],"radius":4},{"position":[22,0,0],"radius":4}],
        "arcs":[{"start":0,"end":1,"filterFlags":1}]
    }}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    nav.swbf2.as_mut().unwrap().clearance.prepare();
    assert!(nav.observe_collision((1, 1), &[]));
    let unit = make_soldier(1, Vec3::new(1., 0., 1.), false);
    let h = Swbf2GroundGoto::new(nav.new_control());
    (nav, unit, h, TestWorld::new(9), Vec3::new(20., 0., -1.))
}

#[test]
fn a_plan_without_work_left_goes_on_in_the_next_tick() {
    let (mut nav, mut unit, mut h, mut world, to) = corridor_order();
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    world.no_work.push(ProbeKind::Plan);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Route);
    // It waits where it is: neither refused, nor asking for recovery, and
    // with all of its checks left.
    assert!(!h.failed && h.waiting.is_some() && h.cursor.is_none());
    assert!(!nav.controls[h.control].stranded);
    assert_eq!(h.budget.drives, super::walk::SearchBudget::new().drives);
    // The same order again keeps its place.
    let place = nav.swbf2_ground_place(0, &h);
    nav.swbf2.as_mut().unwrap().begin_tick(2);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Route);
    assert_eq!(nav.swbf2_ground_place(0, &h), place);
    // Pushed a little aside, it still finishes the plan from where it began.
    unit.position.z += 0.5;
    nav.swbf2_ground_resume(&mut h, &mut unit, &mut world);
    assert_eq!(h.waiting.unwrap().from, Vec3::new(1., 0., 1.));
    world.no_work.clear();
    nav.swbf2.as_mut().unwrap().begin_tick(3);
    nav.swbf2_ground_resume(&mut h, &mut unit, &mut world);
    assert!(!h.failed && h.waiting.is_none() && h.cursor.is_some());
    assert_eq!(nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().start, unit.position);
}

#[test]
fn a_waiting_soldier_walks_its_present_leg_to_the_end() {
    let (mut nav, mut unit, mut h, mut world, to) = corridor_order();
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(3., 0., 8.), GotoMode::Straight);
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    world.no_work.push(ProbeKind::Plan);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Route);
    assert!(h.waiting.is_some() && nav.control_following(h.control));
    assert_eq!(nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().end, leg.end);
}

#[test]
fn a_leg_without_work_left_is_placed_in_a_later_tick() {
    let (mut nav, mut unit, mut h, mut world, to) = corridor_order();
    world.no_work.push(ProbeKind::Leg);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Route);
    assert!(h.cursor.is_some() && h.waiting.is_some() && !h.failed);
    assert!(nav.swbf2.as_ref().unwrap().legs[h.control].is_none());
    world.no_work.clear();
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    nav.swbf2_ground_resume(&mut h, &mut unit, &mut world);
    assert!(h.waiting.is_none() && nav.swbf2.as_ref().unwrap().legs[h.control].is_some());

    // A changed leg holds the soldier until it has been checked again.
    world.no_work.push(ProbeKind::Leg);
    nav.swbf2.as_mut().unwrap().legs[h.control].as_mut().unwrap().check_body = true;
    nav.swbf2_control_update(h.control, 1. / 60., &mut unit, &mut world);
    assert!(!nav.controls[h.control].request.valid);
    let first = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().start;
    unit.position.x += 0.3;
    nav.swbf2_control_update(h.control, 1. / 60., &mut unit, &mut world);
    assert_eq!(nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().start, first);
    world.no_work.clear();
    nav.swbf2_ground_resume(&mut h, &mut unit, &mut world);
    assert!(!nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().check_body);
    // A soldier that something else carries off does not ask again and again.
    world.no_work.push(ProbeKind::Leg);
    nav.swbf2.as_mut().unwrap().legs[h.control].as_mut().unwrap().check_body = true;
    nav.swbf2_control_update(h.control, 1. / 60., &mut unit, &mut world);
    assert!(nav.swbf2.as_ref().unwrap().legs[h.control].is_some());
    unit.position.x += 3.;
    nav.swbf2_control_update(h.control, 1. / 60., &mut unit, &mut world);
    assert!(nav.swbf2.as_ref().unwrap().legs[h.control].is_none());
}

#[test]
fn a_step_without_work_left_waits_in_place() {
    let (mut nav, mut unit, mut h, mut world, _) = corridor_order();
    let to = Vec3::new(6., 0., 1.);
    world.no_work.push(ProbeKind::Step);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Straight);
    assert!(!h.failed && h.waiting.is_some() && !nav.control_following(h.control));
    world.no_work.clear();
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    nav.swbf2_ground_resume(&mut h, &mut unit, &mut world);
    assert!(!h.failed && h.waiting.is_none() && nav.control_following(h.control));
}

#[test]
fn a_newer_order_for_the_control_ends_a_wait() {
    let (mut nav, mut unit, mut h, mut world, to) = corridor_order();
    world.no_work.push(ProbeKind::Plan);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Route);
    let mut other = Swbf2GroundGoto::new(h.control);
    nav.swbf2_ground_goto(&mut other, &mut unit, &mut world, Vec3::new(1., 0., 4.), GotoMode::Straight);
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().end;
    world.no_work.clear();
    nav.swbf2_ground_resume(&mut h, &mut unit, &mut world);
    assert!(h.waiting.is_none() && h.cursor.is_none());
    assert_eq!(nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().end, leg);
}

#[test]
fn standing_legs_go_on_first_and_then_the_longest_wait() {
    let (mut nav, mut unit, mut first, mut world, to) = corridor_order();
    let mut second = Swbf2GroundGoto::new(nav.new_control());
    let mut leg = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(&mut leg, &mut unit, &mut world, Vec3::new(1., 0., 4.), GotoMode::Straight);
    world.no_work.push(ProbeKind::Plan);
    for (tick, h) in [(5, &mut second), (6, &mut first)] {
        nav.swbf2.as_mut().unwrap().begin_tick(tick);
        nav.swbf2_ground_goto(h, &mut unit, &mut world, to, GotoMode::Route);
    }
    nav.swbf2.as_mut().unwrap().legs[leg.control].as_mut().unwrap().check_body = true;
    let mut places: Vec<_> = [&first, &second, &leg]
        .into_iter()
        .enumerate()
        .filter_map(|(slot, h)| nav.swbf2_ground_place(slot, h))
        .map(|(place, _)| place)
        .collect();
    places.sort_unstable();
    assert_eq!(places.iter().map(|p| p.2).collect::<Vec<_>>(), [2, 1, 0]);
}

#[test]
fn local_segment_fails_when_a_barrier_leaves_no_room_to_step() {
    let data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"pillar","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,1,2,0,0],"size":[0.5,3,2],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut world = TestWorld::new(7);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    // Standing right beside the inflated barrier (x = 1.0 is the boundary for r = 0.5):
    // a straight step into the barrier has no walkable progress and must fail so
    // recovery retries immediately instead of installing a zero-length leg.
    let mut blocked = make_soldier(1, Vec3::new(0.85, 0., 0.), false);
    nav.swbf2_ground_goto(
        &mut h,
        &mut blocked,
        &mut world,
        Vec3::new(10., 0., 0.),
        GotoMode::Straight,
    );
    assert!(h.failed && !nav.control_following(h.control));
    assert!(nav.swbf2.as_ref().unwrap().legs[h.control].is_none());
    // Standing 0.9 m clear of the inflated face still allows a short step up to
    // the barrier margin rather than collapsing to zero length.
    let mut gap = make_soldier(1, Vec3::new(0.1, 0., 0.), false);
    nav.swbf2_ground_goto(
        &mut h,
        &mut gap,
        &mut world,
        Vec3::new(10., 0., 0.),
        GotoMode::Straight,
    );
    assert!(!h.failed && nav.control_following(h.control));
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(leg.end.x > 0.6 && leg.end.x < 1.0);
}

#[test]
fn floors_certify_a_ramp_near_one_end_of_a_long_arc() {
    struct EndRamp;
    impl super::walk::Survey for EndRamp {
        fn floors(&mut self, hub: &super::Swbf2Hub, out: &mut Vec<Vec3>) {
            out.push(hub.pos());
        }

        fn floor(&mut self, p: Vec3) -> Option<Vec3> {
            // A narrow ramp sits around x = 10 (fraction 0.125 of an 80 m arc),
            // offset laterally at z = 5, joining the upper floor (y = 4) and
            // lower deck (y = 0).
            if (p.x - 10.).abs() <= 1.5 && (p.z - 5.).abs() <= 1.5 && p.y >= 1.5 {
                Some(Vec3::new(p.x, 2., p.z))
            } else {
                None
            }
        }

        fn crosses(&mut self, a: Vec3, b: Vec3, _: u8) -> bool {
            let on_ramp = |p: Vec3| (p.x - 10.).abs() <= 1.5 && (p.z - 5.).abs() <= 1.5;
            (on_ramp(a) || on_ramp(b)) && (a.y - b.y).abs() <= 2.5
        }
    }

    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[{"position":[0,4,0],"radius":4},{"position":[80,0,0],"radius":4}],
            "arcs":[{"start":0,"end":1,"filterFlags":1}]
        }}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let state = nav.swbf2.as_mut().unwrap();
    state.floors = super::walk::Floors::build(
        state.graph.as_ref().unwrap(),
        &mut nav.common.barriers,
        &mut EndRamp,
    )
    .unwrap();
    let upper = Vec3::new(0., 4., 0.);
    let lower = Vec3::new(80., 0., 0.);
    assert!(nav.route_distance(upper, lower, 0.45, 1, false, |_, _| true).reachable);
    assert!(nav.route_distance(lower, upper, 0.45, 1, false, |_, _| true).reachable);
}

#[test]
fn a_surveyed_proximity_door_approaches_waits_and_respects_barriers() {
    let data = json::parse_bytes(
        br#"{"schema":2,
            "barriers":[{"sourceIndex":0,"name":"gate","enabled":false,
                         "xfrm":[1,0,0,0,1,0,0,0,1,10,0,0],"size":[0.5,3,2],"flags":1}],
            "plan":{
                "hubs":[{"position":[0,0,0],"radius":3},{"position":[20,0,0],"radius":3}],
                "arcs":[{"start":0,"end":1,"filterFlags":1}]
            }}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let door_box = [9.5, -1., -2., 10.5, 3., 2.];
    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        state.floors.record_open_doors(&[0], &[door_box]);
        state.floors.changed_bodies(&[door_box]);
        state.floors.restore_open_passages(&[1]);
    }
    assert!(nav.observe_collision_with((1, 1), &[1], |_| Some(door_box)));
    let (from, to) = (Vec3::new(0., 0., 0.), Vec3::new(20., 0., 0.));
    // Unlocked closed proximity door stays plannable when its barrier is disabled.
    assert!(nav.route_distance(from, to, 0.45, 1, false, |_, _| true).reachable);

    let mut world = TestWorld::new(5).with_wall(10., -4., 10., 4.);
    let mut unit = make_soldier(1, from, false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, to, GotoMode::Route);
    // Far from the closed shutter: approaches the doorway instead of failing.
    assert!(!h.failed && nav.control_following(h.control));
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(leg.end.x >= 7.5 && leg.end.x <= 8.5, "approach leg {:?}", leg.end);

    // Finish the approach leg at the doorway while the shutter is still closed:
    // the soldier waits in place without burning plan budget.
    unit.position = leg.end;
    nav.swbf2_control_update(h.control, 1. / 60., &mut unit, &mut world);
    assert!(!nav.control_following(h.control));
    let drives_before = h.budget.drives;
    nav.swbf2.as_mut().unwrap().begin_tick(2);
    nav.swbf2_ground_tick(&mut h, &mut unit, &mut world, 1. / 60.);
    assert!(!h.failed && h.waiting.is_some());
    assert_eq!(h.budget.drives, drives_before);

    // Once the shutter opens, the waiting order resumes and crosses the doorway.
    world.walls.clear();
    assert!(nav.observe_collision_with((1, 1), &[0], |_| Some(door_box)));
    nav.swbf2.as_mut().unwrap().begin_tick(3);
    nav.swbf2_ground_resume(&mut h, &mut unit, &mut world);
    assert!(!h.failed && h.waiting.is_none());
    assert_eq!(nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().end, to);

    // A shutter that never opens times out cleanly without crossing.
    world.walls.push([10., -4., 10., 4.]);
    assert!(nav.observe_collision_with((1, 1), &[1], |_| Some(door_box)));
    let mut stuck = make_soldier(2, from, false);
    let mut hs = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2.as_mut().unwrap().begin_tick(10);
    nav.swbf2_ground_goto(&mut hs, &mut stuck, &mut world, to, GotoMode::Route);
    stuck.position = nav.swbf2.as_ref().unwrap().legs[hs.control].unwrap().end;
    nav.swbf2_control_update(hs.control, 1. / 60., &mut stuck, &mut world);
    nav.swbf2.as_mut().unwrap().begin_tick(11);
    nav.swbf2_ground_tick(&mut hs, &mut stuck, &mut world, 1. / 60.);
    assert!(!hs.failed && hs.waiting.is_some());
    for tick in 12..=71 {
        nav.swbf2.as_mut().unwrap().begin_tick(tick);
        nav.swbf2_ground_resume(&mut hs, &mut stuck, &mut world);
    }
    assert!(hs.failed && nav.swbf2.as_ref().unwrap().legs[hs.control].is_none());

    // Enabling the door's barrier blocks planning immediately.
    assert!(nav.set_barriers_enabled("gate", true));
    nav.swbf2.as_mut().unwrap().begin_tick(80);
    assert!(!nav.route_distance(from, to, 0.45, 1, false, |_, _| true).reachable);
}

#[test]
fn moving_bodies_appended_and_toggled_keep_certified_floors_and_recheck_legs() {
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[0,0,0],"radius":3},
                {"position":[20,0,0],"radius":3},
                {"position":[40,0,0],"radius":3}
            ],
            "arcs":[
                {"start":0,"end":1,"filterFlags":1},
                {"start":1,"end":2,"filterFlags":1,"jump":true}
            ]
        }}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let door_box = [9.5, -1., -2., 10.5, 3., 2.];
    let distant_static = [400., 50., 400., 410., 60., 410.];
    let craft_a = [1.5, -1., -2., 4.5, 3., 2.];
    let craft_b = [60., -1., -2., 64., 3., 2.];
    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        state.clearance.observe_order((1, 1), &[0, 3], &[1, 1]);
        state.floors.record_open_doors(&[0], &[door_box]);
        state.floors.changed_bodies(&[door_box]);
        state.floors.restore_open_passages(&[1, 1]);
        let graph = state.graph.as_ref().unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }

    let mut world = TestWorld::new(7);
    let mut ua = make_soldier(1, Vec3::new(0., 0., 0.), false);
    let mut ub = make_soldier(2, Vec3::new(20., 0., 0.), false);
    let mut ha = Swbf2GroundGoto::new(nav.new_control());
    let mut hb = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    nav.swbf2_ground_goto(&mut ha, &mut ua, &mut world, Vec3::new(5., 0., 0.), GotoMode::Straight);
    nav.swbf2_ground_goto(&mut hb, &mut ub, &mut world, Vec3::new(25., 0., 0.), GotoMode::Straight);
    assert!(nav.swbf2.as_ref().unwrap().legs[ha.control].is_some());
    assert!(nav.swbf2.as_ref().unwrap().legs[hb.control].is_some());

    let rev0 = nav.swbf2.as_ref().unwrap().revision;
    let is_moving = |body: u32| body == 1 || body == 2;
    let box_of = |body: u32| match body {
        0 => Some(door_box),
        1 => Some(craft_a),
        2 => Some(craft_b),
        3 => Some(distant_static),
        _ => None,
    };

    // Appending moving bodies after the survey bumps scene version only.
    let order = [0u32, 3, 1, 2];
    assert!(nav.observe_collision_classified((2, 1), &order, &[1, 1, 1, 0], is_moving, |i| box_of(order[i])));
    assert_eq!(nav.swbf2.as_ref().unwrap().revision, rev0);
    assert!(nav.swbf2.as_ref().unwrap().legs[ha.control].unwrap().check_body);
    assert!(!nav.swbf2.as_ref().unwrap().legs[hb.control].unwrap().check_body);

    // Surveyed corridors, open door passages, and jump certification all survive without recheck.
    let from = Vec3::new(0., 0., 0.);
    let to = Vec3::new(40., 0., 0.);
    let check_no_corridor_recheck = |nav: &mut MapNavigation, tick: u32| {
        nav.swbf2.as_mut().unwrap().begin_tick(tick);
        let q = nav.try_distance_with_clearance(from, to, 0.45, 1, false, true, |a, b, m| {
            assert_eq!(m, 0, "certified jump corridor must not be rechecked");
            assert!(
                (a.x - b.x).abs() < 15.,
                "certified corridor {a:?}->{b:?} must not be rechecked"
            );
            Some(true)
        });
        assert!(q.reachable, "route through surveyed door and jump arc must stay reachable");
    };
    check_no_corridor_recheck(&mut nav, 2);

    // Toggling an unrelated distant static body leaves surveyed doors, corridors, and rows intact.
    assert!(nav.observe_collision_classified((2, 1), &order, &[1, 0, 1, 0], is_moving, |i| box_of(order[i])));
    assert_eq!(nav.swbf2.as_ref().unwrap().revision, rev0);
    check_no_corridor_recheck(&mut nav, 3);
    // The surveyed proximity door still survives opening and closing afterwards.
    assert!(nav.observe_collision_classified((2, 1), &order, &[0, 0, 1, 0], is_moving, |i| box_of(order[i])));
    assert!(nav.observe_collision_classified((2, 1), &order, &[1, 0, 1, 0], is_moving, |i| box_of(order[i])));
    assert_eq!(nav.swbf2.as_ref().unwrap().revision, rev0);
    check_no_corridor_recheck(&mut nav, 4);

    // Toggling an appended moving body's enabled flag keeps floors intact and flags overlapping legs.
    nav.swbf2.as_mut().unwrap().legs[ha.control].as_mut().unwrap().check_body = false;
    assert!(nav.observe_collision_classified((2, 1), &order, &[1, 0, 0, 0], is_moving, |i| box_of(order[i])));
    assert_eq!(nav.swbf2.as_ref().unwrap().revision, rev0);
    assert!(nav.swbf2.as_ref().unwrap().legs[ha.control].unwrap().check_body);
    assert!(!nav.swbf2.as_ref().unwrap().legs[hb.control].unwrap().check_body);
    check_no_corridor_recheck(&mut nav, 5);

    // Re-enabling then removing an enabled moving body from the tail keeps floors intact
    // while conservatively rechecking active legs.
    assert!(nav.observe_collision_classified((2, 1), &order, &[1, 0, 1, 0], is_moving, |i| box_of(order[i])));
    nav.swbf2.as_mut().unwrap().legs[ha.control].as_mut().unwrap().check_body = false;
    nav.swbf2.as_mut().unwrap().legs[hb.control].as_mut().unwrap().check_body = false;
    let shrunk = [0u32, 3, 2];
    assert!(nav.observe_collision_classified((3, 1), &shrunk, &[1, 0, 0], is_moving, |i| box_of(shrunk[i])));
    assert_eq!(nav.swbf2.as_ref().unwrap().revision, rev0);
    assert!(nav.swbf2.as_ref().unwrap().legs[ha.control].unwrap().check_body);
    assert!(nav.swbf2.as_ref().unwrap().legs[hb.control].unwrap().check_body);
    check_no_corridor_recheck(&mut nav, 6);

    // Shifting static body 0 to a different order place still forces a full collision reset,
    // revoking the certified jump arc and bumping the navigation revision.
    let shifted = [2u32, 3, 0];
    assert!(nav.observe_collision_classified((4, 1), &shifted, &[0, 0, 1], is_moving, |i| box_of(shifted[i])));
    assert!(nav.swbf2.as_ref().unwrap().revision > rev0);
    nav.swbf2.as_mut().unwrap().begin_tick(7);
    let q = nav.try_distance_with_clearance(from, to, 0.45, 1, false, true, |_, _, _| Some(true));
    assert!(!q.reachable, "static reorder must revoke surveyed jump certification");
}

#[test]
fn point_attachments_skip_isolated_nodes_join_ramp_bends_and_clear_wide_obstacles() {
    // Hub 0 is a wide upper room with a wide central barrier (half-width 9.5 m);
    // Hub 1 sits on a balcony (y = 4) directly above an isolated overhang floor
    // (y = 0.5) with no edges; a single-bend ramp connects Hub 1 (4, 4, 0) via
    // bend (14, 1.5, 5) down to Hub 2 (84, 0, 0) on the lower deck.
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[-2,4,0],"radius":14},
                {"position":[4,4,0],"radius":3},
                {"position":[84,0,0],"radius":35}
            ],
            "arcs":[
                {"start":0,"end":1,"filterFlags":1},
                {"start":1,"end":2,"filterFlags":1}
            ]
        },"barriers":[
            {"sourceIndex":0,"enabled":true,"name":"ring","xfrm":[1,0,0, 0,1,0, 0,0,1, -10,4,0],"size":[1.0,2.0,7.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();

    struct OverhangRampSurvey;
    impl super::walk::Survey for OverhangRampSurvey {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            match hub.index {
                0 => out.push(Vec3::new(-2., 4., 0.)),
                1 => {
                    // Upper balcony floor (connected) + isolated floor underneath (no edges).
                    out.push(Vec3::new(4., 4., 0.));
                    out.push(Vec3::new(4., 0.5, 0.));
                }
                2 => out.push(Vec3::new(84., 0., 0.)),
                _ => {}
            }
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            // Expose the ramp bottom near (14, 1.5, 5) for Hub 1 -> Hub 2.
            if (near.x - 14.).abs() < 4. && (near.z - 5.).abs() < 3. && near.y < 3.5 {
                Some(Vec3::new(14., 1.5, 5.))
            } else {
                None
            }
        }
        fn crosses(&mut self, from: Vec3, to: Vec3, _flags: u8) -> bool {
            let isolated = |p: Vec3| {
                (p.x - 4.).abs() < 0.2 && (p.y - 0.5).abs() < 0.2 && (p.z - 0.).abs() < 0.2
            };
            if isolated(from) || isolated(to) {
                return false;
            }
            // Straight balcony-to-deck drop without the ramp bend is blocked.
            if ((from.y - 4.).abs() < 0.2 && (to.y - 0.).abs() < 0.2)
                || ((from.y - 0.).abs() < 0.2 && (to.y - 4.).abs() < 0.2)
            {
                return false;
            }
            true
        }
    }

    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors =
            super::walk::Floors::build(graph, &mut nav.common.barriers, &mut OverhangRampSurvey)
                .unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }

    // Start at (-18, 4, 0), separated from Hub 0 (-2, 4, 0) by the 19 m-wide barrier
    // (requires the +-11.5 m wide room bend). Target sits on the lower deck at (22, 0, 12),
    // which is 63 m from Hub 2 (outside node_reach = 47 m), within node_reach of the isolated
    // overhang node (4, 0.5, 0), and within CORRIDOR_REACH of the ramp bend (14, 1.5, 5).
    let from = Vec3::new(-18., 4., 0.);
    let to = Vec3::new(22., 0., 12.);
    let mut probed_isolated = false;
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    let q = nav.try_distance_with_clearance(from, to, 0.45, 1, false, true, |a, b, _| {
        let isolated = |p: Vec3| {
            (p.x - 4.).abs() < 0.2 && (p.y - 0.5).abs() < 0.2 && (p.z - 0.).abs() < 0.2
        };
        if isolated(a) || isolated(b) {
            probed_isolated = true;
        }
        let direct_drop = ((a.y - 4.).abs() < 0.2 && (b.y - 0.).abs() < 0.2)
            || ((a.y - 0.).abs() < 0.2 && (b.y - 4.).abs() < 0.2);
        Some(!direct_drop)
    });
    assert!(
        !probed_isolated,
        "isolated floor node outside the start component must be skipped with zero physical checks"
    );
    assert!(
        q.reachable,
        "route must detour around the wide room barrier, skip the isolated overhang floor, and join the ramp bend: {q:?}"
    );
}

#[test]
fn may_connect_and_point_attachments_agree_on_bent_ramps_far_from_chord() {
    // Hub 0 sits on an upper balcony (0, 6, 0) and Hub 1 on a lower deck (240, 0, 0),
    // connected only by a bent ramp whose lower bend lies at (30, 0, 8) (offset +8 m
    // from the chord). A target at (30, 0, 23.8) is 38.8 m from Hub 0 (> node_reach + 8 = 35 m),
    // 211 m from Hub 1, and 24.37 m from the straight chord (> CORRIDOR_REACH + 8 = 24 m),
    // yet only 15.8 m from the bent ramp segment (<= CORRIDOR_REACH = 16 m).
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[0,6,0],"radius":3},
                {"position":[240,0,0],"radius":3}
            ],
            "arcs":[
                {"start":0,"end":1,"filterFlags":1}
            ]
        },"barriers":[]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();

    struct BentRampSurvey;
    impl super::walk::Survey for BentRampSurvey {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            match hub.index {
                0 => out.push(Vec3::new(0., 6., 0.)),
                1 => out.push(Vec3::new(240., 0., 0.)),
                _ => {}
            }
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            ((near.x - 30.).abs() < 1.0 && (near.z - 8.).abs() < 1.0)
                .then_some(Vec3::new(30., 0., 8.))
        }
        fn crosses(&mut self, from: Vec3, to: Vec3, _flags: u8) -> bool {
            // Direct balcony-to-deck chord is blocked; the bent ramp via (30, 0, 8) is clear.
            !((from.y - to.y).abs() > 1.0 && from.z.abs() < 1.0 && to.z.abs() < 1.0)
        }
    }

    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors =
            super::walk::Floors::build(graph, &mut nav.common.barriers, &mut BentRampSurvey)
                .unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }

    let upper = Vec3::new(0., 6., 0.);
    let deck_near_bend = Vec3::new(30., 0., 23.8);
    let deck_second_half = Vec3::new(135., 0., 20.0);
    let deck_unreachable = Vec3::new(30., 0., 45.0);

    {
        let state = nav.swbf2.as_ref().unwrap();
        let graph = state.graph.as_ref().unwrap();
        assert!(
            state.floors.may_connect(graph, upper, deck_near_bend, 0.0, 1),
            "may_connect must include bent corridor segments with zero slack"
        );
        assert!(
            state.floors.may_connect(graph, upper, deck_near_bend, 8.0, 1),
            "may_connect must include bent corridor segments with slack"
        );
        assert!(
            state.floors.may_connect(graph, deck_near_bend, upper, 0.0, 1),
            "may_connect must include bent corridor segments for entry"
        );
        assert!(
            state.floors.may_connect(graph, upper, deck_second_half, 0.0, 1),
            "may_connect must include the second half of a bent corridor"
        );
        assert!(
            !state.floors.may_connect(graph, upper, deck_unreachable, 0.0, 1),
            "points beyond CORRIDOR_REACH of every bent segment must not match"
        );
    }

    let walk = |a: Vec3, b: Vec3, _: u8| {
        let chord_drop = (a.y - b.y).abs() > 1.0 && a.z.abs() < 1.0 && b.z.abs() < 1.0;
        Some(!chord_drop)
    };
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    let down = nav.try_distance_with_clearance(upper, deck_near_bend, 0.45, 1, false, true, walk);
    assert!(
        down.reachable,
        "route down bent ramp to deck point >24 m from chord must succeed: {down:?}"
    );
    let down_second =
        nav.try_distance_with_clearance(upper, deck_second_half, 0.45, 1, false, true, walk);
    assert!(
        down_second.reachable,
        "route down bent ramp to second-half deck point must succeed: {down_second:?}"
    );
    let up = nav.try_distance_with_clearance(deck_near_bend, upper, 0.45, 1, false, true, walk);
    assert!(
        up.reachable,
        "route up bent ramp from deck point >24 m from chord must succeed: {up:?}"
    );

    let mut checks = 0usize;
    let miss = nav.try_distance_with_clearance(upper, deck_unreachable, 0.45, 1, false, true, |_, _, _| {
        checks += 1;
        Some(true)
    });
    assert!(!miss.reachable);
    assert_eq!(checks, 0, "may_connect miss must spend zero physical checks");
}

#[test]
fn point_attachments_skip_isolated_start_nodes_and_snap_bent_ramp_exits() {
    // Hub 0 (0, 4, 0) has a real upper floor plus 4 isolated ceiling/overhang nodes
    // that are closer to a soldier at (20, 4, 0) than Hub 0's connected floor is.
    // Hub 0 connects via a downward bent ramp (bend at (20, 1.5, 6)) to Hub 1 (80, 0, 0).
    // A target on the lower deck at (50, 0, 6) is beyond node_reach (27 m) from both hubs
    // and sits next to the lower half (bend -> Hub 1) where the linear chord floats at y = 0.75.
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[0,4,0],"radius":3},
                {"position":[80,0,0],"radius":3}
            ],
            "arcs":[
                {"start":0,"end":1,"filterFlags":1}
            ]
        },"barriers":[]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();

    struct IsolatedAndBentSurvey;
    impl super::walk::Survey for IsolatedAndBentSurvey {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            match hub.index {
                0 => {
                    out.push(Vec3::new(0., 4., 0.));
                    // Four closer disconnected ceiling/lip floor samples at the same hub.
                    out.push(Vec3::new(14., 4.1, 0.));
                    out.push(Vec3::new(15., 4.2, 0.));
                    out.push(Vec3::new(16., 4.3, 0.));
                    out.push(Vec3::new(17., 4.4, 0.));
                }
                1 => out.push(Vec3::new(80., 0., 0.)),
                _ => {}
            }
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            ((near.x - 20.).abs() < 1.5 && (near.z - 6.).abs() < 2.5)
                .then_some(Vec3::new(20., 1.5, 6.))
        }
        fn crosses(&mut self, from: Vec3, to: Vec3, _flags: u8) -> bool {
            let isolated = |p: Vec3| p.x >= 13.5 && p.x <= 17.5 && p.y >= 4.05;
            if isolated(from) || isolated(to) {
                return false;
            }
            !((from.y - to.y).abs() > 2.6 && from.z.abs() < 1.0 && to.z.abs() < 1.0)
        }
    }

    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors = super::walk::Floors::build(
            graph,
            &mut nav.common.barriers,
            &mut IsolatedAndBentSurvey,
        )
        .unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }

    struct DeckSnapWorld {
        base: TestWorld,
        probed_isolated: bool,
    }
    impl GotoWorld for DeckSnapWorld {
        fn time(&self) -> f64 { self.base.time() }
        fn random_a(&mut self) -> &mut dyn crate::ai::random::Random { self.base.random_a() }
        fn random_b(&mut self) -> &mut dyn crate::ai::random::Random { self.base.random_b() }
        fn resolve(&mut self, r: UnitRef) -> Option<Resolved> { self.base.resolve(r) }
        fn leader(&mut self, r: UnitRef) -> Option<NavUnit> { self.base.leader(r) }
        fn remembered(&mut self, target: u64) -> Option<Vec3> { self.base.remembered(target) }
        fn clear_above(&mut self, from: Vec3, unit: &NavUnit) -> bool { self.base.clear_above(from, unit) }
        fn follow_far(&mut self) -> f64 { self.base.follow_far() }
    }
    impl Swbf2GotoWorld for DeckSnapWorld {
        fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld { self }
        fn foot(&mut self, p: Vec3) -> Option<Vec3> {
            // The flat deck under the lower half of the bent ramp sits at y = 0.
            if p.x > 25. && p.y >= -0.5 && p.y <= 1.25 {
                Some(Vec3::new(p.x, 0., p.z))
            } else {
                Some(p)
            }
        }
        fn line_of_walk(&mut self, a: Vec3, b: Vec3, _unit: &NavUnit) -> bool {
            let isolated = |p: Vec3| p.x >= 13.5 && p.x <= 17.5 && p.y >= 4.05;
            if isolated(a) || isolated(b) {
                self.probed_isolated = true;
                return false;
            }
            // Reject any floating deck waypoint whose y was not snapped to y = 0.
            if (a.x > 25. && a.y.abs() > 0.25) || (b.x > 25. && b.y.abs() > 0.25) {
                return false;
            }
            // Block any direct balcony-to-deck drop that skips the intermediate ramp bend.
            (a.y - b.y).abs() <= 2.6
        }
    }

    let mut world = DeckSnapWorld {
        base: TestWorld::new(3),
        probed_isolated: false,
    };
    let mut unit = make_soldier(1, Vec3::new(19., 4., 0.), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    let target = Vec3::new(50., 0., 6.);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, target, GotoMode::Route);
    assert!(
        !world.probed_isolated,
        "isolated start nodes outside target component must be skipped"
    );
    assert!(
        !h.failed && nav.control_following(h.control),
        "route must attach past isolated start nodes and exit the snapped lower half of the bent ramp"
    );
    let cursor = h.cursor.as_ref().unwrap();
    assert_eq!(
        cursor.exit_bend.points().len(),
        2,
        "exiting the lower half of a downward bent ramp must queue both bend and deck projection: {:?}",
        cursor.exit_bend
    );
    assert!(
        cursor.exit_bend.points()[1].y.abs() < 1e-6,
        "deck projection must be snapped to the deck floor, got {:?}",
        cursor.exit_bend.points()[1]
    );
}

#[test]
fn long_rising_corridor_detours_around_a_parked_obstacle_near_start() {
    // A 42 m rising corridor from (0, 0, 0) to (42, 3, 0) has a parked craft
    // blocking the straight line 6 m ahead of start (spanning z in [-3.5, 3.5]).
    // Near-start bypass candidates must clear the obstacle within the first few probes
    // rather than burning the leg budget on small crease shifts.
    let from = Vec3::new(0., 0., 0.);
    let to = Vec3::new(42., 3., 0.);
    let blocked = |a: Vec3, b: Vec3| {
        // Segment crosses x = 6 with |z| <= 3.5.
        let min_x = v8math::min(a.x, b.x);
        let max_x = v8math::max(a.x, b.x);
        if min_x <= 6. && max_x >= 6. && (b.x - a.x).abs() > 1e-6 {
            let t = (6. - a.x) / (b.x - a.x);
            let z = a.z + t * (b.z - a.z);
            z.abs() <= 3.5
        } else {
            false
        }
    };
    let found = super::walk::handoff_bends(from, to)
        .take(16)
        .position(|mid| !blocked(from, mid) && !blocked(mid, to));
    assert!(
        found.is_some(),
        "long rising corridor must bypass a near-start obstacle within 16 probes"
    );
}

#[test]
fn local_segment_never_installs_unchecked_midair_or_wall_leg_from_barrier_ejection() {
    // A rooftop spans x <= 1.2 at y = 5.5; east of x = 1.2 is open air.
    // A barrier spans x in [0.5, 1.5] (inflated [0.0, 2.0] for r = 0.5).
    // A soldier at (1.1, 5.5, 0.0) stands on the roof inside the barrier margin:
    // barriers.correct ejects east to x = 2.01 (off the roof into midair!).
    let data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"parapet","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,1.0,0,0],"size":[0.5,3,2],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut probe = Vec3::new(1.1, 5.5, 0.);
    assert!(
        nav.common.barriers.correct(&mut probe, 0.5, 1) && probe.x > 1.2,
        "test setup must have barriers.correct eject off the roof into midair: {probe:?}"
    );

    struct RoofWorld {
        base: TestWorld,
    }
    impl GotoWorld for RoofWorld {
        fn time(&self) -> f64 { self.base.time() }
        fn random_a(&mut self) -> &mut dyn crate::ai::random::Random { self.base.random_a() }
        fn random_b(&mut self) -> &mut dyn crate::ai::random::Random { self.base.random_b() }
        fn resolve(&mut self, r: UnitRef) -> Option<Resolved> { self.base.resolve(r) }
        fn leader(&mut self, r: UnitRef) -> Option<NavUnit> { self.base.leader(r) }
        fn remembered(&mut self, target: u64) -> Option<Vec3> { self.base.remembered(target) }
        fn clear_above(&mut self, from: Vec3, unit: &NavUnit) -> bool { self.base.clear_above(from, unit) }
        fn follow_far(&mut self) -> f64 { self.base.follow_far() }
    }
    impl Swbf2GotoWorld for RoofWorld {
        fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld { self }
        fn foot(&mut self, p: Vec3) -> Option<Vec3> {
            (p.x <= 1.2).then_some(Vec3::new(p.x, 5.5, p.z))
        }
        fn line_of_walk(&mut self, a: Vec3, b: Vec3, _unit: &NavUnit) -> bool {
            a.x <= 1.2 && b.x <= 1.2
        }
    }

    let mut world = RoofWorld { base: TestWorld::new(7) };
    let mut unit = make_soldier(1, Vec3::new(1.1, 5.5, 0.), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());

    // Straight step east off the roof must fail, never install an unchecked leg to x = 2.01.
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(5., 5.5, 0.), GotoMode::Straight);
    assert!(
        h.failed && nav.swbf2.as_ref().unwrap().legs[h.control].is_none(),
        "midair barrier ejection must not be installed as a leg: {:?}",
        nav.swbf2.as_ref().unwrap().legs[h.control]
    );

    // Straight step west along the supported roof out of the barrier must succeed onto clear floor.
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(-4., 5.5, 0.), GotoMode::Straight);
    assert!(
        !h.failed && nav.control_following(h.control),
        "soldier inside a rooftop barrier must be able to walk inward onto clear roof floor"
    );
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(leg.end.x < 0., "exit leg must land clear of the barrier, got {:?}", leg.end);
}

#[test]
fn local_segment_exits_overlapping_barriers_onto_walkable_floor() {
    // Three overlapping barriers cover the soldier at (0, 0, 0) so every single-barrier
    // ejection candidate of barrier 0 lies inside another barrier (barriers.correct fails),
    // while clear walkable ground begins beyond x = 3.0.
    let data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"b0","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[1.0,3,1.0],"flags":1},
            {"sourceIndex":1,"name":"b1","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0.5,0,0],"size":[2.0,3,3.0],"flags":1},
            {"sourceIndex":2,"name":"b2","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,-1.5,0,0],"size":[1.5,3,3.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut probe = Vec3::new(0., 0., 0.);
    assert!(
        !nav.common.barriers.correct(&mut probe, 0.5, 1),
        "test setup must have barriers.correct fail inside overlapping barriers"
    );

    let mut world = TestWorld::new(9);
    let mut unit = make_soldier(1, Vec3::new(0., 0., 0.), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(6., 0., 0.), GotoMode::Straight);
    assert!(
        !h.failed && nav.control_following(h.control),
        "soldier inside overlapping barriers must walk out onto clear floor"
    );
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(
        nav.common.barriers.point_clear(leg.end, 0.5, 1).0 && leg.end.x >= 3.0,
        "exit leg must land clear of all overlapping barriers: {:?}",
        leg.end
    );
}

#[test]
fn local_segment_steps_off_bounded_awning_drop_and_jets_when_stranded() {
    let data = json::parse_bytes(br#"{"schema":2,"barriers":[]}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();

    struct AwningWorld {
        base: TestWorld,
        awning_y: f64,
    }
    impl GotoWorld for AwningWorld {
        fn time(&self) -> f64 { self.base.time() }
        fn random_a(&mut self) -> &mut dyn crate::ai::random::Random { self.base.random_a() }
        fn random_b(&mut self) -> &mut dyn crate::ai::random::Random { self.base.random_b() }
        fn resolve(&mut self, r: UnitRef) -> Option<Resolved> { self.base.resolve(r) }
        fn leader(&mut self, r: UnitRef) -> Option<NavUnit> { self.base.leader(r) }
        fn remembered(&mut self, target: u64) -> Option<Vec3> { self.base.remembered(target) }
        fn clear_above(&mut self, _from: Vec3, _unit: &NavUnit) -> bool { true }
        fn follow_far(&mut self) -> f64 { self.base.follow_far() }
    }
    impl Swbf2GotoWorld for AwningWorld {
        fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld { self }
        fn foot(&mut self, p: Vec3) -> Option<Vec3> {
            let on_awning = p.x.abs() <= 0.4 && p.z.abs() <= 0.4;
            if on_awning && (p.y - self.awning_y).abs() <= 1.25 {
                Some(Vec3::new(p.x, self.awning_y, p.z))
            } else if !on_awning && p.y.abs() <= 1.25 {
                Some(Vec3::new(p.x, 0., p.z))
            } else {
                None
            }
        }
        fn step_floor(&mut self, p: Vec3) -> Option<Vec3> {
            let on_awning = p.x.abs() <= 0.4 && p.z.abs() <= 0.4;
            if on_awning && (p.y - self.awning_y).abs() <= 0.75 {
                Some(Vec3::new(p.x, self.awning_y, p.z))
            } else if !on_awning && p.y.abs() <= 0.75 {
                Some(Vec3::new(p.x, 0., p.z))
            } else {
                None
            }
        }
        fn combat_floor(&mut self, p: Vec3) -> Option<Vec3> {
            if p.x.abs() <= 0.4 && p.z.abs() <= 0.4 {
                Some(Vec3::new(p.x, self.awning_y, p.z))
            } else if p.y >= 0. && p.y <= 2.5 {
                Some(Vec3::new(p.x, 0., p.z))
            } else {
                None
            }
        }
        fn approach_floor(&mut self, p: Vec3) -> Option<Vec3> {
            self.combat_floor(p).or_else(|| {
                (p.y >= 0. && p.y <= 8.5).then_some(Vec3::new(p.x, 0., p.z))
            })
        }
        fn crosses(&mut self, a: Vec3, b: Vec3, _unit: &NavUnit, movement: u8) -> bool {
            let drop_h = a.y - b.y;
            if movement == super::walk::DROP {
                (0. ..=super::walk::MAX_DROP).contains(&drop_h)
            } else {
                drop_h.abs() <= 0.75
            }
        }
        fn line_of_walk(&mut self, a: Vec3, b: Vec3, _unit: &NavUnit) -> bool {
            (a.y - b.y).abs() <= 0.75
        }
    }

    // 2.2 m awning: bounded DROP fallback steps off safely onto y = 0.
    let mut world = AwningWorld { base: TestWorld::new(11), awning_y: 2.2 };
    let mut unit = make_soldier(1, Vec3::new(0., 2.2, 0.), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(5., 2.2, 0.), GotoMode::Straight);
    assert!(
        !h.failed && nav.control_following(h.control),
        "soldier on a 2.2 m awning must step off via bounded DROP"
    );
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert_eq!(leg.movement, super::walk::DROP);
    assert!(leg.end.y.abs() < 1e-6 && leg.end.x >= 1.0, "unexpected landing {:?}", leg.end);

    // 8.0 m cliff (> MAX_DROP): must refuse to walk off, and a jet-capable unit stranded >= 3 s
    // triggers a jet recovery instead of starving random_behave's retry timer forever.
    world.awning_y = 8.0;
    let mut jet_unit = make_soldier(2, Vec3::new(0., 8.0, 0.), true);
    let mut h2 = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(&mut h2, &mut jet_unit, &mut world, Vec3::new(5., 8.0, 0.), GotoMode::Straight);
    assert!(h2.failed && !nav.controls[h2.control].jet.pending);
    for _ in 0..186 {
        nav.swbf2_control_update(h2.control, 1. / 60., &mut jet_unit, &mut world);
    }
    nav.swbf2_ground_goto(&mut h2, &mut jet_unit, &mut world, Vec3::new(-5., 8.0, 0.), GotoMode::Straight);
    assert!(h2.failed && nav.swbf2.as_ref().unwrap().legs[h2.control].is_none());
    assert!(
        nav.controls[h2.control].jet.pending,
        "stranded jet-capable unit must request a jet recovery when Straight fails repeatedly"
    );
    assert_eq!(nav.controls[h2.control].stranded_for, 0.0);
}

#[test]
fn point_attachments_route_a_soldier_between_chairs_and_pillar_and_along_multi_bend_passages() {
    // Part A: An invented command room with a central pillar and flanking console
    // barriers surrounding a soldier at (0, 0, -5.5), while the room's hub N0 sits
    // on the far side at (0, 0, 7.5). Every straight line and single perpendicular
    // offset clips either the pillar or a flanking barrier; only a 2-corner detour
    // around the barriers reaches N0.
    // Part B: A multi-bend indoor doorway passage connects Hub 0 (0, 0, 7.5) to
    // Hub 1 (40, 0, 7.5) through an offset zig-zag vestibule; a soldier standing
    // beside an interior segment of that passage (screened from both hubs by walls)
    // must match may_connect and attach onto that interior segment.
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[0,0,7.5],"radius":12},
                {"position":[40,0,7.5],"radius":6}
            ],
            "arcs":[
                {"start":0,"end":1,"filterFlags":1}
            ]
        },"barriers":[
            {"sourceIndex":0,"name":"pillar","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[1.6,3,1.6],"flags":1},
            {"sourceIndex":1,"name":"chair_nw","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,-5.0,0,-1.5],"size":[1.5,3,1.5],"flags":1},
            {"sourceIndex":2,"name":"chair_ne","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,5.0,0,-1.5],"size":[1.5,3,1.5],"flags":1},
            {"sourceIndex":3,"name":"chair_sw","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,-5.0,0,3.5],"size":[1.5,3,1.5],"flags":1},
            {"sourceIndex":4,"name":"chair_se","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,5.0,0,3.5],"size":[1.5,3,1.5],"flags":1},
            {"sourceIndex":5,"name":"wing_l","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,-9.5,0,1.0],"size":[2.5,3,6.0],"flags":1},
            {"sourceIndex":6,"name":"wing_r","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,9.5,0,1.0],"size":[2.5,3,6.0],"flags":1},
            {"sourceIndex":7,"name":"door_wall_a","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,16,0,5.5],"size":[1.0,3,6.5],"flags":1},
            {"sourceIndex":8,"name":"door_wall_b","enabled":true,"xfrm":[1,0,0,0,1,0,0,0,-1,28,0,13.5],"size":[1.0,3,6.5],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();

    struct RoomAndPassageSurvey;
    impl super::walk::Survey for RoomAndPassageSurvey {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            match hub.index {
                0 => out.push(Vec3::new(0., 0., 7.5)),
                1 => out.push(Vec3::new(40., 0., 7.5)),
                _ => {}
            }
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            Some(Vec3::new(near.x, 0., near.z))
        }
        fn crosses(&mut self, _from: Vec3, _to: Vec3, _flags: u8) -> bool {
            true
        }
        fn doorway(&self, _from: Vec3, _to: Vec3) -> Option<Vec3> {
            Some(Vec3::new(22., 0., 16.))
        }
    }

    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors =
            super::walk::Floors::build(graph, &mut nav.common.barriers, &mut RoomAndPassageSurvey)
                .unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }

    let walk = |_a: Vec3, _b: Vec3, _: u8| Some(true);
    let goal = Vec3::new(40., 0., 7.5);
    nav.swbf2.as_mut().unwrap().begin_tick(1);

    // Part A: soldier wedged behind the pillar and chair ring attaches via 2-corner barrier detour.
    let behind_pillar = Vec3::new(0., 0., -5.5);
    let r_pillar = nav.try_distance_with_clearance(behind_pillar, goal, 0.45, 1, false, true, walk);
    assert!(
        r_pillar.reachable,
        "soldier wedged between chairs and central pillar must attach and route out: {r_pillar:?}"
    );

    // Part B: soldier standing in the vestibule beside the interior passage segment
    // (screened from Hub 0 by door_wall_a and from Hub 1 by door_wall_b).
    let in_vestibule = Vec3::new(22., 0., 15.5);
    let r_vestibule =
        nav.try_distance_with_clearance(in_vestibule, goal, 0.45, 1, false, true, walk);
    assert!(
        r_vestibule.reachable,
        "soldier beside an interior multi-bend passage segment must attach and route out: {r_vestibule:?}"
    );
}

#[test]
fn local_segment_never_crosses_solid_barrier_body_from_inside_a_margin() {
    // Case A: soldier stands in barrier A's margin only (outside body r = 0,
    // inside inflated r = 0.5) and receives a Straight order through A's body.
    // The installed correction leg must never cross A's solid body.
    let data_a = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"wall_a","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[1.0,3,2.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_a = MapNavigation::new_swbf2(&data_a, None, None, Default::default()).unwrap();
    let mut world = TestWorld::new(13);
    let mut unit = make_soldier(1, Vec3::new(-1.25, 0., 0.), false);
    let mut h = Swbf2GroundGoto::new(nav_a.new_control());
    nav_a.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(4., 0., 0.), GotoMode::Straight);
    assert!(
        !h.failed && nav_a.control_following(h.control),
        "margin-trapped soldier should still eject outward onto clear floor"
    );
    let leg_a = nav_a.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(
        leg_a.end.x < -1.0,
        "correction leg from margin must eject away from barrier body, not cross it: {:?}",
        leg_a.end
    );

    // Case B: soldier starts inside barrier A's body and receives a Straight
    // order straight across a distinct solid barrier B's body.
    let data_b = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"box_a","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[0.8,3,0.8],"flags":1},
            {"sourceIndex":1,"name":"wall_b","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,2.5,0,0],"size":[1.0,3,3.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_b = MapNavigation::new_swbf2(&data_b, None, None, Default::default()).unwrap();
    let mut unit_b = make_soldier(2, Vec3::new(0., 0., 0.), false);
    let mut h_b = Swbf2GroundGoto::new(nav_b.new_control());
    nav_b.swbf2_ground_goto(
        &mut h_b,
        &mut unit_b,
        &mut world,
        Vec3::new(6., 0., 0.),
        GotoMode::Straight,
    );
    if let Some(leg_b) = nav_b.swbf2.as_ref().unwrap().legs[h_b.control] {
        assert!(
            leg_b.end.x < 1.5,
            "correction leg must never cross another solid barrier's body: {:?}",
            leg_b.end
        );
    }
}

#[test]
fn local_segment_uses_checked_barrier_ejection_when_requested_ray_stays_blocked() {
    // Long north-south wall barrier (body x in [-1, 1], z in [-20, 20]; inflated
    // x in [-1.5, 1.5] for r = 0.5). A soldier in its east margin at (1.25, 0, 0)
    // ordered Straight north to (1.25, 0, 6) stays inside the margin along the
    // entire requested ray, so only the checked barrier-ejection branch can step
    // east onto clear floor.
    let data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"long_wall","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[1.0,3,20.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut world = TestWorld::new(17);
    let mut unit = make_soldier(1, Vec3::new(1.25, 0., 0.), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(
        &mut h,
        &mut unit,
        &mut world,
        Vec3::new(1.25, 0., 6.),
        GotoMode::Straight,
    );
    assert!(
        !h.failed && nav.control_following(h.control),
        "checked barrier ejection must rescue a soldier whose requested ray stays in the margin"
    );
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(
        nav.common.barriers.point_clear(leg.end, 0.5, 1).0
            && leg.end.x >= 1.5
            && leg.end.z.abs() < 0.05,
        "ejection leg must step directly out to clear east floor: {:?}",
        leg.end
    );
}

#[test]
fn local_segment_drops_off_rooftop_barrier_ejection_and_escapes_corner_margins() {
    // Part A: Building rooftop (y = 4.5, |x| <= 4, |z| <= 4) covered by its own
    // solid barrier (size [4, 3, 4]), surrounded by a 1 m sloped trim (no flat
    // support at 4 < |z| <= 5) and flat street (y = 0) beyond |z| > 5.
    let roof_data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"building_roof","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[4.0,3,4.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_roof = MapNavigation::new_swbf2(&roof_data, None, None, Default::default()).unwrap();

    let mut roof_world = TestWorld::new(19).with_rooftop(RooftopSpec {
        roof_half: 4.0,
        street_edge: 5.0,
        roof_y: 4.5,
        allow_drop: true,
        allow_jump: false,
    });
    let mut roof_unit = make_soldier(1, Vec3::new(0., 4.5, 2.0), false);
    let mut h_roof = Swbf2GroundGoto::new(nav_roof.new_control());
    nav_roof.swbf2_ground_goto(
        &mut h_roof,
        &mut roof_unit,
        &mut roof_world,
        Vec3::new(0., 4.5, -2.0),
        GotoMode::Straight,
    );
    assert!(
        !h_roof.failed && nav_roof.control_following(h_roof.control),
        "soldier trapped on a barrier-covered rooftop must drop onto the street past the trim"
    );
    let roof_leg = nav_roof.swbf2.as_ref().unwrap().legs[h_roof.control].unwrap();
    assert_eq!(roof_leg.movement, super::walk::DROP);
    assert!(
        roof_leg.end.y.abs() < 1e-6 && roof_leg.end.z > 5.0,
        "rooftop drop must land on clear street floor: {:?}",
        roof_leg.end
    );

    // Part B: L-shaped double-barrier corner margin where barriers.correct ejects
    // through wall_e's solid body while southwest is clear walkable floor.
    let corner_data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"wall_e","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,1.0,0,-2.0],"size":[0.3,3,4.0],"flags":1},
            {"sourceIndex":1,"name":"wall_n","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,-2.0,0,1.0],"size":[3.0,3,0.2],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_corner =
        MapNavigation::new_swbf2(&corner_data, None, None, Default::default()).unwrap();
    let mut corner_world = TestWorld::new(23);
    let mut corner_unit = make_soldier(2, Vec3::new(0.45, 0., 0.45), false);
    let mut h_corner = Swbf2GroundGoto::new(nav_corner.new_control());
    nav_corner.swbf2_ground_goto(
        &mut h_corner,
        &mut corner_unit,
        &mut corner_world,
        Vec3::new(4.0, 0., 0.45),
        GotoMode::Straight,
    );
    assert!(
        !h_corner.failed && nav_corner.control_following(h_corner.control),
        "soldier wedged in an L-corner barrier margin must escape onto open floor"
    );
    let corner_leg = nav_corner.swbf2.as_ref().unwrap().legs[h_corner.control].unwrap();
    assert!(
        nav_corner.common.barriers.point_clear(corner_leg.end, 0.5, 1).0
            && !nav_corner
                .common
                .barriers
                .segment_crosses_solid_body_outside_start(
                    Vec3::new(0.45, 0., 0.45),
                    corner_leg.end,
                    1,
                ),
        "corner escape leg must land clear without crossing either wall body: {:?}",
        corner_leg.end
    );
}

#[test]
fn route_order_from_inside_barrier_steps_out_instead_of_freezing() {
    // A soldier standing inside a barrier (such as a hero who landed on a
    // barrier-covered roof or wedged into an L-corner margin) receiving a
    // Route order cannot attach directly to the planning graph. Instead of
    // freezing in place with no leg, continue_walk must hand off to
    // local_segment so the soldier steps onto clear floor first.
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[0,0,8],"radius":12},
                {"position":[30,0,8],"radius":12}
            ],
            "arcs":[{"start":0,"end":1,"filterFlags":1}]
        },"barriers":[
            {"sourceIndex":0,"name":"wall_e","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,1.0,0,-2.0],"size":[0.3,3,4.0],"flags":1},
            {"sourceIndex":1,"name":"wall_n","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,-2.0,0,1.0],"size":[3.0,3,0.2],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    struct FlatSurvey;
    impl super::walk::Survey for FlatSurvey {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            out.push(Vec3::new(hub.x, hub.y, hub.z));
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            Some(Vec3::new(near.x, 0., near.z))
        }
        fn crosses(&mut self, _from: Vec3, _to: Vec3, _flags: u8) -> bool {
            true
        }
    }
    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors =
            super::walk::Floors::build(graph, &mut nav.common.barriers, &mut FlatSurvey).unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }

    let mut world = TestWorld::new(29);
    let mut unit = make_soldier(1, Vec3::new(0.45, 0., 0.45), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    nav.swbf2_ground_goto(
        &mut h,
        &mut unit,
        &mut world,
        Vec3::new(30., 0., 8.),
        GotoMode::Route,
    );
    assert!(
        nav.control_following(h.control) && nav.swbf2.as_ref().unwrap().legs[h.control].is_some(),
        "Route order from inside a barrier margin must install a checked exit leg instead of freezing"
    );
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(
        nav.common.barriers.point_clear(leg.end, 0.5, 1).0
            && !nav
                .common
                .barriers
                .segment_crosses_solid_body_outside_start(Vec3::new(0.45, 0., 0.45), leg.end, 1),
        "installed exit leg must land clear without crossing any solid barrier body: {:?}",
        leg.end
    );
}

#[test]
fn route_order_from_rooftop_barrier_above_street_hub_drops_to_street() {
    // A soldier stranded on a barrier-covered building rooftop 4.5 m directly
    // above a street hub sits beyond the zero-slack storey band of that hub, so
    // a zero-slack connectivity pre-check would treat the rooftop as
    // disconnected and freeze the Route order. Allowing MAX_DROP vertical slack
    // on the trapped start lets continue_walk hand off to local_segment and
    // drop safely to the clear street below.
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[0,0,2],"radius":12},
                {"position":[30,0,2],"radius":12}
            ],
            "arcs":[{"start":0,"end":1,"filterFlags":1}]
        },"barriers":[
            {"sourceIndex":0,"name":"roof_block","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[4.0,3.0,4.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    struct StreetSurvey;
    impl super::walk::Survey for StreetSurvey {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            out.push(Vec3::new(hub.x, hub.y, hub.z));
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            Some(Vec3::new(near.x, 0., near.z))
        }
        fn crosses(&mut self, _from: Vec3, _to: Vec3, _flags: u8) -> bool {
            true
        }
    }
    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors =
            super::walk::Floors::build(graph, &mut nav.common.barriers, &mut StreetSurvey).unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }

    let mut world = TestWorld::new(31).with_rooftop(RooftopSpec {
        roof_half: 4.0,
        street_edge: 5.0,
        roof_y: 4.5,
        allow_drop: true,
        allow_jump: false,
    });
    let mut unit = make_soldier(1, Vec3::new(0.0, 4.5, 2.0), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    nav.swbf2_ground_goto(
        &mut h,
        &mut unit,
        &mut world,
        Vec3::new(30., 0., 2.),
        GotoMode::Route,
    );
    assert!(
        nav.control_following(h.control) && nav.swbf2.as_ref().unwrap().legs[h.control].is_some(),
        "Route order from a rooftop barrier above a street hub must install a drop leg instead of freezing"
    );
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert_eq!(leg.movement, super::walk::DROP);
    assert!(
        leg.end.y.abs() < 1e-6 && leg.end.z > 4.0,
        "rooftop route fallback must drop onto clear street floor: {:?}",
        leg.end
    );
}

#[test]
fn local_segment_jumps_over_rooftop_parapet_when_plain_drop_is_blocked() {
    // A barrier-covered rooftop rimmed by a low parapet blocks a plain walk-off
    // DROP (jump = false). A grounded soldier sits ~0.02 m below the floor
    // collision plane, so clear_above must be queried above origin.y. On a wide
    // roof (6 m to the rim), PARAPET_DROP walks to the parapet while facing
    // leg.end and fires a forward jump once stalled against the rim.
    let data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"parapet_roof","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[6.0,3.0,6.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();

    let mut world = TestWorld::new(37).with_rooftop(RooftopSpec {
        roof_half: 6.0,
        street_edge: 6.2,
        roof_y: 4.5,
        allow_drop: false,
        allow_jump: true,
    });
    // Floor collision plane sits slightly above the grounded soldier's y = 4.5.
    world.min_clear_above_y = 4.55;
    let mut unit = make_soldier(1, Vec3::new(0.0, 4.5, 0.5), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(
        &mut h,
        &mut unit,
        &mut world,
        Vec3::new(0.0, 4.5, -2.0),
        GotoMode::Straight,
    );
    assert!(
        !h.failed && nav.control_following(h.control),
        "parapet-rimmed wide rooftop must fall back to PARAPET_DROP when plain DROP is blocked"
    );
    let leg = nav.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert_eq!(leg.movement, super::walk::PARAPET_DROP);
    assert!(
        !nav.swbf2.as_ref().unwrap().jump_pending[h.control],
        "PARAPET_DROP must wait until stalled at the parapet instead of jumping at origin"
    );
    assert!(
        leg.end.y.abs() < 1e-6 && leg.end.z > 6.2,
        "parapet jump-drop must land on clear street floor: {:?}",
        leg.end
    );

    // While walking across the roof, enemy aim behind the soldier must not turn
    // them away from leg.end, and no jump fires while moving.
    unit.position = Vec3::new(0.0, 4.5, 3.0);
    unit.velocity = Vec3::new(0.0, 0.0, 4.0);
    unit.soldier.as_mut().unwrap().aim_yaw = Some(std::f64::consts::FRAC_PI_2);
    let moving = nav.swbf2_navigation_input(
        h.control,
        &mut unit,
        1.0 / 60.0,
        Some(Vec3::new(0.0, 4.5, -20.0)),
        false,
        &mut world,
    );
    assert!(!moving.jump);
    assert!(
        moving.turn > 0.0,
        "PARAPET_DROP must turn toward leg.end (+z) instead of enemy aim: turn={}",
        moving.turn
    );

    // Pressed against the parapet within 1 m horizontally of leg.end, executor
    // must hold until y drops near leg.end.y, and stalling while facing leg.end
    // triggers a forward jump with right == 0.
    unit.position = Vec3::new(0.0, 4.5, leg.end.z - 0.6);
    unit.velocity = Vec3::default();
    unit.soldier.as_mut().unwrap().aim_yaw = Some(0.0);
    let mut jumped = false;
    for tick in 0..4 {
        let intent = nav.swbf2_navigation_input(
            h.control,
            &mut unit,
            1.0 / 60.0,
            Some(Vec3::new(0.0, 4.5, -20.0)),
            false,
            &mut world,
        );
        assert!(
            nav.controls[h.control]
                .executor
                .as_ref()
                .is_some_and(|e| e.hold),
            "PARAPET_DROP must hold executor while still elevated above landing"
        );
        if tick < 3 {
            assert!(!intent.jump, "jump must wait for stall ticks (tick {tick})");
        } else {
            assert!(
                intent.jump && intent.right == 0.0 && intent.forward >= 0.75,
                "stalled facing parapet must trigger forward jump: {:?}",
                intent
            );
            jumped = true;
        }
    }
    assert!(jumped);

    // Part B: jet_recover on a stranded open-air rooftop must also query
    // clear_above above the grounded soldier's feet.
    let mut jet_world = TestWorld::new(39).with_rooftop(RooftopSpec {
        roof_half: 6.0,
        street_edge: 6.2,
        roof_y: 4.5,
        allow_drop: false,
        allow_jump: false,
    });
    jet_world.min_clear_above_y = 4.55;
    let mut jet_unit = make_soldier(2, Vec3::new(0.0, 4.5, 0.5), true);
    let mut h_jet = Swbf2GroundGoto::new(nav.new_control());
    nav.control_attach(h_jet.control, Some(&jet_unit));
    nav.controls[h_jet.control].stranded_for = 3.5;
    nav.swbf2_ground_goto(
        &mut h_jet,
        &mut jet_unit,
        &mut jet_world,
        Vec3::new(0.0, 4.5, -2.0),
        GotoMode::Straight,
    );
    assert!(h_jet.failed);
    assert!(
        nav.controls[h_jet.control].jet.pending,
        "stranded jet unit on an open-air rooftop must arm jet_recover when floor sits just above feet"
    );
}

#[test]
fn local_segment_never_installs_zero_length_leg_inside_barrier_and_accepts_short_margin_exit() {
    // Part A: A soldier trapped inside a barrier on a rooftop with no exit must
    // fail cleanly even when the requested horizontal distance is <= 0.5 m,
    // instead of installing a zero-length leg ending inside the barrier.
    let roof_data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"sealed_roof","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[4.0,3.0,4.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_roof = MapNavigation::new_swbf2(&roof_data, None, None, Default::default()).unwrap();
    let mut roof_world = TestWorld::new(41).with_rooftop(RooftopSpec {
        roof_half: 4.0,
        street_edge: 4.2,
        roof_y: 4.5,
        allow_drop: false,
        allow_jump: false,
    });
    let mut roof_unit = make_soldier(1, Vec3::new(0.0, 4.5, 2.0), false);
    let mut h_roof = Swbf2GroundGoto::new(nav_roof.new_control());
    nav_roof.swbf2_ground_goto(
        &mut h_roof,
        &mut roof_unit,
        &mut roof_world,
        Vec3::new(0.0, 4.5, 2.0),
        GotoMode::Straight,
    );
    assert!(
        h_roof.failed && nav_roof.swbf2.as_ref().unwrap().legs[h_roof.control].is_none(),
        "trapped short order inside a barrier must fail instead of installing a zero-length leg"
    );

    // Part B: A soldier standing 0.15 m inside a barrier margin (closer than the
    // 0.20 m ordinary progress threshold) must accept its clear ejection exit.
    let wall_data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"thin_margin_wall","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[1.0,3.0,6.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_wall = MapNavigation::new_swbf2(&wall_data, None, None, Default::default()).unwrap();
    let mut wall_world = TestWorld::new(43);
    let mut wall_unit = make_soldier(2, Vec3::new(1.35, 0.0, 0.0), false);
    let mut h_wall = Swbf2GroundGoto::new(nav_wall.new_control());
    nav_wall.swbf2_ground_goto(
        &mut h_wall,
        &mut wall_unit,
        &mut wall_world,
        Vec3::new(-2.0, 0.0, 0.0),
        GotoMode::Straight,
    );
    assert!(
        !h_wall.failed && nav_wall.control_following(h_wall.control),
        "soldier 0.15 m inside a barrier margin must accept the clear ejection step"
    );
    let leg = nav_wall.swbf2.as_ref().unwrap().legs[h_wall.control].unwrap();
    assert!(
        nav_wall.common.barriers.point_clear(leg.end, 0.5, 1).0 && leg.end.x >= 1.5,
        "short margin ejection leg must land clear outside the barrier: {:?}",
        leg.end
    );
}

#[test]
fn local_segment_skips_drop_fallback_when_airborne_without_floor() {
    // An airborne unit floating 4.5 m above open ground (no floor at origin)
    // must not trigger the Straight awning-drop fallback in mid-air.
    let data = json::parse_bytes(br#"{"schema":2,"barriers":[]}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut world = TestWorld::new(47).with_rooftop(RooftopSpec {
        roof_half: -1.0,
        street_edge: -1.0,
        roof_y: 4.5,
        allow_drop: true,
        allow_jump: false,
    });
    let mut unit = make_soldier(1, Vec3::new(0.0, 4.5, 0.0), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(
        &mut h,
        &mut unit,
        &mut world,
        Vec3::new(5.0, 4.5, 0.0),
        GotoMode::Straight,
    );
    assert!(
        h.failed && nav.swbf2.as_ref().unwrap().legs[h.control].is_none(),
        "airborne unit without floor support must not install a Straight DROP leg in mid-air"
    );
}

#[test]
fn barrier_escape_leg_holds_until_outside_barrier_and_wide_roof_route_hands_off() {
    // Part A: A soldier 0.05 m inside a barrier margin must get an escape leg
    // buffered past the margin, and swbf2_control_update must hold the executor
    // until the unit actually exits the barrier rather than completing on tick 0.
    let wall_data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"margin_wall","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[1.0,3.0,6.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_wall = MapNavigation::new_swbf2(&wall_data, None, None, Default::default()).unwrap();
    let mut wall_world = TestWorld::new(53);
    let mut unit = make_soldier(1, Vec3::new(1.45, 0.0, 0.0), false);
    let mut h = Swbf2GroundGoto::new(nav_wall.new_control());
    nav_wall.swbf2_ground_goto(
        &mut h,
        &mut unit,
        &mut wall_world,
        Vec3::new(-2.0, 0.0, 0.0),
        GotoMode::Straight,
    );
    assert!(!h.failed && nav_wall.control_following(h.control));
    let leg = nav_wall.swbf2.as_ref().unwrap().legs[h.control].unwrap();
    assert!(
        leg.end.x >= 1.70,
        "barrier escape leg must land buffered outside the margin: {:?}",
        leg.end
    );
    // Tick while still inside the barrier (< 1 m from leg.end): first tick consumes
    // the origin node, subsequent ticks must hold the executor instead of completing early.
    for _ in 0..3 {
        nav_wall.swbf2_control_update(h.control, 1.0 / 60.0, &mut unit, &mut wall_world);
    }
    assert!(
        nav_wall.control_following(h.control)
            && nav_wall.controls[h.control]
                .executor
                .as_ref()
                .is_some_and(|e| e.hold),
        "short barrier escape leg must hold executor while unit is still inside the barrier"
    );
    // Once moved outside the barrier, the local leg may complete normally.
    unit.position.x = 1.65;
    nav_wall.swbf2_control_update(h.control, 1.0 / 60.0, &mut unit, &mut wall_world);
    assert!(
        !nav_wall.control_following(h.control),
        "barrier escape leg should complete once unit reaches clear floor outside the barrier"
    );

    // Part B: On a wide building rooftop whose interior sits far from street hub
    // discs, barrier_trapped_route must test connectivity from the ejected edge
    // so Route orders hand off to local_segment instead of freezing.
    let roof_data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[13,0,0],"radius":3},
                {"position":[30,0,0],"radius":3}
            ],
            "arcs":[{"start":0,"end":1,"filterFlags":1}]
        },"barriers":[
            {"sourceIndex":0,"name":"wide_roof","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[9.5,3.0,9.5],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav_roof = MapNavigation::new_swbf2(&roof_data, None, None, Default::default()).unwrap();
    struct StreetSurvey;
    impl super::walk::Survey for StreetSurvey {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            out.push(Vec3::new(hub.x, hub.y, hub.z));
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            Some(Vec3::new(near.x, 0., near.z))
        }
        fn crosses(&mut self, _from: Vec3, _to: Vec3, _flags: u8) -> bool {
            true
        }
    }
    {
        let state = nav_roof.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors =
            super::walk::Floors::build(graph, &mut nav_roof.common.barriers, &mut StreetSurvey)
                .unwrap();
        state.floors.warm(graph, &mut nav_roof.common.barriers);
    }
    let mut roof_world = TestWorld::new(59).with_rooftop(RooftopSpec {
        roof_half: 9.5,
        street_edge: 9.8,
        roof_y: 4.5,
        allow_drop: true,
        allow_jump: false,
    });
    let mut roof_unit = make_soldier(2, Vec3::new(2.0, 4.5, 0.0), false);
    let mut h_roof = Swbf2GroundGoto::new(nav_roof.new_control());
    nav_roof.swbf2.as_mut().unwrap().begin_tick(1);
    nav_roof.swbf2_ground_goto(
        &mut h_roof,
        &mut roof_unit,
        &mut roof_world,
        Vec3::new(30.0, 0.0, 0.0),
        GotoMode::Route,
    );
    assert!(
        nav_roof.control_following(h_roof.control)
            && nav_roof.swbf2.as_ref().unwrap().legs[h_roof.control].is_some(),
        "Route order from wide rooftop far from street hub must hand off to local_segment via ejected edge"
    );
}

#[test]
fn parapet_drop_ignores_wall_avoidance_and_jumps_while_sliding_sideways() {
    let data = json::parse_bytes(
        br#"{"schema":2,"barriers":[
            {"sourceIndex":0,"name":"parapet_roof","enabled":true,
             "xfrm":[1,0,0,0,1,0,0,0,-1,0,0,0],"size":[6.0,3.0,6.0],"flags":1}
        ]}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut world = TestWorld::new(61).with_rooftop(RooftopSpec {
        roof_half: 6.0,
        street_edge: 6.2,
        roof_y: 4.5,
        allow_drop: false,
        allow_jump: true,
    });
    world.min_clear_above_y = 4.55;
    let mut unit = make_soldier(1, Vec3::new(0.0, 4.5, 5.5), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    nav.swbf2_ground_goto(
        &mut h,
        &mut unit,
        &mut world,
        Vec3::new(0.0, 4.5, -2.0),
        GotoMode::Straight,
    );
    assert_eq!(
        nav.swbf2.as_ref().unwrap().legs[h.control].unwrap().movement,
        super::walk::PARAPET_DROP
    );

    // Pressed into the parapet with wall-contact avoidance and 2.5 m/s sideways
    // slide (zero closing speed toward leg.end): must steer forward, preserve
    // stall across a Straight re-issue at tick 3, and fire the jump on tick 4.
    unit.velocity = Vec3::new(2.5, 0.0, 0.0);
    unit.soldier.as_mut().unwrap().aim_yaw = Some(0.0);
    for tick in 0..4 {
        unit.soldier.as_mut().unwrap().avoidance = Some(Vec3::new(0.0, 0.4, -2.0));
        if tick == 3 {
            nav.swbf2_ground_goto(
                &mut h,
                &mut unit,
                &mut world,
                Vec3::new(1.0, 4.5, -2.0),
                GotoMode::Straight,
            );
        }
        let intent = nav.swbf2_navigation_input(
            h.control,
            &mut unit,
            1.0 / 60.0,
            None,
            false,
            &mut world,
        );
        assert!(
            intent.right.abs() < 0.1,
            "PARAPET_DROP must not sidestep away from parapet contact (tick {tick}): {:?}",
            intent
        );
        if tick < 3 {
            assert!(!intent.jump);
        } else {
            assert!(
                intent.jump && intent.right == 0.0 && intent.forward >= 0.75,
                "PARAPET_DROP must jump when closing speed is blocked even while sliding sideways: {:?}",
                intent
            );
        }
    }
}

#[test]
fn an_airborne_bot_keeps_its_leg_and_steers_to_come_down_on_its_waypoint() {
    let data = json::parse_bytes(br#"{"schema":2}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut unit = make_soldier(1, Vec3::default(), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    let mut world = TestWorld::new(3);
    world.flat = true;
    let waypoint = Vec3::new(0., 0., 3.);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, waypoint, GotoMode::Straight);
    assert!(!h.failed);
    let ground = nav.swbf2_navigation_input(h.control, &mut unit, 1. / 60., None, true, &mut world);
    assert!(ground.forward > 0.9);
    let planned = nav.controls[h.control].request.waypoint;
    assert_eq!((planned.x, planned.z), (waypoint.x, waypoint.z));

    // Leaving the ground at 9 m/s toward a waypoint 3 m ahead, 11 m/s upward:
    // it would come down about 10 m past it. The stick brakes from the start.
    unit.soldier.as_mut().unwrap().air = Air::Jump;
    unit.position.y = 0.2;
    unit.velocity = Vec3::new(0., 11., 9.);
    let intent = nav.swbf2_navigation_input(h.control, &mut unit, 1. / 60., None, true, &mut world);
    assert!(intent.forward < -0.9, "brakes for the landing: {}", intent.forward);

    // An order given in the air changes nothing until the soldier lands.
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(10., 0., 0.), GotoMode::Straight);
    nav.swbf2_ground_tick(&mut h, &mut unit, &mut world, 1. / 60.);
    let kept = nav.controls[h.control].request;
    assert!(kept.valid && (kept.waypoint.x, kept.waypoint.z) == (waypoint.x, waypoint.z));
    assert!(!h.failed && !nav.swbf2_ground_arrived(&h));

    // Slow and short of it, the stick pushes on toward the waypoint.
    unit.velocity = Vec3::new(0., 2., 0.5);
    let intent = nav.swbf2_navigation_input(h.control, &mut unit, 1. / 60., None, true, &mut world);
    assert!(intent.forward > 0.9, "carries on to the landing: {}", intent.forward);

    // On landing the order given in the air is planned from where it stands.
    unit.soldier.as_mut().unwrap().air = Air::None;
    unit.position = Vec3::new(0., 0., 2.);
    unit.velocity = Vec3::default();
    nav.swbf2_ground_tick(&mut h, &mut unit, &mut world, 1. / 60.);
    assert!(!h.failed && h.destination.x > 5., "the new order runs: {:?}", h.destination);
}

#[test]
fn a_tick_runs_six_point_searches_and_the_next_order_waits_for_the_next_tick() {
    let hubs = (0..12)
        .map(|i| serde_json::json!({"position":[i*3,0,0],"radius":2}))
        .collect::<Vec<_>>();
    let arcs = (1..12)
        .map(|i| serde_json::json!({"start":i-1,"end":i,"filterFlags":1}))
        .collect::<Vec<_>>();
    let data = serde_json::json!({"schema":2,"plan":{"hubs":hubs,"arcs":arcs}});
    let value = json::parse_bytes(data.to_string().as_bytes()).unwrap();
    let mut nav = MapNavigation::new_swbf2(&value, None, None, Default::default()).unwrap();
    let mut world = TestWorld::new(4);
    world.flat = true;
    let mut bots: Vec<(NavUnit, Swbf2GroundGoto)> = (0..7)
        .map(|i| (make_soldier(i + 1, Vec3::new(1., 0., 0.), false), Swbf2GroundGoto::new(nav.new_control())))
        .collect();
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    for (unit, h) in bots.iter_mut() {
        nav.swbf2_ground_goto(h, unit, &mut world, Vec3::new(30., 0., 0.), GotoMode::Route);
    }
    let placed = |nav: &MapNavigation, h: &Swbf2GroundGoto| nav.controls[h.control].executor.is_some();
    for (_, h) in &bots[..6] {
        assert!(placed(&nav, h) && !h.failed);
    }
    let (unit, h) = &mut bots[6];
    assert!(!placed(&nav, h) && !h.failed && nav.swbf2_ground_waiting(h), "the seventh search waits");
    nav.swbf2.as_mut().unwrap().begin_tick(2);
    nav.swbf2_ground_resume(h, unit, &mut world);
    assert!(placed(&nav, h) && !h.failed, "and runs in the next tick");
}

#[test]
fn a_stalled_recovery_walks_on_without_a_checked_step() {
    let data = json::parse_bytes(br#"{"schema":2}"#).unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    let mut unit = make_soldier(1, Vec3::default(), false);
    let mut h = Swbf2GroundGoto::new(nav.new_control());
    // A wall just ahead fails every checked step toward the point.
    let mut world = TestWorld::new(5).with_wall(0.3, -5., 0.3, 5.);
    world.flat = true;
    let point = Vec3::new(6., 0., 0.);
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, point, GotoMode::Straight);
    assert!(h.failed, "no checked step and no stall yet");
    // Three seconds still in one spot: the walk goes on as the game's does.
    nav.controls[h.control].stranded_for = 3.;
    nav.swbf2_ground_goto(&mut h, &mut unit, &mut world, Vec3::new(6., 0., 0.1), GotoMode::Straight);
    assert!(!h.failed, "a stalled recovery takes the unchecked step");
    assert!((h.destination.x - 2.).abs() < 0.01 && h.destination.z.abs() < 0.1, "{:?}", h.destination);
}

#[test]
fn a_2005_jet_request_jumps_along_its_vector_lights_in_the_jump_and_ends_on_request() {
    use super::super::analogs::Intent;
    use super::goto::apply_jet_and_posture_swbf2;
    let mut unit = make_soldier(7, Vec3::new(0., 0., 0.), true);
    let set_air = |unit: &mut NavUnit, air: Air| unit.soldier.as_mut().unwrap().air = air;
    let vector = Vec3::new(0.5, 0.5, 0.25);
    // The take-off press carries the vector.
    let mut intent = Intent::default();
    let (pending, jetting, end) = apply_jet_and_posture_swbf2(&unit, &mut intent, true, false, vector, 0., false);
    assert!(intent.jump && !pending && jetting && !end);
    assert_eq!(intent.jet_direction, Some(Some(vector)));
    // A zero request clears a vector left from an earlier jet.
    let mut intent = Intent::default();
    apply_jet_and_posture_swbf2(&unit, &mut intent, true, false, Vec3::default(), 0., false);
    assert_eq!(intent.jet_direction, Some(None));
    // In the jump: the second press, with the vector.
    set_air(&mut unit, Air::Jump);
    let mut intent = Intent::default();
    let (_, jetting, _) = apply_jet_and_posture_swbf2(&unit, &mut intent, false, true, vector, 0., false);
    assert!(intent.jump && jetting);
    assert_eq!(intent.jet_direction, Some(Some(vector)));
    // In the jet: no press, until an end request, which presses once and is consumed.
    set_air(&mut unit, Air::Jet);
    let mut intent = Intent::default();
    let (_, jetting, _) = apply_jet_and_posture_swbf2(&unit, &mut intent, false, true, vector, 0., false);
    assert!(!intent.jump && jetting);
    let mut intent = Intent::default();
    let (_, jetting, end) = apply_jet_and_posture_swbf2(&unit, &mut intent, false, true, vector, 0., true);
    assert!(intent.jump && jetting && !end);
    // Out of the jet the sequence is over; an end request while the body is in its jump
    // presses nothing of its own (the jump's press lights the jet, as the game's does).
    set_air(&mut unit, Air::Other);
    let (_, jetting, _) = apply_jet_and_posture_swbf2(&unit, &mut Intent::default(), false, true, vector, 0., false);
    assert!(!jetting);
    set_air(&mut unit, Air::Jump);
    let mut intent = Intent::default();
    let (_, _, end) = apply_jet_and_posture_swbf2(&unit, &mut intent, false, true, vector, 0., true);
    assert!(intent.jump && !end);
    // With no sequence running the request waits.
    set_air(&mut unit, Air::None);
    let (_, jetting, end) = apply_jet_and_posture_swbf2(&unit, &mut Intent::default(), false, false, vector, 0., true);
    assert!(!jetting && end);
}

#[test]
fn a_body_jetting_at_a_target_steers_its_stick_at_the_target_and_only_in_the_jet() {
    use super::goto::jet_stick;
    let mut unit = make_soldier(8, Vec3::new(0., 5., 0.), true);
    assert_eq!(jet_stick(&unit, Vec3::new(0., 0., 10.)), None, "on foot the order's own steering stands");
    unit.soldier.as_mut().unwrap().air = Air::Jet;
    let (forward, right) = jet_stick(&unit, Vec3::new(0., 0., 10.)).unwrap();
    assert!((forward - 1.).abs() < 1e-9 && right.abs() < 1e-9, "ahead: full forward");
    let (forward, right) = jet_stick(&unit, Vec3::new(-10., 0., 0.)).unwrap();
    assert!(forward.abs() < 1e-9 && (right - 1.).abs() < 1e-9, "to the right of a body facing +z: full right");
    let (forward, _) = jet_stick(&unit, Vec3::new(0., 0., -10.)).unwrap();
    assert!((forward + 1.).abs() < 1e-9, "behind: full back");
}

#[test]
fn a_2005_melee_leap_is_one_jump_along_its_vector_and_never_a_jet() {
    use super::super::analogs::Intent;
    use super::goto::leap_swbf2;
    let mut unit = make_soldier(9, Vec3::new(0., 0., 0.), true);
    let vector = Vec3::new(0.6, 0.7, 0.3);
    let mut intent = Intent::default();
    assert!(leap_swbf2(&unit, &mut intent, vector));
    assert!(intent.jump);
    assert_eq!(intent.jet_direction, Some(Some(vector)));
    // In the air the leap is spent with no press (no second press: no jet, no double jump).
    unit.soldier.as_mut().unwrap().air = Air::Jump;
    let mut intent = Intent::default();
    assert!(leap_swbf2(&unit, &mut intent, vector));
    assert!(!intent.jump && intent.jet_direction.is_none());
}

#[test]
fn a_start_on_a_corridor_goes_on_to_its_far_node_instead_of_walking_back() {
    // A straight corridor A (0) -> B (20) -> C (40). The soldier stands 6 m past A on the way
    // to B, inside A's hub, so A attaches directly; the route to C must not walk back to A.
    let data = json::parse_bytes(
        br#"{"schema":2,"plan":{
            "hubs":[
                {"position":[0,0,0],"radius":8},
                {"position":[20,0,0],"radius":3},
                {"position":[40,0,0],"radius":3}
            ],
            "arcs":[
                {"start":0,"end":1,"filterFlags":1},
                {"start":1,"end":2,"filterFlags":1}
            ]
        }}"#,
    )
    .unwrap();
    let mut nav = MapNavigation::new_swbf2(&data, None, None, Default::default()).unwrap();
    struct Line;
    impl super::walk::Survey for Line {
        fn floors(&mut self, hub: &super::graph::Swbf2Hub, out: &mut Vec<Vec3>) {
            out.push(Vec3::new(20. * hub.index as f64, 0., 0.));
        }
        fn floor(&mut self, near: Vec3) -> Option<Vec3> {
            Some(Vec3::new(near.x, 0., near.z))
        }
        fn crosses(&mut self, _from: Vec3, _to: Vec3, _flags: u8) -> bool {
            true
        }
    }
    {
        let state = nav.swbf2.as_mut().unwrap();
        state.clearance.prepare();
        let graph = state.graph.as_ref().unwrap();
        state.floors = super::walk::Floors::build(graph, &mut nav.common.barriers, &mut Line).unwrap();
        state.floors.warm(graph, &mut nav.common.barriers);
    }
    let from = Vec3::new(6., 0., 0.3);
    let to = Vec3::new(40.5, 0., 0.2);
    nav.swbf2.as_mut().unwrap().begin_tick(1);
    // The straight line from the soldier to the goal is closed, so the trip uses the graph.
    let q = nav.try_distance_with_clearance(from, to, 0.45, 1, false, true, |a, b, _| {
        Some(!(super::super::math::d2(a, from) < 0.01 && super::super::math::d2(b, to) < 0.01))
    });
    assert!(q.reachable, "{q:?}");
    assert!(q.distance < 40., "the trip goes on toward B (about 34.5 m), not back to A (about 46.5 m): {q:?}");
}
