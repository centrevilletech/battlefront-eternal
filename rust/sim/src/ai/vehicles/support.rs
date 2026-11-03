//! Fire support for a post a hull cannot stand on (src/ai/classic/vehicles/
//! support.js): a pilot sent to a post its hull's planning barriers keep it
//! from is sent instead to a hub of the planning graph its hull may stand
//! on and route to, within weapon reach of the post, that sees it.
//!
//! The graph and barriers are navigation's (`crate::ai::nav`), loaded with
//! the map; the line of sight is the battle's.

use three::Vector3;
use v8math::hypot;

use crate::ai::nav::barriers::BarrierSet;
use crate::ai::nav::graph::{NavGraph, NO_TABLE};
use crate::ai::nav::math::Vec3;

/// support.js:22.
pub const REACH_MARGIN: f64 = 10.0;
/// support.js:25.
pub const SUPPORT_RANGE: f64 = 100.0;
/// support.js:28.
pub const SUPPORT_SPREAD: f64 = 15.0;
/// support.js:30.
const POST_HEIGHT: f64 = 1.5;

/// A support hub and its distance from the post (`{ hub, distance }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportHub {
    pub hub: usize,
    pub distance: f64,
}

/// hubsAt(graph, p, mask) (support.js:38): the hubs a point stands on.
pub fn hubs_at(graph: &NavGraph, p: Vec3, mask: u32) -> Vec<usize> {
    let a = graph.attach(p, mask);
    if a.kind == 1 {
        return vec![a.hub.expect("a hub attach names its hub")];
    }
    if a.kind == 2 {
        let arc = &graph.arcs[a.arc.expect("an arc attach names its arc")];
        return vec![arc.a, arc.b];
    }
    match graph.nearest_arc(p, mask) {
        Some(i) => vec![graph.arcs[i].a, graph.arcs[i].b],
        None => Vec::new(),
    }
}

/// graph.word(from, table, to) (graph.js:78).
fn word(graph: &NavGraph, from: usize, table: i32, to: usize) -> u32 {
    graph.words[graph.hubs[from].base + table as usize * graph.hubs.len() + to]
}

/// routes(graph, from, to, mask) (support.js:47).
pub fn routes(graph: &NavGraph, from: &[usize], to: usize, mask: u32) -> bool {
    let table = graph.table(mask);
    if table < 0 || table == NO_TABLE {
        return false;
    }
    from.iter().any(|&hub| hub == to || word(graph, hub, table, to) != 0)
}

/// outOfReach(barriers, center, reach, r, mask) (support.js:56).
pub fn out_of_reach(barriers: Option<&mut BarrierSet>, center: &Vector3, reach: f64, r: f64, mask: u32) -> bool {
    let Some(barriers) = barriers else { return false };
    let mut p = Vec3::new(center.x, center.y, center.z);
    if !barriers.correct(&mut p, r, mask) {
        return true;
    }
    hypot(p.x - center.x, p.z - center.z) > reach
}

/// The point the support hubs must see (support.js:70): POST_HEIGHT
/// over the post's centre.
pub fn post_target(center: &Vector3) -> Vector3 {
    Vector3::new(center.x, center.y + POST_HEIGHT, center.z)
}

/// A hub supportHubs keeps before its sight test: its index, place and
/// distance from the post.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub hub: usize,
    pub hub_at: Vector3,
    pub distance: f64,
}

/// supportHubs(nav, los, center, r, mask, eyeHeight)'s graph and barrier
/// tests (support.js:66-75): the hubs of the mask within SUPPORT_RANGE,
/// clear of its barriers, in the graph's order. The caller tests each one's
/// sight of the post (`los`) in the same order and sorts what is left
/// (`sort_by_distance`): the tests here are queries of the map's content,
/// with nothing to observe in between.
pub fn support_candidates(graph: Option<&NavGraph>, mut barriers: Option<&mut BarrierSet>, center: &Vector3, r: f64, mask: u32) -> Vec<Candidate> {
    let Some(graph) = graph else { return Vec::new() };
    let mut out = Vec::new();
    for hub in &graph.hubs {
        if (hub.mask & mask) == 0 {
            continue;
        }
        let distance = hypot(hub.x - center.x, hub.z - center.z);
        if distance > SUPPORT_RANGE {
            continue;
        }
        if let Some(b) = barriers.as_deref_mut() {
            if !b.point_clear(Vec3::new(hub.x, hub.y, hub.z), r, mask).0 {
                continue;
            }
        }
        out.push(Candidate { hub: hub.index, hub_at: Vector3::new(hub.x, hub.y, hub.z), distance });
    }
    out
}

/// `out.sort((a, b) => a.distance - b.distance)`: the distances are finite
/// and the comparator consistent, so a stable sort gives V8's order (a
/// moves after b only when the comparator is above 0).
pub fn sort_by_distance(list: &mut [SupportHub]) {
    for i in 1..list.len() {
        let mut j = i;
        while j > 0 && list[j - 1].distance - list[j].distance > 0.0 {
            list.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// pickSupport(graph, hubs, from, mask, random) (support.js:86) up to its
/// draw: of the post's support hubs, those the pilot at `from` can route
/// to, within SUPPORT_SPREAD of the nearest of them; None when none is
/// routable. The caller picks among more than one with generator A.
pub fn pick_candidates(graph: &NavGraph, hubs: &[SupportHub], from: &Vector3, mask: u32) -> Option<Vec<SupportHub>> {
    let start = hubs_at(graph, Vec3::new(from.x, from.y, from.z), mask);
    if start.is_empty() {
        return None;
    }
    let routable: Vec<SupportHub> = hubs.iter().copied().filter(|c| routes(graph, &start, c.hub, mask)).collect();
    if routable.is_empty() {
        return None;
    }
    let first = routable[0].distance;
    Some(routable.into_iter().filter(|c| c.distance <= first + SUPPORT_SPREAD).collect())
}
