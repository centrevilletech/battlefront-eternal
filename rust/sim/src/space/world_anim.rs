//! Deterministic 60 Hz world-animation runtime for 2005 space battlefields.
//!
//! Evaluates authored `WorldAnimations` clips, groups, and parent-child
//! hierarchies using `v8math` transcendentals and pre-indexed flat tables so
//! orbiting frigates and spinning ship props advance identically across x86-64
//! and ARM64 with zero per-tick allocations.

use bfe_wide::Wide;
use super::SPACE_FIXED_DT;
use super::digest::Fnv;
use crate::collision::spheres::{box_apply_matrix4, Bounds};
use crate::collision::State as CollisionState;
use bfe_content::world_anim::{
    euler_xyz_quaternion as content_euler_xyz, sample_clip_at, WorldAnimClip, WorldAnimations,
};
use three::{Matrix4, Quaternion, Vector3};

/// One pre-resolved object driven by world animations (a parent or a hierarchy
/// child), linked directly to its collision kernel body slots and exposing its
/// previous-tick and current-tick matrices for render interpolation.
#[derive(Clone, Debug)]
pub struct AnimatedObject {
    pub name: String,
    pub prop_instance: usize,
    pub base_matrix: [f64; 16],
    pub base_inverse: [f64; 16],
    /// Authoritative transform at the previous fixed 60 Hz simulation tick
    /// (for drawing interpolation alongside `live_matrix`).
    pub prev_matrix: [f64; 16],
    pub live_matrix: [f64; 16],
    pub live_inverse: [f64; 16],
    /// Up to three collision look kernel slots `[built, destroyed, unbuilt]`
    /// and their local-space AABBs so moving bodies update in O(1).
    pub bodies: [Option<(u32, Bounds)>; 3],
}

#[derive(Clone, Debug)]
struct ResolvedBinding {
    clip_index: usize,
    object_index: usize,
}

#[derive(Clone, Debug)]
struct ResolvedGroup {
    name: String,
    active: bool,
    paused: bool,
    disable_hierarchies: bool,
    elapsed_ticks: u32,
    time_s: f64,
    bindings: Vec<ResolvedBinding>,
}

#[derive(Clone, Debug)]
struct ResolvedHierarchy {
    parent_index: usize,
    children: Vec<usize>,
}

/// Deterministic, zero-allocation-per-tick runtime for 2005 world animations.
/// Every authored frigate or prop motion is computed as a pure function of the
/// integer 60 Hz simulation tick so collision and drawing share one clock.
#[derive(Clone, Debug, Default)]
pub struct SpaceWorldAnims {
    clips: Vec<WorldAnimClip>,
    groups: Vec<ResolvedGroup>,
    hierarchies: Vec<ResolvedHierarchy>,
    pub objects: Vec<AnimatedObject>,
    pub sim_tick: u32,
    /// Pre-allocated scratch flag per object: true when a group with
    /// `disable_hierarchies` touched that parent this tick.
    hierarchy_disabled: Vec<bool>,
    /// Pre-allocated scratch flag per object: true when animated this tick.
    touched: Vec<bool>,
}

/// Interpolates a published pair without borrowing the simulation during drawing.
pub fn interpolate_transform(prev: [f64;16], live: [f64;16], alpha: f64) -> [f64;16] {
    let a = v8math::min(1.0, v8math::max(0.0, alpha));
    let mp = Matrix4 { elements: prev };
    let ml = Matrix4 { elements: live };
    let (mut pp, mut qp, mut sp) = (Vector3::default(), Quaternion::default(), Vector3::new(1.0, 1.0, 1.0));
    let (mut pl, mut ql, mut sl) = (Vector3::default(), Quaternion::default(), Vector3::new(1.0, 1.0, 1.0));
    mp.decompose(&mut pp, &mut qp, &mut sp);
    ml.decompose(&mut pl, &mut ql, &mut sl);
    let pos = Vector3::new(
        pp.x + (pl.x - pp.x) * a,
        pp.y + (pl.y - pp.y) * a,
        pp.z + (pl.z - pp.z) * a,
    );
    let scale = Vector3::new(
        sp.x + (sl.x - sp.x) * a,
        sp.y + (sl.y - sp.y) * a,
        sp.z + (sl.z - sp.z) * a,
    );
    let dot = qp.x * ql.x + qp.y * ql.y + qp.z * ql.z + qp.w * ql.w;
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    let mut q = Quaternion {
        x: qp.x + (ql.x * sign - qp.x) * a,
        y: qp.y + (ql.y * sign - qp.y) * a,
        z: qp.z + (ql.z * sign - qp.z) * a,
        w: qp.w + (ql.w * sign - qp.w) * a,
    };
    q.normalize();
    let mut out = Matrix4::default();
    out.compose(&pos, &q, &scale);
    out.elements
}

fn euler_xyz_quaternion(angles: [f64; 3]) -> Quaternion {
    let q = content_euler_xyz(angles);
    Quaternion { x: q.x, y: q.y, z: q.z, w: q.w }
}

/// Deterministically samples `(position_delta, rotation_euler_rad)` for a clip
/// at `time_s`, delegating directly to the single shared implementation in
/// `bfe_content::world_anim::sample_clip_at`.
pub fn sample_clip_deterministic(clip: &WorldAnimClip, time_s: f64) -> ([f64; 3], [f64; 3]) {
    sample_clip_at(clip, time_s)
}

/// Checks whether a placed object name participates in any world animation
/// binding or hierarchy (parent or child), ASCII case-insensitively.
pub fn is_animated_name(anims: &WorldAnimations, name: &str) -> bool {
    if name.is_empty() || anims.is_empty() {
        return false;
    }
    for g in &anims.groups {
        if g.bindings.iter().any(|b| b.object.eq_ignore_ascii_case(name)) {
            return true;
        }
    }
    for h in &anims.hierarchies {
        if h.parent.eq_ignore_ascii_case(name)
            || h.children.iter().any(|c| c.eq_ignore_ascii_case(name))
        {
            return true;
        }
    }
    false
}

impl SpaceWorldAnims {
    /// Builds the pre-indexed animation runtime from authored `WorldAnimations`
    /// and the list of placed animated objects with their collision body slots.
    pub fn build(anims: &WorldAnimations, objects: Vec<AnimatedObject>) -> Self {
        let mut resolved_groups = Vec::with_capacity(anims.groups.len());
        for group in &anims.groups {
            let mut bindings = Vec::new();
            for b in &group.bindings {
                let Some(clip_index) = anims
                    .clips
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(&b.clip))
                else {
                    continue;
                };
                for (obj_idx, obj) in objects.iter().enumerate() {
                    if obj.name.eq_ignore_ascii_case(&b.object) {
                        bindings.push(ResolvedBinding {
                            clip_index,
                            object_index: obj_idx,
                        });
                    }
                }
            }
            resolved_groups.push(ResolvedGroup {
                name: group.name.clone(),
                active: group.default_on,
                paused: false,
                disable_hierarchies: group.disable_hierarchies,
                elapsed_ticks: 0,
                time_s: 0.0,
                bindings,
            });
        }

        let mut resolved_hierarchies = Vec::new();
        for h in &anims.hierarchies {
            let Some(parent_index) = objects
                .iter()
                .position(|o| o.name.eq_ignore_ascii_case(&h.parent))
            else {
                continue;
            };
            let mut children = Vec::new();
            for child_name in &h.children {
                for (obj_idx, obj) in objects.iter().enumerate() {
                    if obj.name.eq_ignore_ascii_case(child_name) && !children.contains(&obj_idx) {
                        children.push(obj_idx);
                    }
                }
            }
            if !children.is_empty() {
                resolved_hierarchies.push(ResolvedHierarchy {
                    parent_index,
                    children,
                });
            }
        }

        let n = objects.len();
        Self {
            clips: anims.clips.clone(),
            groups: resolved_groups,
            hierarchies: resolved_hierarchies,
            objects,
            sim_tick: 0,
            hierarchy_disabled: vec![false; n],
            touched: vec![false; n],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Wire clock `(state, origin)` for group `index` at `authority_tick`:
    /// - `0` = stopped (`origin = 0`)
    /// - `1` = playing (`origin = authority_tick.wrapping_sub(elapsed_ticks as u32)`, constant while running)
    /// - `2` = paused (`origin = elapsed_ticks as u32`, constant while paused)
    pub fn group_clock(&self, index: usize, authority_tick: u32) -> Option<(u8, u32)> {
        let g = self.groups.get(index)?;
        if !g.active {
            Some((0, 0))
        } else if g.paused {
            Some((2, g.elapsed_ticks as u32))
        } else {
            Some((1, authority_tick.wrapping_sub(g.elapsed_ticks as u32)))
        }
    }

    /// Evaluates all animated object transforms directly from wire `(state, origin)` group clocks
    /// at `authority_tick`, populating both `prev_matrix` (at `authority_tick - 1`) and
    /// `live_matrix` (at `authority_tick`) using the exact same deterministic clip & hierarchy math.
    pub fn sync_from_clocks(
        &mut self,
        clocks: &[(u8, u32)],
        authority_tick: u32,
        mut collision: Option<&mut CollisionState>,
    ) {
        self.sim_tick = authority_tick;
        if self.objects.is_empty() || self.groups.is_empty() {
            return;
        }
        let apply_at = |this: &mut Self, eval_tick: u32| {
            for (g, &(state, origin)) in this.groups.iter_mut().zip(clocks.iter()) {
                match state {
                    0 => {
                        g.active = false;
                        g.paused = false;
                        g.elapsed_ticks = 0;
                        g.time_s = 0.0;
                    }
                    1 => {
                        g.active = true;
                        g.paused = false;
                        g.elapsed_ticks = eval_tick.wrapping_sub(origin);
                        g.time_s = (g.elapsed_ticks as f64) * SPACE_FIXED_DT;
                    }
                    _ => {
                        g.active = true;
                        g.paused = true;
                        g.elapsed_ticks = origin;
                        g.time_s = (g.elapsed_ticks as f64) * SPACE_FIXED_DT;
                    }
                }
            }
            this.hierarchy_disabled.fill(false);
            this.touched.fill(false);
            for obj in &mut this.objects {
                obj.live_matrix = obj.base_matrix;
                obj.live_inverse = obj.base_inverse;
            }
            for group in &this.groups {
                if !group.active {
                    continue;
                }
                let t = group.time_s;
                for binding in &group.bindings {
                    let clip = &this.clips[binding.clip_index];
                    let (pos, rot) = sample_clip_deterministic(clip, t);
                    let q = euler_xyz_quaternion(rot);
                    let mut local_delta = Matrix4::default();
                    local_delta.compose(
                        &Vector3::new(pos[0], pos[1], pos[2]),
                        &q,
                        &Vector3::new(1.0, 1.0, 1.0),
                    );
                    let obj = &mut this.objects[binding.object_index];
                    let base = Matrix4 { elements: obj.base_matrix };
                    let mut live = Matrix4::default();
                    live.multiply_matrices(&base, &local_delta);
                    let mut inv = live;
                    inv.invert();
                    obj.live_matrix = live.elements;
                    obj.live_inverse = inv.elements;
                    this.touched[binding.object_index] = true;
                    if group.disable_hierarchies {
                        this.hierarchy_disabled[binding.object_index] = true;
                    }
                }
            }
            for h in &this.hierarchies {
                if this.hierarchy_disabled[h.parent_index] || !this.touched[h.parent_index] {
                    continue;
                }
                let parent_live = Matrix4 { elements: this.objects[h.parent_index].live_matrix };
                let parent_base_inv = Matrix4 { elements: this.objects[h.parent_index].base_inverse };
                let mut parent_delta = Matrix4::default();
                parent_delta.multiply_matrices(&parent_live, &parent_base_inv);
                for &child_idx in &h.children {
                    let child_base = Matrix4 { elements: this.objects[child_idx].base_matrix };
                    let mut child_live = Matrix4::default();
                    child_live.multiply_matrices(&parent_delta, &child_base);
                    let mut child_inv = child_live;
                    child_inv.invert();
                    this.objects[child_idx].live_matrix = child_live.elements;
                    this.objects[child_idx].live_inverse = child_inv.elements;
                    this.touched[child_idx] = true;
                }
            }
        };
        apply_at(self, authority_tick.saturating_sub(1));
        for obj in &mut self.objects {
            obj.prev_matrix = obj.live_matrix;
        }
        apply_at(self, authority_tick);
        if let Some(collision) = collision.as_deref_mut() {
            for (idx, obj) in self.objects.iter().enumerate() {
                if !self.touched[idx] {
                    continue;
                }
                for slot in obj.bodies.iter().flatten() {
                    let (kernel, local_bounds) = *slot;
                    if let Some(body) = collision.bodies.get_mut(kernel as usize) {
                        if body.moving {
                            body.bounds = box_apply_matrix4(&local_bounds, &obj.live_matrix);
                            body.matrix = obj.live_matrix;
                            body.inverse = obj.live_inverse;
                        }
                    }
                }
            }
        }
    }

    /// Starts or restarts a named animation group.
    pub fn play_group(&mut self, name: &str) -> bool {
        if let Some(g) = self.groups.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)) {
            g.active = true;
            g.paused = false;
            true
        } else {
            false
        }
    }

    /// Pauses or resumes a named animation group.
    pub fn set_group_paused(&mut self, name: &str, paused: bool) -> bool {
        if let Some(g) = self.groups.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)) {
            g.paused = paused;
            true
        } else {
            false
        }
    }

    /// Pauses a named animation group (`PauseAnimationGroup`).
    pub fn pause_group(&mut self, name: &str) -> bool {
        self.set_group_paused(name, true)
    }

    /// Looks up the live world matrix of an animated object by script name.
    pub fn live_matrix_of(&self, name: &str) -> Option<[f64; 16]> {
        self.objects
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name))
            .map(|o| o.live_matrix)
    }

    /// Returns `(prev_matrix, live_matrix)` for placed prop `prop_instance` so
    /// the drawing layer can interpolate between consecutive fixed 60 Hz ticks
    /// without running a second clock.
    pub fn transform_pair_for_instance(
        &self,
        prop_instance: usize,
    ) -> Option<([f64; 16], [f64; 16])> {
        self.objects
            .iter()
            .find(|o| o.prop_instance == prop_instance)
            .map(|o| (o.prev_matrix, o.live_matrix))
    }

    /// Returns an interpolated column-major world matrix for `prop_instance`
    /// at render fraction `alpha` in `[0.0, 1.0]` between `prev_matrix` and
    /// `live_matrix`.
    pub fn interpolated_matrix(&self, prop_instance: usize, alpha: f64) -> Option<[f64; 16]> {
        let (prev, live) = self.transform_pair_for_instance(prop_instance)?;
        Some(interpolate_transform(prev, live, alpha))
    }

    /// Advances active world animation groups by one fixed 60 Hz simulation
    /// tick (`elapsed_ticks * SPACE_FIXED_DT`, independent of caller `dt`),
    /// snapshots `prev_matrix` for drawing interpolation, and updates all bound
    /// `moving` collision bodies in `collision`. Allocates nothing.
    pub fn step(&mut self, _dt: f64, collision: &mut CollisionState) {
        self.sim_tick = self.sim_tick.saturating_add(1);
        if self.objects.is_empty() || self.groups.is_empty() {
            return;
        }
        for obj in &mut self.objects {
            obj.prev_matrix = obj.live_matrix;
        }
        self.hierarchy_disabled.fill(false);
        self.touched.fill(false);

        for group in &mut self.groups {
            if !group.active {
                continue;
            }
            if !group.paused {
                group.elapsed_ticks = group.elapsed_ticks.saturating_add(1);
                group.time_s = (group.elapsed_ticks as f64) * SPACE_FIXED_DT;
            }
            let t = group.time_s;
            for binding in &group.bindings {
                let clip = &self.clips[binding.clip_index];
                let (pos, rot) = sample_clip_deterministic(clip, t);
                let q = euler_xyz_quaternion(rot);
                let mut local_delta = Matrix4::default();
                local_delta.compose(
                    &Vector3::new(pos[0], pos[1], pos[2]),
                    &q,
                    &Vector3::new(1.0, 1.0, 1.0),
                );
                let obj = &mut self.objects[binding.object_index];
                let base = Matrix4 { elements: obj.base_matrix };
                let mut live = Matrix4::default();
                live.multiply_matrices(&base, &local_delta);
                let mut inv = live;
                inv.invert();
                obj.live_matrix = live.elements;
                obj.live_inverse = inv.elements;
                self.touched[binding.object_index] = true;
                if group.disable_hierarchies {
                    self.hierarchy_disabled[binding.object_index] = true;
                }
            }
        }

        // Propagate parent motion to hierarchy children:
        // child_world(t) = parent_world(t) * inverse(parent_base) * child_base.
        for h in &self.hierarchies {
            if self.hierarchy_disabled[h.parent_index] || !self.touched[h.parent_index] {
                continue;
            }
            let parent_live = Matrix4 { elements: self.objects[h.parent_index].live_matrix };
            let parent_base_inv = Matrix4 { elements: self.objects[h.parent_index].base_inverse };
            let mut parent_delta = Matrix4::default();
            parent_delta.multiply_matrices(&parent_live, &parent_base_inv);

            for &child_idx in &h.children {
                let child_base = Matrix4 { elements: self.objects[child_idx].base_matrix };
                let mut child_live = Matrix4::default();
                child_live.multiply_matrices(&parent_delta, &child_base);
                let mut child_inv = child_live;
                child_inv.invert();
                self.objects[child_idx].live_matrix = child_live.elements;
                self.objects[child_idx].live_inverse = child_inv.elements;
                self.touched[child_idx] = true;
            }
        }

        // Write updated transforms to moving collision bodies without bumping
        // fixed_version.
        for (idx, obj) in self.objects.iter().enumerate() {
            if !self.touched[idx] {
                continue;
            }
            for slot in obj.bodies.iter().flatten() {
                let (kernel, local_bounds) = *slot;
                if let Some(body) = collision.bodies.get_mut(kernel as usize) {
                    if body.moving {
                        body.bounds = box_apply_matrix4(&local_bounds, &obj.live_matrix);
                        body.matrix = obj.live_matrix;
                        body.inverse = obj.live_inverse;
                    }
                }
            }
        }
    }

    /// Mixes all animated object live matrices into an FNV-1a digest.
    pub fn mix_digest(&self, hash: Wide) -> Wide {
        let mut m = Fnv::new(hash);
        for g in &self.groups {
            m.small(g.active);
            m.small(g.paused);
            m.small(g.elapsed_ticks);
            m.bits(g.time_s);
        }
        for obj in &self.objects {
            for &word in obj.live_matrix.iter().chain(&obj.prev_matrix) {
                m.bits(word);
            }
        }
        m.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{CollisionOwner, Ray};
    use bfe_content::bvh::{build as build_bvh, Positions};
    use bfe_content::install::CollisionKernel;
    use bfe_content::world_anim::WorldAnimKey;

    #[test]
    fn orbiting_frigate_moves_collision_body_deterministically() {
        let clip = WorldAnimClip {
            name: "orbit".into(),
            duration: 10.0,
            looping: true,
            local_translation: true,
            positions: vec![
                WorldAnimKey { time: 0.0, value: [0.0, 0.0, 0.0], kind: 1, tangents: [0.0; 6] },
                WorldAnimKey { time: 10.0, value: [100.0, 0.0, 0.0], kind: 1, tangents: [0.0; 6] },
            ],
            rotations: vec![
                WorldAnimKey { time: 0.0, value: [0.0, 0.0, 0.0], kind: 1, tangents: [0.0; 6] },
                WorldAnimKey { time: 10.0, value: [0.0, 0.0, 0.0], kind: 1, tangents: [0.0; 6] },
            ],
        };
        let anims = WorldAnimations {
            clips: vec![clip],
            groups: vec![bfe_content::world_anim::WorldAnimGroup {
                name: "frig_loop".into(),
                default_on: true,
                stop_when_controlled: false,
                disable_hierarchies: false,
                bindings: vec![bfe_content::world_anim::WorldAnimBinding {
                    clip: "orbit".into(),
                    object: "orbit_frigate_a".into(),
                }],
            }],
            hierarchies: vec![],
        };

        // Create a 2m cube as a moving collision body at (0, 0, 0).
        let positions: Vec<f32> = vec![
            -1.0, -1.0, -1.0,  1.0, -1.0, -1.0,  1.0,  1.0, -1.0, -1.0,  1.0, -1.0,
            -1.0, -1.0,  1.0,  1.0, -1.0,  1.0,  1.0,  1.0,  1.0, -1.0,  1.0,  1.0,
        ];
        let indices: Vec<u32> = vec![
            0, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6,
            0, 4, 5, 0, 5, 1, 2, 6, 7, 2, 7, 3,
            0, 3, 7, 0, 7, 4, 1, 5, 6, 1, 6, 2,
        ];
        let mut coll = CollisionOwner::new();
        let pos_id = coll.positions(&positions);
        let bvh = build_bvh(Positions::F32(&positions), &indices);
        let tree = coll.tree(pos_id, &indices, &bvh);
        let empty_bvh = build_bvh(Positions::F32(&[]), &[]);
        let empty_pos = coll.positions(&[]);
        let empty_tree = coll.tree(empty_pos, &[], &empty_bvh);
        let world_id = coll.world_new(empty_tree, -1, -1) as usize;
        let body = coll.body_new(tree, 1) as u32;
        coll.state.worlds[world_id].order.push(body);
        coll.state.worlds[world_id].version += 1;

        let local_bounds = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        let id_mat = Matrix4::default().elements;
        let obj = AnimatedObject {
            name: "orbit_frigate_a".into(),
            prop_instance: 0,
            base_matrix: id_mat,
            base_inverse: id_mat,
            prev_matrix: id_mat,
            live_matrix: id_mat,
            live_inverse: id_mat,
            bodies: [Some((body, local_bounds)), None, None],
        };
        let mut runtime = SpaceWorldAnims::build(&anims, vec![obj]);
        let fixed_before = coll.state.fixed_version;

        // Step 300 ticks at 60 Hz (5.0 s -> moved +50 m along +X).
        for _ in 0..300 {
            runtime.step(1.0 / 60.0, &mut coll.state);
        }
        // Moving body updates must never trigger fixed broadphase rebuilds.
        assert_eq!(coll.state.fixed_version, fixed_before);
        let live = runtime.live_matrix_of("orbit_frigate_a").unwrap();
        assert!((live[12] - 50.0).abs() < 1e-9, "expected x=50, got {}", live[12]);
        // Drawing interpolation uses the previous and current simulation tick matrices.
        let (prev_m, live_m) = runtime.transform_pair_for_instance(0).unwrap();
        assert!((prev_m[12] - (299.0 / 6.0)).abs() < 1e-9);
        assert!((live_m[12] - 50.0).abs() < 1e-9);
        let mid_m = runtime.interpolated_matrix(0, 0.5).unwrap();
        assert!((mid_m[12] - 0.5 * (prev_m[12] + live_m[12])).abs() < 1e-9);

        // Ray at old origin misses; ray at x=50 hits the moving frigate.
        let candidates = [0u32];
        let enabled = [1u8];
        let ray_old = Ray { ox: 0.0, oy: 10.0, oz: 0.0, dx: 0.0, dy: -1.0, dz: 0.0 };
        let (code_old, _) = crate::collision::world_cast_ray_in(&mut coll.state, world_id, &ray_old, 20.0, 0.0, &candidates, &enabled);
        assert_eq!(code_old, crate::collision::MISS);

        let ray_new = Ray { ox: 50.0, oy: 10.0, oz: 0.0, dx: 0.0, dy: -1.0, dz: 0.0 };
        let (code_new, hit) = crate::collision::world_cast_ray_in(&mut coll.state, world_id, &ray_new, 20.0, 0.0, &candidates, &enabled);
        assert_eq!(code_new, body as i32);
        assert!((hit.py - 1.0).abs() < 1e-6);
    }

    #[test]
    fn world_animation_ticks_saturate_at_the_bound() {
        let key = |time: f64, x: f64| WorldAnimKey { time, value: [x, 0.0, 0.0], kind: 1, tangents: [0.0; 6] };
        let clip = WorldAnimClip {
            name: "slide".into(),
            duration: 10.0,
            looping: true,
            local_translation: true,
            positions: vec![key(0.0, 0.0), key(10.0, 10.0)],
            rotations: vec![key(0.0, 0.0), key(10.0, 0.0)],
        };
        let anims = WorldAnimations {
            clips: vec![clip],
            groups: vec![bfe_content::world_anim::WorldAnimGroup {
                name: "slider".into(),
                default_on: true,
                stop_when_controlled: false,
                disable_hierarchies: false,
                bindings: vec![bfe_content::world_anim::WorldAnimBinding { clip: "slide".into(), object: "slider_a".into() }],
            }],
            hierarchies: vec![],
        };
        let id_mat = Matrix4::default().elements;
        let obj = AnimatedObject {
            name: "slider_a".into(),
            prop_instance: 0,
            base_matrix: id_mat,
            base_inverse: id_mat,
            prev_matrix: id_mat,
            live_matrix: id_mat,
            live_inverse: id_mat,
            bodies: [None, None, None],
        };
        let mut runtime = SpaceWorldAnims::build(&anims, vec![obj]);
        runtime.sim_tick = u32::MAX - 2;
        runtime.groups[0].elapsed_ticks = u32::MAX - 2;
        let mut coll = CollisionOwner::new();
        for _ in 0..6 {
            runtime.step(1.0 / 60.0, &mut coll.state);
        }
        assert_eq!(runtime.sim_tick, u32::MAX);
        assert_eq!(runtime.groups[0].elapsed_ticks, u32::MAX);
        assert!(runtime.groups[0].time_s.is_finite());
    }
}
