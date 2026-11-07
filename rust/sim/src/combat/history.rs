//! HitHistory (runtime/hit-history.js). Samples never move live bodies back
//! in time. Each ring frame retains its entry and shape storage across ticks.
use crate::gen::{Half, HitShape};
use crate::units::shapes::{segment_shape, segment_sphere, shape_normal};
use three::Vector3;

// HitHistory's saved shapes are used only for rewind intersection. The live
// HitShape also keeps halfSize for JS publication, duplicating a vector Half.
// Keep one full-precision half here; reconstruct the ray input on the stack.
#[derive(Clone, Debug)]
pub struct SavedShape {
    pub r#type: crate::gen::ShapeType,
    pub part: crate::gen::ShapePart,
    pub center: crate::store::Vec3,
    pub radius: f64,
    pub half: Half,
    pub yaw: f64,
}
impl From<&HitShape> for SavedShape {
    fn from(shape: &HitShape) -> Self {
        Self {
            r#type:shape.r#type, part:shape.part, center:shape.center,
            radius:shape.radius, half:match &shape.half {
                Half::F64(v)=>Half::F64(*v), Half::Vec3(v)=>Half::Vec3(*v),
            }, yaw:shape.yaw,
        }
    }
}
impl SavedShape {
    fn ray_shape(&self) -> HitShape {
        HitShape {
            r#type:self.r#type,part:self.part,center:self.center,radius:self.radius,
            half:match &self.half {Half::F64(v)=>Half::F64(*v),Half::Vec3(v)=>Half::Vec3(*v)},
            yaw:self.yaw,half_size:[0.0;3],
        }
    }
}

/// One captured life, in that frame's registry order. Shape storage is pooled:
/// stance changes alter count, rather than dropping capacity every capture.
#[derive(Clone, Debug)]
pub struct Entry<K> {
    pub key: K,
    pub life: f64,
    pub position: Vector3,
    pub center: Vector3,
    pub radius: f64,
    pub shapes: Vec<SavedShape>,
}
impl<K: Copy> Entry<K> {
    pub fn new(key: K, capacity: usize) -> Self {
        Self {
            key,
            life: 0.0,
            position: Vector3::default(),
            center: Vector3::default(),
            radius: 0.0,
            shapes: Vec::with_capacity(capacity),
        }
    }
}
struct Frame<K> {
    tick: f64,
    entries: Vec<Entry<K>>,
    count: usize,
}
impl<K> Default for Frame<K> {
    fn default() -> Self {
        Self {
            tick: -1.0,
            entries: Vec::new(),
            count: 0,
        }
    }
}

/// Live eligibility is tested again at hit time. A captured body cannot be hit
/// after a respawn, nor can an earlier exposed seat make a hidden rider hittable.
pub trait World {
    type Key: Copy + PartialEq;
    fn count(&self) -> usize;
    fn key(&self, index: usize) -> Self::Key;
    fn capture(&mut self, key: Self::Key, out: &mut Entry<Self::Key>) -> bool;
    fn eligible(&mut self, key: Self::Key, life: f64, riders: bool) -> bool;
}

#[derive(Debug, PartialEq)]
pub struct Hit<K> {
    pub key: K,
    pub distance: f64,
    pub point: Vector3,
    pub normal: Vector3,
    pub part: crate::gen::ShapePart,
}

pub struct History<K> {
    frames: Vec<Frame<K>>,
    interpolated: Frame<K>,
    sampled_at: f64,
    pub tick: f64,
    max_ticks: f64,
}
impl<K: Copy + PartialEq> History<K> {
    // HitHistory retains one extra frame so both ends of its oldest accepted
    // fractional tick can coexist. Negative ticks are never ring indices.
    pub fn new(max_ticks: usize) -> Self {
        Self {
            frames: (0..=max_ticks).map(|_| Frame::default()).collect(),
            interpolated: Frame::default(),
            sampled_at: -1.0,
            tick: -1.0,
            max_ticks: max_ticks as f64,
        }
    }

    /// Reserve when a UnitFactory body is built, including bodies still pooled.
    /// The first full battle frame must not discover shape capacity in a tick.
    pub fn reserve(&mut self, capacity: usize, key: K, shapes: usize) {
        for frame in self.frames.iter_mut().chain(std::iter::once(&mut self.interpolated)) {
            while frame.entries.len() < capacity {
                frame.entries.push(Entry::new(key, shapes));
            }
        }
    }

    /// capture (hit-history.js): ask sphere even for an enclosed rider; its
    /// empty strike shape list is separate from the broadphase observation.
    pub fn capture<W: World<Key = K>>(&mut self, tick: u32, world: &mut W) {
        let index = tick as usize % self.frames.len();
        // Overwrite this ring slot in place. An old frame is identified by its
        // tick stamp, so a wrap never makes an unrelated old tick appear valid.
        let frame = &mut self.frames[index];
        frame.tick = tick as f64;
        frame.count = 0;
        for i in 0..world.count() {
            let key = world.key(i);
            assert!(
                frame.count < frame.entries.len(),
                "hit history reserved at construction"
            );
            // Rejected members do not consume a pool entry. This leaves saved
            // samples in exactly the order capture's original loop produced.
            let entry = &mut frame.entries[frame.count];
            if world.capture(key, entry) {
                frame.count += 1;
            }
        }
        self.tick = tick as f64;
        self.sampled_at = -1.0;
    }

    // frame(tick) checks the saved stamp after modulo. The stamp matters when
    // a caller asks beyond the retained window or before the first capture.
    fn frame_index(&self, tick: f64) -> Option<usize> {
        if tick < 0.0 || !tick.is_finite() || tick.fract() != 0.0 {
            return None;
        }
        let index = tick as usize % self.frames.len();
        (self.frames[index].tick == tick).then_some(index)
    }

    /// sample (hit-history.js): keep earlier topology through a stance change;
    /// skip teleports and missing/rebound lives instead of sweeping between them.
    fn sample(&mut self, tick: f64) -> Option<&Frame<K>> {
        if tick.fract() == 0.0 {
            return self.frame_index(tick).map(|i| &self.frames[i]);
        }
        if !tick.is_finite() {
            return None;
        }
        if self.sampled_at == tick {
            return Some(&self.interpolated);
        }
        // Integer samples return their exact frame without interpolation.
        // Repeated fractional samples share the same retained result until the
        // next capture invalidates sampledAt, just as HitHistory.sample does.
        let lo = tick.floor();
        let from = &self.frames[self.frame_index(lo)?];
        let to = &self.frames[self.frame_index(tick.ceil())?];
        let out = &mut self.interpolated;
        out.tick = tick;
        out.count = 0;
        // indexOf tries the previous insertion/removal shift first. It is an
        // optimization of the original ordered search, never a sorted lookup.
        let mut shift: isize = 0;
        for (index, a) in from.entries[..from.count].iter().enumerate() {
            let hint = index as isize + shift;
            let at = if hint >= 0 && (hint as usize) < to.count && to.entries[hint as usize].key == a.key {
                Some(hint as usize)
            } else {
                to.entries[..to.count].iter().position(|b| b.key == a.key)
            };
            let Some(at) = at else { continue };
            shift = at as isize - index as isize;
            // A pooled object can occur at the same place in both lists yet
            // represent different lives. Position proximity cannot merge them.
            let b = &to.entries[at];
            if a.life != b.life || a.position.distance_to_squared(&b.position) > 2.5 * 2.5 {
                continue;
            }
            // A topology change has no limb correspondence. Preserve the
            // earlier full sample, including its sphere and body position.
            let mut alpha = if a.shapes.len() == b.shapes.len() {
                tick - lo
            } else {
                0.0
            };
            if alpha != 0.0
                && a.shapes
                    .iter()
                    .zip(&b.shapes)
                    .any(|(x, y)| x.r#type != y.r#type || x.part != y.part)
            {
                alpha = 0.0;
            }
            let e = &mut out.entries[out.count];
            out.count += 1;
            e.key = a.key;
            e.life = a.life;
            e.position = a.position;
            e.position.lerp(&b.position, alpha);
            e.center = a.center;
            e.center.lerp(&b.center, alpha);
            e.radius = a.radius + (b.radius - a.radius) * alpha;
            // Copy fixed numeric shapes into their retained sample storage.
            // No class definition, live skeleton or physics body is cloned.
            e.shapes.clear();
            for (i, first) in a.shapes.iter().enumerate() {
                let next = if alpha != 0.0 { &b.shapes[i] } else { first };
                let mut saved = SavedShape {
                    r#type:first.r#type,part:first.part,center:first.center,radius:first.radius,
                    half:match &first.half {Half::F64(v)=>Half::F64(*v),Half::Vec3(v)=>Half::Vec3(*v)},yaw:first.yaw,
                };
                let mut center = v(first.center);
                center.lerp(&v(next.center), alpha);
                saved.center = a3(center);
                saved.radius = first.radius + (next.radius - first.radius) * alpha;
                // Yaw crosses the short arc through +/- pi. A direct lerp
                // would rotate the saved limb the long way around that seam.
                let angle = next.yaw - first.yaw;
                let (sin,cos)=v8math::sin_cos(angle);
                saved.yaw = first.yaw + v8math::atan2(sin, cos) * alpha;
                // Boxes/capsules keep vector half extents, while cylinders
                // keep a scalar half-height. Saved vectors own their values;
                // they never alias a future live shape's mutable halfSize.
                match (&first.half, &next.half) {
                    (Half::Vec3(a), Half::Vec3(b)) => {
                        let mut half = v(*a);
                        half.lerp(&v(*b), alpha);
                        saved.half = Half::Vec3(a3(half));
                    }
                    (Half::F64(a), Half::F64(b)) => saved.half = Half::F64(a + (b - a) * alpha),
                    _ => unreachable!("a shape's topology fixes its half kind"),
                }
                e.shapes.push(saved);
            }
        }
        self.sampled_at = tick;
        Some(out)
    }

    /// shotTick: the owner's pipeline bounds the rewind. A stale edge cannot
    /// retain an old rendered tick, and the shooter must occur in that life.
    #[allow(clippy::too_many_arguments)]
    pub fn shot_tick(
        &mut self,
        owner: K,
        life: f64,
        network: bool,
        alive: bool,
        seated: bool,
        view: f64,
        lead: Option<f64>,
        playout: Option<f64>,
    ) -> Option<f64> {
        if !network || !alive || seated || !view.is_finite() || view < 0.0 || view > self.tick {
            return None;
        }
        // The budget includes round trip, accepted input lead, playout delay,
        // its snap allowance and two ticks. Its cap is still the history window.
        let budget = 18.0 + bounded(lead, 24.0) + bounded(playout, 10.0) + 12.0 + 2.0;
        let tick = v8math::max(view, self.tick - v8math::min(self.max_ticks, budget));
        let frame = self.sample(tick)?;
        frame.entries[..frame.count]
            .iter()
            .any(|e| e.key == owner && e.life == life)
            .then_some(tick)
    }

    // HitHistory.position is a read of the saved life. It never substitutes a
    // newly spawned body's current position when the requested sample is gone.
    pub fn position(&mut self, key: K, life: f64, tick: f64) -> Option<Vector3> {
        let frame = self.sample(tick)?;
        frame.entries[..frame.count]
            .iter()
            .find(|e| e.key == key && e.life == life)
            .map(|e| e.position)
    }

    /// raycast: exact ties retain the earlier frame member and shape. Eligibility
    /// uses current life/seat facts, while intersection uses only the saved pose.
    pub fn raycast<W: World<Key = K>>(
        &mut self,
        tick: f64,
        origin: &Vector3,
        dir: &Vector3,
        far: f64,
        exclude: Option<K>,
        riders: bool,
        world: &mut W,
        filter: &mut impl FnMut(K) -> bool,
    ) -> Option<Hit<K>> {
        self.raycast_with(tick, origin, dir, far, exclude, riders, world, &mut |_, key| filter(key))
    }

    /// Predicate services share the caller's observer instead of capturing
    /// another world borrow while the history owns its interpolation frame.
    #[allow(clippy::too_many_arguments)]
    pub fn raycast_with<W: World<Key = K>>(
        &mut self,
        tick: f64,
        origin: &Vector3,
        dir: &Vector3,
        far: f64,
        exclude: Option<K>,
        riders: bool,
        world: &mut W,
        filter: &mut impl FnMut(&mut W, K) -> bool,
    ) -> Option<Hit<K>> {
        let frame = self.sample(tick)?;
        // Broadphase precedes individual shapes, as in Units.raycast. Strict
        // improvement keeps the first member and shape on an exact tie.
        let mut best = far;
        let mut found = None;
        for entry in &frame.entries[..frame.count] {
            if !world.eligible(entry.key, entry.life, riders) || Some(entry.key) == exclude || !filter(world, entry.key) {
                continue;
            }
            if segment_sphere(origin, dir, best, &entry.center, entry.radius).is_none() {
                continue;
            }
            for shape in &entry.shapes {
                if let Some(distance) = segment_shape(origin, dir, best, &shape.ray_shape()) {
                    if distance < best {
                        best = distance;
                        found = Some((entry.key, shape));
                    }
                }
            }
        }
        // The point and normal come from the selected saved shape. Only the
        // eligibility checks above read current state; live geometry stays put.
        let (key, shape) = found?;
        let mut point = *origin;
        point.add_scaled_vector(dir, best);
        Some(Hit {
            key,
            distance: best,
            point,
            normal: shape_normal(&point, &shape.ray_shape(), dir),
            part: shape.part,
        })
    }
}
// rewindBudgetTicks accepts finite client measurements only. Missing or
// nonfinite lead/playout use their normal maximum, rather than zero delay.
fn bounded(value: Option<f64>, max: f64) -> f64 {
    value
        .filter(|v| v.is_finite())
        .map_or(max, |v| v8math::max(0.0, v8math::min(max, v)))
}
fn v(a: [f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}
fn a3(v: Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
