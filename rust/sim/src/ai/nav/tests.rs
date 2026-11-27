//! Focused checks of the port against the JavaScript's answers on the
//! same inputs (the values below are src/ai's, under Node 24.22). The
//! full operation-by-operation gate is tools/nav-twin.mjs with the
//! nav-twin binary.

use super::barriers::{barrier_from_record, BarrierRecord, BarrierSet, Work};
use super::graph::{ArcRecord, GraphRecord, HubRecord, NavGraph};
use super::math::Vec3;
use super::planner::PathStore;
use super::search::{BarrierSearch, OpenHeap, SearchNode};
use crate::ai::random::ClassicRandom;

#[test]
fn classic_random_draws_as_the_javascript() {
    let mut r = ClassicRandom::new(12345.0);
    let next: Vec<u32> = (0..5).map(|_| r.next()).collect();
    assert_eq!(next, [71042361, 2726923020, 483059959, 2355134194, 3364919450]);
    let uniforms: Vec<u64> = (0..3).map(|_| r.uniform().to_bits()).collect();
    assert_eq!(
        uniforms,
        [0.74379026889801025f64.to_bits(), 0.073668837547302246f64.to_bits(), 0.46029198169708252f64.to_bits()]
    );
    let ints: Vec<f64> = (0..5).map(|_| r.int(-2.0, 7.0)).collect();
    assert_eq!(ints, [7.0, 7.0, 4.0, 2.0, 2.0]);
    assert_eq!(r.seed(), 1818069995);
}

#[test]
fn open_heap_ties_come_out_as_the_javascripts() {
    let mut heap = OpenHeap::new(8);
    let mut nodes: Vec<SearchNode> = (1..=4).map(|x| SearchNode { x: x as f64, y: 0.0, z: 0.0, g: 0.0, parent: None, heap: 0 }).collect();
    heap.push(&mut nodes, 0, 5.0);
    heap.push(&mut nodes, 1, 5.0);
    heap.push(&mut nodes, 2, 7.0);
    heap.push(&mut nodes, 3, 5.0);
    let pops: Vec<f64> = (0..4)
        .map(|_| {
            let i = heap.pop(&mut nodes).expect("node");
            nodes[i].x
        })
        .collect();
    assert_eq!(pops, [3.0, 4.0, 1.0, 2.0]);
}

fn one_barrier_set() -> BarrierSet {
    let record = BarrierRecord {
        name: Some("b".into()),
        xfrm: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0],
        size: [2.0, 0.0, 2.0],
        flags: 63.0,
    };
    BarrierSet::new(vec![barrier_from_record(&record, 0).expect("barrier")])
}

#[test]
fn a_leap_probe_from_inside_a_barrier_starts_at_its_nearest_free_edge() {
    let mut set = one_barrier_set();
    let inside = Vec3::new(0.5, 0.0, 0.0);
    let away = Vec3::new(25.0, 0.0, 0.0);
    assert!(set.segment_blocked(inside, away, 0.5, 1, None), "the bare segment test refuses any probe from inside");
    assert!(set.leap_clear(inside, away, 0.5, 1), "the start moves to the near edge, the way out is free");
    assert!(!set.leap_clear(Vec3::new(-10.0, 0.0, 0.0), Vec3::new(15.0, 0.0, 0.0), 0.5, 1), "a probe across it stays blocked");
    assert!(!set.leap_clear(inside, Vec3::new(-25.0, 0.0, 0.0), 0.5, 1), "from the near edge back across the barrier is blocked");
}

#[test]
fn barrier_search_finds_the_javascripts_path() {
    let mut barriers = one_barrier_set();
    let mut store = PathStore::new(900.0, 7200.0);
    let mut search = BarrierSearch::new(None, None);
    assert!(search.begin(&mut barriers, &store, Vec3::new(-10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), 0.5, 63));
    let mut work = Work { count: 0.0, cap: 100000.0 };
    search.step(&mut barriers, &mut work);
    assert!(search.finished && search.success);
    assert_eq!(work.count, 7.0);
    let path = search.reconstruct(&mut store).expect("path");
    let points: Vec<(f64, f64, f64)> = store.paths[path].nodes.iter().map(|n| (n.x, n.y, n.z)).collect();
    assert_eq!(points, [(-10.0, 0.0, 0.0), (-3.0, 0.0, 3.0), (3.0, 0.0, 3.0), (10.0, 0.0, 0.0)]);
}

#[test]
fn graph_attach_and_select_follow_the_tables() {
    // Two hubs joined by one arc; the only route either way is slot 0 at
    // weight 31, in every table.
    let hubs = vec![
        HubRecord { name: None, position: [0.0, 0.0, 0.0], radius: 5.0, arcs: vec![0.0, 255.0, 255.0, 255.0, 255.0, 255.0, 255.0, 255.0] },
        HubRecord { name: None, position: [20.0, 0.0, 0.0], radius: 5.0, arcs: vec![0.0, 255.0, 255.0, 255.0, 255.0, 255.0, 255.0, 255.0] },
    ];
    let arcs = vec![ArcRecord { name: None, hubs: [0.0, 1.0], mask: 63.0 }];
    let mut words = vec![0u32; 5 * 2 * 2];
    for from in 0..2 {
        for t in 0..5 {
            words[(from * 5 + t) * 2 + (1 - from)] = 31 << 3;
        }
    }
    let record = GraphRecord { info: [2.0, 1.0], hubs, arcs, tables: Some(5.0) };
    let mut graph = NavGraph::new(&record, words).expect("graph");
    let inside = graph.attach(Vec3::new(1.0, 0.0, 0.0), 1);
    assert_eq!((inside.kind, inside.hub), (1, Some(0)));
    let midway = graph.attach(Vec3::new(10.0, 4.0, 0.0), 1);
    assert_eq!((midway.kind, midway.arc), (2, Some(0)));
    let mut random = ClassicRandom::new(7.0);
    assert_eq!(graph.select(None, 0, 1, 1, &mut random), Some(0));
    // Flyer-only masks have no table and are reported, not routed.
    assert_eq!(graph.select(None, 0, 1, 32, &mut random), None);
    assert_eq!(graph.reports.len(), 1);
}

#[test]
fn a_four_times_turn_rate_asks_a_quarter_of_the_turn_and_saturates_at_the_same_limit() {
    use super::analogs::{analogs, analogs_with_turn};
    use super::units::{kind, NavUnit, SoldierView};
    let mut unit = NavUnit::new(0, kind::SOLDIER, Vec3::default());
    unit.soldier = Some(SoldierView { aim_yaw: Some(0.0), max_turn: Some(2.0), ..Default::default() });
    let dt = 1.0 / 60.0;
    let small = Vec3::new(-0.01, 0.0, 1.0);
    let once = analogs(&unit, None, Some(small), dt, false, None);
    assert_eq!(once.turn.to_bits(), analogs_with_turn(&unit, None, Some(small), dt, false, None, 1.0).turn.to_bits());
    let four = analogs_with_turn(&unit, None, Some(small), dt, false, None, 4.0);
    assert!(once.turn.abs() < 1.0 && once.turn != 0.0, "{}", once.turn);
    assert!((four.turn - once.turn / 4.0).abs() < 1e-12, "{} {}", four.turn, once.turn);
    let behind = Vec3::new(-1.0, 0.0, -1.0);
    for (scale, facing, limit) in [(1.0, false, 1.0), (4.0, false, 1.0), (4.0, true, 2.0)] {
        let wide = analogs_with_turn(&unit, None, Some(behind), dt, facing, None, scale);
        assert_eq!(wide.turn.abs(), limit);
    }
}
