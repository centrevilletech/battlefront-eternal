//! The rounds in flight (src/ordnance.js), in Rust: what fire makes of each
//! class of round and each tick's flight of the battle's bolts, beams,
//! grenades and rounds (rockets, missiles, shells, fat rays). Every function
//! transliterates the JavaScript method it names, operation by operation and
//! branch by branch (rust/sim/src/ordnance/math.rs has the arithmetic), and
//! reaches the rest of the battle through [`World`] only, at the points and
//! in the order the JavaScript calls src/ordnance-world.js.
//!
//! The state is the store's (`gen::Store`: the ordnance's lists and the
//! rounds by slot). A [`Cx`] reaches it through a pointer and never holds a
//! reference to it across a call to the world, so a world whose calls come
//! back into the module (an explosion's damage reaching the items, a death
//! dropping one) finds the store as the phase left it, as the JavaScript
//! does. A round taken off its list is retired, not dropped: whatever
//! JavaScript still holds of it (a sim event keeps a grenade's position)
//! gets its last state.
//!
//! What only draws (trails, models, lights, impact effects, sounds no
//! capture hears) is not here: a world draws nothing.

pub mod defs;
pub mod math;
// The module's exports and the world its imports make (WebAssembly only).
#[cfg(target_arch = "wasm32")]
pub mod module;
pub mod world;

use std::f64::consts::PI;

use crate::gen::*;
use crate::store::{DefId, Nullable, Quat, Ref, RefSet, StrId, Strings, Vec3};
use defs::{Defs, Field, OrdnanceDef};
use math::*;
use world::*;

const LIMIT: f64 = 5000.0; // m from the origin past which rounds are dropped
const RESTITUTION: f64 = 0.37;
const FRICTION: f64 = 1.5755;
const SLOW: f64 = 1.0; // m/s
const SLOW_BOUNCES: f64 = 4.0;
const MAX_FLIGHT: f64 = 20.0; // s a grenade may fly before it goes off anyway
const MERGE: f64 = 8.0; // m over which a bolt drawn from the muzzle joins its path
const FLOOR: f64 = -500.0;
const FATRAY_RADIUS: f64 = 1.5;

/// What fire() makes of a class (ordnance.js fireRound's dispatch on base).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    Beam,
    Grenade,
    Round(RoundKind),
    Fatray,
    Bolt,
}

fn base_of(def: &OrdnanceDef) -> Base {
    let b = &def.base;
    if b.is("beam") {
        Base::Beam
    } else if b.is("sticky") || b.is("grenade") {
        Base::Grenade
    } else if b.is("bullet") {
        Base::Round(RoundKind::Bullet)
    } else if b.is("missile") {
        Base::Round(RoundKind::Missile)
    } else if b.is("shell") {
        Base::Round(RoundKind::Shell)
    } else if b.is("fatray") {
        Base::Fatray
    } else {
        Base::Bolt
    }
}

/// `OrdnanceOwner` as a beam source or a `gone` test's subject.
pub fn as_source(owner: OrdnanceOwner) -> BeamSource {
    match owner {
        OrdnanceOwner::Null => BeamSource::Null,
        OrdnanceOwner::Player => BeamSource::Player,
        OrdnanceOwner::Unit(r) => BeamSource::Unit(r),
        OrdnanceOwner::Vehicle(r) => BeamSource::Vehicle(r),
        OrdnanceOwner::OwnerSnapshot(r) => BeamSource::OwnerSnapshot(r),
    }
}

fn nullable<T>(x: Option<T>) -> Nullable<T> {
    match x {
        Some(v) => Nullable::Value(v),
        None => Nullable::Null,
    }
}

/// A phase's reach: the store, the instance's strings and the battle's
/// definitions (through pointers: see the module's note), and the world.
pub struct Cx<'a, W: World + ?Sized> {
    store: *mut Store,
    strings: *mut Strings,
    defs: *const Defs,
    pub w: &'a mut W,
}

impl<'a, W: World + ?Sized> Cx<'a, W> {
    /// # Safety
    /// The pointers stay valid while the context lives, and nothing reaches
    /// what they point to meanwhile but this context and those made inside
    /// its world's calls (one at a time, as JavaScript calls back in).
    pub unsafe fn new(store: *mut Store, strings: *mut Strings, defs: *const Defs, w: &'a mut W) -> Self {
        Cx { store, strings, defs, w }
    }

    #[inline]
    pub fn s(&mut self) -> &mut Store {
        // SAFETY: Cx::new's contract; the borrow ends before any world call
        // (the world is reached through `self` too).
        unsafe { &mut *self.store }
    }

    #[inline]
    pub fn strings(&mut self) -> &mut Strings {
        // SAFETY: as for s().
        unsafe { &mut *self.strings }
    }

    #[inline]
    pub fn defs(&self) -> &Defs {
        // SAFETY: as for s(); definitions are only ever added.
        unsafe { &*self.defs }
    }

    #[inline]
    fn ord(&self, id: DefId) -> &OrdnanceDef {
        self.defs().ordnance(id)
    }

    #[inline]
    fn bolt(&mut self, r: Ref<Bolt>) -> &mut Bolt {
        self.s().bolt.get_mut(r).expect("a bolt the ordnance holds")
    }

    #[inline]
    fn beam(&mut self, r: Ref<Beam>) -> &mut Beam {
        self.s().beam.get_mut(r).expect("a beam the ordnance holds")
    }

    #[inline]
    fn grenade(&mut self, r: Ref<Grenade>) -> &mut Grenade {
        self.s().grenade.get_mut(r).expect("a grenade the ordnance holds")
    }

    #[inline]
    fn round(&mut self, r: Ref<Round>) -> &mut Round {
        self.s().round.get_mut(r).expect("a round the ordnance holds")
    }

    /// ordnance.js fireRound: a round of the class `def` fired from
    /// `origin` along `direction`. The round made, for a bolt, rocket,
    /// missile, shell or fat ray; beams and grenades give none.
    /// `emitter`: what a beam follows (Null: its owner). `target_id`,
    /// `target_generation`: a missile's target (None: none given).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_round(&mut self, def: DefId, origin: Vec3, direction: Vec3, owner: OrdnanceOwner, visual: Option<Vec3>, ignore_body: Option<Ref<Body>>, emitter: BeamSource, channel: f64, target_id: Option<f64>, target_generation: Option<f64>) -> Option<Shot> {
        match base_of(self.ord(def)) {
            Base::Beam => {
                self.beam_fire(def, origin, direction, owner, ignore_body, emitter, channel);
                None
            }
            Base::Grenade => {
                self.throw(def, origin, direction, owner, channel);
                None
            }
            Base::Round(kind) => Some(Shot::Round(self.round_fire(kind, def, origin, direction, owner, ignore_body, channel, target_id, target_generation))),
            Base::Fatray => Some(Shot::Round(self.fatray(def, origin, direction, owner, ignore_body, channel))),
            Base::Bolt => Some(Shot::Bolt(self.bolt_fire(def, origin, direction, owner, visual, ignore_body, channel))),
        }
    }

    /// ordnance.js flightMissile: a flyer's missile (vehicles.js fireSeat),
    /// steered by its seeker for the class's missile lifetime at its missile
    /// speed. False for a round no longer flying.
    pub fn flight_missile(&mut self, r: Ref<Round>, flight_class: Option<StrId>, life: f64, speed: f64) -> bool {
        let Some(round) = self.s().round.get_mut(r) else { return false };
        round.flight_missile = Some(true);
        round.flight_class = flight_class;
        round.life = life;
        round.velocity = scale(normalize(round.velocity), speed);
        true
    }

    /// ordnance.js fatray.
    fn fatray(&mut self, def: DefId, origin: Vec3, direction: Vec3, owner: OrdnanceOwner, ignore_body: Option<Ref<Body>>, channel: f64) -> Ref<Round> {
        let dir = normalize(direction);
        let (speed, life) = {
            let d = self.ord(def);
            (d.velocity.or_nullish(100.0), d.life_span.or_nullish(0.45))
        };
        let velocity = scale(dir, speed);
        let mut round = Round {
            kind: RoundKind::Fatray,
            network_fire_channel: channel,
            ignore_body,
            def,
            owner,
            position: origin,
            previous: origin,
            launch_position: origin,
            velocity,
            life,
            impact: None,
            dead: false,
            gravity: 0.0,
            ramp: 0.0,
            desired: dir,
            target_id: None,
            target_generation: None,
            phase: [0.0, 0.0],
            quaternion: QUAT_IDENTITY,
            struck: Some(RefSet::default()),
            gone: None,
            flight_missile: None,
            flight_class: None,
        };
        orient(round.velocity, &mut round.quaternion);
        let r = self.s().round.insert(round);
        self.s().ordnance.rounds.push(r);
        self.w.projectile(Shot::Round(r), ShotKind::Round);
        r
    }

    /// ordnance.js sweepFat: every unit whose sphere comes within
    /// FATRAY_RADIUS of the segment flown is struck, once over the ray's life.
    fn sweep_fat(&mut self, r: Ref<Round>, direction: Vec3, span: f64) {
        if span <= 0.0 {
            return;
        }
        let (previous, owner, ignore_body, def, struck) = {
            let o = self.round(r);
            (o.previous, o.owner, o.ignore_body, o.def, o.struck.take())
        };
        let mid = add_scaled(previous, direction, span * 0.5);
        let empty = RefSet::default();
        let near = self.w.fat_query(mid, span * 0.5 + FATRAY_RADIUS + 4.0, owner, struck.as_ref().unwrap_or(&empty), ignore_body);
        // Units.query only reads its struck filter. Restore the same set
        // before a sphere observation or hit can recursively reach this round.
        self.round(r).struck = struck;
        let Some(near) = near else { return };
        for &unit in &near {
            let Some((center, radius)) = self.w.unit_sphere(unit) else { continue };
            let t = clamp(dot(sub(center, previous), direction), 0.0, span);
            let start = add_scaled(previous, direction, t);
            self.s().ordnance.start = start;
            if distance_to(start, center) > radius + FATRAY_RADIUS {
                continue;
            }
            self.round(r).struck.get_or_insert_with(RefSet::default).add(unit);
            let start = lerp(start, center, 0.5);
            self.s().ordnance.start = start;
            self.w.hit_unit(unit, DefArg::Ordnance(def), Point::Start(start), owner, Nullable::Null, direction);
        }
        self.w.recycle_fat_query(near);
    }

    /// ordnance.js beam: a beam persists for LifeSpan and follows its source.
    #[allow(clippy::too_many_arguments)]
    fn beam_fire(&mut self, def: DefId, origin: Vec3, direction: Vec3, owner: OrdnanceOwner, ignore_body: Option<Ref<Body>>, emitter: BeamSource, channel: f64) {
        let (duration, damage) = {
            let d = self.ord(def);
            // `{ ...def }.Damage`: as the class has it (undefined if none).
            (js_max(0.01, d.life_span.or(0.4)), if matches!(d.damage, Field::Undefined) { None } else { Some(d.damage.num()) })
        };
        let source = if matches!(emitter, BeamSource::Null) { as_source(owner) } else { emitter };
        let history_tick = match self.w.shot_tick(owner, channel) {
            Nullable::Value(t) => Some(t),
            _ => None,
        };
        let local_origin = self.w.beam_local_origin(source, origin);
        let beam = Beam {
            network_fire_channel: channel,
            history_tick,
            def,
            owner,
            source,
            ignore_body,
            life: duration,
            duration,
            effect_time: 0.0,
            position: origin,
            end: origin,
            direction,
            local_origin,
            damage,
        };
        let r = self.s().beam.insert(beam);
        self.s().ordnance.beams.push(r);
        self.w.projectile(Shot::Beam(r), ShotKind::Beam);
    }

    /// ordnance.js stepBeam (a world's: nothing drawn).
    fn step_beam(&mut self, r: Ref<Beam>, dt: f64) {
        let (life, owner, source, local_origin) = {
            let b = self.beam(r);
            (b.life, b.owner, b.source, b.local_origin)
        };
        let span = js_min(dt, life);
        if self.w.gone(as_source(owner)) || self.w.gone(source) {
            self.beam(r).life = 0.0;
            return;
        }
        if let Some(local) = local_origin {
            let (mut position, mut direction) = {
                let b = self.beam(r);
                (b.position, b.direction)
            };
            self.w.beam_pose(source, local, &mut position, &mut direction);
            let b = self.beam(r);
            b.position = position;
            b.direction = direction;
        }
        let (position, direction, ignore_body, history_tick, def, duration) = {
            let b = self.beam(r);
            (b.position, b.direction, b.ignore_body, b.history_tick, b.def, b.duration)
        };
        let hit = self.w.cast(position, direction, 600.0, Out::Hit, nullable(ignore_body), None);
        let unit_hit = match self.cast_units(position, direction, hit.map_or(600.0, |h| h.distance), owner, history_tick, ignore_body, None) {
            Some(u) => Some(u),
            None => self.rider_behind(position, direction, hit, 600.0, owner, history_tick, ignore_body, false),
        };
        self.beam(r).history_tick = None;
        let length = match (unit_hit, hit) {
            (Some(u), _) => u.distance,
            (None, Some(h)) => h.distance,
            (None, None) => 600.0,
        };
        let end = add_scaled(position, direction, length);
        self.beam(r).end = end;
        let share = |cx: &Self| cx.ord(def).damage.or(0.0) * span / duration;
        if let Some(u) = unit_hit {
            let damage = share(self);
            self.beam(r).damage = Some(damage);
            self.w.hit_unit(u.unit, DefArg::Beam(r), Point::BeamEnd(r, end), owner, u.part, direction);
        } else if let Some(body) = hit.and_then(|h| h.body) {
            let damage = share(self);
            self.beam(r).damage = Some(damage);
            self.w.hit(body, DefArg::Beam(r), owner, direction);
        }
        let b = self.beam(r);
        b.effect_time -= dt;
        // A beam leaves its ImpactEffect wherever it touches (drawn by a page).
        if (hit.is_some() || unit_hit.is_some()) && b.effect_time <= 0.0 {
            b.effect_time = 0.12;
        }
        b.life -= dt;
    }

    /// ordnance.js round: a rocket, missile or shell.
    #[allow(clippy::too_many_arguments)]
    fn round_fire(&mut self, kind: RoundKind, def: DefId, origin: Vec3, direction: Vec3, owner: OrdnanceOwner, ignore_body: Option<Ref<Body>>, channel: f64, target_id: Option<f64>, target_generation: Option<f64>) -> Ref<Round> {
        let bullet = kind == RoundKind::Bullet;
        let (speed, mut life, gravity) = {
            let d = self.ord(def);
            (
                d.velocity.or_nullish(if bullet { 150.0 } else { 35.0 }),
                d.life_span.or_nullish(if bullet { 2.0 } else { 3.0 }),
                if kind == RoundKind::Shell { -9.8 * d.gravity.or_nullish(1.0) } else { 0.0 },
            )
        };
        let dir = normalize(direction);
        let mut impact = None;
        if bullet {
            if let Some(hit) = self.w.cast(origin, dir, speed * life, Out::Hit, nullable(ignore_body), Some(true)) {
                life = hit.distance / speed;
                impact = Some(Impact { point: hit.point, normal: hit.normal, terrain: hit.terrain, body: nullable(hit.body), unit: None, part: Nullable::Undefined });
            }
        }
        let velocity = scale(dir, speed);
        let phase = [self.w.waver() * 2.0 * PI, self.w.waver() * 2.0 * PI];
        let mut round = Round {
            kind,
            network_fire_channel: channel,
            ignore_body,
            def,
            owner,
            position: origin,
            previous: origin,
            launch_position: origin,
            velocity,
            life,
            impact,
            dead: false,
            gravity,
            ramp: 0.0,
            desired: dir,
            target_id: Some(target_id.unwrap_or(-1.0)),
            target_generation: Some(target_generation.unwrap_or(-1.0)),
            phase,
            quaternion: QUAT_IDENTITY,
            struck: None,
            gone: None,
            flight_missile: None,
            flight_class: None,
        };
        orient(round.velocity, &mut round.quaternion);
        let r = self.s().round.insert(round);
        self.s().ordnance.rounds.push(r);
        self.w.projectile(Shot::Round(r), ShotKind::Round);
        r
    }

    /// ordnance.js bolt: a bolt's flight traced once at launch (its life cut
    /// at what it will hit), then flown, at once to the world's tick for a
    /// shot judged in the past.
    #[allow(clippy::too_many_arguments)]
    fn bolt_fire(&mut self, def: DefId, origin: Vec3, direction: Vec3, owner: OrdnanceOwner, visual: Option<Vec3>, ignore_body: Option<Ref<Body>>, channel: f64) -> Ref<Bolt> {
        let shot_tick = self.w.shot_tick(owner, channel);
        let (speed, mut life) = {
            let d = self.ord(def);
            (d.velocity.num(), d.life_span.num())
        };
        let mut impact = None;
        if let Some(hit) = self.w.cast(origin, direction, speed * life, Out::Hit, nullable(ignore_body), Some(true)) {
            life = hit.distance / speed;
            impact = Some(Impact { point: hit.point, normal: hit.normal, terrain: hit.terrain, body: nullable(hit.body), unit: None, part: Nullable::Undefined });
        }
        let shooter = self.w.shooter(owner, channel);
        let bolt = Bolt {
            def,
            owner,
            network_fire_id: shooter.fire_id,
            network_fire_channel: channel,
            network_fire_unit: shooter.unit,
            network_fire_generation: shooter.generation,
            ignore_body,
            position: origin,
            direction: normalize(direction),
            speed,
            life,
            drawn: 0.0,
            flown: 0.0,
            offset: visual.map(|v| sub(v, origin)),
            merge: js_max(js_min(MERGE, speed * life), 1e-3),
            impact,
            previous_flown: None,
            previous_drawn: None,
        };
        let r = self.s().bolt.insert(bolt);
        self.w.launch(r, shot_tick);
        if let Nullable::Value(shot_tick) = shot_tick {
            let mut from = shot_tick;
            let mut tick = shot_tick.floor() + 1.0;
            while tick <= self.w.history_tick() && self.bolt(r).life > 0.0 {
                self.step_bolt(r, (tick - from) / 60.0, Some(tick));
                from = tick;
                tick += 1.0;
            }
        }
        if self.bolt(r).life > 0.0 {
            self.s().ordnance.bolts.push(r);
        } else {
            self.s().bolt.retire(r);
        }
        r
    }

    /// ordnance.js throw: a grenade, tumbling about an axis drawn from the
    /// grenade-spin stream (three's Vector3.randomDirection).
    fn throw(&mut self, def: DefId, origin: Vec3, direction: Vec3, owner: OrdnanceOwner, channel: f64) {
        let (speed, gravity) = {
            let d = self.ord(def);
            (d.velocity.num(), -9.8 * d.gravity.or_nullish(1.0))
        };
        let velocity = scale(normalize(direction), speed);
        let theta = self.w.spin() * PI * 2.0;
        let u = self.w.spin() * 2.0 - 1.0;
        let c = (1.0 - u * u).sqrt();
        let grenade = Grenade {
            network_fire_channel: channel,
            def,
            owner,
            position: origin,
            previous: origin,
            launch_position: origin,
            velocity,
            gravity,
            slow: 0.0,
            resting: false,
            flight: 0.0,
            fuse: None,
            spin: [c * cos(theta), u, c * sin(theta)],
            quaternion: QUAT_IDENTITY,
            sinking: None,
            touched: None,
            gone: None,
        };
        let r = self.s().grenade.insert(grenade);
        self.s().ordnance.grenades.push(r);
        self.w.projectile(Shot::Grenade(r), ShotKind::Grenade);
    }

    /// ordnance.js update (a world's: nothing drawn): every beam, bolt,
    /// grenade and round steps in its list's order, then each list keeps
    /// those still flying. A round fired meanwhile (by a call the steps
    /// make) joins its list and steps too, as the JavaScript's loop visits
    /// what is pushed while it runs.
    pub fn update(&mut self, dt: f64) {
        let mut i = 0;
        while i < self.s().ordnance.beams.len() {
            let r = self.s().ordnance.beams[i];
            self.step_beam(r, dt);
            i += 1;
        }
        self.keep_beams();
        let mut i = 0;
        while i < self.s().ordnance.bolts.len() {
            let r = self.s().ordnance.bolts[i];
            self.step_bolt(r, dt, None);
            i += 1;
        }
        self.keep_bolts();
        let mut i = 0;
        while i < self.s().ordnance.grenades.len() {
            let r = self.s().ordnance.grenades[i];
            self.step_grenade(r, dt);
            i += 1;
        }
        self.keep_grenades();
        let mut i = 0;
        while i < self.s().ordnance.rounds.len() {
            let r = self.s().ordnance.rounds[i];
            self.step_round(r, dt);
            i += 1;
        }
        self.keep_rounds();
    }

    // Ordnance.update filters live lists in order and retires the rest. No
    // callback runs during this filter, so retain their construction capacity
    // for the next unit or vehicle shot instead of dropping the old Vec.
    fn keep_beams(&mut self) {
        let s = self.s();
        let slab = &mut s.beam;
        s.ordnance.beams.retain(|&r| {
            if slab.get(r).is_some_and(|b| b.life > 0.0) { true } else { slab.retire(r); false }
        });
    }

    fn keep_bolts(&mut self) {
        let s = self.s();
        let slab = &mut s.bolt;
        s.ordnance.bolts.retain(|&r| {
            if slab.get(r).is_some_and(|b| b.life > 0.0) { true } else { slab.retire(r); false }
        });
    }

    fn keep_grenades(&mut self) {
        let s = self.s();
        let slab = &mut s.grenade;
        s.ordnance.grenades.retain(|&r| {
            if slab.get(r).is_some_and(|g| g.gone != Some(true)) { true } else { slab.retire(r); false }
        });
    }

    fn keep_rounds(&mut self) {
        let s = self.s();
        let slab = &mut s.round;
        s.ordnance.rounds.retain(|&r| {
            if slab.get(r).is_some_and(|o| o.gone != Some(true)) { true } else { slab.retire(r); false }
        });
    }

    /// ordnance.js stepRound.
    fn step_round(&mut self, r: Ref<Round>, dt: f64) {
        let (dead, impact, kind, def, owner) = {
            let o = self.round(r);
            (o.dead, o.impact.clone(), o.kind, o.def, o.owner)
        };
        // Life ran out at the point the spawn ray found: the impact, a frame on.
        if dead {
            if let Some(i) = impact {
                let body = match i.body {
                    Nullable::Value(b) => Some(b),
                    _ => None,
                };
                self.impact_at(r, i.point, i.normal, i.terrain, body, None, Nullable::Null);
            }
            self.remove_round(r);
            return;
        }
        if kind == RoundKind::Shell {
            let o = self.round(r);
            o.velocity[1] += o.gravity * dt;
        }
        if kind == RoundKind::Missile {
            self.steer(r, dt);
        }
        let (previous, position, life, velocity, ignore_body) = {
            let o = self.round(r);
            o.previous = o.position;
            o.position = add_scaled(o.position, o.velocity, js_min(dt, o.life));
            o.life -= dt;
            (o.previous, o.position, o.life, o.velocity, o.ignore_body)
        };
        if position[1] < FLOOR || position[0].abs() > LIMIT || position[2].abs() > LIMIT {
            self.remove_round(r);
            return;
        }
        // Everything meets water coming down through it.
        if let Some(wet) = self.w.water_crossing(previous, position) {
            let blast = self.ord(def).explosion_impact.clone();
            if blast.truthy() {
                self.w.explosion(&blast, add_scaled(wet, UP, 0.2), owner);
            }
            self.remove_round(r);
            return;
        }
        // Curved rounds test all geometry; straight rounds test moving
        // bodies over each segment in addition to their cached static impact.
        let ray = sub(position, previous);
        let length = math::length(ray);
        if length > 1e-6 {
            let ray = divide(ray, length);
            let hit = if kind == RoundKind::Bullet {
                self.w.cast_moving(previous, ray, length, Out::Hit, nullable(ignore_body))
            } else {
                self.w.cast(previous, ray, length, Out::Hit, nullable(ignore_body), None)
            };
            if kind == RoundKind::Fatray {
                self.sweep_fat(r, ray, hit.map_or(length, |h| h.distance));
            }
            let unit_hit = if kind == RoundKind::Fatray {
                None
            } else {
                match self.cast_units(previous, ray, hit.map_or(length, |h| h.distance), owner, None, ignore_body, None) {
                    Some(u) => Some(u),
                    None => {
                        let far = if kind == RoundKind::Bullet { length + js_max(life, 0.0) * math::length(velocity) } else { f64::INFINITY };
                        self.rider_behind(previous, ray, hit, far, owner, None, ignore_body, kind == RoundKind::Bullet)
                    }
                }
            };
            if let Some(u) = unit_hit {
                self.impact_at(r, u.point, u.normal, false, None, Some(u.unit), u.part);
                self.remove_round(r);
                return;
            }
            if let Some(h) = hit {
                self.impact_at(r, h.point, h.normal, h.terrain, h.body, None, Nullable::Null);
                self.remove_round(r);
                return;
            }
        }
        if life <= 0.0 {
            let expire = self.ord(def).explosion_expire.clone();
            if expire.truthy() {
                let position = self.round(r).position;
                self.w.explosion(&expire, position, owner);
            }
            if self.round(r).impact.is_some() {
                self.round(r).dead = true;
            } else {
                self.remove_round(r);
            }
            return;
        }
        let o = self.round(r);
        orient(o.velocity, &mut o.quaternion);
    }

    /// ordnance.js steer: a missile turns toward where it wants to go at
    /// TurnRate (no target: the way it was fired) and wavers by WaverTurn,
    /// both eased in over ScaleTime.
    fn steer(&mut self, r: Ref<Round>, dt: f64) {
        let (def, flight_missile, position, flight_class, target_id, target_generation) = {
            let o = self.round(r);
            (o.def, o.flight_missile, o.position, o.flight_class, o.target_id, o.target_generation)
        };
        if flight_missile == Some(true) {
            let mut velocity = self.round(r).velocity;
            let id = self.w.navigate_missile(position, &mut velocity, flight_class, target_id.unwrap_or(f64::NAN), target_generation.unwrap_or(f64::NAN), dt);
            let o = self.round(r);
            o.velocity = velocity;
            o.target_id = Some(id);
            return;
        }
        match self.w.missile_target(target_id.unwrap_or(f64::NAN), target_generation.unwrap_or(f64::NAN)) {
            Some(aim) => {
                let o = self.round(r);
                o.desired = normalize(sub(aim, o.position));
            }
            None => {
                if target_id.is_some_and(|t| t >= 0.0) {
                    self.round(r).target_id = Some(-1.0);
                }
            }
        }
        let (scale_time, turn_rate, waver_turn, waver_rate) = {
            let d = self.ord(def);
            (d.scale_time.or(1.0), d.turn_rate.or_nullish(1.0), d.waver_turn.or(0.0), d.waver_rate.or(0.0))
        };
        let (mut v, desired, ramp, phase) = {
            let o = self.round(r);
            o.ramp = js_min(o.ramp + dt / scale_time, 1.0);
            (o.velocity, o.desired, o.ramp, o.phase)
        };
        let speed = math::length(v);
        if speed < 1e-6 {
            return;
        }
        let forward = divide(v, speed);
        let turn = turn_rate * ramp;
        if turn > 0.0 {
            let axis = cross(forward, desired);
            let sine = math::length(axis);
            if sine * sine > 1e-8 {
                v = apply_axis_angle(v, divide(axis, sine), js_min(sine, turn * dt));
                self.round(r).velocity = v;
            }
        }
        let amp = waver_turn * ramp;
        if amp > 0.0 {
            let right = normalize(cross(UP, forward));
            let up = cross(forward, right);
            self.s().ordnance.start = up;
            let w = add_scaled(scale(up, sin(phase[1])), right, sin(phase[0]));
            let size = math::length(w);
            if size > 1e-6 {
                v = apply_axis_angle(v, divide(w, size), amp * dt * size);
                self.round(r).velocity = v;
            }
            let a = self.w.waver() * 2.0 * waver_rate * dt;
            self.round(r).phase[0] += a;
            let b = self.w.waver() * 2.0 * waver_rate * dt;
            self.round(r).phase[1] += b;
        }
    }

    /// ordnance.js impactAt: what hit, told to the capture, damaged, and its
    /// ExplosionImpact set off just off the surface.
    #[allow(clippy::too_many_arguments)]
    fn impact_at(&mut self, r: Ref<Round>, point: Vec3, normal: Vec3, terrain: bool, body: Option<Ref<Body>>, unit: Option<Ref<Unit>>, part: Nullable<HitPart>) {
        let (def, owner, velocity) = {
            let o = self.round(r);
            (o.def, o.owner, o.velocity)
        };
        let surface = match unit {
            Some(u) => {
                if self.w.is_droid(u) {
                    3.0
                } else {
                    2.0
                }
            }
            None => {
                if terrain {
                    0.0
                } else {
                    1.0
                }
            }
        };
        self.w.impact(Shot::Round(r), &ImpactInfo::Plain { point, normal, surface });
        let axis = normalize(velocity);
        if let Some(b) = body {
            self.w.hit(b, DefArg::Ordnance(def), owner, axis);
        }
        if let Some(u) = unit {
            self.w.hit_unit(u, DefArg::Ordnance(def), Point::UnitHit(point), owner, part, axis);
        }
        let blast = self.ord(def).explosion_impact.clone();
        if blast.truthy() {
            self.w.explosion(&blast, add_scaled(point, normal, 0.2), owner);
        }
    }

    /// ordnance.js remove (a round's).
    fn remove_round(&mut self, r: Ref<Round>) {
        self.round(r).gone = Some(true);
    }

    /// ordnance.js stepBolt (a world's: nothing drawn). `history_tick`: the
    /// tick a lag-compensated bolt's step meets the others at.
    fn step_bolt(&mut self, r: Ref<Bolt>, dt: f64, history_tick: Option<f64>) {
        let (def, life, start, direction, speed, owner, ignore_body) = {
            let b = self.bolt(r);
            b.previous_flown = Some(b.flown);
            b.previous_drawn = Some(b.drawn);
            (b.def, b.life, b.position, b.direction, b.speed, b.owner, b.ignore_body)
        };
        let step = js_min(dt, life);
        self.s().ordnance.start = start;
        self.bolt(r).position = add_scaled(start, direction, speed * step);
        let moving = self.w.cast_moving(start, direction, speed * step, Out::Hit, nullable(ignore_body));
        let unit_hit = match self.cast_units(start, direction, moving.map_or(speed * step, |h| h.distance), owner, history_tick, ignore_body, None) {
            Some(u) => Some(u),
            None => self.rider_behind(start, direction, moving, speed * life, owner, history_tick, ignore_body, true),
        };
        if let Some(u) = unit_hit {
            let b = self.bolt(r);
            b.position = u.point;
            b.life = 0.0;
            b.impact = Some(Impact { point: u.point, normal: u.normal, terrain: false, body: Nullable::Null, unit: Some(u.unit), part: u.part });
        } else if let Some(h) = moving {
            let b = self.bolt(r);
            b.position = h.point;
            b.life = 0.0;
            b.impact = Some(Impact { point: h.point, normal: h.normal, terrain: false, body: nullable(h.body), unit: None, part: Nullable::Undefined });
        }
        // Water stops a bolt where it goes in.
        let (life, position) = {
            let b = self.bolt(r);
            (b.life, b.position)
        };
        if life > 0.0 {
            if let Some(wet) = self.w.water_crossing(start, position) {
                self.bolt(r).life = 0.0;
                self.w.impact(Shot::Bolt(r), &ImpactInfo::Plain { point: wet, normal: UP, surface: 4.0 });
                self.bolt(r).impact = None;
                return;
            }
        }
        let longest = {
            let d = self.ord(def);
            js_max(js_max(d.laser_length.or(1.0), d.glow()), d.blur_length.or(0.0))
        };
        let (life, impact, position) = {
            let b = self.bolt(r);
            b.flown += speed * step;
            b.drawn = js_min(b.drawn + speed * dt, longest);
            b.life -= dt;
            (b.life, b.impact.clone(), b.position)
        };
        if life <= 0.0 {
            let info = match &impact {
                Some(i) => {
                    let surface = match i.unit {
                        Some(u) => {
                            if self.w.is_droid(u) {
                                3.0
                            } else {
                                2.0
                            }
                        }
                        None => {
                            if i.terrain {
                                0.0
                            } else {
                                1.0
                            }
                        }
                    };
                    ImpactInfo::Struck { impact: i.clone(), surface }
                }
                None => ImpactInfo::Plain { point: position, normal: UP, surface: 5.0 },
            };
            self.w.impact(Shot::Bolt(r), &info);
            let (blast, expire) = {
                let d = self.ord(def);
                (d.explosion_impact.clone(), d.explosion_expire.clone())
            };
            if let Some(i) = impact {
                if let Nullable::Value(body) = i.body {
                    self.w.hit(body, DefArg::Ordnance(def), owner, direction);
                }
                if let Some(u) = i.unit {
                    self.w.hit_unit(u, DefArg::Ordnance(def), Point::Fresh(i.point), owner, i.part, direction);
                }
                // A bolt with an explosion (the orbital strike's) sets it off there.
                if blast.truthy() {
                    self.w.explosion(&blast, add_scaled(i.point, i.normal, 0.2), owner);
                }
                self.bolt(r).impact = None;
            } else if expire.truthy() {
                // A bolt that bursts at the end of its flight (the AAA turret's flak).
                self.w.explosion(&expire, position, owner);
            }
        }
    }

    /// ordnance.js stepGrenade.
    fn step_grenade(&mut self, r: Ref<Grenade>, dt: f64) {
        let def = self.grenade(r).def;
        let fuse_life = self.ord(def).life_span.or(0.0);
        let resting = {
            let g = self.grenade(r);
            g.flight += dt;
            g.resting
        };
        if !resting {
            let (sinking, previous, position) = {
                let g = self.grenade(r);
                if g.sinking == Some(true) {
                    g.velocity = [0.0, -0.2, 0.0];
                } else {
                    g.velocity[1] += g.gravity * dt;
                }
                g.previous = g.position;
                g.position = add_scaled(g.position, g.velocity, dt);
                (g.sinking, g.previous, g.position)
            };
            // Into the water: it sinks at 0.2 m/s with its fuse running.
            if sinking != Some(true) && self.w.water_crossing(previous, position).is_some() {
                let g = self.grenade(r);
                g.sinking = Some(true);
                g.velocity = [0.0, -0.2, 0.0];
                if g.fuse.is_none() {
                    g.fuse = Some(fuse_life);
                }
            }
            self.bounce(r);
            let g = self.grenade(r);
            g.quaternion = multiply(g.quaternion, axis_angle(g.spin, math::length(g.velocity) * dt * 2.0));
        }
        let explode = {
            let g = self.grenade(r);
            if g.resting && g.fuse.is_none() {
                g.fuse = Some(fuse_life);
            }
            if let Some(f) = g.fuse {
                g.fuse = Some(f - dt);
            }
            g.fuse.is_some_and(|f| f <= 0.0) || g.flight > MAX_FLIGHT || g.position[1] < FLOOR
        };
        if explode {
            self.explode(r);
        }
    }

    /// ordnance.js bounce: sweep from just behind where the grenade was to
    /// just past where it is.
    fn bounce(&mut self, r: Ref<Grenade>) {
        let (position, previous, def) = {
            let g = self.grenade(r);
            (g.position, g.previous, g.def)
        };
        let ray = sub(position, previous);
        let length = math::length(ray);
        if length < 1e-6 {
            return;
        }
        let ray = divide(ray, length);
        let start = add_scaled(previous, ray, -0.1);
        self.s().ordnance.start = start;
        let Some(hit) = self.w.cast(start, ray, length + 0.3, Out::Hit, Nullable::Undefined, None) else { return };
        let n = hit.normal;
        let v = self.grenade(r).velocity;
        let d = dot(v, n);
        if d >= 0.0 {
            return;
        }
        let at = add_scaled(hit.point, n, 0.1);
        self.grenade(r).position = at;
        self.w.bounce(def, at, -d);
        if self.grenade(r).touched != Some(true) {
            self.grenade(r).touched = Some(true);
            self.w.grenade_contact(r);
        }
        let sticks = {
            let o = self.ord(def);
            if hit.terrain {
                o.stick_terrain.positive()
            } else {
                o.stick_building.positive()
            }
        };
        let g = self.grenade(r);
        if sticks {
            g.velocity = [0.0, 0.0, 0.0];
            g.resting = true;
            return;
        }
        let mut v = add_scaled(g.velocity, n, -d);
        let along = math::length(v);
        if along > 0.0 {
            v = scale(v, js_max(along - js_min(FRICTION * -d, along), 0.0) / along);
        }
        v = add_scaled(v, n, RESTITUTION * -d);
        g.velocity = v;
        if math::length(v) < SLOW {
            g.slow += 1.0;
            if g.slow >= SLOW_BOUNCES {
                g.velocity = [0.0, 0.0, 0.0];
                g.resting = true;
            }
        }
    }

    /// ordnance.js explode (a grenade's).
    fn explode(&mut self, r: Ref<Grenade>) {
        let (def, position, owner) = {
            let g = self.grenade(r);
            g.gone = Some(true);
            (g.def, g.position, g.owner)
        };
        let expire = self.ord(def).explosion_expire.clone();
        if expire.truthy() {
            self.w.explosion(&expire, position, owner);
        }
    }

    /// ordnance.js castUnits: the first unit a segment meets (the world
    /// says which may be struck). `mount`: the hull a round has gone
    /// through to its riders (riderBehind).
    #[allow(clippy::too_many_arguments)]
    fn cast_units(&mut self, origin: Vec3, direction: Vec3, far: f64, owner: OrdnanceOwner, history_tick: Option<f64>, ignore_body: Option<Ref<Body>>, mount: Option<Ref<Vehicle>>) -> Option<UnitHit> {
        if far <= 0.0 {
            return None;
        }
        self.w.raycast_units(origin, direction, far, owner, history_tick, ignore_body, mount)
    }

    /// ordnance.js riderBehind: a round that met a hull whose collision
    /// doesn't keep rounds off its riders goes on through it to a rider of
    /// it, if it crosses his body before anything else.
    #[allow(clippy::too_many_arguments)]
    fn rider_behind(&mut self, origin: Vec3, direction: Vec3, hit: Option<Hit>, far: f64, owner: OrdnanceOwner, history_tick: Option<f64>, ignore_body: Option<Ref<Body>>, moving: bool) -> Option<UnitHit> {
        // hit?.body
        let body = match hit {
            Some(h) => nullable(h.body),
            None => Nullable::Undefined,
        };
        let mount = self.w.rider_mount(body)?;
        let hit = hit.expect("a hull's riders are struck through what the round hit");
        let reach = js_min(far, hit.distance + 2.0 * or(mount.bound_radius, 1.0));
        let next = if moving {
            self.w.cast_moving(origin, direction, reach, Out::Behind, mount.body)
        } else {
            self.w.cast(origin, direction, reach, Out::Behind, mount.body, None)
        };
        self.cast_units(origin, direction, next.map_or(reach, |n| n.distance), owner, history_tick, ignore_body, Some(mount.vehicle))
    }
}

/// ordnance.js orient: nose (+z) along the flight, its x across the world's up.
fn orient(velocity: Vec3, quaternion: &mut Quat) {
    let forward = normalize(velocity);
    if length_sq(forward) < 1e-8 {
        return;
    }
    let mut right = cross(UP, forward);
    if length_sq(right) < 1e-8 {
        right = [1.0, 0.0, 0.0];
    }
    let right = normalize(right);
    let up = cross(forward, right);
    *quaternion = from_basis(right, up, forward);
}

/// ordnance.js healthScale for the share of an item's blast.
pub fn blast_distance(position: Vec3, center: Vec3, radius: f64) -> f64 {
    distance_to(center, position) - 0.5 * radius
}
