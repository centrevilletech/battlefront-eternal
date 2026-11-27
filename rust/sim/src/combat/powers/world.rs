//! The powers of a 2005 battle in the live world. A battle installs them
//! once; the soldier phase forwards each life's offhand trigger and lock
//! button once per fixed tick; one call per tick then runs the timers.
//!
//! Damage goes through the battle's own per-target damage service, so team
//! damage, death, credit and publication follow the same rules as every other
//! hit. Pushes replace the soldier body's velocity, as the movement step reads
//! it on the next tick.

use super::cast::{PowerInput, PowerKit, PowerState, PowerStep, Request, MAX_POWERS};
use super::def::{self, Aura, Area, Cone, Effect, PowerDef, PowerDefError, MAX_AREA_TARGETS};
use super::effects::{EffectTable, Held};
use super::geometry::{self, AimFrame, Footing, Nearest, Reaction};
use super::lock::{LockCandidate, LockState};
use crate::combat::combo::deflect::{angle_in_range_deg, relative_horizontal_angle_deg};
use crate::combat::combo::{zero_fnv1a_lower, DeflectDef};
use super::key::LifeKey;
use crate::gen::{Air, Attacker, Member, Posture, Unit, UnitBody};
use crate::movement::soldier::swbf2;
use crate::ordnance::math::{self as om, V3};
use crate::store::{Ref, StrId};
use crate::world::owner::World;
use bfe_content::Value;
use three::Vector3;

/// Candidates one emission or acquisition considers, nearest first.
pub const MAX_CANDIDATES: usize = 32;
/// How often a bot checks whether its aura conditions are met while idle.
pub const BOT_AURA_CHECK_TICKS: u32 = 12;
/// Minimum friendly soldiers (including the caster) inside an aura's radius for a bot to cast it.
pub const BOT_AURA_MIN_FRIENDS: usize = 3;
/// Fallback enemy proximity for standalone aura tests without an AI battle.
pub const BOT_AURA_THREAT_RANGE: f64 = 15.0;
/// Events kept for readers that sample less often than every tick.
pub const LOG_LEN: usize = 64;
const DT: f64 = 1.0 / def::TICKS_PER_SECOND;

/// What a power did, for presentation, journals and measurements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEventKind {
    /// An emitter fired, or an area weapon started holding.
    Cast,
    /// A target took the power's damage or effect.
    Hit,
    /// A guard intercepted the power.
    Blocked,
    /// A push threw the target off its feet.
    Thrown,
    /// The target was held by a choke on this tick.
    Held,
    /// The target received an aura's modifiers.
    Buffed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerEvent {
    pub sequence: u32,
    pub tick: u32,
    pub caster: LifeKey,
    pub target: Option<LifeKey>,
    /// Case-folded FNV-1a of the weapon class.
    pub weapon: u32,
    pub kind: PowerEventKind,
    pub bot: bool,
}

/// A bounded ring that readers follow with their own cursor.
#[derive(Clone, Copy, Debug)]
pub struct PowerLog {
    rows: [Option<PowerEvent>; LOG_LEN],
    /// The sequence of the newest event. It saturates at `u32::MAX`, where the log stops taking events.
    pub latest: u32,
}

impl Default for PowerLog {
    fn default() -> Self {
        Self { rows: [None; LOG_LEN], latest: 0 }
    }
}

impl PowerLog {
    fn push(&mut self, mut event: PowerEvent) {
        event.sequence = self.latest.saturating_add(1);
        self.rows[self.latest as usize % LOG_LEN] = Some(event);
        self.latest = event.sequence;
    }

    /// Events after `cursor`, oldest first; older ones may have been overwritten.
    pub fn since(&self, cursor: u32) -> impl Iterator<Item = PowerEvent> + '_ {
        let first = cursor.max(self.latest.saturating_sub(LOG_LEN as u32));
        (first..self.latest).filter_map(|i| self.rows[i as usize % LOG_LEN])
    }
}

/// One power hit about to enter the damage service.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerHit {
    pub caster: Ref<Unit>,
    pub target: Ref<Unit>,
    pub weapon: u32,
    pub amount: f64,
    pub from: V3,
}

/// What the live soldier adapter knows that the world rows do not.
pub trait PowerHost {
    /// The footing as the soldier's movement knows it (a roll, for one).
    fn footing(&mut self, _w: &World, _life: LifeKey) -> Option<Footing> {
        None
    }
    /// The defender's deflect window while it guards.
    fn guard(&mut self, _w: &World, _life: LifeKey) -> Option<DeflectDef> {
        None
    }
    /// Pay a defender's per-interception cost from its stamina.
    fn pay_guard(&mut self, _w: &mut World, _life: LifeKey, cost: f64) -> bool {
        cost <= 0.0
    }
    /// A push threw this soldier: its movement must end any roll or sprint.
    fn thrown(&mut self, _w: &mut World, _life: LifeKey) {}
    /// A fast flat push put this soldier straight into ground tumble.
    fn tumbled(&mut self, w: &mut World, life: LifeKey) {
        self.thrown(w, life);
    }
    /// Apply a hit through the damage service. A journal wraps `apply` to
    /// record the cause of what follows.
    fn damage(&mut self, w: &mut World, _hit: PowerHit, apply: &mut dyn FnMut(&mut World)) {
        apply(w);
    }
}

/// A host with nothing to add: every soldier stands where its body says.
pub struct NoHost;
impl PowerHost for NoHost {}

/// The trigger and lock button of one life on this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Forward {
    pub power: PowerInput,
    /// The lock button went down on this tick.
    pub lock_pressed: bool,
    /// The life is a bot: its powers follow the provisional bot rule.
    pub bot: bool,
}

/// Per-unit 2005 power presentation state sampled once per fixed tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnitPowerPose {
    pub slot: u32,
    pub generation: f64,
    /// Case-folded FNV-1a hash of the animating power or offhand weapon (0 = none).
    pub weapon: u32,
    /// `FireAnim` slot (0 = FIRE, 1 = FIRE2).
    pub fire_anim: u8,
    /// Fixed ticks elapsed since this cast animation began.
    pub elapsed_ticks: u32,
    /// Hold the final frame while true (choke hold or continuously held repeat power).
    pub holding: bool,
    /// Held by another soldier's choke.
    pub choked: bool,
    /// CRC of `human_{SoldierAnimation}` named by the holding choke (0 when unset).
    pub choke_anim: u32,
    /// Active 2005 victim tumble phase (0=none, 1=Thrown, 2=Tumble, 3=Landed, 4=GetUp, 5=Recover, 6=RestDead).
    pub tumble_phase: u8,
    /// Bit 0: front side (1=front, 0=back); bit 1: soft bounce vs land; bits 2..=3: initial fly direction.
    pub tumble_flags: u8,
    /// Seconds elapsed in the active tumble phase.
    pub tumble_elapsed: f32,
    /// Midair visual tumble angle in [0, TAU) during Thrown and Recover.
    pub tumble_angle: f32,
    /// Body yaw offset from aim_yaw aligned to ground travel during Tumble/Landed/GetUp/RestDead.
    pub tumble_heading: f32,
}

/// Complete per-tick 2005 power observation crossing to presentation.
/// Pre-allocated once; steady-state copies perform zero heap allocations.
#[derive(Clone, Debug, Default)]
pub struct PowerFrame {
    pub tick: u32,
    pub units: Vec<UnitPowerPose>,
    pub log: PowerLog,
}

impl PowerFrame {
    pub fn with_capacity(units: usize) -> Self {
        Self {
            tick: 0,
            units: Vec::with_capacity(units),
            log: PowerLog::default(),
        }
    }

    pub fn clear(&mut self) {
        self.units.clear();
    }

    pub fn copy_into(&self, dest: &mut Option<Box<PowerFrame>>) {
        let out = dest.get_or_insert_with(|| {
            Box::new(PowerFrame::with_capacity(self.units.capacity().max(64)))
        });
        out.tick = self.tick;
        out.units.clear();
        out.units.extend_from_slice(&self.units);
        out.log = self.log;
    }

    pub fn unit(&self, slot: u32, generation: f64) -> Option<&UnitPowerPose> {
        self.units
            .iter()
            .find(|u| u.slot == slot && u.generation == generation)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AnimState {
    weapon: u32,
    power: u8,
    fire_anim: u8,
    start_tick: u32,
    holding: bool,
    was_holding_area: bool,
}

#[derive(Clone, Copy, Debug)]
struct LifeRow {
    life: LifeKey,
    kit: u16,
    state: PowerState,
    anim: AnimState,
    lock: LockState,
    held: [Option<LifeKey>; MAX_AREA_TARGETS as usize],
    bot_wait: u32,
}

/// The powers of every class and life in one battle.
#[derive(Debug, Default)]
pub struct Powers {
    kits: Vec<(u32, PowerKit)>,
    aura_effects: Vec<(u32, String)>,
    lives: Vec<Option<LifeRow>>,
    regen_scratch: Vec<(LifeKey, f64)>,
    burn_scratch: Vec<(LifeKey, super::effects::Burn, bool)>,
    log: PowerLog,
    /// The events every viewer draws (casts, hits, throws, blocks, burns) since
    /// the host last published them; bounded, never grown during a battle.
    published: Vec<(PowerEvent, Option<(StrId, f64)>)>,
    tick: u32,
}

/// Effect-bearing power events kept for publication per tick.
const PUBLISHED: usize = 256;

/// A power event as the battle publishes it to every viewer (an Instant Action
/// client and a server's clients alike): who, what, and where the caster stood.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PublishedPower {
    pub caster: LifeKey,
    pub target: Option<LifeKey>,
    /// Case-folded FNV-1a of the weapon class.
    pub weapon: u32,
    pub kind: PowerEventKind,
    /// The caster's position when the host publishes the tick's events.
    pub position: Option<V3>,
    /// A burn on the target: the effect it shows and the seconds it has left.
    pub burn: Option<(StrId, f64)>,
}

/// Move the events published since the last call into `out` (cleared first),
/// each with its caster's current position. Retains both buffers' capacity.
pub fn drain_published_in(w: &mut World, out: &mut Vec<PublishedPower>) {
    out.clear();
    let Some(powers) = w.combat.powers.as_deref_mut() else { return };
    let store = &w.sim.store;
    for (ev, burn) in powers.published.drain(..) {
        let position = ev.caster.unit(store).map(|u| u.position()).map(|p| [p.x, p.y, p.z]);
        out.push(PublishedPower { caster: ev.caster, target: ev.target, weapon: ev.weapon, kind: ev.kind, position, burn });
    }
}

#[derive(Clone, Copy)]
struct Body {
    key: LifeKey,
    reference: Ref<Unit>,
    feet: V3,
    center: V3,
    radius: f64,
    team: f64,
}

fn alive(u: &Unit) -> bool {
    u.alive && !u.dead && !u.removed
}

fn enemy(team: f64, other: f64) -> bool {
    team != 0.0 && other != 0.0 && team != other
}

fn posture_of(u: &Unit) -> Posture {
    match &u.soldier {
        UnitBody::Soldier(s) => s.posture,
        UnitBody::Droideka(_) => Posture::Stand,
    }
}

fn body_of(reference: Ref<Unit>, u: &Unit) -> Body {
    let p = u.position();
    let [middle, radius, _] = crate::combat::combat::blast_body(posture_of(u));
    Body {
        key: LifeKey::of(reference, u),
        reference,
        feet: [p.x, p.y, p.z],
        center: [p.x, p.y + middle, p.z],
        radius,
        team: u.team,
    }
}

fn footing_of(u: &Unit) -> Footing {
    if u.seat.is_some() || matches!(u.held, crate::store::Nullable::Value(_)) {
        return Footing::Seated;
    }
    match &u.soldier {
        UnitBody::Soldier(s) => match s.air {
            Some(Air::Thrown) => Footing::Thrown,
            Some(Air::Jet) => Footing::Jet,
            Some(Air::Jump) | Some(Air::Fall) => Footing::Air,
            None => Footing::Ground,
        },
        UnitBody::Droideka(_) => Footing::Seated,
    }
}

/// Whether static geometry blocks the line from `from` to `to`.
fn blocked(w: &mut World, from: V3, to: V3) -> bool {
    let delta = om::sub(to, from);
    let span = om::length(delta);
    if span < 1e-3 {
        return false;
    }
    let dir = om::divide(delta, span);
    let world = crate::units::f3::collision_world_in(&w.units);
    if world >= w.collision.state.worlds.len() {
        return false;
    }
    let origin = Vector3::new(from[0], from[1], from[2]);
    let direction = Vector3::new(dir[0], dir[1], dir[2]);
    crate::units::native::cast_static_in(w, world, &origin, &direction, span, true)
        .is_some_and(|hit| hit.distance < span - 0.05)
}

fn live_bodies(w: &World, skip: Ref<Unit>, mut visit: impl FnMut(Body, &Unit)) {
    for member in &w.sim.store.units.list {
        let Member::Unit(r) = *member else { continue };
        if r == skip {
            continue;
        }
        let Some(u) = w.sim.store.unit.get(r) else { continue };
        if alive(u) {
            visit(body_of(r, u), u);
        }
    }
}

struct Caster {
    key: LifeKey,
    reference: Ref<Unit>,
    origin: V3,
    yaw: f64,
    pitch: f64,
    team: f64,
}

impl Caster {
    fn axis(&self) -> V3 {
        AimFrame::from_angles(self.yaw, self.pitch).forward
    }
}

fn caster_of(w: &World, reference: Ref<Unit>, lock: Option<LifeKey>) -> Option<Caster> {
    let u = w.sim.store.unit.get(reference)?;
    let UnitBody::Soldier(s) = &u.soldier else { return None };
    let origin = crate::combat::live::origin(u, &u.combat);
    let (mut yaw, mut pitch) = (s.aim_yaw, s.aim_pitch);
    // A locked soldier aims its powers at the target it holds.
    if let Some(target) = lock.and_then(|k| k.unit(&w.sim.store).map(|t| (k, t))).filter(|(_, t)| alive(t)).map(|(k, t)| body_of(k.reference(), t)) {
        let d = om::sub(target.center, origin);
        let flat = v8math::hypot(d[0], d[2]);
        if flat > 1e-6 || v8math::abs(d[1]) > 1e-6 {
            yaw = v8math::atan2(d[0], d[2]);
            pitch = v8math::atan2(d[1], flat);
        }
    }
    Some(Caster { key: LifeKey::of(reference, u), reference, origin, yaw, pitch, team: u.team })
}

impl Powers {
    /// The fixed tick the timers count from (tests start it near its bound).
    #[cfg(test)]
    pub(super) fn tick_for_test(&self) -> u32 {
        self.tick
    }

    #[cfg(test)]
    pub(super) fn start_tick_for_test(&mut self, tick: u32) {
        self.tick = tick;
    }
}

impl Powers {
    /// Build every class's kit from its prepared unit record and size the
    /// per-life tables for `slots` unit slots.
    pub fn new<'a>(slots: usize, classes: impl IntoIterator<Item = (&'a str, &'a Value)>, explosion: impl Fn(&str) -> Option<Value>) -> Result<Self, PowerDefError> {
        let mut kits = Vec::new();
        let mut aura_effects: Vec<(u32, String)> = Vec::new();
        for (name, record) in classes {
            let mut kit = PowerKit::default();
            for weapon in record.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
                // Any weapon may give the lock its reach; an unreadable one gives none.
                kit.lock_range = v8math::max(kit.lock_range, def::lock_range(weapon).unwrap_or(0.0));
                // A blade that names an explosion sets it off from its combo.
                if weapon.get("base").and_then(Value::as_str) == Some("melee") {
                    if let Some(blast) = weapon.get("ExplosionName").and_then(Value::as_str).filter(|n| !n.is_empty()) {
                        let record = explosion(blast).ok_or_else(|| PowerDefError { weapon: blast.to_owned(), message: "an explosion missing from the prepared classes" })?;
                        kit.blast = Some(def::Blast::from_record(blast, &record)?);
                        kit.blast_weapon = weapon.get("class").and_then(Value::as_str).map_or(0, zero_fnv1a_lower);
                    }
                }
                let Some(def) = def::from_weapon(weapon)? else { continue };
                if let Some(fx) = weapon
                    .get("ordnance")
                    .and_then(|o| o.get("PlayEffectOnOwner"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    if !aura_effects.iter().any(|(w, _)| *w == def.weapon) {
                        aura_effects.push((def.weapon, fx.to_string()));
                    }
                }
                let slot = weapon.get("slot").and_then(Value::as_f64).unwrap_or(0.0) as u32;
                if !kit.push(slot, def) {
                    return Err(PowerDefError { weapon: name.to_owned(), message: "more powers than weapon slots" });
                }
            }
            if !kit.is_empty() {
                kits.push((zero_fnv1a_lower(name), kit));
            }
        }
        Ok(Self {
            kits,
            aura_effects,
            lives: vec![None; slots],
            regen_scratch: Vec::with_capacity(slots),
            burn_scratch: Vec::with_capacity(slots),
            log: PowerLog::default(),
            published: Vec::with_capacity(PUBLISHED),
            tick: 0,
        })
    }

    /// The kit of a class, by class name.
    pub fn kit_of(&self, class: &str) -> Option<u16> {
        let hash = zero_fnv1a_lower(class);
        self.kits.iter().position(|(h, _)| *h == hash).map(|i| i as u16)
    }

    pub fn kit(&self, kit: u16) -> Option<&PowerKit> {
        self.kits.get(usize::from(kit)).map(|(_, k)| k)
    }

    pub fn log(&self) -> &PowerLog {
        &self.log
    }

    /// Populate `out` with every active caster animation, choke victim and the
    /// bounded power event log without heap allocation.
    pub fn fill_frame(&self, w: &World, out: &mut PowerFrame) {
        let now = self.tick as u32;
        out.tick = now;
        out.log = self.log;
        out.clear();
        for row in self.lives.iter().flatten() {
            if row.anim.weapon == 0 {
                continue;
            }
            if !row.life.unit(&w.sim.store).is_some_and(alive) {
                continue;
            }
            let held = w.combat.power_effects.held(row.life);
            out.units.push(UnitPowerPose {
                slot: row.life.slot,
                generation: row.life.life,
                weapon: row.anim.weapon,
                fire_anim: row.anim.fire_anim,
                elapsed_ticks: now.saturating_sub(row.anim.start_tick),
                holding: row.anim.holding,
                choked: held.is_some(),
                choke_anim: held.map_or(0, |h| h.anim),
                ..UnitPowerPose::default()
            });
        }
        for (life, held) in w.combat.power_effects.held_entries() {
            if !life.unit(&w.sim.store).is_some_and(alive) {
                continue;
            }
            if let Some(existing) = out
                .units
                .iter_mut()
                .find(|u| u.slot == life.slot && u.generation == life.life)
            {
                existing.choked = true;
                existing.choke_anim = held.anim;
            } else {
                out.units.push(UnitPowerPose {
                    slot: life.slot,
                    generation: life.life,
                    choked: true,
                    choke_anim: held.anim,
                    ..UnitPowerPose::default()
                });
            }
        }
    }

    /// The target a life holds locked.
    pub fn lock(&self, life: LifeKey) -> Option<LifeKey> {
        self.row(life).and_then(|r| r.lock.target)
    }

    /// The cast pose a life shows: its power's weapon class hash, its fire slot, the
    /// ticks since the pose began and whether the power is held.
    pub fn cast_pose(&self, life: LifeKey) -> Option<(u32, u8, u32, bool)> {
        let row = self.row(life)?;
        let elapsed = (self.tick as u32).saturating_sub(row.anim.start_tick);
        (row.anim.weapon != 0).then_some((row.anim.weapon, row.anim.fire_anim, elapsed, row.anim.holding))
    }

    /// The trigger state of a life.
    pub fn state(&self, life: LifeKey) -> Option<&PowerState> {
        self.row(life).map(|r| &r.state)
    }

    /// Visits every life currently holding an area power (such as Force Choke),
    /// yielding `(caster, weapon_hash, first_held_target)`.
    pub fn visit_holds(&self, mut visit: impl FnMut(LifeKey, u32, Option<LifeKey>)) {
        for row in self.lives.iter().flatten() {
            let Some(power_idx) = row.state.holding() else { continue };
            let Some(def) = self.kit(row.kit).and_then(|k| k.get(power_idx)) else { continue };
            let target = row.held.iter().flatten().copied().next();
            visit(row.life, def.weapon, target);
        }
    }

    fn row(&self, life: LifeKey) -> Option<&LifeRow> {
        self.lives.get(life.slot as usize)?.as_ref().filter(|r| r.life == life)
    }

    fn event(&mut self, caster: LifeKey, target: Option<LifeKey>, weapon: u32, kind: PowerEventKind, bot: bool) {
        let tick = self.tick;
        let event = PowerEvent { sequence: 0, tick, caster, target, weapon, kind, bot };
        self.log.push(event);
        // Holds (every tick of a choke) and auras (their own named effect)
        // show nothing of their own: only the rest travel to the viewers.
        let shown = matches!(kind, PowerEventKind::Cast | PowerEventKind::Hit | PowerEventKind::Thrown | PowerEventKind::Blocked);
        if shown && self.published.len() < self.published.capacity() {
            self.published.push((event, None));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn forward(&mut self, w: &mut World, host: &mut dyn PowerHost, caster: Ref<Unit>, kit: u16, input: Forward, energy: &mut swbf2::State, tuning: &swbf2::Tuning) -> PowerStep {
        let idle = PowerStep { thrust: 1.0, ..PowerStep::default() };
        let Some(kit_def) = self.kit(kit).copied() else { return idle };
        let Some(u) = w.sim.store.unit.get(caster) else { return idle };
        let life = LifeKey::of(caster, u);
        let can_act = alive(u)
            && matches!(&u.soldier, UnitBody::Soldier(_))
            && energy.tumble.is_none()
            && !matches!(footing_of(u), Footing::Thrown | Footing::Seated)
            && w.combat.power_effects.held(life).is_none();
        let Some(slot) = self.lives.get_mut(life.slot as usize) else { return idle };
        if slot.is_none_or(|r| r.life != life || r.kit != kit) {
            *slot = Some(LifeRow {
                life,
                kit,
                state: PowerState::new(&kit_def),
                anim: AnimState::default(),
                lock: LockState::default(),
                held: [None; MAX_AREA_TARGETS as usize],
                bot_wait: 0,
            });
        }
        let mut row = slot.unwrap();
        self.update_lock(w, &mut row, &kit_def, input);
        let power_input = if input.bot { self.bot_input(w, &mut row, &kit_def, input.power, energy, tuning) } else { input.power };
        let before = row.state;
        let step = row.state.step(&kit_def, power_input, can_act, energy, tuning);
        if !can_act {
            row.held = [None; MAX_AREA_TARGETS as usize];
        }
        for request in step.requests() {
            match request {
                Request::Fire(i) => {
                    if let Some(def) = kit_def.get(i).copied() {
                        self.event(life, None, def.weapon, PowerEventKind::Cast, input.bot);
                        match def.effect {
                            Effect::Cone(cone) => self.strike(w, host, &row, &def, &cone, input.bot),
                            Effect::Aura(aura) => self.give(w, &row, &def, &aura, input.bot),
                            Effect::Area(_) => {}
                        }
                    }
                }
                Request::Acquire(i) => match kit_def.get(i).map(|d| (*d, d.effect)) {
                    // The effect starts on the tick the targets are found.
                    Some((def, Effect::Area(area))) if self.acquire(w, &mut row, &area) > 0 => {
                        self.event(life, None, def.weapon, PowerEventKind::Cast, input.bot);
                        if self.sustain(w, host, &mut row, &def, &area, false, true, input.bot) == 0 {
                            row.state.release(&kit_def, i, power_input.held);
                        }
                    }
                    _ => row.state.release(&kit_def, i, power_input.held),
                },
                Request::Sustain { power, recheck } => {
                    if let Some((def, Effect::Area(area))) = kit_def.get(power).map(|d| (*d, d.effect)) {
                        if self.sustain(w, host, &mut row, &def, &area, recheck, recheck, input.bot) == 0 {
                            row.state.release(&kit_def, power, power_input.held);
                        }
                    }
                }
            }
        }
        if row.state.holding().is_none() {
            row.held = [None; MAX_AREA_TARGETS as usize];
        }
        self.update_anim(&mut row, &kit_def, &before, &step, power_input, can_act, energy, tuning);
        self.lives[life.slot as usize] = Some(row);
        step
    }

    /// Start, continue or end the caster's power pose from each power's
    /// phase before and after this tick.
    #[allow(clippy::too_many_arguments)]
    fn update_anim(
        &self,
        row: &mut LifeRow,
        kit: &PowerKit,
        before: &PowerState,
        step: &PowerStep,
        input: PowerInput,
        can_act: bool,
        energy: &swbf2::State,
        tuning: &swbf2::Tuning,
    ) {
        if !can_act {
            row.anim = AnimState::default();
            return;
        }
        let fresh = |def: &PowerDef, idx: u8, holding: bool| AnimState {
            weapon: def.weapon,
            power: idx,
            fire_anim: def.fire_anim,
            start_tick: self.tick as u32,
            holding,
            was_holding_area: false,
        };
        for (idx, def) in kit.iter() {
            let fired = matches!(step.requests[usize::from(idx)], Some(Request::Fire(_) | Request::Acquire(_)));
            let was_active = before.is_active(idx);
            let started_windup = !was_active && row.state.is_winding_up(idx);
            let from_idle = !was_active && !before.is_recovering(idx);
            let held_pose = matches!(def.effect, Effect::Area(_)) || (!def.trigger.single && input.held);
            if from_idle && (started_windup || fired) {
                // A fresh press from idle (with InstantPlayFireAnim, or a
                // 0-tick windup firing at once).
                if def.instant_anim || fired {
                    row.anim = fresh(def, idx, held_pose);
                }
            } else if started_windup {
                // A repeating trigger (Force Lightning) restarts its windup
                // from recovery while held; keep start_tick so the
                // outstretched hand holds seamlessly across bursts.
                if before.is_recovering(idx) && row.anim.weapon == def.weapon && row.anim.power == idx && !def.trigger.single {
                    row.anim.holding = true;
                } else if def.instant_anim {
                    row.anim = fresh(def, idx, held_pose);
                }
            }
            // Non-instant powers begin their animation on emission (Fire / Acquire).
            if fired {
                let continuing_repeat = !def.trigger.single && row.anim.weapon == def.weapon && row.anim.power == idx;
                if !def.instant_anim && !continuing_repeat {
                    let holding = row.state.is_holding(idx) || (!def.trigger.single && input.held);
                    row.anim = fresh(def, idx, holding);
                }
            }
        }
        if row.anim.weapon == 0 {
            return;
        }
        let power = row.anim.power;
        let Some(def) = kit.get(power) else {
            row.anim = AnimState::default();
            return;
        };
        if row.state.is_holding(power) {
            row.anim.holding = true;
            row.anim.was_holding_area = true;
        } else if row.anim.was_holding_area {
            // Leaving a choke hold ends the held pose immediately so it blends out cleanly.
            row.anim = AnimState::default();
        } else if row.state.is_winding_up(power) {
            row.anim.holding = matches!(def.effect, Effect::Area(_)) || !def.trigger.single;
        } else if row.state.is_recovering(power) {
            row.anim.holding = !def.trigger.single
                && input.held
                && input.selected == Some(power)
                && Self::affordable(def, &row.state, power, energy, tuning);
        } else {
            // This power is done. When the wheel came back to an area power that still
            // holds, its hold pose shows again: the pose follows the selected power, and
            // that one holds.
            let back = row.state.holding().filter(|&h| h != power && input.selected == Some(h));
            if let Some((held, held_def)) = back.and_then(|h| kit.get(h).map(|d| (h, d))) {
                row.anim = AnimState { was_holding_area: true, ..fresh(held_def, held, true) };
                return;
            }
            row.anim.holding = false;
            if (self.tick as u32).saturating_sub(row.anim.start_tick) > 600 {
                row.anim = AnimState::default();
            }
        }
    }

    fn update_lock(&mut self, w: &mut World, row: &mut LifeRow, kit: &PowerKit, input: Forward) {
        if let Some(target) = row.lock.target {
            let caster = caster_of(w, row.life.reference(), None);
            let target_body = target.unit(&w.sim.store).filter(|t| alive(t)).map(|t| body_of(target.reference(), t));
            match (caster, target_body) {
                (Some(c), Some(t)) => {
                    let seen = !blocked(w, c.origin, t.center);
                    row.lock.keep(om::distance_to(c.origin, t.center), seen);
                }
                _ => row.lock.clear(),
            }
        }
        if !input.lock_pressed {
            return;
        }
        if row.lock.target.is_some() {
            row.lock.clear();
            return;
        }
        let Some(c) = caster_of(w, row.life.reference(), None) else { return };
        // The selected power's own reach while its trigger is down, else the class's longest.
        let selected = input.power.selected.filter(|_| input.power.held || input.power.pressed);
        let range = selected.and_then(|i| kit.get(i)).map(|d| d.lock_range).filter(|r| *r > 0.0).unwrap_or(kit.lock_range);
        let axis = c.axis();
        let mut near: Nearest<LockCandidate, MAX_CANDIDATES> = Nearest::default();
        live_bodies(w, c.reference, |b, _| {
            if !enemy(c.team, b.team) {
                return;
            }
            let d = om::sub(b.center, c.origin);
            let distance = om::length(d);
            if distance > 1e-6 && distance <= range {
                near.insert(distance, LockCandidate { life: b.key, distance, alignment: om::dot(om::divide(d, distance), axis) });
            }
        });
        let mut seen: Nearest<LockCandidate, MAX_CANDIDATES> = Nearest::default();
        for (key, candidate) in near.iter() {
            let Some(t) = candidate.life.unit(&w.sim.store) else { continue };
            let center = body_of(candidate.life.reference(), t).center;
            if !blocked(w, c.origin, center) {
                seen.insert(key, candidate);
            }
        }
        if let Some(best) = LockState::choose(range, seen.iter().map(|(_, c)| c)) {
            row.lock.take(best.life, range);
        }
    }

    /// Fire a cone at the bodies in it, nearest first.
    fn strike(&mut self, w: &mut World, host: &mut dyn PowerHost, row: &LifeRow, def: &PowerDef, cone: &Cone, bot: bool) {
        let Some(c) = caster_of(w, row.life.reference(), row.lock.target) else { return };
        let axis = c.axis();
        let mut near: Nearest<Body, MAX_CANDIDATES> = Nearest::default();
        let mut count = 0usize;
        live_bodies(w, c.reference, |b, _| {
            if cone.relations.accepts(enemy(c.team, b.team)) {
                if let Some(distance) = geometry::in_cone(cone, c.origin, axis, b.center) {
                    count += 1;
                    near.insert(distance, b);
                }
            }
        });
        let amount = cone.damage_per_target(0.0, count);
        let mut struck = 0u8;
        for (_, target) in near.iter() {
            if struck >= cone.max_targets {
                break;
            }
            if blocked(w, c.origin, target.center) {
                continue;
            }
            struck += 1;
            if self.intercepted(w, host, target, c.origin) {
                self.event(c.key, Some(target.key), def.weapon, PowerEventKind::Blocked, bot);
                continue;
            }
            let health = crate::units::f3::health_scale_in(&w.units, target.reference.slot);
            let scaled = amount * def.scale(health);
            if scaled > 0.0 {
                self.hurt(w, host, &c, target, def.weapon, scaled);
            }
            let thrown = cone.push != 0.0 && self.push(w, host, cone, c.origin, target);
            self.event(c.key, Some(target.key), def.weapon, if thrown { PowerEventKind::Thrown } else { PowerEventKind::Hit }, bot);
        }
    }

    /// A guarding defender facing the caster within its deflect angle stops
    /// an emitter as it stops a blade.
    fn intercepted(&mut self, w: &mut World, host: &mut dyn PowerHost, target: Body, from: V3) -> bool {
        let Some(guard) = host.guard(w, target.key) else { return false };
        let Some(t) = target.key.unit(&w.sim.store) else { return false };
        let UnitBody::Soldier(s) = &t.soldier else { return false };
        let angle = relative_horizontal_angle_deg(s.aim_yaw, om::sub(from, target.feet));
        angle_in_range_deg(angle, guard.min_angle_deg, guard.max_angle_deg) && host.pay_guard(w, target.key, guard.energy_cost)
    }

    fn hurt(&mut self, w: &mut World, host: &mut dyn PowerHost, c: &Caster, target: Body, weapon: u32, amount: f64) {
        self.hurt_from(w, host, c.reference, c.origin, target, weapon, amount);
    }

    #[allow(clippy::too_many_arguments)]
    fn hurt_from(&mut self, w: &mut World, host: &mut dyn PowerHost, caster: Ref<Unit>, origin: V3, target: Body, weapon: u32, amount: f64) {
        let hit = PowerHit { caster, target: target.reference, weapon, amount, from: origin };
        let from = Vector3::new(origin[0], origin[1], origin[2]);
        host.damage(w, hit, &mut |w| {
            if w.sim.store.unit.get(hit.caster).is_none() || w.sim.store.unit.get(hit.target).is_none_or(|u| !alive(u)) {
                return;
            }
            crate::units::f3::with_services_in(w, |w, services| {
                crate::combat::live::arc_hit_in(w, crate::combat::live::ArcVictim::Unit(hit.target), hit.amount, from, hit.caster, services);
            });
        });
    }

    /// Replace a soldier's velocity with a cone's push. Returns whether it was thrown.
    fn push(&mut self, w: &mut World, host: &mut dyn PowerHost, cone: &Cone, origin: V3, target: Body) -> bool {
        let Some(t) = target.key.unit(&w.sim.store) else { return false };
        if !alive(t) {
            return false;
        }
        let footing = host.footing(w, target.key).unwrap_or_else(|| footing_of(t));
        let UnitBody::Soldier(s) = &t.soldier else { return false };
        let normal = s.normal;
        let Some(source) = geometry::cone_push(cone, origin, target.center) else { return false };
        let Some((velocity, strength)) = geometry::push_velocity(&source, target.center, target.radius) else { return false };
        let Some(reaction) = geometry::reaction(footing, velocity, strength, normal) else { return false };
        let thrown = matches!(reaction, Reaction::Thrown | Reaction::Tumble);
        if let Some(UnitBody::Soldier(s)) = target.key.unit_mut(&mut w.sim.store).map(|u| &mut u.soldier) {
            s.velocity = velocity;
            if reaction == Reaction::Thrown {
                s.air = Some(Air::Thrown);
                s.state_time = 0.0;
                s.grounded = false;
                s.lock = 0.0;
            } else if reaction == Reaction::Tumble {
                s.air = None;
                s.posture = Posture::Crouch;
                s.state_time = 0.0;
                s.grounded = true;
                s.lock = 0.0;
            }
        }
        crate::units::f3::changed_in(w, target.reference);
        if reaction == Reaction::Thrown {
            host.thrown(w, target.key);
        } else if reaction == Reaction::Tumble {
            host.tumbled(w, target.key);
        }
        thrown
    }

    /// A blast from where `caster` stands: damage and push fall off from
    /// its radii to each body in sight, the character spared unless `HurtOwner`.
    fn blast(&mut self, w: &mut World, host: &mut dyn PowerHost, caster: Ref<Unit>, weapon: u32, blast: &def::Blast) {
        let Some(u) = w.sim.store.unit.get(caster) else { return };
        let owner = LifeKey::of(caster, u);
        let bot = !u.human;
        let p = u.position();
        let origin = [p.x, p.y, p.z];
        self.event(owner, None, weapon, PowerEventKind::Cast, bot);
        let reach = v8math::max(blast.outer, blast.push_outer);
        let mut near: Nearest<Body, MAX_CANDIDATES> = Nearest::default();
        let mut consider = |b: Body| {
            let distance = om::distance_to(b.center, origin) - 0.5 * b.radius;
            if distance < reach {
                near.insert(distance, b);
            }
        };
        live_bodies(w, caster, |b, _| consider(b));
        if blast.hurt_owner && alive(u) {
            consider(body_of(caster, u));
        }
        for (distance, b) in near.iter() {
            let Some(t) = b.key.unit(&w.sim.store) else { continue };
            let [_, _, sight] = crate::combat::combat::blast_body(posture_of(t));
            if blocked(w, [origin[0], origin[1] + 0.1, origin[2]], [b.feet[0], b.feet[1] + sight, b.feet[2]]) {
                continue;
            }
            let health = crate::units::f3::health_scale_in(&w.units, b.reference.slot);
            let damage = blast.damage * geometry::falloff(distance, blast.inner, blast.outer) * blast.scales[health as usize];
            if damage > 0.0 {
                self.hurt_from(w, host, caster, origin, b, weapon, damage);
            }
            let strength = blast.push * geometry::falloff(distance, blast.push_inner, blast.push_outer);
            let thrown = strength > 0.0 && self.shove(w, host, origin, b, strength);
            if damage > 0.0 || thrown {
                self.event(owner, Some(b.key), weapon, if thrown { PowerEventKind::Thrown } else { PowerEventKind::Hit }, bot);
            }
        }
    }

    /// An explosion's push: half of any velocity against it is taken away,
    /// then the lifted direction is added. Returns whether the body was thrown.
    fn shove(&mut self, w: &mut World, host: &mut dyn PowerHost, origin: V3, target: Body, strength: f64) -> bool {
        let Some(t) = target.key.unit(&w.sim.store) else { return false };
        if !alive(t) {
            return false;
        }
        let footing = host.footing(w, target.key).unwrap_or_else(|| footing_of(t));
        let UnitBody::Soldier(s) = &t.soldier else { return false };
        let (normal, before) = (s.normal, s.velocity);
        let delta = om::sub(target.center, origin);
        let span = om::length(delta);
        if !(span > 1e-4) {
            return false;
        }
        let mut dir = om::divide(delta, span);
        dir[1] += geometry::PUSH_LIFT;
        let against = om::dot(before, dir);
        let mut velocity = if against < 0.0 { om::add_scaled(before, dir, -0.5 * against) } else { before };
        velocity = om::add_scaled(velocity, dir, strength);
        let Some(reaction) = geometry::reaction(footing, velocity, strength, normal) else { return false };
        let thrown = matches!(reaction, Reaction::Thrown | Reaction::Tumble);
        if let Some(UnitBody::Soldier(s)) = target.key.unit_mut(&mut w.sim.store).map(|u| &mut u.soldier) {
            s.velocity = velocity;
            if reaction == Reaction::Thrown {
                s.air = Some(Air::Thrown);
                s.state_time = 0.0;
                s.grounded = false;
                s.lock = 0.0;
            } else if reaction == Reaction::Tumble {
                s.air = None;
                s.posture = Posture::Crouch;
                s.state_time = 0.0;
                s.grounded = true;
                s.lock = 0.0;
            }
        }
        crate::units::f3::changed_in(w, target.reference);
        if reaction == Reaction::Thrown {
            host.thrown(w, target.key);
        } else if reaction == Reaction::Tumble {
            host.tumbled(w, target.key);
        }
        thrown
    }

    /// Give an aura's modifiers to everyone it accepts within its radius.
    fn give(&mut self, w: &mut World, row: &LifeRow, def: &PowerDef, aura: &Aura, bot: bool) {
        let Some(c) = caster_of(w, row.life.reference(), None) else { return };
        let Some(feet) = w.sim.store.unit.get(c.reference).map(|u| u.position()).map(|p| [p.x, p.y, p.z]) else { return };
        let mut near: Nearest<Body, MAX_CANDIDATES> = Nearest::default();
        live_bodies(w, c.reference, |b, _| {
            let distance = om::distance_to(b.feet, feet);
            if aura.relations.accepts(enemy(c.team, b.team)) && distance <= aura.radius {
                near.insert(distance, b);
            }
        });
        let fx_name = self
            .aura_effects
            .iter()
            .find(|(w, _)| *w == def.weapon)
            .map(|(_, s)| s.clone());
        if let Some(ref fx) = fx_name {
            crate::units::f3::push_swbf2_effect_in(w, fx, feet, None, None, None);
        }
        let mut any = false;
        for (_, b) in near.iter() {
            if w.combat.power_effects.give(b.key, aura.offense, aura.defense, aura.regen) {
                any = true;
                self.event(c.key, Some(b.key), def.weapon, PowerEventKind::Buffed, bot);
            }
        }
        if any && aura.owner_with_others && aura.relations.friends && w.combat.power_effects.give(c.key, aura.offense, aura.defense, aura.regen) {
            self.event(c.key, Some(c.key), def.weapon, PowerEventKind::Buffed, bot);
        }
    }

    fn ray_order(w: &World, c: &Caster, area: &Area, prefer: Option<LifeKey>) -> Nearest<Body, MAX_CANDIDATES> {
        let frame = AimFrame::from_angles(c.yaw, c.pitch);
        let mut near: Nearest<Body, MAX_CANDIDATES> = Nearest::default();
        live_bodies(w, c.reference, |b, _| {
            if let Some(mut key) = geometry::in_ray(area, c.origin, &frame, b.center, b.radius) {
                // The held lock is preferred over every other target in the area.
                if prefer == Some(b.key) {
                    key *= 1e-4;
                }
                near.insert(key, b);
            }
        });
        near
    }

    /// Pick the targets an area weapon will hold. Returns how many.
    fn acquire(&mut self, w: &mut World, row: &mut LifeRow, area: &Area) -> usize {
        row.held = [None; MAX_AREA_TARGETS as usize];
        let Some(c) = caster_of(w, row.life.reference(), None) else { return 0 };
        let near = Self::ray_order(w, &c, area, row.lock.target);
        let mut n = 0usize;
        for (_, b) in near.iter() {
            if n >= usize::from(area.max_targets) {
                break;
            }
            if !blocked(w, c.origin, b.center) {
                row.held[n] = Some(b.key);
                n += 1;
            }
        }
        n
    }

    /// Hold, hurt and recheck each held target for one tick. Returns how many remain.
    #[allow(clippy::too_many_arguments)]
    fn sustain(&mut self, w: &mut World, host: &mut dyn PowerHost, row: &mut LifeRow, def: &PowerDef, area: &Area, recheck: bool, announce: bool, bot: bool) -> usize {
        let Some(c) = caster_of(w, row.life.reference(), None) else { return 0 };
        let frame = AimFrame::from_angles(c.yaw, c.pitch);
        let mut left = 0usize;
        for i in 0..row.held.len() {
            let Some(key) = row.held[i] else { continue };
            let target = key.unit(&w.sim.store).filter(|t| alive(t)).map(|t| (body_of(key.reference(), t), footing_of(t)));
            let Some((b, body_footing)) = target else {
                row.held[i] = None;
                continue;
            };
            if recheck && (geometry::in_ray(area, c.origin, &frame, b.center, b.radius).is_none() || blocked(w, c.origin, b.center)) {
                row.held[i] = None;
                continue;
            }
            left += 1;
            if area.damage > 0.0 {
                self.hurt(w, host, &c, b, def.weapon, area.damage * DT);
            }
            let footing = host.footing(w, key).unwrap_or(body_footing);
            if area.strength > 0.0 && footing == Footing::Ground && key.unit(&w.sim.store).is_some_and(alive) {
                let toward = om::sub(c.origin, b.center);
                w.combat.power_effects.hold(key, c.key, toward, def.soldier_anim);
                if announce {
                    self.event(c.key, Some(key), def.weapon, PowerEventKind::Held, bot);
                }
            }
        }
        left
    }

    fn affordable(def: &PowerDef, state: &PowerState, index: u8, energy: &swbf2::State, tuning: &swbf2::Tuning) -> bool {
        if def.cost.rounds.is_some() {
            return state.rounds(index) > 0;
        }
        !energy.exhausted && energy.energy >= v8math::min(def.cost.energy, tuning.energy_capacity)
    }

    /// Bot power gating: damaging cones follow weapon-AI triggers, auras cast
    /// only in combat with enough nearby allies, and zero-damage cones or held
    /// area weapons are never cast by bots.
    fn bot_input(&mut self, w: &mut World, row: &mut LifeRow, kit: &PowerKit, trigger: PowerInput, energy: &swbf2::State, tuning: &swbf2::Tuning) -> PowerInput {
        if let Some(index) = trigger.selected {
            if let Some(Effect::Cone(cone)) = kit.get(index).map(|d| d.effect).filter(|e| matches!(e, Effect::Cone(c) if c.damage > 0.0)) {
                if trigger.pressed || trigger.held {
                    // A bot neither starts nor keeps holding a damaging cone
                    // that no enemy it can see is inside.
                    if !Self::enemy_in_reach(w, row, &cone) {
                        return PowerInput { selected: trigger.selected, pressed: false, held: false };
                    }
                    return trigger;
                }
            }
        }
        if let Some(power) = row.state.active() {
            if matches!(kit.get(power).map(|d| d.effect), Some(Effect::Cone(cone)) if cone.damage > 0.0) {
                // The trigger goes on with the cone's selection only: a bot's
                // trigger never starts a power that is not a damaging cone.
                if trigger.selected == Some(power) {
                    return trigger;
                }
                return PowerInput { selected: trigger.selected, pressed: false, held: false };
            }
            return PowerInput { selected: Some(power), pressed: false, held: false };
        }
        if row.bot_wait > 0 {
            row.bot_wait -= 1;
            return PowerInput::default();
        }
        row.bot_wait = BOT_AURA_CHECK_TICKS;
        let Some(c) = caster_of(w, row.life.reference(), row.lock.target) else { return PowerInput::default() };
        for (index, def) in kit.iter() {
            let Effect::Aura(aura) = def.effect else { continue };
            if !Self::affordable(def, &row.state, index, energy, tuning) {
                continue;
            }
            if Self::bot_aura(w, &c, &aura) {
                return PowerInput { selected: Some(index), pressed: true, held: true };
            }
        }
        PowerInput::default()
    }

    /// Whether an enemy the caster can see stands inside `cone` as the caster
    /// aims it: the reach, cone and sight test of a strike, enemies only.
    fn enemy_in_reach(w: &mut World, row: &LifeRow, cone: &Cone) -> bool {
        let Some(c) = caster_of(w, row.life.reference(), row.lock.target) else { return false };
        let axis = c.axis();
        let mut near: Nearest<Body, MAX_CANDIDATES> = Nearest::default();
        live_bodies(w, c.reference, |b, _| {
            if enemy(c.team, b.team) {
                if let Some(distance) = geometry::in_cone(cone, c.origin, axis, b.center) {
                    near.insert(distance, b);
                }
            }
        });
        for (_, b) in near.iter() {
            if !blocked(w, c.origin, b.center) {
                return true;
            }
        }
        false
    }

    fn bot_in_combat(w: &World, caster: Ref<Unit>) -> Option<bool> {
        let battle = w.ai.battle()?;
        let u = w.sim.store.unit.get(caster)?;
        let Some(ctl) = u.controller else { return Some(false) };
        let Some(ci) = battle.state.controllers.core_reference(ctl) else { return Some(false) };
        let Some(row) = battle.runtime.core.controllers.get(ci) else { return Some(false) };
        Some(row.target.is_some() || row.commands.current.kind == crate::ai::control::controller::goal::COMBAT)
    }

    fn bot_aura(w: &World, c: &Caster, aura: &Aura) -> bool {
        if !aura.relations.friends {
            return false;
        }
        let Some(feet) = w.sim.store.unit.get(c.reference).map(|u| u.position()).map(|p| [p.x, p.y, p.z]) else { return false };
        let ai_combat = Self::bot_in_combat(w, c.reference);
        let mut threat = ai_combat.unwrap_or(false);
        if ai_combat == Some(false) {
            return false;
        }
        let mut friends = 1usize; // Include the caster itself.
        live_bodies(w, c.reference, |b, _| {
            if enemy(c.team, b.team) {
                if ai_combat.is_none() && om::distance_to(b.center, c.origin) <= BOT_AURA_THREAT_RANGE {
                    threat = true;
                }
            } else if om::distance_to(b.feet, feet) <= aura.radius {
                friends += 1;
            }
        });
        threat && friends >= BOT_AURA_MIN_FRIENDS
    }

    /// Count down every timer and restore health under regeneration.
    fn finish_tick(&mut self, w: &mut World, dt: f64) {
        self.regen_scratch.clear();
        w.combat.power_effects.tick(dt, |life, rate| {
            self.regen_scratch.push((life, rate));
        });
        for &(life, rate) in &self.regen_scratch {
            let Some(u) = life.unit_mut(&mut w.sim.store) else { continue };
            if !alive(u) {
                continue;
            }
            let health = v8math::min(u.combat.health + rate * dt, u.combat.max_health);
            if health != u.combat.health {
                u.combat.health = health;
                crate::units::f3::changed_in(w, life.reference());
            }
        }
        self.burn_scratch.clear();
        w.combat.power_effects.tick_burns(dt, |life, burn, emit_fx| {
            self.burn_scratch.push((life, burn, emit_fx));
        });
        for &(life, burn, emit_fx) in &self.burn_scratch {
            if !life.unit(&w.sim.store).is_some_and(alive) {
                w.combat.power_effects.clear_slot(life.slot);
                continue;
            }
            let attacker = if burn.attacker.unit(&w.sim.store).is_some() {
                Attacker::Unit(burn.attacker.reference())
            } else {
                Attacker::Null
            };
            crate::units::f3::with_services_in(w, |w, services| {
                crate::combat::live::burn_damage_in(w, life.reference(), burn.dps * dt, attacker, services);
            });
            // The burn's effect travels with the burning life (the viewer puts
            // it on the bones the weapon names) for the seconds it has left.
            if emit_fx && burn.seconds > 0.0 && self.published.len() < self.published.capacity() {
                if let Some(fx) = burn.effect.filter(|_| life.unit(&w.sim.store).is_some_and(alive)) {
                    let event = PowerEvent { sequence: 0, tick: self.tick, caster: burn.attacker, target: Some(life), weapon: 0, kind: PowerEventKind::Hit, bot: false };
                    self.published.push((event, Some((fx, burn.seconds))));
                }
            }
        }
        for slot in 0..self.lives.len() {
            let Some(row) = self.lives[slot] else { continue };
            if row.life.unit(&w.sim.store).is_some_and(alive) {
                continue;
            }
            self.lives[slot] = None;
            if w.combat.power_effects.get(row.life).is_some() {
                w.combat.power_effects.clear_slot(row.life.slot);
            }
        }
        self.tick = self.tick.saturating_add(1);
    }
}

/// Install the powers of a 2005 battle: kits for every class the battle can
/// field, keyed by class name, and per-life tables for `slots` unit slots.
pub fn install_in<'a>(w: &mut World, slots: usize, classes: impl IntoIterator<Item = (&'a str, &'a Value)>, explosion: impl Fn(&str) -> Option<Value>) -> Result<(), PowerDefError> {
    w.combat.powers = Some(Box::new(Powers::new(slots, classes, explosion)?));
    w.combat.power_effects = EffectTable::with_slots(slots);
    Ok(())
}

/// A combo state of `caster` played its blade's explosion (`PlayExplosion`).
pub fn play_explosion_in(w: &mut World, host: &mut dyn PowerHost, caster: Ref<Unit>, kit: u16) {
    let Some(mut powers) = w.combat.powers.take() else { return };
    if let Some((blast, weapon)) = powers.kit(kit).and_then(|k| k.blast.map(|b| (b, k.blast_weapon))) {
        powers.blast(w, host, caster, weapon, &blast);
    }
    w.combat.powers = Some(powers);
}

pub fn powers_in(w: &World) -> Option<&Powers> {
    w.combat.powers.as_deref()
}

/// What powers have left on every life: modifiers, regeneration, holds. The
/// table stays in the world while a pass runs, so every hit in the pass sees it.
pub fn effects_in(w: &World) -> &EffectTable {
    &w.combat.power_effects
}

/// Forward one life's trigger and lock button for this tick. `energy` and
/// `tuning` are the life's stamina account and class, as its movement uses them.
#[allow(clippy::too_many_arguments)]
pub fn forward_in(w: &mut World, host: &mut dyn PowerHost, caster: Ref<Unit>, kit: u16, input: Forward, energy: &mut swbf2::State, tuning: &swbf2::Tuning) -> PowerStep {
    let Some(mut powers) = w.combat.powers.take() else { return PowerStep { thrust: 1.0, ..PowerStep::default() } };
    let step = powers.forward(w, host, caster, kit, input, energy, tuning);
    w.combat.powers = Some(powers);
    step
}

/// Run every power timer once per fixed tick, after all lives were forwarded.
pub fn finish_tick_in(w: &mut World, dt: f64) {
    let Some(mut powers) = w.combat.powers.take() else { return };
    powers.finish_tick(w, dt);
    w.combat.powers = Some(powers);
}

/// The choke holding a life, for its movement and controls.
pub fn held_in(w: &World, life: LifeKey) -> Option<Held> {
    w.combat.power_effects.held(life)
}

/// The multiplier a 2005 battle's timed modifiers put on damage by `attacker`
/// to the life in `victim`: the attacker's offence times the victim's
/// defence. `None` when no modifier applies. Only the 2005 damage dispatch
/// asks, inside a power pass as well as outside it.
pub(crate) fn damage_scale_in(w: &World, attacker: Attacker, victim: LifeKey) -> Option<f64> {
    let attacker = match attacker {
        Attacker::Unit(r) => w.sim.store.unit.get(r).map(|u| LifeKey::of(r, u)),
        _ => None,
    };
    let scale = w.combat.power_effects.damage_scale(attacker, Some(victim));
    (scale != 1.0).then_some(scale)
}

/// Ensure the per-life storage has room for `slots`. It only grows: every
/// running wind-up, hold, lock and modifier is kept as it was.
pub fn reserve_in(w: &mut World, slots: usize) {
    if let Some(p) = w.combat.powers.as_mut() {
        if p.lives.len() < slots {
            p.lives.resize(slots, None);
        }
        p.regen_scratch.reserve(slots.saturating_sub(p.regen_scratch.len()));
        p.burn_scratch.reserve(slots.saturating_sub(p.burn_scratch.len()));
    }
    w.combat.power_effects.reserve(slots);
}

impl LifeRow {
    #[cfg(test)]
    fn held_targets(&self) -> impl Iterator<Item = LifeKey> + '_ {
        self.held.iter().flatten().copied()
    }
}

#[cfg(test)]
impl Powers {
    pub(crate) fn held_targets(&self, life: LifeKey) -> Vec<LifeKey> {
        self.row(life).map(|r| r.held_targets().collect()).unwrap_or_default()
    }
}

const _: () = assert!(MAX_POWERS <= u8::MAX as usize);

#[cfg(test)]
mod log_tests {
    use super::*;

    fn cast(tick: u32) -> PowerEvent {
        PowerEvent {
            sequence: 0,
            tick,
            caster: LifeKey { slot: 0, generation: 1, life: 1.0 },
            target: None,
            weapon: 0,
            kind: PowerEventKind::Cast,
            bot: false,
        }
    }

    #[test]
    fn the_power_log_follows_its_readers_and_saturates_at_the_bound() {
        let mut log = PowerLog::default();
        for tick in 0..70 {
            log.push(cast(tick));
        }
        assert_eq!(log.latest, 70);
        assert_eq!(log.since(0).count(), LOG_LEN);
        assert_eq!(log.since(68).map(|e| e.sequence).collect::<Vec<_>>(), [69, 70]);
        assert_eq!(log.since(71).count(), 0);
        log.latest = u32::MAX - 1;
        for tick in 0..5 {
            log.push(cast(tick));
        }
        assert_eq!(log.latest, u32::MAX);
        assert_eq!(log.since(u32::MAX).count(), 0);
        assert_eq!(log.since(u32::MAX - 2).last().map(|e| e.sequence), Some(u32::MAX));
    }
}
