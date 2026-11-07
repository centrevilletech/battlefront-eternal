//! Vehicles.struck/blast and Vehicle.damage share the fleet's canonical hulls.
//! Only immutable class geometry and numeric explosion bindings live here.
//! A recursive hit takes a reserved working row, commits before crew death,
//! and reloads the live result before its outer damage operation continues.
use super::f3::{self, WorldServices};
use crate::combat::impacts::{BlastPacket, HitPacket};
use crate::gen::*;
use crate::store::{Ref, StrId};
use crate::vehicles::{damage::HullDamageWorld, module::{self as fleet, f2}, Hull};
use three::{Matrix4, Vector3};

// Vehicle construction binds both the collision owner and its mixed-registry
// member. Lookups use those numeric slots; no class names enter a hit loop.
#[derive(Default)]
pub(super) struct Hulls {
    rows: Vec<Option<F3HullSetup>>,
    members: Vec<Option<Ref<Vehicle>>>,
    bodies: Vec<Option<Ref<Vehicle>>>,
    serial: u32,
    keys: Vec<StrId>,
    fact_versions: Vec<Option<u64>>,
}
fn binding(hulls: &Hulls, r: Ref<Vehicle>) -> &F3HullSetup {
    hulls.rows[r.slot as usize].as_ref().filter(|row| row.hull == r).expect("prepared hull combat")
}
// Vehicles.variant supplies this row only after the ordinary fleet adopted
// the object. Its member is already in Units' shared insertion sequence.
// Body and member reverse indices grow here, never during a blast traversal.

pub fn f3_hull_in(world_ctx: &mut crate::world::owner::World) {
    let (row, heap) = world_ctx.sim.calls.args();
    let input = F3HullSetup::take(row, heap);
    let body = world_ctx.sim.store.vehicle.get(input.hull).expect("adopted fleet hull").body;
    let id = input.member.map(super::registry::target_id).map(|id| id as usize);
    let all = world_ctx.units.hulls.get_or_insert_with(Hulls::default);
    all.rows.resize_with(all.rows.len().max(input.hull.slot as usize + 1), || None);
    all.fact_versions.resize(all.rows.len(), None);
    all.fact_versions[input.hull.slot as usize] = None;
    if let Some(id) = id { all.members.resize(all.members.len().max(id + 1), None); }
    all.bodies.resize(all.bodies.len().max(body.slot as usize + 1), None);
    if let Some(id) = id { all.members[id] = Some(input.hull); }
    all.bodies[body.slot as usize] = Some(input.hull);
    let slot = input.hull.slot as usize;
    all.rows[slot] = Some(input);
    // Traps may have been prepared before this variant existed. Fleet birth
    // reserves their victim snapshots too, rather than growing them on crush.
    crate::battle::f4::live().prepare_hulls(all.rows.len());
    // The first native sensor may run before the first fleet update.
    if let Some(id) = id { refresh_in(world_ctx, id as u32); }
    let all = world_ctx.units.hulls.get_or_insert_with(Hulls::default);
    let count = all.rows.iter().flatten().count();
    while all.keys.len() < count {
        all.keys.push(world_ctx.sim.strings.intern_str(&format!("v{}",all.keys.len())));
    }
    f3::reserve_registry_in(world_ctx);
}

// CollisionWorld assigns polygon ranges by construction order, independently
// of private kernel slots. Prepared hulls consume their range only on birth.
pub(crate) fn body_serial_in(units: &mut super::owner::Units, serial: u32) { units.hulls.get_or_insert_with(Hulls::default).serial = serial; }
pub(crate) fn prepared_count_in(units: &super::owner::Units) -> usize { units.hulls.as_ref().expect("prepared unit service").rows.iter().flatten().filter(|r| r.member.is_none()).count() }
pub(crate) fn reserve_members_in(units: &mut super::owner::Units, count: usize) {
    let hulls = units.hulls.get_or_insert_with(Hulls::default);
    hulls.members.resize(hulls.members.len().max(count), None);
}

// Vehicles.variant appends one mixed target at the reached spawn slot. This
// keeps equal-distance ties and ids unchanged when a soldier was added first.
pub(crate) fn activate_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, index: usize) {
    let id = world_ctx.sim.store.units.add(|id| Member::VehicleTarget(Ref::new(id,0)));
    let member = Member::VehicleTarget(Ref::new(id,0));
    let all = world_ctx.units.hulls.get_or_insert_with(Hulls::default);
    let row = all.rows[r.slot as usize].as_mut().expect("prepared hull combat");
    assert!(row.member.is_none());
    row.member = Some(member); row.key = all.keys[index];
    all.members[id as usize] = Some(r);
    all.serial += 1;
    let serial = all.serial;
    let v = world_ctx.sim.store.vehicle.get_mut(r).unwrap();
    v.target_id = id as f64;
    let body = v.body;
    let collision = fleet::collision_world().expect("prepared fleet collision");
    let state = crate::collision::state();
    let kernel = collision.kernel_slot(body.slot).expect("prepared hull body");
    let world = &mut state.worlds[collision.world];
    let at = world.prepared.iter().position(|&slot| slot as usize == kernel).expect("inactive body");
    world.prepared.remove(at);
    world.order.push(kernel as u32); world.version += 1;
    state.bodies[kernel].offset = serial as f64 * 0x1000000 as f64;
    collision.enable_body(state,body.slot,false).expect("activated body place");
    F3HullBirth {hull:r,member_id:id,index:index as u32,body_serial:serial}.emit(&mut world_ctx.sim.events);
    refresh_in(world_ctx, id);
}

/// VehicleTarget.team reads the first occupant before the hull's base team.
/// This target team differs from the team Vehicles.blast uses for damage.
fn target_team_in(sim: &crate::abi::Sim, v: &Vehicle) -> f64 {
    for seat in &v.seats {
        if let Some(lease) = seat.lease.and_then(|r| sim.store.seat_lease.get(r)) {
            return lease.team;
        }
    }
    v.team.unwrap_or(0.0)
}
/// VehicleTarget.sphere/aimPoint use the current physics matrix. Refresh only
/// when a native reader asks, so late seat changes cannot leave stale teams.
/// F2's revision changes at physics stores and seat/damage commands. Repeated
/// facts/aim/LOS reads of that same revision share its prepared target row.
pub(crate) fn refresh_in(world_ctx: &mut crate::world::owner::World, id: u32) {
    let Some(r) = world_ctx.units.hulls.get_or_insert_with(Hulls::default).members.get(id as usize).copied().flatten() else { return };
    let revision = f2::revision(r);
    if world_ctx.units.hulls.get_or_insert_with(Hulls::default).fact_versions[r.slot as usize] == Some(revision) { return; }
    let Some(v) = world_ctx.sim.store.vehicle.get(r) else { return };
    let row = binding(world_ctx.units.hulls.get_or_insert_with(Hulls::default), r);
    let mut matrix = Matrix4::default();
    matrix.elements = v.matrix;
    let mut center = vec(v.center);
    center.apply_matrix4(&matrix);
    // lockSpan uses the collision box, whereas broadphase queries use the
    // bound radius. Neither reader needs a JS root or a posed scene node.
    let fact = F3PropFact {
        id, alive: v.alive, dead: !v.alive, team: target_team_in(&world_ctx.sim, v), health: v.health,
        category: row.category, kind: row.kind, position: v.position, velocity: v.velocity,
        bounds: Bounds { center: array(center), radius: v.bound_radius }, aim: array(center),
        life: v.target_generation, body: Some(v.body),
        r#box: Some(F3TargetBox { min: v.bounds_min, max: v.bounds_max, matrix: v.matrix }),
        ..Default::default()
    };
    f3::prepare_prop_target_in(&mut world_ctx.units, fact);
    world_ctx.units.hulls.get_or_insert_with(Hulls::default).fact_versions[r.slot as usize] = Some(revision);
}
// VehicleTarget keeps its native hull identity independently of registry id.
// Only a matching prepared member can resolve through this reverse index.
pub(crate) fn matches_binding_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Vehicle>,member:Option<Member>)->bool {
    world_ctx.units.hulls.get_or_insert_with(Hulls::default).rows.get(reference.slot as usize).and_then(Option::as_ref)
        .is_some_and(|row|row.hull==reference&&row.member==member&&world_ctx.sim.store.vehicle.contains(reference))
}
pub(crate) fn member_in(units: &super::owner::Units, r: Ref<Vehicle>) -> Member { binding(units.hulls.as_ref().expect("prepared unit service"), r).member.expect("active hull member") }
pub(crate) fn for_member_in(world_ctx: &mut crate::world::owner::World, member: Member) -> Option<Ref<Vehicle>> {
    let r = world_ctx.units.hulls.get_or_insert_with(Hulls::default).members.get(super::registry::target_id(member) as usize).copied().flatten()?;
    let row = world_ctx.units.hulls.get_or_insert_with(Hulls::default).rows.get(r.slot as usize)?.as_ref().filter(|row|row.hull==r)?;
    (row.member == Some(member) && world_ctx.sim.store.vehicle.contains(r)).then_some(r)
}
/// Capture awards need current hull facts while the tail lends Store to its
/// native services. Validate the retained reverse index before reading it.
pub(crate) fn battle_member_in(units: &mut super::owner::Units, s: &Store, key: Member) -> Option<crate::battle::world::Member> {
    let r = units.hulls.get_or_insert_with(Hulls::default).members.get(super::registry::target_id(key) as usize).copied().flatten()?;
    let row = units.hulls.get_or_insert_with(Hulls::default).rows.get(r.slot as usize)?.as_ref().filter(|row|row.hull==r)?;
    if row.member != Some(key) { return None; }
    let v = s.vehicle.get(r)?;
    Some(crate::battle::world::Member { alive: v.alive, position: v.position, ..Default::default() })

}
// repairLockKey is an object's construction index, independent of its life.
pub(crate) fn lock_key_in(units: &super::owner::Units, r: Ref<Vehicle>) -> StrId { binding(units.hulls.as_ref().expect("prepared unit service"), r).key }
pub(crate) fn count_in(sim: &crate::abi::Sim) -> usize { sim.store.vehicles.vehicles.len() }
pub(crate) fn reference_in(sim: &crate::abi::Sim, index: usize) -> Ref<Vehicle> { sim.store.vehicles.vehicles[index] }
pub(crate) fn repairable_in(units: &super::owner::Units, r: Ref<Vehicle>) -> bool { binding(units.hulls.as_ref().expect("prepared unit service"), r).health == 3 }
pub(crate) fn health_scale_in(units: &super::owner::Units, r: Ref<Vehicle>) -> u32 { binding(units.hulls.as_ref().expect("prepared unit service"), r).health }
pub(crate) fn body_index_in(world_ctx: &mut crate::world::owner::World, body: Ref<Body>) -> Option<usize> {
    let r = world_ctx.units.hulls.get_or_insert_with(Hulls::default).bodies.get(body.slot as usize).copied().flatten()?;
    if !world_ctx.sim.store.vehicle.get(r).is_some_and(|v| v.body == body) { return None; }
    world_ctx.sim.store.vehicles.vehicles.iter().position(|&v| v == r)
}

/// OrdnanceWorld.riderMount resolves the collision owner without a facade.
/// Only an occupied animal exposes riders through its hull's collision shape.
pub(crate) fn rider_mount_in(world_ctx: &mut crate::world::owner::World, body: Ref<Body>) -> Option<crate::ordnance::world::RiderMount> {
    let r=world_ctx.units.hulls.get_or_insert_with(Hulls::default).bodies.get(body.slot as usize).copied().flatten()?;
    let hull=world_ctx.sim.store.vehicle.get(r)?;
    if hull.body!=body || !f2::live_seats::animal(r) ||
        !hull.seats.iter().any(|seat|seat.lease.is_some()) {return None;}
    Some(crate::ordnance::world::RiderMount {vehicle:r,bound_radius:hull.bound_radius,body:crate::store::Nullable::Value(body)})
}

/// Vehicle.damage/setHealth can be entered by hits, repair and native physics.
/// A working row comes from F2's retained pool rather than cloning its class.
pub(crate) fn mutate_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, value: f64, attacker: Option<OrdnanceOwner>, services: WorldServices<'_>) {
    let index = world_ctx.sim.store.vehicles.vehicles.iter().position(|&v| v == r).expect("ordered fleet hull");
    let mut hull = f2::take_hull(r, index);
    // The retained spare may last have served a ground-only read. Preserve
    // flight state too before the common store writes this incoming hit.
    if hull.flying { fleet::read_flight(&mut hull); }
    let mut world = HullDamage::new(world_ctx, r, services);
    if let Some(attacker) = attacker { hull.damage(&mut world, value, attacker); }
    else { hull.set_health(&mut world, value); }
    // The solver can reload after a nested death. Store that final result,
    // not the row sampled before its onDamaged or crew hooks.
    fleet::store_hull(&hull);
    f2::recycle(hull);
}
/// Vehicles.struck accepts only the actual hull collision owner. A stale body
/// reference must not redirect a projectile to a recycled vehicle slot.
pub(crate) fn struck_in(world_ctx: &mut crate::world::owner::World, body: Ref<Body>, packet: HitPacket, owner: OrdnanceOwner, services: WorldServices<'_>) {
    let Some(r) = world_ctx.units.hulls.get_or_insert_with(Hulls::default).bodies.get(body.slot as usize).copied().flatten() else { return };
    let Some(v) = world_ctx.sim.store.vehicle.get(r).filter(|v| v.body == body) else { return };
    // Vehicles.struck multiplies the authored health scale before friendly
    // fire. Keep that order even when either factor happens to be zero.
    let typed = packet.scales[binding(world_ctx.units.hulls.get_or_insert_with(Hulls::default), r).health as usize];
    let (team, human, _) = crate::combat::live::owner_facts_in(&world_ctx.sim, &mut world_ctx.units, owner);
    let scale = crate::combat::impacts::team_damage(team, human, v.team.unwrap_or(0.0), false, f3::friendly_fire_in(&world_ctx.units));
    mutate_in(world_ctx, r, packet.damage * typed * scale, Some(owner), services);
}
/// Vehicle.blastSphere keeps the authored model sphere, widened by the box.
/// Its center follows the current matrix, including a destroyed walker's lean.
pub(crate) fn sphere_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, blast: bool) -> (Vector3, f64) {
    let v = world_ctx.sim.store.vehicle.get(r).expect("fleet sphere");
    let row = binding(world_ctx.units.hulls.get_or_insert_with(Hulls::default), r);
    let mut matrix = Matrix4::default();
    matrix.elements = v.matrix;
    let mut center = vec(if blast { row.blast_center } else { v.center });
    center.apply_matrix4(&matrix);
    (center, if blast { row.blast_radius } else { v8math::max(v.size[0], v.size[2]) * 0.5 })
}
/// Vehicles.blast revisits the live fleet length after each immediate death.
/// A hull shields neither itself nor another hull: LOS tests statics only.
pub(crate) fn blast_in(world_ctx: &mut crate::world::owner::World, packet: BlastPacket, at: Vector3, owner: OrdnanceOwner, mut services: WorldServices<'_>) {
    let mut index = 0;
    while index < count_in(&world_ctx.sim) {
        let r = reference_in(&world_ctx.sim, index);
        index += 1;
        let v = world_ctx.sim.store.vehicle.get(r).expect("blast hull");
        if !v.alive { continue; }
        let (body, target_team) = (v.body, v.team.unwrap_or(0.0));
        let (center, radius) = sphere_in(world_ctx, r, true);
        // blastDistance deliberately subtracts half the radius, without a
        // clamp at zero. A source inside the hull keeps that negative value.
        let distance = crate::ordnance::blast_distance(array(at), array(center), radius);
        let typed = packet.hit.scales[binding(world_ctx.units.hulls.get_or_insert_with(Hulls::default), r).health as usize];
        let damage = packet.hit.damage * crate::ordnance::defs::falloff(distance, packet.inner, packet.outer) * typed;
        if !(damage > 0.0) { continue; }
        let mut sight = center;
        sight.sub(&at);
        let span = sight.length();
        if !(span < 1e-3) {
            sight.divide_scalar(span);
            let hit = super::native::ordnance_cast_in(world_ctx, f3::collision_world_in(&world_ctx.units), array(at), array(sight), span, Some(body), true, false);
            if hit.is_some_and(|hit| hit.distance < span - 0.05) { continue; }
        }
        let (team, human, _) = crate::combat::live::owner_facts_in(&world_ctx.sim, &mut world_ctx.units, owner);
        // An earlier death can change an owner's current team. Sample it
        // per reached hull, as Game.teamDamage does in the live JS loop.
        let scale = crate::combat::impacts::team_damage(team, human, target_team, false, f3::friendly_fire_in(&world_ctx.units));
        mutate_in(world_ctx, r, damage * scale, Some(owner), services.reborrow());
    }
}

/// Vehicle's damage hooks preserve the original commit/resume slots. Crew
/// deaths and class explosions use the caller's existing world-service borrow;
/// the only synchronous foreign leaf updates Character achievement credit.
pub(crate) struct HullDamage<'w, 'a> {
    w: &'w mut crate::world::owner::World,
    pub services: WorldServices<'a>,
    pub hull: Ref<Vehicle>,
}
impl<'w, 'a> HullDamage<'w, 'a> {
    pub(crate) fn new(w: &'w mut crate::world::owner::World, hull: Ref<Vehicle>, services: WorldServices<'a>) -> Self { Self { w, hull, services } }
}
impl HullDamageWorld for HullDamage<'_, '_> {
    fn commit_hull(&mut self, hull: &Hull) { fleet::store_hull(hull); }
    fn resume_hull(&mut self, hull: &mut Hull) {
        // A nested crew death may have exploded back through this hull.
        // Its committed canonical row takes precedence over our old copy.
        let v = self.w.sim.store.vehicle.get(hull.id).expect("damaged hull");
        hull.load(v);
        if hull.flying { fleet::read_flight(hull); }
    }
    fn damaged_hull(&mut self, index: usize, amount: f64, attacker: &OrdnanceOwner) {
        // Vehicle.onDamaged is a notice. Its sampled values stay before the
        // later destruction and class-blast records in the shared stream.
        notice_in(self.w, reference_in(&self.w.sim, index), F3HullEvent::Damaged, amount, *attacker);
    }
    fn set_damage_body(&mut self, body: u32, enabled: bool) {
        fleet::collision_world().expect("fleet collision").enable_body(crate::collision::state(), body, enabled).expect("hull collision body");
    }
    fn start_respawn(&mut self, index: usize) { f2::respawn_clock(index); }
    fn destroy_hull_crew(&mut self, index: usize) { f2::live_seats::destroyed_with(index, Some(self.services.reborrow())); }
    fn destroyed_hull(&mut self, index: usize) {
        let r = reference_in(&self.w.sim, index);
        let owner = self.w.sim.store.vehicle.get(r).expect("destroyed hull").last_attacker;
        let (_, _, attacker) = crate::combat::live::owner_facts_in(&self.w.sim, &mut self.w.units, owner);
        let character = match attacker {
            Attacker::Unit(r) => self.w.sim.store.unit.get(r).and_then(|u| u.character),
            Attacker::Player => self.w.sim.store.unit.get(f3::viewpoint_in(&self.w.units)).and_then(|u| u.character),
            Attacker::OwnerSnapshot(r) => self.w.sim.store.owner_snapshot.get(r).and_then(|o| o.character),
            _ => None,
        };
        if crate::battle::f4_core::active() {
            crate::battle::f4_core::achievement(character, F4CommandOp::VehicleDestroyed);
        } else if let Some(character) = character {
            // Game.vehicleDestroyed updates an achievement immediately.
            // It sees committed mirrors but cannot call damage or a solver.
            f3::foreign_barrier_in(self.w);
            import_f3_hull_credit(&F3HullCredit { character });
        }
    }
    fn release_hull_crew(&mut self, index: usize) { f2::live_seats::release_with(index, Some(self.services.reborrow())); }
    fn explode_hull(&mut self, index: usize, final_explosion: bool, at: &Vector3) {
        let r = reference_in(&self.w.sim, index);
        let row = binding(self.w.units.hulls.get_or_insert_with(Hulls::default), r);
        let definition = if final_explosion { row.final_explosion } else { row.explosion };
        if let Some(definition) = definition {
            // Wreck blasts credit the hull object, independently of the
            // attacker credited for destroying it or killing its crew.
            crate::combat::live::blast_in(self.w, crate::store::DefId(definition), array(*at), OrdnanceOwner::Vehicle(r), Some(self.services.reborrow()));
        }
    }
    fn hull_effect(&mut self, name: &str, at: &Vector3) {
        // Vehicle.setHealth has one fallback effect when no class blast exists.
        assert_eq!(name, "explosion");
        let v = self.w.sim.store.vehicle.get(self.hull).expect("hull effect");
        F3HullNotice { hull:self.hull, life:v.target_generation, event: F3HullEvent::ExplosionEffect,
            position: array(*at), ..Default::default() }.emit(&mut self.w.sim.events);
    }
}
fn notice_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, event: F3HullEvent, amount: f64, attacker: OrdnanceOwner) {
    let v = world_ctx.sim.store.vehicle.get(r).expect("hull notice");
    let (attacker_team, _, _) = crate::combat::live::owner_facts_in(&world_ctx.sim, &mut world_ctx.units, attacker);
    // The damage journal resolves the literal player before refOf. Blast
    // launch metadata intentionally keeps its separate no-reference rule.
    let owner = match attacker { OrdnanceOwner::Player => OrdnanceOwner::Unit(f3::viewpoint_in(&world_ctx.units)), _ => attacker };
    let source = crate::combat::live::owner_reference_in(&world_ctx.sim.store, owner);
    let has_attacker_ref = source.is_some();
    F3HullNotice {
        hull:r, event, life:v.target_generation, amount, attacker, position:v.position,
        team:v.team.unwrap_or(0.0), attacker_ref:source.unwrap_or_default(),
        has_attacker_ref, attacker_team,
    }.emit(&mut world_ctx.sim.events);
}
fn vec(a: [f64; 3]) -> Vector3 { Vector3::new(a[0], a[1], a[2]) }
fn array(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }

// facade shims: retained for unconverted callers.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn f3_hull(){ f3_hull_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn activate(r: Ref<Vehicle>, index: usize){ activate_in(crate::abi::world(), r, index) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn refresh(id: u32){ refresh_in(crate::abi::world(), id) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn matches_binding(reference:Ref<Vehicle>,member:Option<Member>)->bool{ matches_binding_in(crate::abi::world(), reference, member) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn member(r: Ref<Vehicle>) -> Member{ member_in(&crate::abi::world().units, r) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn for_member(member: Member) -> Option<Ref<Vehicle>>{ for_member_in(crate::abi::world(), member) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn battle_member(s: &Store, key: Member) -> Option<crate::battle::world::Member>{ battle_member_in(&mut crate::abi::world().units, s, key) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn mutate(r: Ref<Vehicle>, value: f64, attacker: Option<OrdnanceOwner>, services: WorldServices<'_>){ mutate_in(crate::abi::world(), r, value, attacker, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn sphere(r: Ref<Vehicle>, blast: bool) -> (Vector3, f64){ sphere_in(crate::abi::world(), r, blast) }

#[cfg(target_arch = "wasm32")]
pub(crate) struct DamageWorld<'a> { pub services: WorldServices<'a>, pub hull: Ref<Vehicle> }
#[cfg(target_arch = "wasm32")]
impl DamageWorld<'_> { fn with<T>(&mut self, run: impl FnOnce(&mut HullDamage<'_, '_>) -> T) -> T { run(&mut HullDamage::new(crate::abi::world(), self.hull, self.services.reborrow())) } }
#[cfg(target_arch = "wasm32")]
impl HullDamageWorld for DamageWorld<'_> {
    fn commit_hull(&mut self, hull: &Hull) { self.with(|world| world.commit_hull(hull)); }
    fn resume_hull(&mut self, hull: &mut Hull) { self.with(|world| world.resume_hull(hull)); }
    fn damaged_hull(&mut self, index: usize, amount: f64, attacker: &OrdnanceOwner) { self.with(|world| world.damaged_hull(index, amount, attacker)); }
    fn set_damage_body(&mut self, body: u32, enabled: bool) { self.with(|world| world.set_damage_body(body, enabled)); }
    fn start_respawn(&mut self, index: usize) { self.with(|world| world.start_respawn(index)); }
    fn destroy_hull_crew(&mut self, index: usize) { self.with(|world| world.destroy_hull_crew(index)); }
    fn destroyed_hull(&mut self, index: usize) { self.with(|world| world.destroyed_hull(index)); }
    fn release_hull_crew(&mut self, index: usize) { self.with(|world| world.release_hull_crew(index)); }
    fn explode_hull(&mut self, index: usize, final_explosion: bool, at: &Vector3) { self.with(|world| world.explode_hull(index, final_explosion, at)); }
    fn hull_effect(&mut self, name: &str, at: &Vector3) { self.with(|world| world.hull_effect(name, at)); }
}
