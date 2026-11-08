//! Page targeting facts. Browser identities and collision I/O stay outside;
//! cone geometry, ranking and HUD projection share the native combat rules.
//!
//! Exact parse-order range can be supplied by extraction as ReticleRange.
//! Older extracts omit order; projectile lifetime is their fallback.
//! How far off a view (origin, unit dir) a unit lies: the smallest angle
//! (radians) between the view and any point of it, and the distance to its
//! nearest point. A vehicle is its collision box in its own frame, anything
//! else its bounding sphere. 0 when the view runs into it.
//! The slabs: does the view itself pass through the box?
//! Else the nearest direction lies on one of the twelve edges (a face's
//! interior has no nearer point unless the view meets that face).
//! The largest cosine between d and a point of the segment a..a+e (relative
//! to the view's origin): at an end, or where its derivative vanishes.
//! The reticle's target (its colour and the enemy health bar), in the
//! original's cone (widened by the target's bounding radius,
//! scored 16 d^2 + t). A hull counts only once part of it is inside the
//! cone (lockSpan), as for a launcher's lock (src/weapon-targeting.js):
//! a vehicle's bounding radius took it well off the reticle up close.
//!
//! A reticle without art (a seat's weapon with no ReticuleTexture): its four
//! ticks sit as far out as the weapon's spread reaches now, yaw across and
//! pitch up and down, by the same measure.

//!
//! Registry order and identity
//!
//! A row index identifies one observation in the current query, not a unit
//! across frames. The browser resolves that index back to the same resource
//! it supplied. Reused unit IDs therefore cannot borrow an earlier winner.
//! The generated frame and row strides keep this association independent
//! of the simulation store's entity slab or a renderer's draw order.
//!
//! Eligibility and lazy geometry
//!
//! findReticleTarget first rejects the viewer, dead actors, seated bodies
//! and the special non-target registry kind. Only then may the host observe
//! a body's aim point, sphere or hull bounds. The first native pass publishes
//! those eligibility bits for the whole block, before any shape is copied.
//! This preserves lazy pose queries without leaving target policy in JS.
//!
//! Forward depth is measured along a normalized view direction. The 300-row
//! cap counts forward candidates in range, including ones outside the cone.
//! Off-ray candidates therefore still spend the acquisition budget. Neither
//! the packet count nor the number of successful LOS rays replaces that cap.
//! Oversized foreign counts clamp to the reserved row bank before indexing.
//!
//! A hull uses its oriented local collision box for angular admission. Its
//! nearest visible edge can enter the cone while the hull centre remains
//! outside. Sphere candidates instead widen the cone by their body radius.
//! The shared targeting::span owns the slab and edge calculations, so page
//! reticles and native launcher acquisition cannot drift geometrically.
//!
//! Candidate precedence and sight
//!
//! A lockable candidate outranks every visible non-lockable fallback. Among
//! equal lockability, the smaller 16*d2+depth score wins; a tie keeps the first
//! registry row. LOS is asked only after these comparisons. An occluded
//! candidate never replaces the current winner, even when its score is best.
//! A continuation retains that winner while the separate collision owner
//! answers the ray. It does not sort candidates or issue speculative rays.
//!
//! Camera projection and content
//!
//! Projection reads the camera's existing inverse world matrix. It never
//! advances a camera or changes a scene transform. Points behind the view
//! publish a hidden flag instead of a projected reticle. Vertical FOV still
//! converts through the same degree constant used by the displayed camera.
//! Lock ticks use the horizontal FOV convention and the original 1024-wide
//! HUD scale; spread ticks independently supply their yaw and pitch angles.
//!
//! Utility rows have their own fixed frame and volume. A LOS callback may
//! inspect lockSpan or radii without overwriting its suspended target query.
//! None of these paths grows scratch storage while a frame is being drawn.

//!
//! Displayed muzzle samples
//!
//! firePoint's matrix is the attachment already drawn by the body owner. It
//! is intentionally independent of simulation aim and speculative shot timing.
//! Sampling it cannot advance weapon state, choose a barrel or apply recoil.
//! Those operations belong to combat and prediction, which publish their own
//! tick-space gun samples. Cosmetic flashes and charge glows use this view.
//!
//! Decomposition publishes position, quaternion and scale together. Treating
//! the matrix as a rigid pose would lose authored scaling and mirrored axes.
//! The shared three kernel keeps the same determinant and quaternion branch
//! order as the browser resource it replaces. A separate output record lets
//! the host copy these values to an effect without constructing a new pose.
//!
//! A utility request does not retain the muzzle's scene identity. The caller
//! resolves that resource at its existing presentation boundary and supplies
//! the current world matrix. A later hand or camera change therefore cannot
//! leave a cached mathematical pose attached to the wrong weapon model.

use super::targeting::{span, BoxVolume, Volume};
use crate::gen::{PageTargetFrame as F, PageTargetRow as R, VehicleMissilePolicy as P};
use three::{Matrix4, Quaternion, Vector3};
use v8math::max;
pub const TARGETS: usize = 4096;
fn vector(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
fn matrix(v: [f64; 16]) -> Matrix4 {
    let mut m = Matrix4::default();
    m.from_array(&v, 0);
    m
}
fn volume(r: &R) -> Volume {
    Volume {
        center: vector(r.center),
        radius: r.radius,
        hull: if r.hull {
            Some(BoxVolume {
                min: vector(r.min),
                max: vector(r.max),
                matrix: matrix(r.matrix),
            })
        } else {
            None
        },
    }
}
pub struct Query {
    pub utility: [f64; F::WORDS],
    pub volume: [f64; R::WORDS],
    pub name: [u8; 128],
    pub policy: [f64; P::WORDS],
    pub frame: [f64; F::WORDS],
    pub rows: Vec<f64>,
    at: usize,
    accepted: usize,
    best: Option<(usize, bool, f64, f64)>,
    pending: Option<(usize, bool, f64, f64)>,
}
impl Query {
    pub fn new() -> Self {
        Self {
            utility: [0.0; F::WORDS],
            volume: [0.0; R::WORDS],
            name: [0; 128],
            policy: [0.0; P::WORDS],
            frame: [0.0; F::WORDS],
            rows: vec![0.0; TARGETS * R::WORDS],
            at: 0,
            accepted: 0,
            best: None,
            pending: None,
        }
    }
    fn write(&mut self, f: &F) {
        f.put(&mut self.frame, &mut Vec::new());
    }
    /// vehicleMissileDefinition applies the tank launcher content correction
    /// without changing ammunition, damage or its authored firing schedule.
    pub fn tank_policy(&mut self, length: usize) {
        let name = self
            .name
            .get(..length)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("");
        let enabled = matches!(
            name,
            "rep_weap_hover_fightertank_missile"
                | "imp_weap_hover_fightertank_missile"
                | "all_weap_hover_combatspeeder_missile"
        );
        P {
            enabled,
            lock_on_range: 150.0,
            lock_on_angle: 6.0,
            lock_off_angle: 10.0,
            lock_time: 1.0,
            clear_lock_on_fire: 1.0,
            target_enemy: 1.0,
            target_friendly: 0.0,
            target_neutral: 0.0,
            target_person: 0.0,
            target_animal: 0.0,
            target_droid: 0.0,
            target_vehicle: 1.0,
            target_building: 0.0,
            relation_mask: 1,
            category_mask: 8,
            extreme_range: 150.0,
            turn_rate: 6.0,
            scale_time: 1.0,
        }
        .put(&mut self.policy, &mut Vec::new());
    }
    /// findReticleTarget runs in registry order. A continuation asks only for
    /// LOS of a candidate that can improve the winner, preserving ray order.
    pub fn begin(&mut self) {
        self.at = 0;
        self.accepted = 0;
        self.best = None;
        self.pending = None;
        let mut f = F::take(&self.frame, &[]);
        let mut dir = vector(f.direction);
        dir.normalize();
        f.direction = [dir.x, dir.y, dir.z];
        // Eligibility is a batch decision before browser geometry transport.
        // Dead and seated rows must not trigger lazy pose or bounds queries.
        for row in self
            .rows
            .chunks_exact_mut(R::WORDS)
            .take((f.count as usize).min(TARGETS))
        {
            let mut r = R::take(row, &[]);
            r.eligible = !r.viewer && r.alive && !r.seated && !(r.health <= 0.0) && r.kind != 10.0;
            r.put(row, &mut Vec::new());
        }
        f.best = -1;
        f.angle = f.yaw * std::f64::consts::PI / 180.0;
        self.write(&f);
    }
    pub fn resume(&mut self, visible: bool) -> i32 {
        if let Some(p) = self.pending.take() {
            if visible {
                self.best = Some(p);
            }
        }
        let mut f = F::take(&self.frame, &[]);
        let origin = vector(f.origin);
        let direction = vector(f.direction);
        if direction.length_sq() == 0.0 {
            return -1;
        }
        while self.at < (f.count as usize).min(TARGETS) {
            let i = self.at;
            self.at += 1;
            let r = R::take(&self.rows[i * R::WORDS..], &[]);
            if !r.eligible {
                continue;
            }
            let mut delta = vector(r.point);
            delta.sub(&origin);
            let t = delta.dot(&direction);
            if t < 0.0 || t > f.far {
                continue;
            }
            self.accepted += 1;
            if self.accepted > 300 {
                break;
            }
            let d2 = max(0.0, delta.length_sq() - t * t);
            if if r.hull {
                span(&origin, &direction, &volume(&r)).angle > f.angle
            } else {
                d2 > ((if r.has_box {
                    vector(r.half).length()
                } else {
                    r.box_radius
                }) + t * f.angle)
                    .powi(2)
            } {
                continue;
            }
            let bit = if r.relation < 0.0 {
                1
            } else if r.relation > 0.0 {
                4
            } else {
                2
            };
            let lockable = t <= f.lock_range
                && f.relation_mask & bit != 0
                && f.category_mask & (1u32.checked_shl(r.category).unwrap_or(0)) != 0;
            let score = 16.0 * d2 + t;
            if let Some((_, locked, best, _)) = self.best {
                if locked && !lockable || locked == lockable && best <= score {
                    continue;
                }
            }
            self.pending = Some((i, lockable, score, t));
            f.index = i as i32;
            f.point = r.point;
            self.write(&f);
            return i as i32;
        }
        if let Some((i, locked, score, t)) = self.best {
            let r = R::take(&self.rows[i * R::WORDS..], &[]);
            f.best = i as i32;
            f.lockable = locked;
            f.score = score;
            f.distance = t;
            f.point = r.point;
        } else {
            f.best = -1;
        }
        self.write(&f);
        -1
    }
    /// lockSpan keeps oriented box and sphere operation order shared with
    /// authoritative launcher acquisition, with no temporary Three vectors.
    pub fn geometry(&mut self) {
        let mut f = F::take(&self.utility, &[]);
        let r = R::take(&self.volume, &[]);
        let s = span(&vector(f.origin), &vector(f.direction), &volume(&r));
        f.angle = s.angle;
        f.distance = s.distance;
        f.put(&mut self.utility, &mut Vec::new());
    }
    /// updateHud/droidHud project a point 1024 m along the presented aim.
    /// Camera matrices are existing readbacks, never recomputed on this path.
    pub fn project(&mut self) {
        let mut f = F::take(&self.utility, &[]);
        let mut p = vector(f.origin);
        p.add_scaled_vector(&vector(f.direction), 1024.0);
        p.apply_matrix4(&matrix(f.matrix));
        f.value = if p.z >= 0.0 { 0.0 } else { 1.0 };
        let tan = v8math::tan((f.fov * (std::f64::consts::PI / 180.0)) / 2.0);
        f.y = 0.5 - 0.5 * (p.y / (-p.z * tan));
        f.x = 0.5 + 0.5 * (p.x / (-p.z * tan * f.aspect));
        f.put(&mut self.utility, &mut Vec::new());
    }
    /// PlayerCombat.firePoint reads the displayed muzzle, including authored
    /// non-uniform scale. Three resource identity stays outside this record.
    /// Its matrix may belong to a moving hand or a mounted weapon; neither
    /// the actor's physics origin nor its tick-space gun is a substitute.
    /// Keep the determinant sign and quaternion branch order of three r170.
    /// No skin, scene or collider state changes while sampling this point.
    pub fn attachment(&mut self) {
        let mut f = F::take(&self.utility, &[]);
        let mut position = Vector3::default();
        let mut rotation = Quaternion::default();
        let mut scale = Vector3::default();
        matrix(f.matrix).decompose(&mut position, &mut rotation, &mut scale);
        f.point = [position.x, position.y, position.z];
        f.rotation = [rotation.x, rotation.y, rotation.z, rotation.w];
        f.scale = [scale.x, scale.y, scale.z];
        f.put(&mut self.utility, &mut Vec::new());
    }
    /// lockTickRadii / spreadTickRadii share the original horizontal FOV.
    pub fn radii(&mut self) {
        let mut f = F::take(&self.utility, &[]);
        let vfov = f.fov * std::f64::consts::PI / 180.0;
        let hfov = 2.0 * v8math::atan(v8math::tan(vfov / 2.0) * f.aspect);
        let yaw = max(0.0, f.yaw) * std::f64::consts::PI / 180.0;
        let pitch = max(0.0, f.pitch) * std::f64::consts::PI / 180.0;
        f.x = 1024.0 * yaw / hfov;
        f.y = f.height * pitch / (hfov * 0.75);
        f.put(&mut self.utility, &mut Vec::new());
    }
}
/// damageDirection: the same view convention as the native HUD.
pub fn damage_direction(x: f64, z: f64, yaw: f64) -> f64 {
    v8math::atan2(
        -x * v8math::cos(yaw) + z * v8math::sin(yaw),
        x * v8math::sin(yaw) + z * v8math::cos(yaw),
    )
}
/// infantryFireBlocked: page preview uses the authoritative fire gate.
pub fn fire_blocked(flags: u32, stick: f64, lock: f64) -> bool {
    use crate::gen::PageFireFlag as Flag;
    const BLOCKED: u32 = (1 << Flag::Underwater as u8)
        | (1 << Flag::Air as u8)
        | (1 << Flag::Dive as u8)
        | (1 << Flag::Rolled as u8);
    flags & BLOCKED != 0 || lock > 0.0 || flags & (1 << Flag::Prone as u8) != 0 && stick > 0.1
}

#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn page_target_create() -> *mut Query {
        Box::into_raw(Box::new(Query::new()))
    }
    #[no_mangle]
    pub extern "C" fn page_target_capacity() -> u32 {
        TARGETS as u32
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_target_utility(p: *mut Query) -> *mut f64 {
        (*p).utility.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_target_volume(p: *mut Query) -> *mut f64 {
        (*p).volume.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_target_name(p: *mut Query) -> *mut u8 {
        (*p).name.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_target_policy(p: *mut Query, length: u32) -> *const f64 {
        (*p).tank_policy(length as usize);
        (*p).policy.as_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_target_frame(p: *mut Query) -> *mut f64 {
        (*p).frame.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_target_rows(p: *mut Query) -> *mut f64 {
        (*p).rows.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn page_target_run(p: *mut Query, op: u32, visible: u32) -> i32 {
        match op {
            0 => {
                (*p).begin();
                0
            }
            1 => (*p).resume(visible != 0),
            2 => {
                (*p).geometry();
                0
            }
            3 => {
                (*p).project();
                0
            }
            4 => {
                (*p).radii();
                0
            }
            5 => {
                (*p).attachment();
                0
            }
            _ => -1,
        }
    }
    #[no_mangle]
    pub extern "C" fn page_damage_direction(x: f64, z: f64, yaw: f64) -> f64 {
        damage_direction(x, z, yaw)
    }
    #[no_mangle]
    pub extern "C" fn page_fire_blocked(flags: u32, stick: f64, lock: f64) -> u32 {
        u32::from(fire_blocked(flags, stick, lock))
    }
    #[no_mangle]
    pub extern "C" fn page_relation(a: f64, b: f64) -> f64 {
        crate::seats::boarding::team_relation(a, b)
    }
    #[no_mangle]
    pub extern "C" fn page_radar_relation(
        viewer: f64,
        team: f64,
        relation: f64,
        locals: u32,
    ) -> u32 {
        if team == 0.0 {
            0
        } else if team != viewer && locals != 0 {
            3
        } else if relation > 0.0 {
            1
        } else if relation < 0.0 {
            2
        } else {
            0
        }
    }
    #[no_mangle]
    pub extern "C" fn page_reticle_range(reticle: f64, velocity: f64, life: f64, lock: f64) -> f64 {
        if reticle.is_finite() {
            max(0.0, reticle)
        } else {
            let v = velocity * life;
            if v != 0.0 && !v.is_nan() {
                v
            } else if lock != 0.0 && !lock.is_nan() {
                lock
            } else {
                0.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reticle_keeps_visible_fallback_when_better_lock_is_occluded() {
        let mut q = Query::new();
        let f = F {
            direction: [0.0, 0.0, 1.0],
            far: 100.0,
            lock_range: 100.0,
            relation_mask: 1,
            category_mask: 1,
            count: 2,
            ..Default::default()
        };
        q.write(&f);
        for (i, (z, relation)) in [(3.0, 1.0), (20.0, -1.0)].into_iter().enumerate() {
            let r = R {
                eligible: true,
                alive: true,
                health: 100.0,
                point: [0.0, 0.0, z],
                center: [0.0, 0.0, z],
                radius: 0.5,
                box_radius: 0.5,
                relation,
                ..Default::default()
            };
            r.put(&mut q.rows[i * R::WORDS..], &mut Vec::new());
        }
        q.begin();
        assert_eq!(q.resume(false), 0);
        assert_eq!(q.resume(true), 1);
        assert_eq!(q.resume(false), -1);
        let f = F::take(&q.frame, &[]);
        assert_eq!(f.best, 0);
        assert_eq!(f.distance, 3.0);
        assert!(!f.lockable);
    }
    #[test]
    fn oversized_target_block_stops_after_three_hundred_forward_candidates() {
        let mut q = Query::new();
        q.write(&F {
            direction: [0.0, 0.0, 1.0],
            far: 100.0,
            count: u32::MAX,
            ..Default::default()
        });
        for i in 0..TARGETS {
            R {
                eligible: true,
                alive: true,
                health: 100.0,
                point: [0.0, 0.0, 10.0],
                ..Default::default()
            }
            .put(&mut q.rows[i * R::WORDS..], &mut Vec::new());
        }
        let capacity = q.rows.capacity();
        q.begin();
        assert_eq!(q.resume(false), 0);
        assert_eq!(q.resume(true), -1);
        assert_eq!(F::take(&q.frame, &[]).best, 0);
        assert_eq!(q.accepted, 301);
        assert_eq!(q.rows.capacity(), capacity);
    }
}
