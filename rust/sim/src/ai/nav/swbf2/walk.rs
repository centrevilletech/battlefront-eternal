//! Supported floors within the authored hub discs and directed corridors between them.
mod passage;
use super::super::{
    barriers::BarrierSet,
    math::{d2, h2, Vec3},
    route::RouteQuery,
};
use super::graph::{Swbf2Graph, Swbf2Hub, ARC_JET_JUMP, ARC_JUMP};
use passage::Bends;

pub(crate) const FLOOR_LIMIT: usize = 8;
pub(crate) const DROP: u8 = 8;
pub(crate) const PARAPET_DROP: u8 = ARC_JUMP | DROP;
pub(crate) const MAX_DROP: f64 = 6.;
/// A soldier's straight walk off a ledge deeper than a planned drop: as deep
/// as a floor can be found below it (`approach_floor` looks 8 m down).
pub(crate) const DEEP_DROP: u8 = DROP | 16;
pub(crate) const DEEP_DROP_LIMIT: f64 = 8.;

/// The walk off a ledge that falls `height`: a planned drop, a deep one, or
/// none (a step, or deeper than a floor is looked for).
pub(crate) fn drop_kind(height: f64) -> Option<u8> {
    if (0.75..=MAX_DROP).contains(&height) {
        Some(DROP)
    } else if height > MAX_DROP && height <= DEEP_DROP_LIMIT {
        Some(DEEP_DROP)
    } else {
        None
    }
}
const MAX_SURVEY_WORK: usize = 262_144;
/// Who asks for a ground check. All kinds share one allowance of collision
/// work per tick. What can wait for the next tick stops early and leaves the
/// rest to what cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeKind {
    /// Route questions. A question without an answer is Unknown.
    Question,
    /// Direct walks that combat asks about before it orders a pursuit.
    Direct,
    /// Route plans, bends and detours.
    Plan,
    /// Straight and direct orders and recovery steps.
    Step,
    /// A leg under way, the next leg of a resting soldier, and arrival.
    Leg,
    /// The next leg of a soldier that still moves along its last one.
    Turn,
}

impl ProbeKind {
    /// Two milliseconds of collision work on the reference x86-64 machine.
    pub(crate) const WORK_PER_TICK: usize = 10_000;
    pub(crate) const SLOTS: usize = 4;

    /// The work of a tick after which this kind finds none left.
    pub(crate) fn allowance(self) -> usize {
        match self {
            Self::Plan => Self::WORK_PER_TICK / 2,
            Self::Step => Self::WORK_PER_TICK * 2 / 3,
            Self::Question | Self::Direct | Self::Leg | Self::Turn => Self::WORK_PER_TICK,
        }
    }
    /// Whoever asks keeps its place until a later tick has the answer.
    pub(crate) fn waits(self) -> bool {
        matches!(self, Self::Plan | Self::Step | Self::Leg)
    }
    /// A soldier waits for this to begin a walk, not to go on with one.
    pub(crate) fn plans(self) -> bool {
        matches!(self, Self::Plan | Self::Step)
    }
    pub(crate) fn slot(self) -> usize {
        match self {
            Self::Plan => 0,
            Self::Step => 1,
            Self::Leg => 2,
            Self::Question | Self::Direct | Self::Turn => 3,
        }
    }
}

/// A route request owns these credits through all endpoint samples and replans.
#[derive(Clone, Copy, Debug)]
pub(super) struct SearchBudget {
    pub drives: usize,
    pub entries: usize,
    /// A check or a row ran out of this tick's shared work. The request keeps
    /// its credits and goes on in a later tick.
    pub waiting: bool,
}

impl SearchBudget {
    pub const ROW_ENTRIES: usize = 400_000;

    pub fn new() -> Self {
        Self {
            drives: 16,
            entries: Self::ROW_ENTRIES,
            waiting: false,
        }
    }

    pub fn spent(&self) -> bool {
        self.drives == 0 || self.entries == 0
    }

    /// No further search in this tick.
    pub fn stopped(&self) -> bool {
        self.waiting || self.spent()
    }

    pub fn share_rows<T>(
        &mut self,
        tick_left: &mut usize,
        search: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let available = self.entries.min(*tick_left);
        let reserved = self.entries - available;
        self.entries = available;
        let result = search(self);
        *tick_left -= available - self.entries;
        self.waiting |= self.entries == 0 && reserved != 0;
        self.entries += reserved;
        result
    }

    pub fn check(&mut self, test: impl FnOnce() -> Option<bool>) -> Option<bool> {
        spend_check(&mut self.drives, &mut self.waiting, test)
    }
}

/// An answered check costs one credit. One that has to wait costs nothing.
pub(super) fn spend_check(
    drives: &mut usize,
    waiting: &mut bool,
    test: impl FnOnce() -> Option<bool>,
) -> Option<bool> {
    if *waiting || *drives == 0 {
        return None;
    }
    let result = test();
    match result {
        Some(_) => *drives -= 1,
        None => *waiting = true,
    }
    result
}

/// The private soldier mover can advance this many fixed steps on a segment.
pub(crate) fn drive_limit(span: f64) -> usize {
    (span.min(256.) * 25.) as usize + 120
}

const MAX_EDGES: usize = 131_072;
// Round up only graph corridors. The two actual endpoints still use the
// asking soldier's footprint. Unusually large movers keep their own radius.
const BODY_RADII: [f64; 5] = [0.30, 0.40, 0.45, 0.48, 0.50];
const MAX_CACHE_ROWS: usize = 256;

fn corridor_radius(radius: f64) -> f64 {
    BODY_RADII
        .into_iter()
        .find(|&upper| radius <= upper)
        .unwrap_or(radius)
}
/// How far an endpoint may attach to a level, and how much height it may
/// have between.
fn node_reach(hub: &Swbf2Hub) -> f64 {
    v8math::max(hub.radius, 15.) + 12.
}
fn storey(node: Vec3, point: Vec3) -> f64 {
    0.9 + v8math::sqrt(h2(point.x, point.z, node.x, node.z)) * 0.55
}
/// How far an endpoint may join a corridor between two levels.
const CORRIDOR_REACH: f64 = 16.;

/// Where a point stands along a corridor (its polyline from the near node to the far one): the
/// segment whose clamped flat projection is nearest and that projection (height along the
/// segment). None when the nearest place is the near node itself (nothing to go on along).
fn corridor_onward(line: &[Vec3], point: Vec3) -> Option<(usize, Vec3)> {
    let mut best: Option<(f64, usize, Vec3)> = None;
    for (seg, pair) in line.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let span = dx * dx + dz * dz;
        if span < 0.01 {
            continue;
        }
        let t = v8math::min(v8math::max(((point.x - a.x) * dx + (point.z - a.z) * dz) / span, 0.), 1.);
        if seg == 0 && t <= 0. {
            continue;
        }
        let mid = Vec3::new(a.x + t * dx, a.y + t * (b.y - a.y), a.z + t * dz);
        let d = h2(point.x, point.z, mid.x, mid.z);
        if best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, seg, mid));
        }
    }
    best.map(|(_, seg, mid)| (seg, mid))
}
const ROW_BUDGET: usize = 8 * 1024 * 1024;
const NONE: usize = usize::MAX;

/// Direction matters at a step or drop. Compare all coordinates on a hit.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SegmentKey([u64; 6]);

#[derive(Clone, Default)]
pub(crate) struct Clearance {
    pub(crate) epoch: Option<(u64, u64)>,
    pub(crate) has_order: bool,
    pub(crate) order: Vec<u32>,
    pub(crate) flags: Vec<u8>,
    pub(crate) revision: u64,
    entries: Vec<Option<(SegmentKey, bool)>>,
}
impl std::fmt::Debug for Clearance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clearance")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}
impl Clearance {
    pub(crate) fn prepare(&mut self) {
        self.entries.resize(4096, None);
        self.flags
            .reserve(crate::collision::MAX_BODIES.saturating_sub(self.flags.len()));
        self.order
            .reserve(crate::collision::MAX_BODIES.saturating_sub(self.order.len()));
    }
    pub(crate) fn observe(&mut self, epoch: (u64, u64), flags: &[u8]) -> bool {
        if self.entries.is_empty() || flags.len() > self.flags.capacity() {
            return false;
        }
        if self.epoch != Some(epoch) || self.has_order || self.flags != flags {
            self.revision = self.revision.wrapping_add(1);
            self.entries.fill(None);
            self.epoch = Some(epoch);
            self.has_order = false;
            self.order.clear();
            self.flags.clear();
            self.flags.extend_from_slice(flags);
        }
        true
    }
    pub(crate) fn observe_order(
        &mut self,
        epoch: (u64, u64),
        order: &[u32],
        flags: &[u8],
    ) -> bool {
        if self.entries.is_empty()
            || order.len() != flags.len()
            || flags.len() > self.flags.capacity()
            || order.len() > self.order.capacity()
        {
            return false;
        }
        if self.epoch != Some(epoch)
            || !self.has_order
            || self.order != order
            || self.flags != flags
        {
            self.revision = self.revision.wrapping_add(1);
            self.entries.fill(None);
            self.epoch = Some(epoch);
            self.has_order = true;
            self.order.clear();
            self.order.extend_from_slice(order);
            self.flags.clear();
            self.flags.extend_from_slice(flags);
        }
        true
    }
    pub(super) fn try_check(
        &mut self,
        a: Vec3,
        b: Vec3,
        compute: impl FnOnce(Vec3, Vec3) -> Option<bool>,
    ) -> Option<bool> {
        let key = SegmentKey([a.x, a.y, a.z, b.x, b.y, b.z].map(f64::to_bits));
        let mut hash = 0x9e3779b97f4a7c15u64;
        for bits in key.0 {
            hash = (hash ^ bits)
                .rotate_left(21)
                .wrapping_mul(0x9e3779b97f4a7c15);
        }
        let i = ((hash ^ (hash >> 32)) as usize) & (self.entries.len() - 1);
        if let Some((found, value)) = self.entries[i] {
            if found == key {
                return Some(value);
            }
        }
        let value = compute(a, b)?;
        self.entries[i] = Some((key, value));
        Some(value)
    }
}

pub(crate) trait Survey {
    fn floors(&mut self, hub: &Swbf2Hub, out: &mut Vec<Vec3>);
    fn floor(&mut self, near: Vec3) -> Option<Vec3>;
    fn floor_sites(&mut self, hub: &Swbf2Hub, points: &mut Vec<Vec3>, _: &mut BarrierSet) {
        self.floors(hub, points);
    }
    fn doorway(&self, _a: Vec3, _b: Vec3) -> Option<Vec3> {
        None
    }
    fn open_landing(&mut self, _from: Vec3, _to: Vec3) -> bool {
        true
    }
    fn crosses(&mut self, from: Vec3, to: Vec3, flags: u8) -> bool;
}

// A standalone authored graph uses its recorded heights. Battle admission
// replaces this survey with the world's composed collision and soldier mover.
pub(crate) struct Authored;
impl Survey for Authored {
    fn floors(&mut self, hub: &Swbf2Hub, out: &mut Vec<Vec3>) {
        out.push(hub.pos());
    }
    fn floor(&mut self, p: Vec3) -> Option<Vec3> {
        Some(p)
    }
    fn crosses(&mut self, _: Vec3, _: Vec3, _: u8) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    hub: usize,
    point: Vec3,
}
#[derive(Clone, Copy, Debug)]
struct Edge {
    from: usize,
    to: usize,
    arc: usize,
    movement: u8,
    extra: Option<u32>,
    after_jump: u8,
    bend: Option<Vec3>,
    length: f64,
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Waypoints {
    points: [Vec3; 8],
    len: u8,
}

impl Waypoints {
    pub(super) const NONE: Self = Self {
        points: [Vec3 {
            x: 0.,
            y: 0.,
            z: 0.,
        }; 8],
        len: 0,
    };

    pub(super) fn one(point: Option<Vec3>) -> Self {
        let mut out = Self::NONE;
        if let Some(p) = point {
            out.points[0] = p;
            out.len = 1;
        }
        out
    }

    pub(super) fn two(first: Vec3, second: Vec3) -> Self {
        let mut out = Self::NONE;
        out.points[0] = first;
        out.points[1] = second;
        out.len = 2;
        out
    }

    pub(super) fn push(&mut self, point: Vec3) -> bool {
        let idx = usize::from(self.len);
        if idx >= self.points.len() {
            return false;
        }
        self.points[idx] = point;
        self.len += 1;
        true
    }

    pub(super) fn points(&self) -> &[Vec3] {
        &self.points[..usize::from(self.len)]
    }
}

pub(super) trait PointProbe {
    fn foot(&mut self, point: Vec3) -> Option<Vec3>;
    fn walk(&mut self, from: Vec3, to: Vec3, movement: u8) -> Option<bool>;
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Attachment {
    pub node: usize,
    pub bend: Waypoints,
    pub length: f64,
}
pub(super) type Attachments = [Option<Attachment>; 3];

#[derive(Clone, Copy, Debug)]
pub(super) struct Trip {
    pub start: Attachment,
    pub end: Attachment,
    pub direct: bool,
    pub direct_arc: Option<usize>,
    pub first_edge: Option<usize>,
    pub result: RouteQuery,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    goal: usize,
    mask: u32,
    jet: bool,
    radius: u64,
}
#[derive(Clone, Debug)]
struct Row {
    key: Option<Key>,
    next: Vec<usize>,
    distance: Vec<f64>,
    hops: Vec<u16>,
}

#[derive(Clone, Debug, Default)]
pub struct Floors {
    nodes: Vec<Node>,
    by_hub: Vec<std::ops::Range<usize>>,
    edges: Vec<Edge>,
    passages: Vec<Bends>,
    door_slots: Vec<usize>,
    door_boxes: Vec<[f64; 6]>,
    open_passages: Vec<(usize, u64)>,
    /// Levels that corridors join, whatever their direction or state.
    group: Vec<u32>,
    has_edge: Vec<bool>,
    clear_edges: Vec<Option<bool>>,
    // Per corridor, which soldier bands have been tested and which passed.
    barrier_bands: Vec<(u8, u8)>,
    incoming: Vec<Vec<usize>>,
    rows: Vec<Row>,
    used: Vec<bool>,
    replace: usize,
    approaches: Vec<Option<(usize, Waypoints)>>,
}

fn length(a: Vec3, b: Vec3) -> f64 {
    v8math::sqrt(d2(a, b))
}
// Shorten an ordinary leg only within the supported directed corridor. Keep
// the arc hop in the result so callers see that it crosses a connection.
fn along_corridor(a: Vec3, b: Vec3, from: Vec3, to: Vec3) -> bool {
    let (x, z) = (b.x - a.x, b.z - a.z);
    let span = x * x + z * z;
    if span < 0.001 {
        return false;
    }
    let project = |p: Vec3| {
        let t = ((p.x - a.x) * x + (p.z - a.z) * z) / span;
        let h = a.y + t * (b.y - a.y);
        let inside = (0.0..=1.0).contains(&t)
            && (h - p.y).abs() < 0.55
            && h2(a.x + t * x, a.z + t * z, p.x, p.z) < 2.25;
        (t, inside)
    };
    let (first, enters) = project(from);
    let (last, exits) = project(to);
    enters && exits && first <= last
}

pub(in crate::ai::nav) fn clear(
    barriers: &mut BarrierSet,
    a: Vec3,
    b: Vec3,
    r: f64,
    mask: u32,
) -> bool {
    barriers.point_clear(a, r, mask).0
        && barriers.point_clear(b, r, mask).0
        && !barriers.segment_blocked(a, b, r, mask, None)
}

/// Pure-barrier 1- or 2-corner detour around obstructing barriers between two
/// clear points, spending zero physical walk probes.
fn barrier_detour(
    barriers: &mut BarrierSet,
    from: Vec3,
    to: Vec3,
    radius: f64,
    mask: u32,
) -> Option<Waypoints> {
    let (hit, Some(b1), _) = barriers.nearest(from, to, radius, mask, None) else {
        return None;
    };
    if !hit {
        return None;
    }
    let mid_y = (from.y + to.y) * 0.5;
    let pick_corner = |barriers: &mut BarrierSet, bi: usize, k: usize, y: f64| {
        let mut c = super::super::barriers::corner(&barriers.list[bi], k, radius, 0.5);
        c.y = y;
        if barriers.point_clear(c, radius, mask).0 {
            return c;
        }
        let mut tight = super::super::barriers::corner(&barriers.list[bi], k, radius, 0.12);
        tight.y = y;
        tight
    };
    let mut c1s = [(f64::INFINITY, Vec3::default()); 4];
    for (k, slot) in c1s.iter_mut().enumerate() {
        let c = pick_corner(barriers, b1, k, mid_y);
        *slot = (length(from, c) + length(c, to), c);
    }
    for i in 0..4 {
        for j in i + 1..4 {
            if c1s[j].0 < c1s[i].0 {
                c1s.swap(i, j);
            }
        }
    }
    let mut clear_c1 = [false; 4];
    for (i, &(_, c1)) in c1s.iter().enumerate() {
        if clear(barriers, from, c1, radius, mask) {
            clear_c1[i] = true;
            if clear(barriers, c1, to, radius, mask) {
                return Some(Waypoints::one(Some(c1)));
            }
        }
    }
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    for (i, &(_, c1)) in c1s.iter().enumerate() {
        if !clear_c1[i] {
            continue;
        }
        let (hit2, Some(b2), _) = barriers.nearest(c1, to, radius, mask, None) else {
            continue;
        };
        if !hit2 {
            continue;
        }
        for k2 in 0..4 {
            let c2 = pick_corner(barriers, b2, k2, to.y);
            if d2(c1, c2) < 0.04 {
                continue;
            }
            let total = length(from, c1) + length(c1, c2) + length(c2, to);
            if best.is_some_and(|(d, ..)| total >= d) {
                continue;
            }
            if clear(barriers, c1, c2, radius, mask) && clear(barriers, c2, to, radius, mask) {
                best = Some((total, c1, c2));
            }
        }
    }
    best.map(|(_, c1, c2)| Waypoints::two(c1, c2))
}

pub(super) fn handoff_bends(from: Vec3, to: Vec3) -> impl Iterator<Item = Vec3> {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    let span = v8math::max(v8math::hypot(dx, dz), 0.001);
    let long = span > 25.;
    let climb_crease = [0.12, 0.25, 0.5]
        .into_iter()
        .filter(move |_| to.y - from.y > 0.75 && !long)
        .flat_map(move |t| {
            [0.15, -0.15, 0.35, -0.35, 0.65, -0.65]
                .into_iter()
                .map(move |offset| {
                    Vec3::new(
                        from.x + t * dx - offset * dz / span,
                        from.y + t * (to.y - from.y),
                        from.z + t * dz + offset * dx / span,
                    )
                })
        });
    let near_long = [0.125, 0.25]
        .into_iter()
        .filter(move |_| long)
        .flat_map(move |t| {
            [5., -5., 8., -8., 11.5, -11.5]
                .into_iter()
                .map(move |offset| {
                    Vec3::new(
                        from.x + t * dx - offset * dz / span,
                        from.y + t * (to.y - from.y),
                        from.z + t * dz + offset * dx / span,
                    )
                })
        });
    climb_crease.chain(near_long).chain(bends(from, to))
}

pub(super) fn bends(from: Vec3, to: Vec3) -> impl Iterator<Item = Vec3> {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    let norm = v8math::max(v8math::hypot(dx, dz), 0.001);
    [0.5, 0.25, 0.75].into_iter().flat_map(move |fraction| {
        [1.2, -1.2, 2.8, -2.8, 5., -5., 8., -8.]
            .into_iter()
            .map(move |offset| {
                Vec3::new(
                    from.x + fraction * dx - dz / norm * offset,
                    from.y + fraction * (to.y - from.y),
                    from.z + fraction * dz + dx / norm * offset,
                )
            })
    })
}

pub(crate) fn meets(a: Vec3, b: Vec3, box_: &[f64; 6]) -> bool {
    let low = [
        v8math::min(a.x, b.x) - 0.6,
        v8math::min(a.y, b.y) - 0.6,
        v8math::min(a.z, b.z) - 0.6,
    ];
    let high = [
        v8math::max(a.x, b.x) + 0.6,
        v8math::max(a.y, b.y) + 2.,
        v8math::max(a.z, b.z) + 0.6,
    ];
    (0..3).all(|k| high[k] >= box_[k] && low[k] <= box_[k + 3])
}

impl Floors {
    /// Pick a surveyed floor point inside `hub` from a uniform `pick` in `[0, 1)`.
    pub(crate) fn hub_floor_point(&self, hub: usize, pick: f64) -> Option<Vec3> {
        let range = self.by_hub.get(hub)?;
        let count = range.end.saturating_sub(range.start);
        if count == 0 {
            return None;
        }
        let offset = (v8math::floor(pick * count as f64) as usize).min(count - 1);
        self.nodes.get(range.start + offset).map(|n| n.point)
    }

    pub(crate) fn build(
        graph: &Swbf2Graph,
        barriers: &mut BarrierSet,
        survey: &mut impl Survey,
    ) -> Result<Self, String> {
        let mut out = Self::default();
        let mut points = Vec::with_capacity(FLOOR_LIMIT);
        for hub in &graph.hubs {
            points.clear();
            if hub.valid {
                survey.floor_sites(hub, &mut points, barriers);
            }
            let start = out.nodes.len();
            out.nodes
                .extend(points.iter().take(FLOOR_LIMIT).map(|&point| Node {
                    hub: hub.index,
                    point,
                }));
            out.by_hub.push(start..out.nodes.len());
        }
        let count = out.nodes.len();
        out.incoming.resize_with(count, Vec::new);
        let mut remaining = MAX_SURVEY_WORK;
        let mut charge = || {
            remaining = remaining
                .checked_sub(1)
                .ok_or("planning floor survey exceeds work limit")?;
            Ok::<(), String>(())
        };
        for arc in graph
            .arcs
            .iter()
            .filter(|a| !a.degenerate && !a.jump() && !a.jet_jump())
        {
            for (a, b) in [(arc.start, arc.end), (arc.end, arc.start)] {
                if a == arc.end && arc.one_way() {
                    continue;
                }
                let flags = arc.type_flags & (ARC_JUMP | ARC_JET_JUMP);
                for from in out.by_hub[a].clone() {
                    for to in out.by_hub[b].clone() {
                        charge()?;
                        let p = out.nodes[from].point;
                        let q = out.nodes[to].point;
                        let span = v8math::hypot(q.x - p.x, q.z - p.z);
                        if (q.y - p.y).abs() > span + 1. && flags == 0 {
                            continue;
                        }
                        let straight = survey.crosses(p, q, flags);
                        if straight {
                            out.add(from, to, arc.index, None, flags)?;
                        }
                        if flags != 0 {
                            continue;
                        }
                        let mut needed = arc.filter_flags & 31;
                        let class_bits = [1, 2, 4, 8, 16];
                        if straight {
                            for bit in class_bits {
                                if needed & bit != 0 && clear(barriers, p, q, 0.55, bit) {
                                    needed &= !bit;
                                }
                            }
                        }
                        let near_ramp = (q.y - p.y).abs() > 1.0 && span > 25.;
                        let ramp_bends = [0.125, 0.875]
                            .into_iter()
                            .filter(move |_| near_ramp)
                            .flat_map(move |fraction| {
                                let dx = q.x - p.x;
                                let dz = q.z - p.z;
                                let norm = v8math::max(v8math::hypot(dx, dz), 0.001);
                                [1.2, -1.2, 2.8, -2.8, 5., -5., 8., -8.]
                                    .into_iter()
                                    .flat_map(move |offset| {
                                        let bx = p.x + fraction * dx - dz / norm * offset;
                                        let bz = p.z + fraction * dz + dx / norm * offset;
                                        [(p.y + q.y) * 0.5, q.y]
                                            .into_iter()
                                            .map(move |by| Vec3::new(bx, by, bz))
                                    })
                            });
                        for candidate in bends(p, q).chain(ramp_bends) {
                            if needed == 0 {
                                break;
                            }
                            charge()?;
                            let Some(mid) = survey.floor(candidate) else {
                                continue;
                            };
                            let mut served = 0;
                            for bit in class_bits {
                                if needed & bit != 0
                                    && clear(barriers, p, mid, 0.55, bit)
                                    && clear(barriers, mid, q, 0.55, bit)
                                {
                                    served |= bit;
                                }
                            }
                            if served != 0 && survey.crosses(p, mid, 0) && survey.crosses(mid, q, 0)
                            {
                                out.add(from, to, arc.index, Some(mid), 0)?;
                                needed &= !served;
                            }
                        }
                        if needed & 1 != 0 {
                            if let Some(door) = survey.doorway(p, q) {
                                if let Some(path) =
                                    passage::passage(survey, barriers, p, door, q, &mut charge)?
                                {
                                    out.add_passage(from, to, arc.index, path)?;
                                }
                            }
                        }
                    }
                }
            }
        }
        // A step down may leave the floor briefly. Only offer it on the
        // reverse of a demonstrated climb, with a bounded fall and landing.
        for i in 0..out.edges.len() {
            let e = out.edges[i];
            let low = out.nodes[e.from].point;
            let high = out.nodes[e.to].point;
            let height = high.y - low.y;
            if e.extra.is_some()
                || graph.arcs[e.arc].one_way()
                || !(0.75..=MAX_DROP).contains(&height)
                || out.incoming[e.from]
                    .iter()
                    .any(|&j| out.edges[j].from == e.to && out.edges[j].arc == e.arc)
            {
                continue;
            }
            charge()?;
            let via = e.bend.unwrap_or(low);
            if survey.crosses(high, via, DROP)
                && (e.bend.is_none() || survey.crosses(via, low, DROP))
            {
                out.add(e.to, e.from, e.arc, e.bend, DROP)?;
            }
        }
        // A shared planning plane does not specify an elevation. The ordinary
        // incoming and outgoing walks identify its launch and landing levels.
        let mut exits = vec![false; count];
        for e in &out.edges {
            exits[e.from] = true;
        }
        for arc in graph
            .arcs
            .iter()
            .filter(|a| !a.degenerate && (a.jump() || a.jet_jump()))
        {
            let flags = arc.type_flags & (ARC_JUMP | ARC_JET_JUMP);
            for (a, b) in [(arc.start, arc.end), (arc.end, arc.start)] {
                if a == arc.end && arc.one_way() {
                    continue;
                }
                let level = |hub: usize, entering: bool| {
                    let range = out.by_hub[hub].clone();
                    let only = range.len() == 1;
                    let authored_levels = (graph.hubs[a].y - graph.hubs[b].y).abs() > 0.55;
                    let mut levels = range.filter(|&n| {
                        if authored_levels {
                            (out.nodes[n].point.y - graph.hubs[hub].y).abs() <= 0.55
                        } else {
                            only || if entering {
                                !out.incoming[n].is_empty()
                            } else {
                                exits[n]
                            }
                        }
                    });
                    let candidate = levels.next()?;
                    levels.next().is_none().then_some(candidate)
                };
                let (Some(from), Some(to)) = (level(a, true), level(b, false)) else {
                    continue;
                };
                charge()?;
                let (p, q) = (out.nodes[from].point, out.nodes[to].point);
                if survey.open_landing(p, q) && survey.crosses(p, q, flags) {
                    out.add(from, to, arc.index, None, flags)?;
                    continue;
                }
                // A landing hub can be under the lip of the launch platform.
                // Jump onto exposed floor first and walk back under the lip.
                if p.y - q.y > 2. {
                    let (dx, dz) = (q.x - p.x, q.z - p.z);
                    let span = v8math::hypot(dx, dz).max(0.01);
                    let (ux, uz) = (dx / span, dz / span);
                    let mut touchdown = None;
                    'landing: for along in [2., 4., 6., 8.] {
                        for across in [0., -2., 2., -4., 4.] {
                            charge()?;
                            let at = Vec3::new(
                                q.x + along * ux - across * uz,
                                q.y,
                                q.z + along * uz + across * ux,
                            );
                            let Some(land) = survey.floor(at).filter(|v| (v.y - q.y).abs() < 0.55)
                            else {
                                continue;
                            };
                            if survey.open_landing(p, land)
                                && survey.crosses(land, q, 0)
                                && survey.crosses(p, land, flags)
                            {
                                touchdown = Some(land);
                                break 'landing;
                            }
                        }
                    }
                    if let Some(land) = touchdown {
                        out.add(from, to, arc.index, Some(land), flags)?;
                        out.edges.last_mut().unwrap().after_jump = 1;
                        continue;
                    }
                }
                // The takeoff can lie toward the lip of its authored disc.
                // Remain on the identified launch storey until the jump.
                'launch: for fraction in [0.4, 0.8] {
                    for [x, z] in [
                        [1., 0.],
                        [-1., 0.],
                        [0., 1.],
                        [0., -1.],
                        [0.707, 0.707],
                        [-0.707, 0.707],
                        [0.707, -0.707],
                        [-0.707, -0.707],
                    ] {
                        charge()?;
                        let hub = &graph.hubs[a];
                        let at = Vec3::new(
                            hub.x + x * fraction * hub.radius,
                            p.y,
                            hub.z + z * fraction * hub.radius,
                        );
                        if let Some(lip) = survey.floor(at).filter(|v| (v.y - p.y).abs() < 0.5) {
                            if survey.open_landing(lip, q)
                                && survey.crosses(p, lip, 0)
                                && survey.crosses(lip, q, flags)
                            {
                                out.add(from, to, arc.index, Some(lip), flags)?;
                                break 'launch;
                            }
                        }
                    }
                }
            }
        }
        out.clear_edges.resize(out.edges.len(), Some(true));
        out.join_groups();
        out.barrier_bands.resize(out.edges.len(), (0, 0));
        out.used.resize(count, false);
        out.approaches
            .resize(graph.hubs.len() * 2 * FLOOR_LIMIT, None);
        let row_bytes = count * (size_of::<usize>() + size_of::<f64>() + size_of::<u16>());
        let affordable = ROW_BUDGET / row_bytes.max(1);
        let common = count * BODY_RADII.len();
        let rows = if common > 0 && common < affordable && count <= 512 && out.edges.len() <= 2048 {
            common + 1 // separate scratch row for other masks, radii and jet routes
        } else {
            MAX_CACHE_ROWS.min(count.max(1)).min(affordable)
        };
        out.rows = (0..rows)
            .map(|_| Row {
                key: None,
                next: vec![NONE; count],
                distance: vec![f64::INFINITY; count],
                hops: vec![0; count],
            })
            .collect();
        Ok(out)
    }

    fn join_groups(&mut self) {
        self.group = (0..self.nodes.len() as u32).collect();
        self.has_edge = vec![false; self.nodes.len()];
        for edge in &self.edges {
            self.has_edge[edge.from] = true;
            self.has_edge[edge.to] = true;
            let (a, b) = (self.group[edge.from], self.group[edge.to]);
            if a != b {
                for group in &mut self.group {
                    if *group == a.max(b) {
                        *group = a.min(b);
                    }
                }
            }
        }
    }

    /// The groups that an attachment at `point` could join, widened by `slack`.
    fn groups_near(&self, graph: &Swbf2Graph, point: Vec3, slack: f64, mask: u32) -> [u64; 32] {
        let mut groups = [0; 32];
        let mut join = |group: u32| groups[group as usize / 64] |= 1 << (group % 64);
        for (node, &group) in self.nodes.iter().zip(&self.group) {
            let hub = &graph.hubs[node.hub];
            let reach = node_reach(hub) + slack;
            if hub.mask & mask != 0
                && d2(node.point, point) <= reach * reach
                && (node.point.y - point.y).abs() <= storey(node.point, point) + slack
            {
                join(group);
            }
        }
        let reach = CORRIDOR_REACH + slack;
        for edge in &self.edges {
            let mut project = |a: Vec3, b: Vec3| {
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let span = dx * dx + dz * dz;
                let t = ((point.x - a.x) * dx + (point.z - a.z) * dz) / v8math::max(span, 0.01);
                let t = v8math::min(v8math::max(t, 0.), 1.);
                let mid = Vec3::new(a.x + t * dx, a.y + t * (b.y - a.y), a.z + t * dz);
                if h2(point.x, point.z, mid.x, mid.z) <= reach * reach
                    && (point.y - mid.y).abs() <= 2.5 + slack
                {
                    join(self.group[edge.from]);
                }
            };
            let (a, b) = (self.nodes[edge.from].point, self.nodes[edge.to].point);
            if let Some(j) = edge.extra {
                let mut prev = a;
                for &next in self.passages[j as usize].points() {
                    project(prev, next);
                    prev = next;
                }
                project(prev, b);
            } else if let Some(bend) = edge.bend {
                project(a, bend);
                project(bend, b);
            } else {
                project(a, b);
            }
        }
        groups
    }

    /// Whether any entrance a search could use at `from` shares floor with any
    /// exit it could use at `to`. A search across unconnected decks finds
    /// nothing, so it needs no physical check.
    pub(super) fn may_connect(
        &self,
        graph: &Swbf2Graph,
        from: Vec3,
        to: Vec3,
        slack: f64,
        mask: u32,
    ) -> bool {
        self.may_connect_within(graph, from, to, [0., slack], mask)
    }

    pub(super) fn may_connect_within(
        &self,
        graph: &Swbf2Graph,
        from: Vec3,
        to: Vec3,
        slack: [f64; 2],
        mask: u32,
    ) -> bool {
        if self.nodes.len() > 32 * 64 {
            return true;
        }
        let entrances = self.groups_near(graph, from, slack[0], mask);
        let exits = self.groups_near(graph, to, slack[1], mask);
        entrances.iter().zip(&exits).any(|(a, b)| a & b != 0)
    }

    pub(crate) fn warm(&mut self, graph: &Swbf2Graph, barriers: &mut BarrierSet) {
        if self.nodes.len() > 512 || self.edges.len() > 2048 {
            return;
        }
        for &radius in &BODY_RADII {
            for goal in 0..self.nodes.len() {
                let key = Key {
                    goal,
                    mask: 1,
                    jet: false,
                    radius: radius.to_bits(),
                };
                if self.direct_row(key).is_none() {
                    return;
                }
                self.row_index(
                    graph,
                    barriers,
                    key,
                    &mut |_, _, _| None,
                    &mut SearchBudget::new().entries,
                    None,
                );
            }
        }
    }

    fn add(
        &mut self,
        from: usize,
        to: usize,
        arc: usize,
        bend: Option<Vec3>,
        movement: u8,
    ) -> Result<(), String> {
        if self.edges.len() == MAX_EDGES {
            return Err("planning graph exceeds the supported floor corridor limit".into());
        }
        let p = self.nodes[from].point;
        let q = self.nodes[to].point;
        let distance = bend.map_or_else(|| length(p, q), |m| length(p, m) + length(m, q));
        self.incoming[to].push(self.edges.len());
        self.edges.push(Edge {
            from,
            to,
            arc,
            movement,
            extra: None,
            after_jump: 0,
            bend,
            length: distance,
        });
        Ok(())
    }

    fn add_passage(
        &mut self,
        from: usize,
        to: usize,
        arc: usize,
        via: Bends,
    ) -> Result<(), String> {
        let points = via.points();
        self.add(from, to, arc, points.first().copied(), 0)?;
        if points.len() > 1 {
            if self.passages.len() >= 4096 {
                return Err("too many indoor navigation passages".into());
            }
            let edge = self.edges.last_mut().unwrap();
            edge.extra = Some(self.passages.len() as u32);
            self.passages.push(via);
            let mut p = self.nodes[from].point;
            edge.length = 0.;
            for &q in points.iter().chain(std::iter::once(&self.nodes[to].point)) {
                edge.length += length(p, q);
                p = q;
            }
        }
        Ok(())
    }

    pub(crate) fn invalidate(&mut self) {
        self.barrier_bands.fill((0, 0));
        for row in &mut self.rows {
            row.key = None;
        }
    }

    pub(crate) fn record_open_doors(&mut self, slots: &[usize], boxes: &[[f64; 6]]) {
        if slots.is_empty() || slots.len() > 64 {
            return;
        }
        self.door_slots.extend_from_slice(slots);
        self.door_boxes.extend_from_slice(boxes);
        for (i, e) in self.edges.iter().enumerate() {
            if e.movement & ARC_JUMP != 0 {
                continue;
            }
            let via = e
                .extra
                .map_or_else(|| Bends::one(e.bend), |j| self.passages[j as usize]);
            let mut start = self.nodes[e.from].point;
            let mut mask = 0;
            for &end in via
                .points()
                .iter()
                .chain(std::iter::once(&self.nodes[e.to].point))
            {
                for (j, bb) in boxes.iter().enumerate() {
                    if meets(start, end, bb) {
                        mask |= 1u64 << j;
                    }
                }
                start = end;
            }
            if mask != 0 {
                self.open_passages.push((i, mask));
            }
        }
    }
    pub(crate) fn is_surveyed_door(&self, slot: usize) -> bool {
        self.door_slots.contains(&slot)
    }
    pub(crate) fn restore_open_passages(&mut self, _flags: &[u8]) {
        // Surveyed proximity-door passages stay plannable when barriers and
        // planning groups allow them. Locked doors stay gated by barriers or
        // arcs, and live leg execution still requires the shutter to clear.
        for &(edge, _) in &self.open_passages {
            self.clear_edges[edge] = Some(true);
        }
    }
    pub(super) fn closed_door_center(&self, a: Vec3, b: Vec3, flags: &[u8]) -> Option<Vec3> {
        let mut best: Option<(f64, Vec3)> = None;
        for (j, (&slot, bb)) in self.door_slots.iter().zip(&self.door_boxes).enumerate() {
            if flags.get(slot).copied().unwrap_or(0) == 0
                || !self.open_passages.iter().any(|&(_, m)| m & (1u64 << j) != 0)
                || !meets(a, b, bb)
            {
                continue;
            }
            let center = Vec3::new((bb[0] + bb[3]) * 0.5, a.y, (bb[2] + bb[5]) * 0.5);
            let dist = d2(a, center);
            if best.is_none_or(|(d, _)| dist < d) {
                best = Some((dist, center));
            }
        }
        best.map(|(_, c)| c)
    }
    pub(crate) fn forget_open_passages(&mut self) {
        self.open_passages.clear();
        self.door_slots.clear();
        self.door_boxes.clear();
    }

    fn edge_meets(&self, edge: Edge, bounds: &[[f64; 6]]) -> bool {
        if bounds.is_empty() {
            return false;
        }
        let a = self.nodes[edge.from].point;
        let b = self.nodes[edge.to].point;
        let via = edge
            .extra
            .map_or_else(|| Bends::one(edge.bend), |i| self.passages[i as usize]);
        let mut p = a;
        for &q in via.points().iter().chain(std::iter::once(&b)) {
            if bounds.iter().any(|bb| meets(p, q, bb)) {
                return true;
            }
            p = q;
        }
        false
    }

    pub(crate) fn apply_static_changes(
        &mut self,
        door_bounds: &[[f64; 6]],
        other_bounds: &[[f64; 6]],
    ) -> bool {
        if !other_bounds.is_empty() {
            let nodes = &self.nodes;
            let edges = &self.edges;
            let passages = &self.passages;
            self.open_passages.retain(|&(i, _)| {
                let edge = edges[i];
                let a = nodes[edge.from].point;
                let b = nodes[edge.to].point;
                let via = edge
                    .extra
                    .map_or_else(|| Bends::one(edge.bend), |k| passages[k as usize]);
                let mut p = a;
                for &q in via.points().iter().chain(std::iter::once(&b)) {
                    if other_bounds.iter().any(|bb| meets(p, q, bb)) {
                        return false;
                    }
                    p = q;
                }
                true
            });
        }
        let mut changed = false;
        for i in 0..self.edges.len() {
            let edge = self.edges[i];
            if !self.edge_meets(edge, other_bounds) && !self.edge_meets(edge, door_bounds) {
                continue;
            }
            let next = if self.open_passages.iter().any(|&(e, _)| e == i) {
                Some(true)
            } else {
                (edge.movement & ARC_JUMP != 0).then_some(false)
            };
            if self.clear_edges[i] != next {
                self.clear_edges[i] = next;
                changed = true;
            }
        }
        if changed {
            self.invalidate();
        }
        changed
    }

    pub(crate) fn changed_bodies(&mut self, bounds: &[[f64; 6]]) {
        for i in 0..self.edges.len() {
            let edge = self.edges[i];
            if self.edge_meets(edge, bounds) {
                // A runtime walk probe has only one mover. A changed jump
                // needs the full set of admission trials to be certified.
                self.clear_edges[i] = (edge.movement & ARC_JUMP != 0).then_some(false);
            }
        }
        self.invalidate();
    }

    pub(crate) fn collision_changed(&mut self) {
        self.forget_open_passages();
        for (edge, clear) in self.edges.iter().zip(&mut self.clear_edges) {
            *clear = (edge.movement & ARC_JUMP != 0).then_some(false);
        }
        self.invalidate();
    }

    fn direct_row(&self, key: Key) -> Option<usize> {
        if self.rows.len() <= self.nodes.len() * BODY_RADII.len() || key.mask != 1 || key.jet {
            return None;
        }
        let class = BODY_RADII.iter().position(|r| r.to_bits() == key.radius)?;
        Some(class * self.nodes.len() + key.goal)
    }

    fn row_index(
        &mut self,
        graph: &Swbf2Graph,
        barriers: &mut BarrierSet,
        key: Key,
        walk: &mut impl FnMut(Vec3, Vec3, u8) -> Option<bool>,
        work: &mut usize,
        starts: Option<&[Option<Attachment>; 3]>,
    ) -> Option<usize> {
        let index = if let Some(fixed) = self.direct_row(key) {
            fixed
        } else if self.rows.len() > self.nodes.len() * BODY_RADII.len() {
            self.rows.len() - 1
        } else {
            if let Some(hit) = self.rows.iter().position(|r| r.key == Some(key)) {
                return Some(hit);
            }
            let index = self.replace;
            self.replace = (index + 1) % self.rows.len();
            index
        };
        if self.rows[index].key == Some(key) {
            return Some(index);
        }
        let row = &mut self.rows[index];
        row.key = None;
        row.next.fill(NONE);
        row.distance.fill(f64::INFINITY);
        row.hops.fill(0);
        self.used.fill(false);
        row.distance[key.goal] = 0.;
        // Allow four centimetres around the surveyed corner. Earlier turns
        // also check the outgoing segment from the soldier's actual position.
        let radius = f64::from_bits(key.radius);
        let band = if key.mask == 1 {
            BODY_RADII
                .iter()
                .position(|r| r.to_bits() == key.radius)
                .map(|i| 1u8 << i)
        } else {
            None
        };
        loop {
            if row.distance.len() > *work {
                *work = 0;
                return None;
            }
            *work -= row.distance.len();
            let mut cost = f64::INFINITY;
            let mut nearest = NONE;
            for (i, &distance) in row.distance.iter().enumerate() {
                if !self.used[i] && distance < cost {
                    nearest = i;
                    cost = distance;
                }
            }
            if nearest == NONE {
                break;
            }
            self.used[nearest] = true;
            if starts.is_some_and(|s| s.iter().flatten().all(|a| self.used[a.node])) {
                // All requested distances are final. Leave this partial row unkeyed.
                return Some(index);
            }
            if self.incoming[nearest].len() > *work {
                *work = 0;
                return None;
            }
            *work -= self.incoming[nearest].len();
            for &i in &self.incoming[nearest] {
                let edge = self.edges[i];
                let from = self.nodes[edge.from];
                let dest = self.nodes[nearest].point;
                if self.used[edge.from]
                    || !graph.arc_traversable(edge.arc, from.hub, key.mask, key.jet)
                {
                    continue;
                }
                let next = cost + edge.length;
                if next >= row.distance[edge.from] {
                    continue;
                }
                let via = edge
                    .extra
                    .map_or_else(|| Bends::one(edge.bend), |i| self.passages[i as usize]);
                let points = via.points();
                let radius = if points.is_empty() {
                    radius
                } else {
                    radius + 0.05
                };
                let segments = std::iter::once(&from.point)
                    .chain(points)
                    .zip(points.iter().chain(std::iter::once(&dest)));
                let mut test = || {
                    segments
                        .clone()
                        .all(|(&p, &q)| clear(barriers, p, q, radius, key.mask))
                };
                let allowed = if let Some(bit) = band {
                    let (tested, passed) = &mut self.barrier_bands[i];
                    if *tested & bit == 0 {
                        *tested |= bit;
                        if test() {
                            *passed |= bit;
                        }
                    }
                    *passed & bit != 0
                } else {
                    test()
                };
                if !allowed {
                    continue;
                }
                let open = match self.clear_edges[i] {
                    Some(open) => open,
                    None => {
                        let mut open = true;
                        for (n, (&p, &q)) in segments.enumerate() {
                            let movement = if n + usize::from(edge.after_jump) == points.len()
                                || edge.movement == DROP
                            {
                                edge.movement
                            } else {
                                0
                            };
                            if !walk(p, q, movement)? {
                                open = false;
                                break;
                            }
                        }
                        self.clear_edges[i] = Some(open);
                        open
                    }
                };
                if !open {
                    continue;
                }
                row.next[edge.from] = i;
                row.distance[edge.from] = next;
                row.hops[edge.from] = row.hops[nearest] + 1;
            }
        }
        row.key = Some(key);
        Some(index)
    }

    fn home(graph: &Swbf2Graph, p: Vec3) -> Option<&Swbf2Hub> {
        graph.hubs.iter().filter(|h| h.valid).min_by(|a, b| {
            h2(a.x, a.z, p.x, p.z)
                .total_cmp(&h2(b.x, b.z, p.x, p.z))
                .then(a.index.cmp(&b.index))
        })
    }

    pub(super) fn disconnected(
        &self,
        graph: &Swbf2Graph,
        from: Vec3,
        to: Vec3,
        mask: u32,
        jet: bool,
    ) -> bool {
        let (Some(a), Some(b)) = (Self::home(graph, from), Self::home(graph, to)) else {
            return false;
        };
        if graph
            .hub_route_distance(a.index, b.index, mask, jet)
            .reachable
        {
            return false;
        }
        // A point inside a unique disc can still enter a nearby junction.
        // Every plausible entrance must be downstream of this start disc,
        // and every plausible exit upstream of this goal disc.
        let certain = |p, home: &Swbf2Hub, enter| {
            graph.contains_hub_2d(home.index, p)
                && self.nodes.iter().all(|node| {
                    if node.hub == home.index {
                        return true;
                    }
                    let hub = &graph.hubs[node.hub];
                    let reach = hub.radius.max(15.) + 12.;
                    if h2(p.x, p.z, node.point.x, node.point.z) > reach * reach {
                        return true;
                    }
                    let (a, b) = if enter {
                        (home.index, node.hub)
                    } else {
                        (node.hub, home.index)
                    };
                    graph.hub_route_distance(a, b, mask, jet).reachable
                })
        };
        certain(from, a, true) && certain(to, b, false)
    }

    /// Check exact endpoint geometry before reusing a nearby approach.
    /// The stored node and bend are suggestions for the same disc and storey.
    #[allow(clippy::too_many_arguments)]
    fn point_attachments(
        &mut self,
        graph: &Swbf2Graph,
        barriers: &mut BarrierSet,
        point: Vec3,
        radius: f64,
        mask: u32,
        enter: bool,
        nearby: bool,
        allowed_groups: Option<&[u64; 32]>,
        probe: &mut impl PointProbe,
    ) -> [Option<Attachment>; 3] {
        let mut result = [None; 3];
        if !barriers.point_clear(point, radius, mask).0 {
            return result;
        }
        let Some(home) = Self::home(graph, point) else {
            return result;
        };
        let group_ok = |group: u32| {
            allowed_groups
                .is_none_or(|bits| bits[group as usize / 64] & (1 << (group % 64)) != 0)
        };
        let mut nodes = [(f64::INFINITY, NONE); 10];
        for (i, node) in self.nodes.iter().enumerate() {
            let h = &graph.hubs[node.hub];
            let d = d2(node.point, point);
            let reach = node_reach(h);
            if h.mask & mask == 0
                || !group_ok(self.group[i])
                || d >= nodes[9].0
                || d > reach * reach
                || (node.point.y - point.y).abs() > storey(node.point, point)
            {
                continue;
            }
            let mut candidate = (d, i);
            for n in &mut nodes {
                if candidate < *n {
                    std::mem::swap(n, &mut candidate);
                }
            }
        }
        let slot = nodes.iter().find(|(_, n)| *n != NONE).map(|(_, closest)| {
            let level = self.by_hub[self.nodes[*closest].hub].start;
            (2 * home.index + usize::from(enter)) * FLOOR_LIMIT + (*closest - level)
        });
        let permitted = |node: usize| {
            let h = self.nodes[node].hub;
            let (a, b) = if enter {
                (home.index, h)
            } else {
                (h, home.index)
            };
            h == home.index
                || ((graph.hub_route_distance(a, b, mask, false).reachable
                    || !graph.contains_hub_2d(home.index, point)
                    || graph.contains_hub_2d(h, point))
                    && graph.attachment_permitted(home.index, h, point, mask))
        };
        let test = |node: usize,
                    bend: Waypoints,
                    barriers: &mut BarrierSet,
                    probe: &mut dyn PointProbe|
         -> Result<Option<Attachment>, ()> {
            if !permitted(node) {
                return Ok(None);
            }
            let p = self.nodes[node].point;
            let (a, b) = if enter { (point, p) } else { (p, point) };
            let pts = bend.points();
            let mut prev = a;
            for &next in pts.iter().chain(std::iter::once(&b)) {
                if !clear(barriers, prev, next, radius, mask) {
                    return Ok(None);
                }
                prev = next;
            }
            let mut prev = a;
            let mut total = 0.;
            for &next in pts.iter().chain(std::iter::once(&b)) {
                if !probe.walk(prev, next, 0).ok_or(())? {
                    return Ok(None);
                }
                total += length(prev, next);
                prev = next;
            }
            Ok(Some(Attachment {
                node,
                bend,
                length: total,
            }))
        };
        let mut n = 0;
        for (_, node) in nodes.into_iter().take_while(|(_, n)| *n != NONE) {
            if result[..n].iter().flatten().any(|a| a.node == node) {
                continue;
            }
            let Ok(attachment) = test(node, Waypoints::NONE, barriers, probe) else {
                return result;
            };
            if let Some(a) = attachment {
                if n == 0 {
                    if let Some(slot) = slot {
                        self.approaches[slot] = Some((node, Waypoints::NONE));
                    }
                }
                result[n] = Some(a);
                n += 1;
                if n == 3
                    || (!nearby
                        && self.has_edge[node]
                        && self.nodes[node].hub == home.index
                        && graph.contains_hub_2d(home.index, point))
                {
                    break;
                }
            }
        }
        if enter && n > 0 && n < 3 {
            // The game attaches a start to its arc mid-arc: a soldier standing on a corridor
            // that leaves an attached node may go on along it to the node at its far end
            // instead of walking back first. Offer that far node through the rest of the
            // corridor; the trip keeps whichever start gives the shorter route.
            let attached = result.map(|a| a.map(|a| a.node));
            for e in &self.edges {
                if n == 3 {
                    break;
                }
                // No mid-arc start on a one-way, jump or jet-jump arc (the game's attach skips them).
                if e.movement != 0
                    || graph.arcs[e.arc].one_way()
                    || !attached.contains(&Some(e.from))
                    || attached.contains(&Some(e.to))
                    || result[..n].iter().flatten().any(|a| a.node == e.to)
                    || !group_ok(self.group[e.to])
                    || !graph.arc_traversable(e.arc, self.nodes[e.from].hub, mask, false)
                {
                    continue;
                }
                let mut line = Waypoints::NONE;
                let fits = line.push(self.nodes[e.from].point)
                    && match (e.extra, e.bend) {
                        (Some(j), _) => self.passages[j as usize].points().iter().all(|&p| line.push(p)),
                        (None, Some(b)) => line.push(b),
                        (None, None) => true,
                    }
                    && line.push(self.nodes[e.to].point);
                if !fits {
                    continue;
                }
                let Some((seg, mid)) = corridor_onward(line.points(), point) else { continue };
                let Some(mid) = probe.foot(mid) else { continue };
                if d2(point, mid) > CORRIDOR_REACH * CORRIDOR_REACH || (point.y - mid.y).abs() > 1.2 {
                    continue;
                }
                let pts = line.points();
                let mut via = Waypoints::NONE;
                let ok = (d2(mid, pts[seg + 1]) < 0.04 || via.push(mid))
                    && pts[seg + 1..pts.len() - 1].iter().all(|&p| via.push(p));
                if !ok {
                    continue;
                }
                let Ok(attachment) = test(e.to, via, barriers, probe) else {
                    return result;
                };
                if let Some(a) = attachment {
                    result[n] = Some(a);
                    n += 1;
                }
            }
        }
        if n == 0 {
            if let Some((node, bend)) = slot.and_then(|s| self.approaches[s]) {
                if group_ok(self.group[node]) && nodes.iter().any(|&(_, i)| i == node) {
                    let Ok(attachment) = test(node, bend, barriers, probe) else {
                        return result;
                    };
                    if let Some(a) = attachment {
                        result[n] = Some(a);
                        n += 1;
                    }
                }
            }
        }
        let mut segments = [(f64::INFINITY, NONE, Vec3::default(), Waypoints::NONE, 1.2f64); 12];
        if n == 0 {
            // Join an ordinary directed corridor at its floor-snapped projection.
            // Bent corridors and multi-bend passages accept projections on any
            // segment, routing through the remaining certified waypoints.
            let mut push_seg =
                |node: usize, a: Vec3, b: Vec3, clamp_ends: bool, tail: Waypoints| {
                    let (dx, dz) = (b.x - a.x, b.z - a.z);
                    let span = dx * dx + dz * dz;
                    if span < 0.01 {
                        return;
                    }
                    let raw_t = ((point.x - a.x) * dx + (point.z - a.z) * dz) / span;
                    let t = if clamp_ends {
                        v8math::min(v8math::max(raw_t, 0.0), 1.0)
                    } else {
                        if !(0.0..=1.0).contains(&raw_t) {
                            return;
                        }
                        raw_t
                    };
                    let mid = Vec3::new(a.x + t * dx, a.y + t * (b.y - a.y), a.z + t * dz);
                    let d = h2(point.x, point.z, mid.x, mid.z);
                    let max_dy = if clamp_ends && (a.y - b.y).abs() > 1.0 {
                        2.2
                    } else {
                        1.2
                    };
                    if d > CORRIDOR_REACH * CORRIDOR_REACH
                        || d >= segments[segments.len() - 1].0
                        || (point.y - mid.y).abs() > max_dy
                    {
                        return;
                    }
                    let mut entry = (d, node, mid, tail, max_dy);
                    for slot in &mut segments {
                        if entry.0 < slot.0 {
                            std::mem::swap(slot, &mut entry);
                        }
                    }
                };
            for e in &self.edges {
                if e.movement != 0
                    || !graph.arc_traversable(e.arc, self.nodes[e.from].hub, mask, false)
                {
                    continue;
                }
                let (from_pt, to_pt) = (self.nodes[e.from].point, self.nodes[e.to].point);
                if let Some(j) = e.extra {
                    let pts = self.passages[j as usize].points();
                    let rev_ok = !graph.arcs[e.arc].one_way();
                    let forward_node = if enter { e.to } else { e.from };
                    let reverse_node = if enter { e.from } else { e.to };
                    let use_fwd = group_ok(self.group[forward_node]);
                    let use_rev = rev_ok && group_ok(self.group[reverse_node]);
                    if !use_fwd && !use_rev {
                        continue;
                    }
                    for seg_idx in 0..=pts.len() {
                        let a = if seg_idx == 0 {
                            from_pt
                        } else {
                            pts[seg_idx - 1]
                        };
                        let b = if seg_idx == pts.len() {
                            to_pt
                        } else {
                            pts[seg_idx]
                        };
                        if use_fwd {
                            let mut tail = Waypoints::NONE;
                            let ok = if enter {
                                pts[seg_idx..].iter().all(|&p| tail.push(p))
                            } else {
                                pts[..seg_idx].iter().all(|&p| tail.push(p))
                            };
                            if ok {
                                push_seg(forward_node, a, b, true, tail);
                            }
                        }
                        if use_rev {
                            let mut tail = Waypoints::NONE;
                            let ok = if enter {
                                pts[..seg_idx].iter().rev().all(|&p| tail.push(p))
                            } else {
                                pts[seg_idx..].iter().rev().all(|&p| tail.push(p))
                            };
                            if ok {
                                push_seg(reverse_node, a, b, true, tail);
                            }
                        }
                    }
                } else {
                    let node = if enter { e.to } else { e.from };
                    if !group_ok(self.group[node]) {
                        continue;
                    }
                    if let Some(bend) = e.bend {
                        if enter {
                            push_seg(node, bend, to_pt, true, Waypoints::NONE);
                            push_seg(node, from_pt, bend, true, Waypoints::one(Some(bend)));
                        } else {
                            push_seg(node, from_pt, bend, true, Waypoints::NONE);
                            push_seg(node, bend, to_pt, true, Waypoints::one(Some(bend)));
                        }
                    } else {
                        push_seg(node, from_pt, to_pt, false, Waypoints::NONE);
                    }
                }
            }
            for (_, node, raw_mid, tail, max_dy) in
                segments.into_iter().take_while(|(_, node, ..)| *node != NONE)
            {
                let Some(mid) = probe.foot(raw_mid) else {
                    continue;
                };
                if d2(point, mid) > CORRIDOR_REACH * CORRIDOR_REACH
                    || (point.y - mid.y).abs() > max_dy
                {
                    continue;
                }
                let node_pt = self.nodes[node].point;
                let mut via = Waypoints::NONE;
                let ok = if enter {
                    let skip_mid = d2(mid, node_pt) < 0.04
                        || tail.points().first().is_some_and(|&p| d2(p, mid) < 0.04);
                    (skip_mid || via.push(mid)) && tail.points().iter().all(|&p| via.push(p))
                } else {
                    let skip_mid = d2(mid, node_pt) < 0.04
                        || tail.points().last().is_some_and(|&p| d2(p, mid) < 0.04);
                    tail.points().iter().all(|&p| via.push(p)) && (skip_mid || via.push(mid))
                };
                if !ok {
                    continue;
                }
                let Ok(attachment) = test(node, via, barriers, probe) else {
                    return result;
                };
                if let Some(a) = attachment {
                    if let Some(slot) = slot {
                        self.approaches[slot] = Some((node, a.bend));
                    }
                    result[0] = Some(a);
                    n = 1;
                    break;
                }
            }
        }
        if n == 0 {
            // When barriers block direct attachment (for example a room ringed
            // with chairs and a central pillar), try a 1- or 2-corner detour
            // around the blocking barriers before broad physical sweeps. Prefer
            // nodes in the home hub first so behind-wall hubs do not spend drives.
            let mut ordered = [NONE; 4];
            let mut ord_len = 0usize;
            for &home_only in &[true, false] {
                for &(_, node) in &nodes {
                    if node == NONE || ord_len == ordered.len() {
                        break;
                    }
                    if (self.nodes[node].hub == home.index) != home_only
                        || ordered[..ord_len].contains(&node)
                    {
                        continue;
                    }
                    ordered[ord_len] = node;
                    ord_len += 1;
                }
            }
            for &node in &ordered[..ord_len] {
                if !permitted(node) {
                    continue;
                }
                let p = self.nodes[node].point;
                let (a, b) = if enter { (point, p) } else { (p, point) };
                let Some(detour) = barrier_detour(barriers, a, b, radius, mask) else {
                    continue;
                };
                let mut snapped = Waypoints::NONE;
                let mut floor_ok = true;
                for &c in detour.points() {
                    let Some(foot) = probe.foot(c) else {
                        floor_ok = false;
                        break;
                    };
                    if !snapped.push(foot) {
                        floor_ok = false;
                        break;
                    }
                }
                if !floor_ok {
                    continue;
                }
                let Ok(attachment) = test(node, snapped, barriers, probe) else {
                    return result;
                };
                if let Some(att) = attachment {
                    if let Some(slot) = slot {
                        self.approaches[slot] = Some((node, att.bend));
                    }
                    result[0] = Some(att);
                    return result;
                }
            }
            for (_, node, raw_mid, tail, max_dy) in segments
                .into_iter()
                .take(4)
                .take_while(|(_, node, ..)| *node != NONE)
            {
                if !permitted(node) {
                    continue;
                }
                let Some(mid) = probe.foot(raw_mid) else {
                    continue;
                };
                if d2(point, mid) > CORRIDOR_REACH * CORRIDOR_REACH
                    || (point.y - mid.y).abs() > max_dy
                    || !barriers.point_clear(mid, radius, mask).0
                {
                    continue;
                }
                let (a, b) = if enter { (point, mid) } else { (mid, point) };
                let Some(detour) = barrier_detour(barriers, a, b, radius, mask) else {
                    continue;
                };
                let node_pt = self.nodes[node].point;
                let mut via = Waypoints::NONE;
                let ok = if enter {
                    let mut ok = true;
                    for &c in detour.points() {
                        match probe.foot(c) {
                            Some(foot) if via.push(foot) => {}
                            _ => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    let skip_mid = d2(mid, node_pt) < 0.04
                        || tail.points().first().is_some_and(|&p| d2(p, mid) < 0.04);
                    ok && (skip_mid || via.push(mid)) && tail.points().iter().all(|&p| via.push(p))
                } else {
                    let skip_mid = d2(mid, node_pt) < 0.04
                        || tail.points().last().is_some_and(|&p| d2(p, mid) < 0.04);
                    let mut ok = tail.points().iter().all(|&p| via.push(p))
                        && (skip_mid || via.push(mid));
                    if ok {
                        for &c in detour.points() {
                            match probe.foot(c) {
                                Some(foot) if via.push(foot) => {}
                                _ => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                    }
                    ok
                };
                if !ok {
                    continue;
                }
                let Ok(attachment) = test(node, via, barriers, probe) else {
                    return result;
                };
                if let Some(att) = attachment {
                    if let Some(slot) = slot {
                        self.approaches[slot] = Some((node, att.bend));
                    }
                    result[0] = Some(att);
                    return result;
                }
            }
        }
        if n == 0 {
            for (_, node) in nodes.into_iter().take(3).take_while(|(_, n)| *n != NONE) {
                let p = self.nodes[node].point;
                let (a, b) = if enter { (point, p) } else { (p, point) };
                let wide_room = graph.hubs[self.nodes[node].hub].radius >= 10.;
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let norm = v8math::max(v8math::hypot(dx, dz), 0.001);
                let wide_offsets: &[f64] = if wide_room {
                    &[5., -5., 8., -8., 11.5, -11.5, 2.8, -2.8, 1.2, -1.2]
                } else {
                    &[1.2, -1.2, 2.8, -2.8, 5., -5., 8., -8.]
                };
                for fraction in [0.5, 0.25, 0.75] {
                    for &offset in wide_offsets {
                        let cand = Vec3::new(
                            a.x + fraction * dx - dz / norm * offset,
                            a.y + fraction * (b.y - a.y),
                            a.z + fraction * dz + dx / norm * offset,
                        );
                        let Some(mid) = probe.foot(cand) else {
                            continue;
                        };
                        let Ok(attachment) = test(node, Waypoints::one(Some(mid)), barriers, probe) else {
                            return result;
                        };
                        if let Some(att) = attachment {
                            if let Some(slot) = slot {
                                self.approaches[slot] = Some((node, att.bend));
                            }
                            result[0] = Some(att);
                            return result;
                        }
                    }
                }
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn point_trip(
        &mut self,
        graph: &Swbf2Graph,
        barriers: &mut BarrierSet,
        from: Vec3,
        to: Vec3,
        radius: f64,
        mask: u32,
        jet: bool,
        work: &mut usize,
        initial: &mut Option<Attachments>,
        nearby: bool,
        target_slack: f64,
        probe: &mut impl PointProbe,
    ) -> Option<Trip> {
        if graph.arcs.len() > 4096 || self.edges.len() > 8192 {
            return None;
        }
        let starts = if let Some(known) = *initial {
            known
        } else {
            if !probe.walk(from, from, 0)? {
                return None;
            }
            let end_groups = (self.nodes.len() <= 32 * 64)
                .then(|| self.groups_near(graph, to, target_slack, mask));
            let found = self.point_attachments(
                graph,
                barriers,
                from,
                radius,
                mask,
                true,
                nearby,
                end_groups.as_ref(),
                probe,
            );
            *initial = Some(found);
            found
        };
        if starts.iter().all(Option::is_none)
            || !barriers.point_clear(to, radius, mask).0
            || !probe.walk(to, to, 0)?
        {
            return None;
        }
        let mut start_groups = [0u64; 32];
        for a in starts.into_iter().flatten() {
            let group = self.group[a.node];
            start_groups[group as usize / 64] |= 1 << (group % 64);
        }
        let ends = self.point_attachments(
            graph,
            barriers,
            to,
            radius,
            mask,
            false,
            nearby,
            (self.nodes.len() <= 32 * 64).then_some(&start_groups),
            probe,
        );
        let partial = self.clear_edges.contains(&None).then_some(&starts);
        let mut best: Option<Trip> = None;
        let mut walk_fn = |a, b, m| probe.walk(a, b, m);
        for end in ends.into_iter().flatten() {
            let key = Key {
                goal: end.node,
                mask,
                jet,
                radius: corridor_radius(radius).to_bits(),
            };
            let Some(idx) = self.row_index(graph, barriers, key, &mut walk_fn, work, partial) else {
                break;
            };
            for start in starts.into_iter().flatten() {
                let row = &self.rows[idx];
                let d = row.distance[start.node] + start.length + end.length;
                if !d.is_finite() || best.is_some_and(|t| t.result.distance <= d) {
                    continue;
                }
                let mut direct = start.node == end.node;
                let mut direct_arc = None;
                if row.hops[start.node] == 1 {
                    let edge = self.edges[row.next[start.node]];
                    direct_arc = Some(edge.arc);
                    direct = edge.bend.is_none()
                        && edge.movement == 0
                        && along_corridor(
                            self.nodes[start.node].point,
                            self.nodes[end.node].point,
                            from,
                            to,
                        );
                }
                if direct
                    && clear(barriers, from, to, radius, mask)
                    && walk_fn(from, to, 0) == Some(true)
                {
                    return Some(Trip {
                        start,
                        end,
                        direct: true,
                        direct_arc,
                        first_edge: None,
                        result: RouteQuery::found(length(from, to), row.hops[start.node].into()),
                    });
                }
                best = Some(Trip {
                    start,
                    end,
                    direct: false,
                    direct_arc: None,
                    first_edge: (start.node != end.node).then(|| row.next[start.node]),
                    result: RouteQuery::found(d, row.hops[start.node].into()),
                });
            }
        }
        best
    }

    /// A surveyed floor inside the objective can itself be an approach.
    pub(super) fn objective_floor(&self, center: Vec3, reach: f64) -> Option<Vec3> {
        self.nodes
            .iter()
            .map(|n| n.point)
            .filter(|&p| d2(center, p) <= reach * reach)
            .min_by(|&a, &b| d2(center, a).total_cmp(&d2(center, b)))
    }

    pub(super) fn point(&self, node: usize) -> Vec3 {
        self.nodes[node].point
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn next(
        &mut self,
        graph: &Swbf2Graph,
        barriers: &mut BarrierSet,
        here: usize,
        goal: usize,
        radius: f64,
        mask: u32,
        jet: bool,
        budget: &mut SearchBudget,
        walk: &mut impl FnMut(Vec3, Vec3, u8) -> Option<bool>,
    ) -> Option<(usize, Bends, u8, usize, u8)> {
        let key = Key {
            goal,
            mask,
            jet,
            radius: corridor_radius(radius).to_bits(),
        };
        let (drives, waiting) = (&mut budget.drives, &mut budget.waiting);
        let mut check = |a, b, m| spend_check(drives, waiting, || walk(a, b, m));
        let row = self.row_index(graph, barriers, key, &mut check, &mut budget.entries, None)?;
        self.corridor(here, self.rows[row].next[here])
    }

    pub(super) fn corridor(&self, here: usize, index: usize) -> Option<(usize, Bends, u8, usize, u8)> {
        let edge = self.edges.get(index).filter(|e| e.from == here)?;
        let via = edge
            .extra
            .map_or_else(|| Bends::one(edge.bend), |i| self.passages[i as usize]);
        Some((edge.to, via, edge.movement, edge.arc, edge.after_jump))
    }
}
