//! Unit.tick and the following water and death-region checks over the
//! battle's canonical rows. Bodies, collision caches and combat scratch
//! survive ticks; only the aimer and Character bookkeeping cross back to
//! JavaScript. Presentation notices drain after the owning phase returns.

use super::droideka;
use super::muzzles;
use super::native::{self, DroidekaBody, DroidekaClass};
use super::soldier::tick_soldier_in;
use super::shapes::{Bounds, HitShape};
use super::water::{soldier_step_at, WaterEffects};
use super::world::{CastHit, UnitsWorld};
use crate::combat::combat::{self as cb, Edges};
use crate::combat::module::{self as cm, JsWorld};
use crate::combat::world::DamageInfo;
use crate::gen::*;
use crate::store::{word_u32, Nullable, Ref};
use crate::vehicles::module as vm;
use three::Vector3;

/// What the phase knows between its calls.
pub(super) struct Phase {
    world: usize,
    scratch: cb::Scratch,
    droideka_classes: Vec<DroidekaClass>,
    droidekas: Vec<Option<DroidekaBody>>,
}

fn phase_in(units: &mut super::owner::Units) -> &mut Phase {
units.phase.get_or_insert_with(|| Phase { world: 0, scratch: cb::Scratch::default(), droideka_classes: Vec::new(), droidekas: Vec::new() })
}

// Unit's public id is also its canonical slab slot in the owning battle.
// Look up the current slot generation rather than trusting a JS body mirror.
// Commands validate gameplay life separately before reaching this helper.
// A missing row carries an invalid generation and cannot alias a replacement.
fn unit_ref_in(store: &crate::gen::Store, id: u32) -> Ref<Unit> {
store.unit.reference(id).unwrap_or(Ref::new(id, 0))
}

/// Unit `id` and his combat back in the store for an import (the store
/// retains his identity while the phase holds his row), then out
/// again as JavaScript left them.
// Unit.tick's foreign aimer needs the movement result already committed.
// Temporarily restore the row and combat before entering that observation.
// A nested query then sees the canonical state, not an empty taken-out frame.
// Recover the same row afterward so the resumed tick sees accepted commands.
fn out_and_back_in(world_ctx: &mut crate::world::owner::World, id: u32, unit: &mut Unit, changed: bool, call: impl FnOnce(&mut crate::world::owner::World)) {
    let r = unit_ref_in(&world_ctx.sim.store, id);
    cm::put_body_in(world_ctx, r, std::mem::take(unit));
    if changed { super::f3::changed_in(world_ctx, r); }
    call(world_ctx);
    *unit = cm::take_body_in(world_ctx, r);
}

// Unit.tick advances one combat at a time. Damage can nest while that
// combat owns its shot buffers, but cannot start another combat update.
// Lifecycle operations use empty scratch and never borrow these buffers.
fn take_scratch_in(units: &mut super::owner::Units) -> cb::Scratch { std::mem::take(&mut phase_in(units).scratch) }
// UnitFactory construction reserves the shared combat work bound.
// The largest ordinary salvo uses this retained shot and muzzle capacity.
// Adding another body does not allocate another copy of the arc buffers.
// Only one combat update borrows this scratch at a time.
pub(crate) fn reserve_unit_in(units: &mut super::owner::Units, _id:u32) { phase_in(units).scratch.reserve(64); }

/// Bind the prepared collision world before any body is adopted.
pub(crate) fn setup_in(units: &mut super::owner::Units, world: u32) {
    phase_in(units).world = world as usize;
}

/// A droideka class's movement numbers and clip ids, written once in the
/// call block (native.rs). Returns its number for units_droideka_attach.
pub fn units_droideka_class_in(world_ctx: &mut crate::world::owner::World) -> u32 {
    let (row, _) = world_ctx.sim.calls.args();
    let class = DroidekaClass::read(&world_ctx.pose, row);
    let classes = &mut phase_in(&mut world_ctx.units).droideka_classes;
    classes.push(class);
    (classes.len() - 1) as u32
}

/// The class and persistent collision caches for one pooled body.

// Droideka construction binds its immutable clip and movement class.
// The body sidecar owns reusable collision gathers and hidden gun state.
// It follows the pooled body across lives rather than being rebuilt on spawn.
// Class replacement moves this resource with its resident construction cookie.
pub fn units_droideka_attach_in(world_ctx: &mut crate::world::owner::World, unit: u32, class: u32) {
    let p = phase_in(&mut world_ctx.units);
    let body = DroidekaBody::new(&world_ctx.collision.state, &p.droideka_classes[class as usize], p.world);
    if p.droidekas.len() <= unit as usize {
        p.droidekas.resize_with(unit as usize + 1, || None);
    }
    p.droidekas[unit as usize] = Some(body);
}

/// A prepared hidden gun hierarchy, before native posing takes ownership.

// Droideka.arm has prepared a hidden muzzle hierarchy before this call.
// The hierarchy is independent of the client's interpolated drawn bones.
// Gameplay later samples this native rig after movement and before flush.
// No per-barrel JavaScript query runs inside the combat update.
pub fn units_muzzle_attach_in(world_ctx: &mut crate::world::owner::World, unit: u32, class: u32) {
    let parts = phase_in(&mut world_ctx.units).droideka_classes[class as usize].parts();
    let (row, _) = world_ctx.sim.calls.args();
    muzzles::attach_in(&mut world_ctx.units, unit, parts, row);
}

/// Unit.tick and the world's immediately following water/death-region tests.
/// The owned phase supplies retained trigger edges instead of parsing a list.
pub(crate) fn tick_owned_in(world_ctx: &mut crate::world::owner::World, info: &UnitsPhaseUnit, level: f64, dt: f64, edges: Edges, recon: Option<crate::units::f3::WorldServices<'_>>) {
let id = word_u32(info.unit);
    let reference = unit_ref_in(&world_ctx.sim.store,id);
    let world = phase_in(&mut world_ctx.units).world;
    super::f3::invalidate_bounds_in(&mut world_ctx.units,id);
    let mut unit = cm::take_body_in(world_ctx,reference);
    let droideka = phase_in(&mut world_ctx.units).droidekas.get_mut(id as usize).and_then(Option::take);
    let scratch = take_scratch_in(&mut world_ctx.units);
    let mut w = ModuleWorld { w:world_ctx, world, edges, droideka, scratch, recon };
    unit.tick(id, dt, &mut w, None);
    after_tick(id, &mut unit, info, level, dt, &mut w);
    if let Some(body) = w.droideka.take() { phase_in(&mut w.w.units).droidekas[id as usize] = Some(body); }
    phase_in(&mut w.w.units).scratch = std::mem::take(&mut w.scratch);
    cm::put_body_in(w.w,reference,unit);
}

/// Unit.spawn and remove over the same body services as Unit.tick.
// Unit.spawn and remove borrow the same body services as an ordinary tick.
// They cannot borrow the active combat scratch during a nested death.
// Reset and removal need no shot buffers, so use an empty scratch frame.
// The native body sidecar is restored before returning to the owning boundary.
pub(crate) fn lifecycle_owned_in(world_ctx: &mut crate::world::owner::World, id: u32, spawn: Option<([f64; 3], f64)>, recon: Option<crate::units::f3::WorldServices<'_>>) {
super::f3::invalidate_bounds_in(&mut world_ctx.units,id);
    let reference = unit_ref_in(&world_ctx.sim.store,id);
    let p = phase_in(&mut world_ctx.units);
    let world = p.world;
    let droideka = p.droidekas.get_mut(id as usize).and_then(Option::take);
    let mut unit = cm::take_body_in(world_ctx,reference);
    let mut w = ModuleWorld { w:world_ctx, world, edges:Edges::default(), droideka, scratch:cb::Scratch::default(), recon };
    if let Some(([x,y,z],yaw)) = spawn { unit.spawn(id,x,y,z,yaw,&mut w); }
    else { unit.remove(id,&mut w); }
    if let Some(body) = w.droideka.take() { phase_in(&mut w.w.units).droidekas[id as usize] = Some(body); }
    cm::put_body_in(w.w,reference,unit);
}

fn held(u: &Unit) -> bool {
    matches!(u.held, Nullable::Value(_))
}

/// The rest of the step's loop for a unit after his tick:
/// a soldier in the water, a droideka in deep water, a death region.
// Game.step applies water and death regions immediately after Unit.tick.
// Use that tick's body position, including a just-completed droideka movement.
// Hazards can kill immediately and therefore share the combat damage services.
// Their effect notices keep the same observation order as ordinary impacts.
fn after_tick(id: u32, unit: &mut Unit, info: &UnitsPhaseUnit, level: f64, dt: f64, w: &mut ModuleWorld) {
    let water = vm::map_in(w.w).is_some_and(|m| m.water.is_some());
    if water && unit.alive && unit.seat.is_none() && !held(unit) && !unit.walker {
        // unit.waterContact ??= {}: a contact whose clocks are all unset.
        let mut contact = unit.water_contact.take().unwrap_or_default();
        let feet = unit.position();
        let head = Vector3 { x: unit.combat.pivot[0], y: unit.combat.pivot[1], z: unit.combat.pivot[2] };
        let gun = Vector3 { x: unit.combat.origin[0], y: unit.combat.origin[1], z: unit.combat.origin[2] };
        let velocity = unit.velocity();
        let mut splash = Splash { at: None };
        let hurt = {
            let rules = vm::map_in(w.w).expect("the map's rules");
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
            splash.emit(&mut w.w.sim.events);
        }
        if hurt != 0.0 && !hurt.is_nan() {
            w.damage(id, unit, hurt);
        }
    }
    if water && unit.alive && unit.seat.is_none() && !held(unit) && unit.walker {
        if let (Some(rule), UnitBody::Droideka(d)) = (&info.droideka_water, &mut unit.soldier) {
            // waterDamageStep(body, dt, water, rule,
            // body.position, droidekaWaterDepth(def, rolled)).
            let mut hurt = 0.0;
            if dt > 0.0 {
                let depth = if d.mode >= 11.0 { rule.ball } else { rule.upright };
                let rules = vm::map_in(w.w).expect("the map's rules");
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
        if vm::map_in(w.w).is_some_and(|m| m.in_death_region(&p)) {
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
pub(crate) struct ModuleWorld<'w, 'a> {
    pub(crate) w: &'w mut crate::world::owner::World,
    world: usize,
    /// His trigger edges (a network player's).
    edges: Edges,
    droideka: Option<DroidekaBody>,
    scratch: cb::Scratch,
    recon: Option<crate::units::f3::WorldServices<'a>>,
}

/// One soldier's own state, refused by a Battlefront II step, is that soldier's fault: it is
/// counted and logged and the world goes on. Anything else (a family without its data, a bad
/// timestep or class) is every soldier's and fails the step.
fn is_soldier_fault(error: crate::movement::ProfileError, owned_infantry: bool) -> bool {
    owned_infantry && error == crate::movement::ProfileError::InvalidState
}

#[cfg(test)]
mod soldier_fault_tests {
    use super::is_soldier_fault;
    use crate::movement::ProfileError::*;

    #[test]
    fn only_a_refused_soldier_state_of_the_owned_infantry_is_contained() {
        assert!(is_soldier_fault(InvalidState, true));
        assert!(!is_soldier_fault(InvalidState, false));
        for every_soldiers in [MissingStep, UnexpectedStep, InvalidDelta, InvalidTuning] {
            assert!(!is_soldier_fault(every_soldiers, true), "{every_soldiers:?}");
        }
    }
}

impl<'w, 'a> ModuleWorld<'w, 'a> {
    pub(crate) fn sensor_in(w: &'w mut crate::world::owner::World, world: usize) -> Self {
        Self {
            w,
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
        match &mut unit.soldier {
            UnitBody::Soldier(s) => {
                let pass_through = match s.pass_through {
                    Some(body) => vm::collision_world_in(self.w).and_then(|c| c.kernel_slot(body.slot)).map_or(-1, |k| k as i64),
                    None => -1,
                };
                let reference = self.w.units.infantry.as_ref().map(|_| super::f3::unit_reference_in(&self.w.units, id));
                let result = if let Some((reference, profile)) = reference.zip(self.w.units.infantry.as_mut()) {
                    if let Some(feet) = self.w.space.as_deref().and_then(|sp| sp.terminal_feet(id, unit.generation)) {
                        s.previous = s.position;
                        s.position = feet;
                        s.settled = feet;
                        s.velocity = [0.;3];
                        return;
                    }
                    let life = super::infantry::contact::LifeId {slot:id,slab:reference.gen,generation:unit.generation as u32};
                    let registered = !unit.human && profile.bot_inputs.installation.is_some();
                    let bot_frame = if registered {
                        let defs = crate::combat::module::defs_in(&mut self.w.combat);
                        let binding = super::infantry::bot_input::equipped(&mut unit.combat, &defs.weapons);
                        profile.bot_inputs.take(profile.tick, life, binding)
                    } else { None };
                    let controls = if unit.human {
                        let mut controls = self.w.input.mailboxes.get_mut(profile.mailbox as usize).map(|m| m.infantry_controls(life)).unwrap_or_default();
                        // A network human's buttons reach the soldier through
                        // his player shadow; a local one's through the mailbox
                        // above.
                        if let Some(shadow) = self.w.input.players.shadows.get_mut(id as usize) {
                            if shadow.swbf2_life == unit.generation {
                                shadow.swbf2.take_into(&mut controls);
                            }
                        }
                        controls
                    } else {
                        bot_frame.map(|delivery| delivery.frame.controls()).unwrap_or_default()
                    };
                    let held = crate::combat::powers::held_in(self.w, crate::combat::powers::LifeKey { slot: id, generation: reference.gen, life: unit.generation });
                    let profile = self.w.units.infantry.as_mut().unwrap();
                    profile.advance(&mut self.w.movement, &mut self.w.collision.state, id, reference.gen, unit.class_name,
                        unit.generation, unit.human, unit.alive && !unit.dead, controls, bot_frame, held, s, input, h, self.world, pass_through)
                } else {
                    tick_soldier_in(&mut self.w.movement, &mut self.w.collision.state, s, input, h, self.world, pass_through, None)
                };
                if let Err(error) = result {
                    if is_soldier_fault(error, self.w.units.infantry.is_some()) {
                        super::f3::soldier_fault_in(&mut self.w.units, id, error);
                    } else {
                        super::f3::fail_in(&mut self.w.units);
                    }
                }
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
    // The released step: the buffered PlayerCombat.update solver alone, with no
    // per-life soldier input, power or throw handling around it.
    fn released_combat_update(&mut self, id: u32, unit: &mut Unit, h: f64, stick: f64) {
        let mut scratch = std::mem::take(&mut self.scratch);
        self.on_combat(id, unit, |c, u, defs, edges, w| cb::update_buffered(c, id, u, defs, h, stick, edges, w, &mut scratch));
        self.scratch = scratch;
    }

    fn on_combat(&mut self, id: u32, unit: &mut Unit, f: impl FnOnce(&mut Combat, &mut Unit, &cb::CombatDefs, &Edges, &mut JsWorld)) {
        // Damage can precede Unit.onMoved after an impact. Its barrier must
        // publish combat too rather than reusing the last consumed clocks.
        let reference = unit_ref_in(&self.w.sim.store,id);
        super::f3::changed_in(self.w,reference);
        let defs = cm::defs_in(&mut self.w.combat);
        let mut c = std::mem::take(&mut unit.combat);
        let mut w = JsWorld::with_services_in(self.w, reference, unit, &c,self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        f(&mut c, unit, &defs, &self.edges, &mut w);
        w.finish();
        unit.combat = c;
    }

}

impl UnitsWorld for ModuleWorld<'_, '_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
        native::cast_in(self.w, self.world, origin, direction, far)
    }

    fn visit_gathered(&mut self, _id: u32, center: &Vector3, radius: f64, cache: usize,
        visit: &mut dyn FnMut(bool, f64, &[Vector3; 3])) {
        self.droideka.as_mut().expect("the droideka's collision caches")
            .visit_gathered(&self.w.collision.state, self.w.movement.flags_mut(), self.world, center, radius, cache, visit);
    }

    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<(f64, Vector3)> {
        crate::collision::spheres::ground(&mut self.w.collision.state, self.world, x, z, top)
            .map(|(height, n)| (height, Vector3::new(n[0], n[1], n[2])))
    }

    fn seat_exposed(&mut self, id: u32, _unit: &Unit) -> bool {
        super::live_pose::exposed_in(&self.w.fleet,&self.w.sim.store, id, _unit)
    }

    fn rider_shapes(&mut self, id: u32, unit: &Unit, shapes: &mut [HitShape]) -> usize {
        super::live_pose::sample_in(self.w, id, unit, shapes)
    }

    fn held_shapes(&mut self, id: u32, unit: &Unit, shapes: &mut [HitShape]) -> usize {
        super::live_pose::sample_in(self.w, id, unit, shapes)
    }

    fn target_alive(&mut self, id: u32) -> bool { super::f3::target_in(self.w, id).is_some_and(|f| f.alive) }

    fn target_dead(&mut self, id: u32) -> bool { super::f3::target_in(self.w, id).is_some_and(|f| f.dead) }

    fn target_sphere(&mut self, id: u32) -> Bounds {
        super::f3::target_in(self.w, id).map_or_else(Bounds::default, |f| Bounds { center: f.bounds.center, radius: f.bounds.radius })
    }

    fn move_soldier(&mut self, id: u32, unit: &mut Unit, h: f64) {
        let input = unit.input.clone();
        if self.w.space.is_none() || !crate::space::move_mounted(self.w, id, unit) {
            self.move_body(id, unit, &input, h);
        }
    }

    fn carried_in_space(&self, slot: u32, life: f64) -> bool {
        self.w.space.is_some() && crate::space::carried(self.w, slot, life)
    }
    fn map_world_bounds(&self) -> Option<super::damage::MapWorldBounds> {
        self.w.units.map_world_bounds
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
        // The aimer only for a unit with a controller: without one the
        // AI's aim does nothing (aim).
        if unit.controller.is_none() {
            return;
        }
        if self.recon.as_ref().is_some_and(|services|services.ai.is_some()) {
            let services=self.recon.as_mut().unwrap();
            // The current row must be available to native aim observations.
            // Intent and weapon edits mark their own changes; this read does
            // not need to turn every moved bot into a full body publication.
            out_and_back_in(self.w, id,unit,false,|world_ctx| {
                let reference=unit_ref_in(&world_ctx.sim.store, id);
                let key=F3UnitKey::of(reference,world_ctx.sim.store.unit.get(reference).unwrap());
                assert!(services.aim_in(world_ctx,&key,h),"active native AI aimer loan");
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
        out_and_back_in(self.w, id, unit, false, |world_ctx| {
            super::f3::before_hook_in(world_ctx);
            super::f3::begin_aimer_in(&mut world_ctx.units);
            answer = Some(import_unit_aim(&args));
            super::f3::finish_aimer_in(world_ctx);
        });
        if let Some(a) = answer {
            unit.intent.aim = a.aim;
        }
    }

    fn has_remote_step(&mut self, id: u32, unit: &Unit) -> bool {
        {let reference = unit_ref_in(&self.w.sim.store,id); crate::recon::module::context_for_in(self.w,reference,unit.generation)}.is_some()
    }

    fn remote_step(&mut self, id: u32, unit: &mut Unit, h: f64, active: bool) {

            // NetworkRemoteWorld.step follows the aimer and control latch.
            // Commit the owner before recon can fire back through an instant
            // shot, consume controls or kill it with a recursive death blast.
            let context = {let reference = unit_ref_in(&self.w.sim.store,id); crate::recon::module::context_for_in(self.w,reference,unit.generation)}.unwrap();
            let key = self.w.sim.store.recon_context.get(context).unwrap().key;
            let input = crate::recon::Input {
                forward: if active { unit.intent.forward } else { 0.0 },
                right: if active { unit.intent.right } else { 0.0 },
                turn: if active { unit.intent.turn } else { 0.0 },
                ..Default::default()
            };
            let services = crate::units::f3::required_services!(&mut self.w.units; self.recon.as_mut(),());
            let authority = &mut *services.recon;
            let tick = super::f3::tick_in(&self.w.units);
            out_and_back_in(self.w, id, unit, true, |world_ctx| {
                let reference = unit_ref_in(&world_ctx.sim.store,id);
                crate::recon::module::with_native(world_ctx, Some(reference), authority, services.queries, services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks), |authority, store, world| authority.step(store, world, key, tick, h, input)).unwrap_or_else(|_|{crate::units::f3::fail_in(&mut world_ctx.units);false});
                crate::recon::module::publish_active_in(world_ctx, authority);
            });
    }

    fn combat_reset(&mut self, id: u32, unit: &mut Unit) {
        let defs = cm::defs_in(&mut self.w.combat);
        let mut combat = std::mem::take(&mut unit.combat);
        cb::reset(&mut combat, unit, &defs);
        unit.combat = combat;
        super::f3::arsenal_reset_in(&mut self.w.units, id);
    }

    // Unit.tick enters the real buffered PlayerCombat.update solver here.
    // Retained press edges belong to this tick and this gameplay life.
    // Take the phase scratch only for the duration of the serial combat update.
    // The next unit reuses the same storage after this row is restored.
    fn combat_update(&mut self, id: u32, unit: &mut Unit, h: f64, stick: f64) {
        // A battle without the per-life soldier profile (every 2004 battle) runs the released step.
        if self.w.units.infantry.is_none() {
            return self.released_combat_update(id, unit, h, stick);
        }

        let blocked = self.w.units.infantry.as_ref().is_some_and(|profile| {
            profile.blocks_fire(id, unit.generation) || self.w.space().is_some_and(|s| {
                crate::space::carried(self.w, id, unit.generation) || s.terminal_feet(id, unit.generation).is_some()
            })
        });
        let reload_blocked = self.w.units.infantry.as_ref().is_some_and(|profile| {
            profile.blocks_reload(id, unit.generation)
        });
        let buttons = unit.intent.buttons;
        if blocked {
            unit.intent.buttons.fill(0.);
            unit.controls.pressed.fill(false);
        }
        if reload_blocked {
            unit.intent.reload = false;
        }
        let infantry = self.w.units.infantry.is_some();
        let offhand = self.w.units.infantry.as_mut().is_some_and(|i| i.offhand_edge(id, unit.generation, unit.human, buttons[1] > 0.));
        let mut throw = false;
        let mut power = None;
        let mut scratch = std::mem::take(&mut self.scratch);
        let class = self.w.units.infantry.as_ref().and_then(|i| i.class_of(id, unit.generation));
        let powers = class.map(|class| super::infantry::kit_slots(self.w, class)).unwrap_or([None; crate::combat::powers::MAX_POWERS]);
        self.on_combat(id, unit, |c, u, defs, edges, w| {
            if infantry && !blocked {
                let (t, p) = super::infantry::mask_special_actions(c, u, &defs.weapons, offhand, &powers);
                throw = t;
                power = p;
            }
            let result = cb::update_buffered(c, id, u, defs, h, stick, edges, w, &mut scratch);
            // The selected offhand power, whether or not its trigger is down.
            if infantry && power.is_none() {
                power = super::infantry::offhand_power(c, &defs.weapons, &powers);
            }
            result
        });
        self.scratch = scratch;
        if infantry { unit.intent.buttons = buttons; }
        if throw { self.w.units.infantry.as_mut().unwrap().request_throw(id); }
        if let Some(i) = self.w.units.infantry.as_mut() {
            i.set_power_input(id, unit.generation, super::infantry::power_input(offhand && !blocked, buttons[1] > 0. && !blocked, power));
        }

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
            self.on_combat(id, unit, |c, u, defs, _, w| cb::flush_muzzles_buffered(c, u, defs, w, &mut pending));
            self.scratch.pending = pending;

    }

    fn clear_charge(&mut self, _id: u32, _unit: &mut Unit) {
        // Unit.undraw clears the presentation glow when its removal notice drains.
    }

    fn death_blast(&mut self, id: u32, unit: &mut Unit) {

            F3BodyNotice {key:F3UnitKey::of(unit_ref_in(&self.w.sim.store, id),unit),event:F3BodyEvent::DeathBlast,health:0.0}.emit(&mut self.w.sim.events);
    }

    fn blow_apart(&mut self, id: u32, unit: &mut Unit, _health: f64) {

            let recon = self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow);
            out_and_back_in(self.w,id,unit,true,|world_ctx| {let reference = unit_ref_in(&world_ctx.sim.store,id); crate::combat::live::blow_apart_in(world_ctx,reference,0.0,recon)});
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
                if crate::battle::f4_core::owns_lifecycle_in(&mut self.w.battle){
                    let reference=unit_ref_in(&self.w.sim.store, id);
                    if let Some(c)=self.w.sim.store.character.get_mut(character){
                        if c.unit==Some(reference){c.unit=None;crate::battle::f4_core::mark_character_in(&mut self.w.battle, character);}
                    }
                }else{import_f3_character_release(&F3CharacterRelease { unit:unit_ref_in(&self.w.sim.store, id), character });}
            }
    }

    fn release(&mut self, id: u32, unit: &Unit) {

            // UnitFactory's pool is canonical already. The remaining undraw
            // notification can wait for the outer return without an import.
            F3RemovedNotice { key:F3UnitKey::of(unit_ref_in(&self.w.sim.store, id),unit) }.emit(&mut self.w.sim.events);
    }

    fn sample_muzzles(&mut self, id: u32, unit: &mut Unit, h: f64) {
        if unit.walker { assert!(muzzles::sample_in(&mut self.w.units,id, unit, h), "prepared droideka rig"); }
    }
}

/// UnitFactory's inactive body carries its collision and weapon scratch.
/// Moving these buffers at birth preserves their capacities and rig identity.
pub(crate) struct PreparedBody {
    droideka: Option<DroidekaBody>,
    muzzle: Option<muzzles::Muzzles>,
    pose: Option<super::live_pose::Body>,
}
pub(crate) fn take_body_in(units: &mut super::owner::Units, slot: u32) -> PreparedBody {
    PreparedBody {
        droideka: phase_in(units).take_droideka(slot),
        muzzle: muzzles::take_in(units, slot),
        pose: super::live_pose::take_in(units, slot),
    }
}
pub(crate) fn install_body_in(units: &mut super::owner::Units, reference: Ref<Unit>, body: PreparedBody) {
    let p = phase_in(units);
    p.install_droideka(reference.slot, body.droideka);
    muzzles::install_in(units, reference.slot, body.muzzle);
    super::live_pose::install_in(units, reference, body.pose);
}
pub(crate) fn reserve_bodies_in(units: &mut super::owner::Units, count: usize) {
    let p = phase_in(units);
    p.reserve_bodies(count);
    muzzles::reserve_in(units, count);
    super::live_pose::reserve_in(units, count);
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

// Unit.sphere and hitShapes may sample a rider or held pose. Lend only the
// queried row so those services receive the same root without aliasing it.
// Geometry cannot initiate gameplay callbacks or retire this identity.
pub(super) fn with_unit_in<T>(w: &mut crate::world::owner::World, reference: Ref<Unit>, run: impl FnOnce(&mut ModuleWorld<'_, '_>, &mut Unit) -> T) -> Option<T> {
    let mut unit = w.sim.store.unit.lend(reference)?;
    let world = super::f3::collision_world_in(&w.units);
    let mut services = ModuleWorld::sensor_in(w,world);
    let result = run(&mut services,&mut unit);
    assert!(services.w.sim.store.unit.restore(reference,unit).is_ok(),"queried unit identity retained");
    Some(result)
}

// Units.raycast and query retain registry order and the original shape
// observation points. Their slab and list are owned locals during sampling;
// the world keeps every other store field available to rider services.
pub(super) fn with_registry_in<T>(w: &mut crate::world::owner::World, run: impl FnOnce(&mut ModuleWorld<'_, '_>, &crate::gen::Units, &mut crate::store::Slab<Unit>) -> T) -> T {
    let mut slab = std::mem::take(&mut w.sim.store.unit);
    let registry = std::mem::take(&mut w.sim.store.units);
    let world = super::f3::collision_world_in(&w.units);
    let mut services = ModuleWorld::sensor_in(w,world);
    let result = run(&mut services,&registry,&mut slab);
    services.w.sim.store.unit = slab;
    services.w.sim.store.units = registry;
    result
}

