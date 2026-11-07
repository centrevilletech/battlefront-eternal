//! Vehicles.syncCrews publishes animation and death-drop facts. The unit
//! sampler and retained rider geometry stay native. Publication never advances
//! the unit's lazy hit-shape cache; its existing tickCount still controls that.
use super::*;
use crate::rig::rider::{self, RiderPoints, RiderSample, HitShape};
use crate::store::block::Block;
use three::{Matrix4, Quaternion, Vector3};

// The unit owns bind transforms and pose scratch. A seat retains only its
// resolved animation aliases and completed collision/drop publication.
pub(crate) struct Rider {
    life: f64, scale: f64, clips: Vec<F2RiderClip>,
    points: RiderPoints, shapes: [HitShape; 10], shape_count: usize, out: F2RiderOut,
    encoded: Block, previous: Block, dirty: bool,
}
pub(crate) struct Riders { rows: Vec<Option<Rider>>, output: Option<Block> }
impl Riders {
    pub(in crate::vehicles::module) const fn new() -> Self { Self { rows: Vec::new(), output: None } }
}
// UnitFactory prepares complete rider samplers alongside inactive bodies.
// Slot activation only moves buffers; a native first boarding cannot grow
// the row table or publication, nor ask the host to resolve an animation.
pub(crate) fn take_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>) -> Option<Rider> { world_ctx.fleet.riders.take(unit) }
pub(crate) fn install_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, rider: Option<Rider>) { world_ctx.fleet.riders.install(unit, rider) }
// SeatLease acquisition binds the new gameplay life before its first pose.
// This is the native equivalent of F2Riders.ensure's generation refresh.
pub(crate) fn life_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, life: f64) { world_ctx.fleet.riders.life(unit, life) }
pub fn f2_rider_birth_words() -> u32 { F2RiderBirth::WORDS as u32 }

/// riderPoints skeletonOf/bindsOf and the seat's resolved clip are birth data.

pub(crate) fn f2_rider_birth_in(world_ctx: &mut crate::world::owner::World) {
    let a = { let (row, heap) = world_ctx.sim.calls.args(); F2RiderBirth::take(row, heap) };
    world_ctx.fleet.riders.birth(a);
}

/// Vehicle.poseRider's geometry before firing, and syncCrews' later body write.
/// Keeping those slots distinct preserves lazy hit shapes during mounted fire.
pub(crate) fn hull_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, write_body: bool) {
    let Some(v) = world_ctx.sim.store.vehicle.get(r) else { return };
    if !v.alive { return; }
    let animal = super::live_seats::animal_in(&mut world_ctx.fleet, r);
    // aimTick/render already finished the world matrices for this pose.
    for index in 0..v.seats.len() {
        let p = world_ctx.fleet.poses[r.slot as usize].as_mut().unwrap();
        let seat = &v.seats[index];
        let Some(l) = seat.lease.and_then(|l| world_ctx.sim.store.seat_lease.get(l)) else { continue };
        if l.human { continue; }
        let Some(unit) = l.unit else { continue };
        let rider = p.pose.rig.seats[index].rider.as_ref();
        let node = rider.and_then(|r| r.node);
        // syncRider's hidden-seat fallback sits halfway up the hull. A shown
        // rider instead takes the complete seat-node frame, including binds.
        let mut point = vec(&v.position); let mut frame = Matrix4::default();
        if let Some(node) = node {
            frame = p.pose.rig.tree.node(node).matrix_world;
            point.set(frame.elements[12], frame.elements[13], frame.elements[14]);
        } else { point.y += v.size[1] * 0.5; }
        // ninePose keeps the original pilot pitch divisor and gunner arc
        // normalization. These weights also drive the page's visible pose.
        let view = &v.seat_views[index]; let sp = &p.pose.seats[index];
        let (pitch, turn) = if index == 0 { (-seat.pitch / 0.7, 0.0) } else {
            crate::vehicles::pose::nine_pose(view.pitch_limits, view.yaw_limits, view.wrap_yaw,
                p.turn_rates[index], crate::vehicles::hull::angle(seat.aim_yaw - v.yaw), seat.pitch, sp.yaw_rate)
        };
        let q = p.pose.rig.tree.node(p.pose.rig.root).quaternion;
        world_ctx.fleet.riders.sample(&mut world_ctx.units,&world_ctx.sim.store, &world_ctx.pose.clips, unit, Some(r), None, index, node.is_some(), point, frame, animal.then_some(q), pitch, turn, p.pose.age);
        // syncCrews writes bodies after hull firing, not at the earlier pose
        // sample. Keep tickCount in that slot: Unit.updateSpheres is lazy.
        if write_body && super::live_seats::has_actor_in(&mut world_ctx.fleet, unit) {
            let u = world_ctx.sim.store.unit.get_mut(unit).unwrap();
            if let Some(s) = crate::seats::bodies::soldier_mut(u) {
                s.position = v3(&point); s.previous = s.position; s.settled = s.position;
                s.velocity = v.velocity; s.aim_yaw = seat.aim_yaw; s.aim_pitch = seat.pitch;
            }
            u.tick_count += 1.0; super::live_seats::rider_posed_in(&mut world_ctx.fleet,&mut world_ctx.units,&world_ctx.sim.store, unit);
        }
    }
}

/// Turret.riderBody uses its seat's full world turn, including a bound drum.
pub(crate) fn turret_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, turret: Ref<Turret>, show: bool, point: Vector3, turn: Quaternion, pitch: f64, yaw: f64) {
    turret_sample(&mut world_ctx.units,&mut world_ctx.fleet.riders, &world_ctx.sim.store, &world_ctx.pose.clips, unit, turret, show, point, turn, pitch, yaw);
}
pub(super) fn turret_sample(units:&mut crate::units::owner::Units,riders: &mut Riders, store: &Store, clips: &[crate::pose::Clip], unit: Ref<Unit>, turret: Ref<Turret>, show: bool, point: Vector3, turn: Quaternion, pitch: f64, yaw: f64) {
    let mut frame = Matrix4::default(); frame.compose(&point, &turn, &Vector3::new(1.0, 1.0, 1.0));
    riders.sample(units,store, clips, unit, None, Some(turret), 0, show, point, frame, None, pitch, yaw, 0.0);
}
// Vehicles.vacate reads the last completed riderDropPoint. Looking it up
// must not sample a newer pose or advance the unit's hit-shape cache.
pub(crate) fn drop_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>) -> Option<Vector3> { world_ctx.fleet.riders.drop_point(unit) }

/// The unit hit test reads the already completed native rider sample.
/// Copying it does not sample a newer pose or advance Unit.spheresTick.
pub(crate) fn shapes_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, life: f64, out: &mut [crate::gen::HitShape]) -> usize { world_ctx.fleet.riders.shapes(unit, life, out) }

/// Publish only samples whose numeric values changed. A stationary nine-pose
/// rider keeps its shape cache while tickCount continues its normal cadence.
pub(crate) fn publish_in(world_ctx: &mut crate::world::owner::World) -> Option<&[f64]> { Some(world_ctx.fleet.riders.publish_words()) }

impl Riders {
    pub(super) fn reserve(&mut self, count: usize) {
        let count = count.max(self.rows.len());
        self.rows.resize_with(count, || None);
        let words = BlockHeader::WORDS + count * F2RiderOut::WORDS;
        let out = self.output.get_or_insert_with(Block::default);
        out.words.reserve(words.saturating_sub(out.words.len()));
    }
    fn take(&mut self, unit: Ref<Unit>) -> Option<Rider> {
        let row = self.rows.get_mut(unit.slot as usize)?;
        if row.as_ref()?.out.unit != unit { return None; }
        row.take()
    }
    fn install(&mut self, unit: Ref<Unit>, mut rider: Option<Rider>) {
        if let Some(row) = rider.as_mut() { row.out.unit = unit; }
        self.rows[unit.slot as usize] = rider;
    }
    fn life(&mut self, unit: Ref<Unit>, life: f64) {
        let Some(row) = self.rows.get_mut(unit.slot as usize).and_then(Option::as_mut) else { return };
        if row.out.unit != unit { return; }
        row.life = life; row.out.life = life;
    }
    fn drop_point(&self, unit: Ref<Unit>) -> Option<Vector3> {
        self.rows.get(unit.slot as usize).and_then(Option::as_ref)
            .filter(|r| r.out.unit == unit).and_then(|r| r.out.drop.as_ref()).map(vec)
    }
    fn shapes(&self, unit: Ref<Unit>, life: f64, out: &mut [crate::gen::HitShape]) -> usize {
        let Some(row) = self.rows.get(unit.slot as usize).and_then(Option::as_ref) else { return 0 };
        if row.out.unit != unit || row.life != life { return 0; }
        let count = row.shape_count;
        for (source, target) in row.shapes[..count].iter().zip(out) { target.set_rider(source); }
        count
    }
    pub(super) fn words(&self) -> &[f64] { &self.output.as_ref().expect("prepared rider output").words }
    pub(super) fn publish_words(&mut self) -> &[f64] {
        let mut count = 0;
        for row in self.rows.iter_mut().flatten() {
            if !row.dirty { continue; }
            row.encoded.begin(1, F2RiderOut::WORDS);
            let (words, heap) = row.encoded.row(0); row.out.put(words, heap); row.encoded.finish();
            // Compare bits as the boundary does, retaining signed zero. A dirty
            // sample can still be identical, so it need not cross into JavaScript.
            let same = row.encoded.words.len() == row.previous.words.len() && row.encoded.words.iter().zip(&row.previous.words).all(|(a,b)| a.to_bits() == b.to_bits());
            if same { row.dirty = false; } else { count += 1; }
        }
        let out = self.output.get_or_insert_with(Block::default); out.begin(count, F2RiderOut::WORDS); let mut at = 0;
        for row in self.rows.iter_mut().flatten() {
            if !row.dirty { continue; }
            let (words, heap) = out.row(at); row.out.put(words, heap); at += 1;
            // Keep both backing buffers. Clearing their logical counts reuses
            // storage; rebuilding encoded rows each tick would allocate.
            std::mem::swap(&mut row.previous, &mut row.encoded); row.dirty = false;
        }
        out.finish(); &out.words
    }
    fn birth(&mut self, a: F2RiderBirth) {
        let index = a.unit.slot as usize;
        if self.rows.len() <= index { self.rows.resize_with(index + 1, || None); }
        // A class activation installs the native unit sampler separately.
        // The rider row carries stable handles through holder changes.
        if self.rows[index].as_ref().is_none_or(|row|row.out.unit!=a.unit) {
            let out = F2RiderOut { unit: a.unit, life: a.life, clip: -1, ..Default::default() };
            self.rows[index] = Some(Rider { life: a.life, scale: a.scale, clips: Vec::with_capacity(16),
                points: RiderPoints::default(), shapes: [HitShape::default(); 10], shape_count: 0, out,
                encoded: Block { words: Vec::with_capacity(BlockHeader::WORDS + F2RiderOut::WORDS), heap: Vec::new() },
                previous: Block { words: Vec::with_capacity(BlockHeader::WORDS + F2RiderOut::WORDS), heap: Vec::new() }, dirty: false });
        }
        // riderSample resolves clips by holder and seat. Preserve those bindings
        // across seat moves; replacing a life must still refresh its generation.
        if let Some(row) = &mut self.rows[index] {
            row.life = a.life; row.out.life = a.life;
            for clip in a.clips {
                if let Some(old) = row.clips.iter_mut().find(|old| old.hull == clip.hull && old.turret == clip.turret && old.index == clip.index) { *old = clip; }
                else { row.clips.push(clip); }
            }
        }
        // Collision stays native. Reserve only the compact animation/drop
        // publication, so its first changed sample cannot grow the boundary.
        self.output.get_or_insert_with(Block::default).words.reserve(BlockHeader::WORDS + self.rows.len() * F2RiderOut::WORDS);
    }
    // riderBody/riderHitShapes share the frame used for death drops. Animal
    // clips put the pelvis on the saddle, leaving the root below.
    #[allow(clippy::too_many_arguments)]
    fn sample(&mut self,units:&mut crate::units::owner::Units, store: &Store, clips: &[crate::pose::Clip], unit: Ref<Unit>, hull: Option<Ref<Vehicle>>, turret: Option<Ref<Turret>>, index: usize,
        show: bool, point: Vector3, mut frame: Matrix4, animal: Option<Quaternion>, pitch: f64, turn: f64, time: f64) {
        let Some(row) = self.rows.get_mut(unit.slot as usize).and_then(Option::as_mut) else { return };
        let Some(u) = store.unit.get(unit) else { return };
        // Unit reuse must not attach the previous soldier's skeleton or clip.
        if row.out.unit != unit || u.generation != row.life { return; }
        let Some(binding) = row.clips.iter().find(|c| c.hull == hull && c.turret == turret && c.index as usize == index) else { return };
        // riderSample uses an explicit frame for stand-ins and a nine-pose for
        // steering clips. Quirk: timed clips ignore both pitch and turn weights.
        row.out.show = show; row.out.clip = binding.clip;
        row.out.pitch = if binding.time.is_some() || !binding.nine { 0.0 } else { pitch };
        row.out.turn = if binding.time.is_some() || !binding.nine { 0.0 } else { turn };
        row.out.time = binding.time.or_else(|| (!binding.nine).then_some(time));
        row.out.drop = None; row.shape_count = 0;
        if show && binding.clip >= 0 {
            let clip = &clips[binding.clip as usize];
            let sample = RiderSample { clip, pitch: row.out.pitch, turn: row.out.turn, time: row.out.time };
            if crate::units::live_pose::rider_points_in(units,unit, &sample, &mut row.points) {
                // seatRider subtracts the animated pelvis on riding animals.
                // Applying the root's height twice would lift the rider off it.
                if let Some(q) = animal {
                    let mut offset = row.points.pelvis; offset.apply_quaternion(&q);
                    let mut origin = point; origin.sub(&offset);
                    frame.compose(&origin, &q, &Vector3::new(1.0, 1.0, 1.0));
                }
                // riderDropPoint and riderHitShapes share this sample and frame.
                // A synchronous death must fall from exactly the body just hit.
                let mut drop = Vector3::default(); rider::rider_drop_point(&row.points, &mut drop); drop.apply_matrix4(&frame);
                row.out.drop = Some(v3(&drop));
                // Unit hit tests read this retained native prefix directly.
                // No shape list is serialized or decoded by the host.
                row.shape_count = rider::rider_hit_shapes(&row.points, &frame, Some(row.scale), &mut row.shapes);
            }
        }
        row.dirty = true;
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    pub fn publish() -> *const f64 { publish_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f2_rider_birth_words() -> u32 { super::f2_rider_birth_words() }
    #[no_mangle]
    pub extern "C" fn f2_rider_birth() { f2_rider_birth_in(crate::abi::facade::world()) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{f2_rider_birth};
#[cfg(target_arch = "wasm32")]
pub use exports::{publish};
