//! Private hull replay, using the battle's ground and flight models.
//! Each owner keeps its rows, hull, flight, walk state and strings. Queries
//! exchange owned numbers; no store or global vehicle phase is borrowed.

use crate::flight::config::Tuning;
use crate::flight::model::FlightInput;
use crate::flight::{Config, Environment, Flight, FlightClass, FlightWorld, Member, Team, Zone};
use crate::gen::{OrdnanceOwner, Vehicle};
use crate::pose::Clip;
use crate::rig::hull::{HullDesc, HullRig};
use crate::store::{Ref, Strings};
use crate::vehicles::class::HullClass;
use crate::vehicles::pose::{HullPose, WalkClip, WalkClips};
use crate::vehicles::{BodyId, Ground, Hit, Hull, Input, Polygon, VehicleWorld};
use bfe_content::value::{Object, Value};
use std::rc::Rc;
use three::{Matrix4, Quaternion, Vector3};

const INVALID: &str = "invalid prediction hull command";
const QUERY_FAILED: &str = "prediction hull query failed";

struct Reader<'a> {
    words: &'a [f64],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(words: &'a [f64]) -> Self {
        Self { words, at: 0 }
    }
    fn word(&mut self) -> Result<f64, &'static str> {
        let x = *self.words.get(self.at).ok_or(INVALID)?;
        self.at += 1;
        Ok(x)
    }
    fn count(&mut self) -> Result<usize, &'static str> {
        let n = self.word()?;
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {
            return Err(INVALID);
        }
        Ok(n as usize)
    }
    fn flag(&mut self) -> Result<bool, &'static str> {
        Ok(self.word()? != 0.0)
    }
    fn slice(&mut self, n: usize) -> Result<&'a [f64], &'static str> {
        let end = self.at.checked_add(n).ok_or(INVALID)?;
        let words = self.words.get(self.at..end).ok_or(INVALID)?;
        self.at = end;
        Ok(words)
    }
    fn text(&mut self) -> Result<Vec<u16>, &'static str> {
        let n = self.count()?;
        self.slice(n)?
            .iter()
            .map(|&x| {
                if x < 0.0 || x > 65535.0 || x.fract() != 0.0 {
                    Err(INVALID)
                } else {
                    Ok(x as u16)
                }
            })
            .collect()
    }
    fn value(&mut self, depth: usize) -> Result<Value, &'static str> {
        if depth > 16 {
            return Err(INVALID);
        }
        Ok(match self.count()? {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => Value::Number(self.word()?),
            4 => Value::String(Rc::from(String::from_utf16_lossy(&self.text()?))),
            5 => {
                let n = self.count()?;
                if n > self.words.len() - self.at {
                    return Err(INVALID);
                }
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.value(depth + 1)?);
                }
                Value::Array(Rc::new(items))
            }
            6 => {
                let n = self.count()?;
                if n > self.words.len() - self.at {
                    return Err(INVALID);
                }
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    let name = Rc::from(String::from_utf16_lossy(&self.text()?));
                    fields.push((name, self.value(depth + 1)?));
                }
                Value::Object(Rc::new(Object::from_written(fields)))
            }
            _ => return Err(INVALID),
        })
    }
    fn done(&self) -> Result<(), &'static str> {
        if self.at == self.words.len() {
            Ok(())
        } else {
            Err(INVALID)
        }
    }
}

fn walk_clip(r: &mut Reader<'_>) -> Result<Option<WalkClip>, &'static str> {
    if !r.flag()? {
        return Ok(None);
    }
    // Walk advances by duration and stride alone. No pose sampler is run
    // during prediction, so the private clip carries no animation tracks.
    Ok(Some(WalkClip {
        clip: Rc::new(Clip {
            frames: 0,
            fps: 0.0,
            root_motion: false,
            tracks: Vec::new(),
            data: Vec::new(),
        }),
        duration: r.word()?,
        stride: r.word()?,
    }))
}

fn config(r: &mut Reader<'_>) -> Result<Config, &'static str> {
    Ok(Config {
        min_speed: r.word()?,
        cruise_speed: r.word()?,
        max_speed: r.word()?,
        acceleration: r.word()?,
        turn_rate: r.word()?,
        turn_filter: r.word()?,
        pitch_rate: r.word()?,
        pitch_filter: r.word()?,
        bank_angle: r.word()?,
        bank_filter: r.word()?,
        level_filter: r.word()?,
        takeoff_height: r.word()?,
        takeoff_time: r.word()?,
        takeoff_speed: r.word()?,
        landed_height: r.word()?,
        landing_time: r.word()?,
        landing_speed: r.word()?,
        boost_speed: r.word()?,
    })
}

/// A query's reply starts with 1 for success, 0 for a caught foreign
/// exception. All following words belong to that query. Native twins
/// supply the same ordered replies without a JavaScript runtime.
pub trait Query {
    fn query(&mut self, op: u32, args: &[f64]) -> Vec<f64>;
}
impl<F: FnMut(u32, &[f64]) -> Vec<f64>> Query for F {
    fn query(&mut self, op: u32, args: &[f64]) -> Vec<f64> {
        self(op, args)
    }
}

/// Scalar operations shared with vehicle-prediction-sim.js:
/// 0: recursive content value (null, false, true, number, text, array,
/// object, tags 0..6; strings as UTF-16 units).
/// 1: string table, generated Vehicle full row, heap, walk pose flag and
/// phase/blend, two optional duration/stride pairs, then Config's 18 keys.
/// 2: ground input's four words, then flight input's numeric keys and
/// optional sticks. Captures the tick, steps ground twice or flight once,
/// and advances the walk cycle. Returns Vehicle export row, heap, walk
/// from/current phase/blend and optional weapon lock fields.
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    definition: Option<Value>,
    strings: Strings,
    row: Vehicle,
    hull: Option<Hull>,
}
impl Session {
    pub fn run<Q: Query>(&mut self, op: u32, query: &mut Q) -> Result<(), &'static str> {
        self.output.clear();
        let mut r = Reader::new(&self.input);
        match op {
            0 => {
                self.definition = Some(r.value(0)?);
                r.done()?;
            }
            1 => {
                let n = r.count()?;
                let mut strings = Strings::default();
                for i in 0..n {
                    if strings.intern(&r.text()?).0 as usize != i {
                        return Err(INVALID);
                    }
                }
                let row = r.slice(Vehicle::FULL_STRIDE)?;
                let n = r.count()?;
                let heap = r.slice(n)?;
                let mut v = Vehicle::default();
                v.take_full(row, heap);
                if v.seats.len() != v.seat_views.len() || v.seats.len() != v.seat_leases.len() {
                    return Err(INVALID);
                }
                let class = HullClass::new(self.definition.as_ref().ok_or(INVALID)?, v.turn_rate);
                let mut hull = Hull::from_store(Ref::new(0, 0), &v, &class, 0, &strings);
                let has_pose = r.flag()?;
                let walk_phase = r.word()?;
                let walk_blend = r.word()?;
                let left = walk_clip(&mut r)?;
                let right = walk_clip(&mut r)?;
                hull.pose = Some(HullPose {
                    rig: HullRig::new(&HullDesc::default()),
                    visual: Vector3::default(),
                    age: 0.0,
                    hover_phase: 0.0,
                    walk_phase,
                    walk_blend,
                    has_pose,
                    legs: false,
                    wheel_bones: Vec::new(),
                    seats: Vec::new(),
                    clips: Some(WalkClips {
                        left,
                        right,
                        ..WalkClips::default()
                    }),
                    leg_pairs: None,
                });
                hull.flight =
                    Flight::from_store(&v, &strings, config(&mut r)?, None).map_err(|_| INVALID)?;
                r.done()?;
                self.strings = strings;
                self.row = v;
                self.hull = Some(hull);
            }
            2 => {
                let input = Input {
                    forward: r.word()?,
                    right: r.word()?,
                    turn: r.word()?,
                    brake: r.flag()?,
                };
                let mut flight = FlightInput {
                    forward: r.word()?,
                    right: r.word()?,
                    turn: r.word()?,
                    pitch: r.word()?,
                    up: r.word()?,
                    down: r.word()?,
                    takeoff: r.word()?,
                    ..FlightInput::default()
                };
                flight.stick_x = if r.flag()? { Some(r.word()?) } else { None };
                flight.stick_y = if r.flag()? { Some(r.word()?) } else { None };
                flight.boost = r.word()?;
                flight.trick = r.word()?;
                flight.immelmann = r.word()?;
                flight.roll_lock = r.word()?;
                flight.target_lock = r.word()?;
                flight.bot = r.word()?;
                flight.response_curve = r.word()?;
                flight.no_assist = r.flag()?;
                r.done()?;
                let hull = self.hull.as_mut().ok_or(INVALID)?;
                let mut world = World {
                    query,
                    failed: false,
                    environment: Environment::default(),
                };
                hull.capture_tick();
                hull.capture_flight_tick();
                if hull.flying {
                    hull.flyer_tick(&mut world, 1.0 / 60.0, &flight, None);
                } else {
                    hull.tick(&mut world, 1.0 / 120.0, &input);
                    if world.failed {
                        return Err(QUERY_FAILED);
                    }
                    hull.tick(&mut world, 1.0 / 120.0, &input);
                }
                if let Some(sides) = hull.leg_side.as_mut() {
                    world.leg_side(hull.index, sides, false);
                }
                if world.failed {
                    return Err(QUERY_FAILED);
                }
                let p = hull.pose.as_ref().ok_or(INVALID)?;
                let from = [p.walk_phase, p.walk_blend];
                hull.walk(1.0 / 60.0);
                hull.to_store(&mut self.row, &mut self.strings);
                hull.flight.to_store(&mut self.row, &mut self.strings);
                let mut heap = Vec::new();
                self.output.resize(Vehicle::EXPORT_STRIDE, 0.0);
                self.row.put_export(&mut self.output, &mut heap);
                self.output.push(heap.len() as f64);
                self.output.extend(heap);
                let p = hull.pose.as_ref().ok_or(INVALID)?;
                self.output
                    .extend([from[0], from[1], p.walk_phase, p.walk_blend]);
                match hull.flight.lock {
                    None => self.output.push(0.0),
                    Some(lock) => self.output.extend([
                        1.0,
                        lock.target_id,
                        lock.generation,
                        lock.elapsed,
                        lock.complete as u8 as f64,
                    ]),
                }
            }
            _ => return Err(INVALID),
        }
        Ok(())
    }
}

fn vec(words: &[f64], at: usize) -> Vector3 {
    Vector3::new(num(words, at), num(words, at + 1), num(words, at + 2))
}
fn num(words: &[f64], at: usize) -> f64 {
    words.get(at).copied().unwrap_or(f64::NAN)
}
fn flag(words: &[f64], at: usize) -> bool {
    num(words, at) == 1.0
}
fn append(words: &mut Vec<f64>, v: &Vector3) {
    words.extend([v.x, v.y, v.z]);
}
fn quaternion(words: &mut Vec<f64>, q: &Quaternion) {
    words.extend([q.x, q.y, q.z, q.w]);
}
fn optional(words: &[f64], at: usize) -> Option<f64> {
    if flag(words, at) {
        Some(num(words, at + 1))
    } else {
        None
    }
}
fn body(n: f64) -> Option<BodyId> {
    if n >= 0.0 {
        Some(n as BodyId)
    } else {
        None
    }
}

struct World<'a, Q> {
    query: &'a mut Q,
    failed: bool,
    environment: Environment,
}
impl<Q: Query> World<'_, Q> {
    fn ask(&mut self, op: u32, args: &[f64]) -> Vec<f64> {
        if self.failed {
            return Vec::new();
        }
        let mut reply = self.query.query(op, args);
        if reply.first() != Some(&1.0) {
            self.failed = true;
            return Vec::new();
        }
        reply.remove(0);
        reply
    }
    fn forbidden(&mut self) {
        self.failed = true;
    }
}
impl<Q: Query> VehicleWorld for World<'_, Q> {
    fn damage_allowed(&self) -> bool {
        false
    }
    fn leg_side(&mut self, _: usize, sides: &mut Vec<f64>, publish: bool) {
        let mut a = vec![publish as u8 as f64];
        if publish {
            a.extend_from_slice(sides);
        }
        let values = self.ask(21, &a);
        if !self.failed {
            *sides = values;
        }
    }
    fn cast(
        &mut self,
        origin: &Vector3,
        direction: &Vector3,
        far: f64,
        ignore: Option<BodyId>,
    ) -> Option<Hit> {
        let mut a = Vec::with_capacity(8);
        append(&mut a, origin);
        append(&mut a, direction);
        a.extend([far, ignore.map_or(-1.0, f64::from)]);
        let v = self.ask(0, &a);
        if !flag(&v, 0) {
            return None;
        }
        Some(Hit {
            distance: num(&v, 1),
            point: vec(&v, 2),
            normal: vec(&v, 5),
            terrain: flag(&v, 8),
            body: body(num(&v, 9)),
        })
    }
    fn push_walls(
        &mut self,
        _: usize,
        probes: &[Vector3],
        radius: f64,
        q: &Quaternion,
        own: Option<&Quaternion>,
        previous: &Vector3,
        position: &mut Vector3,
        velocity: &mut Vector3,
        terrain: bool,
    ) -> bool {
        let mut a = vec![probes.len() as f64];
        for p in probes {
            append(&mut a, p);
        }
        a.push(radius);
        quaternion(&mut a, q);
        a.push(own.is_some() as u8 as f64);
        if let Some(q) = own {
            quaternion(&mut a, q);
        }
        append(&mut a, previous);
        append(&mut a, position);
        append(&mut a, velocity);
        a.push(terrain as u8 as f64);
        let v = self.ask(1, &a);
        if self.failed {
            return false;
        }
        *position = vec(&v, 1);
        *velocity = vec(&v, 4);
        flag(&v, 0)
    }
    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<Ground> {
        let v = self.ask(2, &[x, z, top]);
        if flag(&v, 0) {
            Some(Ground {
                height: num(&v, 1),
                normal: vec(&v, 2),
            })
        } else {
            None
        }
    }
    fn spring_polygons(
        &mut self,
        _: usize,
        index: usize,
        center: &Vector3,
        radius: f64,
        out: &mut Vec<Polygon>,
    ) {
        let mut a = vec![index as f64];
        append(&mut a, center);
        a.push(radius);
        let v = self.ask(3, &a);
        out.clear();
        let mut r = Reader::new(&v);
        let result = (|| {
            let n = r.count()?;
            for _ in 0..n {
                let id = r.word()?;
                let moving = r.flag()?;
                let n = r.count()?;
                let mut triangles = Vec::with_capacity(n.min(v.len() / 9));
                for _ in 0..n {
                    let t = r.slice(9)?;
                    triangles.push([vec(t, 0), vec(t, 3), vec(t, 6)]);
                }
                out.push(Polygon {
                    id,
                    moving,
                    triangles,
                });
            }
            r.done()
        })();
        if result.is_err() {
            self.failed = true;
            out.clear();
        }
    }
    fn move_body(&mut self, _: BodyId, _: &Matrix4) {}
    fn enable_body(&mut self, body: BodyId, enabled: bool) {
        self.ask(4, &[body as f64, enabled as u8 as f64]);
    }
    fn body_enabled(&mut self, body: BodyId) -> bool {
        flag(&self.ask(5, &[body as f64]), 0)
    }
    fn water_over(&mut self, x: f64, z: f64) -> bool {
        flag(&self.ask(6, &[x, z]), 0)
    }
    fn water_level(&mut self) -> f64 {
        num(&self.ask(7, &[]), 0)
    }
    fn has_water(&mut self) -> bool {
        flag(&self.ask(8, &[]), 0)
    }
    fn in_death_region(&mut self, position: &Vector3) -> bool {
        flag(&self.ask(9, &[position.x, position.y, position.z]), 0)
    }
    fn damaged(&mut self, _: usize, _: f64, _: &OrdnanceOwner) {
        self.forbidden();
    }
    fn destroyed_crew(&mut self, _: usize) {
        self.forbidden();
    }
    fn destroyed(&mut self, _: usize) {
        self.forbidden();
    }
    fn explosion(&mut self, _: usize, _: bool, _: &Vector3) {
        self.forbidden();
    }
    fn effect(&mut self, _: &str, _: &Vector3) {
        self.forbidden();
    }
    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<f64> {
        let mut a = Vec::new();
        append(&mut a, origin);
        append(&mut a, direction);
        a.push(far);
        optional(&self.ask(19, &a), 0)
    }
    fn clock(&mut self) -> f64 {
        num(&self.ask(20, &[]), 0)
    }
    fn tow_released(&mut self, _: usize, _: &str) {
        self.forbidden();
    }
}
impl<Q: Query> FlightWorld for World<'_, Q> {
    fn environment(&mut self) -> &Environment {
        let v = self.ask(10, &[]);
        let mut r = Reader::new(&v);
        let result = (|| {
            let ceiling = if r.flag()? { Some(r.word()?) } else { None };
            let max_fly_height = r.word()?;
            let bounds = if r.flag()? {
                let a = r.slice(6)?;
                Some(([a[0], a[1], a[2]], [a[3], a[4], a[5]]))
            } else {
                None
            };
            let space = r.flag()?;
            let landing_zones = if r.flag()? {
                let n = r.count()?;
                let mut zones = Vec::with_capacity(n.min(v.len() / 7));
                for _ in 0..n {
                    let a = r.slice(7)?;
                    zones.push(Zone {
                        min: [a[0], a[1], a[2]],
                        max: [a[3], a[4], a[5]],
                        floor_y: a[6],
                    });
                }
                Some(zones)
            } else {
                None
            };
            let assists = r.flag()?;
            r.done()?;
            Ok::<_, &'static str>(Environment {
                ceiling,
                max_fly_height,
                bounds,
                space,
                landing_zones,
                assists,
            })
        })();
        match result {
            Ok(env) => self.environment = env,
            Err(_) => self.failed = true,
        }
        &self.environment
    }
    fn tuning(&mut self, class: FlightClass) -> Tuning {
        let v = self.ask(11, &[class as u8 as f64]);
        if v.len() != 38 {
            self.failed = true;
        }
        let mut iter = v.iter();
        let mut n = || iter.next().copied().unwrap_or(f64::NAN);
        Tuning {
            boost_acceleration: n(),
            boost_turn: n(),
            stick_return: n(),
            response_curve: n(),
            landing_height: n(),
            max_slope: n(),
            power_drain: n(),
            power_recharge: n(),
            trick_cost: n(),
            trick_cooldown: n(),
            barrel_displacement: n(),
            missile_turn_rate: n(),
            missile_seeker: n(),
            missile_speed: n(),
            missile_lifetime: n(),
            missile_navigation: n(),
            missile_roll_break_rate: n(),
            missile_evade_range: n(),
            barrel_time: n(),
            loop_time: n(),
            immelmann_time: n(),
            trick_window: n(),
            terrain_lookahead: n(),
            recovery_sharpness: n(),
            lethal_impact_speed: n(),
            lethal_impact_cosine: n(),
            scrape_damage_limit: n(),
            impact_damage_speed: n(),
            assist_cone: n(),
            assist_strength: n(),
            sticky_scale: n(),
            camera_blend_time: n(),
            lock_range: n(),
            lock_cone: n(),
            retain_cone: n(),
            lock_dwell: n(),
            los_grace: n(),
            roll_rate: n(),
        }
    }
    fn member_count(&mut self) -> usize {
        num(&self.ask(12, &[]), 0) as usize
    }
    fn member(&mut self, i: usize) -> Member {
        let v = self.ask(13, &[i as f64]);
        Member {
            id: num(&v, 0),
            generation: optional(&v, 1),
            alive: flag(&v, 3),
            team: optional(&v, 4),
            own: flag(&v, 6),
            unit: flag(&v, 7),
            body: body(num(&v, 8)),
            point: vec(&v, 9),
        }
    }
    fn member_by_id(&mut self, id: f64) -> Option<usize> {
        let index = num(&self.ask(14, &[id]), 0);
        if index >= 0.0 {
            Some(index as usize)
        } else {
            None
        }
    }
    fn relation(&mut self, a: Team, b: Team) -> f64 {
        num(
            &self.ask(
                15,
                &[
                    a.is_some() as u8 as f64,
                    a.unwrap_or(0.0),
                    b.is_some() as u8 as f64,
                    b.unwrap_or(0.0),
                ],
            ),
            0,
        )
    }
    fn aim_point(&mut self, i: usize) -> Vector3 {
        vec(&self.ask(16, &[i as f64]), 0)
    }
    fn body_is_hull(&mut self, body: BodyId) -> bool {
        flag(&self.ask(17, &[body as f64]), 0)
    }
    fn system_death_region(&mut self, position: &Vector3) -> bool {
        flag(&self.ask(18, &[position.x, position.y, position.z]), 0)
    }
}

/// The outer owner remains at a stable address while the entire session
/// is taken out for a command. Reentrant calls can create another owner;
/// an attempt to reuse this busy owner cannot alias its working hull.
#[derive(Default)]
pub struct Handle {
    session: Option<Session>,
    input: Vec<f64>,
    output: Vec<f64>,
    error: Option<&'static str>,
}

#[no_mangle]
pub extern "C" fn prediction_hulls_create() -> *mut Handle {
    Box::into_raw(Box::new(Handle {
        session: Some(Session::default()),
        ..Handle::default()
    }))
}
#[no_mangle]
pub unsafe extern "C" fn prediction_hulls_drop(ptr: *mut Handle) {
    if !ptr.is_null() && (&*ptr).session.is_some() {
        drop(Box::from_raw(ptr));
    }
}
#[no_mangle]
pub unsafe extern "C" fn prediction_hulls_room(ptr: *mut Handle, words: usize) -> *mut f64 {
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    let h = &mut *ptr;
    if h.session.is_none() {
        return std::ptr::null_mut();
    }
    h.input.resize(words, 0.0);
    h.input.as_mut_ptr()
}
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn prediction_hulls_run(ptr: *mut Handle, op: u32, serial: f64) -> u32 {
    if ptr.is_null() {
        return 0;
    }
    let mut session = {
        let h = &mut *ptr;
        let Some(mut session) = h.session.take() else {
            return 0;
        };
        session.input = std::mem::take(&mut h.input);
        session
    };
    let mut query = |op, args: &[f64]| {
        crate::gen::import_prediction_hull_query(&crate::gen::PredictionHullQuery {
            session: serial,
            op,
            args: args.to_vec(),
        })
        .values
    };
    let error = session.run(op, &mut query).err();
    let h = &mut *ptr;
    h.output = std::mem::take(&mut session.output);
    h.input = std::mem::take(&mut session.input);
    h.error = error;
    h.session = Some(session);
    error.is_none() as u32
}
#[no_mangle]
pub unsafe extern "C" fn prediction_hulls_output(ptr: *const Handle) -> *const f64 {
    if ptr.is_null() {
        std::ptr::null()
    } else {
        (&*ptr).output.as_ptr()
    }
}
#[no_mangle]
pub unsafe extern "C" fn prediction_hulls_output_len(ptr: *const Handle) -> usize {
    if ptr.is_null() {
        0
    } else {
        (&*ptr).output.len()
    }
}
#[no_mangle]
pub unsafe extern "C" fn prediction_hulls_error(ptr: *const Handle) -> *const u8 {
    (&*ptr).error.unwrap_or("").as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn prediction_hulls_error_len(ptr: *const Handle) -> usize {
    (&*ptr).error.unwrap_or("").len()
}
