//! Retained rider and held-body samples used by Unit.updateSpheres.
use super::poses::BodyPose;
use crate::abi::sim;
use crate::gen::{F3BodySkeleton, F3PoseCommand, HitShape, Unit};
use crate::rig::{rider::RiderSample, skeleton::Skeleton};
use crate::store::Ref;
use std::ptr::addr_of_mut;
use three::{Matrix4, Quaternion, Vector3};

pub(crate) struct Body {
    reference: Ref<Unit>,
    sampler: BodyPose,
    pose: Option<F3PoseCommand>,
}
static mut BODIES: Vec<Option<Body>> = Vec::new();
fn bodies() -> &'static mut Vec<Option<Body>> {
    // A module thread; sampling never enters JavaScript.
    unsafe { &mut *addr_of_mut!(BODIES) }
}

/// UnitFactory.build's already decomposed bind transforms, loaded once.
#[no_mangle]
pub extern "C" fn f3_body_skeleton() -> u32 {
    let (row, heap) = sim().calls.args();
    let input = F3BodySkeleton::take(row, heap);
    if input.key.validate(&sim().store).is_err() { return 0; }
    let mut names = Vec::with_capacity(input.bones.len());
    let mut parents = Vec::with_capacity(input.bones.len());
    let mut bind = Vec::with_capacity(input.bones.len());
    for b in input.bones {
        names.push(String::from_utf16_lossy(sim().strings.units(b.name)));
        parents.push(b.parent);
        bind.push((Vector3::new(b.position[0], b.position[1], b.position[2]),
            Quaternion::new(b.quaternion[0], b.quaternion[1], b.quaternion[2], b.quaternion[3]),
            Vector3::new(b.scale[0], b.scale[1], b.scale[2])));
    }
    let skeleton = Skeleton::from_bind(names, parents, &bind, false);
    let slot = input.key.slot as usize;
    let list = bodies();
    if list.len() <= slot { list.resize_with(slot + 1, || None); }
    list[slot] = Some(Body { reference: input.key.reference(), sampler: BodyPose::new(&skeleton), pose: None });
    1
}

pub fn set(input:F3PoseCommand) -> bool {
    let Some(body)=bodies().get_mut(input.key.slot as usize).and_then(Option::as_mut) else {return false;};
    if body.reference!=input.key.reference() {return false;}
    body.pose=Some(input);
    true
}
pub fn clear(reference: Ref<Unit>) {
    if let Some(body) = bodies().get_mut(reference.slot as usize).and_then(Option::as_mut) {
        if body.reference==reference {body.pose=None;}
    }
}
/// AnimatedProp.carry places the soldier's root on the foreign grab node.
/// Keep the retained sampler so carrying and later lazy limb queries share
/// the same bind transforms without touching a drawn skeleton.
pub fn held_frame(input: &F3PoseCommand, out: &mut Matrix4) -> bool {
    let Some(clip) = input.clip else { return false };
    let Some(body)=bodies().get_mut(input.key.slot as usize).and_then(Option::as_mut) else {super::f3::fail();return false;};
    if body.reference!=input.key.reference() {super::f3::fail();return false;}
    let Some(clip)=crate::pose::state().clips.get(clip as usize) else {super::f3::fail();return false;};
    let mut node = Matrix4::default(); node.from_array(&input.frame, 0);
    body.sampler.held_frame(clip,input.time,&node,out)
}
pub fn exposed(_id: u32, unit: &Unit) -> bool {
    if let Some(lease) = unit.seat.and_then(|r| sim().store.seat_lease.get(r)) {
        return crate::vehicles::module::f2::live_seats::exposed(lease);
    }
    false
}
// Game.onExplode admits exposed stationary gunners only. Hull riders are
// skipped even when a round can meet their shown rider shapes.
pub(crate) fn blastable(id: u32) -> bool {
    let reference = super::f3::unit_reference(id);
    let unit = sim().store.unit.get(reference).expect("blast seat unit");
    unit.seat.and_then(|r| sim().store.seat_lease.get(r))
        .is_some_and(|l| l.turret.is_some() && crate::vehicles::module::f2::live_seats::exposed(l))
}

/// SeatLease.riderShapes and AnimatedProp.heldShapes read their own sample,
/// while Unit owns when it invalidates and observes the resulting shapes.
pub fn sample(id: u32, unit: &Unit, out: &mut [HitShape]) -> usize {
    if unit.seat.and_then(|r| sim().store.seat_lease.get(r)).is_some() {
        return crate::vehicles::module::f2::riders::shapes(super::f3::unit_reference(id), unit.generation, out);
    }
    let Some(body) = bodies().get_mut(id as usize).and_then(Option::as_mut) else { return 0 };
    let Some(pose) = &body.pose else { return 0 };
    let Some(clip) = pose.clip else { return 0 };
    let clips = crate::pose::state();
    let sample = RiderSample { clip: &clips.clips[clip as usize], pitch: 0.0, turn: 0.0, time: Some(pose.time) };
    let mut frame = Matrix4::default(); frame.from_array(&pose.frame, 0);
    body.sampler.hit_shapes(&sample, &frame, unit.root_scale, out)
}

// OrdnanceWorld.mayStrike needs the lease's mount identity, independently of
// whether that lease exposes a sampled rider pose. No host lookup is needed.
pub(crate) fn mount(id: u32) -> Option<(Option<crate::store::Ref<crate::gen::Body>>, Option<crate::store::Ref<crate::gen::Vehicle>>)> {
    if let Some(unit) = sim().store.unit.get(super::f3::unit_reference(id)) {
        if let Some(lease) = unit.seat.and_then(|r| sim().store.seat_lease.get(r)) {
            if let Some(vehicle) = lease.vehicle {
                return Some((sim().store.vehicle.get(vehicle).map(|v| v.body), Some(vehicle)));
            }
            return Some((lease.turret.and_then(crate::vehicles::module::f2::live_seats::turret_body), None));
        }
    }
    None
}

// PlayerBodies.adopt and UnitFactory.build transfer a prepared sampler. Its
// bind lengths survive class activation without another JS rig import.
pub(crate) fn take(slot: u32) -> Option<Body> {
    bodies().get_mut(slot as usize).and_then(Option::take)
}
pub(crate) fn install(reference: Ref<Unit>, body: Option<Body>) {
    let rows = bodies();
    assert!((reference.slot as usize) < rows.len(), "reserved body pose slots");
    rows[reference.slot as usize] = body.map(|mut body| {
        body.reference = reference;
        body.pose = None;
        body
    });
}
pub(crate) fn reserve(count: usize) {
    let rows = bodies();
    rows.resize_with(count.max(rows.len()), || None);
}
