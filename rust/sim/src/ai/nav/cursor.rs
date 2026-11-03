//! A route through the authored graph, one hub at a time (nav/cursor.js).
//! Hubs and arcs are indices; the graph and barrier set the cursor was
//! made with are passed to its calls (the service owns them).

use super::barriers::BarrierSet;
use super::graph::NavGraph;
use super::math::{d2, f32, normalize, Vec3};
use crate::ai::random::ClassicRandom;

/// GraphCursor (cursor.js:16).
#[derive(Clone, Debug)]
pub struct GraphCursor {
    pub mask: u32,
    pub clearance: f64,
    pub primary: Option<usize>,
    pub secondary: Option<usize>,
    pub exact: Vec3,
    pub attach_type: u8,
    pub current: Option<usize>,
    pub waypoint: Vec3,
    pub previous: Option<usize>,
}

impl Default for GraphCursor {
    fn default() -> Self {
        GraphCursor {
            mask: 0,
            clearance: 0.0,
            primary: None,
            secondary: None,
            exact: Vec3::default(),
            attach_type: 0,
            current: None,
            waypoint: Vec3::default(),
            previous: None,
        }
    }
}

impl GraphCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Route from start to end (cursor.js:33).
    pub fn init(&mut self, graph: &mut NavGraph, start: Vec3, end: Vec3, mask: u32, clearance: f64, random: &mut ClassicRandom) -> bool {
        self.exact = end;
        self.mask = mask;
        self.clearance = clearance;
        self.previous = None;
        self.secondary = None;
        let s = graph.attach(start, mask);
        let e = graph.attach(end, mask);
        self.attach_type = e.kind;
        let mut big_s = s.arc;
        let mut big_t = e.arc;
        if s.kind == 0 {
            big_s = graph.nearest_arc(start, mask);
        }
        if e.kind == 0 {
            big_t = graph.nearest_arc(end, mask);
        }

        if s.kind == 1 {
            let h = s.hub.expect("type 1 names a hub");
            if e.kind == 1 {
                let eh = e.hub.expect("type 1 names a hub");
                self.primary = Some(eh);
                if h == eh {
                    return self.finish(eh);
                }
                self.current = Some(h);
                self.waypoint = start;
                return self.advance(graph, random);
            }
            let Some(t) = big_t else { return self.lost() };
            if h == graph.arcs[t].a || h == graph.arcs[t].b {
                self.primary = Some(h);
                return self.finish(h);
            }
            self.current = Some(h);
            self.waypoint = start;
            let to_c = graph.select(None, graph.arcs[t].a, h, mask, random);
            let to_d = graph.select(None, graph.arcs[t].b, h, mask, random);
            self.pick_destination(graph, t, to_c, to_d, end);
            return self.advance(graph, random);
        }

        let Some(big_s) = big_s else { return self.lost() };
        let a = graph.arcs[big_s].a;
        let b = graph.arcs[big_s].b;
        if e.kind == 1 {
            let j = e.hub.expect("type 1 names a hub");
            self.primary = Some(j);
            if j == a || j == b {
                return self.finish(j);
            }
            self.waypoint = start;
            let from_a = graph.select(None, a, j, mask, random);
            let from_b = graph.select(None, b, j, mask, random);
            if from_a == Some(big_s) {
                self.current = Some(a);
            } else if from_b == Some(big_s) {
                self.current = Some(b);
            } else {
                let (pa, pb) = (graph.hubs[a].pos(), graph.hubs[b].pos());
                self.current = Some(if d2(pa, start) < d2(pb, start) { b } else { a });
                let current = self.current.expect("just set");
                self.previous = graph.alternate_opt(current, Some(big_s), mask, random);
            }
            return self.advance(graph, random);
        }

        let Some(big_t) = big_t else { return self.lost() };
        if big_s == big_t {
            self.primary = None;
            self.current = None;
            self.waypoint = self.exact;
            return true;
        }
        let (ta, tb) = (graph.arcs[big_t].a, graph.arcs[big_t].b);
        if a == ta || a == tb {
            self.primary = Some(a);
            self.current = Some(b);
            self.waypoint = start;
            return self.advance(graph, random);
        }
        if b == ta || b == tb {
            self.primary = Some(b);
            self.current = Some(a);
            self.waypoint = start;
            return self.advance(graph, random);
        }
        self.waypoint = start;
        let q1 = graph.select(None, a, ta, mask, random);
        let q2 = graph.select(None, a, tb, mask, random);
        let q3 = graph.select(None, b, ta, mask, random);
        let q4 = graph.select(None, b, tb, mask, random);
        let current;
        if q1 == Some(big_s) && q2 == Some(big_s) {
            current = a;
        } else if q3 == Some(big_s) && q4 == Some(big_s) {
            current = b;
        } else {
            if q1 == q2 {
                current = b;
            } else if q3 == q4 {
                current = a;
            } else {
                let (pa, pb) = (graph.hubs[a].pos(), graph.hubs[b].pos());
                current = if d2(pa, start) < d2(pb, start) { b } else { a };
            }
            // Its result is replaced below, but the draw stays.
            self.previous = graph.alternate_opt(current, Some(big_s), mask, random);
        }
        let r = graph.other_end(big_s, current);
        let qc = graph.select(None, ta, r, mask, random);
        let qd = graph.select(None, tb, r, mask, random);
        self.pick_destination(graph, big_t, qc, qd, end);
        self.previous = Some(big_s);
        self.current = Some(r);
        self.waypoint = self.shell(graph, r, random);
        true
    }

    /// cursor.js:142.
    fn pick_destination(&mut self, graph: &NavGraph, t: usize, from_c: Option<usize>, from_d: Option<usize>, end: Vec3) {
        let (ta, tb) = (graph.arcs[t].a, graph.arcs[t].b);
        let chosen = if from_c == Some(t) {
            tb
        } else if from_d == Some(t) {
            ta
        } else if d2(graph.hubs[ta].pos(), end) < d2(graph.hubs[tb].pos(), end) {
            ta
        } else {
            tb
        };
        self.primary = Some(chosen);
        self.secondary = Some(graph.other_end(t, chosen));
    }

    fn finish(&mut self, hub: usize) -> bool {
        self.current = Some(hub);
        self.waypoint = self.exact;
        true
    }

    fn lost(&mut self) -> bool {
        self.primary = None;
        self.secondary = None;
        self.current = None;
        self.waypoint = self.exact;
        false
    }

    /// Next hub (cursor.js:167).
    pub fn advance(&mut self, graph: &mut NavGraph, random: &mut ClassicRandom) -> bool {
        // cursor.js:168: current === primary || current === secondary,
        // with nulls comparing equal, as Option equality does.
        if self.current == self.primary || self.current == self.secondary {
            self.primary = None;
            self.secondary = None;
            self.current = None;
            self.previous = None;
            self.waypoint = self.exact;
            return true;
        }
        let current = self.current.expect("advance with a current hub");
        let primary = self.primary.expect("advance with a destination hub");
        let Some(arc) = graph.select(self.previous, current, primary, self.mask, random) else {
            return false;
        };
        self.previous = Some(arc);
        self.current = Some(graph.other_end(arc, current));
        if self.current != self.primary || self.attach_type != 1 {
            let hub = self.current.expect("just set");
            self.waypoint = self.shell(graph, hub, random);
            return true;
        }
        self.waypoint = self.exact;
        true
    }

    /// A random point on the hub's shell (cursor.js:188).
    pub fn shell(&self, graph: &NavGraph, hub: usize, random: &mut ClassicRandom) -> Vec3 {
        let hub = &graph.hubs[hub];
        let r = v8math::max(0.0, f32(hub.radius - self.clearance));
        let z = f32(random.uniform() - 0.5) * 2.0;
        let y = f32(random.uniform() - 0.5) * 2.0;
        let x = f32(random.uniform() - 0.5) * 2.0;
        let mut v = Vec3::new(x, y, z);
        normalize(&mut v);
        Vec3::new(f32(hub.x + v.x * r), f32(hub.y + v.y * r), f32(hub.z + v.z * r))
    }

    /// The waypoint is a hub shell point (cursor.js:198).
    pub fn regenerable(&self) -> bool {
        self.current.is_some() && (self.current != self.primary || self.attach_type != 1)
    }

    /// Replace a waypoint that sits inside a barrier (cursor.js:205).
    pub fn retry(&mut self, graph: &NavGraph, barriers: &mut BarrierSet, radius: f64, random: &mut ClassicRandom) -> bool {
        if !self.regenerable() {
            return false;
        }
        let mut made = false;
        let mut clear = barriers.point_clear(self.waypoint, radius, self.mask).0;
        let mut i = 0;
        while !clear && i < 100 {
            let hub = self.current.expect("regenerable names a hub");
            self.waypoint = self.shell(graph, hub, random);
            made = true;
            clear = barriers.point_clear(self.waypoint, radius, self.mask).0;
            i += 1;
        }
        made
    }
}
