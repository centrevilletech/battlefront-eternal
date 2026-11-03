//! Combat on the module, behind its switch (src/combat-sim.js; off unless a
//! world asks for it): a world's combats are the store's, each unit's in
//! its `combat` field, and JavaScript reads them through a facade shaped
//! like src/combat.js PlayerCombat whose state is a mirror of the store's
//! record and whose methods are these exports (an update, the rounds a
//! droideka's guns owe, a hurt, a blast, a new life, a weapon chosen, a
//! field the battle sets). What combat asks of the rest of the battle comes
//! back as the imports of rust/sim/schema/combat.mjs, which src/combat-sim.js
//! serves with the objects src/combat-world.js hands the battle.
//!
//! Units are still JavaScript's: each command brings the unit's view in
//! (CbIn: his intent, controls, body, life and trigger edges, and the
//! combat's fields the battle's JavaScript keeps), and its answer takes out
//! what combat wrote into the unit (CbOut) with the combat's record, which
//! the facade's mirror is synced from. The store is reached through short
//! borrows of the instance's one `Sim`, never held across an import
//! (P6's and P7's modules): the unit a command works on is taken out of the
//! store while it runs, and before an import that runs the battle's code
//! (a hook, a round fired, a link of the arc) it goes back, JavaScript
//! syncs its facade and the unit from it, runs the code, which may come
//! back into any combat (his own too: a blast his death sets off), gives
//! the unit's view back (cmb_feed), and the command takes the unit out
//! again (P7's current and reload), so the code after the call sees what
//! the battle did, as it does in the JavaScript.

use super::combat::{self as cb, CombatDefs, Edge, Edges};
use super::defs::{DefTable, Js, JsObj, WeaponClass};
use super::weapons::{arm, Admission, Arsenal, Shot, ShotKind, WeaponDraws};
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
    explosions: Vec<Js>,
}

static mut CONTENT: Option<Content> = None;

fn content() -> &'static mut Content {
    // SAFETY: one thread; nothing holds a reference to it across a call
    // out of the module (commands clone the Rc they read).
    unsafe { (*addr_of_mut!(CONTENT)).get_or_insert_with(|| Content { defs: Rc::new(CombatDefs::default()), explosions: Vec::new() }) }
}

/// Each unit's trigger edges as its last command brought them.
static mut EDGES: Vec<Edges> = Vec::new();

fn edges_of(id: u32) -> Edges {
    // SAFETY: one thread, a copy out.
    let all = unsafe { &*addr_of_mut!(EDGES) };
    all.get(id as usize).cloned().unwrap_or_default()
}

fn set_edges(id: u32, edges: Edges) {
    // SAFETY: one thread, no reference kept.
    let all = unsafe { &mut *addr_of_mut!(EDGES) };
    if all.len() <= id as usize {
        all.resize(id as usize + 1, Edges::default());
    }
    all[id as usize] = edges;
}

fn unit_ref(id: u32) -> Ref<Unit> {
    Ref::new(id, 0)
}

// ---- Content: JavaScript values as words (src/combat-sim.js `valueWords`):
// 0 undefined, 1 null, 2 false, 3 true, 4 a number, 5 a string (its index),
// 6 an array (its length, then its items), 7 an object (its keys' count,
// then each key's index and value), 8 a weapon class by its index in the
// battle's weapon table, 9 anything else (an object combat never reads).

fn read_js(words: &[f64], at: &mut usize, strings: &Strings) -> Js {
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
            Js::Obj(Rc::new(JsObj { fields: Vec::new(), def: Some((DefTable::Weapon, DefId(id))) }))
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
pub extern "C" fn cmb_weapon(id: u32) {
    let class = WeaponClass::new(obj_of(call_value()));
    Rc::make_mut(&mut content().defs).weapons.set(DefId(id), class);
}

/// Unit class `id` (the battle's unitClass table): what combat reads of it
/// (AddHealth, base, its loadout), in the call block.
#[no_mangle]
pub extern "C" fn cmb_class(id: u32) {
    let class = obj_of(call_value());
    Rc::make_mut(&mut content().defs).set_class(DefId(id), class);
}

/// Explosion `id` (src/combat-sim.js's table), in the call block.
#[no_mangle]
pub extern "C" fn cmb_explosion(id: u32) {
    let v = call_value();
    let list = &mut content().explosions;
    if list.len() <= id as usize {
        list.resize(id as usize + 1, Js::Undefined);
    }
    list[id as usize] = v;
}

// ---- The unit's view, in and out.

/// A command's view of the unit and of the combat's JavaScript-kept fields
/// into the store's unit (and his trigger edges aside).
fn feed_unit(u: &mut Unit, i: &CbIn) {
    u.generation = i.generation;
    u.class_def = i.class_def;
    u.network_human = i.network_human;
    u.seat = if i.seated { Some(Ref::new(0, 0)) } else { None };
    u.remotes_active = i.remotes_active;
    u.intent = i.intent.clone();
    u.controls = i.controls.clone();
    u.soldier = i.soldier.clone();
    let c = &mut u.combat;
    c.aim_override = i.aim_override;
    c.mounted = i.mounted;
    c.gun_underwater = i.gun_underwater;
    c.team = i.team;
    c.player = i.player;
    c.defer_muzzles = i.defer_muzzles;
    c.random = i.random;
    c.has_remotes = i.has_remotes;
    if i.weapons.len() == c.weapons.len() {
        for (w, x) in c.weapons.iter_mut().zip(&i.weapons) {
            w.random = x.random;
            w.spread_scale = x.spread_scale;
            w.yaw_spread_override = x.yaw_spread_override;
        }
    }
}

fn edge(e: &CbEdge) -> Edge {
    Edge { button: e.button, id: e.id, late: e.late, aim_yaw: e.aim_yaw, aim_pitch: e.aim_pitch }
}

/// The view into the store's unit `id` (made the first time).
fn feed(i: &CbIn) -> Ref<Unit> {
    let id = i.unit as u32;
    let r = unit_ref(id);
    {
        let s = sim();
        let u = s.store.unit.ensure(r);
        feed_unit(u, i);
    }
    set_edges(id, Edges { shots: i.edges.as_ref().map(|list| list.iter().map(edge).collect()), shot: i.edge.as_ref().map(edge) });
    r
}

/// What combat wrote into the unit, for JavaScript to write into his
/// objects (src/combat-sim.js `written`).
fn out_of(u: &Unit) -> CbOut {
    let mut o = CbOut {
        pressed0: u.controls.pressed.first().copied().unwrap_or(false),
        pressed1: u.controls.pressed.get(1).copied().unwrap_or(false),
        reload: u.intent.reload,
        next_primary: u.intent.next_primary,
        previous_primary: u.intent.previous_primary,
        next_secondary: u.intent.next_secondary,
        zoom: u.intent.zoom,
        ..CbOut::default()
    };
    match &u.soldier {
        UnitBody::Soldier(s) => {
            o.aim_pitch = s.aim_pitch;
            o.velocity = s.velocity;
            o.air = s.air;
            o.state_time = s.state_time;
            o.grounded = s.grounded;
        }
        UnitBody::Droideka(d) => {
            o.aim_pitch = d.aim_pitch;
            if let Some(shield) = &d.shield {
                o.shield_on = shield.on;
                o.shield_value = shield.value;
            }
        }
    }
    o
}

/// A command's answer in the call block: what it wrote into the unit, then
/// the combat's record. `flag`: a command's own answer (a weapon chosen).
fn answer(r: Ref<Unit>, flag: bool) -> *const f64 {
    let s = sim();
    let u = s.store.unit.get(r).expect("a unit the store holds");
    let out = out_of(u);
    s.calls.result(1 + CbOut::WORDS + Combat::WORDS, |row, heap| {
        row[0] = if flag { 1.0 } else { 0.0 };
        out.put(&mut row[1..], heap);
        u.combat.put(&mut row[1 + CbOut::WORDS..], heap);
    });
    s.calls.block.ptr()
}

/// The unit out of the store for a command to work on (the store keeps an
/// empty one in its place until it goes back).
fn take(r: Ref<Unit>) -> (Unit, Combat) {
    let s = sim();
    let mut u = std::mem::take(s.store.unit.get_mut(r).expect("a unit the store holds"));
    let c = std::mem::take(&mut u.combat);
    (u, c)
}

fn put(r: Ref<Unit>, mut u: Unit, c: Combat) {
    u.combat = c;
    *sim().store.unit.get_mut(r).expect("a unit the store holds") = u;
}

/// Runs `f` on unit `r` and its combat taken out of the store, with the
/// battle as the imports give it, and puts them back.
fn run(r: Ref<Unit>, f: impl FnOnce(&mut Combat, &mut Unit, &CombatDefs, &Edges, &mut JsWorld)) {
    let defs = content().defs.clone();
    let edges = edges_of(r.slot);
    let (mut u, mut c) = take(r);
    let mut w = JsWorld { unit: r.slot as f64, r };
    f(&mut c, &mut u, &defs, &edges, &mut w);
    put(r, u, c);
}

/// The command's arguments, of the record `T`, out of the call block.
fn args<T>(take: impl FnOnce(&[f64], &[f64]) -> T) -> T {
    let s = sim();
    let (row, heap) = s.calls.args();
    take(row, heap)
}

// ---- The commands (src/combat-sim.js CombatFacade).

/// A combat made for unit `unit` of class `class` (PlayerCombat's
/// constructor): full health, its class's loadout armed (a player's
/// weapons keep a player's spread floor), and the numbers JavaScript took
/// from its class (MaxHealth || 300, the chunk roll, the health scale's
/// name). The answer as every command's.
#[no_mangle]
pub extern "C" fn cmb_new(unit: u32, class: u32, player: u32, max_health: f64, chunks: f64, scale: u32) -> *const f64 {
    let defs = content().defs.clone();
    let r = unit_ref(unit);
    let weapons = arm(&defs.loadout(DefId(class)), &defs.weapons, player != 0);
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
        kick: Kick { rate: 0.0, hold: 0.0, offset: 0.0 },
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
        seen: vec![Seen { known: false, reloading: false, throwing: false }; n],
        ..Combat::default()
    };
    {
        let s = sim();
        let u = s.store.unit.ensure(r);
        u.class_def = DefId(class);
        u.combat = combat;
    }
    answer(r, false)
}

/// PlayerCombat.update(dt, controls, stick), the unit's view (CbIn) in the
/// call block.
#[no_mangle]
pub extern "C" fn cmb_update(dt: f64, stick: f64) -> *const f64 {
    let i = args(CbIn::take);
    let r = feed(&i);
    run(r, |c, u, defs, edges, w| cb::update(c, r.slot, u, defs, dt, stick, edges, w));
    answer(r, false)
}

/// PlayerCombat.flushMuzzles(), the unit's view in the call block.
#[no_mangle]
pub extern "C" fn cmb_flush() -> *const f64 {
    let i = args(CbIn::take);
    let r = feed(&i);
    run(r, |c, u, defs, edges, w| cb::flush_muzzles(c, u, defs, edges, w));
    answer(r, false)
}

/// PlayerCombat.damage(amount, onShield, source, attacker, info), its
/// numbers (CbDamageIn: info.kind === "explosion" as `explosion`) in the
/// call block. The info itself stays JavaScript's (its hook is given it).
#[no_mangle]
pub extern "C" fn cmb_damage() -> *const f64 {
    let a = args(CbDamageIn::take);
    let r = feed(&a.r#in);
    let info = DamageInfo { explosion: a.explosion, ..DamageInfo::default() };
    let source = a.source.map(|p| v(&p));
    run(r, |c, u, defs, _, w| cb::damage_with(c, u, defs, a.amount, a.on_shield, source, a.attacker, &info, w));
    answer(r, false)
}

/// PlayerCombat.explode(explosion, position, owner, scale), CbExplodeIn in
/// the call block.
#[no_mangle]
pub extern "C" fn cmb_explode() -> *const f64 {
    let a = args(CbExplodeIn::take);
    let r = feed(&a.r#in);
    let explosion = content().explosions.get(a.explosion as usize).cloned().unwrap_or_default();
    let position = v(&a.position);
    run(r, |c, u, defs, _, w| cb::explode(c, u, defs, &explosion, &position, a.owner, a.scale, w));
    answer(r, false)
}

/// PlayerCombat.reset(): full health and a fresh loadout (the unit's view,
/// for his class and whether he plays by a player's rules).
#[no_mangle]
pub extern "C" fn cmb_reset() -> *const f64 {
    let i = args(CbIn::take);
    let r = feed(&i);
    run(r, |c, u, defs, _, _| cb::reset(c, u, defs));
    answer(r, false)
}

/// The unit's view back from JavaScript after an import that ran the
/// battle's code (its hook may have changed his body or intent).
#[no_mangle]
pub extern "C" fn cmb_feed() {
    let i = args(CbIn::take);
    feed(&i);
}

/// Unit `unit` as the store holds it now (put back for an import), for
/// JavaScript to sync its facade and his objects from.
#[no_mangle]
pub extern "C" fn cmb_current(unit: u32) -> *const f64 {
    answer(unit_ref(unit), false)
}

/// Arsenal.select(weapon) (the AI's weapon choice): refused while the
/// channel's weapon is busy. The answer's flag is select's.
#[no_mangle]
pub extern "C" fn cmb_select(unit: u32, weapon: u32) -> *const f64 {
    let r = unit_ref(unit);
    let defs = content().defs.clone();
    let chosen = {
        let s = sim();
        let c = &mut s.store.unit.get_mut(r).expect("a unit the store holds").combat;
        select(c, &defs, weapon as usize)
    };
    answer(r, chosen)
}

/// weapons.js Arsenal.select (:517).
fn select(c: &mut Combat, defs: &CombatDefs, i: usize) -> bool {
    if i >= c.weapons.len() {
        return false;
    }
    let channel = c.weapons[i].channel(&defs.weapons);
    let arsenal = Arsenal { weapons: &mut c.weapons, selected: &mut c.selected };
    let list = arsenal.channel(&defs.weapons, channel as usize);
    let Some(at) = list.iter().position(|&x| x == i) else { return false };
    let old = arsenal.current(&defs.weapons, channel as usize);
    if old == Some(i) {
        return true;
    }
    if let Some(o) = old {
        if c.weapons[o].busy() {
            return false;
        }
        c.weapons[o].deselect();
    }
    c.selected[channel as usize] = at as f64;
    true
}

/// Arsenal.next(channel, direction). The answer's flag is next's.
#[no_mangle]
pub extern "C" fn cmb_next(unit: u32, channel: u32, direction: f64) -> *const f64 {
    let r = unit_ref(unit);
    let defs = content().defs.clone();
    let moved = {
        let s = sim();
        let c = &mut s.store.unit.get_mut(r).expect("a unit the store holds").combat;
        Arsenal { weapons: &mut c.weapons, selected: &mut c.selected }.next(&defs.weapons, channel as usize, direction)
    };
    answer(r, moved)
}

/// A field the battle sets on a combat (src/combat-sim.js CombatFacade's
/// setters): 0 health, 1 zoomStep, 2 dead, 3 deathTime, 4 blowApart.
#[no_mangle]
pub extern "C" fn cmb_set(unit: u32, field: u32, value: f64) {
    let s = sim();
    let c = &mut s.store.unit.get_mut(unit_ref(unit)).expect("a unit the store holds").combat;
    match field {
        0 => c.health = value,
        1 => c.zoom_step = value,
        2 => c.dead = value != 0.0,
        3 => c.death_time = value,
        4 => c.blow_apart = value != 0.0,
        _ => panic!("no combat field {field} to set"),
    }
}

/// A field the battle sets on a weapon (WeaponFacade's setters): 0
/// reserve, 1 kick, 2 clip, 3 heat.
#[no_mangle]
pub extern "C" fn cmb_weapon_set(unit: u32, weapon: u32, field: u32, value: f64) {
    let s = sim();
    let c = &mut s.store.unit.get_mut(unit_ref(unit)).expect("a unit the store holds").combat;
    let w = &mut c.weapons[weapon as usize];
    match field {
        0 => w.reserve = value,
        1 => w.kick = value,
        2 => w.clip = value,
        3 => w.heat = value,
        _ => panic!("no weapon field {field} to set"),
    }
}

/// The words of the records src/combat-sim.js reads and writes: 0 CbIn,
/// 1 CbOut, 2 Combat, 3 CbDamageIn, 4 CbExplodeIn.
#[no_mangle]
pub extern "C" fn cmb_words(which: u32) -> u32 {
    (match which {
        0 => CbIn::WORDS,
        1 => CbOut::WORDS,
        2 => Combat::WORDS,
        3 => CbDamageIn::WORDS,
        4 => CbExplodeIn::WORDS,
        _ => 0,
    }) as u32
}

// ---- The battle, as the imports give it.

fn v(a: &Vec3) -> Vector3 {
    Vector3 { x: a[0], y: a[1], z: a[2] }
}

fn a3(v: &Vector3) -> Vec3 {
    [v.x, v.y, v.z]
}

/// The battle as the module's imports give it, for unit `unit`'s combat.
struct JsWorld {
    unit: f64,
    r: Ref<Unit>,
}

impl JsWorld {
    /// An import that runs the battle's code: the unit and his combat back
    /// in the store for it (JavaScript syncs from them first and gives his
    /// view back after), then out again as the battle left them.
    fn changing(&mut self, u: &mut Unit, c: &mut Combat, call: impl FnOnce()) {
        put(self.r, std::mem::take(u), std::mem::take(c));
        call();
        let (nu, nc) = take(self.r);
        *u = nu;
        *c = nc;
    }

    fn shot(&self, shot: &Shot) -> CbShot {
        let mut s = CbShot { unit: self.unit, weapon: shot.weapon as f64, ..CbShot::default() };
        match shot.kind {
            ShotKind::Round { yaw, pitch, charge, target_id, target_generation, admission } => {
                s.kind = 0.0;
                s.yaw = yaw;
                s.pitch = pitch;
                s.charge = charge;
                s.target_id = target_id;
                s.target_generation = target_generation;
                s.admission = admission;
            }
            ShotKind::Repair => s.kind = 1.0,
            ShotKind::MeleeStart { swing } => {
                s.kind = 2.0;
                s.swing = swing;
            }
            ShotKind::MeleeHit | ShotKind::MeleeEnd => unreachable!("a swing's hit or end is not a shot the battle is told of"),
        }
        s
    }
}

impl WeaponDraws for JsWorld {
    fn draw(&mut self, weapon: u32) -> f64 {
        import_combat_draw(&CbDraw { unit: self.unit, weapon: weapon as f64, legacy: false }).value
    }

    fn legacy(&mut self, weapon: u32) -> f64 {
        import_combat_draw(&CbDraw { unit: self.unit, weapon: weapon as f64, legacy: true }).value
    }

    fn seeded(&mut self, _seed: f64, _value: f64) {}

    fn admit(&mut self, weapon: u32) -> Admission {
        let a = import_combat_admit(&CbWeapon { unit: self.unit, weapon: weapon as f64 });
        match word_u32(a.kind) {
            0 => Admission::Yes,
            1 => Admission::No,
            _ => Admission::Token(a.token),
        }
    }
}

impl CombatWorld for JsWorld {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, static_only: bool) -> Option<CastHit> {
        let h = import_combat_cast(&CbRay { unit: self.unit, origin: a3(origin), direction: a3(direction), far, static_only });
        h.found.then(|| CastHit { distance: h.distance, point: v(&h.point), normal: v(&h.normal) })
    }

    fn remote_of(&mut self, weapon: u32) -> Option<Droid> {
        let d = import_combat_remote_of(&CbWeapon { unit: self.unit, weapon: weapon as f64 });
        d.found.then_some(Droid(d.droid as u32))
    }

    fn remote_enter(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat) {
        let a = CbDroidCall { unit: self.unit, droid: droid.0 as f64 };
        self.changing(unit, combat, || import_combat_remote_enter(&a));
    }

    fn remote_kill(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat) {
        let a = CbDroidCall { unit: self.unit, droid: droid.0 as f64 };
        self.changing(unit, combat, || import_combat_remote_kill(&a));
    }

    fn remote_launch(&mut self, weapon: u32, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, admission: Option<f64>, unit: &mut Unit, combat: &mut Combat) {
        let a = CbLaunch { unit: self.unit, weapon: weapon as f64, origin: a3(origin), velocity: a3(velocity), yaw, team, admission };
        self.changing(unit, combat, || import_combat_remote_launch(&a));
    }

    fn lock_on(&mut self, eye: &Vector3, dir: &Vector3, weapon: u32, tool: &Weapon, current: Option<RepairTarget>, held: bool, team: f64) -> Option<RepairTarget> {
        import_combat_lock_on(&CbLockOn { unit: self.unit, eye: a3(eye), dir: a3(dir), weapon: weapon as f64, level: tool.level, current, held, team }).target
    }

    fn lock_key(&mut self, target: Option<RepairTarget>) -> StrId {
        // repairLockKey(live, null) is "" without asking.
        match target {
            None => sim().strings.intern_str(""),
            Some(_) => import_combat_lock_key(&CbLockKey { unit: self.unit, target }).value,
        }
    }

    fn repair(&mut self, weapon: u32, target: Option<RepairTarget>, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool {
        let a = CbRepair { unit: self.unit, weapon: weapon as f64, target, team };
        let mut done = false;
        self.changing(unit, combat, || done = import_combat_repair(&a).value);
        done
    }

    fn lock_pick(&mut self, weapon: u32, launcher: &Weapon, origin: &Vector3, dir: &Vector3) -> LockPick {
        let p = import_combat_lock_pick(&CbLockPick {
            unit: self.unit,
            weapon: weapon as f64,
            level: launcher.level,
            lock_complete: launcher.lock_complete,
            lock_target_id: launcher.lock_target_id,
            lock_generation: launcher.lock_generation,
            origin: a3(origin),
            dir: a3(dir),
        });
        match word_u32(p.pick) {
            0 => LockPick::Nothing,
            1 => LockPick::None,
            _ => LockPick::Unit { id: p.id, generation: p.generation, alive: p.alive },
        }
    }

    fn fire_round(&mut self, from: &Vector3, dir: &Vector3, shot: &Shot, bind: bool, selected: Option<(f64, f64)>, unit: &mut Unit, combat: &mut Combat) {
        let (index, barrel) = selected.unwrap_or((-1.0, -1.0));
        let a = CbFire { shot: self.shot(shot), from: a3(from), from_origin: selected.is_none(), dir: a3(dir), bind, selected: selected.is_some(), selected_index: index, selected_barrel: barrel };
        self.changing(unit, combat, || import_combat_fire_round(&a));
    }

    fn dispense(&mut self, weapon: u32, most: f64, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool {
        let a = CbDispense { unit: self.unit, weapon: weapon as f64, most, origin: a3(origin), velocity: a3(velocity), yaw, team };
        let mut made = false;
        self.changing(unit, combat, || made = import_combat_dispense(&a).value);
        made
    }

    fn melee_strike(&mut self, unit: &mut Unit, combat: &mut Combat) {
        let a = CbUnit { unit: self.unit };
        self.changing(unit, combat, || import_combat_melee_strike(&a));
    }

    fn arc_targets(&mut self) -> Vec<ArcTarget> {
        import_combat_arc_targets(&CbUnit { unit: self.unit })
            .targets
            .iter()
            .map(|t| ArcTarget { origin: v(&t.origin), center: t.center.as_ref().map(v), radius: t.radius })
            .collect()
    }

    fn hurt_arc_target(&mut self, index: usize, amount: f64, from: &Vector3, unit: &mut Unit, combat: &mut Combat) {
        let a = CbHurtArc { unit: self.unit, index: index as f64, amount, from: a3(from) };
        self.changing(unit, combat, || import_combat_hurt_arc(&a));
    }

    fn has_lightning(&mut self, name: &str) -> bool {
        let value = sim().strings.intern_str(name);
        import_combat_lightning(&CbText { value }).value
    }

    fn fired(&mut self, shot: &Shot, unit: &mut Unit, combat: &mut Combat) {
        let a = self.shot(shot);
        self.changing(unit, combat, || import_combat_fired(&a));
    }

    fn damaged(&mut self, amount: f64, attacker: Attacker, info: &DamageInfo, _source: Option<Vector3>, unit: &mut Unit, combat: &mut Combat) {
        let a = CbDamaged {
            unit: self.unit,
            amount,
            attacker,
            explosion: info.explosion,
            info_amount: info.amount.unwrap_or(f64::NAN),
            typed: info.typed.unwrap_or(f64::NAN),
            generic: info.generic.unwrap_or(f64::NAN),
        };
        self.changing(unit, combat, || import_combat_damaged(&a));
    }

    fn died(&mut self, unit: &mut Unit, combat: &mut Combat) {
        let a = CbUnit { unit: self.unit };
        self.changing(unit, combat, || import_combat_died(&a));
    }

    fn blown_apart(&mut self, health: f64, unit: &mut Unit, combat: &mut Combat) {
        let a = CbHealth { unit: self.unit, health };
        self.changing(unit, combat, || import_combat_blown_apart(&a));
    }

    fn respawn(&mut self, unit: &mut Unit, combat: &mut Combat) {
        let a = CbUnit { unit: self.unit };
        self.changing(unit, combat, || import_combat_respawn(&a));
    }

    fn presentation(&mut self, event: &Presentation) {
        let s = sim();
        let mut a = CbPresent { unit: self.unit, name: s.strings.intern_str(""), bank: s.strings.intern_str(""), ..CbPresent::default() };
        match event {
            Presentation::WeaponAction { action, bank, posture, prepare } => {
                a.kind = 0.0;
                a.action = Some(s.strings.intern_str(action));
                a.bank = s.strings.intern_str(bank);
                a.posture = posture.map(|p| s.strings.intern_str(p));
                a.prepare = *prepare;
            }
            Presentation::EndWeaponAction { action } => {
                a.kind = 1.0;
                a.action = action.map(|x| s.strings.intern_str(x));
            }
            Presentation::Lightning { name, from, to, charge, channel } => {
                a.kind = 2.0;
                a.name = s.strings.intern_str(name);
                a.from = a3(from);
                a.to = a3(to);
                a.charge = *charge;
                a.channel = *channel;
            }
        }
        import_combat_present(&a);
    }

    fn next_gun(&mut self, unit: &mut Unit, _combat: &mut Combat) -> Option<Vector3> {
        let g = import_combat_next_gun(&CbUnit { unit: self.unit });
        // The body moved on to its next gun (JavaScript's droideka did):
        // the same here.
        if let UnitBody::Droideka(d) = &mut unit.soldier {
            d.gun = g.gun;
            if g.has_last {
                d.last_gun = Some(LastGun { index: g.last_index, barrel: g.last_barrel });
            }
        }
        g.found.then(|| v(&g.point))
    }

    fn gun_at(&mut self, selected: (f64, f64)) -> Option<Vector3> {
        let p = import_combat_gun_at(&CbGunAt { unit: self.unit, index: selected.0, barrel: selected.1 });
        p.found.then(|| v(&p.point))
    }

    fn shield_hit(&mut self, taken: f64, left: f64, _source: Option<Vector3>, explosion: bool) {
        import_combat_shield_hit(&CbShield { unit: self.unit, taken, left, explosion });
    }

    fn draw_combat(&mut self, generator: bool) -> f64 {
        import_combat_draw(&CbDraw { unit: self.unit, weapon: -1.0, legacy: !generator }).value
    }

    fn text(&self, id: StrId) -> String {
        String::from_utf16_lossy(sim().strings.units(id))
    }
}
