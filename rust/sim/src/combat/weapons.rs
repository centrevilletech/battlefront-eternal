//! Weapons (src/weapons.js Weapon and Arsenal, with src/heroes.js's melee
//! swing and src/seats.js seatWeaponPolicy): a weapon's clip, reserve, heat
//! and kick, its trigger's timers, salvos, throws and charges, the spread
//! each round draws, a launcher's lock, and a soldier's two channels. The
//! same code serves a soldier's weapons, a seat's and a turret's: a weapon
//! is the store's record (`gen::Weapon`, rust/sim/schema/combat.mjs) held by
//! value by its owner, named by its place among its owner's weapons.
//!
//! Each function is its JavaScript's operation by operation (src/weapons.js
//! line numbers on rust/sim-p11-combat): `x || d` and `x ?? d` on the
//! class's fields read as [`Js::or`] and [`Js::nn`], Math.min/max as
//! jsrt's, a press's seeded draws as mulberry32 (jsrt::rng::Mulberry32).

use super::defs::{Js, Level, WeaponClass, WeaponDefs, K};
use crate::gen::{Melee, Posture, Weapon};
use crate::store::DefId;
use jsrt::conv::{js_max, js_min, js_trunc};
use jsrt::rng::Mulberry32;

/// Degrees to radians (`Math.PI / 180`).
pub const DEG: f64 = std::f64::consts::PI / 180.0;
/// s; a press landing on the end of the lockout counts (weapons.js:70).
pub const EPSILON: f64 = 1e-6;

/// A trigger as a weapon's update reads it: held, pressed this tick, the
/// press's spread seed (a network player's predicted press, spreadSeed) and
/// how late the press reached the authority (s).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trigger {
    pub held: bool,
    pub pressed: bool,
    pub seed: Option<f64>,
    pub late: f64,
}

/// What a weapon's update reads of its holder: the stick's push, his
/// posture (undefined for a droideka: it spreads as standing), whether he
/// is zoomed, and whether a repair tool has a target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub stick: f64,
    pub posture: Option<Posture>,
    pub zoomed: bool,
    pub repair_target: bool,
}

/// What an authority says of a shot it may refuse (weapon.admitShot: a
/// recon droid's launch, counted by the network's world): yes, no, or a
/// token the launch hands back (its slot, by the world's numbering).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Admission {
    Yes,
    No,
    Token(f64),
}

/// A shot (src/weapons.js:487, a repair tick :293, a melee swing's
/// steps src/heroes.js:44-58), its weapon by its place among its owner's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub weapon: u32,
    pub kind: ShotKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShotKind {
    /// A round: its spread (and pattern) offsets in radians, the charge it
    /// was fired at, a launcher's lock (target id, or -1, and generation),
    /// and an admitted launch's token.
    Round { yaw: f64, pitch: f64, charge: f64, target_id: f64, target_generation: f64, admission: Option<f64> },
    Repair,
    /// A swing begins (its number, from 1).
    MeleeStart { swing: f64 },
    MeleeHit,
    MeleeEnd,
}

/// Where a weapon's draws come from: its generator (weapon.random: the
/// AI's, or a world stream), the legacy spread stream (Math.random) for one
/// without, and the authority that admits its shots. A press's seeded draws
/// are the weapon's own (salvoRandom); `seeded` tells the caller of each.
pub trait WeaponDraws {
    /// weapon.random() (the weapon at `weapon` among its owner's).
    fn draw(&mut self, weapon: u32) -> f64;
    /// The legacy spread stream's next number (a weapon with no generator).
    fn legacy(&mut self, weapon: u32) -> f64;
    /// A seeded draw was made (its seed and value).
    fn seeded(&mut self, seed: f64, value: f64);
    /// weapon.admitShot(weapon), for a weapon that has one.
    fn admit(&mut self, weapon: u32) -> Admission;
    /// A level assignment occurred, including a write of the current level.
    /// Private checkpoints can restore a definition without its numeric field.
    fn level_written(&mut self, _weapon: u32, _level: f64) {}
    /// Ordinary authority callbacks can inspect and change the weapon at
    /// the call site. Private and replay hosts keep the original callbacks.
    /// `first` captures one round's generator; nested updates need their
    /// own invocation frame so they cannot replace an outer round's `r`.
    fn draw_visible(&mut self, weapon: u32, _state: &mut Weapon, _first: bool) -> f64 { self.draw(weapon) }
    fn legacy_visible(&mut self, weapon: u32, _state: &mut Weapon) -> f64 { self.legacy(weapon) }
    fn admit_visible(&mut self, weapon: u32, _state: &mut Weapon) -> Admission { self.admit(weapon) }
    /// A new press replaces its retained generator before any admission.
    fn salvo_started(&mut self, _weapon: u32, _state: &Weapon) {}
    /// The authority may retain a separately consumed seeded closure.
    /// Default hosts retain the original diagnostic event and value.
    fn seeded_visible(&mut self, _weapon: u32, _state: &mut Weapon, _first: bool, seed: f64, value: f64) -> f64 {
        self.seeded(seed, value); value
    }
}

/// `a < b` where `a` is a class's field (ToNumber; undefined is NaN).
#[inline]
fn lt(a: &Js, b: f64) -> bool {
    a.num() < b
}

impl Weapon {
    /// The class at the level it is at (`this.def`).
    #[inline]
    pub fn level<'d>(&self, class: &'d WeaponClass) -> &'d Level {
        &class.levels[self.level as usize]
    }

    /// `new Weapon(def, { player })` (weapons.js:120): its generator none
    /// (random), which the owner binds.
    pub fn create(def: DefId, defs: &WeaponDefs, player: bool) -> Weapon {
        let d = defs.class(def).base();
        let rounds = d.get(K::RoundsPerClip).num();
        let bottomless = !(rounds >= 1.0);
        let ammo = d.get(K::Ammo).num();
        let floor = if player { -d.get(K::SpreadThreshold).or(0.0) } else { 0.0 };
        Weapon {
            def,
            level: 0.0,
            admit_shot: false,
            deployed: false,
            firing: false,
            timer: 0.0,
            bottomless,
            per_shot: if bottomless { 0.0 } else { 1.0 / (rounds + 0.0001) },
            clip: 1.0,
            reserve: if ammo > 0.0 { ammo - 1.0 } else { f64::INFINITY },
            heat: 0.0,
            overheated: false,
            floor,
            kick: floor,
            random: false,
            spread_scale: 1.0,
            sniper_scale: 0.0,
            yaw_spread_override: None,
            wait: 0.0,
            reload_left: 0.0,
            throw_time: -1.0,
            released: false,
            hidden: false,
            charging: false,
            held: 0.0,
            charge: 1.0,
            limit: 1.0,
            pattern: 0.0,
            salvos_left: 0.0,
            salvo_wait: 0.0,
            salvo_seed: None,
            salvo_count: 0.0,
            lock_target_id: -1.0,
            lock_generation: -1.0,
            lock_elapsed: 0.0,
            lock_complete: false,
            melee: None,
        }
    }

    /// prepareMelee (heroes.js:17) on one weapon: a melee weapon's clip is
    /// bottomless and its update is its swing's.
    pub fn prepare_melee(&mut self, class: &WeaponClass) {
        if !class.base().get(K::Base).is_str("melee") {
            return;
        }
        self.bottomless = true;
        self.clip = 1.0;
        self.reserve = f64::INFINITY;
        self.melee = Some(Melee { time: -1.0, index: 0.0, active: false });
    }

    pub fn channel(&self, defs: &WeaponDefs) -> f64 {
        defs.class(self.def).base().get(K::Channel).num()
    }

    /// Whether `def.channel === c`.
    pub fn in_channel(&self, defs: &WeaponDefs, c: f64) -> bool {
        matches!(defs.class(self.def).base().get(K::Channel), Js::Num(x) if *x == c)
    }

    /// Thrown: grenades, dispensers' mines and power-ups, recon droids.
    pub fn thrown(&self, defs: &WeaponDefs) -> bool {
        let base = defs.class(self.def).base().get(K::Base);
        base.is_str("grenade") || base.is_str("dispenser") || base.is_str("remote")
    }

    pub fn repair(&self, defs: &WeaponDefs) -> bool {
        defs.class(self.def).base().get(K::Base).is_str("repair")
    }

    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }

    pub fn busy(&self) -> bool {
        self.throw_time >= 0.0 || self.firing || self.salvos_left > 0.0
    }

    pub fn can_shoot(&self) -> bool {
        self.bottomless || self.clip >= self.per_shot - 1e-6
    }

    pub fn empty(&self) -> bool {
        !self.can_shoot() && self.clip + self.reserve < self.per_shot - 1e-6
    }

    pub fn hand_empty(&self) -> bool {
        self.hidden
    }

    pub fn base_level(&mut self) {
        self.level = 0.0;
    }

    /// deselect() (weapons.js:208).
    pub fn deselect(&mut self) {
        self.clear_lock();
        self.reload_left = 0.0;
        self.charging = false;
        self.held = 0.0;
        if !self.busy() {
            self.base_level();
        }
    }

    /// clearLock() (weapons.js:216).
    pub fn clear_lock(&mut self) {
        self.lock_target_id = -1.0;
        self.lock_generation = -1.0;
        self.lock_elapsed = 0.0;
        self.lock_complete = false;
    }

    /// updateLock(dt, target) (weapons.js:225): launcher state 4. `target`:
    /// the picked unit's id, generation and whether it lives, or none.
    pub fn update_lock(&mut self, class: &WeaponClass, dt: f64, target: Option<LockTarget>) {
        let d = self.level(class);
        let launcher = d.get(K::Base).is_str("launcher") && d.get(K::Ordnance).get("base").is_str("missile");
        let Some(t) = target.filter(|t| launcher && t.alive) else {
            self.clear_lock();
            return;
        };
        if self.lock_target_id != t.id || self.lock_generation != t.generation {
            self.clear_lock();
            self.lock_target_id = t.id;
            self.lock_generation = t.generation;
        }
        if self.busy() || self.reloading() || self.wait > EPSILON {
            return;
        }
        let lock_time = d.get(K::LockTime);
        let duration = if lock_time.num() > 0.0 { lock_time.num() } else { d.get(K::MaxPressedTime).or(0.0) };
        self.lock_elapsed = js_min(duration, self.lock_elapsed + dt);
        self.lock_complete = duration > 0.0 && self.lock_elapsed >= duration - EPSILON;
    }

    /// startReload() (weapons.js:241).
    pub fn start_reload(&mut self, class: &WeaponClass) -> bool {
        if self.bottomless || self.clip >= 1.0 || self.reloading() {
            return false;
        }
        if self.clip + self.reserve < self.per_shot - 1e-6 {
            return false;
        }
        self.reserve += self.clip;
        self.clip = 0.0;
        self.reload_left = js_max(self.level(class).get(K::ReloadTime).num(), 0.01);
        self.limit = self.reload_left;
        true
    }

    /// spread(stick, posture, zoomed) (weapons.js:252): degrees per axis.
    pub fn spread(&self, class: &WeaponClass, stick: f64, posture: Option<Posture>, zoomed: bool) -> [f64; 2] {
        let d = self.level(class);
        let (still_k, aim_k, move_k) = match posture {
            Some(Posture::Crouch) => (K::CrouchStillSpread, K::CrouchAimSpread, K::CrouchMoveSpread),
            Some(Posture::Prone) => (K::ProneStillSpread, K::ProneAimSpread, K::ProneMoveSpread),
            _ => (K::StandStillSpread, K::StandAimSpread, K::StandMoveSpread),
        };
        let still = if zoomed {
            let aimed = d.get(aim_k);
            if aimed.is_nullish() {
                d.get(still_k)
            } else {
                aimed
            }
        } else {
            d.get(still_k)
        };
        let mv = d.get(move_k).nn(0.0);
        let s = still.nn(0.0);
        let pose = s + (mv - s) * js_min(stick, 1.0);
        let kick = js_max(self.kick, 0.0);
        let yaw = match self.yaw_spread_override {
            Some(v) => v,
            None => d.get(K::YawSpread).or(0.0),
        };
        [kick + pose + yaw, kick + pose + d.get(K::PitchSpread).or(0.0)]
    }

    /// climb(most) (weapons.js:422): up through the charge levels the held
    /// time has passed.
    fn climb(&mut self, class: &WeaponClass, most: f64) {
        let mut level = self.level as usize;
        while level + 1 < class.levels.len() && lt(class.levels[level + 1].get(K::At), self.held / most) {
            level += 1;
        }
        self.level = level as f64;
    }

    /// The press's seeded generator's number `n` (from 0: salvoRandom(),
    /// seededRandom(salvoSeed) drawn n + 1 times).
    fn seeded_draw(seed: f64, n: f64) -> (f64, f64) {
        let seed = jsrt::conv::to_uint32(seed) as f64;
        let mut m = Mulberry32::new(seed);
        let mut v = 0.0;
        for _ in 0..=(n as u64) {
            v = m.next_f64();
        }
        (seed, v)
    }

    /// The `k`th of a round's four draws: its press's (salvoCount was
    /// counted on by four as the round began), else its generator's.
    fn salvo_draw(&mut self, index: u32, k: f64, seed: Option<f64>, start: f64, draws: &mut dyn WeaponDraws) -> f64 {
        if let Some(seed) = seed {
            let (seed, value) = Self::seeded_draw(seed, start + k);
            draws.seeded_visible(index, self, k == 0.0, seed, value)
        } else {
            draws.draw_visible(index, self, k == 0.0)
        }
    }

    /// shoot(shots, aim) (weapons.js:458): a salvo: the ammo spent, the heat,
    /// each round's offsets, then the kick.
    fn shoot(&mut self, index: u32, class: &WeaponClass, aim: &Aim, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        let d = self.level(class);
        let count = js_max(d.get(K::ShotsPerSalvo).or(1.0), 1.0);
        let [yaw, pitch] = self.spread(class, aim.stick, aim.posture, aim.zoomed);
        let pattern = d.get(K::ShotPatternPitchYaw);
        let length = pattern.len() as f64;
        let entries = if pattern.len() > 0 { js_min(d.get(K::ShotPatternCount).or(length), length) } else { 0.0 };
        let first = if entries != 0.0 { length - entries } else { 0.0 };
        let target_id = if self.lock_complete { self.lock_target_id } else { -1.0 };
        let target_generation = self.lock_generation;
        let before = out.len();
        let mut i = 0.0;
        while i < count && self.can_shoot() {
            let admission = if self.admit_shot { draws.admit_visible(index, self) } else { Admission::Yes };
            if admission == Admission::No {
                break;
            }
            let admitted = match admission {
                Admission::Token(t) => Some(t),
                _ => None,
            };
            if !self.bottomless {
                self.clip = js_max(self.clip - self.per_shot, 0.0);
            }
            self.heat += d.get(K::HeatPerShot).or(0.0);
            let (mut down, mut aside) = (0.0, 0.0);
            if entries != 0.0 {
                let entry = pattern.at((first + jsrt::conv::js_mod(self.pattern, entries)) as usize);
                down = entry.at(0).num();
                aside = entry.at(1).num();
                self.pattern = jsrt::conv::js_mod(self.pattern + 1.0, entries);
            }
            // JavaScript retains `r` for all four draws of this round.
            // A callback can re-enter and replace the weapon's generator.
            let seed = self.salvo_seed;
            let start = self.salvo_count;
            if seed.is_some() || self.random {
                // A press's seeded draws or the AI's generator, in the
                // original game's order: pitch, then yaw.
                if seed.is_some() {
                    self.salvo_count += 4.0;
                }
                let a = self.salvo_draw(index, 0.0, seed, start, draws);
                let tipped = a - self.salvo_draw(index, 1.0, seed, start, draws);
                let b = self.salvo_draw(index, 2.0, seed, start, draws);
                let turned = b - self.salvo_draw(index, 3.0, seed, start, draws);
                out.push(Shot { weapon: index, kind: ShotKind::Round { yaw: (aside + yaw * turned) * DEG, pitch: (-down + pitch * tipped) * DEG, charge: self.charge, target_id, target_generation, admission: admitted } });
                i += 1.0;
                continue;
            }
            let a = draws.legacy_visible(index, self);
            let turned = a - draws.legacy_visible(index, self);
            let shot_yaw = (aside + yaw * turned) * DEG;
            let b = draws.legacy_visible(index, self);
            let tipped = b - draws.legacy_visible(index, self);
            out.push(Shot { weapon: index, kind: ShotKind::Round { yaw: shot_yaw, pitch: (-down + pitch * tipped) * DEG, charge: self.charge, target_id, target_generation, admission: admitted } });
            i += 1.0;
        }
        if self.admit_shot && out.len() == before {
            return;
        }
        self.kick = js_min(self.kick + d.get(K::SpreadPerShot).or(0.0) * self.spread_scale, d.get(K::SpreadLimit).nn(f64::INFINITY));
    }

    /// startBurst(shots, aim, seed) (weapons.js:397).
    fn start_burst(&mut self, index: u32, class: &WeaponClass, aim: &Aim, seed: Option<f64>, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        self.salvo_seed = seed;
        self.salvo_count = 0.0;
        draws.salvo_started(index, self);
        let d = self.level(class);
        self.salvos_left = js_max(1.0, jsrt::conv::js_floor(d.get(K::SalvoCount).or(1.0)));
        self.salvo_wait = js_max(0.0, d.get(K::InitialSalvoDelay).or(0.0));
        self.limit = d.get(K::ShotDelay).or(0.0);
        self.salvo_step(index, class, aim, false, draws, out);
    }

    /// salvoStep(shots, aim, cooldown) (weapons.js:405).
    fn salvo_step(&mut self, index: u32, class: &WeaponClass, aim: &Aim, cooldown: bool, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        while self.salvos_left > 0.0 && self.salvo_wait <= EPSILON {
            if !self.can_shoot() {
                self.salvos_left = 0.0;
                break;
            }
            self.shoot(index, class, aim, draws, out);
            self.salvos_left -= 1.0;
            self.salvo_wait += js_max(0.0, self.level(class).get(K::SalvoDelay).or(0.0));
        }
        if self.salvos_left == 0.0 {
            let d = self.level(class);
            if cooldown {
                self.wait = js_max(self.wait, self.salvo_wait + d.get(K::ShotDelay).or(0.0));
            }
            if d.get(K::Base).is_str("launcher") && !matches!(d.get(K::ClearLockOnFire), Js::Num(x) if *x == 0.0) {
                self.lock_elapsed = 0.0;
                self.lock_complete = false;
            }
        }
    }

    /// startThrow(shots, aim) (weapons.js:430).
    fn start_throw(&mut self, index: u32, class: &WeaponClass, aim: &Aim, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        let d = self.level(class);
        self.throw_time = 0.0;
        self.released = false;
        self.wait = d.get(K::InitialSalvoDelay).num() + d.get(K::ShotDelay).num();
        self.limit = d.get(K::ShotDelay).or(0.0);
        self.throw_step(index, class, aim, draws, out);
    }

    /// throwStep(shots, aim) (weapons.js:438).
    fn throw_step(&mut self, index: u32, class: &WeaponClass, aim: &Aim, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        let d = self.level(class);
        let initial = d.get(K::InitialSalvoDelay).num();
        let shot_delay = d.get(K::ShotDelay).num();
        let hide = !matches!(d.get(K::HideOnFire), Js::Num(x) if *x == 0.0);
        if !self.released && self.throw_time >= initial {
            let before = out.len();
            self.shoot(index, class, aim, draws, out);
            if self.admit_shot && out.len() == before {
                self.throw_time = -1.0;
                self.wait = 0.0;
                self.hidden = false;
                self.base_level();
                draws.level_written(index, self.level);
                return;
            }
            self.released = true;
            self.hidden = hide;
        }
        if self.throw_time >= initial + shot_delay {
            self.throw_time = -1.0;
            if self.can_shoot() {
                self.hidden = false;
            }
            self.base_level();
            draws.level_written(index, self.level);
        }
    }

    /// update(dt, selected, trigger, reload, aim) (weapons.js:265): one tick;
    /// the shots fired go to `out`. A melee weapon's update is its swing's
    /// (heroes.js:39).
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, index: u32, class: &WeaponClass, dt: f64, selected: bool, trigger: &Trigger, reload: bool, aim: &Aim, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        if self.melee.is_some() {
            self.melee_update(index, class, dt, selected, trigger, draws, out);
            return;
        }
        let d = self.level(class);
        let heat_recover = d.get(K::HeatRecoverRate).or(0.0);
        let heat_threshold = d.get(K::HeatThreshold).nn(1.0);
        let spread_recover = d.get(K::SpreadRecoverRate).or(0.0);
        let shot_delay = d.get(K::ShotDelay).clone();
        if self.heat >= 1.0 {
            self.overheated = true;
        }
        self.heat = js_max(self.heat - heat_recover * dt, 0.0);
        if self.overheated && self.heat <= heat_threshold {
            self.overheated = false;
        }
        self.kick = js_max(self.kick - spread_recover * dt, self.floor);
        self.wait -= dt;
        if !selected {
            self.clear_lock();
            self.salvos_left = 0.0;
            if self.wait < 0.0 {
                self.wait = 0.0;
            }
            self.firing = false;
            return;
        }

        if self.salvos_left > 0.0 {
            self.salvo_wait -= dt;
            self.salvo_step(index, class, aim, true, draws, out);
            return;
        }

        if self.repair_of(class) {
            let can = trigger.held && !self.overheated && aim.repair_target;
            if self.firing {
                self.timer += dt;
            } else if can {
                self.firing = true;
                self.timer = 0.0;
                out.push(Shot { weapon: index, kind: ShotKind::Repair });
            }
            while self.firing && self.timer >= shot_delay.num() {
                if can {
                    self.timer -= shot_delay.num();
                    out.push(Shot { weapon: index, kind: ShotKind::Repair });
                } else {
                    self.firing = false;
                }
            }
            return;
        }

        if self.throw_time >= 0.0 {
            self.throw_time += dt;
            self.throw_step(index, class, aim, draws, out);
            return;
        }

        if self.reloading() {
            self.reload_left -= dt;
            if self.reload_left <= 0.0 {
                self.reload_left = 0.0;
                let take = js_min(1.0, self.reserve);
                self.clip = take;
                self.reserve -= take;
                self.hidden = false;
            }
            if self.wait < 0.0 {
                self.wait = 0.0;
            }
            return;
        }
        if !self.can_shoot() || (reload && self.clip < 1.0 && self.reserve >= self.per_shot) {
            self.start_reload(class);
            if self.wait < 0.0 {
                self.wait = 0.0;
            }
            return;
        }

        let single = d.get(K::TriggerSingle).num() > 0.0;
        let most = class.base().get(K::MaxPressedTime).or(0.0);
        let thrown = {
            let base = class.base().get(K::Base);
            base.is_str("grenade") || base.is_str("dispenser") || base.is_str("remote")
        };
        if most > 0.0 {
            if self.charging {
                if trigger.held {
                    self.held = js_min(self.held + dt, most);
                    self.climb(class, most);
                    draws.level_written(index, self.level);
                    return;
                }
                // Let go: fire at the charge reached.
                self.charging = false;
                self.charge = self.held / most;
                self.held = 0.0;
                if thrown {
                    self.start_throw(index, class, aim, draws, out);
                } else {
                    self.start_burst(index, class, aim, None, draws, out);
                    self.wait = self.level(class).get(K::ShotDelay).num();
                }
                return;
            }
            if !self.busy() {
                self.base_level();
                draws.level_written(index, self.level);
            }
            if trigger.held && self.wait <= EPSILON && !self.overheated {
                self.charging = true;
                self.limit = most;
                self.held = js_min(dt, most);
                self.climb(class, most);
                draws.level_written(index, self.level);
            }
            if self.wait < 0.0 {
                self.wait = 0.0;
            }
            return;
        }
        if thrown {
            if trigger.pressed && self.wait <= EPSILON && !self.deployed {
                self.charge = 1.0;
                self.start_throw(index, class, aim, draws, out);
            }
            return;
        }
        if self.overheated {
            if self.wait < 0.0 {
                self.wait = 0.0;
            }
            return;
        }
        if single {
            if trigger.pressed && self.wait <= EPSILON {
                self.start_burst(index, class, aim, trigger.seed, draws, out);
                let late = if trigger.late != 0.0 && !trigger.late.is_nan() { trigger.late } else { 0.0 };
                self.wait = shot_delay.num() - js_min(late, shot_delay.or(0.0) * 0.5);
            }
        } else if trigger.held {
            if trigger.pressed && self.wait < 0.0 {
                self.wait = 0.0;
            }
            // Only the press's own cycle has its seed; held repeats draw as before.
            let mut seed = if trigger.pressed { trigger.seed } else { None };
            while self.wait <= EPSILON && self.can_shoot() {
                self.start_burst(index, class, aim, seed, draws, out);
                seed = None;
                self.wait += js_max(shot_delay.or(0.0), EPSILON);
                if self.salvos_left > 0.0 || !(shot_delay.num() > 0.0) {
                    break;
                }
            }
        }
        if self.wait < 0.0 {
            self.wait = 0.0;
        }
    }

    fn repair_of(&self, class: &WeaponClass) -> bool {
        class.base().get(K::Base).is_str("repair")
    }

    /// MeleeSwing.update(dt, selected, trigger) (heroes.js:39): a press
    /// starts a swing; holding and presses during it do nothing. The entry
    /// update sets the time to zero without advancing it; the hit lands
    /// when old <= hit < old + dt; the swing ends strictly past its length.
    fn melee_update(&mut self, index: u32, class: &WeaponClass, dt: f64, selected: bool, trigger: &Trigger, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        let d = self.level(class);
        let swings = melee_swings(d);
        let mut m = self.melee.clone().expect("a melee weapon's swing");
        if m.active && m.time > swings[m.index as usize].duration.num() {
            m.active = false;
            self.firing = false;
            out.push(Shot { weapon: index, kind: ShotKind::MeleeEnd });
        }
        if !m.active && selected && trigger.pressed {
            self.melee = Some(m);
            let r = if self.random { draws.draw_visible(index, self, true) } else { draws.legacy_visible(index, self) };
            m = self.melee.clone().expect("a melee weapon's swing");
            let n = swings.len() as f64;
            m.index = js_max(0.0, js_min(n - 1.0, js_trunc(r * n - 0.001)));
            m.active = true;
            m.time = -1.0;
            self.firing = true;
            out.push(Shot { weapon: index, kind: ShotKind::MeleeStart { swing: m.index + 1.0 } });
        }
        if m.active {
            if m.time < 0.0 {
                m.time = 0.0;
            } else {
                let before = m.time;
                m.time += js_max(dt, 0.0);
                let hit = swings[m.index as usize].hit.num();
                if before <= hit && hit < m.time {
                    out.push(Shot { weapon: index, kind: ShotKind::MeleeHit });
                }
            }
        }
        self.melee = Some(m);
    }
}

/// A melee swing's length and hit time.
struct Swing {
    duration: Js,
    hit: Js,
}

/// meleeSwings(weapon) (heroes.js:30): its class's swings, up to three, or
/// one second-long swing that hits halfway.
fn melee_swings(d: &Level) -> Vec<Swing> {
    let swings = d.field("swings");
    if swings.len() > 0 {
        (0..swings.len().min(3)).map(|i| Swing { duration: swings.at(i).get("duration").clone(), hit: swings.at(i).get("hit").clone() }).collect()
    } else {
        vec![Swing { duration: Js::Num(1.0), hit: Js::Num(0.5) }]
    }
}

/// What a lock is on (a unit's id, generation and whether it lives).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LockTarget {
    pub id: f64,
    pub generation: f64,
    pub alive: bool,
}

/// seatWeaponPolicy(weapon, human, random) (seats.js:76): a seat's weapon
/// as its occupant fires it.
pub fn seat_weapon_policy(w: &mut Weapon, defs: &WeaponDefs, human: bool, random: bool) {
    w.floor = if human { -defs.class(w.def).level_of(w).get(K::SpreadThreshold).or(0.0) } else { 0.0 };
    if w.kick < w.floor {
        w.kick = w.floor;
    }
    w.spread_scale = 1.0;
    w.sniper_scale = 0.0;
    w.yaw_spread_override = None;
    w.random = random;
}

impl WeaponClass {
    /// The level a weapon of this class is at.
    pub fn level_of(&self, w: &Weapon) -> &Level {
        &self.levels[w.level as usize]
    }
}

/// An arsenal's two channels (src/weapons.js Arsenal): its weapons, in
/// order, and each channel's selection (an index among that channel's).
pub struct Arsenal<'a> {
    pub weapons: &'a mut [Weapon],
    pub selected: &'a mut [f64; 2],
}

/// The input an arsenal's update reads (combat-weapon-input.js
/// readWeaponInput, with each press's seed and lateness).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ArsenalInput {
    pub primary: Trigger,
    pub secondary: Trigger,
    pub reload: bool,
}

/// `new Arsenal(loadout, { player })` with prepareMelee: its weapons.
pub fn arm(loadout: &[DefId], defs: &WeaponDefs, player: bool) -> Vec<Weapon> {
    loadout
        .iter()
        .map(|&def| {
            let mut w = Weapon::create(def, defs, player);
            w.prepare_melee(defs.class(def));
            w
        })
        .collect()
}

impl Arsenal<'_> {
    /// The weapons of a channel (`channels[c]`): their places, in order.
    pub fn channel(&self, defs: &WeaponDefs, c: usize) -> Vec<usize> {
        (0..self.weapons.len()).filter(|&i| self.weapons[i].in_channel(defs, c as f64)).collect()
    }

    /// current(channel) (weapons.js:511): the selected weapon's place.
    pub fn current(&self, defs: &WeaponDefs, c: usize) -> Option<usize> {
        let list = self.channel(defs, c);
        let s = self.selected[c];
        if s >= 0.0 && (s as usize) < list.len() && s.fract() == 0.0 {
            Some(list[s as usize])
        } else {
            None
        }
    }

    /// next(channel, direction) (weapons.js:530).
    pub fn next(&mut self, defs: &WeaponDefs, c: usize, direction: f64) -> bool {
        let list = self.channel(defs, c);
        if list.len() < 2 {
            return false;
        }
        if let Some(old) = self.current(defs, c) {
            if self.weapons[old].busy() {
                return false;
            }
            self.weapons[old].deselect();
        }
        let n = list.len() as f64;
        self.selected[c] = jsrt::conv::js_mod(self.selected[c] + direction + n, n);
        true
    }

    /// update(dt, input, aim) (weapons.js:542): every weapon's tick, the
    /// selected ones' triggers given (only one channel fires at a time).
    pub fn update(&mut self, defs: &WeaponDefs, dt: f64, input: &ArsenalInput, aim: &Aim, blocked: bool, draws: &mut dyn WeaponDraws, out: &mut Vec<Shot>) {
        let primary = self.current(defs, 0);
        let secondary = self.current(defs, 1);
        let none = Trigger::default();
        let second_busy = match secondary {
            Some(s) => self.weapons[s].busy() || (input.secondary.pressed && !self.weapons[s].empty()),
            None => false,
        };
        for i in 0..self.weapons.len() {
            let mut trigger = none;
            if !blocked && Some(i) == secondary {
                trigger = input.secondary;
            }
            if !blocked && Some(i) == primary && !second_busy {
                trigger = input.primary;
            }
            let selected = Some(i) == primary || Some(i) == secondary;
            let reload = Some(i) == primary && input.reload;
            let class = defs.class(self.weapons[i].def);
            self.weapons[i].update(i as u32, class, dt, selected, &trigger, reload, aim, draws, out);
        }
    }
}
