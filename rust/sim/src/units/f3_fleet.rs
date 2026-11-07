//! Vehicle and turret targeting read the same canonical registry as infantry.
//! Cheap flags precede lazy Unit spheres, keeping the original lockPick and
//! flight aim-point observation slots even when crew deaths change a target.
use super::{f3, f3_hulls, f3_sensors::Sensors, registry};
use crate::abi::sim;
use crate::combat::targeting::Targets;
use crate::gen::*;
use crate::store::Ref;
use three::Vector3;

pub(crate) fn count() -> usize { sim().store.units.list.len() }
pub(crate) fn key(index: usize) -> Member { sim().store.units.list[index] }
pub(crate) fn life(member: Member) -> f64 {
    match member {
        Member::Unit(r) => sim().store.unit.get(r).expect("fleet target Unit").generation,
        _ => f3::target(registry::target_id(member)).expect("fleet target facts").life,
    }
}
// Game.mountedShot captures refs before subsequent crew work can change a
// pooled source life. Presentation drains only after the native span returns.
pub(crate) fn mounted_sample(owner: OrdnanceOwner, mount: Member) -> F3MountedSample {
    let source = crate::combat::live::owner_reference(owner);
    let has_source = source.is_some();
    F3MountedSample {
        source: source.unwrap_or_default(), has_source,
        team: crate::combat::live::owner_facts(owner).0,
        mount: F3LifeRef { id: registry::target_id(mount) as f64, generation: life(mount) },
    }
}
// Game.mountedShot's counter belongs to the source Unit, not its hull. A
// frozen ordnance owner can name an earlier life; do not count it for reuse.
pub(crate) fn wire_shot(sample: &F3MountedSample) {
    if !sample.has_source { return; }
    let life = sim().store.unit.at(sample.source.id as u32).map_or(f64::NAN, |(_, unit)| unit.generation);
    crate::wire::shots::native_observe(sample.source.id, sample.source.generation, life);
}
// Unit.aimPoint observes its lazy sphere. Hull and prop aim points instead
// follow their current matrix or prepared foreign fact, without a JS rig read.
pub(crate) fn aim(member: Member) -> Vector3 {
    if matches!(member, Member::Unit(_)) { Sensors.volume(member).center }
    else { vec(f3::target(registry::target_id(member)).expect("fleet aim point").aim) }
}
fn body(member: Member) -> Option<Ref<Body>> {
    if matches!(member, Member::Unit(_)) { None }
    else { f3::target(registry::target_id(member)).expect("fleet target body").body }
}
// FlightWorld.member keeps a Unit's point lazy. Own-hull exclusion is based
// on its actual registry member, not a coincidentally matching slab slot.
pub(crate) fn flight(index: usize, craft: u32) -> crate::flight::Member {
    let member = key(index);
    // FlightWorld.member consumes only life/team/alive flags and an optional
    // foreign aim point. Do not build infantry targeting facts or read a
    // hull's prepared row three times for fields from the same observation.
    if let Member::Unit(r)=member {
        let unit=sim().store.unit.get(r).expect("fleet target Unit");
        return crate::flight::Member {
            id:r.slot as f64,generation:Some(unit.generation),alive:unit.alive,
            team:Some(unit.team),own:false,unit:true,body:None,point:Vector3::default(),
        };
    }
    let facts=f3::target(registry::target_id(member)).expect("fleet target facts");
    let hull=f3_hulls::for_member(member);
    crate::flight::Member {
        id:registry::target_id(member) as f64,generation:Some(facts.life),alive:facts.alive,
        team:Some(facts.team),own:hull.is_some_and(|r|r.slot==craft),unit:false,
        body:hull.and_then(|r|sim().store.vehicle.get(r).map(|v|v.body.slot)),point:vec(facts.aim),
    }
}
// Target masks need the viewer's team, not its position, health or exposure.
// Like facts(), this does not observe any lazy Unit shapes.
fn team(member:Member) -> f64 {
    match member {
        Member::Unit(r)=>sim().store.unit.get(r).expect("fleet viewer Unit").team,
        _=>f3::target(registry::target_id(member)).expect("fleet viewer facts").team,
    }
}
// F2Firing.targetFacts and F2Seats.target use the same eligibility rule.
// Asking only flags must not refresh the shapes of dead or seated candidates.
pub(crate) fn target(member: Member, viewer: Option<Member>, shapes: bool, with_aim: bool) -> F2TurretTarget {
    let facts = Sensors.facts(member);
    let team = viewer.map_or(0.0, team);
    let eligible = Some(member) != viewer && facts.alive && !facts.seated && facts.health > 0.0 && facts.kind != 10.0;
    let hull = f3_hulls::for_member(member);
    let mut out = F2TurretTarget {
        mode: F2TurretTargetMode::Unit, id: registry::target_id(member) as f64,
        generation: life(member), alive: facts.alive, eligible,
        relation: f3::relation(team, facts.team), category: facts.category,
        hull, center: array(facts.position), ..Default::default()
    };
    if shapes && hull.is_none() {
        let volume = Sensors.volume(member);
        out.center = array(volume.center); out.radius = v8math::max(0.0, volume.radius);
    }
    if with_aim { out.aim = array(aim(member)); out.body = body(member); }
    out
}
// Vehicles' target reader supplies a body's identity independently of its
// sight-point request. Unit geometry remains lazy until shapes or aim is asked.
pub(crate) fn hull_target(viewer: Ref<Vehicle>, index: usize, shapes: bool, with_aim: bool) -> FfTargetFacts {
    let member = key(index);
    let facts = target(member, Some(f3_hulls::member(viewer)), false, with_aim);
    let (center, radius) = if shapes && facts.eligible {
        let volume = Sensors.volume(member);
        (array(volume.center), v8math::max(0.0, volume.radius))
    } else { (facts.center, 0.0) };
    FfTargetFacts {
        alive: facts.alive, eligible: facts.eligible, relation: facts.relation, category: facts.category,
        center, radius, aim: if with_aim { facts.aim } else { center },
        body: body(member).map_or(-1.0, |body| body.slot as f64),
    }
}
// SeatsWorld.raycast and VehiclesWorld.raycastUnits share strict first-hit
// ordering with infantry. Ordinary mounted aim does not include exposed riders.
pub(crate) fn raycast(origin: &Vector3, direction: &Vector3, far: f64, exclude: Option<u32>) -> Option<f64> {
    let exclude = exclude.map_or(registry::Exclude::Nobody, |slot| registry::Exclude::One(Member::Unit(f3::unit_reference(slot))));
    f3::raycast(array(*origin), array(*direction), far, exclude, false, false).map(|hit| hit.distance)
}
fn vec(a: [f64;3]) -> Vector3 { Vector3::new(a[0],a[1],a[2]) }
fn array(v: Vector3) -> [f64;3] { [v.x,v.y,v.z] }
