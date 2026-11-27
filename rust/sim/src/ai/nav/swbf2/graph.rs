//! 2005 planning graph, `plnp` dynamic arc-group XOR deltas, validation,
//! pre-allocated deterministic routing, and O(1) cached reachability/distance
//! queries.
//!
//! All arithmetic uses `v8math` and deterministic index tie-breaking so x86-64
//! and ARM64 produce identical results, and every steady-state query reuses
//! load-time scratch buffers with zero heap allocations.

use super::super::math::{f32, h2, lowest_bit, Vec3};
use crate::ai::random::Random;
use bfe_content::zero_hash;
use flate2::{Decompress, FlushDecompress, Status};

pub const UNUSED_SLOT: u8 = 255;
pub const TABLE_COUNT: usize = 5;
pub const NO_TABLE: i32 = -1;

/// Arc type flags in 2005 planning data.
pub const ARC_ONE_WAY: u8 = 1;
pub const ARC_JUMP: u8 = 2;
pub const ARC_JET_JUMP: u8 = 4;

/// Aggregate storage for verified dynamic routing deltas.
pub(super) const MAX_DELTA_BYTES: usize = 32 * 1024 * 1024;

/// Small additive penalty (metres) for jump and jet-jump arcs in fallback
/// shortest-path routing so equivalent flat ground routes are preferred.
const JUMP_ARC_PENALTY: f64 = 2.5;
const JET_JUMP_ARC_PENALTY: f64 = 4.0;

/// Maximum candidate hubs tested against world collision during nearest-hub
/// attachment so collision work remains strictly bounded.
const MAX_WALK_CHECK_CANDIDATES: usize = 12;

/// Vertical difference above which a hub's Y coordinate represents a map-wide
/// planning plane (e.g. capital-ship interiors) rather than local floor height,
/// so horizontal `line_of_walk` attachment rays run at the query point's deck Y.
const HUB_Y_REPROJECT_THRESHOLD: f64 = 6.0;

pub use super::super::route::RouteQuery;

/// One hub in a 2005 planning graph.
#[derive(Clone, Debug)]
pub struct Swbf2Hub {
    pub index: usize,
    pub name: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub radius: f64,
    pub slots: [u8; 8],
    pub mask: u32,
    pub weight_counts: [u8; TABLE_COUNT],
    pub valid: bool,
}

impl Swbf2Hub {
    pub fn pos(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

/// One arc in a 2005 planning graph.
#[derive(Clone, Debug)]
pub struct Swbf2Arc {
    pub index: usize,
    pub name: Option<String>,
    pub name_hash: u32,
    pub start: usize,
    pub end: usize,
    pub filter_flags: u32,
    pub type_flags: u8,
    pub length: f64,
    pub degenerate: bool,
    pub blocked_by_group: bool,
    pub blocked_manual: bool,
}

impl Swbf2Arc {
    pub fn one_way(&self) -> bool {
        (self.type_flags & ARC_ONE_WAY) != 0
    }

    pub fn jump(&self) -> bool {
        (self.type_flags & ARC_JUMP) != 0
    }

    pub fn jet_jump(&self) -> bool {
        (self.type_flags & ARC_JET_JUMP) != 0
    }

    pub fn is_blocked(&self) -> bool {
        self.blocked_by_group || self.blocked_manual
    }

    pub fn other_end(&self, hub: usize) -> usize {
        if hub == self.start {
            self.end
        } else {
            self.start
        }
    }
}

/// One dynamic blocking group declared by `planningGroups`.
#[derive(Clone, Debug)]
pub struct PlanningGroup {
    pub index: usize,
    pub arc_hashes: Vec<u32>,
    pub arc_indices: Vec<usize>,
}

/// Compressed XOR-delta block metadata from `planningGroups.blocks`.
#[derive(Clone, Copy, Debug)]
pub struct DeltaBlockMeta {
    pub offset: usize,
    pub size: usize,
    pub unpacked_size: usize,
}

/// Non-fatal validation findings collected at load time.
#[derive(Clone, Debug, Default)]
pub struct ValidationReport {
    pub dangling_arcs: Vec<usize>,
    pub bad_hub_slots: Vec<(usize, usize)>,
    pub non_finite_hubs: Vec<usize>,
    pub zero_length_arcs: Vec<usize>,
    pub weight_mismatches: Vec<usize>,
    pub bad_delta_blocks: Vec<usize>,
    /// Weakly-connected components (sorted hub indices) for each of the 5
    /// standard ground/vehicle traversal tables (bits 0..4).
    pub components_by_table: [Vec<Vec<usize>>; TABLE_COUNT],
    /// Number of hubs with a matching mask that lie outside the largest
    /// component for each traversal table.
    pub isolated_hubs_by_table: [usize; TABLE_COUNT],
}

/// Precomputed all-pairs shortest-path table for one traversal class, giving
/// O(1), zero-allocation reachability and distance lookups.
#[derive(Clone, Debug)]
struct ApspTable {
    dist: Vec<f32>,
    hops: Vec<u16>,
}

/// Pre-allocated scratch buffers so routing and nearest-hub attachment perform
/// zero heap allocations during steady-state simulation ticks.
#[derive(Clone, Debug, Default)]
struct RouterScratch {
    dist: Vec<f64>,
    hops: Vec<u16>,
    prev_hub: Vec<u32>,
    prev_arc: Vec<u32>,
    seen: Vec<u32>,
    stamp: u32,
    heap_cost: Vec<f64>,
    heap_hub: Vec<u32>,
    route_hubs: Vec<u32>,
    route_arcs: Vec<u32>,
    /// `(gap_or_radius, center_sq, hub_idx)` candidates for nearest-hub selection.
    candidates: Vec<(f64, f64, usize)>,
}

impl RouterScratch {
    fn new(hub_count: usize, arc_count: usize) -> Self {
        // A settled vertex relaxes each directed incidence at most once.
        let heap_cap = 2 * arc_count + 1;
        Self {
            dist: vec![0.0; hub_count],
            hops: vec![0; hub_count],
            prev_hub: vec![u32::MAX; hub_count],
            prev_arc: vec![u32::MAX; hub_count],
            seen: vec![0; hub_count],
            stamp: 0,
            heap_cost: Vec::with_capacity(heap_cap),
            heap_hub: Vec::with_capacity(heap_cap),
            route_hubs: Vec::with_capacity(hub_count),
            route_arcs: Vec::with_capacity(hub_count),
            candidates: Vec::with_capacity(hub_count),
        }
    }

    fn next_stamp(&mut self) -> u32 {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.seen.fill(0);
            self.stamp = 1;
        }
        self.stamp
    }

    fn heap_clear(&mut self) {
        self.heap_cost.clear();
        self.heap_hub.clear();
    }

    fn heap_push(&mut self, cost: f64, hub: u32) {
        debug_assert!(self.heap_cost.len() < self.heap_cost.capacity());
        let mut i = self.heap_cost.len();
        self.heap_cost.push(cost);
        self.heap_hub.push(hub);
        while i > 0 {
            let p = (i - 1) >> 1;
            if heap_less(self.heap_cost[i], self.heap_hub[i], self.heap_cost[p], self.heap_hub[p]) {
                self.heap_cost.swap(i, p);
                self.heap_hub.swap(i, p);
                i = p;
            } else {
                break;
            }
        }
    }

    fn heap_pop(&mut self) -> Option<(f64, u32)> {
        if self.heap_cost.is_empty() {
            return None;
        }
        let top = (self.heap_cost[0], self.heap_hub[0]);
        let last_cost = self.heap_cost.pop().expect("non-empty");
        let last_hub = self.heap_hub.pop().expect("non-empty");
        if !self.heap_cost.is_empty() {
            self.heap_cost[0] = last_cost;
            self.heap_hub[0] = last_hub;
            let len = self.heap_cost.len();
            let mut i = 0usize;
            loop {
                let l = i * 2 + 1;
                if l >= len {
                    break;
                }
                let mut best = l;
                let r = l + 1;
                if r < len
                    && heap_less(self.heap_cost[r], self.heap_hub[r], self.heap_cost[l], self.heap_hub[l])
                {
                    best = r;
                }
                if heap_less(self.heap_cost[best], self.heap_hub[best], self.heap_cost[i], self.heap_hub[i]) {
                    self.heap_cost.swap(i, best);
                    self.heap_hub.swap(i, best);
                    i = best;
                } else {
                    break;
                }
            }
        }
        Some(top)
    }
}

#[inline]
fn heap_less(cost_a: f64, hub_a: u32, cost_b: f64, hub_b: u32) -> bool {
    if cost_a != cost_b {
        cost_a < cost_b
    } else {
        hub_a < hub_b
    }
}

/// Raw hub input passed to `Swbf2Graph::new`.
#[derive(Clone, Debug)]
pub struct Swbf2HubInput {
    pub name: Option<String>,
    pub position: [f64; 3],
    pub radius: f64,
    pub slots: Vec<f64>,
    pub weight_counts: [u8; TABLE_COUNT],
    pub weights: Vec<u8>,
}

/// Raw arc input passed to `Swbf2Graph::new`.
#[derive(Clone, Debug)]
pub struct Swbf2ArcInput {
    pub name: Option<String>,
    pub start: f64,
    pub end: f64,
    pub filter_flags: u32,
    pub type_flags: u8,
}

/// Raw dynamic group input passed to `Swbf2Graph::new`.
#[derive(Clone, Debug, Default)]
pub struct PlanningGroupsInput {
    pub group_counts: Vec<usize>,
    pub connection_hashes: Vec<u32>,
    pub blocks: Vec<DeltaBlockMeta>,
    pub raw_bin: Vec<u8>,
}

/// 2005 navigation planning graph with dynamic `plnp` delta support,
/// pre-allocated deterministic routing, and O(1) cached all-pairs reachability.
#[derive(Clone, Debug)]
pub struct Swbf2Graph {
    pub branch_count: usize,
    pub hubs: Vec<Swbf2Hub>,
    pub arcs: Vec<Swbf2Arc>,
    /// Packed routing words `(from * TABLE_COUNT + table) * hub_count + dest`.
    /// Each byte `k in 0..4` packs `slot = b & 7`, `weight = (b >> 3) & 31`.
    pub words: Vec<u32>,
    pub table_has_weights: [bool; TABLE_COUNT],
    /// CSR adjacency: outgoing traversable candidate arcs per hub (in arc-index order).
    adj_offsets: Vec<usize>,
    adj_arcs: Vec<usize>,
    pub groups: Vec<PlanningGroup>,
    pub group_state: u32,
    deltas: Vec<Option<Vec<u8>>>,
    manual_block_count: usize,
    has_jet_arcs: bool,
    apsp_ground: [Option<ApspTable>; TABLE_COUNT],
    apsp_jet: [Option<ApspTable>; TABLE_COUNT],
    pub validation: ValidationReport,
    scratch: RouterScratch,
}

impl Swbf2Graph {
    pub fn delta_block_count(&self) -> usize {
        self.deltas.len()
    }

    /// Build and validate a 2005 planning graph without panicking on dangling
    /// arcs, non-finite numbers, weight mismatches, or disconnected islands.
    pub(crate) fn new(
        branch_count: usize,
        hub_inputs: &[Swbf2HubInput],
        arc_inputs: &[Swbf2ArcInput],
        groups_input: Option<PlanningGroupsInput>,
    ) -> Self {
        let n = hub_inputs.len();
        let tables = branch_count.min(TABLE_COUNT);
        let mut validation = ValidationReport::default();

        // 1. Validate hubs.
        let mut hubs = Vec::with_capacity(n);
        for (index, h) in hub_inputs.iter().enumerate() {
            let finite = h.position[0].is_finite()
                && h.position[1].is_finite()
                && h.position[2].is_finite()
                && h.radius.is_finite()
                && h.radius >= 0.0;
            if !finite {
                validation.non_finite_hubs.push(index);
            }
            let (x, y, z, radius) = if finite {
                (f32(h.position[0]), f32(h.position[1]), f32(h.position[2]), f32(h.radius))
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
            let mut slots = [UNUSED_SLOT; 8];
            for (slot_idx, slot) in slots.iter_mut().enumerate() {
                let raw = h.slots.get(slot_idx).copied().unwrap_or(f64::from(UNUSED_SLOT));
                if !raw.is_finite() || raw < 0.0 || raw.fract() != 0.0 {
                    if raw != f64::from(UNUSED_SLOT) {
                        validation.bad_hub_slots.push((index, slot_idx));
                    }
                    *slot = UNUSED_SLOT;
                    continue;
                }
                let idx = raw as usize;
                if idx == usize::from(UNUSED_SLOT) {
                    *slot = UNUSED_SLOT;
                } else if idx < arc_inputs.len() && idx < usize::from(UNUSED_SLOT) {
                    *slot = idx as u8;
                } else {
                    validation.bad_hub_slots.push((index, slot_idx));
                    *slot = UNUSED_SLOT;
                }
            }
            hubs.push(Swbf2Hub {
                index,
                name: h.name.clone(),
                x,
                y,
                z,
                radius,
                slots,
                mask: 0,
                weight_counts: h.weight_counts,
                valid: finite,
            });
        }

        // 2. Validate arcs and drop/flag dangling references cleanly.
        let mut arcs = Vec::with_capacity(arc_inputs.len());
        let mut has_jet_arcs = false;
        for (index, a) in arc_inputs.iter().enumerate() {
            let start_ok = a.start.is_finite()
                && a.start >= 0.0
                && a.start.fract() == 0.0
                && (a.start as usize) < n;
            let end_ok = a.end.is_finite()
                && a.end >= 0.0
                && a.end.fract() == 0.0
                && (a.end as usize) < n;
            let name_hash = a.name.as_deref().map_or(0, zero_hash::fnv1a);
            if !start_ok || !end_ok {
                validation.dangling_arcs.push(index);
                arcs.push(Swbf2Arc {
                    index,
                    name: a.name.clone(),
                    name_hash,
                    start: 0,
                    end: 0,
                    filter_flags: 0,
                    type_flags: a.type_flags,
                    length: 0.0,
                    degenerate: true,
                    blocked_by_group: false,
                    blocked_manual: false,
                });
                continue;
            }
            let start = a.start as usize;
            let end = a.end as usize;
            let ha = &hubs[start];
            let hb = &hubs[end];
            let dx = hb.x - ha.x;
            let dy = hb.y - ha.y;
            let dz = hb.z - ha.z;
            let dist_xz_sq = dx * dx + dz * dz;
            let dist_3d_sq = dist_xz_sq + dy * dy;
            let degenerate = !ha.valid || !hb.valid || start == end || dist_3d_sq == 0.0;
            if degenerate {
                validation.zero_length_arcs.push(index);
            } else if (a.type_flags & ARC_JET_JUMP) != 0 {
                has_jet_arcs = true;
            }
            let length = if dist_xz_sq > 0.0 {
                v8math::sqrt(dist_xz_sq)
            } else {
                v8math::sqrt(dist_3d_sq)
            };
            arcs.push(Swbf2Arc {
                index,
                name: a.name.clone(),
                name_hash,
                start,
                end,
                filter_flags: a.filter_flags,
                type_flags: a.type_flags,
                length,
                degenerate,
                blocked_by_group: false,
                blocked_manual: false,
            });
        }

        // Clear hub slots that ended up pointing at dangling or non-incident arcs.
        for hub in &mut hubs {
            for (slot_idx, slot) in hub.slots.iter_mut().enumerate() {
                if *slot == UNUSED_SLOT {
                    continue;
                }
                let arc_idx = usize::from(*slot);
                let touches = arcs.get(arc_idx).is_some_and(|arc| {
                    !arc.degenerate && (arc.start == hub.index || arc.end == hub.index)
                });
                if !touches {
                    if !arcs.get(arc_idx).is_some_and(|a| a.degenerate) {
                        validation.bad_hub_slots.push((hub.index, slot_idx));
                    }
                    *slot = UNUSED_SLOT;
                }
            }
        }

        // 3. Build CSR adjacency list and hub capability masks.
        let mut counts = vec![0usize; n];
        for arc in &arcs {
            if arc.degenerate {
                continue;
            }
            counts[arc.start] += 1;
            hubs[arc.start].mask |= arc.filter_flags;
            hubs[arc.end].mask |= arc.filter_flags;
            if !arc.one_way() {
                counts[arc.end] += 1;
            }
        }
        let mut adj_offsets = vec![0usize; n + 1];
        for i in 0..n {
            adj_offsets[i + 1] = adj_offsets[i] + counts[i];
        }
        let mut adj_arcs = vec![0usize; adj_offsets[n]];
        let mut cursor = adj_offsets[..n].to_vec();
        for arc in &arcs {
            if arc.degenerate {
                continue;
            }
            let s = arc.start;
            adj_arcs[cursor[s]] = arc.index;
            cursor[s] += 1;
            if !arc.one_way() {
                let e = arc.end;
                adj_arcs[cursor[e]] = arc.index;
                cursor[e] += 1;
            }
        }

        // 4. Unpack 2005 row-major hub weight tables into flat u32 routing words.
        let mut words = vec![0u32; n.saturating_mul(TABLE_COUNT).saturating_mul(n)];
        let mut table_has_weights = [false; TABLE_COUNT];
        for (h_idx, h_in) in hub_inputs.iter().enumerate() {
            let expected_rows: usize = h_in.weight_counts[..tables]
                .iter()
                .map(|&c| usize::from(c))
                .sum();
            let expected_bytes = expected_rows.saturating_mul(n);
            if expected_bytes == 0 {
                continue;
            }
            if h_in.weights.len() != expected_bytes {
                validation.weight_mismatches.push(h_idx);
                continue;
            }
            let mut offset = 0usize;
            for (t, &row_count) in h_in.weight_counts.iter().enumerate().take(tables) {
                let rows = usize::from(row_count);
                if rows == 0 {
                    continue;
                }
                let word_base = (h_idx * TABLE_COUNT + t) * n;
                for row in 0..rows {
                    let row_slice = &h_in.weights[offset..offset + n];
                    offset += n;
                    if row < 4 {
                        let shift = (row as u32) * 8;
                        for (dest, &byte) in row_slice.iter().enumerate() {
                            if byte != 0 {
                                words[word_base + dest] |= u32::from(byte) << shift;
                                table_has_weights[t] = true;
                            }
                        }
                    }
                }
            }
        }

        // 5. Parse dynamic `plnp` groups and pre-validate/pre-decode delta blocks.
        let mut groups = Vec::new();
        let mut deltas = Vec::new();
        if let Some(pg) = groups_input {
            let mut hash_cursor = 0usize;
            for (g_idx, &count) in pg.group_counts.iter().enumerate() {
                let end = hash_cursor.saturating_add(count).min(pg.connection_hashes.len());
                let arc_hashes = pg.connection_hashes[hash_cursor..end].to_vec();
                hash_cursor = end;
                let mut arc_indices = Vec::new();
                for &hash in &arc_hashes {
                    for arc in &arcs {
                        if arc.name_hash == hash && !arc_indices.contains(&arc.index) {
                            arc_indices.push(arc.index);
                        }
                    }
                }
                groups.push(PlanningGroup { index: g_idx, arc_hashes, arc_indices });
            }
            let mut remaining = MAX_DELTA_BYTES;
            for (index, meta) in pg.blocks.iter().enumerate() {
                let data = meta.offset.checked_add(meta.size)
                    .and_then(|end| pg.raw_bin.get(meta.offset..end))
                    .filter(|_| meta.unpacked_size <= remaining)
                    .and_then(|bytes| decode_delta(bytes, meta.unpacked_size, n, tables));
                if let Some(bytes) = &data { remaining -= bytes.len(); }
                else { validation.bad_delta_blocks.push(index); }
                deltas.push(data);
            }
        }

        // 6. Compute weakly-connected components per traversal class for diagnostics.
        for t in 0..tables {
            let mask = 1u32 << t;
            let comps = compute_weak_components(&hubs, &arcs, mask);
            let largest = comps.iter().map(Vec::len).max().unwrap_or(0);
            let total: usize = comps.iter().map(Vec::len).sum();
            validation.isolated_hubs_by_table[t] = total.saturating_sub(largest);
            validation.components_by_table[t] = comps;
        }

        let mut graph = Swbf2Graph {
            branch_count: tables,
            hubs,
            arcs,
            words,
            table_has_weights,
            adj_offsets,
            adj_arcs,
            groups,
            group_state: 0,
            deltas,
            manual_block_count: 0,
            has_jet_arcs,
            apsp_ground: std::array::from_fn(|_| None),
            apsp_jet: std::array::from_fn(|_| None),
            validation,
            scratch: RouterScratch::new(n, arc_inputs.len()),
        };
        graph.rebuild_apsp();
        graph
    }

    /// Routing table index for `mask`, or `NO_TABLE` if unsupported (e.g. Flyer).
    pub fn table(&self, mask: u32) -> i32 {
        if !mask.is_power_of_two() {
            return NO_TABLE;
        }
        let t = lowest_bit(f64::from(mask)) as usize;
        if t < self.branch_count {
            t as i32
        } else {
            NO_TABLE
        }
    }

    /// Whether `point` lies strictly inside `hub`'s 2D XZ disc.
    ///
    /// 2005 planning graphs use 2D XZ hub containment so multi-elevation spaces
    /// (such as ship interiors with a common hub Y plane) still attach cleanly.
    pub fn contains_hub_2d(&self, hub_idx: usize, point: Vec3) -> bool {
        let Some(hub) = self.hubs.get(hub_idx) else {
            return false;
        };
        if !hub.valid || hub.radius <= 0.0 {
            return false;
        }
        h2(hub.x, hub.z, point.x, point.z) < hub.radius * hub.radius
    }

    /// Whether `arc_idx` can be traversed starting from `from_hub` by a unit
    /// with `mask` and `can_jet`.
    pub fn arc_traversable(&self, arc_idx: usize, from_hub: usize, mask: u32, can_jet: bool) -> bool {
        let Some(arc) = self.arcs.get(arc_idx) else {
            return false;
        };
        if self.table(mask) == NO_TABLE || arc.degenerate || arc.is_blocked() || (arc.filter_flags & mask) == 0 {
            return false;
        }
        if arc.jet_jump() && !can_jet {
            return false;
        }
        if arc.start == from_hub {
            true
        } else {
            !arc.one_way() && arc.end == from_hub
        }
    }

    /// A local approach stays on its side of a restricted connection. Scope
    /// the separating plane to the corridor so a route can go around its end.
    pub(super) fn attachment_permitted(&self, home:usize, candidate:usize, point:Vec3, mask:u32)->bool {
        let end=self.hubs[candidate].pos();
        for arc in &self.arcs {
            if !arc.is_blocked() && !arc.one_way() && !arc.jump() && !arc.jet_jump()
                && arc.filter_flags & mask != 0 { continue; }
            if arc.degenerate { continue; }
            let a=&self.hubs[arc.start];let b=&self.hubs[arc.end];
            let dx=b.x-a.x; let dz=b.z-a.z; let square=dx*dx+dz*dz;
            if square<0.0001 { continue; }
            let mx=(a.x+b.x)*0.5;let mz=(a.z+b.z)*0.5;
            let u=(point.x-mx)*dx+(point.z-mz)*dz;
            let v=(end.x-mx)*dx+(end.z-mz)*dz;
            if (u<0.)==(v<0.) || u==v { continue; }
            let t=u/(u-v);
            let x=point.x+t*(end.x-point.x)-mx;
            let z=point.z+t*(end.z-point.z)-mz;
            let width=v8math::max(v8math::max(a.radius,b.radius),1.);
            if x*x+z*z<=width*width || (home==arc.start && candidate==arc.end)
                || (home==arc.end && candidate==arc.start) {return false;}
        }
        true
    }

    /// Find the best hub for `point` matching `mask`, checking `line_of_walk`
    /// on the closest candidates (preferring 2D-containing hubs with smallest
    /// radius, then closest collision-clear hub, with deterministic tie-breaking
    /// by hub index).
    pub fn nearest_hub<F>(
        &mut self,
        point: Vec3,
        mask: u32,
        exclude_hub: Option<usize>,
        line_of_walk: F,
    ) -> Option<usize>
    where
        F: FnMut(Vec3, Vec3) -> bool,
    {
        self.find_hub_inner(point, mask, exclude_hub, line_of_walk)
    }

    /// Strict collision-clear hub attachment used by reachability queries:
    /// returns `None` if `point` has no line-of-walk to any nearby hub (for
    /// example, a target sealed behind an unbroken wall).
    pub fn nearest_reachable_hub<F>(
        &mut self,
        point: Vec3,
        mask: u32,
        line_of_walk: F,
    ) -> Option<usize>
    where
        F: FnMut(Vec3, Vec3) -> bool,
    {
        self.find_hub_inner(point, mask, None, line_of_walk)
    }

    fn find_hub_inner<F>(
        &mut self,
        point: Vec3,
        mask: u32,
        exclude_hub: Option<usize>,
        mut line_of_walk: F,
    ) -> Option<usize>
    where
        F: FnMut(Vec3, Vec3) -> bool,
    {
        if self.hubs.is_empty() || mask == 0 {
            return None;
        }

        // Pass 1: 2D XZ hub containment, smallest radius first, deterministic index tie-break.
        self.scratch.candidates.clear();
        for hub in &self.hubs {
            if !hub.valid || (hub.mask & mask) == 0 || Some(hub.index) == exclude_hub {
                continue;
            }
            let d2_xz = h2(hub.x, hub.z, point.x, point.z);
            if d2_xz < hub.radius * hub.radius {
                self.scratch.candidates.push((hub.radius, d2_xz, hub.index));
            }
        }
        if !self.scratch.candidates.is_empty() {
            sort_candidates(&mut self.scratch.candidates);
            for i in 0..self.scratch.candidates.len().min(MAX_WALK_CHECK_CANDIDATES) {
                let idx = self.scratch.candidates[i].2;
                let mut hub_pos = self.hubs[idx].pos();
                if v8math::abs(hub_pos.y - point.y) > HUB_Y_REPROJECT_THRESHOLD {
                    hub_pos.y = point.y;
                }
                if line_of_walk(point, hub_pos) {
                    return Some(idx);
                }
            }
        }

        // Pass 2: Closest hubs outside containment, ranked by horizontal center
        // distance, then hub radius, then index (so large distant hubs do not
        // steal attachment from nearby corridor/doorway hubs).
        self.scratch.candidates.clear();
        for hub in &self.hubs {
            if !hub.valid || (hub.mask & mask) == 0 || Some(hub.index) == exclude_hub {
                continue;
            }
            let d2_xz = h2(hub.x, hub.z, point.x, point.z);
            self.scratch.candidates.push((d2_xz, hub.radius, hub.index));
        }
        if self.scratch.candidates.is_empty() {
            return None;
        }
        sort_candidates(&mut self.scratch.candidates);
        let min_d2 = self.scratch.candidates[0].0;
        let max_walk_d2 = v8math::max(25.0 * 25.0, min_d2 * 2.25);
        let check_limit = self.scratch.candidates.len().min(MAX_WALK_CHECK_CANDIDATES);
        for i in 0..check_limit {
            if self.scratch.candidates[i].0 > max_walk_d2 {
                break;
            }
            let idx = self.scratch.candidates[i].2;
            let mut hub_pos = self.hubs[idx].pos();
            if v8math::abs(hub_pos.y - point.y) > HUB_Y_REPROJECT_THRESHOLD {
                hub_pos.y = point.y;
            }
            if line_of_walk(point, hub_pos) {
                return Some(idx);
            }
        }
        None
    }

    /// O(1), zero-allocation reachability and shortest-path distance query
    /// between two hub indices, respecting `mask`, `can_jet`, `oneWay`, and
    /// active dynamic blocks.
    pub fn hub_route_distance(
        &self,
        from_hub: usize,
        to_hub: usize,
        mask: u32,
        can_jet: bool,
    ) -> RouteQuery {
        let n = self.hubs.len();
        if from_hub >= n || to_hub >= n || self.table(mask) == NO_TABLE {
            return RouteQuery::UNREACHABLE;
        }
        if from_hub == to_hub {
            if !self.hubs[from_hub].valid || (self.hubs[from_hub].mask & mask) == 0 {
                return RouteQuery::UNREACHABLE;
            }
            return RouteQuery::found(0.0, 0);
        }
        let table = self.table(mask);
        if table == NO_TABLE {
            return RouteQuery::UNREACHABLE;
        }
        let t = table as usize;
        let cache = if can_jet && self.has_jet_arcs {
            self.apsp_jet[t].as_ref()
        } else {
            self.apsp_ground[t].as_ref()
        };
        let Some(apsp) = cache else {
            return RouteQuery::UNREACHABLE;
        };
        let idx = from_hub * n + to_hub;
        let d = f64::from(apsp.dist[idx]);
        if !d.is_finite() {
            RouteQuery::UNREACHABLE
        } else {
            RouteQuery::found(d, u32::from(apsp.hops[idx]))
        }
    }

    /// Return the reachable hub in `from_hub`'s connected component that lies
    /// horizontally closest to `target` (strictly closer than `from_hub`), or
    /// `None` if `from_hub` is already the closest reachable hub. Zero-alloc
    /// O(N) scan over the precomputed APSP row with deterministic index
    /// tie-breaking.
    pub fn closest_reachable_hub_toward(
        &self,
        from_hub: usize,
        target: Vec3,
        mask: u32,
        can_jet: bool,
    ) -> Option<usize> {
        let n = self.hubs.len();
        if from_hub >= n || mask == 0 {
            return None;
        }
        let start_d2 = h2(self.hubs[from_hub].x, self.hubs[from_hub].z, target.x, target.z);
        let mut best_hub = None;
        let mut best_d2 = start_d2;
        for h in 0..n {
            if h == from_hub || !self.hubs[h].valid || (self.hubs[h].mask & mask) == 0 {
                continue;
            }
            if !self.hub_route_distance(from_hub, h, mask, can_jet).reachable {
                continue;
            }
            let d2 = h2(self.hubs[h].x, self.hubs[h].z, target.x, target.z);
            if d2 < best_d2 {
                best_d2 = d2;
                best_hub = Some(h);
            }
        }
        best_hub
    }

    /// Select the next outgoing arc from `from` toward `to` using the baked
    /// 2005 routing weight table when valid and unblocked, falling back to
    /// deterministic shortest-path Dijkstra whenever weights are absent, zero,
    /// blocked, or `force_shortest` is set.
    #[allow(clippy::too_many_arguments)]
    /// Provisional policy: sample eligible weighted slots, then try a shortest route.
    pub fn next_arc(
        &mut self,
        prev_arc: Option<usize>,
        avoid_arc: Option<usize>,
        from: usize,
        to: usize,
        mask: u32,
        can_jet: bool,
        force_shortest: bool,
        random: &mut dyn Random,
    ) -> Option<usize> {
        if from >= self.hubs.len() || to >= self.hubs.len() || from == to {
            return None;
        }
        if !force_shortest && self.manual_block_count == 0 && avoid_arc.is_none() {
            if let Some(arc) = self.select_weighted(prev_arc, from, to, mask, can_jet, random) {
                return Some(arc);
            }
        }
        self.shortest_first_arc(from, to, mask, can_jet, avoid_arc.or(prev_arc))
            .or_else(|| self.shortest_first_arc(from, to, mask, can_jet, None))
    }

    fn select_weighted(
        &self,
        prev_arc: Option<usize>,
        from: usize,
        to: usize,
        mask: u32,
        can_jet: bool,
        random: &mut dyn Random,
    ) -> Option<usize> {
        let table = self.table(mask);
        if table == NO_TABLE {
            return None;
        }
        let t = table as usize;
        if !self.table_has_weights[t] {
            return None;
        }
        let n = self.hubs.len();
        let word = self.words[(from * TABLE_COUNT + t) * n + to];
        if word == 0 {
            return None;
        }

        let mut cands: [(usize, u32); 4] = [(0, 0); 4];
        let mut count = 0usize;
        let mut total = 0u32;
        let mut w = word;
        for _ in 0..4 {
            let byte = (w & 0xff) as u8;
            if byte == 0 {
                break;
            }
            let slot_idx = usize::from(byte & 7);
            let weight = u32::from((byte >> 3) & 31);
            let slot_arc = self.hubs[from].slots[slot_idx];
            if slot_arc != UNUSED_SLOT && weight > 0 {
                let arc_idx = usize::from(slot_arc);
                if Some(arc_idx) != prev_arc && self.arc_traversable(arc_idx, from, mask, can_jet) {
                    cands[count] = (arc_idx, weight);
                    count += 1;
                    total += weight;
                }
            }
            w >>= 8;
        }
        if count == 0 || total == 0 {
            return None;
        }
        if count == 1 {
            return Some(cands[0].0);
        }
        let mut pick = random.next() % total;
        for &(arc_idx, weight) in &cands[..count] {
            if pick < weight {
                return Some(arc_idx);
            }
            pick -= weight;
        }
        Some(cands[count - 1].0)
    }

    /// Compute the deterministic shortest hub/arc route from `from` to `to`
    /// into internal pre-allocated scratch buffers (zero heap allocations).
    pub fn plan_route(
        &mut self,
        from: usize,
        to: usize,
        mask: u32,
        can_jet: bool,
        avoid_arc: Option<usize>,
    ) -> Option<(&[u32], &[u32])> {
        let n = self.hubs.len();
        if from >= n || to >= n || self.table(mask) == NO_TABLE
            || !self.hubs[from].valid || !self.hubs[to].valid
            || self.hubs[from].mask & self.hubs[to].mask & mask == 0 {
            return None;
        }
        self.scratch.route_hubs.clear();
        self.scratch.route_arcs.clear();
        if from == to {
            self.scratch.route_hubs.push(from as u32);
            return Some((&self.scratch.route_hubs, &self.scratch.route_arcs));
        }

        let stamp = self.scratch.next_stamp();
        self.scratch.heap_clear();
        self.scratch.seen[from] = stamp;
        self.scratch.dist[from] = 0.0;
        self.scratch.prev_hub[from] = u32::MAX;
        self.scratch.prev_arc[from] = u32::MAX;
        self.scratch.heap_push(0.0, from as u32);

        let mut found = false;
        while let Some((cost, u_u32)) = self.scratch.heap_pop() {
            let u = u_u32 as usize;
            if cost > self.scratch.dist[u] {
                continue;
            }
            if u == to {
                found = true;
                break;
            }
            let start_edge = self.adj_offsets[u];
            let end_edge = self.adj_offsets[u + 1];
            for edge_pos in start_edge..end_edge {
                let arc_idx = self.adj_arcs[edge_pos];
                if Some(arc_idx) == avoid_arc {
                    continue;
                }
                if !self.arc_traversable(arc_idx, u, mask, can_jet) {
                    continue;
                }
                let arc = &self.arcs[arc_idx];
                let v = arc.other_end(u);
                let penalty = if arc.jet_jump() {
                    JET_JUMP_ARC_PENALTY
                } else if arc.jump() {
                    JUMP_ARC_PENALTY
                } else {
                    0.0
                };
                let next_cost = cost + arc.length + penalty;
                let better = self.scratch.seen[v] != stamp
                    || next_cost < self.scratch.dist[v];
                if better {
                    self.scratch.seen[v] = stamp;
                    self.scratch.dist[v] = next_cost;
                    self.scratch.prev_hub[v] = u as u32;
                    self.scratch.prev_arc[v] = arc_idx as u32;
                    self.scratch.heap_push(next_cost, v as u32);
                }
            }
        }

        if !found {
            return None;
        }

        let mut cur = to;
        let mut guard = 0usize;
        while cur != from && guard <= n {
            self.scratch.route_hubs.push(cur as u32);
            let arc = self.scratch.prev_arc[cur];
            let prev = self.scratch.prev_hub[cur];
            if prev == u32::MAX || arc == u32::MAX {
                self.scratch.route_hubs.clear();
                self.scratch.route_arcs.clear();
                return None;
            }
            self.scratch.route_arcs.push(arc);
            cur = prev as usize;
            guard += 1;
        }
        if cur != from {
            self.scratch.route_hubs.clear();
            self.scratch.route_arcs.clear();
            return None;
        }
        self.scratch.route_hubs.push(from as u32);
        self.scratch.route_hubs.reverse();
        self.scratch.route_arcs.reverse();
        Some((&self.scratch.route_hubs, &self.scratch.route_arcs))
    }

    fn shortest_first_arc(
        &mut self,
        from: usize,
        to: usize,
        mask: u32,
        can_jet: bool,
        avoid_arc: Option<usize>,
    ) -> Option<usize> {
        let (_, arcs) = self.plan_route(from, to, mask, can_jet, avoid_arc)?;
        arcs.first().map(|&a| a as usize)
    }

    /// Rebuild cached all-pairs shortest-path tables (cold load or dynamic arc
    /// toggle). Reuses existing `ApspTable` vectors when already allocated so
    /// dynamic toggles do not grow memory.
    fn rebuild_apsp(&mut self) {
        let n = self.hubs.len();
        if n == 0 {
            return;
        }
        for t in 0..self.branch_count {
            let mask = 1u32 << t;
            let any_hub = self.hubs.iter().any(|h| h.valid && (h.mask & mask) != 0);
            if !any_hub {
                self.apsp_ground[t] = None;
                self.apsp_jet[t] = None;
                continue;
            }
            let mut ground = self.apsp_ground[t].take().unwrap_or_else(|| ApspTable {
                dist: vec![f32::INFINITY; n * n],
                hops: vec![0; n * n],
            });
            self.fill_apsp_table(&mut ground, mask, false);
            self.apsp_ground[t] = Some(ground);

            if self.has_jet_arcs {
                let mut jet = self.apsp_jet[t].take().unwrap_or_else(|| ApspTable {
                    dist: vec![f32::INFINITY; n * n],
                    hops: vec![0; n * n],
                });
                self.fill_apsp_table(&mut jet, mask, true);
                self.apsp_jet[t] = Some(jet);
            } else {
                self.apsp_jet[t] = None;
            }
        }
    }

    fn fill_apsp_table(&mut self, table: &mut ApspTable, mask: u32, can_jet: bool) {
        let n = self.hubs.len();
        table.dist.fill(f32::INFINITY);
        table.hops.fill(0);

        for src in 0..n {
            if !self.hubs[src].valid || (self.hubs[src].mask & mask) == 0 {
                continue;
            }
            let stamp = self.scratch.next_stamp();
            self.scratch.heap_clear();
            self.scratch.seen[src] = stamp;
            self.scratch.dist[src] = 0.0;
            self.scratch.hops[src] = 0;
            self.scratch.heap_push(0.0, src as u32);

            while let Some((cost, u_u32)) = self.scratch.heap_pop() {
                let u = u_u32 as usize;
                if cost > self.scratch.dist[u] {
                    continue;
                }
                let u_hops = self.scratch.hops[u];
                let start_edge = self.adj_offsets[u];
                let end_edge = self.adj_offsets[u + 1];
                for edge_pos in start_edge..end_edge {
                    let arc_idx = self.adj_arcs[edge_pos];
                    if !self.arc_traversable(arc_idx, u, mask, can_jet) {
                        continue;
                    }
                    let arc = &self.arcs[arc_idx];
                    let v = arc.other_end(u);
                    let next_cost = cost + arc.length;
                    let next_hops = u_hops.saturating_add(1);
                    let better = self.scratch.seen[v] != stamp
                        || next_cost < self.scratch.dist[v]
                        || (next_cost == self.scratch.dist[v] && next_hops < self.scratch.hops[v]);
                    if better {
                        self.scratch.seen[v] = stamp;
                        self.scratch.dist[v] = next_cost;
                        self.scratch.hops[v] = next_hops;
                        self.scratch.heap_push(next_cost, v as u32);
                    }
                }
            }

            let row = src * n;
            for dst in 0..n {
                if self.scratch.seen[dst] == stamp {
                    table.dist[row + dst] = f32(self.scratch.dist[dst]) as f32;
                    table.hops[row + dst] = self.scratch.hops[dst];
                }
            }
        }
    }

    /// Set a `plnp` dynamic blocking group state bit (`true` = blocked),
    /// applying the corresponding XOR weight delta when available.
    pub fn set_group_blocked(&mut self, group_idx: usize, blocked: bool) -> bool {
        if group_idx >= self.groups.len() || group_idx >= 32 {
            return false;
        }
        let bit = 1u32 << group_idx;
        let next_state = if blocked {
            self.group_state | bit
        } else {
            self.group_state & !bit
        };
        self.apply_group_state(next_state)
    }

    /// Toggle a `plnp` dynamic blocking group by index.
    pub fn toggle_group(&mut self, group_idx: usize) -> bool {
        if group_idx >= self.groups.len() || group_idx >= 32 {
            return false;
        }
        let next_state = self.group_state ^ (1u32 << group_idx);
        self.apply_group_state(next_state)
    }

    /// Block or unblock dynamic arcs by connection name (case-insensitive FNV-1a).
    /// A single-connection weight group can switch with its connection.
    /// Multi-connection groups switch by index. A named door in such a group
    /// changes only the matching arcs, so adjacent doors can remain open.
    pub fn set_arcs_blocked(&mut self, name: &str, blocked: bool) -> bool {
        let hash = zero_hash::fnv1a(name);
        let mut matched_group = false;
        let mut next_state = self.group_state;
        for g in &self.groups {
            if g.index < 32 && g.arc_hashes.as_slice() == [hash] {
                matched_group = true;
                let bit = 1u32 << g.index;
                if blocked {
                    next_state |= bit;
                } else {
                    next_state &= !bit;
                }
            }
        }
        if matched_group {
            return self.apply_group_state(next_state);
        }
        let mut matched_arc = false;
        let mut changed = false;
        for arc in &mut self.arcs {
            if arc.name_hash == hash {
                matched_arc = true;
                if arc.blocked_manual != blocked {
                    arc.blocked_manual = blocked;
                    changed = true;
                    if blocked {
                        self.manual_block_count += 1;
                    } else {
                        self.manual_block_count = self.manual_block_count.saturating_sub(1);
                    }
                }
            }
        }
        if changed {
            self.rebuild_apsp();
        }
        matched_arc
    }

    /// Toggle dynamic arcs by connection name.
    pub fn toggle_arcs(&mut self, name: &str) -> bool {
        let hash = zero_hash::fnv1a(name);
        let mut matched_group = false;
        let mut next_state = self.group_state;
        for g in &self.groups {
            if g.index < 32 && g.arc_hashes.as_slice() == [hash] {
                matched_group = true;
                next_state ^= 1u32 << g.index;
            }
        }
        if matched_group {
            return self.apply_group_state(next_state);
        }
        let mut matched_arc = false;
        for arc in &mut self.arcs {
            if arc.name_hash == hash {
                matched_arc = true;
                arc.blocked_manual = !arc.blocked_manual;
                if arc.blocked_manual {
                    self.manual_block_count += 1;
                } else {
                    self.manual_block_count = self.manual_block_count.saturating_sub(1);
                }
            }
        }
        if matched_arc {
            self.rebuild_apsp();
        }
        matched_arc
    }

    fn apply_group_state(&mut self, next_state: u32) -> bool {
        let old_state = self.group_state;
        if old_state == next_state {
            return true;
        }
        if next_state != 0 && !self.deltas.get(next_state as usize).is_some_and(Option::is_some) {
            return false;
        }
        if old_state != 0 {
            self.xor_delta_block(old_state as usize);
        }
        self.group_state = next_state;
        if next_state != 0 {
            self.xor_delta_block(next_state as usize);
        }
        for arc in &mut self.arcs {
            arc.blocked_by_group = false;
        }
        for g in &self.groups {
            if g.index < 32 && (self.group_state & (1u32 << g.index)) != 0 {
                for &arc_idx in &g.arc_indices {
                    if let Some(arc) = self.arcs.get_mut(arc_idx) {
                        arc.blocked_by_group = true;
                    }
                }
            }
        }
        self.rebuild_apsp();
        true
    }

    fn xor_delta_block(&mut self, block_idx: usize) {
        if let Some(Some(delta)) = self.deltas.get(block_idx) {
            xor_sparse_delta(&mut self.words, self.hubs.len(), self.branch_count, delta);
        }
    }
}

fn decode_delta(bytes: &[u8], size: usize, hubs: usize, tables: usize) -> Option<Vec<u8>> {
    if size < hubs || size > hubs * (1 + tables * (1 + 4 * hubs)) { return None; }
    let mut data = vec![0; size + 1];
    let mut decoder = Decompress::new(true);
    let status = decoder.decompress(bytes, &mut data, FlushDecompress::Finish).ok()?;
    if status != Status::StreamEnd || decoder.total_in() != bytes.len() as u64
        || decoder.total_out() != size as u64 { return None; }
    data.truncate(size);
    let mut pos = 0;
    for _ in 0..hubs {
        let changed = *data.get(pos)?;
        pos += 1;
        if changed == 0 { continue; }
        for _ in 0..tables {
            let rows = *data.get(pos)?;
            if rows > 4 { return None; }
            pos += 1 + usize::from(rows) * hubs;
        }
    }
    (pos == size).then_some(data)
}

/// Apply one decompressed `plnp` sparse XOR delta stream to `words`.
/// Format per hub `0..N`: `changed: u8`; if nonzero, for each table `0..tables`,
/// `nrows: u8` followed by `nrows * N` row-major XOR delta bytes.
fn xor_sparse_delta(words: &mut [u32], hub_count: usize, tables: usize, delta: &[u8]) {
    let mut pos = 0usize;
    for h in 0..hub_count {
        if pos >= delta.len() {
            return;
        }
        let changed = delta[pos];
        pos += 1;
        if changed == 0 {
            continue;
        }
        for t in 0..tables {
            if pos >= delta.len() {
                return;
            }
            let nrows = usize::from(delta[pos]);
            pos += 1;
            let needed = nrows.saturating_mul(hub_count);
            if pos + needed > delta.len() {
                return;
            }
            let base = (h * TABLE_COUNT + t) * hub_count;
            for row in 0..nrows {
                let row_slice = &delta[pos..pos + hub_count];
                pos += hub_count;
                if row < 4 {
                    let shift = (row as u32) * 8;
                    for (dest, &b) in row_slice.iter().enumerate() {
                        if b != 0 {
                            words[base + dest] ^= u32::from(b) << shift;
                        }
                    }
                }
            }
        }
    }
}

fn sort_candidates(list: &mut [(f64, f64, usize)]) {
    list.sort_unstable_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.total_cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
}

fn compute_weak_components(hubs: &[Swbf2Hub], arcs: &[Swbf2Arc], mask: u32) -> Vec<Vec<usize>> {
    let n = hubs.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for arc in arcs {
        if arc.degenerate || (arc.filter_flags & mask) == 0 {
            continue;
        }
        adj[arc.start].push(arc.end);
        adj[arc.end].push(arc.start);
    }
    let mut visited = vec![false; n];
    let mut comps = Vec::new();
    let mut stack = Vec::new();
    for start in 0..n {
        if visited[start] || !hubs[start].valid || (hubs[start].mask & mask) == 0 {
            continue;
        }
        visited[start] = true;
        stack.push(start);
        let mut comp = Vec::new();
        while let Some(u) = stack.pop() {
            comp.push(u);
            for &v in &adj[u] {
                if !visited[v] && hubs[v].valid && (hubs[v].mask & mask) != 0 {
                    visited[v] = true;
                    stack.push(v);
                }
            }
        }
        comp.sort_unstable();
        comps.push(comp);
    }
    comps
}

#[cfg(test)]
mod delta_tests {
    use super::decode_delta;
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;

    #[test]
    fn reject_incomplete_or_overwide_delta_before_use() {
        for (raw, ok) in [(&[1, 1, 18, 3, 0][..], true),
            (&[1, 1, 18, 3], false), (&[1, 5, 0, 0, 0], false), (&[0, 0, 1], false)] {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(raw).unwrap();
            let compressed = encoder.finish().unwrap();
            assert_eq!(decode_delta(&compressed, raw.len(), 2, 1).is_some(), ok);
            assert!(decode_delta(&compressed, raw.len() + 1, 2, 1).is_none());
        }
    }
}
