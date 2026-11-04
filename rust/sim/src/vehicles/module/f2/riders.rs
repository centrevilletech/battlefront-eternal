//! Vehicles.syncCrews and riderBody. Skeletons, clip bindings and sampler
//! scratch persist with each actor. Publishing samples does not invalidate a
//! Unit's lazy hit-shape cache; its existing tickCount still controls that.
use super::*;
use crate::rig::rider::{self, RiderSkeleton, RiderScratch, RiderPoints, RiderSample, HitShape, ShapeKind, ShapePart};
use crate::rig::skeleton::Skeleton;
use crate::store::block::Block;
use three::{Matrix4, Quaternion, Vector3};

struct Rider {
    life: f64, scale: f64, skeleton: RiderSkeleton, scratch: RiderScratch,
    q: Vec<Quaternion>, p: Vec<Vector3>, clips: Vec<F2RiderClip>,
    points: RiderPoints, shapes: [HitShape; 10], out: F2RiderOut,
    encoded: Block, previous: Block, dirty: bool,
}
static mut RIDERS: Vec<Option<Rider>> = Vec::new();
static mut OUTPUT: Option<Block> = None;
fn rows() -> &'static mut Vec<Option<Rider>> { unsafe { &mut *addr_of_mut!(RIDERS) } }
fn output() -> &'static mut Block { unsafe { (*addr_of_mut!(OUTPUT)).get_or_insert_with(Block::default) } }

#[no_mangle]
pub extern "C" fn f2_rider_birth_words() -> u32 { F2RiderBirth::WORDS as u32 }
/// riderPoints skeletonOf/bindsOf and the seat's resolved clip are birth data.
#[no_mangle]
pub extern "C" fn f2_rider_birth() {
    let a = { let (row, heap) = sim().calls.args(); F2RiderBirth::take(row, heap) };
    let index = a.unit.slot as usize;
    if rows().len() <= index { rows().resize_with(index + 1, || None); }
    if !a.bones.is_empty() {
        let names = a.bones.iter().map(|b| String::from_utf16_lossy(sim().strings.units(b.name))).collect();
        let parents = a.bones.iter().map(|b| b.parent).collect();
        let bind: Vec<_> = a.bones.iter().map(|b| (vec(&b.p), Quaternion::new(b.q[0], b.q[1], b.q[2], b.q[3]), vec(&b.s))).collect();
        let skeleton = Skeleton::from_bind(names, parents, &bind, false);
        let mut skeleton = RiderSkeleton::new(&skeleton);
        for (i, b) in a.bones.iter().enumerate() { skeleton.binds[i] = b.bind.as_ref().map(vec); }
        skeleton.root_bind = a.root_bind.as_ref().map(vec);
        let mut out = F2RiderOut { unit: a.unit, life: a.life, clip: -1, ..Default::default() };
        out.shapes.reserve(10);
        rows()[index] = Some(Rider { life: a.life, scale: a.scale, scratch: RiderScratch::new(&skeleton), skeleton,
            q: bind.iter().map(|b| b.1).collect(), p: bind.iter().map(|b| b.0).collect(), clips: Vec::with_capacity(16),
            points: RiderPoints::default(), shapes: [HitShape::default(); 10], out,
            encoded: Block { words: Vec::with_capacity(256), heap: Vec::with_capacity(128) },
            previous: Block { words: Vec::with_capacity(256), heap: Vec::with_capacity(128) }, dirty: false });
    }
    if let Some(row) = &mut rows()[index] {
        row.life = a.life; row.out.life = a.life;
        for clip in a.clips {
            if let Some(old) = row.clips.iter_mut().find(|old| old.hull == clip.hull && old.turret == clip.turret && old.index == clip.index) { *old = clip; }
            else { row.clips.push(clip); }
        }
    }
    output().words.reserve(5 + rows().len() * (F2RiderOut::WORDS + 10 * F2RiderShape::WORDS));
    output().heap.reserve(rows().len() * 10 * F2RiderShape::WORDS);
}

/// riderBody/riderHitShapes share the exact sampler and frame used for death
/// drops. Animal clips put their pelvis on the saddle, leaving the root below.
#[allow(clippy::too_many_arguments)]
fn sample(unit: Ref<Unit>, hull: Option<Ref<Vehicle>>, turret: Option<Ref<Turret>>, index: usize,
    show: bool, point: Vector3, mut frame: Matrix4, animal: Option<Quaternion>, pitch: f64, turn: f64, time: f64) {
    let Some(row) = rows().get_mut(unit.slot as usize).and_then(Option::as_mut) else { return };
    let Some(u) = sim().store.unit.get(unit) else { return };
    if u.generation != row.life { return; }
    let Some(binding) = row.clips.iter().find(|c| c.hull == hull && c.turret == turret && c.index as usize == index) else { return };
    row.out.show = show; row.out.clip = binding.clip;
    row.out.pitch = if binding.time.is_some() || !binding.nine { 0.0 } else { pitch };
    row.out.turn = if binding.time.is_some() || !binding.nine { 0.0 } else { turn };
    row.out.time = binding.time.or_else(|| (!binding.nine).then_some(time));
    row.out.drop = None; row.out.shapes.clear();
    if show && binding.clip >= 0 {
        let clip = &crate::pose::state().clips[binding.clip as usize];
        let sample = RiderSample { clip, pitch: row.out.pitch, turn: row.out.turn, time: row.out.time };
        if rider::rider_points(&row.skeleton, &row.q, &row.p, &sample, &mut row.scratch, &mut row.points, false) {
            if let Some(q) = animal {
                let mut offset = row.points.pelvis; offset.apply_quaternion(&q);
                let mut origin = point; origin.sub(&offset);
                frame.compose(&origin, &q, &Vector3::new(1.0, 1.0, 1.0));
            }
            let mut drop = Vector3::default(); rider::rider_drop_point(&row.points, &mut drop); drop.apply_matrix4(&frame);
            row.out.drop = Some(v3(&drop));
            let count = rider::rider_hit_shapes(&row.points, &frame, Some(row.scale), &mut row.shapes);
            for shape in &row.shapes[..count] {
                row.out.shapes.push(F2RiderShape { capsule: shape.kind == ShapeKind::Capsule, head: shape.part == ShapePart::Head,
                    center: v3(&shape.center), half: v3(&shape.half_size), radius: shape.radius });
            }
        }
    }
    row.dirty = true;
}

/// Vehicle.poseRider's geometry before firing, and syncCrews' later body write.
/// Keeping those slots distinct preserves lazy hit shapes during mounted fire.
pub fn hull(r: Ref<Vehicle>, write_body: bool) {
    let Some(v) = sim().store.vehicle.get(r) else { return };
    if !v.alive { return; }
    let p = poses()[r.slot as usize].as_mut().unwrap();
    p.pose.rig.tree.update_matrix_world(p.pose.rig.root, true);
    for index in 0..v.seats.len() {
        let seat = &v.seats[index];
        let Some(l) = seat.lease.and_then(|l| sim().store.seat_lease.get(l)) else { continue };
        if l.human { continue; }
        let Some(unit) = l.unit else { continue };
        let rider = p.pose.rig.seats[index].rider.as_ref();
        let node = rider.and_then(|r| r.node);
        let mut point = vec(&v.position); let mut frame = Matrix4::default();
        if let Some(node) = node {
            frame = p.pose.rig.tree.node(node).matrix_world;
            point.set(frame.elements[12], frame.elements[13], frame.elements[14]);
        } else { point.y += v.size[1] * 0.5; }
        let view = &v.seat_views[index]; let sp = &p.pose.seats[index];
        let (pitch, turn) = if index == 0 { (-seat.pitch / 0.7, 0.0) } else {
            crate::vehicles::pose::nine_pose(view.pitch_limits, view.yaw_limits, view.wrap_yaw,
                p.turn_rates[index], crate::vehicles::hull::angle(seat.aim_yaw - v.yaw), seat.pitch, sp.yaw_rate)
        };
        let q = p.pose.rig.tree.node(p.pose.rig.root).quaternion;
        sample(unit, Some(r), None, index, node.is_some(), point, frame, v.creature.then_some(q), pitch, turn, p.pose.age);
        if write_body && super::live_seats::has_actor(unit) {
            let u = sim().store.unit.get_mut(unit).unwrap();
            if let Some(s) = crate::seats::bodies::soldier_mut(u) {
                s.position = v3(&point); s.previous = s.position; s.settled = s.position;
                s.velocity = v.velocity; s.aim_yaw = seat.aim_yaw; s.aim_pitch = seat.pitch;
            }
            u.tick_count += 1.0; super::live_seats::rider_posed(unit);
        }
    }
}

/// Turret.riderBody uses its seat's full world turn, including a bound drum.
pub fn turret(unit: Ref<Unit>, turret: Ref<Turret>, show: bool, point: Vector3, turn: Quaternion, pitch: f64, yaw: f64) {
    let mut frame = Matrix4::default(); frame.compose(&point, &turn, &Vector3::new(1.0, 1.0, 1.0));
    sample(unit, None, Some(turret), 0, show, point, frame, None, pitch, yaw, 0.0);
}
pub fn drop(unit: Ref<Unit>) -> Option<Vector3> {
    rows().get(unit.slot as usize).and_then(Option::as_ref).and_then(|r| r.out.drop.as_ref()).map(vec)
}

/// Publish only samples whose numeric values changed. A stationary nine-pose
/// rider keeps its shape cache while tickCount continues its normal cadence.
pub fn publish() -> *const f64 {
    let mut count = 0;
    for row in rows().iter_mut().flatten() {
        if !row.dirty { continue; }
        row.encoded.begin(1, F2RiderOut::WORDS);
        let (words, heap) = row.encoded.row(0); row.out.put(words, heap); row.encoded.finish();
        let same = row.encoded.words.len() == row.previous.words.len() && row.encoded.words.iter().zip(&row.previous.words).all(|(a,b)| a.to_bits() == b.to_bits());
        if same { row.dirty = false; } else { count += 1; }
    }
    let out = output(); out.begin(count, F2RiderOut::WORDS); let mut at = 0;
    for row in rows().iter_mut().flatten() {
        if !row.dirty { continue; }
        let (words, heap) = out.row(at); row.out.put(words, heap); at += 1;
        std::mem::swap(&mut row.previous, &mut row.encoded); row.dirty = false;
    }
    out.finish(); out.ptr()
}
