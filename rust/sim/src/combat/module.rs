//! Native combat services over canonical unit rows. Construction loads
//! immutable definitions; the owning unit phase supplies retained scratch,
//! registry queries and ordnance services. Character bookkeeping is the
//! declared synchronous foreign leaf; presentation returns as notices.

use super::combat::{CombatDefs, Edge};
use super::defs::{DefTable, Js, JsObj, WeaponClass};
use super::weapons::{arm, Admission, Shot, ShotKind, WeaponDraws};
use super::world::{ArcTarget, CastHit, CombatWorld, DamageInfo, Droid, LockPick, Presentation};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{word_u32, DefId, Nullable, Ref, StrId, Strings};
use std::ptr::addr_of_mut;
use std::rc::Rc;
use three::Vector3;

/// The battle's content as combat reads it: weapon and unit classes
/// (cmb_weapon, cmb_class) and explosions (cmb_explosion). A command holds
/// its own handle on them for its whole run: a class registered while it
/// runs (a unit built by a hook) makes a new table rather than moving the
/// one it reads.
struct Content {
    defs: Rc<CombatDefs>,
    explosion_packets: Vec<super::impacts::BlastPacket>,
}

/// Combat definitions and the lent-body marker have one lifetime. The marker
/// is a disjoint scalar loan: holding an Rc definition never borrows it.
pub(crate) struct CombatOwner { content: Option<Content>, taken: Option<u32> }
impl CombatOwner {
    pub(crate) const fn new() -> Self { Self { content: None, taken: None } }
}

fn content() -> &'static mut Content {
    // SAFETY: one thread; nothing holds a reference to it across a call
    // out of the module (commands clone the Rc they read).
    unsafe {
        (*addr_of_mut!(crate::abi::cold().combat.content)).get_or_insert_with(|| Content {
            defs: Rc::new(CombatDefs::default()),
            explosion_packets: Vec::new(),
        })
    }
}

/// The combat classes and weapons the battle has given the module (a
/// command's or the units' phase's own handle on them).
pub(crate) fn defs() -> Rc<CombatDefs> { content().definitions() }

// PlayerCombat.zoom is a native seat read with no foreign callback. Borrow
// prepared definitions directly; each rider needs no new Rc owner or array.
pub(crate) fn zoom(c: &mut Combat) -> f64 { content().zoom(c) }

/// Numeric blast fields are cached with the same construction id as the
/// legacy definition. Victim traversal copies this small value, not a table.
pub(crate) fn blast_packet(id: u32) -> super::impacts::BlastPacket { content().blast_packet(id) }

fn unit_ref(id: u32) -> Ref<Unit> {
    sim().store.unit.reference(id).unwrap_or(Ref::new(id, 0))
}

// ---- Content: JavaScript values as words (src/f3-combat.js `valueWords`):
// 0 undefined, 1 null, 2 false, 3 true, 4 a number, 5 a string (its index),
// 6 an array (its length, then its items), 7 an object (its keys' count,
// then each key's index and value), 8 a weapon class by its index in the
// battle's weapon table, 9 anything else (an object combat never reads).

pub(crate) fn read_js(words: &[f64], at: &mut usize, strings: &Strings) -> Js {
    let tag = word_u32(words[*at]);
    *at += 1;
    match tag {
        0 => Js::Undefined,
        1 => Js::Null,
        2 => Js::Bool(false),
        3 => Js::Bool(true),
        4 => {
            let v = words[*at];
            *at += 1;
            Js::Num(v)
        }
        5 => {
            let id = word_u32(words[*at]);
            *at += 1;
            Js::Str(Rc::from(String::from_utf16_lossy(strings.units(StrId(id))).as_str()))
        }
        6 => {
            let n = word_u32(words[*at]) as usize;
            *at += 1;
            let items = (0..n).map(|_| read_js(words, at, strings)).collect();
            Js::Arr(Rc::new(items))
        }
        7 => {
            let n = word_u32(words[*at]) as usize;
            *at += 1;
            let mut fields = Vec::with_capacity(n);
            for _ in 0..n {
                let key = word_u32(words[*at]);
                *at += 1;
                let name: Rc<str> = Rc::from(String::from_utf16_lossy(strings.units(StrId(key))).as_str());
                fields.push((name, read_js(words, at, strings)));
            }
            Js::Obj(Rc::new(JsObj { fields, def: None }))
        }
        8 => {
            let id = word_u32(words[*at]);
            *at += 1;
            Js::Obj(Rc::new(JsObj {
                fields: Vec::new(),
                def: Some((DefTable::Weapon, DefId(id))),
            }))
        }
        _ => Js::Obj(Rc::new(JsObj::default())),
    }
}

/// The value JavaScript wrote into the call block.
fn call_value() -> Js {
    let s = sim();
    let (row, heap) = s.calls.args();
    let words: Vec<f64> = row.iter().chain(heap.iter()).copied().collect();
    let mut at = 0;
    read_js(&words, &mut at, &s.strings)
}

fn obj_of(v: Js) -> Rc<JsObj> {
    match v {
        Js::Obj(o) => o,
        _ => Rc::new(JsObj::default()),
    }
}

/// Weapon class `id` (the battle's weapon table), its definition in the
/// call block.
#[no_mangle]
pub extern "C" fn cmb_weapon(id: u32) { let value = call_value(); content().weapon(id, value); }

/// Unit class `id` (the battle's unitClass table): what combat reads of it
/// (AddHealth, base, its loadout), in the call block.
#[no_mangle]
pub extern "C" fn cmb_class(id: u32) { let value = call_value(); content().class(id, value); }

/// Explosion `id` (src/f3-combat.js's table), in the call block.
#[no_mangle]
pub extern "C" fn cmb_explosion(id: u32) { let value = call_value(); content().explosion(id, value); }

/// A command's answer in the call block: what it wrote into the unit, then
/// the combat's record. `flag`: a command's own answer (a weapon chosen).
fn answer(r: Ref<Unit>) -> *const f64 {
    let s = sim();
    let u = s.store.unit.get(r).expect("a constructed unit");
    s.calls.result(Combat::WORDS, |row, heap| u.combat.put(row, heap));
    s.calls.block.ptr()
}

/// The unit a command or the units' phase has out of the store, if any
/// (its slot holds an empty unit until it goes back): what JavaScript must
/// not write into meanwhile, and what the phase's syncs leave out.

/// The slot of the unit out of the store now.
pub(crate) fn taken() -> Option<u32> {
    // SAFETY: one thread, a copy out.
    unsafe { *addr_of_mut!(crate::abi::cold().combat.taken) }
}



// Unit.tick and Unit.onMoved own the complete body. Lend its combat in place
// too; splitting it just to reunite it would construct another empty Combat.
pub(crate) fn take_body(r: Ref<Unit>) -> Unit { CombatOwner::lend_body(&mut sim().store, unsafe { &mut *addr_of_mut!(crate::abi::cold().combat.taken) }, r) }
pub(crate) fn put_body(r: Ref<Unit>, unit: Unit) { CombatOwner::restore_body(&mut sim().store, unsafe { &mut *addr_of_mut!(crate::abi::cold().combat.taken) }, r, unit) }

// PlayerCombat methods borrow body and combat separately. Only that call
// frame needs an empty Combat field; movement and aim handoffs do not.
pub(crate) fn take(r: Ref<Unit>) -> (Unit, Combat) {
    let mut unit = take_body(r);
    let combat = std::mem::take(&mut unit.combat);
    (unit, combat)
}
pub(crate) fn put(r: Ref<Unit>, mut unit: Unit, combat: Combat) {
    unit.combat = combat;
    put_body(r,unit);
}

// ---- The commands (src/f3-combat.js CombatFacade).

/// A combat made for unit `unit` of class `class` (PlayerCombat's
/// constructor): full health, its class's loadout armed (a player's
/// weapons keep a player's spread floor), and the numbers JavaScript took
/// from its class (MaxHealth || 300, the chunk roll, the health scale's
/// name). The answer as every command's.
#[no_mangle]
pub extern "C" fn cmb_new(unit: u32, class: u32, player: u32, max_health: f64, chunks: f64, scale: u32) -> *const f64 {
    let defs = content().defs.clone();
    let r = unit_ref(unit);
    let weapons = arm(defs.loadout(DefId(class)), &defs.weapons, player != 0);
    let n = weapons.len();
    let combat = Combat {
        owner: r,
        player: player != 0,
        last_attacker: Attacker::Undefined,
        aim_override: Nullable::Null,
        max_health,
        health: max_health,
        dead: false,
        death_time: 0.0,
        team: 0.0,
        repair_target: None,
        repair_lock: sim().strings.intern_str(""),
        blow_apart: false,
        chunks,
        zoom_step: 0.0,
        kick: Kick {
            rate: 0.0,
            hold: 0.0,
            offset: 0.0,
        },
        scale: StrId(scale),
        first_person: false,
        mounted: Some(false),
        gun_underwater: false,
        defer_muzzles: false,
        pending_muzzles: Vec::new(),
        random: false,
        has_remotes: false,
        weapons,
        selected: [0.0, 0.0],
        seen: vec![
            Seen {
                known: false,
                reloading: false,
                throwing: false
            };
            n
        ],
        ..Combat::default()
    };
    {
        let s = sim();
        let u = s.store.unit.ensure(r);
        u.class_def = DefId(class);
        u.combat = combat;
    }
    answer(r)
}

/// weapons.js Arsenal.select (:517).
pub(crate) fn select(c: &mut Combat, defs: &CombatDefs, i: usize) -> bool {
    super::weapons::select(c,&defs.weapons,i)
}

/// A field the battle sets on a combat (src/f3-combat.js CombatFacade's
/// setters): 0 health, 1 zoomStep, 2 dead, 3 deathTime, 4 blowApart; and
/// what the battle's JavaScript keeps on a combat, which the units' phase
/// reads from the store: 5 mounted, 6 gunUnderwater, 7 team, 8 player,
/// 9 deferMuzzles, 10 whether it has a generator (random), 11 whether it
/// has recon droids (remotes).
#[no_mangle]
pub extern "C" fn cmb_set(unit: u32, field: u32, value: f64) -> u32 {
    let Ok(code) = u8::try_from(field) else {
        return 0;
    };
    let Some(field) = F3CombatField::from_u8(code) else {
        return 0;
    };
    if taken() == Some(unit) {
        return 0;
    }
    let Some((_, body)) = sim().store.unit.at_mut(unit) else {
        return 0;
    };
    let c = &mut body.combat;
    match field {
        F3CombatField::Health => c.health = value,
        F3CombatField::ZoomStep => c.zoom_step = value,
        F3CombatField::Dead => c.dead = value != 0.,
        F3CombatField::DeathTime => c.death_time = value,
        F3CombatField::BlowApart => c.blow_apart = value != 0.,
        F3CombatField::Mounted => c.mounted = Some(value != 0.),
        F3CombatField::GunUnderwater => c.gun_underwater = value != 0.,
        F3CombatField::Team => c.team = value,
        F3CombatField::Player => c.player = value != 0.,
        F3CombatField::DeferMuzzles => c.defer_muzzles = value != 0.,
        F3CombatField::Random => c.random = value != 0.,
        F3CombatField::Remotes => c.has_remotes = value != 0.,
    }
    1
}

/// WeaponFacade settings validate their generated field and arsenal index.
/// A rejected command leaves the row intact and reports failure to its host.
#[no_mangle]
pub extern "C" fn cmb_weapon_set(unit: u32, weapon: u32, field: u32, value: f64) -> u32 {
    let Ok(code) = u8::try_from(field) else {
        return 0;
    };
    let Some(field) = F3WeaponField::from_u8(code) else {
        return 0;
    };
    if taken() == Some(unit) {
        return 0;
    }
    let Some((_, body)) = sim().store.unit.at_mut(unit) else {
        return 0;
    };
    let Some(w) = body.combat.weapons.get_mut(weapon as usize) else {
        return 0;
    };
    match field {
        F3WeaponField::Reserve => w.reserve = value,
        F3WeaponField::Kick => w.kick = value,
        F3WeaponField::Clip => w.clip = value,
        F3WeaponField::Heat => w.heat = value,
        F3WeaponField::Floor => w.floor = value,
        F3WeaponField::Random => w.random = value != 0.,
        F3WeaponField::SpreadScale => w.spread_scale = value,
        F3WeaponField::YawSpreadOverride => w.yaw_spread_override = Some(value),
        F3WeaponField::ClearYawSpread => w.yaw_spread_override = None,
    }
    1
}

// ---- The battle, as the imports give it.

fn a3(v: &Vector3) -> Vec3 {
    [v.x, v.y, v.z]
}

/// The battle as the module's imports give it, for unit `unit`'s combat.
pub(crate) struct JsWorld<'a> {
    unit: f64,
    r: Ref<Unit>,
    /// Whether he has recon droids (combat.remotes): without, no droid of
    /// his is out and nobody is asked.
    remotes: bool,
    team: f64,
    life: f64,
    character: Option<Ref<Character>>,
    human: bool,
    // Game.arcTargets snapshots one acquisition. Nested blasts can mutate
    // its victims, but cannot replace which index each later link names.
    arc: Option<Vec<super::live::ArcVictim>>,
    recon: Option<crate::units::f3::WorldServices<'a>>,
}

impl<'a> JsWorld<'a> {
    pub(crate) fn with_services(r: Ref<Unit>, unit: &Unit, combat: &Combat, recon: Option<crate::units::f3::WorldServices<'a>>) -> Self {
        Self {
            unit: r.slot as f64,
            r,
            remotes: combat.has_remotes,
            team: combat.team,
            life: unit.generation,
            character: unit.character,
            human: unit.human,
            arc: None,
            recon,
        }
    }

    // Notifications can be emitted while the Unit is out of its slab. Keep
    // the invocation's gameplay life rather than reading an empty placeholder.
    fn key(&self) -> F3UnitKey {
        F3UnitKey {
            slot: self.r.slot,
            generation: self.r.gen,
            life: self.life,
        }
    }

    /// An import that runs the battle's code: the unit and his combat back
    /// in the store for it (JavaScript syncs from them first and gives his
    /// view back after), then out again as the battle left them.
    fn changing(&mut self, u: &mut Unit, c: &mut Combat, call: impl FnOnce(Option<crate::units::f3::WorldServices<'_>>)) {
        put(self.r, std::mem::take(u), std::mem::take(c));
        // Native fire, repair and recursive deaths need committed state, but
        // no mirror reader ran. Keep their touched rows pending until an
        // actual foreign leaf or phase return consumes the publication.
        crate::units::f3::changed(self.r);
        call(self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        let (nu, nc) = take(self.r);
        *u = nu;
        *c = nc;
    }

    fn shot(&self, shot: &Shot) -> CbShot {
        let mut s = CbShot {
            unit: self.unit,
            weapon: shot.weapon as f64,
            ..CbShot::default()
        };
        match shot.kind {
            ShotKind::Round {
                yaw,
                pitch,
                charge,
                target_id,
                target_generation,
                admission,
            } => {
                s.kind = CbShotKind::Round;
                s.yaw = yaw;
                s.pitch = pitch;
                s.charge = charge;
                s.target_id = target_id;
                s.target_generation = target_generation;
                s.admission = admission;
            }
            ShotKind::Repair => s.kind = CbShotKind::Repair,
            ShotKind::MeleeStart { swing } => {
                s.kind = CbShotKind::MeleeStart;
                s.swing = swing;
            }
            ShotKind::MeleeHit | ShotKind::MeleeEnd => unreachable!("a swing's hit or end is not a shot the battle is told of"),
        }
        s
    }
}

impl Drop for JsWorld<'_> {
    fn drop(&mut self) {
        // Return the invocation's handle buffer even when the shot produced
        // no links. The next tick must not allocate another acquisition list.
        if let Some(targets) = self.arc.take() {
            crate::units::f3::put_arc(self.r, targets);
        }
    }
}

impl WeaponDraws for JsWorld<'_> {
    fn draw(&mut self, weapon: u32) -> f64 {
        return crate::units::f3::random_draw(self.r, Some(weapon));
    }

    fn legacy(&mut self, _weapon: u32) -> f64 {
        crate::units::f3::fail();
        0.0
    }

    fn seeded(&mut self, _seed: f64, _value: f64) {}

    fn admit(&mut self, _weapon: u32) -> Admission {
        // CombatWorld.admitRemote: `remotes?.admitLaunch?.(weapon) ?? true`.
        if !self.remotes {
            return Admission::Yes;
        }

        return crate::units::f3::required_services!(self.recon.as_mut(), Admission::No)
            .recon
            .reserve(&sim().store)
            .map_or(Admission::No, |r| Admission::Token(crate::recon::Authority::token(r)));
    }
}

impl CombatWorld for JsWorld<'_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, static_only: bool) -> Option<CastHit> {
        return crate::units::native::cast_static(crate::units::f3::collision_world(), origin, direction, far, static_only).map(|h| CastHit {
            distance: h.distance,
            point: h.point,
            normal: h.normal,
        });
    }

    fn remote_of(&mut self, weapon: u32) -> Option<Droid> {
        // CombatWorld.remoteOf: `remotes?.of(weapon) || null`.
        if !self.remotes {
            return None;
        }

        let context = crate::recon::module::context_for(self.r, self.life)?;
        return crate::units::f3::required_services!(self.recon.as_ref(), None)
            .recon
            .of(&sim().store, context, weapon)
            .map(|r| Droid(r.slot));
    }

    fn remote_enter(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat) {
        self.changing(unit, combat, |authority| {
            let services = authority.unwrap();
            let authority = services.recon;
            let reference = sim().store.recon_droid.at(droid.0).expect("live remote droid").0;
            authority.enter(&mut sim().store, reference);
            crate::recon::module::publish_active(authority);
        });
        return;
    }

    fn remote_kill(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat) {
        self.changing(unit, combat, |authority| {
            let mut services = authority.unwrap();
            let authority = services.recon;
            let reference = sim().store.recon_droid.at(droid.0).expect("live remote droid").0;
            authority.kill(&mut sim().store, &mut crate::recon::module::NativeWorld::idle(services.queries,services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks)), reference);
            crate::recon::module::publish_active(authority);
        });
        return;
    }

    fn remote_launch(&mut self, weapon: u32, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, admission: Option<f64>, unit: &mut Unit, combat: &mut Combat) {
        // CombatWorld.remoteLaunch is a no-op for bots without a context,
        // even though their remote weapon still consumes its throw.
        let Some(context) = crate::recon::module::context_for(self.r, self.life) else { return };
        let definition = combat.weapons[weapon as usize].def;
        let (class, immediate) = crate::recon::module::weapon(definition);
        let (origin, velocity) = (*origin, *velocity);
        self.changing(unit, combat, |authority| {
            let mut services = authority.unwrap();
            let authority = services.recon;
            let token = admission
                .and_then(|t| authority.reservation(t))
                .or_else(|| if admission.is_none() { authority.reserve(&sim().store) } else { None });
            if let Some(token) = token {
                if authority
                    .launch(
                        &mut sim().store,
                        &mut crate::recon::module::NativeWorld::idle(services.queries,services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks)),
                        context,
                        token,
                        class,
                        weapon,
                        origin,
                        velocity,
                        yaw,
                        team,
                        immediate,
                    )
                    .is_err()
                {
                    authority.cancel(token);
                    crate::units::f3::fail();
                    return;
                }
                crate::recon::module::publish_active(authority);
            }
        });
        return;
    }

    fn lock_on(&mut self, eye: &Vector3, dir: &Vector3, _weapon: u32, tool: &Weapon, current: Option<RepairTarget>, held: bool, team: f64) -> Option<RepairTarget> {
        let defs = defs();
        let definition = &tool.level(defs.weapons.class(tool.def)).repair;
        return crate::units::f3_props::lock_on(
            *eye,
            *dir,
            definition,
            current,
            held,
            team,
            crate::units::f3::required_services!(self.recon.as_mut(), None).reborrow(),
        );
    }

    fn lock_key(&mut self, target: Option<RepairTarget>) -> StrId {
        target.map_or_else(crate::units::f3::empty_text, crate::units::f3_props::lock_key)
    }

    fn repair(&mut self, weapon: u32, target: Option<RepairTarget>, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool {
        let defs = defs();
        let tool = &combat.weapons[weapon as usize];
        let definition = &tool.level(defs.weapons.class(tool.def)).repair;
        let mut done = false;
        self.changing(unit, combat, |services| {
            done = crate::units::f3_props::repair(target, definition, team, crate::units::f3::required_services!(services, ()));
        });
        return done;
    }

    fn lock_pick(&mut self, _weapon: u32, launcher: &Weapon, origin: &Vector3, dir: &Vector3) -> LockPick {
        return crate::units::f3_sensors::lock_pick(self.r, self.team, launcher, origin, dir);
    }

    fn fire_round(&mut self, from: &Vector3, dir: &Vector3, shot: &Shot, _bind: bool, _selected: Option<(f64, f64)>, unit: &mut Unit, combat: &mut Combat) {
        let _allocations = crate::allocations::at(F3AllocationStage::Fire);
        let weapon = &combat.weapons[shot.weapon as usize];
        let service = crate::units::f3::weapon_service(weapon.def, weapon.level);
        let ShotKind::Round { target_id, target_generation, .. } = shot.kind else { return };
        let (from, direction, owner) = (a3(from), a3(dir), OrdnanceOwner::Unit(self.r));
        self.changing(unit, combat, |recon| {
            let (store, strings) = crate::ordnance::module::parts();
            let mut world = crate::ordnance::module::JsWorld::with_services(store, strings, recon);
            // Definitions are immutable during this phase. The shooter is
            // back in the store before F1 can emit instant-hit callbacks.
            let mut cx = unsafe { crate::ordnance::Cx::new(store, strings, crate::ordnance::module::defs(), &mut world) };
            unsafe {
                (*service).fire(&mut cx, from, direction, owner, target_id, target_generation);
            }
        });
        return;
    }

    fn dispense(&mut self, weapon: u32, most: f64, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool {
        let w = &combat.weapons[weapon as usize];
        let service = crate::units::f3::weapon_service(w.def, w.level);
        // Borrow buffers out of the sidecar before F1 can recursively
        // damage or publish items. No binding borrow crosses that work.
        let mut frame = crate::units::f3::items().take(self.r.slot, self.life, w.def, w.level as u32);
        let (origin, velocity, owner) = (a3(origin), a3(velocity), ItemOwner::Unit(self.r));
        let mut made = false;
        self.changing(unit, combat, |recon| {
            let (store, strings) = crate::ordnance::module::parts();
            let mut world = crate::ordnance::module::JsWorld::with_services(store, strings, recon);
            let mut cx = unsafe { crate::ordnance::Cx::new(store, strings, crate::ordnance::module::defs(), &mut world) };
            made = unsafe { (*service).dispense(&mut cx, frame.key, &mut frame.work, &mut frame.spare, most, origin, velocity, yaw, owner, team) };
        });
        crate::units::f3::items().restore(frame);
        return made;
    }

    fn melee_strike(&mut self, unit: &mut Unit, combat: &mut Combat) {
        // Commit the attacker before a defender's damage can recursively
        // explode. The candidate buffer belongs to this invocation's Unit.
        let (owner, origin, aim, source) = (self.r, combat.origin, combat.aim_dir, a3(&unit.position()));
        self.changing(unit, combat, |recon| crate::units::f3_sensors::melee(owner, origin, aim, source, recon));
        return;
    }

    fn arc_targets(&mut self) -> Vec<ArcTarget> {
        unreachable!("native arc acquisition requires retained scratch");
    }

    fn arc_targets_into(&mut self, out: &mut Vec<ArcTarget>) {
        let targets = self.arc.get_or_insert_with(|| crate::units::f3::take_arc(self.r));
        crate::units::f3_sensors::arc_targets(
            self.r,
            self.team,
            self.human,
            out,
            targets,
            crate::units::f3::required_services!(self.recon.as_ref(), ()).recon,
        );
    }

    fn hurt_arc_target(&mut self, index: usize, amount: f64, from: &Vector3, unit: &mut Unit, combat: &mut Combat) {
        let victim = self.arc.as_ref().expect("acquired arc targets")[index];
        let (owner, from) = (self.r, *from);
        // Commit the shooter before a link can kill a mine or start a
        // class blast that reaches back into this same combat.
        self.changing(unit, combat, |recon| super::live::arc_hit(victim, amount, from, owner, recon));
        return;
    }

    fn has_lightning(&mut self, _name: &str) -> bool {
        unreachable!("native lightning requires a prepared weapon service");
    }

    fn has_lightning_for(&mut self, definition: DefId, level: f64, _name: &str) -> bool {
        return unsafe { (*crate::units::f3::weapon_service(definition, level)).has_lightning };
    }

    fn headshot_effect(&mut self, point: &Vector3) {
        F3EffectNotice {
            key: self.key(),
            effect: F3Effect::Headshot,
            position: a3(point),
        }
        .emit(&mut sim().events);
    }

    fn headshot_credit(&mut self, previous: Attacker) {
        // Game.onHitUnit credits the previous attacker, before damage replaces
        // it. The victim itself may be out of the slab during that decision.
        let character = match previous {
            Attacker::Unit(_) | Attacker::Player => {
                let r = match previous {
                    Attacker::Unit(r) => r,
                    _ => crate::units::f3::viewpoint(),
                };
                if r == self.r {
                    self.character
                } else {
                    sim().store.unit.get(r).and_then(|u| u.character)
                }
            }
            Attacker::OwnerSnapshot(r) => sim().store.owner_snapshot.get(r).and_then(|o| o.character),
            // Hulls and turrets have no Character of their own. A null credit
            // does not create or change any BattleStats record.
            _ => None,
        };
        if crate::battle::f4_core::active() {
            crate::battle::f4_core::achievement(character, F4CommandOp::Headshot);
        } else if let Some(character) = character { import_f3_head_credit(&F3HeadCredit { character }); }
    }

    fn fired(&mut self, shot: &Shot, unit: &mut Unit, combat: &mut Combat) {
        // instrument.onFired is a pure notice. Capture its origin and life
        // now; later separation or reuse must not change the journal row.
        crate::wire::shots::native_observe(self.r.slot as f64, unit.generation, unit.generation);
        F3ShotNotice {
            key: F3UnitKey::of(self.r, unit),
            shot: self.shot(shot),
            position: super::live::origin(unit, combat),
            direction: combat.shot_dir,
            team: unit.team,
        }
        .emit(&mut sim().events);
        return;
    }

    fn damaged(&mut self, amount: f64, attacker: Attacker, info: &DamageInfo, _source: Option<Vector3>, unit: &mut Unit, _combat: &mut Combat) {
        let _allocations = crate::allocations::at(F3AllocationStage::Hit);
        // instrument.onDamaged keeps the authored amount/scale fields as
        // well as actual health damage. Preserve absent and null points.
        let (attacker_ref, attacker_team) = super::live::attribution(attacker, self.r, unit);
        let point_state = match info.point {
            None => F3OptionalState::Absent,
            Some(None) => F3OptionalState::Null,
            Some(Some(_)) => F3OptionalState::Value,
        };
        let point = info.point.flatten().map_or([0.0; 3], |p| a3(&p));
        F3DamageNotice {
            key: F3UnitKey::of(self.r, unit),
            amount,
            attacker,
            has_attacker_ref: attacker_ref.is_some(),
            attacker_ref: attacker_ref.unwrap_or_default(),
            attacker_team,
            position: a3(&unit.position()),
            team: unit.team,
            info: F3DamageInfo {
                explosion: info.explosion,
                amount: info.amount,
                typed: info.typed,
                generic: info.generic,
                known: info.known,
                point_state,
                point,
                source_present: _source.is_some(),
                source: _source.map_or([0.0; 3], |p| a3(&p)),
            },
        }
        .emit(&mut sim().events);
        return;
    }

    fn died(&mut self, unit: &mut Unit, combat: &mut Combat) {
        let _allocations = crate::allocations::at(F3AllocationStage::Death);
        // DroidekaAnimator.die marks the body on the damage tick, before
        // UnitFactory.onDeath. A blown-apart body skips that animator call.
        if !combat.blow_apart {
            if let UnitBody::Droideka(body) = &mut unit.soldier {
                body.dead = true;
            }
        }
        // UnitFactory.onDeath changes life flags before Game.onDeath.
        // Character bookkeeping must finish before the drop's RNG draw.
        unit.alive = false;
        unit.dead = true;
        unit.dead_time = 0.0;
        if unit.seat.is_some() {
            // Game.onDeath calls leaveKilled before Character credit and
            // item drops. A gunner falls from the last native rider pose.
            let reference = self.r;
            self.changing(unit, combat, |services| {
                crate::vehicles::module::f2::live_seats::leave_killed(reference, crate::units::f3::required_services!(services, ()));
            });
        }
        if let UnitBody::Soldier(body) = &mut unit.soldier {
            if body.air == Some(Air::Jet) {
                body.air = Some(Air::Fall);
                body.posture = Posture::Stand;
                body.state_time = 0.0;
            }
        }
        let key = F3UnitKey::of(self.r, unit);
        let attacker = combat.last_attacker;
        let (attacker_ref, attacker_team) = super::live::attribution(attacker, self.r, unit);
        let killer = super::live::character(attacker, self.r, unit);
        let credit = F3DeathCredit {
            key: F3UnitKey::of(self.r, unit),
            unit: self.r,
            killer,
            relation: if killer.is_some() { crate::units::f3::relation(attacker_team, unit.team) } else { 0.0 },
        };
        let mut controller = None;
        self.changing(unit, combat, |services| {
            if crate::battle::f4_core::owns_lifecycle() {
                controller=crate::battle::f4_core::died(credit.unit,credit.killer,credit.relation,crate::units::f3::required_services!(services,()));
            } else {
                crate::units::f3::foreign_barrier();
                controller=import_f3_death_credit(&credit).controller;
            }
        });
        unit.controller = controller;
        F3DeathNotice {
            key,
            attacker,
            has_attacker_ref: attacker_ref.is_some(),
            attacker_ref: attacker_ref.unwrap_or_default(),
            attacker_team,
            position: a3(&unit.position()),
            team: unit.team,
        }
        .emit(&mut sim().events);
        let (class, mut position, velocity, yaw) = (unit.class_def, a3(&unit.position()), a3(&unit.velocity()), unit.aim_yaw());
        position[1] += 1.048;
        // Items.dropFor draws once even when the authored list is empty.
        // The retained immutable list has no sidecar borrow across spawn.
        let drops = crate::units::f3::items().drops(class) as *const [crate::items::Drop];
        self.changing(unit, combat, |recon| {
            let (store, strings) = crate::ordnance::module::parts();
            let mut world = crate::ordnance::module::JsWorld::with_services(store, strings, recon);
            let mut cx = unsafe { crate::ordnance::Cx::new(store, strings, crate::ordnance::module::defs(), &mut world) };
            unsafe {
                let _allocations = crate::allocations::at(F3AllocationStage::Drop);
                cx.drop_for(&*drops, position, velocity, yaw);
            }
        });
        return;
    }

    fn blown_apart(&mut self, health: f64, unit: &mut Unit, combat: &mut Combat) {
        let reference = self.r;
        self.changing(unit, combat, |recon| super::live::blow_apart(reference, health, recon));
        return;
    }

    fn respawn(&mut self, unit: &mut Unit, combat: &mut Combat) {
        if crate::battle::f4_tail::owns_player(self.r) {
            self.changing(unit,combat,|services| crate::battle::f4_tail::auto_respawn(services.expect("native player body services")));
            return;
        }
        // PlayerCombat.onRespawn only notifies the host. Deliver it after
        // the owning phase, when its canonical health is already published.

        F3BodyNotice {
            key: self.key(),
            event: F3BodyEvent::Respawn,
            health: combat.health,
        }
        .emit(&mut sim().events);
        return;
    }

    fn presentation(&mut self, event: &Presentation) {
        let empty = crate::units::f3::empty_text();
        let mut notice = F3Presentation {
            key: self.key(),
            bank: empty,
            name: empty,
            ..Default::default()
        };
        match event {
            Presentation::WeaponAction {
                definition,
                action,
                posture,
                prepare,
                ..
            } => {
                notice.kind = F3PresentKind::WeaponAction;
                notice.action = Some(present_action(action));
                notice.bank = unsafe { (*crate::units::f3::weapon_service(*definition, 0.0)).bank };
                notice.posture = posture.map(|p| match p {
                    "prone" => Posture::Prone,
                    "crouch" => Posture::Crouch,
                    _ => Posture::Stand,
                });
                notice.prepare = *prepare;
            }
            Presentation::EndWeaponAction { action } => {
                notice.kind = F3PresentKind::EndWeaponAction;
                notice.action = action.map(present_action);
            }
            Presentation::Lightning {
                definition,
                level,
                from,
                to,
                charge,
                channel,
                ..
            } => {
                notice.kind = F3PresentKind::Lightning;
                notice.name = unsafe { (*crate::units::f3::weapon_service(*definition, *level)).lightning };
                notice.from = a3(from);
                notice.to = a3(to);
                notice.charge = *charge;
                notice.channel = *channel;
            }
        }
        notice.emit(&mut sim().events);
        return;
    }

    fn next_gun(&mut self, unit: &mut Unit, _combat: &mut Combat) -> Option<Vector3> {
        match crate::units::muzzles::next_gun(self.r.slot, unit) {
            Some(point) => point,
            None => { crate::units::f3::fail(); None }
        }
    }

    fn gun_at(&mut self, selected: (f64, f64)) -> Option<Vector3> {
        match crate::units::muzzles::gun_at(self.r.slot, selected) {
            Some(point) => point,
            None => { crate::units::f3::fail(); None }
        }
    }

    fn shield_hit(&mut self, taken: f64, left: f64, _source: Option<Vector3>, explosion: bool) {
        F3ShieldNotice {
            key: self.key(),
            taken,
            left,
            explosion,
            source_present: _source.is_some(),
            source: _source.map_or([0.0; 3], |p| a3(&p)),
        }
        .emit(&mut sim().events);
        return;
    }

    fn draw_combat(&mut self, generator: bool) -> f64 {
        if !generator { crate::units::f3::fail(); return 0.; }
        return crate::units::f3::random_draw(self.r, None);
    }

    fn text(&self, _id: StrId) -> String {
        unreachable!("native explosions require prepared health scales");
    }
}

// PlayerCombat uses this finite action vocabulary; an action is never a
// content lookup or a newly allocated String on the owned phase path.
fn present_action(name: &str) -> F3WeaponAction {
    match name {
        "reload" => F3WeaponAction::Reload,
        "shoot2" => F3WeaponAction::Shoot2,
        "shoot3" => F3WeaponAction::Shoot3,
        "shoot" => F3WeaponAction::Shoot,
        _ => unreachable!("weapon action"),
    }
}

/// Copy the generation-qualified network edge into retained native controls.
pub(crate) fn edge(e: &CbEdge) -> Edge {
    Edge {
        button: e.button,
        id: e.id,
        late: e.late,
        aim_yaw: e.aim_yaw,
        aim_pitch: e.aim_pitch,
    }
}

impl CombatOwner {
    fn lend_body(store: &mut Store, taken: &mut Option<u32>, r: Ref<Unit>) -> Unit {
        let unit = store.unit.lend(r).expect("a unit the store holds");
        *taken = Some(r.slot);
        unit
    }
}

impl CombatOwner {
    fn restore_body(store: &mut Store, taken: &mut Option<u32>, r: Ref<Unit>, unit: Unit) {
        store.unit.restore(r,unit).unwrap_or_else(|_| panic!("return the same owned unit"));
        *taken = None;
    }
}

impl Content {
    fn definitions(&self) -> Rc<CombatDefs> {
        self.defs.clone()
    }
    fn zoom(&self, c: &mut Combat) -> f64 {
        super::combat::zoom_factor(c, &self.defs)
    }
    fn blast_packet(&self, id: u32) -> super::impacts::BlastPacket {
        self
            .explosion_packets
            .get(id as usize)
            .copied()
            .unwrap_or_else(|| super::impacts::BlastPacket::read(&Js::Undefined))
    }
    fn weapon(&mut self, id: u32, value: Js) {
        let class = WeaponClass::new(obj_of(value));
        Rc::make_mut(&mut self.defs).weapons.set(DefId(id), class);
    }
    fn class(&mut self, id: u32, value: Js) {
        let class = obj_of(value);
        Rc::make_mut(&mut self.defs).set_class(DefId(id), class);
    }
    fn explosion(&mut self, id: u32, value: Js) {
        let v = value;
        let packet = super::impacts::BlastPacket::read(&v);
        let packets = &mut self.explosion_packets;
        if packets.len() <= id as usize {
            packets.resize_with(id as usize + 1, || super::impacts::BlastPacket::read(&Js::Undefined));
        }
        packets[id as usize] = packet;
    }
}
