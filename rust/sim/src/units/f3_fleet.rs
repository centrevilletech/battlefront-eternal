//! Vehicle and turret targeting read the same canonical registry as infantry.
//! Cheap flags precede lazy Unit spheres, keeping the original lockPick and
//! flight aim-point observation slots even when crew deaths change a target.
use super::{f3, f3_hulls, f3_sensors::Sensors, registry};
use crate::combat::targeting::Targets;
use crate::gen::*;
use crate::store::Ref;
use three::Vector3;

pub(crate) fn count_in(sim: &crate::abi::Sim) -> usize { sim.store.units.list.len() }
pub(crate) fn key_in(sim: &crate::abi::Sim, index: usize) -> Member { sim.store.units.list[index] }
pub(crate) fn life_in(world_ctx: &mut crate::world::owner::World, member: Member) -> f64 {
    match member {
        Member::Unit(r) => world_ctx.sim.store.unit.get(r).expect("fleet target Unit").generation,
        _ => f3::target_in(world_ctx, registry::target_id(member)).expect("fleet target facts").life,
    }
}
// Game.mountedShot captures refs before subsequent crew work can change a
// pooled source life. Presentation drains only after the native span returns.
pub(crate) fn mounted_sample_in(world_ctx: &mut crate::world::owner::World, owner: OrdnanceOwner, mount: Member) -> F3MountedSample {
    let source = crate::combat::live::owner_reference_in(&world_ctx.sim.store, owner);
    let has_source = source.is_some();
    F3MountedSample {
        source: source.unwrap_or_default(), has_source,
        team: crate::combat::live::owner_facts_in(&world_ctx.sim, &mut world_ctx.units, owner).0,
        mount: F3LifeRef { id: registry::target_id(mount) as f64, generation: life_in(world_ctx, mount) },
    }
}
// Game.mountedShot's counter belongs to the source Unit, not its hull. A
// frozen ordnance owner can name an earlier life; do not count it for reuse.
pub(crate) fn wire_shot_in(w:&mut crate::world::owner::World, sample: &F3MountedSample) {
    if !sample.has_source { return; }
    let life = w.sim.store.unit.at(sample.source.id as u32).map_or(f64::NAN, |(_, unit)| unit.generation);
    crate::wire::shots::native_observe_in(w,sample.source.id, sample.source.generation, life);
}
// Unit.aimPoint observes its lazy sphere. Hull and prop aim points instead
// follow their current matrix or prepared foreign fact, without a JS rig read.
pub(crate) fn aim_in(world_ctx: &mut crate::world::owner::World, member: Member) -> Vector3 {
    if matches!(member, Member::Unit(_)) { (Sensors { w: world_ctx }).volume(member).center }
    else { vec(f3::target_in(world_ctx, registry::target_id(member)).expect("fleet aim point").aim) }
}
fn body_in(world_ctx: &mut crate::world::owner::World, member: Member) -> Option<Ref<Body>> {
    if matches!(member, Member::Unit(_)) { None }
    else { f3::target_in(world_ctx, registry::target_id(member)).expect("fleet target body").body }
}
// FlightWorld.member keeps a Unit's point lazy. Own-hull exclusion is based
// on its actual registry member, not a coincidentally matching slab slot.
pub(crate) fn flight_in(world_ctx: &mut crate::world::owner::World, index: usize, craft: u32) -> crate::flight::Member {
    let member = key_in(&world_ctx.sim, index);
    // FlightWorld.member consumes only life/team/alive flags and an optional
    // foreign aim point. Do not build infantry targeting facts or read a
    // hull's prepared row three times for fields from the same observation.
    if let Member::Unit(r)=member {
        let unit=world_ctx.sim.store.unit.get(r).expect("fleet target Unit");
        return crate::flight::Member {
            id:r.slot as f64,generation:Some(unit.generation),alive:unit.alive,
            team:Some(unit.team),own:false,unit:true,body:None,point:Vector3::default(),
        };
    }
    let facts=f3::target_in(world_ctx, registry::target_id(member)).expect("fleet target facts");
    let (life, alive, team, aim) = (facts.life, facts.alive, facts.team, facts.aim);
    let hull=f3_hulls::for_member_in(world_ctx, member);
    crate::flight::Member {
        id:registry::target_id(member) as f64,generation:Some(life),alive:alive,
        team:Some(team),own:hull.is_some_and(|r|r.slot==craft),unit:false,
        body:hull.and_then(|r|world_ctx.sim.store.vehicle.get(r).map(|v|v.body.slot)),point:vec(aim),
    }
}
// Target masks need the viewer's team, not its position, health or exposure.
// Like facts(), this does not observe any lazy Unit shapes.
fn team_in(world_ctx: &mut crate::world::owner::World, member:Member) -> f64 {
    match member {
        Member::Unit(r)=>world_ctx.sim.store.unit.get(r).expect("fleet viewer Unit").team,
        _=>f3::target_in(world_ctx, registry::target_id(member)).expect("fleet viewer facts").team,
    }
}
// F2Firing.targetFacts and F2Seats.target use the same eligibility rule.
// Asking only flags must not refresh the shapes of dead or seated candidates.
pub(crate) fn target_in(world_ctx: &mut crate::world::owner::World, member: Member, viewer: Option<Member>, shapes: bool, with_aim: bool) -> F2TurretTarget {
    let facts = (Sensors { w: world_ctx }).facts(member);
    let team = viewer.map_or(0.0, |member| team_in(world_ctx, member));
    let eligible = Some(member) != viewer && facts.alive && !facts.seated && facts.health > 0.0 && facts.kind != 10.0;
    let hull = f3_hulls::for_member_in(world_ctx, member);
    let mut out = F2TurretTarget {
        mode: F2TurretTargetMode::Unit, id: registry::target_id(member) as f64,
        generation: life_in(world_ctx, member), alive: facts.alive, eligible,
        relation: f3::relation_in(&world_ctx.units, team, facts.team), category: facts.category,
        hull, center: array(facts.position), ..Default::default()
    };
    if shapes && hull.is_none() {
        let volume = (Sensors { w: world_ctx }).volume(member);
        out.center = array(volume.center); out.radius = v8math::max(0.0, volume.radius);
    }
    if with_aim { out.aim = array(aim_in(world_ctx, member)); out.body = body_in(world_ctx, member); }
    out
}
// Vehicles' target reader supplies a body's identity independently of its
// sight-point request. Unit geometry remains lazy until shapes or aim is asked.
pub(crate) fn hull_target_in(world_ctx: &mut crate::world::owner::World, viewer: Ref<Vehicle>, index: usize, shapes: bool, with_aim: bool) -> FfTargetFacts {
    let member = key_in(&world_ctx.sim, index);
    let facts = target_in(world_ctx, member, Some(f3_hulls::member_in(&world_ctx.units, viewer)), false, with_aim);
    let (center, radius) = if shapes && facts.eligible {
        let volume = (Sensors { w: world_ctx }).volume(member);
        (array(volume.center), v8math::max(0.0, volume.radius))
    } else { (facts.center, 0.0) };
    FfTargetFacts {
        alive: facts.alive, eligible: facts.eligible, relation: facts.relation, category: facts.category,
        center, radius, aim: if with_aim { facts.aim } else { center },
        body: body_in(world_ctx, member).map_or(-1.0, |body| body.slot as f64),
    }
}
// SeatsWorld.raycast and VehiclesWorld.raycastUnits share strict first-hit
// ordering with infantry. Ordinary mounted aim does not include exposed riders.
pub(crate) fn raycast_in(world_ctx: &mut crate::world::owner::World, origin: &Vector3, direction: &Vector3, far: f64, exclude: Option<u32>) -> Option<f64> {
    let exclude = exclude.map_or(registry::Exclude::Nobody, |slot| registry::Exclude::One(Member::Unit(f3::unit_reference_in(&world_ctx.units, slot))));
    f3::raycast_in(world_ctx, array(*origin), array(*direction), far, exclude, false, false).map(|hit| hit.distance)
}
fn vec(a: [f64;3]) -> Vector3 { Vector3::new(a[0],a[1],a[2]) }
fn array(v: Vector3) -> [f64;3] { [v.x,v.y,v.z] }

// facade shims: retained for unconverted callers.
#[cfg(target_arch = "wasm32")]
pub(crate) fn count() -> usize{ count_in(&crate::abi::world().sim) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn key(index: usize) -> Member{ key_in(&crate::abi::world().sim, index) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn life(member: Member) -> f64{ life_in(crate::abi::world(), member) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn mounted_sample(owner: OrdnanceOwner, mount: Member) -> F3MountedSample{ mounted_sample_in(crate::abi::world(), owner, mount) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_shot(sample: &F3MountedSample){ wire_shot_in(crate::abi::world(), sample) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn aim(member: Member) -> Vector3{ aim_in(crate::abi::world(), member) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn flight(index: usize, craft: u32) -> crate::flight::Member{ flight_in(crate::abi::world(), index, craft) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn target(member: Member, viewer: Option<Member>, shapes: bool, with_aim: bool) -> F2TurretTarget{ target_in(crate::abi::world(), member, viewer, shapes, with_aim) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn hull_target(viewer: Ref<Vehicle>, index: usize, shapes: bool, with_aim: bool) -> FfTargetFacts{ hull_target_in(crate::abi::world(), viewer, index, shapes, with_aim) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn raycast(origin: &Vector3, direction: &Vector3, far: f64, exclude: Option<u32>) -> Option<f64>{ raycast_in(crate::abi::world(), origin, direction, far, exclude) }
