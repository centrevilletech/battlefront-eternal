//! TreeCanopies.update selects nearby source groups before either scene draws.
//! The groups' geometry stays with its resource owner; this retained selector
//! owns frustum/range decisions, stable distance order and changed leaf slots.
use crate::math::{self, Frustum, Mat4};
use crate::scene::layout::{scene_canopy_view as v, scene_canopy_header as h,
    scene_canopy_selection as s, scene_canopy_limits as limits};
use std::cmp::Ordering;

pub struct CanopySelection {
    pub view: [f64; v::STRIDE],
    pub bounds: Vec<f64>,
    pub states: Vec<f32>,
    pub output: [f64; h::STRIDE + limits::SELECTED * s::STRIDE],
    generation: u32,
    candidates: Vec<(usize, f64)>,
    active: [usize; limits::SELECTED],
    active_count: usize,
    last: [f64; v::STRIDE],
    valid: bool,
    pub clipped: u64,
}
impl Default for CanopySelection {
    fn default() -> Self {
        Self { view: [0.; v::STRIDE], bounds: Vec::new(), states: Vec::new(),
            output: [0.; h::STRIDE + limits::SELECTED * s::STRIDE], generation: 0,
            candidates: Vec::new(), active: [usize::MAX; limits::SELECTED],
            active_count: 0, last: [0.; v::STRIDE], valid: false, clipped: 0 }
    }
}
impl CanopySelection {
    // Cold content admission reserves every candidate and state row once.
    // A replacement generation starts a new canopy resource, never an alias
    // of the preceding group's bounds or retained active leaf positions.
    pub fn reserve(&mut self, count: usize, generation: u32) {
        let count = count.min(limits::GROUPS);
        if generation == 0 || generation < self.generation { self.clipped += 1; return; }
        let replaced = generation != self.generation || count * 4 != self.bounds.len();
        if replaced {
            self.active.fill(usize::MAX); self.active_count = 0;
            self.output.fill(0.); self.valid = false;
        }
        self.generation = generation;
        self.bounds.resize(count * 4, 0.); self.states.resize(count * 4, 0.);
        if replaced { self.states.fill(0.); }
        self.candidates.reserve(count.saturating_sub(self.candidates.len()));
    }

    // The output has fixed room for twelve groups, independent of map size.
    // Only the selected prefix is live; inactive rows are never commands.
    // Clear transient change flags even when the camera cache can be reused.
    pub fn select(&mut self) -> usize {
        self.output[h::CHANGED] = 0.;
        for i in 0..self.active_count { self.output[h::STRIDE + i * s::STRIDE + s::CHANGED] = 0.; }
        if self.view[v::GENERATION] != self.generation as f64 {
            self.valid = false; self.clipped += 1;
            self.output[h::COUNT] = 0.; self.output[h::SHOWN] = 0.; return 0;
        }
        self.output[h::SHOWN] = (self.active_count > 0 && self.view[v::LAYERS] as i64 as u32 & self.view[v::NODE_LAYERS] as i64 as u32 != 0) as u8 as f64;
        self.output[h::DIRTY] = f64::INFINITY;
        self.output[h::DIRTY + 1] = 0.;
        // TreeCanopies.lastCamera/lastMatrix/lastProjection deliberately ignore
        // the inverse matrix. Numeric equality also treats signed zeros alike,
        // exactly as Matrix4.equals did. NaN must invalidate this cache.
        let same = self.valid && self.view[v::CAMERA] == self.last[v::CAMERA]
            && self.view[v::ENABLED] == self.last[v::ENABLED]
            && self.view[v::WORLD..v::WORLD + 16] == self.last[v::WORLD..v::WORLD + 16]
            && self.view[v::PROJECTION..v::PROJECTION + 16] == self.last[v::PROJECTION..v::PROJECTION + 16];
        if same { return self.active_count; }
        // Frustum construction follows Matrix4.multiplyMatrices before plane
        // normalization. Reordering those f64 operations changes edge cases.
        // The same spheres feed both near leaves and distant-card masking.
        self.last = self.view; self.valid = true; self.candidates.clear();
        let projection: Mat4 = self.view[v::PROJECTION..v::PROJECTION + 16].try_into().unwrap_or([0.; 16]);
        let camera: Mat4 = self.view[v::VIEW..v::VIEW + 16].try_into().unwrap_or([0.; 16]);
        let frustum = Frustum::from_matrix(&math::mul(&projection, &camera));
        if self.view[v::ENABLED] != 0. {
            for (id, sphere) in self.bounds.chunks_exact(4).enumerate() {
                let center = [sphere[0], sphere[1], sphere[2]];
                if !frustum.sphere(&center, sphere[3]) { continue; }
                // The original helper used camera.position, including cube
                // children with a transformed parent. Do not substitute the
                // world-space eye merely because it seems more conventional.
                let dx = sphere[0] - self.view[v::EYE];
                let dy = sphere[1] - self.view[v::EYE + 1];
                let dz = sphere[2] - self.view[v::EYE + 2];
                let distance = dx * dx + dy * dy + dz * dz;
                if distance.sqrt() - sphere[3] < limits::FAR as f64 {
                    if self.candidates.len() < self.candidates.capacity() {
                        self.candidates.push((id, distance));
                    } else { self.clipped += 1; }
                }
            }
        }
        // Array.sort was stable: equal distances preserve authored group order.
        // The explicit source index keeps that contract without sort scratch.
        self.candidates.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal).then(a.0.cmp(&b.0)));
        let count = self.candidates.len().min(limits::SELECTED);
        let mut changed = count != self.active_count;
        for i in 0..count {
            let (id, distance) = self.candidates[i];
            let moved = self.active[i] != id;
            changed |= moved; self.active[i] = id;
            let at = h::STRIDE + i * s::STRIDE;
            self.output[at + s::ID] = id as f64;
            self.output[at + s::GENERATION] = self.generation as f64;
            self.output[at + s::DISTANCE] = distance;
            self.output[at + s::FIRST] = (i * limits::LEAVES) as f64;
            self.output[at + s::COUNT] = limits::LEAVES as f64;
            self.output[at + s::CHANGED] = moved as u8 as f64;
            if moved {
                self.output[h::DIRTY] = self.output[h::DIRTY].min((i * limits::LEAVES) as f64);
                self.output[h::DIRTY + 1] = ((i + 1) * limits::LEAVES) as f64;
            }
        }
        // Clearing the inactive tail matters when a disabled view is followed
        // by re-enabling the same groups: every selected slot then uploads.
        self.active[count..].fill(usize::MAX); self.active_count = count;
        // TreeCanopies' mask texture follows the chosen source groups, not
        // their slot indices. Reordering two groups still dirties leaf bytes,
        // while their state values continue to identify the authored groups.
        if changed {
            self.states.fill(0.);
            for &id in &self.active[..count] { self.states[id * 4] = 1.; }
            self.output[h::VERSION] += 1.; self.output[h::CHANGED] = 1.;
        }
        // Group selection does not depend on object layers. The draw does;
        // preserve that distinction for callers reusing a cached camera.
        // No host parent walk or boolean selection is needed after this row.
        self.output[h::COUNT] = count as f64;
        self.output[h::SHOWN] = (count > 0 && self.view[v::LAYERS] as i64 as u32 & self.view[v::NODE_LAYERS] as i64 as u32 != 0) as u8 as f64;
        count
    }
}
