//! The units' phase on the module, behind its switch (src/units-sim.js;
//! off unless a world asks for it: ?units=rust): the step's loop over the
//! battle's units (src/game.js: each one's tick, a soldier's water, a
//! droideka's deep water, the death regions) runs here, unit by unit in
//! the battle's order, on the store's units.
//!
//! The units stay JavaScript's between phases (P7's hull phase does the
//! same with its hulls): JavaScript writes every unit's rows into the store
//! before the phase, and the store's rows of every unit the phase changed
//! go back into the same objects after it. In between, the store holds
//! them, and the battle's JavaScript sees them as the phase left them
//! wherever it runs inside the phase:
//!
//! - an import that reads the battle (the Enhanced aimer, a launcher's lock,
//!   the arc caster's targets) first has the units the phase changed since
//!   the last such import synced into their objects (`units_sync`);
//! - an import that runs the battle's code (a hook: a death, a round fired,
//!   a droideka's body) does that, and has every unit written back into the
//!   store after (the code may have changed any of them).
//!
//! The unit being ticked is out of the store meanwhile (combat's module's
//! take and put); it goes back around each import of the phase's own, so
//! what JavaScript does to him there (a setter on his combat, a hook) lands
//! in the store, and the phase takes him out again after.
//!
//! With combat on the module too (?combat=rust) a unit's combat is the
//! store's, and the phase calls it directly: his update, the rounds a
//! droideka's guns owe, the hurts of a fall, of the water and of a death
//! region. Combat's own imports are src/combat-sim.js's, which runs the
//! hooks among them the phase's way while it runs. Otherwise his combat is
//! JavaScript's (src/combat.js PlayerCombat): those calls are imports, and
//! the phase reads of it what its own code reads (dead, the aim's pivot
//! and gun origin), written in with the units.
//!
//! What the phase asks of the map (the water, the death regions) is the
//! module's map rules, and a soldier moves through the module's own copy of
//! the collision world (rust/sim/src/vehicles/native.rs: the same instances
//! the hull phase uses, which src/units-sim.js sets up when it does not).

use super::soldier::tick_soldier;
use super::shapes::{Bounds, HitShape};
use super::water::{soldier_step_at, WaterEffects};
use super::world::{CastHit, GatherPolygon, UnitsWorld};
use crate::abi::sim;
use crate::combat::combat::{self as cb, Edges};
use crate::combat::module::{self as cm, JsWorld};
use crate::combat::world::DamageInfo;
use crate::gen::*;
use crate::store::block::Block;
use crate::store::{word_u32, Nullable, Ref};
use crate::vehicles::module as vm;
use std::ptr::addr_of_mut;
use three::Vector3;

/// What the phase knows between its calls.
struct Phase {
    /// Whether the battle's combats are the module's.
    combat: bool,
    /// The collision world the soldiers move in (the kernel's world).
    world: usize,
    /// The unit being ticked (with his water and death regions after).
    current: Option<u32>,
    /// The units the phase changed since JavaScript last synced them, the
    /// current one aside, each once.
    dirty: Vec<u32>,
    marked: Vec<bool>,
    /// What `units_sync` hands JavaScript.
    block: Block,
}

static mut PHASE: Option<Phase> = None;

fn phase() -> &'static mut Phase {
    // SAFETY: one thread; each caller holds it only for a call of its own
    // and never across an import.
    unsafe {
        (*addr_of_mut!(PHASE)).get_or_insert_with(|| Phase {
            combat: false,
            world: 0,
            current: None,
            dirty: Vec::new(),
            marked: Vec::new(),
            block: Block::default(),
        })
    }
}

/// A unit the phase changed, for the next sync.
fn mark(id: u32) {
    let p = phase();
    let i = id as usize;
    if p.marked.len() <= i {
        p.marked.resize(i + 1, false);
    }
    if !p.marked[i] {
        p.marked[i] = true;
        p.dirty.push(id);
    }
}

fn unit_ref(id: u32) -> Ref<Unit> {
    Ref::new(id, 0)
}

/// Unit `id` and his combat back in the store for an import (the store
/// holds an empty unit in his place while the phase has him), then out
/// again as JavaScript left them.
fn out_and_back(id: u32, unit: &mut Unit, call: impl FnOnce()) {
    let r = unit_ref(id);
    let mut u = std::mem::take(unit);
    let c = std::mem::take(&mut u.combat);
    cm::put(r, u, c);
    call();
    let (mut u, c) = cm::take(r);
    u.combat = c;
    *unit = u;
}

/// The phase's setup: whether combats are the module's, and the
/// collision world the soldiers move in (the kernel's).
#[no_mangle]
pub extern "C" fn units_setup(combat: u32, world: u32) {
    let p = phase();
    p.combat = combat != 0;
    p.world = world as usize;
}

/// The unit being ticked, or -1 between units (src/units-sim.js tells a
/// hook of his from one of a command its code started).
#[no_mangle]
pub extern "C" fn units_current() -> i32 {
    phase().current.map_or(-1, |id| id as i32)
}

/// The unit out of the store now (mid-combat), or -1.
#[no_mangle]
pub extern "C" fn units_taken() -> i32 {
    cm::taken().map_or(-1, |id| id as i32)
}

/// The words of the records src/units-sim.js writes and reads: 0
/// UnitsPhase, 1 UnitsCombatViews, 2 Combat (a synced unit's combat).
#[no_mangle]
pub extern "C" fn units_words(which: u32) -> u32 {
    (match which {
        0 => UnitsPhase::WORDS,
        1 => UnitsCombatViews::WORDS,
        2 => Combat::WORDS,
        _ => 0,
    }) as u32
}

/// What the phase reads of the units' combats while they are
/// JavaScript's (UnitsCombatViews in the call block): written in with the
/// units, before the phase and after each hook.
#[no_mangle]
pub extern "C" fn units_combat_views() {
    let s = sim();
    let (row, heap) = s.calls.args();
    let views = UnitsCombatViews::take(row, heap);
    for v in &views.list {
        if let Some(u) = s.store.unit.get_mut(unit_ref(word_u32(v.unit))) {
            u.combat.dead = v.dead;
            u.combat.pivot = v.pivot;
            u.combat.origin = v.origin;
        }
    }
}

/// Units the phase changed, for JavaScript to sync into its objects: a
/// block of rows, each the unit's export row, then with combat on the
/// module his combat's record, then whether his gun is under water (the
/// water step's, which JavaScript keeps on his combat). `which`: 0 every
/// unit the phase changed since the last such call (the one being ticked
/// among them, unless he is out of the store mid-combat: he is left for
/// the next); 1 the same but the one being ticked (whose objects the
/// battle's JavaScript is changing now: a droideka's body); 2 only the one
/// being ticked (the others stay marked).
#[no_mangle]
pub extern "C" fn units_sync(which: u32) -> *const f64 {
    let p = phase();
    let taken = cm::taken();
    let mut list = Vec::new();
    if which != 2 {
        list = std::mem::take(&mut p.dirty);
        for &id in &list {
            p.marked[id as usize] = false;
        }
    }
    if let Some(id) = p.current {
        if which != 1 && taken != Some(id) && !list.contains(&id) {
            list.push(id);
        }
    }
    let combat = p.combat;
    let stride = Unit::EXPORT_STRIDE + if combat { Combat::WORDS } else { 0 } + 1;
    let s = sim();
    let block = &mut p.block;
    block.begin(list.len(), stride);
    for (i, &id) in list.iter().enumerate() {
        let u = s.store.unit.get(unit_ref(id)).expect("a unit the store holds");
        let (row, heap) = block.row(i);
        row[0] = id as f64;
        row[1] = 0.0;
        u.put_export(row, heap);
        let mut at = Unit::EXPORT_STRIDE;
        if combat {
            u.combat.put(&mut row[at..], heap);
            at += Combat::WORDS;
        }
        row[at] = if u.combat.gun_underwater { 1.0 } else { 0.0 };
    }
    block.finish();
    block.ptr()
}

/// The phase (UnitsPhase in the call block): each unit in its order, as
/// the step's loop does (src/game.js): his tick, then for a soldier in the
/// water his wakes, gun and breath, for a droideka its deep water, and a
/// death region's kill.
#[no_mangle]
pub extern "C" fn units_phase() {
    let args = {
        let s = sim();
        let (row, heap) = s.calls.args();
        UnitsPhase::take(row, heap)
    };
    let (combat, world) = {
        let p = phase();
        (p.combat, p.world)
    };
    let dt = args.dt;
    for info in &args.units {
        let id = word_u32(info.unit);
        phase().current = Some(id);
        let (mut unit, c) = cm::take(unit_ref(id));
        unit.combat = c;
        let edges = Edges { shots: info.edges.as_ref().map(|list| list.iter().map(cm::edge).collect()), shot: info.edge.as_ref().map(cm::edge) };
        let mut w = ModuleWorld { combat, world, remote: info.remote, edges };
        unit.tick(id, dt, &mut w, None);
        after_tick(id, &mut unit, info, args.level, dt, &mut w);
        let c = std::mem::take(&mut unit.combat);
        cm::put(unit_ref(id), unit, c);
        phase().current = None;
        mark(id);
    }
}

fn held(u: &Unit) -> bool {
    matches!(u.held, Nullable::Value(_))
}

/// The rest of the step's loop for a unit after his tick (src/game.js):
/// a soldier in the water, a droideka in deep water, a death region.
fn after_tick(id: u32, unit: &mut Unit, info: &UnitsPhaseUnit, level: f64, dt: f64, w: &mut ModuleWorld) {
    let water = vm::map().is_some_and(|m| m.water.is_some());
    if water && unit.alive && unit.seat.is_none() && !held(unit) && !unit.walker {
        // unit.waterContact ??= {}: a contact whose clocks are all unset.
        let mut contact = unit.water_contact.take().unwrap_or_default();
        let feet = unit.position();
        let head = Vector3 { x: unit.combat.pivot[0], y: unit.combat.pivot[1], z: unit.combat.pivot[2] };
        let gun = Vector3 { x: unit.combat.origin[0], y: unit.combat.origin[1], z: unit.combat.origin[2] };
        let velocity = unit.velocity();
        let mut splash = Splash { at: None };
        let hurt = {
            let rules = vm::map().expect("the map's rules");
            let water = rules.water.as_ref().expect("the map's water");
            soldier_step_at(level, dt, &mut contact, &feet, &head, &gun, &velocity, &mut |x, z| water.over(x, z), &mut splash)
        };
        unit.combat.gun_underwater = contact.gun_under == Some(true);
        unit.water_contact = Some(contact);
        // (A splash crosses after the step, as the JavaScript's callback
        // only makes the effect: no state of the step reads it.)
        if let Some(at) = splash.at {
            out_and_back(id, unit, || import_unit_splash(&UnitSplash { at: [at.x, at.y, at.z] }));
        }
        if hurt != 0.0 && !hurt.is_nan() {
            w.damage(id, unit, hurt);
        }
    }
    if water && unit.alive && unit.seat.is_none() && !held(unit) && unit.walker {
        if let (Some(rule), UnitBody::Droideka(d)) = (&info.droideka_water, &mut unit.soldier) {
            // src/vehicle-water.js waterDamageStep(body, dt, water, rule,
            // body.position, droidekaWaterDepth(def, rolled)).
            let mut hurt = 0.0;
            if dt > 0.0 {
                let depth = if d.mode >= 11.0 { rule.ball } else { rule.upright };
                let rules = vm::map().expect("the map's rules");
                let over = rules.water.as_ref().is_some_and(|wt| wt.over(d.position[0], d.position[2]));
                if !(over && level >= d.position[1] + depth) {
                    d.water_clock = Some(rule.interval);
                } else {
                    let clock = d.water_clock.unwrap_or(rule.interval) - dt;
                    d.water_clock = Some(clock);
                    if clock < 0.0 {
                        hurt = rule.amount * 60.0 * dt;
                    }
                }
            }
            if hurt > 0.0 {
                w.damage(id, unit, hurt);
            }
        }
    }
    if unit.alive && !held(unit) {
        let p = unit.position();
        if vm::map().is_some_and(|m| m.in_death_region(&p)) {
            w.damage(id, unit, f64::INFINITY);
        }
    }
}

/// Where a fall into the water splashed, if it did (the ripples are a
/// page's: a world's water makes none).
struct Splash {
    at: Option<Vector3>,
}

impl WaterEffects for Splash {
    fn ripple(&mut self, _at: &Vector3, _life: f64, _radius: f64, _velocity: &Vector3) {}

    fn splash(&mut self, at: &Vector3) {
        self.at = Some(*at);
    }
}

/// The battle as the phase meets it for one unit.
struct ModuleWorld {
    combat: bool,
    world: usize,
    /// Whether a network player's recon droid steps with him.
    remote: bool,
    /// His trigger edges (a network player's).
    edges: Edges,
}

impl ModuleWorld {
    /// Runs `f` on his combat, the module's, taken out of the unit.
    fn on_combat(&mut self, id: u32, unit: &mut Unit, f: impl FnOnce(&mut Combat, &mut Unit, &cb::CombatDefs, &Edges, &mut JsWorld)) {
        let defs = cm::defs();
        let mut c = std::mem::take(&mut unit.combat);
        let mut w = JsWorld::new(unit_ref(id), c.has_remotes);
        f(&mut c, unit, &defs, &self.edges, &mut w);
        unit.combat = c;
    }

    /// What a JavaScript combat's call left of it that the phase reads.
    fn view(unit: &mut Unit, v: &UnitsCombatView) {
        unit.combat.dead = v.dead;
        unit.combat.pivot = v.pivot;
        unit.combat.origin = v.origin;
    }
}

impl UnitsWorld for ModuleWorld {
    fn cast(&mut self, _origin: &Vector3, _direction: &Vector3, _far: f64) -> Option<CastHit> {
        unreachable!("a droideka's body ticks in JavaScript (unitBodyTick)")
    }

    fn gather(&mut self, _id: u32, _center: &Vector3, _radius: f64, _cache: usize, _out: &mut Vec<GatherPolygon>) {
        unreachable!("a droideka's body ticks in JavaScript (unitBodyTick)")
    }

    fn seat_exposed(&mut self, _id: u32, _unit: &Unit) -> bool {
        unreachable!("the phase asks no unit's hit shapes")
    }

    fn rider_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [HitShape]) -> usize {
        unreachable!("the phase asks no unit's hit shapes")
    }

    fn held_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [HitShape]) -> usize {
        unreachable!("the phase asks no unit's hit shapes")
    }

    fn target_alive(&mut self, _id: u32) -> bool {
        unreachable!("the phase asks the registry nothing")
    }

    fn target_dead(&mut self, _id: u32) -> bool {
        unreachable!("the phase asks the registry nothing")
    }

    fn target_sphere(&mut self, _id: u32) -> Bounds {
        unreachable!("the phase asks the registry nothing")
    }

    fn move_soldier(&mut self, id: u32, unit: &mut Unit, h: f64) {
        match &mut unit.soldier {
            UnitBody::Soldier(s) => {
                // The body he falls through (a rider's mount), as the
                // kernel numbers it.
                let pass_through = match s.pass_through {
                    Some(body) => vm::collision_world().and_then(|c| c.kernel_slot(body.slot)).map_or(-1, |k| k as i64),
                    None => -1,
                };
                tick_soldier(s, &unit.input, h, self.world, pass_through);
            }
            // A droideka's body is the battle's JavaScript's for now (its
            // clips and gun rig): its tick may set off its death's blasts.
            UnitBody::Droideka(_) => out_and_back(id, unit, || import_unit_body_tick(&UnitStep { unit: id as f64, dt: h })),
        }
    }

    fn damage(&mut self, id: u32, unit: &mut Unit, amount: f64) {
        if self.combat {
            // combat.damage(amount): onShield the same, no source, nobody
            // credited, no info.
            let info = DamageInfo::default();
            self.on_combat(id, unit, |c, u, defs, _, w| cb::damage_with(c, u, defs, amount, amount, None, Attacker::Null, &info, w));
        } else {
            let mut view = None;
            out_and_back(id, unit, || view = Some(import_unit_damage(&UnitDamageArgs { unit: id as f64, amount })));
            if let Some(v) = view {
                Self::view(unit, &v);
            }
        }
    }

    fn aim(&mut self, id: u32, unit: &mut Unit, h: f64) {
        // The aimer only for a unit with a controller: without one the
        // AI's aim does nothing (src/ai/index.js aim).
        if unit.controller.is_none() {
            return;
        }
        let args = UnitAimArgs { unit: id as f64, dt: h, aim_yaw: unit.aim_yaw(), aim_pitch: unit.aim_pitch() };
        let mut answer = None;
        out_and_back(id, unit, || answer = Some(import_unit_aim(&args)));
        if let Some(a) = answer {
            unit.intent.aim = a.aim;
        }
    }

    fn has_remote_step(&mut self, _id: u32, _unit: &Unit) -> bool {
        self.remote
    }

    fn remote_step(&mut self, id: u32, unit: &mut Unit, h: f64, active: bool) {
        out_and_back(id, unit, || import_unit_remote_step(&UnitRemoteStep { unit: id as f64, dt: h, active }));
    }

    fn combat_reset(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!("no unit spawns in the phase")
    }

    fn combat_update(&mut self, id: u32, unit: &mut Unit, h: f64, stick: f64) {
        if self.combat {
            self.on_combat(id, unit, |c, u, defs, edges, w| cb::update(c, id, u, defs, h, stick, edges, w));
        } else {
            // A JavaScript combat's update, with its muzzles flushed (the
            // next call of the tick's): one import.
            let mut view = None;
            out_and_back(id, unit, || view = Some(import_unit_combat(&UnitCombatArgs { unit: id as f64, dt: h, stick })));
            if let Some(v) = view {
                Self::view(unit, &v);
            }
        }
    }

    fn flush_muzzles(&mut self, id: u32, unit: &mut Unit) {
        if self.combat {
            self.on_combat(id, unit, |c, u, defs, edges, w| cb::flush_muzzles(c, u, defs, edges, w));
        }
    }

    fn clear_charge(&mut self, _id: u32, _unit: &mut Unit) {
        // (In unitRemoved, as Unit.undraw calls it.)
    }

    fn death_blast(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!("a droideka's body ticks in JavaScript (unitBodyTick)")
    }

    fn blow_apart(&mut self, _id: u32, _unit: &mut Unit, _health: f64) {
        unreachable!("a droideka's body ticks in JavaScript (unitBodyTick)")
    }

    fn let_go_of_character(&mut self, _id: u32, _unit: &mut Unit) {
        // (In unitRemoved, where the battle's Character is.)
    }

    fn release(&mut self, id: u32, unit: &Unit) {
        // Unit.remove's rest, the battle's: his Character lets go of him,
        // his charge's glow goes out, the factory takes his body back.
        // (`unit` is the store's own here: remove() runs inside the tick,
        // which holds him; nothing of him changes in it.)
        let _ = unit;
        import_unit_removed(&UnitRef { unit: id as f64 });
    }

    fn sample_muzzles(&mut self, id: u32, unit: &mut Unit, h: f64) {
        out_and_back(id, unit, || import_unit_sample_muzzles(&UnitStep { unit: id as f64, dt: h }));
    }
}
