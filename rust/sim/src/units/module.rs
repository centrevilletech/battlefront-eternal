//! Unit.tick and the following water and death-region checks over the
//! battle's canonical rows. Bodies, collision caches and combat scratch
//! survive ticks; only the aimer and Character bookkeeping cross back to
//! JavaScript. Presentation notices drain after the owning phase returns.

use super::droideka;
use super::muzzles;
use super::native::{self, DroidekaBody, DroidekaClass};
use super::soldier::tick_soldier;
use super::shapes::{Bounds, HitShape};
use super::water::{soldier_step_at, WaterEffects};
use super::world::{CastHit, UnitsWorld};
use crate::abi::sim;
use crate::combat::combat::{self as cb, Edges};
use crate::combat::module::{self as cm, JsWorld};
use crate::combat::world::DamageInfo;
use crate::gen::*;
use crate::store::{word_u32, Nullable, Ref};
use crate::vehicles::module as vm;
use std::ptr::addr_of_mut;
use three::Vector3;

/// What the phase knows between its calls.
pub(super) struct Phase {
    world: usize,
    scratch: cb::Scratch,
    droideka_classes: Vec<DroidekaClass>,
    droidekas: Vec<Option<DroidekaBody>>,
}


fn phase() -> &'static mut Phase {
    // SAFETY: one thread; each caller holds it only for a call of its own
    // and never across an import.
    unsafe {
        (*addr_of_mut!(super::owner::UNITS.phase)).get_or_insert_with(|| Phase {
            world: 0,
            scratch: cb::Scratch::default(),
            droideka_classes: Vec::new(),
            droidekas: Vec::new(),
        })
    }
}

// Unit's public id is also its canonical slab slot in the owning battle.
// Look up the current slot generation rather than trusting a JS body mirror.
// Commands validate gameplay life separately before reaching this helper.
// A missing row carries an invalid generation and cannot alias a replacement.
fn unit_ref(id: u32) -> Ref<Unit> {
    sim().store.unit.reference(id).unwrap_or(Ref::new(id, 0))
}

/// Unit `id` and his combat back in the store for an import (the store
/// retains his identity while the phase holds his row), then out
/// again as JavaScript left them.
// Unit.tick's foreign aimer needs the movement result already committed.
// Temporarily restore the row and combat before entering that observation.
// A nested query then sees the canonical state, not an empty taken-out frame.
// Recover the same row afterward so the resumed tick sees accepted commands.
fn out_and_back(id: u32, unit: &mut Unit, changed: bool, call: impl FnOnce()) {
    let r = unit_ref(id);
    cm::put_body(r, std::mem::take(unit));
    if changed { super::f3::changed(r); }
    call();
    *unit = cm::take_body(r);
}

// Unit.tick advances one combat at a time. Damage can nest while that
// combat owns its shot buffers, but cannot start another combat update.
// Lifecycle operations use empty scratch and never borrow these buffers.
fn take_scratch() -> cb::Scratch { std::mem::take(&mut phase().scratch) }
// UnitFactory construction reserves the shared combat work bound.
// The largest ordinary salvo uses this retained shot and muzzle capacity.
// Adding another body does not allocate another copy of the arc buffers.
// Only one combat update borrows this scratch at a time.
pub(crate) fn reserve_unit(_id:u32) { phase().scratch.reserve(64); }

/// Bind the prepared collision world before any body is adopted.
pub(crate) fn setup(world: u32) {
    phase().world = world as usize;
}

/// A droideka class's movement numbers and clip ids, written once in the
/// call block (native.rs). Returns its number for units_droideka_attach.
#[no_mangle]
pub extern "C" fn units_droideka_class() -> u32 {
    let (row, _) = sim().calls.args();
    let class = DroidekaClass::read(row);
    let classes = &mut phase().droideka_classes;
    classes.push(class);
    (classes.len() - 1) as u32
}

/// The class and persistent collision caches for one pooled body.
#[no_mangle]
// Droideka construction binds its immutable clip and movement class.
// The body sidecar owns reusable collision gathers and hidden gun state.
// It follows the pooled body across lives rather than being rebuilt on spawn.
// Class replacement moves this resource with its resident construction cookie.
pub extern "C" fn units_droideka_attach(unit: u32, class: u32) {
    let p = phase();
    let body = DroidekaBody::new(&p.droideka_classes[class as usize], p.world);
    if p.droidekas.len() <= unit as usize {
        p.droidekas.resize_with(unit as usize + 1, || None);
    }
    p.droidekas[unit as usize] = Some(body);
}

/// A prepared hidden gun hierarchy, before native posing takes ownership.
#[no_mangle]
// Droideka.arm has prepared a hidden muzzle hierarchy before this call.
// The hierarchy is independent of the page's interpolated drawn bones.
// Gameplay later samples this native rig after movement and before flush.
// No per-barrel JavaScript query runs inside the combat update.
pub extern "C" fn units_muzzle_attach(unit: u32, class: u32) {
    let parts = phase().droideka_classes[class as usize].parts();
    let (row, _) = sim().calls.args();
    muzzles::attach(unit, parts, row);
}

/// Unit.tick and the world's immediately following water/death-region tests.
/// The owned phase supplies retained trigger edges instead of parsing a list.
pub(crate) fn tick_owned(info: &UnitsPhaseUnit, level: f64, dt: f64, edges: Edges, recon: Option<crate::units::f3::WorldServices<'_>>) {
    let id = word_u32(info.unit);
    let world = phase().world;
    // A completed body's JS tick stamp can lag until the outer publication.
    // Invalidate its bounds mirror before any native movement or aimer runs.
    super::f3::invalidate_bounds(id);
    let mut unit = cm::take_body(unit_ref(id));
    let droideka = phase().droidekas.get_mut(id as usize).and_then(Option::take);
    let mut w = ModuleWorld {
        world,
        edges,
        droideka,
        scratch: take_scratch(),
        recon,
    };
    unit.tick(id, dt, &mut w, None);
    after_tick(id, &mut unit, info, level, dt, &mut w);
    if let Some(body) = w.droideka.take() { phase().droidekas[id as usize] = Some(body); }
    phase().scratch = std::mem::take(&mut w.scratch);
    cm::put_body(unit_ref(id), unit);
}

/// Unit.spawn and remove over the same body services as Unit.tick.
// Unit.spawn and remove borrow the same body services as an ordinary tick.
// They cannot borrow the active combat scratch during a nested death.
// Reset and removal need no shot buffers, so use an empty scratch frame.
// The native body sidecar is restored before returning to the owning boundary.
pub(crate) fn lifecycle_owned(id: u32, spawn: Option<([f64; 3], f64)>, recon: Option<crate::units::f3::WorldServices<'_>>) {
    super::f3::invalidate_bounds(id);
    let p = phase();
    let world = p.world;
    let droideka = p.droidekas.get_mut(id as usize).and_then(Option::take);
    let mut unit = cm::take_body(unit_ref(id));
    let mut w = ModuleWorld {
        world,
        edges: Edges::default(),
        droideka,
        scratch: cb::Scratch::default(),
        recon,
    };
    if let Some(([x, y, z], yaw)) = spawn { unit.spawn(id, x, y, z, yaw, &mut w); }
    else { unit.remove(id, &mut w); }
    if let Some(body) = w.droideka.take() { phase().droidekas[id as usize] = Some(body); }
    cm::put_body(unit_ref(id), unit);
}

fn held(u: &Unit) -> bool {
    matches!(u.held, Nullable::Value(_))
}

/// The rest of the step's loop for a unit after his tick (src/game.js):
/// a soldier in the water, a droideka in deep water, a death region.
// Game.step applies water and death regions immediately after Unit.tick.
// Use that tick's body position, including a just-completed droideka movement.
// Hazards can kill immediately and therefore share the combat damage services.
// Their effect notices keep the same observation order as ordinary impacts.
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
            let splash = UnitSplash { at: [at.x, at.y, at.z] };
            // Game.presentEffect cannot affect water damage. Drain this
            // notice after the ownership interval, alongside other effects.
            splash.emit(&mut sim().events);
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
pub(crate) struct ModuleWorld<'a> {
    world: usize,
    /// His trigger edges (a network player's).
    edges: Edges,
    droideka: Option<DroidekaBody>,
    scratch: cb::Scratch,
    recon: Option<crate::units::f3::WorldServices<'a>>,
}

impl<'a> ModuleWorld<'a> {
    pub(crate) fn sensor(world: usize) -> Self {
        Self {
            world,
            edges: Edges::default(),
            droideka: None,
            scratch: cb::Scratch::default(),
            recon:None,
        }
    }
    // Unit.tick delegates movement according to its canonical body variant.
    // Soldiers use the retained movement kernel and current pass-through hull.
    // Droidekas use their prepared clips and persistent collision sidecar.
    // The allocation-stage label counts real requests without changing solver order.
    fn move_body(&mut self, id: u32, unit: &mut Unit, input: &MoveInput, h: f64) {
        let _allocations = crate::allocations::at(F3AllocationStage::Movement);
        match &mut unit.soldier {
            UnitBody::Soldier(s) => {
                let pass_through = match s.pass_through {
                    Some(body) => vm::collision_world().and_then(|c| c.kernel_slot(body.slot)).map_or(-1, |k| k as i64),
                    None => -1,
                };
                tick_soldier(s, input, h, self.world, pass_through);
            }
            UnitBody::Droideka(_) => {
                let mut parts = self.droideka.as_ref().expect("the droideka's class and clips").parts();
                droideka::tick_input(id, unit, input, h, self, &mut parts);
            }
        }
    }

    /// Runs `f` on his combat, the module's, taken out of the unit.
    // PlayerCombat operations temporarily split combat from its owning Unit.
    // Definitions are borrowed shared handles, not copied content tables.
    // The native world services permit immediate hits and nested explosions.
    // The caller restores the combat row before publishing a boundary result.
    fn on_combat(&mut self, id: u32, unit: &mut Unit, f: impl FnOnce(&mut Combat, &mut Unit, &cb::CombatDefs, &Edges, &mut JsWorld)) {
        let _allocations = crate::allocations::at(F3AllocationStage::Combat);
        // Damage can precede Unit.onMoved after an impact. Its barrier must
        // publish combat too rather than reusing the last consumed clocks.
        super::f3::changed(unit_ref(id));
        let defs = cm::defs();
        let mut c = std::mem::take(&mut unit.combat);
        let mut w = JsWorld::with_services(unit_ref(id), unit, &c,self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        f(&mut c, unit, &defs, &self.edges, &mut w);
        unit.combat = c;
    }

}

impl UnitsWorld for ModuleWorld<'_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
        native::cast(self.world, origin, direction, far)
    }

    fn visit_gathered(&mut self, _id: u32, center: &Vector3, radius: f64, cache: usize,
        visit: &mut dyn FnMut(bool, &[Vector3; 3])) {
        self.droideka.as_mut().expect("the droideka's collision caches")
            .visit_gathered(self.world, center, radius, cache, visit);
    }

    fn seat_exposed(&mut self, id: u32, _unit: &Unit) -> bool {
        super::live_pose::exposed(id, _unit)
    }

    fn rider_shapes(&mut self, id: u32, unit: &Unit, shapes: &mut [HitShape]) -> usize {
        super::live_pose::sample(id, unit, shapes)
    }

    fn held_shapes(&mut self, id: u32, unit: &Unit, shapes: &mut [HitShape]) -> usize {
        super::live_pose::sample(id, unit, shapes)
    }

    fn target_alive(&mut self, id: u32) -> bool { super::f3::target(id).is_some_and(|f| f.alive) }

    fn target_dead(&mut self, id: u32) -> bool { super::f3::target(id).is_some_and(|f| f.dead) }

    fn target_sphere(&mut self, id: u32) -> Bounds {
        super::f3::target(id).map_or_else(Bounds::default, |f| Bounds { center: f.bounds.center, radius: f.bounds.radius })
    }

    fn move_soldier(&mut self, id: u32, unit: &mut Unit, h: f64) {
        let input = unit.input.clone();
        self.move_body(id, unit, &input, h);
    }

    fn damage(&mut self, id: u32, unit: &mut Unit, amount: f64) {

            // combat.damage(amount): onShield the same, no source, nobody
            // credited, no info.
            let info = DamageInfo::default();
            self.on_combat(id, unit, |c, u, defs, _, w| cb::damage_with(c, u, defs, amount, amount, None, Attacker::Null, &info, w));

    }

    // Unit.tick calls its bot aimer only after native movement has completed.
    // Supply the declared current movement facts to the foreign aimer.
    // The reply contains qualified scalar deltas, never another full body import.
    // No other gameplay callback may use this observation to start a second phase.
    fn aim(&mut self, id: u32, unit: &mut Unit, h: f64) {
        let _allocations = crate::allocations::at(F3AllocationStage::Aimer);
        // The aimer only for a unit with a controller: without one the
        // AI's aim does nothing (src/ai/index.js aim).
        if unit.controller.is_none() {
            return;
        }
        if self.recon.as_ref().is_some_and(|services|services.ai.is_some()) {
            let services=self.recon.as_mut().unwrap();
            // The current row must be available to native aim observations.
            // Intent and weapon edits mark their own changes; this read does
            // not need to turn every moved bot into a full body publication.
            out_and_back(id,unit,false,|| {
                let reference=unit_ref(id);
                let key=F3UnitKey::of(reference,sim().store.unit.get(reference).unwrap());
                assert!(services.aim(&key,h),"active native AI aimer loan");
            });
            return;
        }
        // The actual Classic/Enhanced aimers need facing and movement, not
        // a second decode of every normal, water clock and weapon buffer.
        // Other completed units and immediate damage still flush below.
        let position = unit.position();
        let velocity = unit.velocity();
        let args = UnitAimArgs {
            unit: id as f64, dt: h, aim_yaw: unit.aim_yaw(), aim_pitch: unit.aim_pitch(),
            position: [position.x, position.y, position.z],
            velocity: [velocity.x, velocity.y, velocity.z],
            posture: match &unit.soldier { UnitBody::Soldier(s) => Some(s.posture), _ => None },
            tick_count: unit.tick_count,
        };

        let mut answer = None;
        out_and_back(id, unit, false, || {
            super::f3::before_hook();
            super::f3::begin_aimer();
            answer = Some(import_unit_aim(&args));
            super::f3::finish_aimer();
        });
        if let Some(a) = answer {
            unit.intent.aim = a.aim;
        }
    }

    fn has_remote_step(&mut self, id: u32, unit: &Unit) -> bool {
        crate::recon::module::context_for(unit_ref(id),unit.generation).is_some()
    }

    fn remote_step(&mut self, id: u32, unit: &mut Unit, h: f64, active: bool) {

            // NetworkRemoteWorld.step follows the aimer and control latch.
            // Commit the owner before recon can fire back through an instant
            // shot, consume controls or kill it with a recursive death blast.
            let context = crate::recon::module::context_for(unit_ref(id),unit.generation).unwrap();
            let key = sim().store.recon_context.get(context).unwrap().key;
            let input = crate::recon::Input {
                forward: if active { unit.intent.forward } else { 0.0 },
                right: if active { unit.intent.right } else { 0.0 },
                turn: if active { unit.intent.turn } else { 0.0 },
                ..Default::default()
            };
            let services = crate::units::f3::required_services!(self.recon.as_mut(),());
            let authority = &mut *services.recon;
            out_and_back(id, unit, true, || {
                authority.step(&mut sim().store, &mut crate::recon::module::NativeWorld::controlled(unit_ref(id), services.queries,services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks)),

                    key, super::f3::tick(), h,input).unwrap_or_else(|_|{crate::units::f3::fail();false});
                crate::recon::module::publish_active(authority);
            });
    }

    fn combat_reset(&mut self, id: u32, unit: &mut Unit) {
        let defs = cm::defs();
        let mut combat = std::mem::take(&mut unit.combat);
        cb::reset(&mut combat, unit, &defs);
        unit.combat = combat;
        super::f3::arsenal_reset(id);
    }

    // Unit.tick enters the real buffered PlayerCombat.update solver here.
    // Retained press edges belong to this tick and this gameplay life.
    // Take the phase scratch only for the duration of the serial combat update.
    // The next unit reuses the same storage after this row is restored.
    fn combat_update(&mut self, id: u32, unit: &mut Unit, h: f64, stick: f64) {

            let mut scratch = std::mem::take(&mut self.scratch);
            self.on_combat(id, unit, |c, u, defs, edges, w| cb::update_buffered(c, id, u, defs, h, stick, edges, w, &mut scratch));
            self.scratch = scratch;

    }

    // Unit.tick flushes deferred droideka shots after hidden rig posing.
    // Only the pending muzzle list is borrowed from the shared combat scratch.
    // The newly posed barrel combines with the shot's retained spread and owner.
    // An ordinary soldier has no deferred list to emit at this point.
    fn flush_muzzles(&mut self, id: u32, unit: &mut Unit) {
        // PlayerCombat.flushMuzzles has no work for an empty queue. Ordinary
        // soldiers never defer a shot, so do not split and restore their
        // complete combat or construct world services just to clear nothing.
        if unit.combat.pending_muzzles.is_empty() { return; }

            let mut pending = std::mem::take(&mut self.scratch.pending);
            self.on_combat(id, unit, |c, u, defs, edges, w| cb::flush_muzzles_buffered(c, u, defs, edges, w, &mut pending));
            self.scratch.pending = pending;

    }

    fn clear_charge(&mut self, _id: u32, _unit: &mut Unit) {
        // Unit.undraw clears the presentation glow when its removal notice drains.
    }

    fn death_blast(&mut self, id: u32, unit: &mut Unit) {

            F3BodyNotice {key:F3UnitKey::of(unit_ref(id),unit),event:F3BodyEvent::DeathBlast,health:0.0}.emit(&mut sim().events);
    }

    fn blow_apart(&mut self, id: u32, unit: &mut Unit, _health: f64) {

            let recon = self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow);
            out_and_back(id,unit,true,|| crate::combat::live::blow_apart(unit_ref(id),0.0,recon));
    }

    // Unit.remove detaches its Character synchronously before pool return.
    // Foreign battle bookkeeping can observe the removal at that exact slot.
    // The released Unit remains a stable registry identity for corpse readers.
    // Pure drawing cleanup is delivered later as a qualified notification.
    fn let_go_of_character(&mut self, id: u32, unit: &mut Unit) {

            // Unit.remove detaches its Character before clearing the row and
            // returning the body to its pool. This foreign pointer operation
            // cannot run movement, combat or factory adapters.
            if let Some(character) = unit.character {
                if crate::battle::f4_core::owns_lifecycle(){
                    let reference=unit_ref(id);
                    if let Some(c)=sim().store.character.get_mut(character){
                        if c.unit==Some(reference){c.unit=None;crate::battle::f4_core::mark_character(character);}
                    }
                }else{import_f3_character_release(&F3CharacterRelease { unit:unit_ref(id), character });}
            }
    }

    fn release(&mut self, id: u32, unit: &Unit) {

            // UnitFactory's pool is canonical already. The remaining undraw
            // notification can wait for the outer return without an import.
            F3RemovedNotice { key:F3UnitKey::of(unit_ref(id),unit) }.emit(&mut sim().events);
    }

    fn sample_muzzles(&mut self, id: u32, unit: &mut Unit, h: f64) {
        if unit.walker { assert!(muzzles::sample(id, unit, h), "prepared droideka rig"); }
    }
}

/// UnitFactory's inactive body carries its collision and weapon scratch.
/// Moving these buffers at birth preserves their capacities and rig identity.
pub(crate) struct PreparedBody {
    droideka: Option<DroidekaBody>,
    muzzle: Option<muzzles::Muzzles>,
    pose: Option<super::live_pose::Body>,
}
pub(crate) fn take_body(slot: u32) -> PreparedBody {
    PreparedBody {
        droideka: phase().take_droideka(slot),
        muzzle: muzzles::take(slot),
        pose: super::live_pose::take(slot),
    }
}
pub(crate) fn install_body(reference: Ref<Unit>, body: PreparedBody) {
    let p = phase();
    p.install_droideka(reference.slot, body.droideka);
    muzzles::install(reference.slot, body.muzzle);
    super::live_pose::install(reference, body.pose);
}
pub(crate) fn reserve_bodies(count: usize) {
    let p = phase();
    p.reserve_bodies(count);
    muzzles::reserve(count);
    super::live_pose::reserve(count);
}

impl Phase {
    fn take_droideka(&mut self, slot: u32) -> Option<DroidekaBody> {
        self.droidekas.get_mut(slot as usize).and_then(Option::take)
    }
    fn install_droideka(&mut self, slot: u32, body: Option<DroidekaBody>) {
        self.droidekas[slot as usize] = body;
    }
    fn reserve_bodies(&mut self, count: usize) {
        self.droidekas.resize_with(count.max(self.droidekas.len()), || None);
    }
}
