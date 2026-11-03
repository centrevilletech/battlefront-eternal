//! Enhanced aim (src/ai/enhanced/aim.js): a reaction time per new target,
//! a wandering aim error that grows with range and the bot's own speed, an
//! acquisition bias that fades, a slew limit, and the player's spread rules
//! for a bot's weapons. Its draws come from its own generator (C), seeded
//! from the match seed. f64 throughout, as the JavaScript computes it.

use std::collections::HashMap;
use three::Vector3;

use super::aim::resolve;
use super::binding::CombatAi;
use super::weapons_ai::{consume_fire, def_at, selected_weapon, sniper_weapon, weapon_mut};
use super::world::{uniform, Gen, Obj, World};

const DEG: f64 = std::f64::consts::PI / 180.0;

/// aim.js:38 `AIM_PROFILES`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub reaction: f64,
    pub reaction_floor: f64,
    pub convergence: f64,
    pub sigma25: f64,
    pub slew: f64,
}

/// The tier's profile (1 Easy, 2 Normal, 3 Hard; others Normal).
pub fn profile(tier: f64) -> Profile {
    if tier == 1.0 {
        Profile { reaction: 0.45, reaction_floor: 0.3, convergence: 0.65, sigma25: 3.0, slew: 120.0 }
    } else if tier == 3.0 {
        Profile { reaction: 0.25, reaction_floor: 0.17, convergence: 0.35, sigma25: 1.0, slew: 200.0 }
    } else {
        Profile { reaction: 0.33, reaction_floor: 0.22, convergence: 0.48, sigma25: 1.6, slew: 160.0 }
    }
}

/// aim.js:44 `AIM_MODEL`.
const TAU: f64 = 0.25;
const BIAS: f64 = 2.5;
const REACTION_SPREAD: f64 = 0.25;
const MOVEMENT: f64 = 0.5;
const VARIATION: f64 = 0.1;
const RETAIN: f64 = 1.5;
const SCOPE: f64 = 0.5;

/// aim.js:64 `EnhancedAimer` (its generator is the battle's C).
#[derive(Clone, Debug, PartialEq)]
pub struct EnhancedAimer {
    pub profile: Profile,
    pub skill: f64,
    pub direction: Vector3,
    pub started: bool,
    pub error: [f64; 2],
    pub bias: [f64; 2],
    /// The target being aimed at, as `${id}:${generation}`.
    pub key: Option<String>,
    pub last_key: Option<String>,
    pub lost_at: f64,
    pub acquired_at: f64,
    pub ready_at: f64,
}

/// `${id}:${generation}` as JavaScript formats it.
fn key_of(id: f64, generation: f64) -> String {
    format!("{}:{}", jsrt::numfmt::number_to_string(id), jsrt::numfmt::number_to_string(generation))
}

impl EnhancedAimer {
    pub fn new(profile: Profile, skill: f64) -> EnhancedAimer {
        EnhancedAimer {
            profile,
            skill,
            direction: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
            started: false,
            error: [0.0, 0.0],
            bias: [0.0, 0.0],
            key: None,
            last_key: None,
            lost_at: f64::NEG_INFINITY,
            acquired_at: f64::NEG_INFINITY,
            ready_at: f64::NEG_INFINITY,
        }
    }

    /// aim.js:79 `gaussian()`.
    fn gaussian<W: World + ?Sized>(w: &mut W) -> f64 {
        let u = v8math::max(uniform(w, Gen::C), 1e-12);
        let v = uniform(w, Gen::C);
        v8math::sqrt(-2.0 * v8math::log(u)) * v8math::cos(2.0 * std::f64::consts::PI * v)
    }

    /// aim.js:85 `reset(direction)`.
    fn reset(&mut self, direction: &Vector3) {
        self.direction = *direction;
        self.started = true;
    }

    /// aim.js:92 `observe(target, now)`: a new target starts a reaction
    /// time and an acquisition bias.
    pub fn observe<W: World + ?Sized>(&mut self, w: &mut W, target: Option<Obj>, now: f64) {
        let key = target.map(|t| {
            let u = w.unit(t);
            key_of(u.id, u.generation)
        });
        if key == self.key {
            return;
        }
        let Some(key) = key else {
            self.last_key = self.key.take();
            self.lost_at = now;
            return;
        };
        self.key = Some(key.clone());
        if Some(&key) == self.last_key.as_ref() && now - self.lost_at < RETAIN {
            return;
        }
        let p = self.profile;
        self.acquired_at = now;
        let reaction = p.reaction * self.skill * v8math::exp(REACTION_SPREAD * Self::gaussian(w));
        self.ready_at = now + v8math::max(p.reaction_floor * self.skill, reaction);
        self.bias[0] = BIAS * Self::gaussian(w);
        self.bias[1] = BIAS * Self::gaussian(w);
    }

    /// aim.js:112 `reacting(now)`.
    pub fn reacting(&self, now: f64) -> bool {
        self.key.is_some() && now < self.ready_at
    }

    /// aim.js:118 `sigma(d, speedFraction, scoped)`.
    fn sigma(&self, d: f64, speed_fraction: f64, scoped: bool) -> f64 {
        let range = 0.7 + 0.3 * v8math::min(v8math::max(d / 25.0, 0.0), 4.0);
        let scope = if scoped { SCOPE } else { 1.0 };
        self.profile.sigma25 * DEG * self.skill * range * (1.0 + MOVEMENT * speed_fraction) * scope
    }

    /// aim.js:125 `update(desired, dt, options)`: the gun direction.
    #[allow(clippy::too_many_arguments)]
    pub fn update<W: World + ?Sized>(&mut self, w: &mut W, desired: &Vector3, dt: f64, distance: f64, speed_fraction: f64, scoped: bool, now: f64, hero: bool) -> Vector3 {
        if !self.started {
            self.reset(desired);
        }
        if hero || !(dt > 0.0) {
            if hero {
                self.direction = *desired;
            }
            return self.direction;
        }
        let a = v8math::exp(-dt / TAU);
        let b = v8math::sqrt(1.0 - a * a);
        self.error[0] = a * self.error[0] + b * Self::gaussian(w);
        self.error[1] = a * self.error[1] + b * Self::gaussian(w);
        let fade = v8math::exp((-3.0 * v8math::max(0.0, now - self.acquired_at)) / self.profile.convergence);
        let s = self.sigma(distance, speed_fraction, scoped);
        let yaw = s * (self.error[0] + self.bias[0] * fade);
        let pitch = s * (self.error[1] + self.bias[1] * fade);
        let up = Vector3 { x: 0.0, y: 1.0, z: 0.0 };
        let mut right = Vector3::default();
        right.cross_vectors(desired, &up);
        if right.length_sq() < 1e-8 {
            right.set(1.0, 0.0, 0.0);
        }
        right.normalize();
        let mut upv = Vector3::default();
        upv.cross_vectors(&right, desired);
        upv.normalize();
        let mut aimed = *desired;
        aimed.add_scaled_vector(&right, v8math::tan(yaw));
        aimed.add_scaled_vector(&upv, v8math::tan(pitch));
        aimed.normalize();
        let angle = self.direction.angle_to(&aimed);
        let step = self.profile.slew * DEG * dt;
        if angle <= step || !angle.is_finite() {
            self.direction = aimed;
        } else {
            self.direction.lerp(&aimed, step / angle);
            self.direction.normalize();
        }
        if !self.direction.x.is_finite() {
            self.direction = *desired;
        }
        self.direction
    }
}

/// aim.js:174 `playerSpread(weapon)`: no extra spread or sniper error, and
/// the player's spread floor.
pub fn player_spread<W: World + ?Sized>(w: &mut W, unit: Obj, index: usize) {
    let def = def_at(w, unit, index);
    let wv = weapon_mut(w, unit, index);
    wv.spread_scale = 1.0;
    wv.sniper_scale = Some(0.0);
    if !wv.enhanced_floor {
        wv.enhanced_floor = true;
        let t = def.get("SpreadThreshold");
        let floor = -(if t.truthy() { t.num() } else { 0.0 });
        if floor < wv.floor {
            wv.floor = floor;
            wv.kick = v8math::min(wv.kick, floor);
        }
    }
}

/// aim.js:192 `enhancedCombat(world, { tier, seed })`'s state: the profile,
/// the Characters' skills, the hook flag. Its generator is the world's C,
/// `ClassicRandom((seed ^ 0x51ed270b) >>> 0)`.
#[derive(Clone, Debug, PartialEq)]
pub struct EnhancedCombat {
    pub profile: Profile,
    /// aim.js:197 `skills`: a stable skill factor per Character.
    pub skills: HashMap<Obj, f64>,
    pub hooked: bool,
}

/// The seed of the Enhanced aim's generator (aim.js:195).
pub fn seed_c(seed: f64) -> u32 {
    v8math::to_uint32(seed) ^ 0x51ed_270b
}

impl EnhancedCombat {
    pub fn new(tier: f64) -> EnhancedCombat {
        EnhancedCombat { profile: profile(tier), skills: HashMap::new(), hooked: false }
    }

    /// aim.js:201 `aimerOf(controller)`: made at first ask, with its
    /// Character's skill (drawn once a Character).
    fn ensure_aimer<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) -> bool {
        if w.ctl(controller).perception.is_none() {
            return false;
        }
        if w.ctl(controller).perception.as_ref().unwrap().enhanced.is_none() {
            let character = w.ctl(controller).character;
            let mut skill = character.and_then(|c| self.skills.get(&c).copied());
            if skill.is_none() {
                let s = 1.0 + VARIATION * (2.0 * uniform(w, Gen::C) - 1.0);
                if let Some(c) = character {
                    self.skills.insert(c, s);
                }
                skill = Some(s);
            }
            let aimer = EnhancedAimer::new(self.profile, skill.unwrap());
            w.ctl_mut(controller).perception.as_mut().unwrap().enhanced = Some(aimer);
        }
        true
    }

    fn with_aimer<W: World + ?Sized, R>(w: &mut W, controller: Obj, f: impl FnOnce(&mut W, &mut EnhancedAimer) -> R) -> R {
        let mut aimer = w.ctl_mut(controller).perception.as_mut().unwrap().enhanced.take().unwrap();
        let r = f(w, &mut aimer);
        w.ctl_mut(controller).perception.as_mut().unwrap().enhanced = Some(aimer);
        r
    }

    /// aim.js:214 `updateAim(controller, dt)`.
    pub fn update_aim<W: World + ?Sized>(&mut self, w: &mut W, cx: &CombatAi, controller: Obj, dt: f64) -> Option<Vector3> {
        let unit = w.ctl(controller).unit?;
        if !w.unit(unit).alive || w.ctl(controller).perception.is_none() {
            return None;
        }
        self.ensure_aimer(w, controller);
        let target = resolve(w, w.ctl(controller).target);
        let now = w.time();
        Self::with_aimer(w, controller, |w, a| a.observe(w, target, now));
        let desired = w.forward(unit);
        let weapon = selected_weapon(w, unit, Some(controller));
        if let Some(i) = weapon {
            player_spread(w, unit, i);
        }
        let mut distance = 25.0;
        if let Some(t) = target {
            let origin = w.origin(unit);
            let point = w.aim_point(t);
            distance = origin.distance_to(&point);
        }
        let u = w.unit(unit);
        let top_soldier = u.soldier.as_ref().map(|s| s.max_speed.clone()).unwrap_or_default();
        let top = if top_soldier.truthy() {
            top_soldier.num()
        } else {
            let c = u.class.as_ref().map(|c| c.max_speed.clone()).unwrap_or_default();
            if c.truthy() {
                c.num()
            } else {
                0.0
            }
        };
        let speed = match u.velocity {
            Some(v) => v8math::hypot(v.x, v.z),
            None => 0.0,
        };
        let scoped = match weapon {
            Some(i) => u.kind == 0.0 && sniper_weapon(&cx.shared.weapons, Some(&def_at(w, unit, i))),
            None => false,
        };
        let hero = w.unit(unit).hero;
        let speed_fraction = if top > 0.0 { v8math::min(speed / top, 1.0) } else { 0.0 };
        let now = w.time();
        let direction = Self::with_aimer(w, controller, |w, a| a.update(w, &desired, dt, distance, speed_fraction, scoped, now, hero));
        // The fire cone reads the Classic Aimer's direction: keep it the gun's.
        let p = w.ctl_mut(controller).perception.as_mut().unwrap();
        p.aimer.direction = direction;
        p.aimer.velocity = Vector3::default();
        p.aimer.started = true;
        Some(direction)
    }

    /// aim.js:236 `consumeFire(controller, unit, step, intent)`: held while
    /// reacting.
    pub fn consume_fire<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, controller: Obj, unit: Obj, step: f64, intent: bool) {
        if self.ensure_aimer(w, controller) {
            let target = resolve(w, w.ctl(controller).target);
            let now = w.time();
            let reacting = Self::with_aimer(w, controller, |w, a| {
                a.observe(w, target, now);
                a.reacting(now)
            });
            if reacting {
                let c = w.ctl_mut(controller);
                c.fire_a = false;
                c.fire_b = false;
            }
        }
        consume_fire(w, &mut cx.shared, controller, Some(unit));
        if !self.hooked && intent {
            let aim = self.update_aim(w, cx, controller, step);
            w.set_intent_aim(unit, aim);
        }
    }

    /// aim.js:248 `aim(controller, dt)`.
    pub fn aim<W: World + ?Sized>(&mut self, w: &mut W, cx: &CombatAi, controller: Obj, dt: f64) -> Option<Vector3> {
        self.hooked = true;
        let direction = self.update_aim(w, cx, controller, dt);
        if let Some(u) = w.ctl(controller).unit {
            if w.unit(u).intent {
                w.set_intent_aim(u, direction);
            }
        }
        direction
    }
}
