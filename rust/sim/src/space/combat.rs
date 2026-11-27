//! 2005 (`GameFamily::Swbf2`) space weapons, lock-on, ordnance pool, guided
//! rockets, infantry boarding timebombs, spread/barrage/auto-aim, and
//! armor/shield damage scaling.
//!
//! Fixed 60 Hz deterministic arithmetic (`v8math` only), pre-allocated fixed
//! projectile pool (zero per-tick allocations on hot paths).

use bfe_wide::Wide;
use super::SpaceOwner;
use super::digest::Fnv;
use super::flight::{
    quat_from_axis_angle, rotate_vec, SpaceFighter, SpaceFlightProfile, SpaceLockState,
    SpaceLockTargetKind, SpaceOrdnanceKind, SpaceWeaponSpec, PROVISIONAL_DEFAULT_MUZZLE_PAD,
    PROVISIONAL_MIN_SHOT_INTERVAL,
};
use crate::collision::{CollisionOwner, Ray, MISS};
use three::{Quaternion, Vector3};

const PI: f64 = std::f64::consts::PI;

/// Pre-allocated fixed capacity of the space projectile pool (zero runtime
/// allocations during battle ticks).
pub const MAX_SPACE_PROJECTILES: usize = 512;

/// Provisional line-of-sight grace period before an acquired missile lock
/// drops when terrain or a capital ship passes between seeker and target (s).
pub const PROVISIONAL_LOCK_LOS_GRACE: f64 = 0.3;
pub const PROVISIONAL_MIN_MUZZLE_DISTANCE: f64 = 2.0;
pub const PROVISIONAL_MIN_BOMB_FUSE: f64 = 0.1;

/// Provisional close-range threshold below which lock-on skips LOS raycast (m).
pub const PROVISIONAL_LOCK_LOS_NEAR_M: f64 = 2.0;

/// Provisional shortening margin before target center during lock-on LOS ray (m).
pub const PROVISIONAL_LOCK_LOS_SHORT_M: f64 = 1.0;

/// Provisional hit-distance tolerance near target center during lock-on LOS ray (m).
pub const PROVISIONAL_LOCK_LOS_TARGET_TOL_M: f64 = 3.0;

/// Provisional warm-up decay rate when a spin-up weapon trigger is released (1/s).
pub const PROVISIONAL_WARMUP_DECAY_RATE: f64 = 2.0;

/// Provisional pilot steering turn rate for in-flight guided rockets (rad/s).
pub const PROVISIONAL_GUIDED_TURN_RATE: f64 = 2.2;

/// Provisional speed acceleration/deceleration rate for boosted guided rockets (m/s^2).
pub const PROVISIONAL_GUIDED_ACCEL: f64 = 180.0;

/// Provisional fallback bounding-sphere radius for logic-only fighters lacking
/// a collision kernel body (metres).
pub const PROVISIONAL_FALLBACK_FIGHTER_HIT_RADIUS: f64 = 3.5;

/// Deterministic zero-allocation 64-bit PRNG for weapon spread and barrage
/// counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpaceRng {
    state: Wide,
}

impl SpaceRng {
    pub const fn new(seed: Wide) -> Self {
        Self {
            state: if seed.is_zero() {
                Wide::new(0x9e37_79b9, 0x7f4a_7c15)
            } else {
                seed
            },
        }
    }

    #[inline]
    pub fn state(&self) -> Wide {
        self.state
    }

    #[inline]
    pub fn next_word(&mut self) -> Wide {
        let mut x = self.state;
        x = x.xor(x.shr(12));
        x = x.xor(x.shl(25));
        x = x.xor(x.shr(27));
        self.state = x;
        x.wrapping_mul(Wide::new(0x2545_f491, 0x4f6c_dd1d))
    }

    /// Uniform float in `[0.0, 1.0)`.
    #[inline]
    pub fn next_unit(&mut self) -> f64 {
        self.next_word().shr(11).to_f64() * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Inclusive integer draw in `[lo, hi]`.
    #[inline]
    pub fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        let draw = self.next_word();
        match (hi - lo).checked_add(1) {
            Some(span) => lo + draw.rem(span),
            // The whole 32-bit range: the draw modulo 2^32 is its low word.
            None => lo.wrapping_add(draw.low()),
        }
    }
}

/// Target armor / health category matching the ODF `*Scale` properties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(usize)]
pub enum SpaceDamageCategory {
    #[default]
    Person = 0,
    Animal = 1,
    Droid = 2,
    Vehicle = 3,
    Building = 4,
}

/// A blast: what it does in full inside its inner radius, falling to nothing
/// at its outer one, and how much of that each kind of thing takes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpaceExplosionSpec {
    pub damage: f64,
    pub inner_radius: f64,
    pub outer_radius: f64,
    /// Multipliers indexed by `SpaceDamageCategory as usize`.
    pub scales: [f64; 5],
    pub shield_scale: f64,
}

impl SpaceExplosionSpec {
    pub const NONE: Self = Self { damage: 0.0, inner_radius: 0.0, outer_radius: 0.0, scales: [0.0; 5], shield_scale: 0.0 };

    /// Whether it does anything at all.
    pub fn hurts(&self) -> bool {
        self.damage > 0.0 && (self.outer_radius > 0.0 || self.inner_radius > 0.0)
    }

    /// The share of its damage a thing with a bounding sphere takes from a
    /// blast at `at`: by the distance to the sphere's centre less half its
    /// radius, as of every explosion (`crate::ordnance::blast_distance`).
    pub fn share(&self, at: [f64; 3], center: [f64; 3], radius: f64) -> f64 {
        crate::ordnance::defs::falloff(crate::ordnance::blast_distance(at, center, radius), self.inner_radius, self.outer_radius)
    }
}

/// Complete typed damage payload for direct impacts and splash blasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceDamagePacket {
    pub raw_damage: f64,
    /// Multipliers indexed by `SpaceDamageCategory as usize`.
    pub scales: [f64; 5],
    pub shield_scale: f64,
    pub attacker_unit: Option<SpaceOwner>,
    pub attacker_team: i32,
    pub attacker_is_ai: bool,
}

impl SpaceDamagePacket {
    pub fn from_weapon_direct(
        spec: &SpaceWeaponSpec,
        age: f64,
        speed_fraction: f64,
        attacker_unit: Option<SpaceOwner>,
        attacker_team: i32,
        attacker_is_ai: bool,
    ) -> Self {
        let raw_damage = if spec.guided_missile {
            if speed_fraction >= spec.guided_boost_threshold {
                spec.guided_damage_boost
            } else {
                spec.guided_damage
            }
        } else {
            evaluate_falloff_damage(
                spec.max_damage,
                spec.final_damage,
                spec.damage_transition_delay,
                spec.damage_transition_period,
                age,
            )
        };
        Self {
            raw_damage,
            scales: [
                spec.person_scale,
                spec.animal_scale,
                spec.droid_scale,
                spec.vehicle_scale,
                spec.building_scale,
            ],
            shield_scale: spec.shield_scale,
            attacker_unit,
            attacker_team,
            attacker_is_ai,
        }
    }

    pub fn from_blast(
        blast: &SpaceExplosionSpec,
        share: f64,
        attacker_unit: Option<SpaceOwner>,
        attacker_team: i32,
        attacker_is_ai: bool,
    ) -> Self {
        Self {
            raw_damage: blast.damage * share,
            scales: blast.scales,
            shield_scale: blast.shield_scale,
            attacker_unit,
            attacker_team,
            attacker_is_ai,
        }
    }
}

/// Evaluates `MaxDamage -> DamageFinalDamage` linear interpolation at impact
/// flight time `age`.
#[inline]
pub fn evaluate_falloff_damage(
    base_damage: f64,
    final_damage: Option<f64>,
    delay: f64,
    period: f64,
    age: f64,
) -> f64 {
    let Some(fin) = final_damage else {
        return base_damage;
    };
    if age <= delay || period <= 0.0 {
        base_damage
    } else {
        let u = v8math::min(1.0, (age - delay) / period);
        base_damage + (fin - base_damage) * u
    }
}

/// Result of applying a damage packet against a shielded/armored object.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpaceDamageOutcome {
    pub shield_loss: f64,
    pub hull_loss: f64,
    pub shield_broke: bool,
    pub destroyed: bool,
}

/// Applies the 2005 shield-absorption, armor-category scaling, and
/// `AIDamageThreshold` floor rules to an object's `(cur_shield, health)`.
pub fn apply_shield_and_hull_damage(
    cur_shield: &mut f64,
    health: &mut f64,
    max_health: f64,
    ai_damage_threshold: f64,
    category: SpaceDamageCategory,
    packet: &SpaceDamagePacket,
) -> SpaceDamageOutcome {
    let mut out = SpaceDamageOutcome::default();
    if !packet.raw_damage.is_finite()
        || packet.raw_damage <= 0.0
        || !health.is_finite()
        || *health <= 0.0
    {
        return out;
    }

    let mut raw_remainder = packet.raw_damage;
    if *cur_shield > 0.0 && packet.shield_scale > 0.0 {
        let wanted_shield = packet.raw_damage * packet.shield_scale;
        if *cur_shield >= wanted_shield {
            *cur_shield -= wanted_shield;
            out.shield_loss = wanted_shield;
            raw_remainder = 0.0;
        } else {
            let prev = *cur_shield;
            *cur_shield = 0.0;
            out.shield_loss = prev;
            out.shield_broke = true;
            raw_remainder = v8math::max(0.0, packet.raw_damage - prev / packet.shield_scale);
        }
    }

    let cat_scale = packet.scales[category as usize];
    if raw_remainder > 0.0 && cat_scale > 0.0 {
        let wanted_hull = raw_remainder * cat_scale;
        let floor = if packet.attacker_is_ai && ai_damage_threshold > 0.0 && max_health.is_finite() {
            v8math::min(max_health, v8math::max(0.0, ai_damage_threshold * max_health))
        } else {
            0.0
        };
        let next_health = v8math::max(floor, *health - wanted_hull);
        out.hull_loss = v8math::max(0.0, *health - next_health);
        *health = next_health;
        if *health <= 0.0 {
            out.destroyed = true;
        }
    }
    out
}

/// Candidate lock-on / auto-aim target (either an enemy starfighter or a
/// lock-enabled critical ship system / frigate).
#[derive(Clone, Copy, Debug)]
pub struct LockCandidate {
    pub kind: SpaceLockTargetKind,
    pub index: usize,
    pub generation: u64,
    pub team: i32,
    pub alive: bool,
    pub evading: bool,
    pub position: [f64; 3],
    pub kernel_body: Option<u32>,
}

/// Updates a starfighter's lock-on state for `weapon_index` / `weapon` over
/// one tick `dt`. Works for any lock-capable weapon (`lock_on_range > 0.0`),
/// not only missiles. Zero allocations.
#[allow(clippy::too_many_arguments)]
pub fn update_fighter_lock_on(
    lock: &mut SpaceLockState,
    weapon_index: u8,
    shooter_pos: [f64; 3],
    shooter_fwd: [f64; 3],
    shooter_team: i32,
    shooter_body: Option<u32>,
    weapon: &SpaceWeaponSpec,
    dt: f64,
    request_lock: bool,
    candidates: &[LockCandidate],
    collision: &mut CollisionOwner,
    collision_world: usize,
) {
    let max_acquire_dist = if weapon.lock_on_range > 0.0 {
        v8math::min(weapon.lock_on_range, weapon.target_lock_max_distance)
    } else {
        0.0
    };
    if max_acquire_dist <= 0.0 {
        *lock = SpaceLockState::default();
        return;
    }
    if lock.kind != SpaceLockTargetKind::None && lock.weapon_index != weapon_index {
        *lock = SpaceLockState::default();
    }
    let acquire_cos = v8math::cos(weapon.lock_on_angle_deg * PI / 180.0);
    let retain_cos = v8math::cos(weapon.lock_off_angle_deg * PI / 180.0);
    let lose_dist = v8math::max(max_acquire_dist, weapon.target_lock_max_distance_lose);
    let ignore = shooter_body.map_or(-1, |k| k as i32);

    let mut check_los = |target: &LockCandidate, dir: [f64; 3], dist: f64| -> bool {
        if dist <= PROVISIONAL_LOCK_LOS_NEAR_M {
            return true;
        }
        let hit = collision.ray_hit(
            collision_world as u32,
            Ray {
                ox: shooter_pos[0],
                oy: shooter_pos[1],
                oz: shooter_pos[2],
                dx: dir[0],
                dy: dir[1],
                dz: dir[2],
            },
            dist - PROVISIONAL_LOCK_LOS_SHORT_M,
            ignore,
        );
        match hit {
            None => true,
            Some((code, h)) => {
                if code == MISS || target.kernel_body == Some(code as u32) {
                    true
                } else {
                    h.distance >= dist - PROVISIONAL_LOCK_LOS_TARGET_TOL_M
                }
            }
        }
    };

    // Validate currently tracked target.
    let mut current: Option<&LockCandidate> = if lock.kind != SpaceLockTargetKind::None {
        candidates.iter().find(|c| {
            c.kind == lock.kind
                && c.index == lock.target_index
                && c.generation == lock.target_generation
                && c.alive
                && !c.evading
                && c.team != 0
                && c.team != shooter_team
        })
    } else {
        None
    };

    if let Some(target) = current {
        let dx = target.position[0] - shooter_pos[0];
        let dy = target.position[1] - shooter_pos[1];
        let dz = target.position[2] - shooter_pos[2];
        let dist = v8math::hypot3(dx, dy, dz);
        let in_range = dist > 1.0 && dist <= lose_dist;
        let dir = if dist > 1e-6 {
            [dx / dist, dy / dist, dz / dist]
        } else {
            shooter_fwd
        };
        let cos_ang = dir[0] * shooter_fwd[0] + dir[1] * shooter_fwd[1] + dir[2] * shooter_fwd[2];
        if in_range && cos_ang >= retain_cos && check_los(target, dir, dist) {
            lock.lost_los_time = 0.0;
            lock.progress += dt;
            if lock.progress >= weapon.lock_time {
                lock.locked = true;
            }
        } else {
            lock.lost_los_time += dt;
            if lock.lost_los_time > PROVISIONAL_LOCK_LOS_GRACE {
                current = None;
                *lock = SpaceLockState::default();
            }
        }
    } else {
        *lock = SpaceLockState::default();
    }

    // Acquire best enemy candidate in cone when no lock is active.
    if current.is_none() && request_lock {
        let mut best: Option<(&LockCandidate, f64)> = None;
        for c in candidates {
            if !c.alive || c.evading || c.team == 0 || c.team == shooter_team {
                continue;
            }
            let dx = c.position[0] - shooter_pos[0];
            let dy = c.position[1] - shooter_pos[1];
            let dz = c.position[2] - shooter_pos[2];
            let dist = v8math::hypot3(dx, dy, dz);
            if dist < 1.0 || dist > max_acquire_dist {
                continue;
            }
            let dir = [dx / dist, dy / dist, dz / dist];
            let cos_ang = dir[0] * shooter_fwd[0] + dir[1] * shooter_fwd[1] + dir[2] * shooter_fwd[2];
            if cos_ang < acquire_cos {
                continue;
            }
            if best.is_none_or(|(_, bscore)| cos_ang > bscore) && check_los(c, dir, dist) {
                best = Some((c, cos_ang));
            }
        }
        if let Some((target, _)) = best {
            lock.weapon_index = weapon_index;
            lock.kind = target.kind;
            lock.target_index = target.index;
            lock.target_generation = target.generation;
            lock.progress = dt;
            lock.locked = weapon.lock_time <= dt;
            lock.lost_los_time = 0.0;
        }
    }
}

/// A gun's clock runs in steps of at least this long.
const SHOT_CLOCK_STEP: f64 = 1.0 / 32768.0;

/// A 2005 gun's shot clock. A shot is `salvo_count` salvos: the first
/// `initial_salvo_delay` after the shot begins (each shot of a held trigger
/// anew) and each of the others a time after the one before it, drawn between
/// `salvo_delay` and `salvo_delay_max`. The shot delay is counted once the last
/// salvo is out, from the next tick on unless the salvos follow each other at
/// once. What a tick leaves over carries into the next wait, so a gun keeps its
/// rate whatever the tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpaceShotClock {
    /// Seconds left of the shot delay.
    pub cooldown: f64,
    /// Salvos of the shot still to come.
    pub salvos: u32,
    /// Seconds to the next of them.
    pub wait: f64,
}

impl SpaceShotClock {
    /// Runs through `left` seconds of a tick to the next salvo, beginning a new
    /// shot once the last one's delay is over if the gun `may_start` one. True
    /// when a salvo is due; `left` keeps the rest of the tick.
    pub fn due(&mut self, spec: &SpaceWeaponSpec, may_start: bool, left: &mut f64) -> bool {
        while *left > SHOT_CLOCK_STEP {
            if self.salvos > 0 {
                if self.wait <= 0.0 {
                    return true;
                }
                let step = v8math::min(self.wait, *left);
                self.wait -= step;
                *left -= step;
            } else {
                let step = v8math::min(self.cooldown, *left);
                self.cooldown -= step;
                *left -= step;
                if self.cooldown > 0.0 || !may_start {
                    return false;
                }
                self.salvos = spec.salvo_count.max(1);
                self.wait = spec.initial_salvo_delay;
                self.cooldown = v8math::max(PROVISIONAL_MIN_SHOT_INTERVAL, spec.shot_delay);
            }
        }
        false
    }

    /// The salvo that was due is out.
    pub fn fired(&mut self, spec: &SpaceWeaponSpec, left: &mut f64, rng: &mut SpaceRng) {
        let spread = spec.salvo_delay_max - spec.salvo_delay;
        self.wait += if spread > 0.0 { spec.salvo_delay + spread * rng.next_unit() } else { spec.salvo_delay };
        self.salvos = self.salvos.saturating_sub(1);
        if self.salvos == 0 {
            *left -= v8math::min(self.wait, *left);
            self.wait = 0.0;
        }
    }

    /// A tick of a gun nobody works: what was left of its shot is dropped and
    /// its shot delay runs on.
    pub fn idle(&mut self, dt: f64) {
        (self.salvos, self.wait) = (0, 0.0);
        self.cooldown = v8math::max(0.0, self.cooldown - dt);
    }

    /// No shot is in progress and the last one's delay is over.
    pub fn done(&self) -> bool {
        self.salvos == 0 && self.cooldown <= 0.0
    }
}

/// A 2005 gun's kick: the spread its salvos add on both axes, and the state
/// that keeps it from falling while the gun spins up and down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpaceKick {
    /// Radians added to both spreads while positive.
    pub value: f64,
    /// The gun has been pulled and has not stopped since.
    pub spun: bool,
    /// Seconds left in which the kick does not fall.
    pub cool_down: f64,
}

impl SpaceKick {
    /// One tick before the tick's shots, as the 2005 game runs it. The kick
    /// falls by `spread_recover_rate` a second, to zero for a bot and to minus
    /// `spread_threshold` for a player, except while the gun warms up with its
    /// trigger held and for `cool_down_time` after it stops: once its trigger
    /// is up (or it reloads or overheats, its trigger held or not) and its
    /// last salvo and shot delay are done. Let go in the middle of its
    /// warm-up, it cools for the warmed share of that time. `warm_up` is the
    /// seconds the gun has warmed.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(&mut self, spec: &SpaceWeaponSpec, held: bool, warm_up: f64, cycle_done: bool, stopped: bool, player: bool, dt: f64) {
        self.cool_down = v8math::max(0.0, self.cool_down - dt);
        let warming = spec.warm_up_time > 0.0 && warm_up < spec.warm_up_time;
        // A gun that cannot fire starts no new pull, its trigger held or not.
        if held && !stopped && !self.spun {
            self.spun = true;
            self.cool_down = 0.0;
        }
        if self.spun && !held && warming {
            self.spun = false;
            self.cool_down = spec.cool_down_time * warm_up / spec.warm_up_time;
        } else if self.spun && !warming && cycle_done && (!held || stopped) {
            self.spun = false;
            self.cool_down = spec.cool_down_time;
        }
        if (self.spun && warming) || self.cool_down > 0.0 {
            return;
        }
        let floor = if player { -spec.spread_threshold } else { 0.0 };
        let fallen = self.value - spec.spread_recover_rate * dt;
        self.value = if fallen < floor { floor } else { fallen };
    }

    /// The kick one salvo adds: `spread_per_shot` times the spread scale its
    /// user hands the gun, up to `spread_limit`.
    pub fn salvo(&mut self, spec: &SpaceWeaponSpec, scale: f64) {
        let raised = self.value + spec.spread_per_shot * scale;
        self.value = if spec.spread_limit < raised { spec.spread_limit } else { raised };
    }

    /// The spread (pitch, yaw) a round draws now: the gun's own plus the kick
    /// while it is positive.
    pub fn spread(&self, spec: &SpaceWeaponSpec) -> (f64, f64) {
        let kick = if self.value > 0.0 { self.value } else { 0.0 };
        (spec.pitch_spread + kick, spec.yaw_spread + kick)
    }
}

/// Turns a unit aim direction `dir` by one shot's spread, as the 2005 game
/// does: `pitch_spread * (u1 - u2)` radians down from the aim and
/// `yaw_spread * (u3 - u4)` radians to its right, each `u` a uniform draw in
/// [0, 1) taken in that order, in the frame whose up is `up_hint` made square
/// to `dir` (the world's when `up_hint` lies along `dir`). A weapon with no
/// spread draws nothing.
pub fn apply_weapon_spread(
    dir: [f64; 3],
    up_hint: [f64; 3],
    pitch_spread: f64,
    yaw_spread: f64,
    rng: &mut SpaceRng,
) -> [f64; 3] {
    if pitch_spread == 0.0 && yaw_spread == 0.0 {
        return dir;
    }
    let pitch = pitch_spread * (rng.next_unit() - rng.next_unit());
    let yaw = yaw_spread * (rng.next_unit() - rng.next_unit());
    let side = |up: [f64; 3]| {
        let r = [up[1] * dir[2] - up[2] * dir[1], up[2] * dir[0] - up[0] * dir[2], up[0] * dir[1] - up[1] * dir[0]];
        let len = v8math::hypot3(r[0], r[1], r[2]);
        (len > 1e-6 && len.is_finite()).then(|| [r[0] / len, r[1] / len, r[2] / len])
    };
    let world_up = if dir[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let Some(right) = side(up_hint).or_else(|| side(world_up)) else {
        return dir;
    };
    let up = [
        dir[1] * right[2] - dir[2] * right[1],
        dir[2] * right[0] - dir[0] * right[2],
        dir[0] * right[1] - dir[1] * right[0],
    ];
    let (sp, cp) = (v8math::sin(pitch), v8math::cos(pitch));
    let (sy, cy) = (v8math::sin(yaw), v8math::cos(yaw));
    let out = [
        dir[0] * cp * cy + right[0] * cp * sy - up[0] * sp,
        dir[1] * cp * cy + right[1] * cp * sy - up[1] * sp,
        dir[2] * cp * cy + right[2] * cp * sy - up[2] * sp,
    ];
    let olen = v8math::hypot3(out[0], out[1], out[2]);
    if olen > 1e-9 {
        [out[0] / olen, out[1] / olen, out[2] / olen]
    } else {
        dir
    }
}

/// Pulls `dir` toward the best aligned live enemy inside the cone
/// `auto_aim_size` (radians: the weapon's `AutoAimSize` as its class keeps it,
/// see `flight::AUTO_AIM_SCALE`) when it is above zero.
pub fn apply_auto_aim(
    origin: [f64; 3],
    dir: [f64; 3],
    auto_aim_size: f64,
    max_range: f64,
    shooter_team: i32,
    candidates: &[LockCandidate],
) -> [f64; 3] {
    if auto_aim_size <= 0.0 || candidates.is_empty() {
        return dir;
    }
    let min_cos = v8math::cos(auto_aim_size);
    let mut best: Option<([f64; 3], f64)> = None;
    for c in candidates {
        if !c.alive || c.team == 0 || c.team == shooter_team {
            continue;
        }
        let dx = c.position[0] - origin[0];
        let dy = c.position[1] - origin[1];
        let dz = c.position[2] - origin[2];
        let dist = v8math::hypot3(dx, dy, dz);
        if dist < 1.0 || dist > max_range {
            continue;
        }
        let cand_dir = [dx / dist, dy / dist, dz / dist];
        let cos_a = cand_dir[0] * dir[0] + cand_dir[1] * dir[1] + cand_dir[2] * dir[2];
        if cos_a >= min_cos && best.is_none_or(|(_, b)| cos_a > b) {
            best = Some((cand_dir, cos_a));
        }
    }
    best.map_or(dir, |(d, _)| d)
}

/// Active projectile in the pre-allocated space ordnance pool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EmittingBody {
    Fighter { index: usize, generation: u32 },
    Building { index: usize, life: f64 },
}

#[derive(Clone, Copy, Debug)]
pub struct SpaceProjectile {
    pub active: bool,
    pub generation: u32,
    pub previous_position: [f64; 3],
    pub kind: SpaceOrdnanceKind,
    pub owner_unit: Option<SpaceOwner>,
    pub owner_team: i32,
    pub owner_is_ai: bool,
    /// True for infantry boarding shots that are blocked by hangar forcefields.
    pub from_infantry: bool,
    pub ignore_body: Option<u32>,
    /// Whether a beam goes on behind the craft and soldiers it strikes.
    pub pass_through: bool,
    pub emitter: Option<EmittingBody>,
    /// Which of the emitter's weapons fired the round (0 for a ship gun's).
    pub weapon_index: u8,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub age: f64,
    pub life_span: f64,
    pub gravity: f64,
    pub turn_rate: f64,
    pub lock_kind: SpaceLockTargetKind,
    pub lock_target: usize,
    pub lock_generation: u64,
    pub is_tracer: bool,
    /// Dynamic direct-impact damage parameters evaluated at impact time.
    pub base_damage: f64,
    pub final_damage: Option<f64>,
    pub damage_transition_delay: f64,
    pub damage_transition_period: f64,
    pub scales: [f64; 5],
    pub shield_scale: f64,
    /// Live guided-rocket steering & speed-threshold payload.
    pub guided_pilot_fighter: Option<(usize, u32, u8)>,
    pub guided_damage: f64,
    pub guided_damage_boost: f64,
    pub guided_boost_threshold: f64,
    pub guided_cruise_speed: f64,
    pub guided_boost_speed: f64,
    /// A steered rocket's time left outside the battlefield (-1 inside).
    pub guided_out_of_bounds: f64,
    /// The blast it sets off where it strikes.
    pub blast: SpaceExplosionSpec,
    /// The one it sets off at the end of its life.
    pub expire_blast: SpaceExplosionSpec,
    /// The one a guided rocket sets off instead past its boost threshold.
    pub guided_blast_boost: SpaceExplosionSpec,
}

impl Default for SpaceProjectile {
    fn default() -> Self {
        Self {
            generation: 0, previous_position: [0.0; 3],
            active: false,
            kind: SpaceOrdnanceKind::Bolt,
            owner_unit: None,
            owner_team: 0,
            owner_is_ai: false,
            from_infantry: false,
            ignore_body: None,
            pass_through: false,
            emitter: None,
            weapon_index: 0,
            position: [0.0; 3],
            velocity: [0.0; 3],
            age: 0.0,
            life_span: 0.0,
            gravity: 0.0,
            turn_rate: 0.0,
            lock_kind: SpaceLockTargetKind::None,
            lock_target: 0,
            lock_generation: 0,
            is_tracer: false,
            base_damage: 0.0,
            final_damage: None,
            damage_transition_delay: 0.0,
            damage_transition_period: 0.0,
            scales: [1.0; 5],
            shield_scale: 1.0,
            guided_pilot_fighter: None,
            guided_damage: 0.0,
            guided_damage_boost: 0.0,
            guided_boost_threshold: 0.8,
            guided_cruise_speed: 150.0,
            guided_boost_speed: 225.0,
            guided_out_of_bounds: -1.0,
            blast: SpaceExplosionSpec::NONE,
            expire_blast: SpaceExplosionSpec::NONE,
            guided_blast_boost: SpaceExplosionSpec::NONE,
        }
    }
}

/// True when a guided rocket's speed meets or exceeds its boost threshold fraction.
#[inline]
pub fn guided_rocket_is_boosted(speed: f64, boost_speed: f64, threshold: f64) -> bool {
    let frac = speed / v8math::max(1.0, boost_speed);
    frac >= threshold
}

/// Direct-hit damage for a guided rocket at `speed`, choosing boosted vs base damage.
#[inline]
pub fn guided_rocket_direct_damage(
    speed: f64,
    boost_speed: f64,
    threshold: f64,
    base_damage: f64,
    boost_damage: f64,
) -> f64 {
    if guided_rocket_is_boosted(speed, boost_speed, threshold) {
        boost_damage
    } else {
        base_damage
    }
}

impl SpaceProjectile {
    /// Whether a guided rocket is past its boost threshold.
    fn boosted(&self) -> bool {
        let speed = v8math::hypot3(self.velocity[0], self.velocity[1], self.velocity[2]);
        speed / v8math::max(1.0, self.guided_boost_speed) >= self.guided_boost_threshold
    }

    /// The blast it sets off where it strikes now.
    pub fn impact_blast(&self) -> SpaceExplosionSpec {
        if self.kind == SpaceOrdnanceKind::GuidedRocket && self.boosted() { self.guided_blast_boost } else { self.blast }
    }

    /// The blast it sets off if its life ends now: a time bomb its own, a
    /// guided rocket the one it would strike with (the plain one without its
    /// pilot), any other round the one its class names for that.
    pub fn end_blast(&self, piloted: bool) -> SpaceExplosionSpec {
        match self.kind {
            SpaceOrdnanceKind::TimeBomb => self.blast,
            SpaceOrdnanceKind::GuidedRocket if piloted => self.impact_blast(),
            SpaceOrdnanceKind::GuidedRocket => self.blast,
            _ => self.expire_blast,
        }
    }

    /// Evaluates the direct-hit damage packet dynamically at impact time using
    /// live flight `age` (`DamageFinalDamage` falloff) and live speed fraction
    /// (`GuidedMissileBoostExplosionSpeedThreshold`).
    pub fn impact_damage_packet(&self) -> SpaceDamagePacket {
        let raw_damage = if self.kind == SpaceOrdnanceKind::GuidedRocket {
            if self.boosted() { self.guided_damage_boost } else { self.guided_damage }
        } else {
            evaluate_falloff_damage(
                self.base_damage,
                self.final_damage,
                self.damage_transition_delay,
                self.damage_transition_period,
                self.age,
            )
        };
        SpaceDamagePacket {
            raw_damage,
            scales: self.scales,
            shield_scale: self.shield_scale,
            attacker_unit: self.owner_unit,
            attacker_team: self.owner_team,
            attacker_is_ai: self.owner_is_ai,
        }
    }

    /// Returns true if this projectile should never be silently overwritten
    /// when the ordnance pool saturates.
    #[inline]
    pub fn is_protected(&self) -> bool {
        matches!(
            self.kind,
            SpaceOrdnanceKind::TimeBomb | SpaceOrdnanceKind::GuidedRocket
        )
    }
}

/// Fixed-capacity projectile pool (zero runtime heap allocations).
#[derive(Clone, Debug)]
pub struct SpaceOrdnancePool {
    pub slots: Vec<SpaceProjectile>,
    next_cursor: usize,
    pub refused: u32,
    pub observations: Option<super::observations::Observations>,
}

impl SpaceOrdnancePool {
    pub fn new(capacity: usize) -> Self {
        Self {
            slots: vec![SpaceProjectile::default(); capacity],
            next_cursor: 0,
            refused: 0,
            observations: None,
        }
    }

    /// Claims a free slot. A saturated pool keeps every in-flight projectile.
    pub fn spawn(&mut self, proj: SpaceProjectile) -> Option<usize> {
        let len = self.slots.len();
        for offset in 0..len {
            let idx = (self.next_cursor + offset) % len;
            if !self.slots[idx].active {
                let Some(generation) = self.slots[idx].generation.checked_add(1) else { continue; };
                self.slots[idx] = proj;
                self.slots[idx].generation = generation;
                self.slots[idx].previous_position = proj.position;
                self.slots[idx].active = true;
                self.next_cursor = (idx + 1) % len;
                if let Some(obs) = &mut self.observations { obs.rounds[idx] = Default::default(); }
                return Some(idx);
            }
        }
        self.refused = self.refused.saturating_add(1);
        None
    }

    pub(super) fn spawn_weapon(&mut self, proj: SpaceProjectile, weapon: &str, barrel: Option<usize>) -> Option<usize> {
        let slot = self.spawn(proj)?;
        if let Some(obs) = &mut self.observations { obs.launch(slot, &self.slots[slot], weapon, barrel); }
        Some(slot)
    }

    pub fn active_count(&self) -> usize {
        self.slots.iter().filter(|p| p.active).count()
    }

    /// Mixes active projectiles into an FNV-1a digest.
    pub fn mix_digest(&self, hash: Wide) -> Wide {
        let mut m = Fnv::new(hash);
        m.small(self.refused);
        m.count(self.next_cursor);
        for p in &self.slots {
            m.small(p.generation);
            m.small(p.active);
            if !p.active {
                continue;
            }
            m.small(p.kind as u8);
            m.small(p.owner_unit.is_some());
            m.small(p.guided_pilot_fighter.is_some());
            m.small(p.final_damage.is_some());
            m.small(p.from_infantry); m.small(p.is_tracer);
            m.option_small(p.ignore_body);
            m.small(p.pass_through);
            m.bits(p.guided_boost_threshold);
            if let Some(owner) = p.owner_unit { m.small(owner.slot); m.small(owner.gen); }
            for n in p.previous_position.into_iter().chain(p.scales).chain([
                p.base_damage,p.final_damage.unwrap_or(0.0),p.life_span,p.shield_scale,p.gravity,p.turn_rate,
                p.guided_damage,p.guided_damage_boost,
                p.guided_cruise_speed,p.guided_boost_speed,p.damage_transition_delay,p.damage_transition_period]) { m.bits(n); }
            for blast in [p.blast, p.expire_blast, p.guided_blast_boost] {
                for n in blast.scales.into_iter().chain([blast.damage, blast.inner_radius, blast.outer_radius, blast.shield_scale]) { m.bits(n); }
            }
            m.small(p.lock_kind as u8); m.count(p.lock_target); m.le(p.lock_generation.to_le_bytes());
            m.signed(p.owner_team); m.small(p.owner_is_ai);
            if let Some((i,g,seat)) = p.guided_pilot_fighter { m.count(i); m.small(g); m.small(seat); }
            match p.emitter {
                None => m.small(0u32),
                Some(EmittingBody::Fighter { index, generation }) => {
                    m.small(1u32); m.count(index); m.small(generation);
                }
                Some(EmittingBody::Building { index, life }) => {
                    m.small(2u32); m.count(index); m.bits(life);
                }
            }
            m.bits(p.age);
            m.bits(p.guided_out_of_bounds);
            for &c in &p.position {
                m.bits(c);
            }
            for &c in &p.velocity {
                m.bits(c);
            }
        }
        m.finish()
    }
}

/// Steers an in-flight pilot-guided rocket (`SpaceOrdnanceKind::GuidedRocket`)
/// from pilot stick/boost inputs, accelerating up to `boost_speed` while boost
/// is held and coasting toward `cruise_speed` when released. Pure `v8math`.
#[allow(clippy::too_many_arguments)]
pub fn steer_guided_rocket(
    vel: &mut [f64; 3],
    stick_x: f64,
    stick_y: f64,
    boost: bool,
    cruise_speed: f64,
    boost_speed: f64,
    turn_rate: f64,
    dt: f64,
) {
    steer_guided_rocket_with_rates(
        vel,
        stick_x,
        stick_y,
        boost,
        0.0,
        cruise_speed,
        boost_speed,
        PROVISIONAL_GUIDED_ACCEL,
        PROVISIONAL_GUIDED_ACCEL,
        turn_rate,
        turn_rate,
        dt,
    );
}

/// Shared guided-rocket steering step honoring class-authored minimum speed,
/// cruise/boost accelerations, and independent turn/pitch rates. Pure `v8math`.
#[allow(clippy::too_many_arguments)]
pub fn steer_guided_rocket_with_rates(
    vel: &mut [f64; 3],
    stick_x: f64,
    stick_y: f64,
    boost: bool,
    min_speed: f64,
    cruise_speed: f64,
    boost_speed: f64,
    accel: f64,
    boost_accel: f64,
    turn_rate: f64,
    pitch_rate: f64,
    dt: f64,
) {
    let cur_speed = v8math::hypot3(vel[0], vel[1], vel[2]);
    if cur_speed < 1e-6 {
        return;
    }
    let dir = [vel[0] / cur_speed, vel[1] / cur_speed, vel[2] / cur_speed];
    let target_speed = if boost { boost_speed } else { cruise_speed };
    let max_dv = v8math::max(0.0, if boost { boost_accel } else { accel }) * dt;
    let next_speed = v8math::max(
        min_speed,
        v8math::min(
            cur_speed + max_dv,
            v8math::max(cur_speed - max_dv, target_speed),
        ),
    );

    let sx = v8math::min(1.0, v8math::max(-1.0, stick_x));
    let sy = v8math::min(1.0, v8math::max(-1.0, stick_y));
    let yaw_ang = -sx * turn_rate * dt;
    let pitch_ang = -sy * pitch_rate * dt;

    // Build orthonormal basis around current velocity direction.
    let up_ref = if dir[1].abs() < 0.95 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let rx = up_ref[1] * dir[2] - up_ref[2] * dir[1];
    let ry = up_ref[2] * dir[0] - up_ref[0] * dir[2];
    let rz = up_ref[0] * dir[1] - up_ref[1] * dir[0];
    let rlen = v8math::hypot3(rx, ry, rz);
    let mut new_dir = Vector3::new(dir[0], dir[1], dir[2]);
    if rlen > 1e-6 && (yaw_ang.abs() > 1e-9 || pitch_ang.abs() > 1e-9) {
        let right = [rx / rlen, ry / rlen, rz / rlen];
        let up = [
            dir[1] * right[2] - dir[2] * right[1],
            dir[2] * right[0] - dir[0] * right[2],
            dir[0] * right[1] - dir[1] * right[0],
        ];
        let qy: Quaternion = quat_from_axis_angle(up, yaw_ang);
        let qp: Quaternion = quat_from_axis_angle(right, pitch_ang);
        new_dir.apply_quaternion(&qy);
        new_dir.apply_quaternion(&qp);
    }
    let len = v8math::hypot3(new_dir.x, new_dir.y, new_dir.z);
    if len > 1e-6 {
        vel[0] = (new_dir.x / len) * next_speed;
        vel[1] = (new_dir.y / len) * next_speed;
        vel[2] = (new_dir.z / len) * next_speed;
    }
}

/// Fires one weapon slot on a starfighter if ready (honoring `WarmUpTime`, its
/// shot clock, `BarrageMin`/`BarrageMax`/`BarrageDelay`,
/// `HeatPerShot` / overheat, `RoundsPerClip` / `ReloadTime`,
/// `ShotsPerSalvo`, `AutoAimSize`, the spread and its kick, real vehicle-pack fire points, and
/// `SecondaryOrdnancePeriod`). Returns `(emitted_count, last_origin, last_dir)`.
#[allow(clippy::too_many_arguments)]
pub fn try_fire_fighter_weapon(
    fighter: &mut SpaceFighter,
    profile: &SpaceFlightProfile,
    weapon_idx: usize,
    dt: f64,
    trigger_held: bool,
    guided_boost: bool,
    reticle_point: [f64; 3],
    candidates: &[LockCandidate],
    rng: &mut SpaceRng,
    pool: &mut SpaceOrdnancePool,
) -> (u32, [f64; 3], [f64; 3]) {
    let Some(spec) = profile.weapons.get(weapon_idx) else {
        return (0, fighter.position, fighter.forward());
    };
    let pilot = fighter.occupant(spec.seat);
    if pilot.is_none() {return (0, fighter.position, fighter.forward());}
    let fwd = if spec.seat == 0 { fighter.forward() } else { fighter.seat_input(spec.seat).aim.unwrap_or_else(|| fighter.forward()) };
    let f_pos = fighter.position;
    let f_quat = fighter.quaternion;
    let f_id = fighter.id;
    let f_team = fighter.team;
    let f_body = fighter.kernel_body;
    let f_lock = fighter.weapon_locks[weapon_idx];
    // Only a bot's skill scales its kick: a player's is always 1.
    let scale = match pilot {
        Some(p) if !p.is_human => fighter.seat_input(spec.seat).spread_scale.filter(|s| s.is_finite()).unwrap_or(1.0),
        _ => 1.0,
    };
    let rt = &mut fighter.weapons[weapon_idx];
    rt.trigger_was_held = trigger_held;
    if !trigger_held {
        rt.warm_up = v8math::max(0.0, rt.warm_up - dt * PROVISIONAL_WARMUP_DECAY_RATE);
    } else if rt.warm_up < spec.warm_up_time {
        rt.warm_up = v8math::min(spec.warm_up_time, rt.warm_up + dt);
    }

    // A barrage is how a bot works the trigger: a player's gun has none.
    let barrage = spec.barrage_max > 0 && pilot.is_some_and(|p| !p.is_human);
    let mut emitted = 0u32;
    let mut last_origin = f_pos;
    let mut last_dir = fwd;
    let mut left = dt;
    // The rounds still to go of the salvo in hand, which fires its
    // `shots_per_salvo` at once.
    let mut rounds = None;
    loop {
        if rounds == Some(0) {
            rounds = None;
            rt.kick.salvo(spec, scale);
            rt.clock.fired(spec, &mut left, rng);
            // A clip emptied mid-shot waits until the shot ends before
            // reloading, as in the 2005 game, and never restarts a reload
            // already counting.
            if spec.rounds_per_clip > 0
                && rt.ammo_in_clip == 0
                && rt.clock.salvos == 0
                && spec.reload_time > 0.0
                && rt.reload_timer <= 0.0
            {
                rt.reload_timer = spec.reload_time;
            }
            if barrage && rt.clock.salvos == 0 {
                rt.barrage_remaining = rt.barrage_remaining.saturating_sub(1);
                if rt.barrage_remaining == 0 && spec.barrage_delay > 0.0 {
                    rt.barrage_timer = spec.barrage_delay;
                }
            }
        }
        if rounds.is_none() {
            let loaded = spec.rounds_per_clip == 0 || rt.ammo_in_clip > 0;
            let may_start = trigger_held
                && rt.warm_up >= spec.warm_up_time
                && !rt.overheated
                && rt.reload_timer <= 0.0
                && !(barrage && rt.barrage_timer > 0.0);
            if may_start && !loaded && rt.clock.done() {
                rt.reload_timer = spec.reload_time;
            }
            if !rt.clock.due(spec, may_start && loaded, &mut left) {
                break;
            }
            if barrage && rt.barrage_remaining == 0 {
                rt.barrage_remaining = rng.next_range_u32(spec.barrage_min.max(1), spec.barrage_max);
            }
            rounds = Some(spec.shots_per_salvo.max(1));
        }
        // An empty clip or an overheated gun cuts the shot short. The salvo it
        // cut kicks, whether or not a round of it was let go, as in the 2005
        // game.
        let empty = spec.rounds_per_clip > 0 && rt.ammo_in_clip == 0;
        if empty || (rt.overheated && spec.heat_per_shot > 0.0) {
            rt.clock.salvos = 1;
            rounds = Some(0);
            continue;
        }
        if spec.rounds_per_clip > 0 {
            rt.ammo_in_clip -= 1;
        }
        let barrel_idx = rt.shots_fired as usize;
        rt.shots_fired = rt.shots_fired.wrapping_add(1);
        let is_tracer = spec.tracer(barrel_idx as u32);
        if spec.heat_per_shot > 0.0 {
            rt.heat = v8math::min(1.0, rt.heat + spec.heat_per_shot);
            if rt.heat >= 1.0 {
                rt.overheated = true;
            }
        }

        // Resolve real vehicle-pack fire point offset rotated by fighter attitude.
        let origin = if !spec.muzzles.is_empty() {
            let local = spec.muzzles[barrel_idx % spec.muzzles.len()];
            let off = rotate_vec(f_quat, local);
            [f_pos[0] + off[0], f_pos[1] + off[1], f_pos[2] + off[2]]
        } else {
            let muzzle_dist = v8math::max(PROVISIONAL_MIN_MUZZLE_DISTANCE, profile.bounds_max[2] + PROVISIONAL_DEFAULT_MUZZLE_PAD);
            [
                f_pos[0] + fwd[0] * muzzle_dist,
                f_pos[1] + fwd[1] * muzzle_dist,
                f_pos[2] + fwd[2] * muzzle_dist,
            ]
        };

        // Converge laterally-offset wingtip muzzles onto the pilot's nose
        // reticle point (matching the 1024 m / raycast reticle anchor).
        let base_dir = if !spec.muzzles.is_empty() {
            let dx = reticle_point[0] - origin[0];
            let dy = reticle_point[1] - origin[1];
            let dz = reticle_point[2] - origin[2];
            let len = v8math::hypot3(dx, dy, dz);
            if len > 1e-6 && (dx * fwd[0] + dy * fwd[1] + dz * fwd[2]) > 0.0 {
                [dx / len, dy / len, dz / len]
            } else {
                fwd
            }
        } else {
            fwd
        };
        let aimed = apply_auto_aim(origin, base_dir, spec.auto_aim_size, spec.max_range, f_team, candidates);
        // The spread, the gun's own and its kick, is drawn in the craft's
        // frame, square to its up.
        let (pitch_spread, yaw_spread) = rt.kick.spread(spec);
        let shot_dir = apply_weapon_spread(aimed, rotate_vec(f_quat, [0.0, 1.0, 0.0]), pitch_spread, yaw_spread, rng);
        last_origin = origin;
        last_dir = shot_dir;

        let speed = if spec.guided_missile && guided_boost {
            spec.guided_boost_speed
        } else {
            spec.velocity
        };
        let can_home = f_lock.locked
            && (spec.ordnance_kind == SpaceOrdnanceKind::Missile
                || (spec.lock_on_range > 0.0 && f_lock.weapon_index as usize == weapon_idx));
        let (lock_kind, lock_target, lock_gen) = if can_home {
            (f_lock.kind, f_lock.target_index, f_lock.target_generation)
        } else {
            (SpaceLockTargetKind::None, 0, 0)
        };

        let attacker_unit = pilot.map(|p| p.owner);
        let attacker_team = pilot.map_or(f_team, |p| p.team);
        let attacker_is_ai = pilot.is_some_and(|p| !p.is_human);

        let proj = SpaceProjectile {
            generation: 0, previous_position: [0.0; 3],
            active: true,
            kind: spec.ordnance_kind,
            owner_unit: attacker_unit,
            owner_team: attacker_team,
            owner_is_ai: attacker_is_ai,
            from_infantry: false,
            ignore_body: f_body,
            pass_through: spec.pass_through,
            emitter: Some(EmittingBody::Fighter { index:f_id, generation:fighter.generation }),
            weapon_index: weapon_idx as u8,
            position: origin,
            velocity: [shot_dir[0] * speed, shot_dir[1] * speed, shot_dir[2] * speed],
            age: 0.0,
            life_span: spec.life_span,
            gravity: spec.gravity,
            turn_rate: spec.turn_rate,
            lock_kind,
            lock_target,
            lock_generation: lock_gen,
            is_tracer,
            base_damage: spec.max_damage,
            final_damage: spec.final_damage,
            damage_transition_delay: spec.damage_transition_delay,
            damage_transition_period: spec.damage_transition_period,
            scales: [
                spec.person_scale,
                spec.animal_scale,
                spec.droid_scale,
                spec.vehicle_scale,
                spec.building_scale,
            ],
            shield_scale: spec.shield_scale,
            guided_pilot_fighter: spec.guided_missile.then_some((f_id, fighter.generation, spec.seat)),
            guided_damage: spec.guided_damage,
            guided_damage_boost: spec.guided_damage_boost,
            guided_boost_threshold: spec.guided_boost_threshold,
            guided_cruise_speed: spec.velocity,
            guided_boost_speed: spec.guided_boost_speed,
            guided_out_of_bounds: -1.0,
            blast: spec.blast,
            expire_blast: spec.expire_blast,
            guided_blast_boost: spec.guided_blast_boost,
        };
        if pool.spawn_weapon(proj, &spec.name, barrel_idx.checked_rem(spec.muzzles.len())).is_some() {
            emitted += 1;
        }
        rounds = rounds.map(|n| n - 1);
    }

    (emitted, last_origin, last_dir)
}

/// Plants an infantry boarding timebomb (`LifeSpan = 5.0` s by default) that
/// detonates with full radial splash damage against nearby ship systems.
#[allow(clippy::too_many_arguments)]
pub fn plant_boarding_timebomb(
    pool: &mut SpaceOrdnancePool,
    position: [f64; 3],
    owner_unit: Option<SpaceOwner>,
    owner_team: i32,
    owner_is_ai: bool,
    fuse_s: f64,
    damage: f64,
    inner_radius: f64,
    outer_radius: f64,
) -> Option<usize> {
    pool.spawn(SpaceProjectile {
        generation: 0, previous_position: [0.0; 3],
            active: true,
        kind: SpaceOrdnanceKind::TimeBomb,
        owner_unit,
        owner_team,
        owner_is_ai,
        from_infantry: true,
        ignore_body: None,
        pass_through: false,
        emitter: None,
        weapon_index: 0,
        position,
        velocity: [0.0; 3],
        age: 0.0,
        life_span: v8math::max(PROVISIONAL_MIN_BOMB_FUSE, fuse_s),
        gravity: 0.0,
        turn_rate: 0.0,
        lock_kind: SpaceLockTargetKind::None,
        lock_target: 0,
        lock_generation: 0,
        is_tracer: false,
        base_damage: damage,
        final_damage: None,
        damage_transition_delay: 0.0,
        damage_transition_period: 0.0,
        scales: [1.0; 5],
        shield_scale: 1.0,
        guided_pilot_fighter: None,
        guided_damage: 0.0,
        guided_damage_boost: 0.0,
        guided_boost_threshold: 0.8,
        guided_cruise_speed: 0.0,
        guided_boost_speed: 0.0,
        guided_out_of_bounds: -1.0,
        blast: SpaceExplosionSpec {
            damage,
            inner_radius,
            outer_radius: v8math::max(inner_radius, outer_radius),
            scales: [1.0; 5],
            shield_scale: 1.0,
        },
        expire_blast: SpaceExplosionSpec::NONE,
        guided_blast_boost: SpaceExplosionSpec::NONE,
    })
}

/// Steers a homing missile velocity toward `target_pos` at up to `turn_rate`
/// rad/s while preserving speed. Pure `v8math`, zero allocations.
pub fn steer_homing_velocity(
    pos: [f64; 3],
    vel: &mut [f64; 3],
    target_pos: [f64; 3],
    turn_rate: f64,
    dt: f64,
) {
    let speed = v8math::hypot3(vel[0], vel[1], vel[2]);
    if speed < 1e-6 || turn_rate <= 0.0 {
        return;
    }
    let dx = target_pos[0] - pos[0];
    let dy = target_pos[1] - pos[1];
    let dz = target_pos[2] - pos[2];
    let dist = v8math::hypot3(dx, dy, dz);
    if dist < 1e-6 {
        return;
    }
    let cur = [vel[0] / speed, vel[1] / speed, vel[2] / speed];
    let desired = [dx / dist, dy / dist, dz / dist];
    let dot = v8math::min(
        1.0,
        v8math::max(-1.0, cur[0] * desired[0] + cur[1] * desired[1] + cur[2] * desired[2]),
    );
    let angle = v8math::acos(dot);
    if angle < 1e-6 {
        return;
    }
    let max_turn = turn_rate * dt;
    let frac = v8math::min(1.0, max_turn / angle);
    let bx = cur[0] + (desired[0] - cur[0]) * frac;
    let by = cur[1] + (desired[1] - cur[1]) * frac;
    let bz = cur[2] + (desired[2] - cur[2]) * frac;
    let blen = v8math::hypot3(bx, by, bz);
    if blen > 1e-6 {
        vel[0] = (bx / blen) * speed;
        vel[1] = (by / blen) * speed;
        vel[2] = (bz / blen) * speed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content::bvh::{build as build_bvh, Positions};
    use bfe_content::install::CollisionKernel;

    #[test]
    fn a_weapons_auto_aim_cone_is_its_authored_size_as_the_class_keeps_it() {
        use crate::space::flight::parse_space_weapon;
        let spec = parse_space_weapon(&bfe_content::json::parse(r#"{"class":"t","properties":[["AutoAimSize","2"]]}"#).unwrap(), 0);
        // Two units: 0.144 rad, a little over 8 degrees.
        assert!((spec.auto_aim_size - 0.144).abs() < 1e-12);
        let at = |degrees: f64| {
            let a = degrees * PI / 180.0;
            LockCandidate { kind: SpaceLockTargetKind::Fighter, index: 0, generation: 1, team: 2, alive: true, evading: false,
                position: [1000.0 * v8math::sin(a), 0.0, 1000.0 * v8math::cos(a)], kernel_body: None }
        };
        let pulled = |degrees: f64| apply_auto_aim([0.0; 3], [0.0, 0.0, 1.0], spec.auto_aim_size, 2000.0, 1, &[at(degrees)]) != [0.0, 0.0, 1.0];
        assert!(pulled(7.0));
        assert!(!pulled(10.0), "outside the cone the round goes where it was aimed");
    }

    #[test]
    fn shield_absorption_break_and_zero_scale_follow_exact_formula() {
        let packet = SpaceDamagePacket {
            raw_damage: 500.0,
            scales: [1.0, 1.0, 1.0, 1.0, 0.5],
            shield_scale: 0.5,
            attacker_unit: Some(crate::store::Ref::new(1, 1)),
            attacker_team: 1,
            attacker_is_ai: false,
        };
        let mut shield = 400.0;
        let mut hp = 1000.0;
        let o1 = apply_shield_and_hull_damage(
            &mut shield,
            &mut hp,
            1000.0,
            0.0,
            SpaceDamageCategory::Building,
            &packet,
        );
        assert!((o1.shield_loss - 250.0).abs() < 1e-9);
        assert!((shield - 150.0).abs() < 1e-9);
        assert_eq!(o1.hull_loss, 0.0);
        assert!(!o1.shield_broke);

        shield = 100.0;
        hp = 1000.0;
        let o2 = apply_shield_and_hull_damage(
            &mut shield,
            &mut hp,
            1000.0,
            0.0,
            SpaceDamageCategory::Building,
            &packet,
        );
        assert!((o2.shield_loss - 100.0).abs() < 1e-9);
        assert_eq!(shield, 0.0);
        assert!(o2.shield_broke);
        assert!((o2.hull_loss - 150.0).abs() < 1e-9);
        assert!((hp - 850.0).abs() < 1e-9);

        let zero_shield_scale = SpaceDamagePacket {
            shield_scale: 0.0,
            ..packet
        };
        shield = 200.0;
        hp = 1000.0;
        let o3 = apply_shield_and_hull_damage(
            &mut shield,
            &mut hp,
            1000.0,
            0.0,
            SpaceDamageCategory::Building,
            &zero_shield_scale,
        );
        assert_eq!(o3.shield_loss, 0.0);
        assert_eq!(shield, 200.0);
        assert!((o3.hull_loss - 250.0).abs() < 1e-9);
    }

    #[test]
    fn dynamic_impact_damage_evaluates_falloff_at_impact_age() {
        let mut proj = SpaceProjectile {
            generation: 0, previous_position: [0.0; 3],
            active: true,
            kind: SpaceOrdnanceKind::Bolt,
            base_damage: 400.0,
            final_damage: Some(100.0),
            damage_transition_delay: 0.25,
            damage_transition_period: 0.5,
            ..Default::default()
        };
        proj.age = 0.1;
        assert!((proj.impact_damage_packet().raw_damage - 400.0).abs() < 1e-9);
        proj.age = 0.5; // halfway through transition period
        assert!((proj.impact_damage_packet().raw_damage - 250.0).abs() < 1e-9);
        proj.age = 1.0; // past end of transition period
        assert!((proj.impact_damage_packet().raw_damage - 100.0).abs() < 1e-9);
    }

    #[test]
    fn guided_rocket_steering_and_live_boost_threshold() {
        let mut proj = SpaceProjectile {
            generation: 0, previous_position: [0.0; 3],
            active: true,
            kind: SpaceOrdnanceKind::GuidedRocket,
            velocity: [0.0, 0.0, 120.0],
            guided_damage: 1500.0,
            guided_damage_boost: 5000.0,
            guided_boost_threshold: 0.8,
            guided_cruise_speed: 120.0,
            guided_boost_speed: 225.0,
            ..Default::default()
        };
        // Below 0.8 * 225 = 180 m/s -> base guided damage.
        assert_eq!(proj.impact_damage_packet().raw_damage, 1500.0);

        // Steer right + boost for 0.6 s -> heading bends and speed crosses threshold.
        for _ in 0..36 {
            steer_guided_rocket(
                &mut proj.velocity,
                1.0,
                0.0,
                true,
                proj.guided_cruise_speed,
                proj.guided_boost_speed,
                PROVISIONAL_GUIDED_TURN_RATE,
                1.0 / 60.0,
            );
        }
        let speed = v8math::hypot3(proj.velocity[0], proj.velocity[1], proj.velocity[2]);
        assert!(speed >= 200.0, "expected boosted speed >= 200, got {speed}");
        assert!(proj.velocity[0].abs() > 20.0, "expected lateral steering change, got {:?}", proj.velocity);
        assert_eq!(proj.impact_damage_packet().raw_damage, 5000.0);
    }

    #[test]
    fn pool_full_never_overwrites_armed_timebomb_or_guided_rocket() {
        let mut pool = SpaceOrdnancePool::new(4);
        let bomb_slot = plant_boarding_timebomb(&mut pool, [0.0; 3], Some(crate::store::Ref::new(1, 1)), 1, false, 5.0, 500.0, 4.0, 8.0).unwrap();
        let guided_slot = pool
            .spawn(SpaceProjectile {
                kind: SpaceOrdnanceKind::GuidedRocket,
                ..Default::default()
            })
            .unwrap();
        assert!(pool.spawn(SpaceProjectile::default()).is_some());
        assert!(pool.spawn(SpaceProjectile::default()).is_some());
        assert_eq!(pool.spawn(SpaceProjectile::default()), None);
        assert_eq!(pool.refused, 1);
        assert_eq!(pool.slots[bomb_slot].kind, SpaceOrdnanceKind::TimeBomb);
        assert_eq!(pool.slots[guided_slot].kind, SpaceOrdnanceKind::GuidedRocket);
    }

    #[test]
    fn weapon_spread_is_deterministic_and_barrage_delays_between_bursts() {
        let mut r1 = SpaceRng::new(Wide::from_u32(42));
        let mut r2 = SpaceRng::new(Wide::from_u32(42));
        let d1 = apply_weapon_spread([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], 0.05, 0.10, &mut r1);
        let d2 = apply_weapon_spread([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], 0.05, 0.10, &mut r2);
        assert_eq!(d1, d2);
        assert!(d1[2] < 1.0 && d1[2] > 0.98);
    }

    /// The generator is the xorshift of the released Space battle written with two-word arithmetic: its first
    /// words from the seed 42, a ranged draw, a draw over the whole 32-bit range and a unit float are those
    /// of the same recurrence computed outside Rust.
    #[test]
    fn the_space_generator_follows_its_reference_stream() {
        let mut rng = SpaceRng::new(Wide::from_u32(42));
        let words: Vec<String> = (0..3)
            .map(|_| {
                let w = rng.next_word();
                format!("{:08x}_{:08x}", w.high(), w.low())
            })
            .collect();
        assert_eq!(words, ["56ce4ab7_719ba3a0", "c841eb53_ebbb2dda", "ca466be0_c9980276"]);
        assert_eq!(rng.next_range_u32(3, 9), 9, "word f1acc733_4a7b70df modulo 7, plus 3");
        assert_eq!(rng.next_range_u32(0, u32::MAX), 4_220_520_966, "the low word of the fifth word");
        let unit = rng.next_unit();
        assert!((unit - 0.835_739_858_920_418_7).abs() < 1e-15, "{unit}");
        assert_eq!(rng.next_range_u32(5, 5), 5);
        assert_eq!(SpaceRng::new(Wide::ZERO).state(), Wide::new(0x9e37_79b9, 0x7f4a_7c15), "a zero seed is replaced");
    }

    #[test]
    fn ai_damage_threshold_stops_ai_at_floor_while_human_destroys_target() {
        let mut shield = 0.0;
        let mut hp = 1000.0;
        let ai_hit = SpaceDamagePacket {
            raw_damage: 900.0,
            scales: [1.0; 5],
            shield_scale: 1.0,
            attacker_unit: Some(crate::store::Ref::new(2, 1)),
            attacker_team: 1,
            attacker_is_ai: true,
        };
        let o_ai = apply_shield_and_hull_damage(
            &mut shield,
            &mut hp,
            1000.0,
            0.33,
            SpaceDamageCategory::Building,
            &ai_hit,
        );
        assert!(!o_ai.destroyed);
        assert!((hp - 330.0).abs() < 1e-9);

        let human_hit = SpaceDamagePacket {
            attacker_is_ai: false,
            ..ai_hit
        };
        let o_human = apply_shield_and_hull_damage(
            &mut shield,
            &mut hp,
            1000.0,
            0.33,
            SpaceDamageCategory::Building,
            &human_hit,
        );
        assert!(o_human.destroyed);
        assert_eq!(hp, 0.0);
    }

    #[test]
    fn lock_on_supports_non_missile_lock_weapons_and_breaks_on_barrel_roll() {
        let mut coll = CollisionOwner::new();
        let empty_bvh = build_bvh(Positions::F32(&[]), &[]);
        let empty_pos = coll.positions(&[]);
        let empty_tree = coll.tree(empty_pos, &[], &empty_bvh);
        let world_id = coll.world_new(empty_tree, -1, -1) as usize;

        let weapon = SpaceWeaponSpec {
            ordnance_kind: SpaceOrdnanceKind::Bolt,
            lock_on_range: 600.0,
            lock_on_angle_deg: 8.0,
            lock_off_angle_deg: 12.0,
            lock_time: 0.2,
            target_lock_max_distance: 2000.0,
            target_lock_max_distance_lose: 2500.0,
            ..Default::default()
        };
        let mut lock = SpaceLockState::default();
        let building_target = [LockCandidate {
            kind: SpaceLockTargetKind::Object,
            index: 7,
            generation: 1,
            team: 2,
            alive: true,
            evading: false,
            position: [0.0, 0.0, 300.0],
            kernel_body: None,
        }];

        for _ in 0..15 {
            update_fighter_lock_on(
                &mut lock,
                1,
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                1,
                None,
                &weapon,
                1.0 / 60.0,
                true,
                &building_target,
                &mut coll,
                world_id,
            );
        }
        assert!(lock.locked);
        assert_eq!(lock.weapon_index, 1);
        assert_eq!(lock.kind, SpaceLockTargetKind::Object);
        assert_eq!(lock.target_index, 7);

        let mut fighter_target = [LockCandidate {
            kind: SpaceLockTargetKind::Fighter,
            index: 3,
            generation: 1,
            team: 2,
            alive: true,
            evading: false,
            position: [0.0, 0.0, 250.0],
            kernel_body: None,
        }];
        lock = SpaceLockState::default();
        for _ in 0..15 {
            update_fighter_lock_on(
                &mut lock,
                1,
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                1,
                None,
                &weapon,
                1.0 / 60.0,
                true,
                &fighter_target,
                &mut coll,
                world_id,
            );
        }
        assert!(lock.locked);
        fighter_target[0].evading = true;
        update_fighter_lock_on(
            &mut lock,
            1,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            1,
            None,
            &weapon,
            1.0 / 60.0,
            false,
            &fighter_target,
            &mut coll,
            world_id,
        );
        assert!(!lock.locked);
        assert_eq!(lock.kind, SpaceLockTargetKind::None);
    }
}
