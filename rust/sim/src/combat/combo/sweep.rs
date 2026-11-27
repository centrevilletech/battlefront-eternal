//! Swept blade hit tests, attack-group hit deduplication, and melee block
//! resolution for Battlefront II combos.
//!
//! Between consecutive 60 Hz ticks, a moving blade edge sweeps a bilinear quad
//! triangulated into two triangles thickened by the attack's hit radius so a
//! fast swing cannot tunnel past a target. All arithmetic uses deterministic
//! IEEE-754 operations and `v8math`, with zero per-tick heap allocations.

use super::data::{AttackDef, DeflectDef, WeaponMeleeSpec};
use super::deflect::{angle_in_range_deg, relative_horizontal_angle_deg, DeflectDirection};
use super::CombatEnergy;
use crate::combat::impacts::HealthScale;
use crate::gen::{HitShape, ShapeType};
use crate::ordnance::math::{self as om, V3};

/// Maximum number of distinct targets stored in one attack's hit list.
pub const MAX_TARGETS_PER_ATTACK: usize = 8;

/// Maximum number of concurrent active per-attack hit lists tracked per hero
/// without heap allocation.
pub const MAX_ACTIVE_HIT_LISTS: usize = 16;

/// Time in seconds an attack's hit list remains active past its stored end time
/// (rebased across state changes) before retiring.
pub const HIT_LIST_LINGER_SECS: f64 = 0.30;

/// Maximum blade pose samples retained for one active melee attack interval.
pub const MAX_DAMAGE_SAMPLES: usize = 255;

/// Nominal spacing (seconds) between consecutive blade pose samples (30 Hz).
pub const DAMAGE_SAMPLE_BASE_STEP_SECS: f64 = 1.0 / 30.0;

/// Minimum step (seconds) below which adaptive refinement stops (300 Hz).
pub const DAMAGE_SAMPLE_MIN_STEP_SECS: f64 = 1.0 / 300.0;

/// Maximum squared base or tip travel between adjacent samples before refining.
pub const DAMAGE_SAMPLE_MAX_TRAVEL_SQ: f64 = 0.25;

/// Provisional until verified against the 2005 game: `DamageWidth` is treated
/// as total blade hit width, so the swept volume radius is half of it.
pub const PROVISIONAL_DAMAGE_WIDTH_RADIUS_SCALE: f64 = 0.5;

/// Blocking an enemy melee attack inside `DeflectAngle` cancels health and
/// shield damage completely (`0.0`).
pub const MELEE_BLOCK_DAMAGE_SCALE: f64 = 0.0;

/// Blocking an enemy melee strike charges only `DeflectDef::energy_cost`
/// (`0.0` extra cost).
pub const MELEE_BLOCK_EXTRA_ENERGY_COST: f64 = 0.0;

/// Blocking an enemy melee strike does not stagger or interrupt the attacker.
pub const MELEE_BLOCK_STAGGERS_ATTACKER: bool = false;

/// Provisional synthetic swing arc (radians, half-sweep from right to left)
/// used when skeletal firepoint poses are not supplied by the caller.
pub const PROVISIONAL_SYNTHETIC_ARC_HALF_RAD: f64 = 1.1;

/// Provisional synthetic swing origin height above feet (metres) used when
/// skeletal firepoint poses are not supplied by the caller.
pub const PROVISIONAL_SYNTHETIC_SWING_HEIGHT: f64 = 1.15;

/// World-space segment along one active blade edge at a single tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BladeSegment {
    pub base: V3,
    pub tip: V3,
}

impl BladeSegment {
    /// Build a blade segment from a sampled world-space firepoint origin and
    /// unit blade axis, extending by `length` (`DamageLength` / `DamageDistance`
    /// or the weapon edge's default length).
    pub fn from_firepoint(origin: V3, axis: V3, length: f64) -> Self {
        let dir = om::normalize(axis);
        Self {
            base: origin,
            tip: om::add_scaled(origin, dir, v8math::max(0.0, length)),
        }
    }

    /// Deterministic fallback blade pose for headless simulation or tests
    /// without a skeletal rig. Sweeps smoothly across the attacker's front arc
    /// as `window_progress` advances from `0.0` to `1.0`.
    pub fn synthetic_swing(
        origin: V3,
        aim_yaw: f64,
        aim_pitch: f64,
        window_progress: f64,
        edge: u8,
        length: f64,
    ) -> Self {
        let p = om::clamp(window_progress, 0.0, 1.0);
        let dir_sign = if (edge & 1) == 0 { 1.0 } else { -1.0 };
        let sweep_yaw = aim_yaw + dir_sign * (PROVISIONAL_SYNTHETIC_ARC_HALF_RAD * (2.0 * p - 1.0));
        let (sy, cy) = v8math::sin_cos(sweep_yaw);
        let (sp, cp) = v8math::sin_cos(aim_pitch);
        let axis = [sy * cp, sp, cy * cp];
        let base = [
            origin[0],
            origin[1] + PROVISIONAL_SYNTHETIC_SWING_HEIGHT,
            origin[2],
        ];
        Self::from_firepoint(base, axis, length)
    }
}

/// Swept blade quad from the previous tick to the current tick, thickened by
/// `radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BladeSweep {
    pub prev: BladeSegment,
    pub curr: BladeSegment,
    pub radius: f64,
}

impl BladeSweep {
    pub fn from_attack(prev: BladeSegment, curr: BladeSegment, attack: &AttackDef) -> Self {
        Self {
            prev,
            curr,
            radius: v8math::max(
                0.0,
                attack.damage_width * PROVISIONAL_DAMAGE_WIDTH_RADIUS_SCALE,
            ),
        }
    }

    /// Test the swept blade volume against a single [`HitShape`]. Returns the
    /// closest contact point on the blade quad on hit.
    pub fn hit_shape(&self, shape: &HitShape) -> Option<V3> {
        let a0 = self.prev.base;
        let b0 = self.prev.tip;
        let a1 = self.curr.base;
        let b1 = self.curr.tip;
        let r = self.radius;
        // Two triangles covering the bilinear swept quad (a0, b0, b1, a1).
        triangle_hits_shape(a0, b0, b1, r, shape)
            .or_else(|| triangle_hits_shape(a0, b1, a1, r, shape))
    }

    /// Test against all shapes of one unit, returning whichever shape index and
    /// contact point matches earliest.
    pub fn hit_shapes(&self, shapes: &[HitShape]) -> Option<(usize, V3)> {
        for (idx, shape) in shapes.iter().enumerate() {
            if let Some(point) = self.hit_shape(shape) {
                return Some((idx, point));
            }
        }
        None
    }
}

/// One sampled world-space blade firepoint pose `(time, position, direction)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DamageSample {
    pub time: f64,
    pub position: V3,
    pub direction: V3,
}

impl DamageSample {
    pub fn to_segment(&self, length: f64) -> BladeSegment {
        BladeSegment::from_firepoint(self.position, self.direction, length)
    }
}

/// Fixed-capacity sequence of blade pose samples across an active attack
/// interval (zero heap allocation, capped at [`MAX_DAMAGE_SAMPLES`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageSampleList {
    samples: [DamageSample; MAX_DAMAGE_SAMPLES],
    len: u16,
}

impl Default for DamageSampleList {
    fn default() -> Self {
        Self::new()
    }
}

impl DamageSampleList {
    pub const fn new() -> Self {
        Self {
            samples: [DamageSample {
                time: 0.0,
                position: [0.0; 3],
                direction: [0.0, 0.0, 1.0],
            }; MAX_DAMAGE_SAMPLES],
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[DamageSample] {
        let n = (self.len as usize).min(MAX_DAMAGE_SAMPLES);
        &self.samples[..n]
    }

    pub fn push(&mut self, sample: DamageSample) -> bool {
        let n = self.len as usize;
        if n >= MAX_DAMAGE_SAMPLES {
            return false;
        }
        self.samples[n] = sample;
        self.len = (n + 1) as u16;
        true
    }
}

/// Build an adaptive sequence of `(time, position, direction)` blade pose
/// samples across `[start_time, end_time]` starting at 1/30 s, refining
/// whenever base or tip squared travel exceeds 0.25 square units down to a
/// 1/300 s minimum step, clipping to `end_time`, and capping at 255 samples.
pub fn build_damage_samples<F>(
    start_time: f64,
    end_time: f64,
    damage_length: f64,
    mut sample_pose: F,
) -> DamageSampleList
where
    F: FnMut(f64) -> (V3, V3),
{
    let mut out = DamageSampleList::new();
    if !start_time.is_finite()
        || !end_time.is_finite()
        || !damage_length.is_finite()
        || end_time < start_time
    {
        return out;
    }
    let len = v8math::max(0.0, damage_length);
    let (p0, d0_raw) = sample_pose(start_time);
    let d0 = om::normalize(d0_raw);
    let mut prev = DamageSample {
        time: start_time,
        position: p0,
        direction: d0,
    };
    out.push(prev);
    if end_time <= start_time + 1e-9 {
        return out;
    }

    let mut t = start_time;
    while t < end_time - 1e-9 && out.len() < MAX_DAMAGE_SAMPLES {
        let remaining = end_time - t;
        let mut step = v8math::min(DAMAGE_SAMPLE_BASE_STEP_SECS, remaining);
        let prev_tip = om::add_scaled(prev.position, prev.direction, len);
        let chosen = loop {
            let next_t = v8math::min(end_time, t + step);
            let (pos, dir_raw) = sample_pose(next_t);
            let dir = om::normalize(dir_raw);
            let tip = om::add_scaled(pos, dir, len);
            let travel_sq = v8math::max(dist_sq(prev.position, pos), dist_sq(prev_tip, tip));
            if travel_sq <= DAMAGE_SAMPLE_MAX_TRAVEL_SQ || step * 0.5 < DAMAGE_SAMPLE_MIN_STEP_SECS
            {
                break DamageSample {
                    time: next_t,
                    position: pos,
                    direction: dir,
                };
            }
            step *= 0.5;
        };
        out.push(chosen);
        t = chosen.time;
        prev = chosen;
    }
    out
}

/// Names one target in an attack's hit list. A unit life and a collision body are different kinds,
/// so their numbers never collide; a life is named by the serial its observation was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetId {
    /// A soldier's life: the serial of the observation of that life.
    Life(u32),
    /// A collision body of a slab: its slot and the slab's generation.
    Body { generation: u32, slot: u32 },
}

impl Default for TargetId {
    fn default() -> Self {
        Self::Life(0)
    }
}

/// Per-attack (or shared nonzero low-20-bit `AttackId`) hit list storing up to
/// [`MAX_TARGETS_PER_ATTACK`] struck or blocked targets until
/// `expires_at + HIT_LIST_LINGER_SECS` on the current state's evaluation clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitListRecord {
    /// Unique per-record key `(state_id << 16) | attack_index`.
    pub attack_key: u32,
    /// Low 20 bits of the authored `AttackId` hash when shared, or `0` when
    /// unshared.
    pub shared_id: u32,
    /// Stored attack window end time on the current state's evaluation clock.
    pub expires_at: f64,
    targets: [TargetId; MAX_TARGETS_PER_ATTACK],
    len: u8,
}

impl Default for HitListRecord {
    fn default() -> Self {
        Self::new(0, 0, 0.0)
    }
}

impl HitListRecord {
    pub const fn new(attack_key: u32, shared_id: u32, expires_at: f64) -> Self {
        Self {
            attack_key,
            shared_id,
            expires_at,
            targets: [TargetId::Life(0); MAX_TARGETS_PER_ATTACK],
            len: 0,
        }
    }

    pub fn from_parts(
        attack_key: u32,
        shared_id: u32,
        expires_at: f64,
        slice: &[TargetId],
    ) -> Option<Self> {
        if slice.len() > MAX_TARGETS_PER_ATTACK {
            return None;
        }
        let mut rec = Self::new(attack_key, shared_id, expires_at);
        rec.targets[..slice.len()].copy_from_slice(slice);
        rec.len = slice.len() as u8;
        Some(rec)
    }

    pub fn matches(&self, attack_key: u32, shared_id: u32) -> bool {
        self.attack_key == attack_key || (shared_id != 0 && self.shared_id == shared_id)
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        (self.len as usize) >= MAX_TARGETS_PER_ATTACK
    }

    pub fn targets(&self) -> &[TargetId] {
        let n = (self.len as usize).min(MAX_TARGETS_PER_ATTACK);
        &self.targets[..n]
    }

    pub fn has_target(&self, target_id: TargetId) -> bool {
        self.targets().contains(&target_id)
    }

    pub fn record_target(&mut self, target_id: TargetId) -> bool {
        if self.has_target(target_id) {
            return false;
        }
        let n = (self.len as usize).min(MAX_TARGETS_PER_ATTACK);
        if n >= MAX_TARGETS_PER_ATTACK {
            return false;
        }
        self.targets[n] = target_id;
        self.len = (n + 1) as u8;
        true
    }
}

/// Fixed-capacity collection of active per-attack hit lists.
///
/// Each attack record (or group sharing a nonzero low-20-bit `AttackId` hash)
/// owns an independent 8-target list that survives state transitions via clock
/// rebasing and retires `HIT_LIST_LINGER_SECS` (0.30 s) after its stored end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitLists<const N: usize> {
    lists: [HitListRecord; N],
    len: u8,
}

impl<const N: usize> Default for HitLists<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> HitLists<N> {
    pub const fn new() -> Self {
        Self {
            lists: [HitListRecord::new(0, 0, 0.0); N],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn lists(&self) -> &[HitListRecord] {
        let n = (self.len as usize).min(N);
        &self.lists[..n]
    }

    pub fn from_lists(slice: &[HitListRecord]) -> Option<Self> {
        if slice.len() > N {
            return None;
        }
        let mut h = Self::new();
        h.lists[..slice.len()].copy_from_slice(slice);
        h.len = slice.len() as u8;
        Some(h)
    }

    pub(crate) fn has_duplicates(&self) -> bool {
        self.lists().iter().enumerate().any(|(index, list)| {
            self.lists()[..index]
                .iter()
                .any(|prior| prior.matches(list.attack_key, list.shared_id))
                || list
                    .targets()
                    .iter()
                    .enumerate()
                    .any(|(i, t)| list.targets()[..i].contains(t))
        })
    }

    pub fn find_index(&self, attack_key: u32, shared_id: u32) -> Option<usize> {
        self.lists()
            .iter()
            .position(|r| r.matches(attack_key, shared_id))
    }

    /// Ensure an active hit list exists for `(attack_key, shared_id)` with at
    /// least `expires_at` on the current evaluation clock.
    pub fn ensure_list(
        &mut self,
        attack_key: u32,
        shared_id: u32,
        expires_at: f64,
    ) -> Option<usize> {
        if let Some(idx) = self.find_index(attack_key, shared_id) {
            if expires_at > self.lists[idx].expires_at {
                self.lists[idx].expires_at = expires_at;
            }
            return Some(idx);
        }
        let n = (self.len as usize).min(N);
        if n >= N {
            return None;
        }
        self.lists[n] = HitListRecord::new(attack_key, shared_id, expires_at);
        self.len = (n + 1) as u8;
        Some(n)
    }

    pub fn has_hit(&self, attack_key: u32, shared_id: u32, target_id: TargetId) -> bool {
        self.find_index(attack_key, shared_id)
            .is_some_and(|idx| self.lists[idx].has_target(target_id))
    }

    /// Record a struck or blocked target in the attack's own 8-target hit list
    /// without affecting any unrelated attack list or extending an already
    /// scheduled expiry time.
    pub fn record(
        &mut self,
        attack_key: u32,
        shared_id: u32,
        expires_at: f64,
        target_id: TargetId,
    ) -> bool {
        let idx = if let Some(idx) = self.find_index(attack_key, shared_id) {
            idx
        } else {
            let Some(idx) = self.ensure_list(attack_key, shared_id, expires_at) else {
                return false;
            };
            idx
        };
        self.lists[idx].record_target(target_id)
    }

    /// Rebase all active hit-list expiry times on a state transition where the
    /// evaluation clock shifts by `clock_delta = old_clock - new_clock`.
    /// Any infinite expiry (from an indefinite held-final-frame attack) is
    /// capped at `old_clock` before rebasing so it retires 0.30 s after exit.
    pub fn rebase(&mut self, clock_delta: f64, old_clock: f64) {
        let n = (self.len as usize).min(N);
        for rec in &mut self.lists[..n] {
            if !rec.expires_at.is_finite() {
                rec.expires_at = old_clock;
            }
            rec.expires_at -= clock_delta;
        }
    }

    /// Retire hit lists whose stored end time plus [`HIT_LIST_LINGER_SECS`] has
    /// been reached by `current_clock`.
    pub fn retire(&mut self, current_clock: f64) {
        let n = (self.len as usize).min(N);
        let mut write = 0usize;
        for read in 0..n {
            let rec = self.lists[read];
            if !rec.expires_at.is_finite() || current_clock < rec.expires_at + HIT_LIST_LINGER_SECS
            {
                self.lists[write] = rec;
                write += 1;
            }
        }
        self.len = write as u8;
    }
}

/// Active windows of a carried melee weapon.
pub type HitHistory = HitLists<MAX_ACTIVE_HIT_LISTS>;
/// One independent target list for each half of a thrown blade's flight.
pub type ThrowHitHistory = HitLists<2>;

/// Target facts inspected by [`resolve_melee_hit`].
pub struct MeleeTarget<'a> {
    pub target_id: TargetId,
    pub position: V3,
    pub aim_yaw: f64,
    pub health_scale: HealthScale,
    pub shapes: &'a [HitShape],
    pub deflect: Option<DeflectDef>,
}

/// Result of testing one active attack sweep against one candidate target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeleeHitOutcome {
    Missed,
    AlreadyHit,
    Blocked {
        hit_point: V3,
        direction: DeflectDirection,
        reaction: Option<u16>,
        damage_taken: f64,
        shield_damage: f64,
        energy_drained: f64,
        stagger_attacker: bool,
    },
    Hit {
        hit_point: V3,
        damage: f64,
        shield_damage: f64,
        push_impulse: V3,
    },
}

/// Resolve one swept blade attack against a single target, enforcing single-hit
/// deduplication per attack record or shared low-20-bit `AttackId` group
/// (including recording blocked contacts) and directional blocking when the
/// defender is in an active `Deflect` state.
#[allow(clippy::too_many_arguments)]
pub fn resolve_melee_hit<const N: usize>(
    attacker_pos: V3,
    attack: &AttackDef,
    weapon: &WeaponMeleeSpec,
    sweep: &BladeSweep,
    history: &mut HitLists<N>,
    target: &MeleeTarget<'_>,
    defender_energy: Option<&mut dyn CombatEnergy>,
) -> MeleeHitOutcome {
    if history.has_hit(attack.attack_key, attack.shared_id, target.target_id) {
        return MeleeHitOutcome::AlreadyHit;
    }
    let Some((_, hit_point)) = sweep.hit_shapes(target.shapes) else {
        return MeleeHitOutcome::Missed;
    };
    apply_contact(
        attacker_pos,
        attack,
        weapon,
        hit_point,
        history,
        target,
        defender_energy,
    )
}

pub(crate) fn apply_contact<const N: usize>(
    attacker_pos: V3,
    attack: &AttackDef,
    weapon: &WeaponMeleeSpec,
    hit_point: V3,
    history: &mut HitLists<N>,
    target: &MeleeTarget<'_>,
    defender_energy: Option<&mut dyn CombatEnergy>,
) -> MeleeHitOutcome {
    if !history.record(attack.attack_key, attack.shared_id, 0.0, target.target_id) {
        return MeleeHitOutcome::Missed;
    }

    let typed_damage = attack.damage * weapon.damage_scales[target.health_scale as usize];
    let shield_damage = attack.damage * weapon.shield_scale;

    if let Some(deflect) = target.deflect {
        let toward_attacker = om::sub(attacker_pos, target.position);
        let rel_angle = relative_horizontal_angle_deg(target.aim_yaw, toward_attacker);
        if angle_in_range_deg(rel_angle, deflect.min_angle_deg, deflect.max_angle_deg) {
            let cost = deflect.energy_cost + MELEE_BLOCK_EXTRA_ENERGY_COST;
            let can_pay = match defender_energy {
                Some(e) => e.pay(cost),
                None => cost <= 0.0,
            };
            if can_pay {
                let (direction, reaction) = deflect.reaction(rel_angle);
                return MeleeHitOutcome::Blocked {
                    hit_point,
                    direction,
                    reaction,
                    damage_taken: typed_damage * MELEE_BLOCK_DAMAGE_SCALE,
                    shield_damage: shield_damage * MELEE_BLOCK_DAMAGE_SCALE,
                    energy_drained: cost,
                    stagger_attacker: MELEE_BLOCK_STAGGERS_ATTACKER,
                };
            }
        }
    }

    let mut away = [
        target.position[0] - attacker_pos[0],
        0.0,
        target.position[2] - attacker_pos[2],
    ];
    let len = om::length(away);
    if len > 1e-6 {
        away = om::divide(away, len);
    } else {
        away = [0.0, 0.0, 0.0];
    }
    let push_impulse = om::scale(away, attack.push);

    MeleeHitOutcome::Hit {
        hit_point,
        damage: typed_damage,
        shield_damage,
        push_impulse,
    }
}

/// Resolve an adaptive sequence of blade pose samples against one target by
/// sweeping adjacent sample pairs in chronological order and stopping at the
/// first contact or already-recorded hit.
#[allow(clippy::too_many_arguments)]
pub fn resolve_melee_hit_samples<const N: usize>(
    attacker_pos: V3,
    attack: &AttackDef,
    weapon: &WeaponMeleeSpec,
    samples: &[DamageSample],
    history: &mut HitLists<N>,
    target: &MeleeTarget<'_>,
    defender_energy: Option<&mut dyn CombatEnergy>,
) -> MeleeHitOutcome {
    if samples.is_empty() {
        return MeleeHitOutcome::Missed;
    }
    if history.has_hit(attack.attack_key, attack.shared_id, target.target_id) {
        return MeleeHitOutcome::AlreadyHit;
    }
    if samples.len() == 1 {
        let seg = samples[0].to_segment(attack.damage_length);
        let sweep = BladeSweep::from_attack(seg, seg, attack);
        return resolve_melee_hit(
            attacker_pos,
            attack,
            weapon,
            &sweep,
            history,
            target,
            defender_energy,
        );
    }
    for pair in samples.windows(2) {
        let sweep = BladeSweep::from_attack(
            pair[0].to_segment(attack.damage_length),
            pair[1].to_segment(attack.damage_length),
            attack,
        );
        if let Some((_, point)) = sweep.hit_shapes(target.shapes) {
            return apply_contact(
                attacker_pos,
                attack,
                weapon,
                point,
                history,
                target,
                defender_energy,
            );
        }
    }
    MeleeHitOutcome::Missed
}

// ---- Exact geometric intersection primitives (no trig except yaw rotation for OBB)

fn triangle_hits_shape(a: V3, b: V3, c: V3, blade_radius: f64, shape: &HitShape) -> Option<V3> {
    match shape.r#type {
        ShapeType::Sphere => {
            let p = closest_point_on_triangle(shape.center, a, b, c);
            let limit = shape.radius + blade_radius;
            (dist_sq(p, shape.center) <= limit * limit).then_some(p)
        }
        ShapeType::Capsule => {
            let hv = shape.half_vector();
            if !hv.x.is_finite() || !hv.y.is_finite() || !hv.z.is_finite() {
                return None;
            }
            let half = [hv.x, hv.y, hv.z];
            let s0 = om::sub(shape.center, half);
            let s1 = om::add(shape.center, half);
            let (_on_seg, on_tri, d_sq) = closest_segment_triangle(s0, s1, a, b, c);
            let limit = shape.radius + blade_radius;
            (d_sq <= limit * limit).then_some(on_tri)
        }
        ShapeType::Cylinder => {
            let half = shape.half_number();
            if !half.is_finite() {
                return None;
            }
            cylinder_triangle_hit(shape.center, shape.radius, half, a, b, c, blade_radius)
        }
        ShapeType::Box => {
            let hv = shape.half_vector();
            if !hv.x.is_finite() || !hv.y.is_finite() || !hv.z.is_finite() {
                return None;
            }
            box_triangle_hit(
                shape.center,
                [hv.x, hv.y, hv.z],
                shape.yaw,
                a,
                b,
                c,
                blade_radius,
            )
        }
    }
}

#[inline]
fn dist_sq(a: V3, b: V3) -> f64 {
    om::length_sq(om::sub(a, b))
}

/// Closest point on triangle `(a, b, c)` to point `p` (Ericson Voronoi regions).
pub(crate) fn closest_point_on_triangle(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = om::sub(b, a);
    let ac = om::sub(c, a);
    let ap = om::sub(p, a);
    let d1 = om::dot(ab, ap);
    let d2 = om::dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = om::sub(p, b);
    let d3 = om::dot(ab, bp);
    let d4 = om::dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let denom = d1 - d3;
        let v = if denom.abs() > 1e-12 { d1 / denom } else { 0.0 };
        return om::add_scaled(a, ab, v);
    }

    let cp = om::sub(p, c);
    let d5 = om::dot(ab, cp);
    let d6 = om::dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let denom = d2 - d6;
        let w = if denom.abs() > 1e-12 { d2 / denom } else { 0.0 };
        return om::add_scaled(a, ac, w);
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let denom = (d4 - d3) + (d5 - d6);
        let w = if denom.abs() > 1e-12 {
            (d4 - d3) / denom
        } else {
            0.0
        };
        return om::add_scaled(b, om::sub(c, b), w);
    }

    let denom_raw = va + vb + vc;
    if denom_raw.abs() <= 1e-12 {
        return a;
    }
    let inv = 1.0 / denom_raw;
    let v = vb * inv;
    let w = vc * inv;
    om::add_scaled(om::add_scaled(a, ab, v), ac, w)
}

/// Closest points `(c1, c2)` between segments `(p1, q1)` and `(p2, q2)`.
fn closest_points_segments(p1: V3, q1: V3, p2: V3, q2: V3) -> (V3, V3) {
    let d1 = om::sub(q1, p1);
    let d2 = om::sub(q2, p2);
    let r = om::sub(p1, p2);
    let a = om::dot(d1, d1);
    let e = om::dot(d2, d2);
    let f = om::dot(d2, r);

    let (s, t) = if a <= 1e-12 && e <= 1e-12 {
        (0.0, 0.0)
    } else if a <= 1e-12 {
        (0.0, om::clamp(f / e, 0.0, 1.0))
    } else {
        let c = om::dot(d1, r);
        if e <= 1e-12 {
            (om::clamp(-c / a, 0.0, 1.0), 0.0)
        } else {
            let b = om::dot(d1, d2);
            let denom = a * e - b * b;
            let mut s = if denom.abs() > 1e-12 {
                om::clamp((b * f - c * e) / denom, 0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = om::clamp(-c / a, 0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = om::clamp((b - c) / a, 0.0, 1.0);
            }
            (s, t)
        }
    };
    (om::add_scaled(p1, d1, s), om::add_scaled(p2, d2, t))
}

/// Exact segment-triangle intersection via Moller-Trumbore.
fn segment_intersects_triangle(s0: V3, s1: V3, a: V3, b: V3, c: V3) -> Option<V3> {
    let dir = om::sub(s1, s0);
    let e1 = om::sub(b, a);
    let e2 = om::sub(c, a);
    let h = om::cross(dir, e2);
    let det = om::dot(e1, h);
    if det.abs() <= 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = om::sub(s0, a);
    let u = inv * om::dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = om::cross(s, e1);
    let v = inv * om::dot(dir, q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = inv * om::dot(e2, q);
    if (0.0..=1.0).contains(&t) {
        Some(om::add_scaled(s0, dir, t))
    } else {
        None
    }
}

/// Exact minimum squared distance between segment `(s0, s1)` and triangle `(a, b, c)`.
fn closest_segment_triangle(s0: V3, s1: V3, a: V3, b: V3, c: V3) -> (V3, V3, f64) {
    if let Some(hit) = segment_intersects_triangle(s0, s1, a, b, c) {
        return (hit, hit, 0.0);
    }
    let p0 = closest_point_on_triangle(s0, a, b, c);
    let mut best = (s0, p0, dist_sq(s0, p0));

    let p1 = closest_point_on_triangle(s1, a, b, c);
    let d1 = dist_sq(s1, p1);
    if d1 < best.2 {
        best = (s1, p1, d1);
    }

    for (e0, e1) in [(a, b), (b, c), (c, a)] {
        let (cs, ct) = closest_points_segments(s0, s1, e0, e1);
        let d = dist_sq(cs, ct);
        if d < best.2 {
            best = (cs, ct, d);
        }
    }
    best
}

/// Exact distance squared from a cylinder-local point to an upright finite cylinder.
fn point_to_cylinder_local_dist_sq(p: V3, radius: f64, half_height: f64) -> f64 {
    let horiz = v8math::hypot(p[0], p[2]);
    let dr = v8math::max(0.0, horiz - radius);
    let dy = v8math::max(0.0, v8math::abs(p[1]) - half_height);
    dr * dr + dy * dy
}

/// Exact segment vs thickened upright finite cylinder in cylinder-local coordinates.
fn segment_cylinder_local_hit(
    p0: V3,
    p1: V3,
    radius: f64,
    half_height: f64,
    blade_radius: f64,
) -> Option<V3> {
    let br_sq = blade_radius * blade_radius;
    if point_to_cylinder_local_dist_sq(p0, radius, half_height) <= br_sq + 1e-12 {
        return Some(p0);
    }
    if point_to_cylinder_local_dist_sq(p1, radius, half_height) <= br_sq + 1e-12 {
        return Some(p1);
    }

    // Clip t in [0, 1] to vertical slab |y(t)| <= half_height and minimize
    // radial x(t)^2 + z(t)^2 over the clipped interval.
    let dy = p1[1] - p0[1];
    let slab = if dy.abs() <= 1e-12 {
        (v8math::abs(p0[1]) <= half_height).then_some((0.0, 1.0))
    } else {
        let mut t1 = (-half_height - p0[1]) / dy;
        let mut t2 = (half_height - p0[1]) / dy;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        let t_lo = v8math::max(0.0, t1);
        let t_hi = v8math::min(1.0, t2);
        (t_lo <= t_hi).then_some((t_lo, t_hi))
    };
    if let Some((t_lo, t_hi)) = slab {
        let dx = p1[0] - p0[0];
        let dz = p1[2] - p0[2];
        let a = dx * dx + dz * dz;
        let t_star = if a > 1e-12 {
            om::clamp(-(p0[0] * dx + p0[2] * dz) / a, t_lo, t_hi)
        } else {
            t_lo
        };
        let pt = om::lerp(p0, p1, t_star);
        let r_lim = radius + blade_radius;
        if pt[0] * pt[0] + pt[2] * pt[2] <= r_lim * r_lim + 1e-12 {
            return Some(pt);
        }
    }

    // For nonzero blade_radius, minimize the convex squared distance over [0, 1]
    // with deterministic golden-section search to cover cap-rim grazes.
    if blade_radius > 0.0 {
        const INV_PHI: f64 = 0.618_033_988_749_894_9;
        let mut lo = 0.0;
        let mut hi = 1.0;
        let mut mid_lo = hi - INV_PHI * (hi - lo);
        let mut mid_hi = lo + INV_PHI * (hi - lo);
        let mut f1 = point_to_cylinder_local_dist_sq(om::lerp(p0, p1, mid_lo), radius, half_height);
        let mut f2 = point_to_cylinder_local_dist_sq(om::lerp(p0, p1, mid_hi), radius, half_height);
        for _ in 0..36 {
            if f1 <= f2 {
                hi = mid_hi;
                mid_hi = mid_lo;
                f2 = f1;
                mid_lo = hi - INV_PHI * (hi - lo);
                f1 = point_to_cylinder_local_dist_sq(om::lerp(p0, p1, mid_lo), radius, half_height);
            } else {
                lo = mid_lo;
                mid_lo = mid_hi;
                f1 = f2;
                mid_hi = lo + INV_PHI * (hi - lo);
                f2 = point_to_cylinder_local_dist_sq(om::lerp(p0, p1, mid_hi), radius, half_height);
            }
        }
        let pt = om::lerp(p0, p1, 0.5 * (lo + hi));
        if point_to_cylinder_local_dist_sq(pt, radius, half_height) <= br_sq + 1e-12 {
            return Some(pt);
        }
    }
    None
}

/// Clip a small convex polygon (at most 5 vertices) to one half-space `y >= min_y`
/// or `y <= max_y` on the stack without allocation.
fn clip_poly_y(input: &[V3], out: &mut [V3; 6], plane_y: f64, keep_above: bool) -> usize {
    let n = input.len();
    if n == 0 {
        return 0;
    }
    let inside = |p: V3| {
        if keep_above {
            p[1] >= plane_y - 1e-12
        } else {
            p[1] <= plane_y + 1e-12
        }
    };
    let mut len = 0usize;
    let mut prev = input[n - 1];
    let mut prev_in = inside(prev);
    for &curr in input {
        let curr_in = inside(curr);
        if curr_in != prev_in {
            let dy = curr[1] - prev[1];
            let t = if dy.abs() > 1e-12 {
                om::clamp((plane_y - prev[1]) / dy, 0.0, 1.0)
            } else {
                0.0
            };
            if len < out.len() {
                out[len] = om::lerp(prev, curr, t);
                len += 1;
            }
        }
        if curr_in && len < out.len() {
            out[len] = curr;
            len += 1;
        }
        prev = curr;
        prev_in = curr_in;
    }
    len
}

fn cylinder_triangle_hit(
    center: V3,
    radius: f64,
    half_height: f64,
    a: V3,
    b: V3,
    c: V3,
    blade_radius: f64,
) -> Option<V3> {
    let r = v8math::max(0.0, radius);
    let hh = v8math::max(0.0, half_height);
    let br = v8math::max(0.0, blade_radius);
    let br_sq = br * br;

    let la = om::sub(a, center);
    let lb = om::sub(b, center);
    let lc = om::sub(c, center);
    let to_world = |l: V3| om::add(l, center);

    let min_y = v8math::min(la[1], v8math::min(lb[1], lc[1]));
    let max_y = v8math::max(la[1], v8math::max(lb[1], lc[1]));
    if max_y < -(hh + br) - 1e-12 || min_y > (hh + br) + 1e-12 {
        return None;
    }

    // 1. Cylinder axis segment vs triangle (covers interior crossings around axis).
    let (_, on_tri, _) = closest_segment_triangle([0.0, -hh, 0.0], [0.0, hh, 0.0], la, lb, lc);
    if point_to_cylinder_local_dist_sq(on_tri, r, hh) <= br_sq + 1e-12 {
        return Some(to_world(on_tri));
    }

    // 2. Triangle edges vs finite cylinder (covers slab entries and cap-rim grazes).
    for (e0, e1) in [(la, lb), (lb, lc), (lc, la)] {
        if let Some(hit) = segment_cylinder_local_hit(e0, e1, r, hh, br) {
            return Some(to_world(hit));
        }
    }

    // 3. Clip triangle to the vertical slab |y| <= hh so cap-disk slices whose
    //    endpoints lie outside the slab are tested exactly.
    let mut buf1 = [[0.0; 3]; 6];
    let mut buf2 = [[0.0; 3]; 6];
    let n1 = clip_poly_y(&[la, lb, lc], &mut buf1, -hh, true);
    let n2 = clip_poly_y(&buf1[..n1], &mut buf2, hh, false);
    if n2 >= 2 {
        for i in 0..n2 {
            let p0 = buf2[i];
            let p1 = buf2[(i + 1) % n2];
            if let Some(hit) = segment_cylinder_local_hit(p0, p1, r, hh, br) {
                return Some(to_world(hit));
            }
        }
    }

    // 4. Interior triangle point vs cylinder support generators/rims along +/-normal,
    //    plus horizontal cap centers when the triangle is parallel to the caps.
    let normal = om::cross(om::sub(lb, la), om::sub(lc, la));
    let nh = v8math::hypot(normal[0], normal[2]);
    if nh > 1e-12 {
        let ux = r * (normal[0] / nh);
        let uz = r * (normal[2] / nh);
        for sign in [1.0, -1.0] {
            let gx = sign * ux;
            let gz = sign * uz;
            let (_, pt, _) = closest_segment_triangle([gx, -hh, gz], [gx, hh, gz], la, lb, lc);
            if point_to_cylinder_local_dist_sq(pt, r, hh) <= br_sq + 1e-12 {
                return Some(to_world(pt));
            }
        }
    }
    for cap_y in [-hh, hh] {
        let pt = closest_point_on_triangle([0.0, cap_y, 0.0], la, lb, lc);
        if point_to_cylinder_local_dist_sq(pt, r, hh) <= br_sq + 1e-12 {
            return Some(to_world(pt));
        }
    }
    None
}

fn point_to_aabb_dist_sq(p: V3, half: V3) -> f64 {
    let mut sum = 0.0;
    for i in 0..3 {
        let over = v8math::max(0.0, v8math::abs(p[i]) - half[i]);
        sum += over * over;
    }
    sum
}

/// Yaw-rotated box (`units/shapes.rs` frame: local Z along `(sin yaw, 0, cos yaw)`,
/// local X along `(cos yaw, 0, -sin yaw)`) vs thickened triangle.
fn box_triangle_hit(
    center: V3,
    half: V3,
    yaw: f64,
    a: V3,
    b: V3,
    c: V3,
    blade_radius: f64,
) -> Option<V3> {
    let (sin, cos) = v8math::sin_cos(yaw);
    let to_local = |p: V3| -> V3 {
        let dx = p[0] - center[0];
        let dy = p[1] - center[1];
        let dz = p[2] - center[2];
        [dx * cos - dz * sin, dy, dx * sin + dz * cos]
    };
    let to_world = |l: V3| -> V3 {
        [
            center[0] + l[0] * cos + l[2] * sin,
            center[1] + l[1],
            center[2] - l[0] * sin + l[2] * cos,
        ]
    };

    let la = to_local(a);
    let lb = to_local(b);
    let lc = to_local(c);
    let r_sq = blade_radius * blade_radius;

    // 1. Closest point on triangle to box origin (handles interior and face hits).
    let closest_to_origin = closest_point_on_triangle([0.0, 0.0, 0.0], la, lb, lc);
    if point_to_aabb_dist_sq(closest_to_origin, half) <= r_sq {
        return Some(to_world(closest_to_origin));
    }

    // 2. Test each of the 8 box corners against the triangle and clamp back.
    for mask in 0u8..8 {
        let corner = [
            if mask & 1 != 0 { half[0] } else { -half[0] },
            if mask & 2 != 0 { half[1] } else { -half[1] },
            if mask & 4 != 0 { half[2] } else { -half[2] },
        ];
        let pt = closest_point_on_triangle(corner, la, lb, lc);
        if point_to_aabb_dist_sq(pt, half) <= r_sq {
            return Some(to_world(pt));
        }
    }

    // 3. Test the 3 triangle edges against the expanded box slabs.
    let expanded = [
        half[0] + blade_radius,
        half[1] + blade_radius,
        half[2] + blade_radius,
    ];
    for (e0, e1) in [(la, lb), (lb, lc), (lc, la)] {
        if let Some(hit) = segment_aabb_point(e0, e1, expanded) {
            if point_to_aabb_dist_sq(hit, half) <= r_sq {
                return Some(to_world(hit));
            }
        }
    }
    None
}

fn segment_aabb_point(p0: V3, p1: V3, half: V3) -> Option<V3> {
    let d = om::sub(p1, p0);
    let mut tmin = 0.0;
    let mut tmax = 1.0;
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if p0[i] < -half[i] || p0[i] > half[i] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d[i];
        let mut t1 = (-half[i] - p0[i]) * inv;
        let mut t2 = (half[i] - p0[i]) * inv;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = v8math::max(tmin, t1);
        tmax = v8math::min(tmax, t2);
        if tmin > tmax {
            return None;
        }
    }
    Some(om::add_scaled(p0, d, tmin))
}
