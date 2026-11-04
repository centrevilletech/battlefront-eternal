//! Private recon replay. A command yields owned query or callback records;
//! the caller resumes it after foreign code has finished. No Rust call or
//! accessible working-state reference remains on the foreign call stack.

use crate::combat::defs::{Js, JsObj, Level, WeaponClass, WeaponDefs};
use crate::combat::weapons::{Admission, Aim, Shot, ShotKind, Trigger, WeaponDraws};
use crate::gen::{Posture, Weapon};
use crate::store::DefId;
use jsrt::rng::ClassicRandom;
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};
use three::{Euler, EulerOrder, Quaternion, Triangle, Vector3};
use v8math::{atan, cos, exp, hypot, max, min, sin, tan};

const INVALID: &str = "invalid recon prediction command";
const FOREIGN: &str = "recon prediction callback failed";
const RADIUS: f64 = 0.271;
const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    max(lo, min(hi, x))
}
fn vector(a: &[f64]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}
fn put_vec(a: &mut Vec<f64>, v: &Vector3) {
    a.extend([v.x, v.y, v.z]);
}
fn put_quat(a: &mut Vec<f64>, q: &Quaternion) {
    a.extend([q.x, q.y, q.z, q.w]);
}
fn truth(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}
fn or_zero(x: f64) -> f64 {
    if truth(x) {
        x
    } else {
        0.0
    }
}

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
        let x = self.word()?;
        if !x.is_finite() || x < 0.0 || x.fract() != 0.0 || x > u32::MAX as f64 {
            Err(INVALID)
        } else {
            Ok(x as usize)
        }
    }
    fn flag(&mut self) -> Result<bool, &'static str> {
        Ok(self.word()? != 0.0)
    }
    fn slice(&mut self, n: usize) -> Result<&'a [f64], &'static str> {
        let end = self.at.checked_add(n).ok_or(INVALID)?;
        let x = self.words.get(self.at..end).ok_or(INVALID)?;
        self.at = end;
        Ok(x)
    }
    fn list(&mut self) -> Result<Vec<f64>, &'static str> {
        let n = self.count()?;
        Ok(self.slice(n)?.to_vec())
    }
    fn vec(&mut self) -> Result<Vector3, &'static str> {
        Ok(vector(self.slice(3)?))
    }
    fn quat(&mut self) -> Result<Quaternion, &'static str> {
        let a = self.slice(4)?;
        Ok(Quaternion::new(a[0], a[1], a[2], a[3]))
    }
    fn text(&mut self) -> Result<Rc<str>, &'static str> {
        let a = self.list()?;
        Ok(Rc::from(String::from_utf16_lossy(
            &a.iter().map(|&x| x as u16).collect::<Vec<_>>(),
        )))
    }
    fn value(&mut self, depth: usize) -> Result<Js, &'static str> {
        if depth > 16 {
            return Err(INVALID);
        }
        Ok(match self.count()? {
            0 => Js::Undefined,
            1 => Js::Null,
            2 => Js::Bool(false),
            3 => Js::Bool(true),
            4 => Js::Num(self.word()?),
            5 => Js::Str(self.text()?),
            6 => {
                let n = self.count()?;
                let mut a = Vec::new();
                for _ in 0..n {
                    a.push(self.value(depth + 1)?);
                }
                Js::Arr(Rc::new(a))
            }
            7 => {
                let n = self.count()?;
                let mut fields = Vec::new();
                for _ in 0..n {
                    fields.push((self.text()?, self.value(depth + 1)?));
                }
                Js::Obj(Rc::new(JsObj { fields, def: None }))
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

#[derive(Default)]
struct Definitions {
    values: Vec<Js>,
    weapons: WeaponDefs,
}
impl Definitions {
    fn get(&self, id: usize) -> Result<Js, &'static str> {
        self.values.get(id).cloned().ok_or(INVALID)
    }
}

#[derive(Clone, Default)]
struct Camera {
    enabled: bool,
    position: Vector3,
    q: Quaternion,
    aspect: f64,
    fov: f64,
    reach: f64,
    fresh: bool,
    hfov: f64,
    first_fov: f64,
    vertical_fov: f64,
    motion_offset: Vector3,
    motion_turn: Vector3,
    sections: f64,
    eye: Vector3,
    center: Vector3,
    offset: Vector3,
    pivot: Vector3,
    target: Vector3,
    boom: Vector3,
    local: Vector3,
    right: Vector3,
    up: Vector3,
    dir: Vector3,
    want: Quaternion,
    inverse: Quaternion,
    euler: Vector3,
    tilt: f64,
}
#[derive(Clone)]
struct Salvo {
    def: usize,
    at: Vector3,
    x: Vector3,
    y: Vector3,
    z: Vector3,
    left: f64,
    wait: f64,
}
#[derive(Clone)]
struct Droid {
    def: usize,
    position: Vector3,
    velocity: usize,
    eye: Vector3,
    aim: Vector3,
    target: Vector3,
    yaw: f64,
    pitch: f64,
    health: f64,
    dead: bool,
    countdown: f64,
    selected: [f64; 2],
    weapons: Vec<usize>,
    channels: [Vec<usize>; 2],
    salvo: Option<usize>,
}
struct Core {
    defs: Rc<RefCell<Definitions>>,
    droids: Vec<Droid>,
    lists: Vec<Vec<usize>>,
    order: usize,
    strikes: usize,
    piloted: Option<usize>,
    look_x: f64,
    look_y: f64,
    zoom: f64,
    first: f64,
    rng: usize,
    streams: Vec<ClassicRandom>,
    velocities: Vec<Vector3>,
    salvos: Vec<Salvo>,
    weapons: Vec<Weapon>,
    reported_levels: Vec<f64>,
    level_ids: Vec<Vec<usize>>,
    weapon_rng: Vec<Option<usize>>,
    camera: Camera,
    scratch: [Vector3; 5],
    closest: Vector3,
    on_fired: bool,
}
impl Core {
    fn read(defs: Rc<RefCell<Definitions>>, words: &[f64]) -> Result<Self, &'static str> {
        let mut r = Reader::new(words);
        let look_x = r.word()?;
        let look_y = r.word()?;
        let zoom = r.word()?;
        let first = r.word()?;
        let pilot = r.word()?;
        let rng = r.count()?;
        let n = r.count()?;
        let mut streams = Vec::new();
        for _ in 0..n {
            streams.push(ClassicRandom::new(r.word()?));
        }
        if rng >= streams.len() {
            return Err(INVALID);
        }
        let on_fired = r.flag()?;
        let camera = Camera {
            enabled: r.flag()?,
            position: r.vec()?,
            q: r.quat()?,
            aspect: r.word()?,
            fov: r.word()?,
            reach: r.word()?,
            fresh: r.flag()?,
            hfov: r.word()?,
            first_fov: r.word()?,
            vertical_fov: r.word()?,
            motion_offset: r.vec()?,
            motion_turn: r.vec()?,
            sections: r.word()?,
            eye: r.vec()?,
            center: r.vec()?,
            offset: r.vec()?,
            pivot: r.vec()?,
            target: r.vec()?,
            boom: r.vec()?,
            local: r.vec()?,
            right: r.vec()?,
            up: r.vec()?,
            dir: r.vec()?,
            want: r.quat()?,
            inverse: r.quat()?,
            euler: r.vec()?,
            tilt: r.word()?,
        };
        let scratch = [r.vec()?, r.vec()?, r.vec()?, r.vec()?, r.vec()?];
        let closest = r.vec()?;
        let n = r.count()?;
        let mut weapons = Vec::new();
        let mut reported_levels = Vec::new();
        let mut level_ids = Vec::new();
        let mut weapon_rng = Vec::new();
        for _ in 0..n {
            let w = Weapon::take(r.slice(Weapon::WORDS)?, &[]);
            if !defs.borrow().weapons.has(w.def) {
                return Err(INVALID);
            }
            weapons.push(w);
            reported_levels.push(r.word()?);
            level_ids.push(
                r.list()?
                    .into_iter()
                    .map(|x| x as usize)
                    .collect::<Vec<_>>(),
            );
            let stream = r.word()?;
            weapon_rng.push(if stream < 0.0 {
                None
            } else {
                Some(stream as usize)
            });
        }
        let n = r.count()?;
        let mut velocities = Vec::new();
        for _ in 0..n {
            velocities.push(r.vec()?);
        }
        let n = r.count()?;
        let mut salvos = Vec::new();
        for _ in 0..n {
            salvos.push(Salvo {
                def: r.count()?,
                at: r.vec()?,
                x: r.vec()?,
                y: r.vec()?,
                z: r.vec()?,
                left: r.word()?,
                wait: r.word()?,
            });
        }
        let n = r.count()?;
        let mut droids = Vec::new();
        for _ in 0..n {
            let def = r.count()?;
            defs.borrow().get(def)?;
            let position = r.vec()?;
            let velocity = r.count()?;
            let eye = r.vec()?;
            let aim = r.vec()?;
            let target = r.vec()?;
            let yaw = r.word()?;
            let pitch = r.word()?;
            let health = r.word()?;
            let dead = r.flag()?;
            let countdown = r.word()?;
            let selected = [r.word()?, r.word()?];
            let weapons = r.list()?.into_iter().map(|x| x as usize).collect();
            let channels = [
                r.list()?.into_iter().map(|x| x as usize).collect(),
                r.list()?.into_iter().map(|x| x as usize).collect(),
            ];
            let salvo = r.word()?;
            let salvo = if salvo < 0.0 {
                None
            } else {
                Some(salvo as usize)
            };
            droids.push(Droid {
                def,
                position,
                velocity,
                eye,
                aim,
                target,
                yaw,
                pitch,
                health,
                dead,
                countdown,
                selected,
                weapons,
                channels,
                salvo,
            });
        }
        let n = r.count()?;
        let mut lists = Vec::new();
        for _ in 0..n {
            lists.push(
                r.list()?
                    .into_iter()
                    .map(|x| x as usize)
                    .collect::<Vec<_>>(),
            );
        }
        let order = r.count()?;
        let strikes = r.count()?;
        r.done()?;
        if order >= lists.len() || strikes >= lists.len() {
            return Err(INVALID);
        }
        let piloted = if pilot >= 0.0 {
            Some(pilot as usize)
        } else {
            None
        };
        if lists
            .iter()
            .flatten()
            .copied()
            .chain(piloted)
            .any(|i| i >= droids.len())
        {
            return Err(INVALID);
        }
        Ok(Self {
            defs,
            droids,
            lists,
            order,
            strikes,
            piloted,
            look_x,
            look_y,
            zoom,
            first,
            rng,
            velocities,
            salvos,
            streams,
            weapons,
            reported_levels,
            level_ids,
            weapon_rng,
            camera,
            scratch,
            closest,
            on_fired,
        })
    }
    fn write(&self) -> Vec<f64> {
        let c = &self.camera;
        let mut a = vec![
            self.look_x,
            self.look_y,
            self.zoom,
            self.first,
            self.piloted.map_or(-1.0, |x| x as f64),
            self.rng as f64,
            self.streams.len() as f64,
        ];
        a.extend(self.streams.iter().map(|r| r.seed()));
        a.extend([self.on_fired as u8 as f64, c.enabled as u8 as f64]);
        put_vec(&mut a, &c.position);
        put_quat(&mut a, &c.q);
        a.extend([
            c.aspect,
            c.fov,
            c.reach,
            c.fresh as u8 as f64,
            c.hfov,
            c.first_fov,
            c.vertical_fov,
        ]);
        put_vec(&mut a, &c.motion_offset);
        put_vec(&mut a, &c.motion_turn);
        a.push(c.sections);
        for v in [
            &c.eye, &c.center, &c.offset, &c.pivot, &c.target, &c.boom, &c.local, &c.right, &c.up,
            &c.dir,
        ] {
            put_vec(&mut a, v);
        }
        put_quat(&mut a, &c.want);
        put_quat(&mut a, &c.inverse);
        put_vec(&mut a, &c.euler);
        a.push(c.tilt);
        for v in &self.scratch {
            put_vec(&mut a, v);
        }
        put_vec(&mut a, &self.closest);
        a.push(self.weapons.len() as f64);
        for (i, w) in self.weapons.iter().enumerate() {
            let mut row = [0.0; Weapon::WORDS];
            w.put(&mut row, &mut Vec::new());
            a.extend(row);
            a.push(self.reported_levels[i]);
            a.push(self.level_ids[i].len() as f64);
            a.extend(self.level_ids[i].iter().map(|&id| id as f64));
            a.push(self.weapon_rng[i].map_or(-1.0, |id| id as f64));
        }
        a.push(self.velocities.len() as f64);
        for v in &self.velocities {
            put_vec(&mut a, v);
        }
        a.push(self.salvos.len() as f64);
        for s in &self.salvos {
            a.push(s.def as f64);
            for v in [&s.at, &s.x, &s.y, &s.z] {
                put_vec(&mut a, v);
            }
            a.extend([s.left, s.wait]);
        }
        a.push(self.droids.len() as f64);
        for d in &self.droids {
            a.push(d.def as f64);
            put_vec(&mut a, &d.position);
            a.push(d.velocity as f64);
            for v in [&d.eye, &d.aim, &d.target] {
                put_vec(&mut a, v);
            }
            a.extend([
                d.yaw,
                d.pitch,
                d.health,
                d.dead as u8 as f64,
                d.countdown,
                d.selected[0],
                d.selected[1],
            ]);
            for list in [&d.weapons, &d.channels[0], &d.channels[1]] {
                a.push(list.len() as f64);
                a.extend(list.iter().map(|&id| id as f64));
            }
            a.push(d.salvo.map_or(-1.0, |id| id as f64));
        }
        a.push(self.lists.len() as f64);
        for list in &self.lists {
            a.push(list.len() as f64);
            a.extend(list.iter().map(|&i| i as f64));
        }
        a.extend([self.order as f64, self.strikes as f64]);
        a
    }
    fn def(&self, i: usize) -> Js {
        self.defs.borrow().values[self.droids[i].def].clone()
    }
    fn current(&self, i: usize, channel: usize) -> Option<usize> {
        let d = &self.droids[i];
        let selected = d.selected[channel];
        if selected >= 0.0 && selected.fract() == 0.0 {
            d.channels[channel].get(selected as usize).copied()
        } else {
            None
        }
    }
    fn level(&self, w: usize) -> Js {
        self.defs.borrow().values[self.level_ids[w][self.weapons[w].level as usize]].clone()
    }
    fn zoom_value(&self) -> f64 {
        self.piloted
            .and_then(|i| self.current(i, 0))
            .map_or(1.0, |w| {
                if truth(self.zoom) {
                    self.level(w).get("ZoomMin").or(1.0)
                } else {
                    1.0
                }
            })
    }
    fn exit(&mut self) {
        self.piloted = None;
        self.camera.enabled = false;
        self.zoom = 0.0;
    }
    fn kill(&mut self, i: usize) {
        if self.droids[i].dead {
            return;
        }
        self.droids[i].dead = true;
        self.droids[i].health = 0.0;
        if self.piloted == Some(i) {
            self.exit();
        }
    }
    fn hurt(&mut self, i: usize, amount: f64) {
        if self.droids[i].dead || !(amount > 0.0) {
            return;
        }
        self.droids[i].health -= amount;
        if self.droids[i].health <= 0.0 {
            self.kill(i);
        }
    }
}

#[derive(Default)]
struct Reply {
    ok: bool,
    values: Vec<f64>,
    state: Vec<f64>,
}
struct Request {
    op: u32,
    args: Vec<f64>,
    state: Vec<f64>,
}
#[derive(Default)]
struct Exchange {
    request: Option<Request>,
    reply: Option<Reply>,
}
#[derive(Clone)]
struct World(Rc<RefCell<Exchange>>);
impl World {
    async fn request(&self, op: u32, args: Vec<f64>, state: Vec<f64>) -> Reply {
        self.0.borrow_mut().request = Some(Request { op, args, state });
        poll_fn(|_| {
            self.0
                .borrow_mut()
                .reply
                .take()
                .map_or(Poll::Pending, Poll::Ready)
        })
        .await
    }
}
impl Core {
    async fn ask(
        &mut self,
        world: &World,
        op: u32,
        args: Vec<f64>,
    ) -> Result<Vec<f64>, &'static str> {
        let reply = world.request(op, args, self.write()).await;
        if !reply.state.is_empty() {
            *self = Core::read(self.defs.clone(), &reply.state)?;
        }
        if reply.ok {
            Ok(reply.values)
        } else {
            Err(FOREIGN)
        }
    }
    async fn cast(
        &mut self,
        world: &World,
        from: &mut Vector3,
        direction: &mut Vector3,
        far: f64,
        tag: u32,
        droid: usize,
    ) -> Result<Option<Hit>, &'static str> {
        match tag {
            0 => {}
            1 | 2 => {
                self.scratch[0] = *from;
                self.scratch[2] = *direction;
            }
            3 => {
                self.scratch[0] = *from;
                self.scratch[4] = *direction;
            }
            5 => {
                self.scratch[3] = *from;
            }
            _ => {}
        }
        let mut a = Vec::new();
        put_vec(&mut a, from);
        put_vec(&mut a, direction);
        a.extend([far, tag as f64, droid as f64]);
        let a = self.ask(world, 0, a).await?;
        let mut r = Reader::new(&a);
        let hit = if r.flag()? {
            Some(Hit {
                distance: r.word()?,
                point: r.vec()?,
                normal: r.vec()?,
            })
        } else {
            None
        };
        *from = r.vec()?;
        *direction = r.vec()?;
        Ok(hit)
    }
    async fn take(&mut self, world: &World, action: u32) -> Result<bool, &'static str> {
        Ok(self.ask(world, 4, vec![action as f64]).await?.first() == Some(&1.0))
    }
}

struct Hit {
    distance: f64,
    point: Vector3,
    normal: Vector3,
}

impl Core {
    async fn clearance(&mut self, world: &World) -> Result<f64, &'static str> {
        if self.camera.boom.length_sq() < 1e-8 {
            return Ok(1.0);
        }
        self.camera.right = Vector3::new(0.4, 0.0, 0.0);
        self.camera.right.apply_quaternion(&self.camera.q);
        self.camera.up = Vector3::new(0.0, 0.4, 0.0);
        self.camera.up.apply_quaternion(&self.camera.q);
        let mut reach = 1.0;
        for (a, b) in [
            (0.0, 0.0),
            (1.0, 1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
        ] {
            self.camera.dir = self.camera.boom;
            self.camera
                .dir
                .add_scaled_vector(&self.camera.right, a)
                .add_scaled_vector(&self.camera.up, b);
            let far = self.camera.dir.length();
            self.camera.dir.divide_scalar(far);
            let mut pivot = self.camera.pivot;
            let mut dir = self.camera.dir;
            if let Some(hit) = self
                .cast(world, &mut pivot, &mut dir, far + 0.08, 0, 0)
                .await?
            {
                reach = min(reach, max(0.0, hit.distance - 0.08) / far);
            }
        }
        Ok(reach)
    }
    async fn camera_update(
        &mut self,
        world: &World,
        droid: &Droid,
        _def: &Js,
        dt: f64,
        zoom: f64,
        first: bool,
    ) -> Result<(), &'static str> {
        if first {
            self.scratch[1] = droid.eye;
            self.scratch[1].add_scaled_vector(&droid.aim, if zoom > 1.0 { -0.5 } else { -0.3 });
        }
        let sections = self.defs.borrow().values[self.camera.sections as usize].clone();
        let section = if zoom > 1.0 && sections.get("STANDZOOM").truthy() {
            sections.get("STANDZOOM")
        } else {
            sections.get("STAND")
        };
        self.camera.eye = if section.get("EyePointOffset").truthy() {
            js_vec(section.get("EyePointOffset"))
        } else if section.get("TrackCenter").truthy() {
            js_vec(section.get("TrackCenter"))
        } else {
            Vector3::new(0.0, 1.8, 0.0)
        };
        self.camera.center = if section.get("TrackCenter").truthy() {
            js_vec(section.get("TrackCenter"))
        } else if section.get("EyePointOffset").truthy() {
            js_vec(section.get("EyePointOffset"))
        } else {
            Vector3::new(0.0, 1.8, 0.0)
        };
        self.camera.offset = if section.get("TrackOffset").truthy() {
            js_vec(section.get("TrackOffset"))
        } else {
            Vector3::new(0.0, 0.0, 3.5)
        };
        self.camera.tilt = section.get("TiltValue").nn(5.0) * (std::f64::consts::PI / 180.0);
        let c = &mut self.camera;
        let base = if first || zoom > 1.0 {
            c.first_fov
        } else {
            c.hfov
        };
        let want = 2.0 * atan(tan(base / 2.0) / zoom);
        c.fov += (want - c.fov) * (1.0 - exp(-25.0 * dt));
        c.vertical_fov = (2.0 * atan(tan(c.fov / 2.0) / c.aspect)) * (180.0 / std::f64::consts::PI);
        self.ask(world, 9, vec![]).await?;
        let c = &mut self.camera;
        if first {
            c.euler = Vector3::new(droid.pitch, droid.yaw + std::f64::consts::PI, 0.0);
            c.q.set_from_euler(&Euler::new(
                c.euler.x,
                c.euler.y,
                c.euler.z,
                EulerOrder::YXZ,
            ));
            c.position = self.scratch[1];
            c.local = c.motion_offset;
            c.local.apply_quaternion(&c.q);
            c.position.add(&c.local);
            c.want.set_from_euler(&Euler::new(
                c.motion_turn.x,
                c.motion_turn.y,
                c.motion_turn.z,
                EulerOrder::ZYX,
            ));
            c.q.multiply(&c.want);
            c.fresh = true;
            return Ok(());
        }
        let (sn, cs) = (sin(droid.yaw), cos(droid.yaw));
        c.pivot = Vector3::new(
            droid.position.x + sn * c.center.z - cs * c.center.x,
            droid.position.y + c.center.y,
            droid.position.z + cs * c.center.z + sn * c.center.x,
        );
        c.euler = Vector3::new(droid.pitch - c.tilt, droid.yaw + std::f64::consts::PI, 0.0);
        c.want.set_from_euler(&Euler::new(
            c.euler.x,
            c.euler.y,
            c.euler.z,
            EulerOrder::YXZ,
        ));
        if c.fresh || c.position.distance_to(&c.pivot) > 80.0 {
            c.q = c.want;
        } else {
            c.q.slerp(&c.want, 1.0 - exp(-30.0 * dt));
        }
        c.boom = c.offset;
        c.boom.apply_quaternion(&c.q);
        c.target = c.pivot;
        c.target.add(&c.boom);
        let reach = self.clearance(world).await?;
        let c = &mut self.camera;
        if reach < c.reach {
            c.reach = reach;
        } else {
            c.reach += (reach - c.reach) * (1.0 - exp(-6.0 * dt));
        }
        c.target = c.pivot;
        c.target.add_scaled_vector(&c.boom, c.reach);
        if c.fresh || c.position.distance_to(&c.target) > 80.0 {
            c.position = c.target;
            c.fresh = false;
            return Ok(());
        }
        let k = 1.0 - exp(-15.0 * dt);
        c.inverse = c.q;
        c.inverse.invert();
        c.local = c.target;
        c.local
            .sub(&c.position)
            .apply_quaternion(&c.inverse)
            .multiply_scalar(k);
        c.local.apply_quaternion(&c.q);
        c.position.add(&c.local);
        c.boom = c.position;
        c.boom.sub(&c.pivot);
        let clear = self.clearance(world).await?;
        let c = &mut self.camera;
        if clear < 1.0 {
            c.position = c.pivot;
            c.position.add_scaled_vector(&c.boom, clear);
        }
        Ok(())
    }
}

fn js_vec(v: &Js) -> Vector3 {
    Vector3::new(v.at(0).num(), v.at(1).num(), v.at(2).num())
}

struct Draws<'a> {
    streams: &'a mut [ClassicRandom],
    streams_by_weapon: &'a [Option<usize>],
    rng: usize,
    levels: &'a mut [f64],
}
impl WeaponDraws for Draws<'_> {
    fn draw(&mut self, weapon: u32) -> f64 {
        self.streams[self.streams_by_weapon[weapon as usize].unwrap_or(self.rng)].uniform()
    }
    fn legacy(&mut self, _: u32) -> f64 {
        self.streams[self.rng].uniform()
    }
    fn seeded(&mut self, _: f64, _: f64) {}
    fn admit(&mut self, _: u32) -> Admission {
        Admission::Yes
    }
    fn level_written(&mut self, weapon: u32, level: f64) {
        self.levels[weapon as usize] = level;
    }
}

impl Core {
    async fn shoot(
        &mut self,
        world: &World,
        i: usize,
        dt: f64,
        controlled: bool,
    ) -> Result<(), &'static str> {
        let primary = if controlled {
            self.take(world, 5).await?
        } else {
            false
        };
        let secondary = if controlled {
            self.take(world, 6).await?
        } else {
            false
        };
        let held = if controlled {
            self.ask(world, 5, vec![0.0]).await?.first() == Some(&1.0)
        } else {
            false
        };
        let primary_trigger = Trigger {
            held: held || primary,
            pressed: primary,
            ..Trigger::default()
        };
        let aim = Aim {
            stick: 0.0,
            posture: Some(Posture::Stand),
            zoomed: self.zoom_value() > 1.0,
            repair_target: false,
        };
        let mut shots = Vec::new();
        {
            let primary = self.current(i, 0);
            let secondary = self.current(i, 1);
            let second_busy = secondary.is_some_and(|w| self.weapons[w].busy());
            let defs = self.defs.borrow();
            let mut draws = Draws {
                streams: &mut self.streams,
                streams_by_weapon: &self.weapon_rng,
                rng: self.rng,
                levels: &mut self.reported_levels,
            };
            for &w in &self.droids[i].weapons {
                let trigger = if Some(w) == primary && !second_busy {
                    primary_trigger
                } else {
                    Trigger::default()
                };
                let selected = Some(w) == primary || Some(w) == secondary;
                let class = defs.weapons.class(self.weapons[w].def);
                self.weapons[w].update(
                    w as u32, class, dt, selected, &trigger, false, &aim, &mut draws, &mut shots,
                );
            }
        }
        for shot in shots {
            let weapon = shot.weapon as usize;
            let def = self.level(weapon);
            if !def.get("ordnance").truthy() {
                continue;
            }
            let point = self.ask(world, 6, vec![i as f64]).await?;
            let mut r = Reader::new(&point);
            let has_point = r.flag()?;
            let mut at = if has_point {
                r.vec()?
            } else {
                Vector3::default()
            };
            if has_point {
                let mut q = Quaternion::default();
                q.set_from_axis_angle(&UP, self.droids[i].yaw);
                at.apply_quaternion(&q);
            }
            at.add(&self.droids[i].position);
            let mut dir = self.droids[i].target;
            dir.sub(&at).normalize();
            let (yaw, pitch) = match shot.kind {
                ShotKind::Round { yaw, pitch, .. } => (yaw, pitch),
                _ => (f64::NAN, f64::NAN),
            };
            let mut q = Quaternion::default();
            q.set_from_axis_angle(&UP, yaw);
            dir.apply_quaternion(&q);
            let mut axis = Vector3::new(dir.z, 0.0, -dir.x);
            axis.normalize();
            q.set_from_axis_angle(&axis, -pitch);
            dir.apply_quaternion(&q);
            self.scratch[2] = at;
            self.scratch[3] = axis;
            self.scratch[4] = dir;
            if self.on_fired {
                let mut args = vec![i as f64, weapon as f64];
                put_shot(&mut args, &shot);
                put_vec(&mut args, &at);
                put_vec(&mut args, &dir);
                let reply = world.request(7, args, self.write()).await;
                if !reply.state.is_empty() {
                    *self = Core::read(self.defs.clone(), &reply.state)?;
                }
                if !reply.ok {
                    return Err(FOREIGN);
                }
            }
        }
        let destruct = self.current(i, 1);
        if let Some(w) = destruct {
            if self.droids[i].countdown < 0.0 && secondary && self.level(w).get("strike").truthy() {
                self.droids[i].countdown = 0.0;
            }
        }
        if self.droids[i].countdown >= 0.0 && self.droids[i].salvo.is_none() {
            self.droids[i].countdown += dt;
            let most = destruct.map_or(3.0, |w| self.level(w).get("MaxPressedTime").or(3.0));
            if self.droids[i].countdown >= most {
                let w = destruct.ok_or(INVALID)?;
                let def_id = self.level_ids[w][self.weapons[w].level as usize];
                self.strike(i, def_id);
            }
        }
        Ok(())
    }
    fn strike(&mut self, i: usize, id: usize) {
        let def = self.defs.borrow().values[id].clone();
        let mut at = self.droids[i].position;
        at.y += 0.155;
        self.scratch[0] = at;
        let elevation = def.get("AimElevation").nn(90.0) * std::f64::consts::PI / 180.0;
        let azimuth = def.get("AimAzimuth").nn(0.0) * std::f64::consts::PI / 180.0;
        let z = Vector3::new(
            sin(azimuth) * cos(elevation),
            sin(elevation),
            cos(azimuth) * cos(elevation),
        );
        let x = Vector3::new(cos(azimuth), 0.0, -sin(azimuth));
        let mut y = Vector3::default();
        y.cross_vectors(&z, &x);
        self.droids[i].salvo = Some(self.salvos.len());
        self.salvos.push(Salvo {
            def: id,
            at,
            x,
            y,
            z,
            left: def.get("SalvoCount").or(1.0),
            wait: 0.0,
        });
        self.lists[self.strikes].push(i);
    }
    fn update_strikes(&mut self, dt: f64) -> Result<(), &'static str> {
        for i in self.lists[self.strikes].clone() {
            let id = self.droids[i]
                .salvo
                .ok_or("recon strike requires a salvo")?;
            let s = &mut self.salvos[id];
            let def = self.defs.borrow().values[s.def].clone();
            s.wait -= dt;
            while s.left > 0.0 && s.wait <= 0.0 {
                let scatter = def.get("ScatterDistance").or(0.0);
                let mut start = s.at;
                start.add_scaled_vector(
                    &s.x,
                    scatter * (self.streams[self.rng].uniform() - self.streams[self.rng].uniform()),
                );
                start.add_scaled_vector(
                    &s.y,
                    scatter * (self.streams[self.rng].uniform() - self.streams[self.rng].uniform()),
                );
                start.add_scaled_vector(&s.z, def.get("AimDistance").nn(100.0));
                // The private context's fire callback is a no-op. Preserve
                // all scatter draws and arithmetic before advancing the salvo.
                s.left -= 1.0;
                s.wait += def.get("SalvoDelay").or(0.0);
            }
            if s.left <= 0.0 {
                self.droids[i].salvo = None;
                self.kill(i);
            }
        }
        let filtered = self.lists[self.strikes]
            .iter()
            .copied()
            .filter(|&i| self.droids[i].salvo.is_some())
            .collect();
        self.strikes = self.lists.len();
        self.lists.push(filtered);
        Ok(())
    }
    async fn tick(&mut self, world: &World, input: Vec<f64>) -> Result<(), &'static str> {
        let mut r = Reader::new(&input);
        let dt = r.word()?;
        let first = r.word()?;
        let aspect = r.word()?;
        let scale = r.word()?;
        let invert = r.flag()?;
        let n = r.count()?;
        if !first.is_nan() {
            self.first = first;
        }
        if aspect.is_finite() && aspect > 0.0 {
            self.camera.aspect = aspect;
        }
        for _ in 0..n {
            let x = r.word()?;
            let y = r.word()?;
            if self.piloted.is_some() {
                self.look_x += clamp(x, -10000.0, 10000.0) * min(scale, 2.0);
                self.look_y += (if invert {
                    clamp(y, -10000.0, 10000.0)
                } else {
                    -clamp(y, -10000.0, 10000.0)
                }) * min(scale, 2.0);
            }
        }
        r.done()?;
        if let Some(i) = self.piloted {
            if self.camera.enabled {
                self.first = truth(self.first) as u8 as f64;
                let zoom = self.zoom_value();
                let def = self.def(i);
                let d = self.droids[i].clone();
                self.camera_update(world, &d, &def, dt, zoom, truth(self.first) || zoom > 1.0)
                    .await?;
            }
        }
        let movement = self.ask(world, 8, vec![]).await?;
        let movement = [movement[0], movement[1], movement[2]];
        if let Some(i) = self.piloted {
            if self.take(world, 0).await?
                || self.take(world, 1).await?
                || self.take(world, 2).await?
                || self.take(world, 3).await?
            {
                self.exit();
            } else if self.take(world, 4).await?
                && self
                    .current(i, 0)
                    .map_or(1.0, |w| self.level(w).get("ZoomMax").or(1.0))
                    > 1.0
            {
                self.zoom = if truth(self.zoom) { 0.0 } else { 1.0 };
            }
        }
        let order = self.order;
        let mut at = 0;
        while at < self.lists[order].len() {
            let i = self.lists[order][at];
            at += 1;
            if self.droids[i].dead {
                continue;
            }
            let flying = self.piloted == Some(i);
            self.fly(world, i, dt, if flying { Some(movement) } else { None })
                .await?;
            if self.droids[i].dead {
                continue;
            }
            self.aim(world, i, flying).await?;
            for &w in &self.droids[i].weapons {
                if !self.weapons[w].random {
                    self.weapons[w].random = true;
                    self.weapon_rng[w] = Some(self.rng);
                }
            }
            self.shoot(world, i, dt, flying).await?;
            self.droids[i].health += self.def(i).get("AddHealth").nn(0.0) * dt;
            if self.droids[i].health <= 0.0 || self.droids[i].position.y < -500.0 {
                self.kill(i);
            }
        }
        let filtered = self.lists[self.order]
            .iter()
            .copied()
            .filter(|&i| !self.droids[i].dead || self.droids[i].salvo.is_some())
            .collect();
        self.order = self.lists.len();
        self.lists.push(filtered);
        self.update_strikes(dt)?;
        Ok(())
    }
}

fn put_shot(a: &mut Vec<f64>, shot: &Shot) {
    match shot.kind {
        ShotKind::Round {
            yaw,
            pitch,
            charge,
            target_id,
            target_generation,
            ..
        } => a.extend([0.0, yaw, pitch, charge, target_id, target_generation, 0.0]),
        ShotKind::Repair => a.extend([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ShotKind::MeleeStart { swing } => a.extend([2.0, 0.0, 0.0, 0.0, 0.0, 0.0, swing]),
        ShotKind::MeleeHit => a.extend([3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ShotKind::MeleeEnd => a.extend([4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    }
}

type Job = Pin<Box<dyn Future<Output = (Core, Result<(), &'static str>)>>>;
/// 0 registers [id, kind, recursive Js value] (kind 0 is a value,
/// 1 a class, and 2 its exact retained levels). 1 adopts the
/// selectively restored JS state. 2 starts one command; 3 resumes a yield
/// with [success, value count/values, state count/state]. Output is either
/// [0,state count/state] or [1,query,args count/args,state count/state].
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    defs: Rc<RefCell<Definitions>>,
    core: Option<Core>,
    job: Option<Job>,
    exchange: Rc<RefCell<Exchange>>,
}
impl Session {
    pub fn run(&mut self, op: u32) -> Result<(), &'static str> {
        self.output.clear();
        match op {
            0 => {
                let mut r = Reader::new(&self.input);
                let id = r.count()?;
                let kind = r.count()?;
                let value = r.value(0)?;
                r.done()?;
                let mut defs = self.defs.borrow_mut();
                if defs.values.len() <= id {
                    defs.values.resize(id + 1, Js::Undefined);
                }
                if kind == 1 {
                    let Js::Obj(obj) = &value else {
                        return Err(INVALID);
                    };
                    defs.weapons
                        .set(DefId(id as u32), WeaponClass::new(obj.clone()));
                } else if kind == 2 {
                    let Js::Arr(values) = &value else {
                        return Err(INVALID);
                    };
                    let mut levels = Vec::new();
                    for value in values.iter() {
                        let Js::Obj(obj) = value else {
                            return Err(INVALID);
                        };
                        levels.push(Level::new(obj.clone()));
                    }
                    if levels.is_empty() {
                        return Err(INVALID);
                    }
                    defs.weapons.set(DefId(id as u32), WeaponClass { levels });
                } else if kind != 0 {
                    return Err(INVALID);
                }
                defs.values[id] = value;
            }
            1 => {
                if self.job.is_some() {
                    return Err(INVALID);
                }
                self.core = Some(Core::read(self.defs.clone(), &self.input)?);
            }
            2 => {
                if self.job.is_some() {
                    return Err(INVALID);
                }
                let mut core = self.core.take().ok_or(INVALID)?;
                let input = self.input.clone();
                let world = World(self.exchange.clone());
                self.job = Some(Box::pin(async move {
                    let result = core.tick(&world, input).await;
                    (core, result)
                }));
                return self.poll();
            }
            3 => {
                if self.job.is_none() {
                    return Err(INVALID);
                }
                let mut r = Reader::new(&self.input);
                let ok = r.flag()?;
                let values = r.list()?;
                let state = r.list()?;
                r.done()?;
                self.exchange.borrow_mut().reply = Some(Reply { ok, values, state });
                return self.poll();
            }
            _ => return Err(INVALID),
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<(), &'static str> {
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        match self.job.as_mut().ok_or(INVALID)?.as_mut().poll(&mut cx) {
            Poll::Ready((core, result)) => {
                let state = core.write();
                self.output.extend([0.0, state.len() as f64]);
                self.output.extend(state);
                self.core = Some(core);
                self.job = None;
                result
            }
            Poll::Pending => {
                let request = self.exchange.borrow_mut().request.take().ok_or(INVALID)?;
                self.output
                    .extend([1.0, request.op as f64, request.args.len() as f64]);
                self.output.extend(request.args);
                self.output.push(request.state.len() as f64);
                self.output.extend(request.state);
                Ok(())
            }
        }
    }
}

impl Core {
    async fn fly(
        &mut self,
        world: &World,
        i: usize,
        dt: f64,
        input: Option<[f64; 3]>,
    ) -> Result<(), &'static str> {
        let velocity_id = self.droids[i].velocity;
        let def_id = self.droids[i].def;
        let def = self.def(i);
        let altitude = def.get("SetAltitude").nn(1.75);
        let reach = RADIUS + 10.0 * altitude;
        let mut c = self.droids[i].position;
        c.y += 0.155;
        let mut height = reach - 0.155;
        let mut n = UP;
        self.scratch[0] = c;
        self.scratch[1] = n;
        self.scratch[2] = Vector3::new(0.0, -1.0, 0.0);
        if let Some(below) = self
            .cast(
                world,
                &mut c,
                &mut Vector3::new(0.0, -1.0, 0.0),
                reach,
                1,
                i,
            )
            .await?
        {
            height = below.distance - 0.155;
            n = below.normal;
            self.scratch[1] = n;
        }
        if self.ask(world, 2, vec![c.x, c.z]).await?.first() == Some(&1.0) {
            let level = self.ask(world, 3, vec![]).await?[0];
            if c.y - level < height + 0.155 {
                let level = self.ask(world, 3, vec![]).await?[0];
                height = min(height, c.y - level - 0.155);
                n = UP;
                self.scratch[1] = n;
            }
        }
        let v = self.velocities[velocity_id];
        if hypot(v.x, v.z) > 5.0 {
            let mut ray = Vector3::new(v.x, v.y - reach, v.z);
            let length = ray.length();
            ray.divide_scalar(length);
            self.scratch[2] = ray;
            if let Some(ahead) = self.cast(world, &mut c, &mut ray, length, 2, i).await? {
                let mut along = ahead.point;
                along.sub(&c);
                self.scratch[3] = along;
                height = min(height, -along.dot(&ahead.normal));
                if ahead.normal.y > 0.0 {
                    n.lerp(
                        &ahead.normal,
                        (1.0 - ahead.distance / length) * ahead.normal.y,
                    )
                    .normalize();
                    self.scratch[1] = n;
                }
            }
        }
        let def = self.defs.borrow().values[def_id].clone();
        let f = clamp(1.0 - (height - altitude) / (9.0 * altitude), 0.0, 1.0);
        let v = &mut self.velocities[velocity_id];
        v.y -= three::lerp(1.0, def.get("GravityScale").nn(1.0), f) * 9.8 * dt;
        let lift = (altitude - height) * def.get("LiftSpring").nn(5.0) + 9.8 * f
            - v.dot(&n) * def.get("LiftDamp").nn(2.0);
        if lift > 0.0 {
            v.add_scaled_vector(&n, lift * dt);
        }
        let yaw = self.droids[i].yaw;
        let mut forward = Vector3::new(sin(yaw), 0.0, cos(yaw));
        forward.add_scaled_vector(&n, -forward.dot(&n)).normalize();
        let mut right = Vector3::default();
        right.cross_vectors(&forward, &n).normalize();
        self.scratch[3] = forward;
        self.scratch[2] = right;
        let v = &mut self.velocities[velocity_id];
        let along = v.dot(&forward);
        let across = v.dot(&right);
        let most = def.get("Acceleraton").nn(70.0) * f * max(n.y, 0.0) * dt;
        v.add_scaled_vector(
            &forward,
            clamp(
                def.get("MaxSpeed").nn(9.0) * input.map_or(0.0, |a| or_zero(a[0])) - along,
                -most,
                most,
            ),
        );
        v.add_scaled_vector(
            &right,
            clamp(
                def.get("MaxStrafeSpeed").nn(7.0) * input.map_or(0.0, |a| or_zero(a[1])) - across,
                -most,
                most,
            ),
        );
        if let Some(input) = input {
            let rate = def.get("MaxTurnSpeed").nn(2.0);
            let zoom = self.zoom_value();
            let d = &mut self.droids[i];
            d.yaw -= or_zero(input[2]) * rate * dt;
            d.yaw -= self.look_x * (rate / 2048.0);
            let limits = def.get("PitchLimits");
            let lo = if limits.truthy() {
                limits.at(0).num()
            } else {
                -50.0
            } * std::f64::consts::PI
                / 180.0;
            let hi = if limits.truthy() {
                limits.at(1).num()
            } else {
                50.0
            } * std::f64::consts::PI
                / 180.0;
            d.pitch = clamp(d.pitch + (self.look_y * (rate / 2048.0)) / zoom, lo, hi);
            self.look_x = 0.0;
            self.look_y = 0.0;
        }
        let velocity = self.velocities[velocity_id];
        self.droids[i].position.add_scaled_vector(&velocity, dt);
        self.collide(world, i).await
    }
    async fn collide(&mut self, world: &World, i: usize) -> Result<(), &'static str> {
        let velocity_id = self.droids[i].velocity;
        let def = self.def(i);
        let limits = def.get("CollisionThreshold");
        let (base, slope) = if limits.truthy() {
            (limits.at(0).num(), limits.at(1).num())
        } else {
            (7.5, 2.5)
        };
        let scale = match def.get("CollisionScale") {
            Js::Arr(a) => a.first().unwrap_or(&Js::Undefined).num(),
            x => x.nn(10.0),
        };
        for _ in 0..4 {
            let mut c = self.droids[i].position;
            c.y += 0.155;
            let mut best = f64::INFINITY;
            let mut n = self.scratch[1];
            self.scratch[0] = c;
            let a = self.ask(world, 1, vec![c.x, c.y, c.z, RADIUS]).await?;
            let mut r = Reader::new(&a);
            let count = r.count()?;
            let mut triangles = Vec::new();
            for _ in 0..count {
                triangles.push(Triangle::new(r.vec()?, r.vec()?, r.vec()?));
            }
            c = r.vec()?;
            for tri in triangles {
                let mut closest = Vector3::default();
                tri.closest_point_to_point(&c, &mut closest);
                self.closest = closest;
                let distance = closest.distance_to(&c);
                if distance < RADIUS && distance < best && distance > 1e-5 {
                    best = distance;
                    n = c;
                    n.sub(&closest).divide_scalar(distance);
                    self.scratch[1] = n;
                }
            }
            let depth = if best < f64::INFINITY {
                RADIUS - best
            } else {
                match self
                    .cast(
                        world,
                        &mut c,
                        &mut Vector3::new(0.0, -1.0, 0.0),
                        RADIUS,
                        3,
                        i,
                    )
                    .await?
                {
                    Some(hit) => {
                        n = hit.normal;
                        self.scratch[1] = n;
                        RADIUS - hit.distance
                    }
                    None => break,
                }
            };
            if depth <= 0.0 {
                break;
            }
            self.droids[i].position.add_scaled_vector(&n, depth);
            let approach = self.velocities[velocity_id].dot(&n);
            if approach >= 0.0 {
                continue;
            }
            let speed = -approach;
            let threshold = base + slope * n.dot(&UP);
            if speed > threshold {
                let excess = speed - threshold;
                self.hurt(i, scale * (excess * excess));
            }
            let v = &mut self.velocities[velocity_id];
            v.add_scaled_vector(&n, -approach);
            let length = v.length();
            if length > 1e-6 {
                v.multiply_scalar(1.0 - min(1.0, (0.28125 * speed) / length));
            }
            v.add_scaled_vector(&n, 0.125 * speed);
        }
        Ok(())
    }
    async fn aim(&mut self, world: &World, i: usize, flying: bool) -> Result<(), &'static str> {
        let def = self.def(i);
        let zoom = self.zoom_value();
        let d = &mut self.droids[i];
        d.eye = if def.get("EyePointOffset").truthy() {
            js_vec(def.get("EyePointOffset"))
        } else {
            Vector3::new(0.0, 0.5, 0.0)
        };
        d.eye.add(&d.position);
        let p = d.pitch;
        d.aim.set(sin(d.yaw) * cos(p), sin(p), cos(d.yaw) * cos(p));
        let mut from = d.eye;
        let chase = flying && self.camera.enabled && !truth(self.first) && zoom <= 1.0;
        if chase {
            from = self.camera.position;
            let mut away = from;
            away.sub(&d.eye);
            from.add_scaled_vector(&d.aim, -away.dot(&d.aim));
        }
        let mut dir = d.aim;
        let hit = self
            .cast(
                world,
                &mut from,
                &mut dir,
                512.0,
                if chase { 5 } else { 4 },
                i,
            )
            .await?;
        let dir = self.droids[i].aim;
        self.droids[i].target = from;
        self.droids[i]
            .target
            .add_scaled_vector(&dir, hit.map_or(512.0, |h| h.distance));
        Ok(())
    }
}
