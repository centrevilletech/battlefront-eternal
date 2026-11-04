//! Immutable hit history and fractional rewind, from runtime/hit-history.js.
//! Captures retain 49 frames by default. JavaScript supplies live references
//! and lazy hit-shape reads; storage, interpolation and intersection are native.

use crate::units::shapes::{
    arr, segment_shape, segment_sphere, shape_normal, v3, Half, HitShape, ShapePart, ShapeType,
};
use std::rc::Rc;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Extent {
    #[default]
    Missing,
    Number(f64),
    Vector([f64; 3]),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    /// The four native shape codes, or an unknown type (which cannot hit).
    pub kind: u32,
    /// An opaque token, preserving arbitrary JavaScript part values.
    pub part: u32,
    pub center: [f64; 3],
    pub radius: Option<f64>,
    pub yaw: Option<f64>,
    pub half: Extent,
}

impl Shape {
    fn geometric(&self) -> Option<HitShape> {
        Some(HitShape {
            r#type: ShapeType::from_u8(u8::try_from(self.kind).ok()?)?,
            part: ShapePart::Body,
            center: self.center,
            radius: self.radius.unwrap_or(f64::NAN),
            half: match self.half {
                Extent::Missing => Half::F64(f64::NAN),
                Extent::Number(v) => Half::F64(v),
                Extent::Vector(v) => Half::Vec3(v),
            },
            yaw: self.yaw.unwrap_or(f64::NAN),
            half_size: [0.0; 3],
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub key: u32,
    pub generation: Option<f64>,
    pub position: [f64; 3],
    pub center: [f64; 3],
    pub radius: f64,
    pub shapes: Vec<Shape>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub tick: f64,
    pub entries: Vec<Entry>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            tick: -1.0,
            entries: Vec::new(),
        }
    }
}

pub struct HitHistory {
    pub max_ticks: f64,
    pub tick: f64,
    frames: Vec<Rc<Frame>>,
    interpolated: Rc<Frame>,
    sampled_at: f64,
}

fn lerp(a: [f64; 3], b: [f64; 3], alpha: f64) -> [f64; 3] {
    let mut v = v3(a);
    v.lerp(&v3(b), alpha);
    arr(&v)
}

fn number_lerp(a: Option<f64>, b: Option<f64>, alpha: f64) -> Option<f64> {
    a.map(|a| a + (b.unwrap_or(f64::NAN) - a) * alpha)
}

fn extent_lerp(a: &Extent, b: &Extent, alpha: f64) -> Extent {
    match a {
        Extent::Missing => Extent::Missing,
        Extent::Number(a) => {
            let b = match b {
                Extent::Number(v) => *v,
                _ => f64::NAN,
            };
            Extent::Number(a + (b - a) * alpha)
        }
        Extent::Vector(a) => {
            let b = match b {
                Extent::Vector(v) => *v,
                _ => [f64::NAN; 3],
            };
            Extent::Vector(lerp(*a, b, alpha))
        }
    }
}

impl HitHistory {
    pub fn new(max_ticks: f64) -> Self {
        Self {
            max_ticks,
            tick: -1.0,
            frames: (0..(max_ticks + 1.0) as usize)
                .map(|_| Rc::new(Frame::default()))
                .collect(),
            interpolated: Rc::new(Frame::default()),
            sampled_at: -1.0,
        }
    }

    fn index(&self, tick: f64) -> Option<usize> {
        let i = tick % self.frames.len() as f64;
        (i.is_finite() && i >= 0.0 && i.fract() == 0.0).then_some(i as usize)
    }

    pub fn capture(&mut self, tick: f64, entries: &[Entry]) {
        let index = self
            .index(tick)
            .expect("a nonnegative integer history tick");
        let frame = Rc::make_mut(&mut self.frames[index]);
        frame.tick = tick;
        frame.entries.truncate(entries.len());
        for (i, entry) in entries.iter().enumerate() {
            if i < frame.entries.len() {
                frame.entries[i].clone_from(entry);
            } else {
                frame.entries.push(entry.clone());
            }
        }
        self.tick = tick;
        self.sampled_at = -1.0;
    }

    pub fn frame(&self, tick: f64) -> Option<&Frame> {
        let frame = self.frames.get(self.index(tick)?)?;
        (frame.tick == tick).then_some(frame.as_ref())
    }

    pub fn sample(&mut self, tick: f64) -> Option<&Frame> {
        if tick.is_finite() && tick.fract() == 0.0 {
            return self.frame(tick);
        }
        if !tick.is_finite() {
            return None;
        }
        if self.sampled_at == tick {
            return Some(&self.interpolated);
        }
        let from_index = self.index(tick.floor())?;
        let to_index = self.index(tick.ceil())?;
        let from = &self.frames[from_index];
        let to = &self.frames[to_index];
        if from.tick != tick.floor() || to.tick != tick.ceil() {
            return None;
        }
        let interpolated = Rc::make_mut(&mut self.interpolated);
        interpolated.tick = tick;
        let out = &mut interpolated.entries;
        let mut count = 0;
        let mut shift = 0isize;
        for (k, a) in from.entries.iter().enumerate() {
            let hint = k as isize + shift;
            let at = if hint >= 0
                && to
                    .entries
                    .get(hint as usize)
                    .is_some_and(|e| e.key == a.key)
            {
                Some(hint as usize)
            } else {
                to.entries.iter().position(|e| e.key == a.key)
            };
            let Some(at) = at else { continue };
            shift = at as isize - k as isize;
            let b = &to.entries[at];
            if a.generation != b.generation
                || v3(a.position).distance_to_squared(&v3(b.position)) > 2.5 * 2.5
            {
                continue;
            }
            let mut alpha = if a.shapes.len() == b.shapes.len() {
                tick - tick.floor()
            } else {
                0.0
            };
            if alpha != 0.0
                && a.shapes
                    .iter()
                    .zip(&b.shapes)
                    .any(|(a, b)| a.kind != b.kind || a.part != b.part)
            {
                alpha = 0.0;
            }
            if count == out.len() {
                out.push(a.clone());
            }
            let e = &mut out[count];
            count += 1;
            e.key = a.key;
            e.generation = a.generation;
            e.position = lerp(a.position, b.position, alpha);
            e.center = lerp(a.center, b.center, alpha);
            e.radius = a.radius + (b.radius - a.radius) * alpha;
            e.shapes.clone_from(&a.shapes);
            for (i, first) in a.shapes.iter().enumerate() {
                let next = if alpha != 0.0 { &b.shapes[i] } else { first };
                let saved = &mut e.shapes[i];
                saved.center = lerp(first.center, next.center, alpha);
                saved.radius = number_lerp(first.radius, next.radius, alpha);
                saved.yaw = first.yaw.map(|yaw| {
                    let delta = next.yaw.unwrap_or(f64::NAN) - yaw;
                    yaw + v8math::atan2(v8math::sin(delta), v8math::cos(delta)) * alpha
                });
                saved.half = extent_lerp(&first.half, &next.half, alpha);
            }
        }
        out.truncate(count);
        self.sampled_at = tick;
        Some(&self.interpolated)
    }

    pub fn sample_owned(&mut self, tick: f64) -> Option<Rc<Frame>> {
        self.sample(tick)?;
        if tick.fract() == 0.0 {
            Some(self.frames[self.index(tick)?].clone())
        } else {
            Some(self.interpolated.clone())
        }
    }

    pub fn position(&mut self, tick: f64, key: u32, generation: Option<f64>) -> Option<[f64; 3]> {
        self.sample(tick)?
            .entries
            .iter()
            .find(|e| e.key == key && e.generation == generation)
            .map(|e| e.position)
    }

    pub fn shot_tick(
        &mut self,
        view: f64,
        key: u32,
        generation: Option<f64>,
        allowed: bool,
        lead: f64,
        playout: f64,
    ) -> Option<f64> {
        if !allowed || !view.is_finite() || view < 0.0 || view > self.tick {
            return None;
        }
        let tick = v8math::max(
            view,
            self.tick - v8math::min(self.max_ticks, rewind_budget_ticks(lead, playout)),
        );
        self.position(tick, key, generation).map(|_| tick)
    }
}

pub fn rewind_budget_ticks(lead: f64, playout: f64) -> f64 {
    let lead = if lead.is_finite() {
        v8math::max(0.0, v8math::min(24.0, lead))
    } else {
        24.0
    };
    let playout = if playout.is_finite() {
        v8math::max(0.0, v8math::min(10.0, playout))
    } else {
        10.0
    };
    18.0 + lead + playout + 12.0 + 2.0
}

/// A query owns its sampled frame. The host evaluates live-unit predicates
/// between consider calls, with no borrow into the history across a callback.
pub struct Raycast {
    pub frame: Rc<Frame>,
    origin: [f64; 3],
    direction: [f64; 3],
    best: f64,
    found: Option<(u32, Shape)>,
    result: [f64; 9],
}

impl Raycast {
    pub fn new(
        frame: impl Into<Rc<Frame>>,
        origin: [f64; 3],
        direction: [f64; 3],
        far: f64,
    ) -> Self {
        Self {
            frame: frame.into(),
            origin,
            direction,
            best: far,
            found: None,
            result: [0.0; 9],
        }
    }

    pub fn consider(&mut self, index: usize) {
        let entry = &self.frame.entries[index];
        let origin = v3(self.origin);
        let direction = v3(self.direction);
        if segment_sphere(
            &origin,
            &direction,
            self.best,
            &v3(entry.center),
            entry.radius,
        )
        .is_none()
        {
            return;
        }
        for candidate in &entry.shapes {
            let Some(geometry) = candidate.geometric() else {
                continue;
            };
            if let Some(distance) = segment_shape(&origin, &direction, self.best, &geometry) {
                if distance < self.best {
                    self.best = distance;
                    self.found = Some((entry.key, candidate.clone()));
                }
            }
        }
    }

    /// key, part, distance, point.xyz, normal.xyz.
    pub fn finish(&mut self) -> Option<&[f64; 9]> {
        let (key, shape) = self.found.as_ref()?;
        let direction = v3(self.direction);
        let mut point = v3(self.origin);
        point.add_scaled_vector(&direction, self.best);
        let normal = shape_normal(&point, &shape.geometric()?, &direction);
        self.result = [
            *key as f64,
            shape.part as f64,
            self.best,
            point.x,
            point.y,
            point.z,
            normal.x,
            normal.y,
            normal.z,
        ];
        Some(&self.result)
    }
}

/// Flat frame exchange: tick, count, then each entry's key, optional
/// generation (present,value), position.xyz, center.xyz, radius, shape count.
/// A shape is type, part, center.xyz, optional radius, optional yaw, then
/// half's tag (0 missing, 1 scalar, 2 vector) and three component words.
/// Buffers belong to this session. The bridge refreshes views after calls.
#[cfg(target_arch = "wasm32")]
pub struct Session {
    pub history: HitHistory,
    input: Vec<f64>,
    output: Vec<f64>,
}

#[cfg(any(target_arch = "wasm32", test))]
fn read_optional(words: &[f64], at: usize) -> Option<f64> {
    (words[at] != 0.0).then_some(words[at + 1])
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_entries_into(words: &[f64], out: &mut Vec<Entry>) {
    let mut at = 2;
    out.resize_with(words[1] as usize, Entry::default);
    for e in out.iter_mut() {
        let count = words[at + 10] as usize;
        e.key = words[at] as u32;
        e.generation = read_optional(words, at + 1);
        e.position = [words[at + 3], words[at + 4], words[at + 5]];
        e.center = [words[at + 6], words[at + 7], words[at + 8]];
        e.radius = words[at + 9];
        e.shapes.resize_with(count, Shape::default);
        at += 11;
        for shape in &mut e.shapes {
            *shape = Shape {
                kind: words[at] as u32,
                part: words[at + 1] as u32,
                center: [words[at + 2], words[at + 3], words[at + 4]],
                radius: read_optional(words, at + 5),
                yaw: read_optional(words, at + 7),
                half: match words[at + 9] as u32 {
                    0 => Extent::Missing,
                    1 => Extent::Number(words[at + 10]),
                    _ => Extent::Vector([words[at + 10], words[at + 11], words[at + 12]]),
                },
            };
            at += 13;
        }
    }
    assert_eq!(at, words.len(), "history frame length");
}

#[cfg(any(target_arch = "wasm32", test))]
fn encode_frame(frame: &Frame, out: &mut Vec<f64>) {
    out.clear();
    out.extend([frame.tick, frame.entries.len() as f64]);
    for e in &frame.entries {
        out.extend([
            e.key as f64,
            e.generation.is_some() as u8 as f64,
            e.generation.unwrap_or(0.0),
        ]);
        out.extend(e.position);
        out.extend(e.center);
        out.extend([e.radius, e.shapes.len() as f64]);
        for s in &e.shapes {
            out.extend([s.kind as f64, s.part as f64]);
            out.extend(s.center);
            out.extend([
                s.radius.is_some() as u8 as f64,
                s.radius.unwrap_or(0.0),
                s.yaw.is_some() as u8 as f64,
                s.yaw.unwrap_or(0.0),
            ]);
            match s.half {
                Extent::Missing => out.extend([0.0; 4]),
                Extent::Number(v) => out.extend([1.0, v, 0.0, 0.0]),
                Extent::Vector(v) => {
                    out.push(2.0);
                    out.extend(v);
                }
            }
        }
    }
}

// The pointer ABI is confined to wasm. Native callers own HitHistory and
// Raycast values directly; parallel native tests share no mutable globals.
#[cfg(target_arch = "wasm32")]
mod module {
    use super::*;

    #[no_mangle]
    pub extern "C" fn history_create(max_ticks: f64) -> *mut Session {
        Box::into_raw(Box::new(Session {
            history: HitHistory::new(max_ticks),
            input: Vec::new(),
            output: Vec::new(),
        }))
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_drop(ptr: *mut Session) {
        if !ptr.is_null() {
            drop(Box::from_raw(ptr));
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_room(ptr: *mut Session, words: u32) -> *mut f64 {
        let s = &mut *ptr;
        s.input.resize(words as usize, 0.0);
        s.input.as_mut_ptr()
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_capture(ptr: *mut Session) {
        let s = &mut *ptr;
        let tick = s.input[0];
        let index = s
            .history
            .index(tick)
            .expect("a nonnegative integer history tick");
        let frame = Rc::make_mut(&mut s.history.frames[index]);
        frame.tick = tick;
        decode_entries_into(&s.input, &mut frame.entries);
        s.history.tick = tick;
        s.history.sampled_at = -1.0;
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_sample(
        ptr: *mut Session,
        tick: f64,
        exact: u32,
    ) -> *const f64 {
        let s = &mut *ptr;
        let frame = if exact != 0 {
            s.history.frame(tick)
        } else {
            s.history.sample(tick)
        };
        let Some(frame) = frame else {
            return std::ptr::null();
        };
        encode_frame(frame, &mut s.output);
        s.output.as_ptr()
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_shot_tick(
        ptr: *mut Session,
        view: f64,
        key: u32,
        has_generation: u32,
        generation: f64,
        allowed: u32,
        lead: f64,
        playout: f64,
    ) -> f64 {
        (*ptr)
            .history
            .shot_tick(
                view,
                key,
                (has_generation != 0).then_some(generation),
                allowed != 0,
                lead,
                playout,
            )
            .unwrap_or(f64::NAN)
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_create(
        ptr: *mut Session,
        tick: f64,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        far: f64,
    ) -> *mut Raycast {
        let Some(frame) = (*ptr).history.sample_owned(tick) else {
            return std::ptr::null_mut();
        };
        Box::into_raw(Box::new(Raycast::new(
            frame,
            [ox, oy, oz],
            [dx, dy, dz],
            far,
        )))
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_count(ptr: *mut Raycast) -> u32 {
        let ray = &*ptr;
        ray.frame.entries.len() as u32
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_key(ptr: *mut Raycast, index: u32) -> u32 {
        let ray = &*ptr;
        ray.frame.entries[index as usize].key
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_generation(
        ptr: *mut Raycast,
        index: u32,
        has_generation: u32,
        generation: f64,
    ) -> u32 {
        let ray = &*ptr;
        (ray.frame.entries[index as usize].generation
            == (has_generation != 0).then_some(generation)) as u32
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_test(ptr: *mut Raycast, index: u32) {
        (*ptr).consider(index as usize);
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_vectors(
        ptr: *mut Raycast,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
    ) {
        (*ptr).origin = [ox, oy, oz];
        (*ptr).direction = [dx, dy, dz];
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_finish(ptr: *mut Raycast) -> *const f64 {
        (*ptr).finish().map_or(std::ptr::null(), |a| a.as_ptr())
    }

    #[no_mangle]
    pub unsafe extern "C" fn history_ray_drop(ptr: *mut Raycast) {
        if !ptr.is_null() {
            drop(Box::from_raw(ptr));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(x: f64) -> Entry {
        Entry {
            key: 1,
            generation: Some(1.0),
            position: [x, 0.0, 4.0],
            center: [x, 0.0, 4.0],
            radius: 1.0,
            shapes: vec![Shape {
                kind: 0,
                part: 7,
                center: [x, 0.0, 4.0],
                radius: Some(0.5),
                yaw: None,
                half: Extent::Missing,
            }],
        }
    }

    #[test]
    fn retains_49_frames_and_interpolates_only_same_life_nearby() {
        let mut h = HitHistory::new(48.0);
        for tick in 0..50 {
            h.capture(tick as f64, &[entry(tick as f64 / 10.0)]);
        }
        assert!(h.frame(0.0).is_none());
        assert_eq!(h.frame(1.0).unwrap().entries[0].position[0], 0.1);
        assert_eq!(h.sample(48.5).unwrap().entries[0].position[0], 4.85);
        h.capture(50.0, &[entry(20.0)]);
        assert!(h.sample(49.5).unwrap().entries.is_empty());
        let mut e = entry(20.0);
        e.generation = Some(2.0);
        h.capture(51.0, &[e]);
        assert!(h.sample(50.5).unwrap().entries.is_empty());
    }

    #[test]
    fn old_topology_and_undefined_shape_fields_are_preserved() {
        let mut h = HitHistory::new(48.0);
        h.capture(0.0, &[entry(0.0)]);
        let mut next = entry(1.0);
        next.shapes[0].part = 9;
        h.capture(1.0, &[next]);
        assert_eq!(h.sample(0.5).unwrap().entries[0], entry(0.0));
    }

    #[test]
    fn rewind_budget_generation_and_ray_ties_follow_history_order() {
        let mut h = HitHistory::new(48.0);
        for tick in 0..=80 {
            h.capture(tick as f64, &[entry(0.0)]);
        }
        assert_eq!(
            h.shot_tick(0.0, 1, Some(1.0), true, f64::NAN, f64::NAN),
            Some(32.0)
        );
        assert_eq!(h.shot_tick(0.0, 1, Some(2.0), true, 0.0, 0.0), None);
        assert_eq!(h.shot_tick(81.0, 1, Some(1.0), true, 0.0, 0.0), None);
        assert_eq!(rewind_budget_ticks(-5.0, f64::NAN), 42.0);
        let mut frame = h.sample(79.5).unwrap().clone();
        let mut e = entry(0.0);
        e.key = 2;
        frame.entries.push(e);
        let mut ray = Raycast::new(frame, [0.0; 3], [0.0, 0.0, 1.0], 10.0);
        ray.consider(0);
        ray.consider(1);
        assert_eq!(
            *ray.finish().unwrap(),
            [1.0, 7.0, 3.5, 0.0, 0.0, 3.5, 0.0, 0.0, -1.0]
        );
    }

    #[test]
    fn exchange_round_trip_keeps_optional_values_and_parts() {
        let frame = Frame {
            tick: 12.0,
            entries: vec![entry(-0.0)],
        };
        let mut words = Vec::new();
        encode_frame(&frame, &mut words);
        let mut entries = Vec::new();
        decode_entries_into(&words, &mut entries);
        assert_eq!(entries, frame.entries);
        assert_eq!(entries[0].position[0].to_bits(), (-0.0f64).to_bits());
    }
}
