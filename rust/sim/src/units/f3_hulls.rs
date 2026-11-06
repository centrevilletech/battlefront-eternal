//! Vehicles.struck/blast and Vehicle.damage share the fleet's canonical hulls.
//! Only immutable class geometry and numeric explosion bindings live here.
//! A recursive hit takes a reserved working row, commits before crew death,
//! and reloads the live result before its outer damage operation continues.
use super::f3::{self, WorldServices};
use crate::abi::sim;
use crate::combat::impacts::{BlastPacket, HitPacket};
use crate::gen::*;
use crate::store::{Ref, StrId};
use crate::vehicles::{damage::HullDamageWorld, module::{self as fleet, f2}, Hull};
use std::ptr::addr_of_mut;
use three::{Matrix4, Vector3};

// Vehicle construction binds both the collision owner and its mixed-registry
// member. Lookups use those numeric slots; no class names enter a hit loop.
#[derive(Default)]
struct Hulls {
    rows: Vec<Option<F3HullSetup>>,
    members: Vec<Option<Ref<Vehicle>>>,
    bodies: Vec<Option<Ref<Vehicle>>>,
    serial: u32,
    keys: Vec<StrId>,
    fact_versions: Vec<Option<u64>>,
}
static mut HULLS: Option<Hulls> = None;
fn hulls() -> &'static mut Hulls {
    unsafe { (*addr_of_mut!(HULLS)).get_or_insert_with(Hulls::default) }
}
fn binding(r: Ref<Vehicle>) -> &'static F3HullSetup {
    hulls().rows[r.slot as usize].as_ref().filter(|row| row.hull == r).expect("prepared hull combat")
}
// Vehicles.variant supplies this row only after the ordinary fleet adopted
// the object. Its member is already in Units' shared insertion sequence.
// Body and member reverse indices grow here, never during a blast traversal.
#[no_mangle]
pub extern "C" fn f3_hull() {
    let (row, heap) = sim().calls.args();
    let input = F3HullSetup::take(row, heap);
    let body = sim().store.vehicle.get(input.hull).expect("adopted fleet hull").body;
    let id = input.member.map(super::registry::target_id).map(|id| id as usize);
    let all = hulls();
    all.rows.resize_with(all.rows.len().max(input.hull.slot as usize + 1), || None);
    all.fact_versions.resize(all.rows.len(), None);
    all.fact_versions[input.hull.slot as usize] = None;
    if let Some(id) = id { all.members.resize(all.members.len().max(id + 1), None); }
    all.bodies.resize(all.bodies.len().max(body.slot as usize + 1), None);
    if let Some(id) = id { all.members[id] = Some(input.hull); }
    all.bodies[body.slot as usize] = Some(input.hull);
    let slot = input.hull.slot as usize;
    all.rows[slot] = Some(input);
    // The first native sensor may run before the first fleet update.
    if let Some(id) = id { refresh(id as u32); }
    let count = all.rows.iter().flatten().count();
    while all.keys.len() < count {
        all.keys.push(sim().strings.intern_str(&format!("v{}",all.keys.len())));
    }
    f3::reserve_registry();
}

// CollisionWorld assigns polygon ranges by construction order, independently
// of private kernel slots. Prepared hulls consume their range only on birth.
pub(crate) fn body_serial(serial: u32) { hulls().serial = serial; }
pub(crate) fn prepared_count() -> usize { hulls().rows.iter().flatten().filter(|r| r.member.is_none()).count() }
pub(crate) fn reserve_members(count: usize) {
    hulls().members.resize(hulls().members.len().max(count),None);
}

// Vehicles.variant appends one mixed target at the reached spawn slot. This
// keeps equal-distance ties and ids unchanged when a soldier was added first.
pub(crate) fn activate(r: Ref<Vehicle>, index: usize) {
    let id = sim().store.units.add(|id| Member::VehicleTarget(Ref::new(id,0)));
    let member = Member::VehicleTarget(Ref::new(id,0));
    let all = hulls();
    let row = all.rows[r.slot as usize].as_mut().expect("prepared hull combat");
    assert!(row.member.is_none());
    row.member = Some(member); row.key = all.keys[index];
    all.members[id as usize] = Some(r);
    all.serial += 1;
    let serial = all.serial;
    let v = sim().store.vehicle.get_mut(r).unwrap();
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
    F3HullBirth {hull:r,member_id:id,index:index as u32,body_serial:serial}.emit(&mut sim().events);
    refresh(id);
}

/// VehicleTarget.team reads the first occupant before the hull's base team.
/// This target team differs from the team Vehicles.blast uses for damage.
fn target_team(v: &Vehicle) -> f64 {
    for seat in &v.seats {
        if let Some(lease) = seat.lease.and_then(|r| sim().store.seat_lease.get(r)) {
            return lease.team;
        }
    }
    v.team.unwrap_or(0.0)
}
/// VehicleTarget.sphere/aimPoint use the current physics matrix. Refresh only
/// when a native reader asks, so late seat changes cannot leave stale teams.
/// F2's revision changes at physics stores and seat/damage commands. Repeated
/// facts/aim/LOS reads of that same revision share its prepared target row.
pub(crate) fn refresh(id: u32) {
    let Some(r) = hulls().members.get(id as usize).copied().flatten() else { return };
    let revision = f2::revision(r);
    if hulls().fact_versions[r.slot as usize] == Some(revision) { return; }
    let Some(v) = sim().store.vehicle.get(r) else { return };
    let row = binding(r);
    let mut matrix = Matrix4::default();
    matrix.elements = v.matrix;
    let mut center = vec(v.center);
    center.apply_matrix4(&matrix);
    // lockSpan uses the collision box, whereas broadphase queries use the
    // bound radius. Neither reader needs a JS root or a posed scene node.
    f3::prepare_prop_target(F3PropFact {
        id, alive: v.alive, dead: !v.alive, team: target_team(v), health: v.health,
        category: row.category, kind: row.kind, position: v.position, velocity: v.velocity,
        bounds: Bounds { center: array(center), radius: v.bound_radius }, aim: array(center),
        life: v.target_generation, body: Some(v.body),
        r#box: Some(F3TargetBox { min: v.bounds_min, max: v.bounds_max, matrix: v.matrix }),
        ..Default::default()
    });
    hulls().fact_versions[r.slot as usize] = Some(revision);
}
// VehicleTarget keeps its native hull identity independently of registry id.
// Only a matching prepared member can resolve through this reverse index.
pub(crate) fn member(r: Ref<Vehicle>) -> Member { binding(r).member.expect("active hull member") }
pub(crate) fn for_member(member: Member) -> Option<Ref<Vehicle>> {
    let r = hulls().members.get(super::registry::target_id(member) as usize).copied().flatten()?;
    let row = hulls().rows.get(r.slot as usize)?.as_ref().filter(|row|row.hull==r)?;
    (row.member == Some(member) && sim().store.vehicle.contains(r)).then_some(r)
}
// repairLockKey is an object's construction index, independent of its life.
pub(crate) fn lock_key(r: Ref<Vehicle>) -> StrId { binding(r).key }
pub(crate) fn count() -> usize { sim().store.vehicles.vehicles.len() }
pub(crate) fn reference(index: usize) -> Ref<Vehicle> { sim().store.vehicles.vehicles[index] }
pub(crate) fn repairable(r: Ref<Vehicle>) -> bool { binding(r).health == 3 }
pub(crate) fn health_scale(r: Ref<Vehicle>) -> u32 { binding(r).health }
pub(crate) fn body_index(body: Ref<Body>) -> Option<usize> {
    let r = hulls().bodies.get(body.slot as usize).copied().flatten()?;
    if !sim().store.vehicle.get(r).is_some_and(|v| v.body == body) { return None; }
    sim().store.vehicles.vehicles.iter().position(|&v| v == r)
}

/// Vehicle.damage/setHealth can be entered by hits, repair and native physics.
/// A working row comes from F2's retained pool rather than cloning its class.
pub(crate) fn mutate(r: Ref<Vehicle>, value: f64, attacker: Option<OrdnanceOwner>, services: WorldServices<'_>) {
    let index = sim().store.vehicles.vehicles.iter().position(|&v| v == r).expect("ordered fleet hull");
    let mut hull = f2::take_hull(r, index);
    // The retained spare may last have served a ground-only read. Preserve
    // flight state too before the common store writes this incoming hit.
    if hull.flying { fleet::read_flight(&mut hull); }
    let mut world = DamageWorld { services, hull: r };
    if let Some(attacker) = attacker { hull.damage(&mut world, value, attacker); }
    else { hull.set_health(&mut world, value); }
    // The solver can reload after a nested death. Store that final result,
    // not the row sampled before its onDamaged or crew hooks.
    fleet::store_hull(&hull);
    f2::recycle(hull);
}
/// Vehicles.struck accepts only the actual hull collision owner. A stale body
/// reference must not redirect a projectile to a recycled vehicle slot.
pub(crate) fn struck(body: Ref<Body>, packet: HitPacket, owner: OrdnanceOwner, services: WorldServices<'_>) {
    let Some(r) = hulls().bodies.get(body.slot as usize).copied().flatten() else { return };
    let Some(v) = sim().store.vehicle.get(r).filter(|v| v.body == body) else { return };
    // Vehicles.struck multiplies the authored health scale before friendly
    // fire. Keep that order even when either factor happens to be zero.
    let typed = packet.scales[binding(r).health as usize];
    let (team, human, _) = crate::combat::live::owner_facts(owner);
    let scale = crate::combat::impacts::team_damage(team, human, v.team.unwrap_or(0.0), false, f3::friendly_fire());
    mutate(r, packet.damage * typed * scale, Some(owner), services);
}
/// Vehicle.blastSphere keeps the authored model sphere, widened by the box.
/// Its center follows the current matrix, including a destroyed walker's lean.
pub(crate) fn sphere(r: Ref<Vehicle>, blast: bool) -> (Vector3, f64) {
    let v = sim().store.vehicle.get(r).expect("fleet sphere");
    let row = binding(r);
    let mut matrix = Matrix4::default();
    matrix.elements = v.matrix;
    let mut center = vec(if blast { row.blast_center } else { v.center });
    center.apply_matrix4(&matrix);
    (center, if blast { row.blast_radius } else { v8math::max(v.size[0], v.size[2]) * 0.5 })
}
/// Vehicles.blast revisits the live fleet length after each immediate death.
/// A hull shields neither itself nor another hull: LOS tests statics only.
pub(crate) fn blast(packet: BlastPacket, at: Vector3, owner: OrdnanceOwner, mut services: WorldServices<'_>) {
    let mut index = 0;
    while index < count() {
        let r = reference(index);
        index += 1;
        let v = sim().store.vehicle.get(r).expect("blast hull");
        if !v.alive { continue; }
        let (center, radius) = sphere(r, true);
        // blastDistance deliberately subtracts half the radius, without a
        // clamp at zero. A source inside the hull keeps that negative value.
        let distance = crate::ordnance::blast_distance(array(at), array(center), radius);
        let typed = packet.hit.scales[binding(r).health as usize];
        let damage = packet.hit.damage * crate::ordnance::defs::falloff(distance, packet.inner, packet.outer) * typed;
        if !(damage > 0.0) { continue; }
        let mut sight = center;
        sight.sub(&at);
        let span = sight.length();
        if !(span < 1e-3) {
            sight.divide_scalar(span);
            let hit = super::native::ordnance_cast(f3::collision_world(), array(at), array(sight), span, Some(v.body), true, false);
            if hit.is_some_and(|hit| hit.distance < span - 0.05) { continue; }
        }
        let (team, human, _) = crate::combat::live::owner_facts(owner);
        // An earlier death can change an owner's current team. Sample it
        // per reached hull, as Game.teamDamage does in the live JS loop.
        let scale = crate::combat::impacts::team_damage(team, human, v.team.unwrap_or(0.0), false, f3::friendly_fire());
        mutate(r, damage * scale, Some(owner), services.reborrow());
    }
}

/// Vehicle's damage hooks preserve the original commit/resume slots. Crew
/// deaths and class explosions use the caller's existing world-service borrow;
/// the only synchronous foreign leaf updates Character achievement credit.
pub(crate) struct DamageWorld<'a> {
    pub services: WorldServices<'a>,
    pub hull: Ref<Vehicle>,
}
impl HullDamageWorld for DamageWorld<'_> {
    fn commit_hull(&mut self, hull: &Hull) { fleet::store_hull(hull); }
    fn resume_hull(&mut self, hull: &mut Hull) {
        // A nested crew death may have exploded back through this hull.
        // Its committed canonical row takes precedence over our old copy.
        let v = sim().store.vehicle.get(hull.id).expect("damaged hull");
        hull.load(v);
        if hull.flying { fleet::read_flight(hull); }
    }
    fn damaged_hull(&mut self, index: usize, amount: f64, attacker: &OrdnanceOwner) {
        // Vehicle.onDamaged is a notice. Its sampled values stay before the
        // later destruction and class-blast records in the shared stream.
        notice(reference(index), F3HullEvent::Damaged, amount, *attacker);
    }
    fn set_damage_body(&mut self, body: u32, enabled: bool) {
        fleet::collision_world().expect("fleet collision").enable_body(crate::collision::state(), body, enabled).expect("hull collision body");
    }
    fn start_respawn(&mut self, index: usize) { f2::respawn_clock(index); }
    fn destroy_hull_crew(&mut self, index: usize) { f2::live_seats::destroyed_with(index, Some(self.services.reborrow())); }
    fn destroyed_hull(&mut self, index: usize) {
        let r = reference(index);
        let owner = sim().store.vehicle.get(r).expect("destroyed hull").last_attacker;
        let (_, _, attacker) = crate::combat::live::owner_facts(owner);
        let character = match attacker {
            Attacker::Unit(r) => sim().store.unit.get(r).and_then(|u| u.character),
            Attacker::Player => sim().store.unit.get(f3::viewpoint()).and_then(|u| u.character),
            Attacker::OwnerSnapshot(r) => sim().store.owner_snapshot.get(r).and_then(|o| o.character),
            _ => None,
        };
        if let Some(character) = character {
            // Game.vehicleDestroyed updates an achievement immediately.
            // It sees committed mirrors but cannot call damage or a solver.
            f3::foreign_barrier();
            import_f3_hull_credit(&F3HullCredit { character });
        }
    }
    fn release_hull_crew(&mut self, index: usize) { f2::live_seats::release_with(index, Some(self.services.reborrow())); }
    fn explode_hull(&mut self, index: usize, final_explosion: bool, at: &Vector3) {
        let r = reference(index);
        let row = binding(r);
        let definition = if final_explosion { row.final_explosion } else { row.explosion };
        if let Some(definition) = definition {
            // Wreck blasts credit the hull object, independently of the
            // attacker credited for destroying it or killing its crew.
            crate::combat::live::blast(crate::store::DefId(definition), array(*at), OrdnanceOwner::Vehicle(r), Some(self.services.reborrow()));
        }
    }
    fn hull_effect(&mut self, name: &str, at: &Vector3) {
        // Vehicle.setHealth has one fallback effect when no class blast exists.
        assert_eq!(name, "explosion");
        let v = sim().store.vehicle.get(self.hull).expect("hull effect");
        F3HullNotice { hull:self.hull, life:v.target_generation, event: F3HullEvent::ExplosionEffect,
            position: array(*at), ..Default::default() }.emit(&mut sim().events);
    }
}
fn notice(r: Ref<Vehicle>, event: F3HullEvent, amount: f64, attacker: OrdnanceOwner) {
    let v = sim().store.vehicle.get(r).expect("hull notice");
    let (attacker_team, _, _) = crate::combat::live::owner_facts(attacker);
    // The damage journal resolves the literal player before refOf. Blast
    // launch metadata intentionally keeps its separate no-reference rule.
    let owner = match attacker { OrdnanceOwner::Player => OrdnanceOwner::Unit(f3::viewpoint()), _ => attacker };
    let source = crate::combat::live::owner_reference(owner);
    let has_attacker_ref = source.is_some();
    F3HullNotice {
        hull:r, event, life:v.target_generation, amount, attacker, position:v.position,
        team:v.team.unwrap_or(0.0), attacker_ref:source.unwrap_or_default(),
        has_attacker_ref, attacker_team,
    }.emit(&mut sim().events);
}
fn vec(a: [f64; 3]) -> Vector3 { Vector3::new(a[0], a[1], a[2]) }
fn array(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
