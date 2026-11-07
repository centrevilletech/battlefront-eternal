//! LiveObjects' combat services (props.js). Prepared geometry and native
//! lifecycle effects share F4's canonical health and trap rows. Only the
//! touched prop's visibility and foreign notifications cross synchronously.
use super::f3::{self, WorldServices};
use crate::battle::f4;
use crate::battle::{live, props, traps};
use crate::combat::impacts::{BlastPacket, HealthScale, HitPacket};
use crate::gen::*;
use crate::ordnance::{defs::{ExplosionDef, Field}, world::DefArg};
use crate::store::{DefId, Ref, StrId};
use three::Vector3;

// Targeting metadata names the canonical F4 row. Qualified attribution stays
// with that row after the shooter returns to its pool; prepared bounds, body
// handles and visitation order remain independent of the shooter's next life.
struct Entry {
    target: RepairTarget,
    member: Option<Member>,
    lock_key: StrId,
    settings: props::Settings,
    owner: F4Handle,
    geometry_state: props::State,
    team: f64,
    target_team: f64,
    gunner: Option<Ref<Unit>>,
    bodies: [Option<Ref<Body>>; 3],
    explosion: Option<DefId>,
    // Trap.sphere always observes the build pad, including while logs swing.
    trap_sphere: Option<(Vector3, f64)>,
}
impl Entry {
    fn frame(&self) -> props::Building { f4::frame(&self.owner) }
    fn attacker(&self) -> Attacker { f4::attacker(&self.owner) }
    fn trap(&self) -> Option<traps::Trap> { f4::trap(&self.owner) }
    fn put_frame(&self, frame: &props::Building) { f4::put_frame(&self.owner, frame); }
    fn put_attacker(&self, attacker: Attacker) { f4::set_attacker(&self.owner, attacker); }
    fn put_trap(&self, trap: traps::Trap) { f4::put_trap(&self.owner, trap); }
}
/// captureWorldPresentation reads F4's canonical frame and cold affiliation.
/// Only Turret defines facing/yaw in LiveObjects.buildings; that class has its
/// own entity record. Other buildings preserve the original absent-to-0 fields.
pub(crate) fn wire_building_in(world_ctx: &mut crate::world::owner::World, index: usize) -> Option<crate::wire::store_world::Building> {
    use crate::wire::store_world::{Building, Station};
    let entry = f3::props_in(&mut world_ctx.units).entries.get(index)?; let frame = entry.frame();
    let station = if entry.owner.kind == F4ObjectKind::Trap { None }
        else { let (value, _, supply) = f4::station(&entry.owner);
            supply.map(|_| Station { active_mode: value.active_mode, active: value.active, pulse: value.pulse,
                served_health: value.served_health, served_ammo: value.served_ammo }) };
    Some(Building { turret: matches!(entry.target, RepairTarget::Turret(_)),
        state: match frame.state { props::State::Unbuilt => 0, props::State::Built => 1, props::State::Destroyed => 2 },
        health: frame.health, team: entry.team, facing: 0., yaw: 0., station })
}

// Prepared order and reverse lookups never grow during a phase. Buildings
// absent from the mixed registry still participate in repair, blast and melee.
pub(crate) struct Props {
    entries: Vec<Entry>,
    extra_melee: Vec<usize>,
    building_slots: Vec<Option<usize>>,
    turret_slots: Vec<Option<usize>>,
    body_slots: Vec<Option<usize>>,
    member_slots: Vec<Option<usize>>,
}
impl Props {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            extra_melee: Vec::new(),
            building_slots: Vec::new(),
            turret_slots: Vec::new(),
            body_slots: Vec::new(),
            member_slots: Vec::new(),
        }
    }
    // repairLockKey names the actual pooled object, not the registry target.
    // A recycled kind slot with another generation cannot inherit that lock.
    fn index(&self, store: &Store, target: RepairTarget) -> Option<usize> {
        let (slots, slot) = match target {
            RepairTarget::Building(r) => (&self.building_slots, r.slot),
            RepairTarget::Turret(r) => (&self.turret_slots, r.slot),
            // Hull repair uses the canonical F2 store when its service joins.
            RepairTarget::Vehicle(r) => return store.vehicles.vehicles.iter().position(|&v| v == r)
                .map(|i| self.entries.len() + i),
        };
        let index = slots.get(slot as usize).copied().flatten()?;
        (self.entries[index].target == target).then_some(index)
    }
}
// Turret targeting uses its prepared mixed-registry viewer. This lookup
// follows the prop identity even after a new occupant changes its target team.
pub(crate) fn member_in(world_ctx: &mut crate::world::owner::World, target: RepairTarget) -> Option<Member> {
    let props = f3::props_in(&mut world_ctx.units);
    props.index(&world_ctx.sim.store, target).and_then(|index| props.entries.get(index)).and_then(|entry| entry.member)
}

/// AI cold content must name the same prop, registry entry and F4 owner.
/// Matching an enum variant alone would accept another building's handle.
pub(crate) fn matches_binding_in(world_ctx: &mut crate::world::owner::World, target:RepairTarget,member:Option<Member>,owner:&F4Handle)->bool {
    let props=f3::props_in(&mut world_ctx.units);
    props.index(&world_ctx.sim.store, target).and_then(|index|props.entries.get(index))
        .is_some_and(|entry|entry.member==member&&entry.owner==*owner)
}

/// Game.los compares a target with its authored built body, even when the
/// building currently shows a wreck. The qualified prop binding owns it.
pub(crate) fn built_body_in(world_ctx: &mut crate::world::owner::World, target:RepairTarget)->Option<Ref<Body>> {
    let props=f3::props_in(&mut world_ctx.units);let entry=props.entries.get(props.index(&world_ctx.sim.store, target)?)?;
    entry.bodies[0]
}

/// Trap.sphere and position use the authored build pad even while logs swing.
/// Its static frame already belongs to the same canonical prop binding.
pub(crate) fn trap_geometry_in(world_ctx: &mut crate::world::owner::World, target:RepairTarget)->Option<(crate::ai::combat::world::Sphere,three::Matrix4)> {
    let props=f3::props_in(&mut world_ctx.units);let entry=props.entries.get(props.index(&world_ctx.sim.store, target)?)?;
    let (center,radius)=entry.trap_sphere?;
    Some((crate::ai::combat::world::Sphere {center,radius},entry.settings.matrix))
}

/// EntityWorldCapture.turret reads authored building order and canonical
/// health/life. A turret with no mixed registry target has no wire identity.
pub(crate) fn wire_turret_in(world_ctx: &mut crate::world::owner::World, reference: Ref<Turret>) -> Option<(usize, Member, props::Building, f64)> {
    let all = f3::props_in(&mut world_ctx.units);
    let index = all.index(&world_ctx.sim.store, RepairTarget::Turret(reference))?;
    let entry = &all.entries[index];
    Some((index, entry.member?, entry.frame(), entry.target_team))
}

// LiveObjects construction gives each collision look its own body identity.
// Precompute reverse indices now so struck never searches the building list.
fn bind(slots: &mut Vec<Option<usize>>, slot: u32, index: usize) {
    slots.resize(slots.len().max(slot as usize + 1), None);
    slots[slot as usize] = Some(index);
}
fn identity( attacker: Attacker, index: usize) -> props::Identity {
    // Building's opaque identity names this row's attribution cell. The actual
    // qualified attacker stays in Entry, including captured launch generations.
    match attacker {
        Attacker::Undefined => props::Identity::Undefined,
        Attacker::Null => props::Identity::Null,
        _ => props::Identity::Object(index),
    }
}
// Building.state uses the existing generated names at the foreign boundary.
// Converting these tags does not intern text or allocate a per-target String.
fn state(name: PbStateName) -> props::State {
    match name {
        PbStateName::Built => props::State::Built,
        PbStateName::Destroyed => props::State::Destroyed,
        PbStateName::Unbuilt => props::State::Unbuilt,
    }
}
fn state_name(state: props::State) -> PbStateName {
    match state {
        props::State::Built => PbStateName::Built,
        props::State::Destroyed => PbStateName::Destroyed,
        props::State::Unbuilt => PbStateName::Unbuilt,
    }
}
// Configure Building's existing kernel, not its per-method JS adapter. The
// settings and three model looks remain immutable through recursive damage.

pub fn f3_building_in(world_ctx: &mut crate::world::owner::World) {
    let (row, heap) = world_ctx.sim.calls.args();
    let input = F3BuildingSetup::take(row, heap);
    let index = input.index as usize;
    let all = f3::props_in(&mut world_ctx.units);
    assert_eq!(index, all.entries.len(), "building construction order");
    let mut settings = crate::battle::props::Settings::from(input.settings);
    settings.has_damaged = input.damaged;
    let frame = input.state;
    let bodies: [Option<Ref<Body>>; 3] = input.bodies.try_into().expect("three building bodies");
    // Built, destroyed and unbuilt collision may each receive a round. The
    // owner lookup must accept all three while preserving each exact Ref.
    for body in bodies.into_iter().flatten() { bind(&mut all.body_slots, body.slot, index); }
    match input.target {
        RepairTarget::Building(r) => bind(&mut all.building_slots, r.slot, index),
        RepairTarget::Turret(r) => bind(&mut all.turret_slots, r.slot, index),
        RepairTarget::Vehicle(_) => panic!("building construction target"),
    }
    // heroes.meleeStrike appends only buildings without an AI target after
    // the mixed registry. Keep that stable suffix without a query-time filter.
    if input.member.is_none() { all.extra_melee.push(index); }
    if let Some(member) = input.member { bind(&mut all.member_slots, super::registry::target_id(member), index); }
    all.entries.push(Entry {
        target: input.target,
        member: input.member,
        lock_key: input.key,
        settings,
        owner: f4::adopt(input.live, input.trap, world_ctx.sim.store.vehicles.vehicles.len()),
        geometry_state: state(frame.state),
        team: frame.team,
        target_team: frame.target_team,
        gunner: frame.gunner,
        bodies,
        explosion: input.explosion.map(DefId),
        trap_sphere: input.trap_sphere.map(|b| (v(b.center), b.radius)),
    });
    // TurretTarget and ObjectiveTarget already have their mixed registry ids.
    // Install their geometry from the prepared native model, avoiding a second
    // JS sphere query for every unchanged building during each world tick.
    if let Some(member) = input.member {
        bind(&mut all.member_slots, super::registry::target_id(member), index);
        let entry = &all.entries[index];
        let (center, radius) = entry.trap_sphere.unwrap_or_else(|| entry.frame().sphere(&entry.settings));
        let mut position = Vector3::default();
        position.set_from_matrix_position(&entry.settings.matrix);
        let fact = F3PropFact {
            id: super::registry::target_id(member), alive: entry.frame().alive(), dead: !entry.frame().alive(),
            team: entry.target_team, bounds: Bounds { center: a(center), radius }, health: entry.frame().health,
            category: input.category, kind: input.kind, position: a(position), aim: a(center),
            life: entry.frame().target_generation.unwrap_or(0.0), body: entry.bodies[0], ..Default::default()
        };
        f3::prepare_prop_target_in(&mut world_ctx.units, fact);
    }
}
// heroes.meleeStrike acquires every registry member first, then these
// otherwise-unregistered buildings. The shared targeting kernel still caps
// the complete sequence at ten and visits those candidates in reverse order.
pub(crate) fn extra_melee_count_in(units: &super::owner::Units) -> usize { f3::props_read(units).extra_melee.len() }
pub(crate) fn extra_melee_in(units: &super::owner::Units, index: usize) -> usize { f3::props_read(units).extra_melee[index] }
pub(crate) fn melee_body_in(units: &super::owner::Units, index: usize) -> Option<Ref<Body>> { f3::props_read(units).entries[index].bodies[0] }
pub(crate) fn melee_facts_in(units: &super::owner::Units, index: usize) -> crate::combat::targeting::Facts {
    let entry = &f3::props_read(units).entries[index];
    let mut position = Vector3::default();
    position.set_from_matrix_position(&entry.settings.matrix);
    // Trap.placePad gives it the pad's position too, not the log rig's root.
    // Its melee direction must use the same point as its broadphase sphere.
    if let Some((center, _)) = entry.trap_sphere { position = center; }
    crate::combat::targeting::Facts {
        alive: entry.frame().alive(), seated: false, exposed: false,
        health: entry.frame().health, team: entry.team, category: 4, kind: 0.0, position,
    }
}
pub(crate) fn melee_volume_in(units: &super::owner::Units, index: usize) -> crate::combat::targeting::Volume {
    let entry = &f3::props_read(units).entries[index];
    let (center, radius) = entry.trap_sphere.unwrap_or_else(|| entry.frame().sphere(&entry.settings));
    crate::combat::targeting::Volume { center, radius, hull: None }
}
// LiveObjects.update runs outside this phase. Its next changed row replaces
// only mutable foreign facts; geometry, body names and list order stay put.
pub(crate) fn changed_in(world_ctx: &mut crate::world::owner::World, frame: F3BuildingState) {
    let entry = &mut f3::props_in(&mut world_ctx.units).entries[frame.index as usize];
    entry.put_frame(&props::Building {
        state: state(frame.state), health: frame.health, add_health: frame.add_health,
        last_attacker: identity(frame.attacker, frame.index as usize), target_generation: frame.generation,
    });
    entry.put_attacker(frame.attacker);
    entry.team = frame.team;
    entry.target_team = frame.target_team;
    entry.gunner = frame.gunner;
    if frame.trap_mode { entry.put_trap(frame.trap_state.into()); }
    let body = entry.frame();
    commit_in(world_ctx, frame.index as usize, &body);
}
// The ordered notification retains this touched row at its source point.
// Facade readers keep their existing object, body and model identities.
fn snapshot_in(units: &mut super::owner::Units, index: usize) -> F3BuildingState {
    let entry = &f3::props_in(units).entries[index];
    F3BuildingState {
        index: index as u32, state: state_name(entry.frame().state), health: entry.frame().health,
        add_health: entry.frame().add_health, attacker: entry.attacker(), generation: entry.frame().target_generation,
        team: entry.team, target_team: entry.target_team, gunner: entry.gunner,
        trap_mode: entry.trap().is_some(), trap_state: entry.trap().as_ref().map_or_else(TrState::default, Into::into),
    }
}
// Building.show changes collision visibility before its next callback. Updating
// the existing flag bytes here keeps an immediate nested blast off the old BVH.
fn commit_in(world_ctx: &mut crate::world::owner::World, index: usize, frame: &props::Building) {
    let entry = &mut f3::props_in(&mut world_ctx.units).entries[index];
    entry.put_frame(frame); entry.geometry_state = frame.state;
    if frame.last_attacker == props::Identity::Null { entry.put_attacker(Attacker::Null); }
    // Death does not advance a turret's target generation; a completed repair
    // does. Update the registry's health and life at the same write point.
    let member = entry.member;
    let (center, radius) = entry.trap_sphere.unwrap_or_else(|| frame.sphere(&entry.settings));
    let bodies = entry.bodies;
    let team = target_team(&world_ctx.sim.store, entry);
    if let RepairTarget::Turret(turret) = entry.target {
        crate::vehicles::module::f2::live_seats::turret_life(turret, frame.alive(), entry.attacker(),
            frame.target_generation.unwrap_or(0.0), center, radius);
    }
    if let Some(member) = member {
        f3::prop_target_in(&mut world_ctx.units, member, frame.alive(), frame.health, a(center), radius, frame.target_generation, team);
    }
    let collision = crate::vehicles::module::collision_world().expect("prepared prop collision world");
    for (look, body) in bodies.into_iter().enumerate() {
        let Some(kernel) = body.and_then(|body| collision.kernel_slot(body.slot)) else { continue };
        let world = &world_ctx.collision.state.worlds[f3::collision_world_in(&world_ctx.units)];
        if let Some(order) = world.order.iter().position(|&slot| slot as usize == kernel) {
            world_ctx.movement.flags_mut()[order] = u8::from(look == frame.state.index());
        }
    }
}
// Turret.damage reports the untyped amount even when BuildingScale changed
// health by another amount. A null direction still belongs to a struck packet;
// it differs from a blast, which supplies no hit packet at all.
#[derive(Clone, Copy)]
struct HitInfo {
    direction: Option<Vector3>,
    amount: f64,
    typed: f64,
    generic: f64,
}
struct BuildingWorld<'w, 'a> {
    w: &'w mut crate::world::owner::World,
    index: usize,
    services: WorldServices<'a>,
    hit: Option<HitInfo>,
}
impl BuildingWorld<'_, '_> {
    fn leaf(&mut self, frame: &props::Building, kind: F3PropEffectKind) {
        commit_in(self.w, self.index, frame);
        // Building.show/onDestroyed observe this source-point state. Record
        // it beside recursive deaths and shots; facade delivery cannot lend
        // the authoritative world back to JavaScript mid-tick.
        F3PropEffect {kind,state:snapshot_in(&mut self.w.units, self.index)}.emit(&mut self.w.sim.events);
    }
    // Building.destroyed resumes after any immediate nested explosion. Reload
    // the live row, rather than overwriting what recursive damage left there.
    fn reload(&self, frame: &mut props::Building) {
        *frame = f3::props_read(&self.w.units).entries[self.index].frame();
    }
}
impl props::World for BuildingWorld<'_, '_> {
    fn show(&mut self, frame: &props::Building) { self.leaf(frame, F3PropEffectKind::Show); }
    // Building.destroyed kills its occupant before onDestroyed or show. Crew
    // ownership must join here directly; a JS seat callback would reenter WASM.
    fn crew_destroyed(&mut self, frame: &mut props::Building) {
        commit_in(self.w, self.index, frame);
        if let RepairTarget::Turret(turret) = f3::props_in(&mut self.w.units).entries[self.index].target {
            crate::vehicles::module::f2::live_seats::turret_destroyed(turret, self.services.reborrow());
        }
        self.reload(frame);
    }
    fn destroyed(&mut self, frame: &mut props::Building) {
        // Building.onDestroyed follows crew death and precedes changing looks.
        // The ordered facade notice carries the already completed native death.
        self.leaf(frame, F3PropEffectKind::Destroyed);
        self.reload(frame);
    }
    fn explode(&mut self, frame: &mut props::Building, position: Vector3) {
        commit_in(self.w, self.index, frame);
        if let Some(definition) = f3::props_in(&mut self.w.units).entries[self.index].explosion {
            // Game builds these callbacks with a null explosion owner, even
            // when the building remembers who killed its turret gunner.
            crate::combat::live::blast_in(self.w, definition, a(position), OrdnanceOwner::Null, Some(self.services.reborrow()));
        }
        self.reload(frame);
    }
    // Turret.damage notifies after super.damage completes, including its full
    // destruction chain. Keep that position, target life and attribution for
    // later journal delivery instead of reading a potentially rebuilt object.
    fn damaged(&mut self, frame: &mut props::Building, amount: f64, _: props::Identity, _: props::Identity) {
        commit_in(self.w, self.index, frame);
        let entry = &f3::props_in(&mut self.w.units).entries[self.index];
        let mut position = Vector3::default();
        position.set_from_matrix_position(&entry.settings.matrix);
        let (direction, untyped, typed, generic) = self.hit.map_or((None, amount, 1.0, 1.0), |hit| (hit.direction.map(a), hit.amount, hit.typed, hit.generic));
        // A crew may have boarded or died since the foreign facts arrived.
        // TurretTarget.team is the lease's team at this exact callback slot.
        let team = match entry.target {
            RepairTarget::Turret(turret) => crate::vehicles::module::f2::live_seats::turret_team(turret, entry.team)
                .unwrap_or(entry.target_team),
            _ => entry.target_team,
        };
        F3PropDamage {
            index: self.index as u32, amount, attacker: entry.attacker(),
            // Damage credit samples the occupied target's team; policy uses
            // the building's authored team earlier in the hit.
            has_direction: direction.is_some(), direction: direction.unwrap_or([0.0; 3]), untyped, typed, generic, position: a(position), team, generation: frame.target_generation,
        }.emit(&mut self.w.sim.events);
    }
    fn served(&mut self, _: &props::Station, _: &mut props::Target, _: u8) { unreachable!("foreign station phase"); }
    fn serve_sound(&mut self, _: &props::Station, _: Option<usize>) { unreachable!("foreign station phase"); }
}
// Trap.setHealth shows a newly built pad before clearing sprung and starting
// its reset clock. Publish that intermediate state at the original show point;
// the final publication below then carries the completed reset transition.
impl traps::HealthWorld for BuildingWorld<'_, '_> {
    fn health_show(&mut self, trap: &mut traps::Trap, built: bool) {
        let entry = &mut f3::props_in(&mut self.w.units).entries[self.index];
        entry.put_trap(*trap);
        let mut frame = entry.frame();
        frame.state = if built { props::State::Built } else { props::State::Unbuilt };
        frame.health = trap.health;
        self.leaf(&frame, F3PropEffectKind::Show);
    }
}
/// Native LiveObjects construction and regeneration share combat's owner.
/// A phase reads fixed copies, never an entry borrow spanning a crew callback.
pub(crate) fn count_in(units: &super::owner::Units) -> usize { f3::props_read(units).entries.len() }
pub(crate) fn owner_in(world_ctx: &mut crate::world::owner::World, index: usize) -> F4Handle {
    let handle = &f3::props_in(&mut world_ctx.units).entries[index].owner;
    match handle.kind {
        F4ObjectKind::Trap => f4::live().traps.current(handle),
        _ => f4::live().buildings.current(handle),
    }.expect("current prop owner")
}
/// An idle prop has no world service to call. Check its canonical scalar
/// frame before lending the complete Store around a regeneration callback.
pub(crate) fn needs_regeneration_in(units: &super::owner::Units, index: usize) -> bool {
    let entry = &f3::props_read(units).entries[index];
    if !entry.settings.regenerates { return false; }
    let frame = entry.frame(); frame.add_health > 0.0 && frame.health < entry.settings.max_health
}
pub(crate) fn regenerate_in(world_ctx: &mut crate::world::owner::World, index: usize, dt: f64, services: WorldServices<'_>) {
    let entry = &f3::props_in(&mut world_ctx.units).entries[index];
    let mut frame = entry.frame(); let settings = entry.settings;
    if !settings.regenerates || !(frame.add_health > 0.0 && frame.health < settings.max_health) { return; }
    frame.regenerate(dt, &settings, &mut BuildingWorld { w: world_ctx, index, services, hit: None });
    commit_in(world_ctx, index, &frame);
}

/// Explicit Turret.damage commands retain the same heading and scale sample as
/// native ordnance. Journal delivery follows the completed destruction chain.
pub(crate) fn command_hit_in(world_ctx: &mut crate::world::owner::World, index:usize,value:f64,attacker:Option<Attacker>,hit:Option<F4HitInfo>,services:WorldServices<'_>){
    let hit=hit.map(|h|HitInfo{direction:h.direction.map(|p|Vector3::new(p[0],p[1],p[2])),amount:h.amount,typed:h.typed,generic:h.generic});
    mutate_in(world_ctx, index,value,attacker,hit,services);
}
pub(crate) fn trap_show_in(world_ctx: &mut crate::world::owner::World, index: usize, trap: &mut traps::Trap, built: bool, services: WorldServices<'_>) {
    traps::HealthWorld::health_show(&mut BuildingWorld { w: world_ctx, index, services, hit: None }, trap, built);
}
// Building.damage and setHealth share the existing lifecycle arithmetic. A
// stack frame releases the foreign table before a crew death or explosion can
// recurse; copying these fixed facts allocates neither a table nor a Vec.
fn mutate_in(world_ctx: &mut crate::world::owner::World, index: usize, amount: f64, attacker: Option<Attacker>, hit: Option<HitInfo>, services: WorldServices<'_>) {
    let entry = &mut f3::props_in(&mut world_ctx.units).entries[index];
    let mut frame = entry.frame();
    let mut settings = entry.settings;
    settings.has_crew = match entry.target {
        RepairTarget::Turret(turret) => crate::vehicles::module::f2::live_seats::turret_gunner(turret).is_some(),
        _ => entry.gunner.is_some(),
    };
    // Building.damage rejects a dead target before replacing lastAttacker.
    // A repair has no attacker write, so an old repair lock or credit survives.
    if let Some(attacker) = attacker {
        if !frame.alive() || !(amount > 0.0) { return; }
        entry.put_attacker(attacker);
        frame.last_attacker = identity(attacker, index);
    }
    let trap = entry.trap();
    entry.put_frame(&frame);
    let mut world = BuildingWorld { w: world_ctx, index, services, hit };
    if let Some(mut trap) = trap {
        // Building.damage delegates to Trap.setHealth for this subclass. At
        // zero health it stays built until the next outside trap phase.
        let value = if attacker.is_some() { frame.health - amount } else { amount };
        trap.set_health_with(value, settings.max_health, &mut world);
        f3::props_in(&mut world.w.units).entries[index].put_trap(trap);
        frame.health = trap.health;
        frame.state = if trap.built { props::State::Built } else { props::State::Unbuilt };
    } else if attacker.is_some() {
        frame.damage(amount, props::Identity::Object(index), props::Identity::Null, &settings, &mut world);
    } else {
        frame.set_health(amount, &settings, &mut world);
    }
    // Full rebuild changes the turret life after show returns. Publish that
    // final scalar state too, without resending any other building or Unit.
    world.leaf(&frame, F3PropEffectKind::Publish);
}
struct QueryWorld<'w, 'a> {
    w: &'w mut crate::world::owner::World,
    ai:Option<&'a mut dyn crate::ai::owned::callbacks::Callbacks>,
    recon: &'a mut crate::recon::Authority,
    owner: OrdnanceOwner,
}
impl live::World for QueryWorld<'_, '_> {
    // LiveObjects.repairs tests health type and flags before asking for a
    // sphere. A destroyed objective is never rebuildable; an ordinary wreck is.
    fn target(&mut self, index: usize) -> live::Target {
        let buildings = f3::props_in(&mut self.w.units).entries.len();
        if index >= buildings {
            let r = super::f3_hulls::reference_in(&self.w.sim, index - buildings);
            let v = self.w.sim.store.vehicle.get(r).expect("repair hull");
            let mut matrix = three::Matrix4::default(); matrix.elements = v.matrix;
            return live::Target { alive:v.alive, state:None,
                health_type:super::f3_hulls::repairable_in(&self.w.units, r).then_some(live::Health::Vehicle),
                health:v.health, max_health:v.max_health, team:v.team.unwrap_or(0.0),
                body:Some(v.body.slot as usize), gunner:false, position:Vector3::new(v.position[0],v.position[1],v.position[2]), matrix };
        }
        let entry = &f3::props_in(&mut self.w.units).entries[index];
        let frame = entry.frame();
        let health_type = if entry.settings.kind == props::Kind::Objective && !frame.alive() { None } else {
            Some(match frame.state { props::State::Built => live::Health::Building,
                props::State::Destroyed => live::Health::Rebuild, props::State::Unbuilt => live::Health::Build })
        };
        let mut position = Vector3::default();
        position.set_from_matrix_position(&entry.settings.matrix);
        live::Target {
            alive: frame.alive(), state: Some(frame.state), health_type, health: frame.health,
            max_health: entry.settings.max_health, team: entry.team,
            body: entry.bodies[frame.state.index()].map(|b| b.slot as usize),
            gunner: match entry.target { RepairTarget::Turret(r) => crate::vehicles::module::f2::live_seats::turret_gunner(r).is_some(), _ => false },
            position, matrix: entry.settings.matrix,
        }
    }
    // Building.blastSphere prefers a model sphere in every state, whereas
    // repair bounds use it only on an unbuilt pad. Both keep the unscaled
    // radius even when the placement matrix scales the model.
    fn sphere(&mut self, index: usize, blast: bool, _: Vector3) -> (Vector3, f64) {
        let buildings = f3::props_in(&mut self.w.units).entries.len();
        if index >= buildings { return super::f3_hulls::sphere_in(self.w, super::f3_hulls::reference_in(&self.w.sim, index - buildings), blast); }
        let entry = &f3::props_in(&mut self.w.units).entries[index];
        if let Some(sphere) = entry.trap_sphere { return sphere; }
        if blast { entry.frame().blast_sphere(&entry.settings) } else { entry.frame().sphere(&entry.settings) }
    }
    // blastBlocked ignores the victim's current collision look. Repair LOS
    // instead accepts a hit owned by the candidate itself. The native world
    // ray retains that body identity, including a different collision look.
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, ignore: Option<usize>, static_only: bool, _: live::Out) -> Option<live::Hit> {
        let ignored = ignore.and_then(|slot| f3::props_in(&mut self.w.units).body_slots.get(slot).copied().flatten())
            .and_then(|i| f3::props_in(&mut self.w.units).entries[i].bodies.into_iter().flatten().find(|r| Some(r.slot as usize) == ignore));
        super::native::ordnance_cast_in(self.w, f3::collision_world_in(&self.w.units), a(origin), a(direction), far, ignored, static_only, false).map(|hit| {
            let owner = hit.body.and_then(|b| f3::props_in(&mut self.w.units).body_slots.get(b.slot as usize).copied().flatten()
                .or_else(|| super::f3_hulls::body_index_in(self.w, b).map(|i| f3::props_in(&mut self.w.units).entries.len() + i)));
            live::Hit { distance: hit.distance, owner }
        })
    }
    // Game.teamDamage observes the building's team, not a turret occupant's
    // team from its AI target. Reevaluate it for each recursively reached row.
    fn team_scale(&mut self, _: &mut live::Queries, index: usize) -> f64 {
        let (team, human, _) = crate::combat::live::owner_facts_in(&self.w.sim, &mut self.w.units, self.owner);
        let buildings = f3::props_in(&mut self.w.units).entries.len();
        let target_team = if index < buildings { f3::props_in(&mut self.w.units).entries[index].team }
            else { self.w.sim.store.vehicle.get(super::f3_hulls::reference_in(&self.w.sim, index - buildings)).expect("repair hull").team.unwrap_or(0.0) };
        crate::combat::impacts::team_damage(team, human, target_team, false, f3::friendly_fire_in(&self.w.units))
    }
    fn damage(&mut self, queries: &mut live::Queries, index: usize, amount: f64, _: props::Identity, info: Option<live::Damage>) {
        let buildings = f3::props_in(&mut self.w.units).entries.len();
        if index >= buildings {
            super::f3_hulls::mutate_in(self.w, super::f3_hulls::reference_in(&self.w.sim, index - buildings), amount, Some(self.owner), WorldServices { ai:self.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks),recon:self.recon, queries });
            return;
        }
        let (_, _, attacker) = crate::combat::live::owner_facts_in(&self.w.sim, &mut self.w.units, self.owner);
        let hit = info.map(|i| HitInfo { direction: i.direction, amount: i.amount, typed: i.typed, generic: i.generic.num() });
        mutate_in(self.w, index, amount, Some(attacker), hit, WorldServices { ai:self.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks),recon: self.recon, queries });
    }
    fn set_health(&mut self, queries: &mut live::Queries, index: usize, value: Field) {
        let buildings = f3::props_in(&mut self.w.units).entries.len();
        if index >= buildings {
            super::f3_hulls::mutate_in(self.w, super::f3_hulls::reference_in(&self.w.sim, index - buildings), value.num(), None, WorldServices { ai:self.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks),recon:self.recon, queries });
            return;
        }
        mutate_in(self.w, index, value.num(), None, None, WorldServices { ai:self.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks),recon: self.recon, queries });
    }
    fn may_enter(&mut self, _: &mut live::Queries, _: usize) -> bool { unreachable!("native seat admission service"); }
}
// LiveObjects.lockOn keeps the old current target first, then construction
// order. Its strict score comparison preserves equal-distance ties.
pub(crate) fn lock_on_in(world_ctx: &mut crate::world::owner::World, eye: Vector3, direction: Vector3, definition: &live::Definition, current: Option<RepairTarget>, held: bool, team: f64, services: WorldServices<'_>) -> Option<RepairTarget> {
    let current = current.and_then(|r| f3::props_in(&mut world_ctx.units).index(&world_ctx.sim.store, r));
    let count = f3::props_in(&mut world_ctx.units).entries.len() + super::f3_hulls::count_in(&world_ctx.sim);
    let mut world = QueryWorld { w: world_ctx, ai:services.ai,recon: services.recon, owner: OrdnanceOwner::Null };
    // LiveObjects.lockOn visits buildings before the fleet, keeping a held
    // current target first even when it comes from the later hull sequence.

    let picked = services.queries.lock_on_ordered(0..count, eye, direction, definition, current, held, team, true, &mut world);
    picked.map(|i| if i < f3::props_in(&mut world_ctx.units).entries.len() { f3::props_in(&mut world_ctx.units).entries[i].target }
        else { RepairTarget::Vehicle(super::f3_hulls::reference_in(&world_ctx.sim, i - f3::props_in(&mut world_ctx.units).entries.len())) })
}
// repairLockKey deliberately omits health state and gameplay generation.
// Its construction-time string survives damage and rebuilding of that object.
pub(crate) fn lock_key_in(world_ctx: &mut crate::world::owner::World, target: RepairTarget) -> StrId {
    if let RepairTarget::Vehicle(r) = target { return super::f3_hulls::lock_key_in(&world_ctx.units, r); }
    let index = f3::props_in(&mut world_ctx.units).index(&world_ctx.sim.store, target).expect("prepared repair target");
    f3::props_in(&mut world_ctx.units).entries[index].lock_key
}
// LiveObjects.repair rechecks eligibility when the tick fires. A preceding
// repair or nested explosion may have changed the earlier acquisition result.
pub(crate) fn repair_in(world_ctx: &mut crate::world::owner::World, target: Option<RepairTarget>, definition: &live::Definition, team: f64, services: WorldServices<'_>) -> bool {
    let target = target.and_then(|r| f3::props_in(&mut world_ctx.units).index(&world_ctx.sim.store, r));
    let mut world = QueryWorld { w: world_ctx, ai:services.ai,recon: services.recon, owner: OrdnanceOwner::Null };
    services.queries.repair(target, definition, team, &mut world)
}
// LiveObjects.blast reads the live construction sequence after every nested
// death. Only the fixed numeric BuildingScale fields are needed by this layer.
pub(crate) fn blast_in(world_ctx: &mut crate::world::owner::World, packet: BlastPacket, position: Vector3, owner: OrdnanceOwner, services: WorldServices<'_>) {
    let definition = ExplosionDef {
        damage: Field::Num(packet.hit.damage), damage_radius_inner: Field::Num(packet.inner),
        damage_radius_outer: Field::Num(packet.outer), scales: packet.hit.scales.map(Field::Num),
    };
    let mut world = QueryWorld { w: world_ctx, ai:services.ai,recon: services.recon, owner };
    let mut index = 0;
    while index < f3::props_in(&mut world.w.units).entries.len() {
        let target = index;
        services.queries.blast(&[target], &definition, position, props::Identity::Null, true, true, &mut world);
        index += 1;
    }
}
// Game.onHit visits props before hulls. A body with no building owner is a
// no-op here; the coordinator's F2 service still receives the same body next.
pub(crate) fn struck_in(world_ctx: &mut crate::world::owner::World, body: Ref<Body>, packet: HitPacket, owner: OrdnanceOwner, direction: Option<Vector3>, services: WorldServices<'_>) {
    let Some(index) = f3::props_in(&mut world_ctx.units).body_slots.get(body.slot as usize).copied().flatten() else { return };
    if !f3::props_in(&mut world_ctx.units).entries[index].bodies.contains(&Some(body)) { return; }
    let (team, human, attacker) = crate::combat::live::owner_facts_in(&world_ctx.sim, &mut world_ctx.units, owner);
    let scale = crate::combat::impacts::team_damage(team, human, f3::props_in(&mut world_ctx.units).entries[index].team, false, f3::friendly_fire_in(&world_ctx.units));
    let typed = packet.scales[HealthScale::Building as usize];
    let hit = Some(HitInfo { direction, amount: packet.damage * scale, typed, generic: packet.shield });
    mutate_in(world_ctx, index, packet.damage * typed * scale, Some(attacker), hit, services);
}
// OrdnanceWorld.hit uses a following beam's current damage override while
// retaining its immutable health and shield factors from the prepared class.
pub(crate) fn hit_policy_in(world_ctx: &mut crate::world::owner::World, definition: DefArg) -> HitPacket {
    let (definition, damage) = match definition {
        DefArg::Ordnance(definition) => (definition, None),
        DefArg::Beam(r) => {
            let beam = world_ctx.sim.store.beam.get(r).expect("live beam hit");
            (beam.def, beam.damage)
        }
    };
    let mut packet = f3::hit_policy_in(&world_ctx.units, definition);
    if let Some(damage) = damage { packet.damage = damage; }
    packet
}
fn a(value: Vector3) -> [f64; 3] { [value.x, value.y, value.z] }
fn v(value: [f64; 3]) -> Vector3 { Vector3::new(value[0], value[1], value[2]) }

/// Placed-post readers need only the canonical health and captured attacker.
pub(crate) fn battle_building_in(world_ctx: &mut crate::world::owner::World, r: Ref<Building>) -> (bool, Attacker) {
    let p = f3::props_in(&mut world_ctx.units); let index = p.index(&world_ctx.sim.store, RepairTarget::Building(r)).expect("prepared post building");
    (p.entries[index].frame().alive(),p.entries[index].attacker())
}

fn target_team(s: &Store, entry: &Entry) -> f64 {
    match entry.target {
        RepairTarget::Turret(r) => crate::vehicles::module::f2::live_seats::battle_team(s,r).unwrap_or(entry.team),
        _ => entry.team,
    }
}
/// Registry facts refresh from canonical owners at the read, including a
/// lease acquired earlier in this same native phase. Geometry stays prepared.
pub(crate) fn refresh_in(world_ctx: &mut crate::world::owner::World, id: u32) {
    let previous = f3::prepared_target_in(&world_ctx.units, id).map(|fact| (fact.alive, fact.health.to_bits(), fact.life.to_bits(), fact.team.to_bits()));
    let all = f3::props_in(&mut world_ctx.units);
    let Some(index) = all.member_slots.get(id as usize).copied().flatten() else {return};
    let entry = &mut all.entries[index]; let frame = entry.frame();
    let team = target_team(&world_ctx.sim.store, entry);
    // Damage/regeneration already commit these same canonical facts. Most AI
    // reads need neither another sphere transform nor another registry write.
    if entry.geometry_state == frame.state && previous == Some((frame.alive(), frame.health.to_bits(), frame.target_generation.unwrap_or(0.0).to_bits(), team.to_bits())) { return; }
    let (center,radius) = entry.trap_sphere.unwrap_or_else(|| frame.sphere(&entry.settings));
    entry.geometry_state = frame.state;
    let member = entry.member.expect("registered prop");
    f3::prop_target_in(&mut world_ctx.units, member,frame.alive(),frame.health,a(center),radius,
        frame.target_generation,team);
}
pub(crate) fn battle_member_in(world_ctx: &mut crate::world::owner::World, s: &Store, key: Member) -> Option<crate::battle::world::Member> {
    let all = f3::props_in(&mut world_ctx.units);
    let index = all.member_slots.get(super::registry::target_id(key) as usize).copied().flatten()?;
    let entry = &all.entries[index]; if entry.member != Some(key) {return None;}
    let mut position = Vector3::default(); position.set_from_matrix_position(&entry.settings.matrix);
    Some(crate::battle::world::Member {alive:entry.frame().alive(),team:target_team(s,entry),position:a(position),
        ..crate::battle::world::Member::default()})
}

// facade shims: retained for unconverted callers.
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_building(index: usize) -> Option<crate::wire::store_world::Building>{ wire_building_in(crate::abi::world(), index) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn member(target: RepairTarget) -> Option<Member>{ member_in(crate::abi::world(), target) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn matches_binding(target:RepairTarget,member:Option<Member>,owner:&F4Handle)->bool{ matches_binding_in(crate::abi::world(), target, member, owner) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn built_body(target:RepairTarget)->Option<Ref<Body>>{ built_body_in(crate::abi::world(), target) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn trap_geometry(target:RepairTarget)->Option<(crate::ai::combat::world::Sphere,three::Matrix4)>{ trap_geometry_in(crate::abi::world(), target) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_turret(reference: Ref<Turret>) -> Option<(usize, Member, props::Building, f64)>{ wire_turret_in(crate::abi::world(), reference) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn f3_building(){ f3_building_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn count() -> usize{ count_in(&crate::abi::world().units) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn owner(index: usize) -> F4Handle{ owner_in(crate::abi::world(), index) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn needs_regeneration(index: usize) -> bool{ needs_regeneration_in(&crate::abi::world().units, index) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn regenerate(index: usize, dt: f64, services: WorldServices<'_>){ regenerate_in(crate::abi::world(), index, dt, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn command_hit(index:usize,value:f64,attacker:Option<Attacker>,hit:Option<F4HitInfo>,services:WorldServices<'_>){ command_hit_in(crate::abi::world(), index, value, attacker, hit, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn trap_show(index: usize, trap: &mut traps::Trap, built: bool, services: WorldServices<'_>){ trap_show_in(crate::abi::world(), index, trap, built, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn lock_key(target: RepairTarget) -> StrId{ lock_key_in(crate::abi::world(), target) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn battle_building(r: Ref<Building>) -> (bool, Attacker){ battle_building_in(crate::abi::world(), r) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn battle_member(s: &Store, key: Member) -> Option<crate::battle::world::Member>{ battle_member_in(crate::abi::world(), s, key) }
