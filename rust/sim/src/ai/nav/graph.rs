//! The authored planning graph and its stochastic routing (nav/graph.js).
//! Hubs and arcs are indices into `hubs` / `arcs` where the JavaScript
//! holds the objects; identity comparisons become index comparisons.
//! The loader's `report(kind, detail)` calls collect in `reports`.

use super::math::{f32, lowest_bit, Vec3};
use crate::ai::random::ClassicRandom;

/// graph.js:20.
pub const UNUSED: u32 = 255;
/// graph.js:21.
pub const NO_TABLE: i32 = -1;

const MAX_PASSES: u32 = 64; // graph.js:28

fn search_start() -> f64 {
    f32(10000.0)
}
fn far() -> f64 {
    f32(1e20)
}

/// One `report()` from the loader or the selector (graph.js:62,90,124).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GraphReport {
    ZeroLengthArc { arc: usize },
    RouteTable { mask: u32 },
    RouteHang { hub: usize, table: i32, destination: usize, word: u32 },
}

/// A hub record of ai-nav.json's graph, as numbers (data.rs makes these
/// from the parsed JSON).
#[derive(Clone, Debug)]
pub struct HubRecord {
    pub name: Option<String>,
    pub position: [f64; 3],
    pub radius: f64,
    pub arcs: Vec<f64>,
}

/// An arc record of ai-nav.json's graph.
#[derive(Clone, Debug)]
pub struct ArcRecord {
    pub name: Option<String>,
    pub hubs: [f64; 2],
    pub mask: f64,
}

/// The `graph` object of ai-nav.json as the constructor reads it:
/// `info` is [hubCount, arcCount, ...], `tables` is `routes?.tables`.
#[derive(Clone, Debug)]
pub struct GraphRecord {
    pub info: [f64; 2],
    pub hubs: Vec<HubRecord>,
    pub arcs: Vec<ArcRecord>,
    pub tables: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct Hub {
    pub index: usize,
    pub name: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub radius: f64,
    pub slots: [u8; 8],
    pub mask: u32,
    pub base: usize,
}

impl Hub {
    pub fn pos(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

#[derive(Clone, Debug)]
pub struct Arc {
    pub index: usize,
    pub name: Option<String>,
    pub a: usize,
    pub b: usize,
    pub mask: u32,
    pub degenerate: bool,
}

/// The result of `attach` (graph.js:160): type 0, 1 (hub) or 2 (arc).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Attach {
    pub kind: u8,
    pub hub: Option<usize>,
    pub arc: Option<usize>,
}

/// NavGraph (graph.js:30).
#[derive(Clone, Debug)]
pub struct NavGraph {
    pub tables: f64,
    pub words: Vec<u32>,
    pub hubs: Vec<Hub>,
    pub arcs: Vec<Arc>,
    pub reports: Vec<GraphReport>,
}

impl NavGraph {
    /// graph.js:32. Bad input is an Err with the JavaScript's message.
    pub fn new(graph: &GraphRecord, words: Vec<u32>) -> Result<Self, String> {
        let hub_count = graph.info[0];
        let arc_count = graph.info[1];
        if !(hub_count >= 0.0 && arc_count >= 0.0) {
            return Err("ai-nav: negative graph counts".into());
        }
        if graph.hubs.len() as f64 != hub_count || graph.arcs.len() as f64 != arc_count {
            return Err("ai-nav: graph counts do not match INFO".into());
        }
        let tables = graph.tables.unwrap_or(5.0);
        if words.len() as f64 != tables * hub_count * hub_count {
            return Err(format!(
                "ai-nav: {} routing words, expected {}",
                words.len(),
                super::data::num_text(tables * hub_count * hub_count)
            ));
        }
        let mut hubs = Vec::with_capacity(graph.hubs.len());
        for (index, h) in graph.hubs.iter().enumerate() {
            if h.arcs.len() != 8 {
                return Err(format!("ai-nav: hub {index} does not have 8 arc slots"));
            }
            for v in [h.position[0], h.position[1], h.position[2], h.radius] {
                if !v.is_finite() {
                    return Err(format!("ai-nav: hub {index} is not finite"));
                }
            }
            for a in &h.arcs {
                if *a != UNUSED as f64 && !(*a >= 0.0 && *a < arc_count) {
                    return Err(format!("ai-nav: hub {index} names arc {}", super::data::num_text(*a)));
                }
            }
            let mut slots = [0u8; 8];
            for (i, a) in h.arcs.iter().enumerate() {
                slots[i] = v8math::to_uint32(*a) as u8; // Uint8Array.from
            }
            hubs.push(Hub {
                index,
                name: h.name.clone(),
                x: f32(h.position[0]),
                y: f32(h.position[1]),
                z: f32(h.position[2]),
                radius: f32(h.radius),
                slots,
                mask: 0,
                base: (index as f64 * tables * hub_count) as usize,
            });
        }
        let mut arcs = Vec::with_capacity(graph.arcs.len());
        for (index, a) in graph.arcs.iter().enumerate() {
            let (i, j) = (a.hubs[0], a.hubs[1]);
            if !(i >= 0.0 && i < hub_count && j >= 0.0 && j < hub_count) {
                return Err(format!("ai-nav: arc {index} joins missing hubs"));
            }
            arcs.push(Arc {
                index,
                name: a.name.clone(),
                a: i as usize,
                b: j as usize,
                mask: v8math::to_uint32(a.mask),
                degenerate: false,
            });
        }
        let mut reports = Vec::new();
        for arc in &mut arcs {
            let (a, b) = (&hubs[arc.a], &hubs[arc.b]);
            arc.degenerate = a.x == b.x && a.y == b.y && a.z == b.z;
            if arc.degenerate {
                reports.push(GraphReport::ZeroLengthArc { arc: arc.index });
            }
        }
        for hub in &mut hubs {
            let mut mask = 0u32;
            for s in hub.slots {
                if s as u32 != UNUSED {
                    mask |= arcs[s as usize].mask;
                }
            }
            hub.mask = mask;
        }
        Ok(NavGraph { tables, words, hubs, arcs, reports })
    }

    /// The routing table a traversal mask uses, or NO_TABLE (graph.js:73).
    pub fn table(&self, mask: u32) -> i32 {
        let t = lowest_bit(mask as f64);
        if t < self.tables {
            t as i32
        } else {
            NO_TABLE
        }
    }

    fn word(&self, from: usize, table: i32, to: usize) -> u32 {
        self.words[self.hubs[from].base + table as usize * self.hubs.len() + to]
    }

    /// The weighted next arc from hub `from` toward hub `to`, or None
    /// (graph.js:87). `previous` is excluded as the JavaScript excludes it.
    pub fn select(&mut self, previous: Option<usize>, from: usize, to: usize, mask: u32, random: &mut ClassicRandom) -> Option<usize> {
        let table = self.table(mask);
        if table == NO_TABLE {
            self.reports.push(GraphReport::RouteTable { mask });
            return None;
        }
        let first = self.word(from, table, to);
        let mut total: i32 = 31;
        let mut excluded: u32 = 8;
        if let Some(prev) = previous {
            for i in 0..8 {
                if self.hubs[from].slots[i] as usize == prev {
                    excluded = i as u32;
                    break;
                }
            }
            let mut w = first;
            loop {
                if (w & 7) == excluded {
                    total = 31 - ((w >> 3) & 31) as i32;
                    if total < 1 {
                        return self.alternate_opt(from, Some(prev), mask, random);
                    }
                    break;
                }
                w >>= 8;
                if w == 0 {
                    break;
                }
            }
        }
        if first == 0 {
            return None;
        }
        // random.next() % total, in f64 as the JavaScript computes it; the
        // operands are small whole numbers, so it is the integer remainder.
        let mut r = (random.next() % total as u32) as i32;
        let mut w = first;
        let mut passes = 0u32;
        loop {
            r -= ((w >> 3) & 31) as i32;
            if r <= 0 && (w & 7) != excluded {
                break;
            }
            w >>= 8;
            if w == 0 {
                w = first;
                passes += 1;
                if passes > MAX_PASSES {
                    self.reports.push(GraphReport::RouteHang { hub: from, table, destination: to, word: first });
                    return None;
                }
            }
        }
        Some(self.hubs[from].slots[(w & 7) as usize] as usize)
    }

    /// The uniform pick among the hub's other mask-compatible arcs
    /// (graph.js:135): Some(previous) when there is no other, None when
    /// the pick falls off the end, as the JavaScript returns them.
    pub fn alternate_opt(&self, hub: usize, previous: Option<usize>, mask: u32, random: &mut ClassicRandom) -> Option<usize> {
        let mut count = 0u32;
        for i in 0..8 {
            let s = self.hubs[hub].slots[i] as u32;
            if s == UNUSED {
                break;
            }
            let arc = s as usize;
            if (self.arcs[arc].mask & mask) != 0 && Some(arc) != previous {
                count += 1;
            }
        }
        if count == 0 {
            return previous;
        }
        let mut r = (random.next() % count) as i32;
        for i in 0..8 {
            let s = self.hubs[hub].slots[i] as u32;
            if s == UNUSED {
                continue;
            }
            let arc = s as usize;
            if (self.arcs[arc].mask & mask) == 0 || Some(arc) == previous {
                continue;
            }
            r -= 1;
            if r < 0 {
                return Some(arc);
            }
        }
        None
    }

    /// Attach a point to the graph (graph.js:160).
    pub fn attach(&self, p: Vec3, mask: u32) -> Attach {
        let mut out = Attach::default();
        let mut best = search_start();
        for hub in &self.hubs {
            if (hub.mask & mask) == 0 || !(hub.radius < best) {
                continue;
            }
            if !contains_hub(hub, p) {
                continue;
            }
            best = hub.radius;
            out.hub = Some(hub.index);
        }
        if best < search_start() {
            out.kind = 1;
            return out;
        }
        best = search_start();
        for arc in &self.arcs {
            if (arc.mask & mask) == 0 || arc.degenerate {
                continue;
            }
            let sum = f32(self.hubs[arc.a].radius + self.hubs[arc.b].radius);
            if !(sum < best) || !self.contains_arc(arc.index, p) {
                continue;
            }
            best = sum;
            out.arc = Some(arc.index);
        }
        if best < search_start() {
            out.kind = 2;
            return out;
        }
        out.arc = None;
        out
    }

    /// The compatible arc whose tapered corridor is nearest (graph.js:193).
    pub fn nearest_arc(&self, p: Vec3, mask: u32) -> Option<usize> {
        let mut best = far();
        let mut found = None;
        for arc in &self.arcs {
            if (arc.mask & mask) == 0 || arc.degenerate {
                continue;
            }
            let d = self.corridor_gap(arc.index, p);
            if d < best {
                best = d;
                found = Some(arc.index);
            }
        }
        found
    }

    /// Inside the arc's tapered corridor (graph.js:223).
    pub fn contains_arc(&self, arc: usize, p: Vec3) -> bool {
        let a = &self.hubs[self.arcs[arc].a];
        let b = &self.hubs[self.arcs[arc].b];
        let (abx, aby, abz) = (b.x - a.x, b.y - a.y, b.z - a.z);
        let t = f32((abx * (p.x - a.x) + aby * (p.y - a.y) + abz * (p.z - a.z)) / (abx * abx + aby * aby + abz * abz));
        if !(t >= 0.0 && t <= 1.0) {
            return false;
        }
        let dx = p.x - (a.x + t * abx);
        let dy = p.y - (a.y + t * aby);
        let dz = p.z - (a.z + t * abz);
        v8math::sqrt(dx * dx + dy * dy + dz * dz) < (1.0 - t) * a.radius + t * b.radius
    }

    /// Distance outside the corridor (graph.js:237).
    pub fn corridor_gap(&self, arc: usize, p: Vec3) -> f64 {
        let a = &self.hubs[self.arcs[arc].a];
        let b = &self.hubs[self.arcs[arc].b];
        let (abx, aby, abz) = (b.x - a.x, b.y - a.y, b.z - a.z);
        let t = f32((abx * (p.x - a.x) + aby * (p.y - a.y) + abz * (p.z - a.z)) / (abx * abx + aby * aby + abz * abz));
        if t >= 0.0 && t <= 1.0 {
            let r = (1.0 - t) * a.radius + t * b.radius;
            let dx = p.x - (a.x + t * abx);
            let dy = p.y - (a.y + t * aby);
            let dz = p.z - (a.z + t * abz);
            return v8math::max(0.0, v8math::sqrt(dx * dx + dy * dy + dz * dz) - r);
        }
        let end = if t < 0.0 { a } else { b };
        let dx = p.x - end.x;
        let dy = p.y - end.y;
        let dz = p.z - end.z;
        v8math::max(0.0, v8math::sqrt(dx * dx + dy * dy + dz * dz) - end.radius)
    }

    /// The other end of an arc from hub h (graph.js:209): hub b unless h
    /// is b... exactly as written, `h === arc.a ? arc.b : arc.a`.
    pub fn other_end(&self, arc: usize, h: usize) -> usize {
        let a = &self.arcs[arc];
        if h == a.a {
            a.b
        } else {
            a.a
        }
    }
}

/// Strictly inside the hub's sphere (graph.js:214).
pub fn contains_hub(hub: &Hub, p: Vec3) -> bool {
    let dx = p.x - hub.x;
    let dy = p.y - hub.y;
    let dz = p.z - hub.z;
    dx * dx + dy * dy + dz * dz < hub.radius * hub.radius
}
