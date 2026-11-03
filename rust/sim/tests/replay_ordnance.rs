//! The rounds' and items' Rust (rust/sim/src/ordnance, rust/sim/src/items)
//! replayed against captures of real battles (tools/sim-capture.mjs --specs
//! ordnance,items; the corpus BFE_REPLAY_CORPUS names). Each frame starts
//! from the state the JavaScript started from; every call it makes across
//! its boundary (src/ordnance-world.js) is served from the capture, every
//! argument checked bit for bit; and the state it ends with is compared
//! with the capture's. Without a corpus nothing is replayed.
#![cfg(feature = "replay")]

use std::collections::HashMap;
use std::panic::{catch_unwind, panic_any, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

/// The harness's own test: every step one ulp longer than captured.
static NUDGE: AtomicBool = AtomicBool::new(false);
/// The tests share NUDGE and the panic hook: one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

use bfe_sim::gen::*;
use bfe_sim::items::Drop;
use bfe_sim::ordnance::defs::{Defs, ExplosionDef, Field, ItemDef, OrdnanceDef};
use bfe_sim::ordnance::world::*;
use bfe_sim::ordnance::Cx;
use bfe_sim::store::{DefId, Nullable, Ref, RefSet, StrId, Vec3};
use sim_replay::{diff, replay_corpus, same_number, Arena, Call, Capture, Corpus, Frame, Log, Node, Outcome, Report, Value, V};

/// A replay that parted from the capture (raised as a panic from inside the
/// world's calls, caught per frame).
struct Parted(String);

fn fail(message: String) -> ! {
    panic_any(Parted(message))
}

fn show(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() {
        "-0".into()
    } else {
        format!("{x:?} ({:016x})", x.to_bits())
    }
}

// ---- Definitions: each def the frame's store names, read from the capture.

#[derive(Default)]
struct Book {
    /// Explosions met in the definitions, by their capture keys.
    keys: Vec<String>,
    index: HashMap<String, u32>,
}

impl Book {
    fn explosion(&mut self, key: &str) -> DefId {
        if let Some(&id) = self.index.get(key) {
            return DefId(id);
        }
        let id = self.keys.len() as u32;
        self.keys.push(key.to_string());
        self.index.insert(key.to_string(), id);
        DefId(id)
    }
}

fn field_of(v: V, book: &mut Book) -> Field {
    match v.value {
        Value::Undefined => Field::Undefined,
        Value::Null => Field::Null,
        Value::Bool(b) => Field::Bool(*b),
        Value::Num(x) => Field::Num(*x),
        Value::Str(s) => Field::Str(s.clone()),
        Value::Str16(u) => Field::Str(String::from_utf16_lossy(u)),
        Value::Handle(kind, key) if kind == "def" => match &**key {
            Value::Str(k) => Field::Explosion(book.explosion(k)),
            _ => Field::Object,
        },
        _ => Field::Object,
    }
}

/// Every definition the replay context's tables name, and the explosions
/// they name (and `explosions`, met elsewhere), read from the capture.
fn defs_of(capture: &Capture, cx: &mut Replay, explosions: &[&str]) -> (Defs, Book) {
    let mut book = Book::default();
    for key in explosions {
        book.explosion(key);
    }
    let mut defs = Defs::default();
    for table in ["ordnance", "item"] {
        let mut i = 0;
        loop {
            let h = cx.def_out(table, DefId(i));
            let key = match &h {
                Value::Handle(_, key) => match &**key {
                    Value::Str(k) => k.clone(),
                    _ => break,
                },
                _ => break,
            };
            let rec = capture.def(&key).unwrap_or_else(|| fail(format!("no definition {key} in the capture")));
            if table == "ordnance" {
                defs.ordnance.push(OrdnanceDef::read(|name| field_of(rec.get(name), &mut book)));
            } else {
                defs.item.push(ItemDef::read(|name| field_of(rec.get(name), &mut book)));
            }
            i += 1;
        }
    }
    let mut i = 0;
    while i < book.keys.len() {
        let key = book.keys[i].clone();
        let rec = capture.def(&key).unwrap_or_else(|| fail(format!("no explosion {key} in the capture")));
        defs.explosion.push(ExplosionDef::read(|name| field_of(rec.get(name), &mut book)));
        i += 1;
    }
    (defs, book)
}

// ---- The world, served from a frame's log.

struct LogWorld<'f> {
    frame: &'f Frame,
    log: Log<'f>,
    cx: *mut Replay,
    book: *const Book,
}

impl<'f> LogWorld<'f> {
    fn cx(&mut self) -> &mut Replay {
        // SAFETY: the replay context outlives the world, and the port holds
        // no reference into it across a call to the world.
        unsafe { &mut *self.cx }
    }

    /// Steps over the frame's own Math.random draws: three.js's ids for the
    /// models a world makes and never draws (quirk Q11; no gameplay reads
    /// Math.random since the determinism prep), which the port does not make.
    fn skip_ids(&mut self) {
        while matches!(self.log.peek(), Some(sim_replay::Event::Draw(d)) if d.stream == "Math.random") {
            let _ = self.log.draw("Math.random");
        }
    }

    fn call(&mut self, label: &str) -> &'f Call {
        self.skip_ids();
        match self.log.call(label) {
            Ok(c) => c,
            Err(e) => fail(e.0),
        }
    }

    fn num(&self, what: &str, captured: V, mine: f64) {
        if !matches!(captured.opt_num(), Some(x) if same_number(x, mine)) {
            fail(format!("{}: {what}: the capture has {}, the port {}", self.frame.place(), captured.show(), show(mine)));
        }
    }

    fn vec3(&self, what: &str, captured: V, mine: Vec3) {
        match captured.vec3() {
            Some(v) if v.iter().zip(mine).all(|(a, b)| same_number(*a, b)) => {}
            _ => fail(format!("{}: {what}: the capture has {}, the port ({}, {}, {})", self.frame.place(), captured.show(), show(mine[0]), show(mine[1]), show(mine[2]))),
        }
    }

    fn boolean(&self, what: &str, captured: V, mine: bool) {
        if captured.opt_bool() != Some(mine) {
            fail(format!("{}: {what}: the capture has {}, the port {mine}", self.frame.place(), captured.show()));
        }
    }

    /// A value the port builds (with the replay context's handles) against the capture's.
    fn value(&mut self, what: &str, captured: V, build: impl FnOnce(&mut Replay, &mut Arena) -> Value) {
        let mut a = Arena::default();
        let mine = build(self.cx(), &mut a);
        if let Some(d) = diff(captured, a.view(&mine)) {
            fail(format!("{}: {what}: {d} (capture | port)", self.frame.place()));
        }
    }

    fn owner(&mut self, what: &str, captured: V, owner: OrdnanceOwner) {
        self.value(what, captured, |cx, a| owner.to_value(cx, a));
    }

    fn source(&mut self, what: &str, captured: V, source: BeamSource) {
        self.value(what, captured, |cx, a| source.to_value(cx, a));
    }

    fn body_arg(&mut self, what: &str, captured: V, body: Nullable<Ref<Body>>) {
        self.value(what, captured, |cx, _| match body {
            Nullable::Value(b) => cx.ref_out(KIND_BODY, b),
            Nullable::Null => Value::Null,
            Nullable::Undefined => Value::Undefined,
        });
    }

    fn opt_body(b: Option<Ref<Body>>) -> Nullable<Ref<Body>> {
        match b {
            Some(b) => Nullable::Value(b),
            None => Nullable::Null,
        }
    }

    fn unit_arg(&mut self, what: &str, captured: V, unit: Ref<Unit>) {
        self.value(what, captured, |cx, _| cx.ref_out(KIND_UNIT, unit));
    }

    fn def_arg(&mut self, what: &str, captured: V, table: &str, def: DefId) {
        self.value(what, captured, |cx, _| cx.def_out(table, def));
    }

    fn field_arg(&mut self, what: &str, captured: V, f: &Field) {
        // SAFETY: the book is read only while the frame replays.
        let book = unsafe { &*self.book };
        let mine = match f {
            Field::Undefined => Value::Undefined,
            Field::Null => Value::Null,
            Field::Bool(b) => Value::Bool(*b),
            Field::Num(x) => Value::Num(*x),
            Field::Str(s) => Value::Str(s.clone()),
            Field::Explosion(id) => Value::handle("def", Value::str(&book.keys[id.0 as usize])),
            Field::Object => fail(format!("{}: {what}: an object the replay cannot name", self.frame.place())),
        };
        let a = Arena::default();
        if let Some(d) = diff(captured, a.view(&mine)) {
            fail(format!("{}: {what}: {d} (capture | port)", self.frame.place()));
        }
    }

    fn part_arg(&self, what: &str, captured: V, part: Nullable<HitPart>) {
        let ok = match part {
            Nullable::Value(p) => captured.str() == Some(HitPart::NAMES[p as usize]),
            Nullable::Null => captured.is_null(),
            Nullable::Undefined => captured.is_undefined(),
        };
        if !ok {
            fail(format!("{}: {what}: the capture has {}, the port {part:?}", self.frame.place(), captured.show()));
        }
    }

    fn body_of(&mut self, v: V) -> Option<Ref<Body>> {
        if v.is_nullish() {
            return None;
        }
        Some(self.cx().ref_in(KIND_BODY, v, "a hit's body").unwrap_or_else(|e| fail(e)))
    }

    fn hit_of(&mut self, v: V) -> Option<Hit> {
        if v.is_nullish() {
            return None;
        }
        Some(Hit {
            distance: v.get("distance").num(),
            point: v.get("point").vec3().unwrap(),
            normal: v.get("normal").vec3().unwrap(),
            terrain: v.get("terrain").bool(),
            body: self.body_of(v.get("body")),
        })
    }

    fn part_of(v: V) -> Nullable<HitPart> {
        match v.value {
            Value::Undefined => Nullable::Undefined,
            Value::Null => Nullable::Null,
            _ => Nullable::Value(match v.str() {
                Some("head") => HitPart::Head,
                Some("body") => HitPart::Body,
                _ => fail(format!("a hit part of {}", v.show())),
            }),
        }
    }

    /// The fields of an entity of the store (its exported value) against a
    /// JavaScript object a call was handed, field by field.
    fn same_fields(&self, what: &str, raw: V, mine: V) {
        if let Some(Node::Object { fields, .. }) = mine.node() {
            if raw.node().is_some() && matches!(raw.node(), Some(Node::Object { .. })) {
                for (k, v) in fields {
                    // A beam's damage lives on its copy of the definition.
                    if k == "damage" {
                        continue;
                    }
                    self.same_fields(&format!("{what}.{k}"), raw.get(k), mine.arena.view(v));
                }
                return;
            }
        }
        if let Some(d) = diff(raw, mine) {
            fail(format!("{}: {what}: {d} (capture | port)", self.frame.place()));
        }
    }

    fn shot_value(&mut self, shot: Shot, a: &mut Arena) -> Value {
        let cx = self.cx();
        cx.next_record();
        fn find<T: Clone>(slab: &bfe_sim::store::Slab<T>, r: Ref<T>) -> T {
            slab.get(r).cloned().or_else(|| slab.retired.iter().find(|(x, _)| *x == r).map(|(_, v)| v.clone())).expect("an entity of the store")
        }
        match shot {
            Shot::Bolt(r) => find(&cx.store.bolt, r).to_value(cx, a),
            Shot::Beam(r) => find(&cx.store.beam, r).to_value(cx, a),
            Shot::Grenade(r) => find(&cx.store.grenade, r).to_value(cx, a),
            Shot::Round(r) => find(&cx.store.round, r).to_value(cx, a),
        }
    }

    fn shot_arg(&mut self, what: &str, captured: V, shot: Shot) {
        let mut a = Arena::default();
        let mine = self.shot_value(shot, &mut a);
        self.same_fields(what, captured, a.view(&mine));
    }

    /// The items as an explosion's call left them: each item the store held
    /// takes its state there, and those the call made join the list.
    fn apply_items(&mut self, after: V) {
        let frame = self.frame;
        let cx = self.cx();
        cx.next_record();
        let list: Vec<V> = after.get("items").items().collect();
        let mine = cx.store.items.items.clone();
        if list.len() < mine.len() {
            fail(format!("{}: the items after an explosion are fewer than before ({} < {})", frame.place(), list.len(), mine.len()));
        }
        let mut by_node: HashMap<u32, Ref<Item>> = HashMap::new();
        for (i, v) in list.iter().enumerate() {
            let r = if i < mine.len() {
                let fresh = Item::from_value(*v, cx, "items").unwrap_or_else(|e| fail(e));
                *cx.store.item.get_mut(mine[i]).expect("an item the store holds") = fresh;
                mine[i]
            } else {
                let r = Item::ref_in(*v, cx, "items").unwrap_or_else(|e| fail(e));
                cx.store.items.items.push(r);
                r
            };
            if let Some(n) = v.node_id() {
                by_node.insert(n, r);
            }
        }
        let mut owned = StrMap::default();
        for (key, items) in after.get("owned").entries() {
            let key = cx.str_in(key, "owned").unwrap_or_else(|e| fail(e));
            let refs = items.items().map(|v| *v.node_id().and_then(|n| by_node.get(&n)).unwrap_or_else(|| fail("an owned item not in the list".into()))).collect();
            owned.set(key, refs);
        }
        cx.store.items.owned = owned;
    }
}

impl<'f> World for LogWorld<'f> {
    fn cast(&mut self, origin: Vec3, direction: Vec3, far: f64, _out: Out, ignore_body: Nullable<Ref<Body>>, static_only: Option<bool>) -> Option<Hit> {
        let c = self.call("ordnance.world.cast");
        self.vec3("cast origin", c.arg(0), origin);
        self.vec3("cast direction", c.arg(1), direction);
        self.num("cast far", c.arg(2), far);
        self.body_arg("cast ignoreBody", c.arg(4), ignore_body);
        match static_only {
            None => {
                if !c.arg(5).is_undefined() {
                    fail(format!("{}: cast staticOnly: the capture has {}", self.frame.place(), c.arg(5).show()));
                }
            }
            Some(b) => self.boolean("cast staticOnly", c.arg(5), b),
        }
        self.hit_of(c.result())
    }

    fn cast_moving(&mut self, origin: Vec3, direction: Vec3, far: f64, _out: Out, ignore_body: Nullable<Ref<Body>>) -> Option<Hit> {
        let c = self.call("ordnance.world.castMoving");
        self.vec3("castMoving origin", c.arg(0), origin);
        self.vec3("castMoving direction", c.arg(1), direction);
        self.num("castMoving far", c.arg(2), far);
        self.body_arg("castMoving ignoreBody", c.arg(4), ignore_body);
        self.hit_of(c.result())
    }

    fn raycast_units(&mut self, origin: Vec3, direction: Vec3, far: f64, owner: OrdnanceOwner, history_tick: Option<f64>, ignore_body: Option<Ref<Body>>, mount: Option<Ref<Vehicle>>) -> Option<UnitHit> {
        let c = self.call("ordnance.world.raycastUnits");
        self.vec3("raycastUnits origin", c.arg(0), origin);
        self.vec3("raycastUnits direction", c.arg(1), direction);
        self.num("raycastUnits far", c.arg(2), far);
        self.owner("raycastUnits owner", c.arg(3), owner);
        match history_tick {
            Some(t) => self.num("raycastUnits historyTick", c.arg(4), t),
            None => {
                if !c.arg(4).is_null() {
                    fail(format!("{}: raycastUnits historyTick: the capture has {}, the port null", self.frame.place(), c.arg(4).show()));
                }
            }
        }
        self.body_arg("raycastUnits ignoreBody", c.arg(5), Self::opt_body(ignore_body));
        self.value("raycastUnits mount", c.arg(6), |cx, _| match mount {
            Some(v) => cx.ref_out(KIND_VEHICLE, v),
            None => Value::Null,
        });
        let r = c.result();
        if r.is_nullish() {
            return None;
        }
        let unit = self.cx().ref_in(KIND_UNIT, r.get("unit"), "a unit hit").unwrap_or_else(|e| fail(e));
        Some(UnitHit { unit, distance: r.get("distance").num(), point: r.get("point").vec3().unwrap(), normal: r.get("normal").vec3().unwrap(), part: Self::part_of(r.get("part")) })
    }

    fn rider_mount(&mut self, body: Nullable<Ref<Body>>) -> Option<RiderMount> {
        let c = self.call("ordnance.world.riderMount");
        self.body_arg("riderMount body", c.arg(0), body);
        let r = c.result();
        if r.is_nullish() {
            return None;
        }
        let vehicle = self.cx().ref_in(KIND_VEHICLE, r.get("vehicle"), "a rider's mount").unwrap_or_else(|e| fail(e));
        let body = match r.get("body").value {
            Value::Undefined => Nullable::Undefined,
            Value::Null => Nullable::Null,
            _ => Nullable::Value(self.body_of(r.get("body")).unwrap()),
        };
        Some(RiderMount { vehicle, bound_radius: r.get("boundRadius").opt_num().unwrap_or(f64::NAN), body })
    }

    fn fat_query(&mut self, center: Vec3, radius: f64, owner: OrdnanceOwner, struck: &RefSet<Unit>, ignore_body: Option<Ref<Body>>) -> Option<Vec<Ref<Unit>>> {
        let c = self.call("ordnance.world.fatQuery");
        self.vec3("fatQuery center", c.arg(0), center);
        self.num("fatQuery radius", c.arg(1), radius);
        self.owner("fatQuery owner", c.arg(2), owner);
        let items = struck.items.clone();
        self.value("fatQuery struck", c.arg(3), |cx, a| {
            let members = items.iter().map(|&u| cx.ref_out(KIND_UNIT, u)).collect();
            a.set(members)
        });
        self.body_arg("fatQuery ignoreBody", c.arg(4), Self::opt_body(ignore_body));
        let r = c.result();
        if r.is_nullish() {
            return None;
        }
        Some(r.items().map(|u| self.cx().ref_in(KIND_UNIT, u, "a fat ray's unit").unwrap_or_else(|e| fail(e))).collect())
    }

    fn unit_sphere(&mut self, unit: Ref<Unit>) -> Option<(Vec3, f64)> {
        let c = self.call("ordnance.world.unitSphere");
        self.unit_arg("unitSphere unit", c.arg(0), unit);
        let r = c.result();
        if r.is_nullish() {
            return None;
        }
        Some((r.get("center").vec3().unwrap(), r.get("radius").num()))
    }

    fn is_droid(&mut self, unit: Ref<Unit>) -> bool {
        let c = self.call("ordnance.world.isDroid");
        self.unit_arg("isDroid unit", c.arg(0), unit);
        c.result().bool()
    }

    fn gone(&mut self, x: BeamSource) -> bool {
        let c = self.call("ordnance.world.gone");
        self.source("gone of", c.arg(0), x);
        c.result().bool()
    }

    fn beam_local_origin(&mut self, source: BeamSource, origin: Vec3) -> Option<Vec3> {
        let c = self.call("ordnance.world.beamLocalOrigin");
        self.source("beamLocalOrigin source", c.arg(0), source);
        self.vec3("beamLocalOrigin origin", c.arg(1), origin);
        c.result().vec3()
    }

    fn beam_pose(&mut self, source: BeamSource, local_origin: Vec3, position: &mut Vec3, direction: &mut Vec3) {
        let c = self.call("ordnance.world.beamPose");
        self.source("beamPose source", c.arg(0), source);
        self.vec3("beamPose localOrigin", c.arg(1), local_origin);
        self.vec3("beamPose position", c.arg(2), *position);
        self.vec3("beamPose direction", c.arg(3), *direction);
        *position = c.out(2).vec3().unwrap();
        *direction = c.out(3).vec3().unwrap();
    }

    fn shot_tick(&mut self, owner: OrdnanceOwner, channel: f64) -> Nullable<f64> {
        let c = self.call("ordnance.world.shotTick");
        self.owner("shotTick owner", c.arg(0), owner);
        self.num("shotTick channel", c.arg(1), channel);
        match c.result().value {
            Value::Undefined => Nullable::Undefined,
            Value::Null => Nullable::Null,
            Value::Num(x) => Nullable::Value(*x),
            _ => fail(format!("{}: shotTick gave {}", self.frame.place(), c.result().show())),
        }
    }

    fn history_tick(&mut self) -> f64 {
        self.call("ordnance.world.historyTick").result().num()
    }

    fn shooter(&mut self, owner: OrdnanceOwner, channel: f64) -> Shooter {
        let c = self.call("ordnance.world.shooter");
        self.owner("shooter owner", c.arg(0), owner);
        self.num("shooter channel", c.arg(1), channel);
        let r = c.result();
        Shooter { fire_id: r.get("fireId").num(), unit: r.get("unit").num(), generation: r.get("generation").num() }
    }

    fn missile_target(&mut self, target_id: f64, target_generation: f64) -> Option<Vec3> {
        let c = self.call("ordnance.world.missileTarget");
        self.num("missileTarget targetId", c.arg(0), target_id);
        self.num("missileTarget targetGeneration", c.arg(1), target_generation);
        if c.result().bool() {
            Some(c.out(2).vec3().unwrap())
        } else {
            None
        }
    }

    fn navigate_missile(&mut self, position: Vec3, velocity: &mut Vec3, flight_class: Option<StrId>, target_id: f64, target_generation: f64, dt: f64) -> f64 {
        let c = self.call("ordnance.world.navigateMissile");
        self.vec3("navigateMissile position", c.arg(0), position);
        self.vec3("navigateMissile velocity", c.arg(1), *velocity);
        self.value("navigateMissile flightClass", c.arg(2), |cx, _| match flight_class {
            Some(s) => cx.str_out(s),
            None => Value::Undefined,
        });
        self.num("navigateMissile targetId", c.arg(3), target_id);
        self.num("navigateMissile targetGeneration", c.arg(4), target_generation);
        self.num("navigateMissile dt", c.arg(5), dt);
        *velocity = c.out(1).vec3().unwrap();
        c.result().num()
    }

    fn water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let c = self.call("ordnance.world.waterCrossing");
        self.vec3("waterCrossing from", c.arg(0), from);
        self.vec3("waterCrossing to", c.arg(1), to);
        let r = c.result();
        if r.is_nullish() {
            None
        } else {
            Some(r.vec3().unwrap())
        }
    }

    fn waver(&mut self) -> f64 {
        self.call("ordnance.world.waver").result().num()
    }

    fn spin(&mut self) -> f64 {
        self.call("ordnance.world.spin").result().num()
    }

    fn explosion(&mut self, def: &Field, position: Vec3, owner: OrdnanceOwner) {
        let c = self.call("ordnance.world.explosion");
        self.field_arg("explosion def", c.arg(0), def);
        self.vec3("explosion position", c.arg(1), position);
        self.owner("explosion owner", c.arg(2), owner);
    }

    fn hit(&mut self, body: Ref<Body>, def: DefArg, owner: OrdnanceOwner, direction: Vec3) {
        let c = self.call("ordnance.world.hit");
        self.body_arg("hit body", c.arg(0), Nullable::Value(body));
        self.def_of("hit def", c.arg(1), def);
        self.owner("hit owner", c.arg(2), owner);
        self.vec3("hit direction", c.arg(3), direction);
    }

    fn hit_unit(&mut self, unit: Ref<Unit>, def: DefArg, point: Point, owner: OrdnanceOwner, part: Nullable<HitPart>, direction: Vec3) {
        let c = self.call("ordnance.world.hitUnit");
        self.unit_arg("hitUnit unit", c.arg(0), unit);
        self.def_of("hitUnit def", c.arg(1), def);
        self.vec3("hitUnit point", c.arg(2), point.value());
        self.owner("hitUnit owner", c.arg(3), owner);
        self.part_arg("hitUnit part", c.arg(4), part);
        self.vec3("hitUnit direction", c.arg(5), direction);
    }

    fn projectile(&mut self, shot: Shot, kind: ShotKind) {
        let c = self.call("ordnance.world.projectile");
        self.shot_arg("projectile", c.arg(0), shot);
        if c.arg(1).str() != Some(kind.name()) {
            fail(format!("{}: projectile type: the capture has {}, the port {}", self.frame.place(), c.arg(1).show(), kind.name()));
        }
    }

    fn launch(&mut self, bolt: Ref<Bolt>, tick: Nullable<f64>) {
        let c = self.call("ordnance.world.launch");
        self.shot_arg("launch", c.arg(0), Shot::Bolt(bolt));
        let ok = match tick {
            Nullable::Value(t) => matches!(c.arg(1).opt_num(), Some(x) if same_number(x, t)),
            Nullable::Null => c.arg(1).is_null(),
            Nullable::Undefined => c.arg(1).is_undefined(),
        };
        if !ok {
            fail(format!("{}: launch tick: the capture has {}, the port {tick:?}", self.frame.place(), c.arg(1).show()));
        }
    }

    fn impact(&mut self, shot: Shot, info: &ImpactInfo) {
        let c = self.call("ordnance.world.impact");
        self.shot_arg("impact of", c.arg(0), shot);
        let got = c.arg(1);
        match info {
            ImpactInfo::Plain { point, normal, surface } => {
                let keys: Vec<&str> = got.fields().map(|(k, _)| k).collect();
                if keys != ["point", "normal", "surface"] {
                    fail(format!("{}: impact: the capture's keys {keys:?}, the port's point, normal, surface", self.frame.place()));
                }
                self.vec3("impact point", got.get("point"), *point);
                self.vec3("impact normal", got.get("normal"), *normal);
                self.num("impact surface", got.get("surface"), *surface);
            }
            ImpactInfo::Struck { impact, surface } => {
                let imp = impact.clone();
                let mut a = Arena::default();
                let mine = imp.to_value(self.cx(), &mut a);
                self.same_fields("impact", got, a.view(&mine));
                self.num("impact surface", got.get("surface"), *surface);
            }
        }
    }

    fn grenade_contact(&mut self, grenade: Ref<Grenade>) {
        let c = self.call("ordnance.world.grenadeContact");
        self.shot_arg("grenadeContact", c.arg(0), Shot::Grenade(grenade));
    }

    fn bounce(&mut self, def: DefId, position: Vec3, speed: f64) {
        let c = self.call("ordnance.world.bounce");
        self.def_arg("bounce def", c.arg(0), "ordnance", def);
        self.vec3("bounce position", c.arg(1), position);
        self.num("bounce speed", c.arg(2), speed);
    }

    fn item_cast(&mut self, origin: Vec3, direction: Vec3, far: f64) -> Option<Hit> {
        let c = self.call("items.world.cast");
        self.vec3("items cast origin", c.arg(0), origin);
        self.vec3("items cast direction", c.arg(1), direction);
        self.num("items cast far", c.arg(2), far);
        self.hit_of(c.result())
    }

    fn blocked(&mut self, position: Vec3, target: Vec3) -> bool {
        let c = self.call("items.world.blocked");
        self.vec3("blocked position", c.arg(0), position);
        self.vec3("blocked target", c.arg(1), target);
        c.result().bool()
    }

    fn item_water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let c = self.call("items.world.waterCrossing");
        self.vec3("items waterCrossing from", c.arg(0), from);
        self.vec3("items waterCrossing to", c.arg(1), to);
        let r = c.result();
        if r.is_nullish() {
            None
        } else {
            Some(r.vec3().unwrap())
        }
    }

    fn item_model(&mut self, def: DefId) -> ItemModel {
        let c = self.call("items.world.model");
        self.def_arg("model def", c.arg(0), "item", def);
        let r = c.result();
        ItemModel { mesh: !r.get("mesh").is_nullish(), radius: r.get("radius").num(), center: r.get("center").vec3() }
    }

    fn owner_key(&mut self, owner: ItemOwner) -> String {
        let c = self.call("items.world.ownerKey");
        self.value("ownerKey owner", c.arg(0), |cx, a| owner.to_value(cx, a));
        c.result().str().unwrap_or_else(|| fail("ownerKey gave no string".into())).to_string()
    }

    fn drop_roll(&mut self) -> f64 {
        self.call("items.world.dropRoll").result().num()
    }

    fn explode(&mut self, def: &Field, position: Vec3, owner: ItemOwner) {
        let c = self.call("items.world.explode");
        self.field_arg("explode def", c.arg(0), def);
        self.vec3("explode position", c.arg(1), position);
        self.value("explode owner", c.arg(2), |cx, a| owner.to_value(cx, a));
        let after = c.extra().get("items");
        if !after.is_undefined() {
            self.apply_items(after);
        }
    }

    fn watch(&mut self, index: usize, team: f64) -> Watched {
        let c = self.call("items.world.watch");
        self.num("watch index", c.arg(0), index as f64);
        self.num("watch team", c.arg(1), team);
        let r = c.result();
        let nan = [f64::NAN; 3];
        Watched { relation: r.get("relation").num(), center: r.get("center").vec3().unwrap_or(nan), radius: r.get("radius").opt_num().unwrap_or(f64::NAN), velocity: r.get("velocity").vec3().unwrap_or(nan) }
    }

    fn standing(&mut self, index: usize) -> Vec3 {
        let c = self.call("items.world.standing");
        self.num("standing index", c.arg(0), index as f64);
        c.result().vec3().unwrap()
    }

    fn give(&mut self, index: usize, def: DefId) -> bool {
        let c = self.call("items.world.give");
        self.num("give index", c.arg(0), index as f64);
        self.def_arg("give def", c.arg(1), "item", def);
        c.result().bool()
    }

    fn pickup(&mut self, def: DefId, position: Vec3) {
        let c = self.call("items.world.pickup");
        self.def_arg("pickup def", c.arg(0), "item", def);
        self.vec3("pickup position", c.arg(1), position);
    }
}

impl<'f> LogWorld<'f> {
    fn def_of(&mut self, what: &str, captured: V, def: DefArg) {
        match def {
            DefArg::Ordnance(id) => self.def_arg(what, captured, "ordnance", id),
            // A beam's copy of its class: a definition of its own.
            DefArg::Beam(_) => {
                if captured.handle_str("def").is_none() {
                    fail(format!("{}: {what}: the capture has {}, the port a beam's copy of its class", self.frame.place(), captured.show()));
                }
            }
        }
    }
}

// ---- The frames.

fn ordnance_owner(cx: &mut Replay, v: V) -> OrdnanceOwner {
    OrdnanceOwner::from_value(v, cx, "owner").unwrap_or_else(|e| fail(e))
}

/// Runs `port` on a frame from the state `read` reads, then checks the
/// calls were all made and the end state is the capture's.
fn replay<R>(capture: &Capture, frame: &Frame, read: impl FnOnce(&mut Replay), port: impl FnOnce(&mut Cx<LogWorld>, &mut Replay) -> R, check: impl FnOnce(&mut Replay, &LogWorld, R)) -> Outcome {
    let run = catch_unwind(AssertUnwindSafe(|| {
        let mut cx = Replay::default();
        read(&mut cx);
        // A blast's explosion is its argument.
        let blast: Vec<&str> = if frame.name == "items.blast" { frame.arg(0).handle_str("def").into_iter().collect() } else { Vec::new() };
        let (mut defs, mut book) = defs_of(capture, &mut cx, &blast);
        // The battle's item pools, which the items' frames carry.
        for (k, v) in frame.before().get("pools").fields() {
            defs.pools.push((k.to_string(), field_of(v, &mut book)));
        }
        let cxp: *mut Replay = &mut cx;
        let mut world = LogWorld { frame, log: Log::new(frame), cx: cxp, book: &book };
        // SAFETY: the store, strings and definitions outlive the context,
        // and only it (and the world, between its calls) reaches them.
        let result = {
            let mut ordnance = unsafe { Cx::new(&mut (*cxp).store, &mut (*cxp).strings, &defs, &mut world) };
            port(&mut ordnance, unsafe { &mut *cxp })
        };
        world.skip_ids();
        if let Err(e) = world.log.finish() {
            fail(e.0);
        }
        check(unsafe { &mut *cxp }, &world, result);
    }));
    match run {
        Ok(()) => Outcome::Same,
        Err(payload) => match payload.downcast::<Parted>() {
            Ok(p) => Outcome::Failed(p.0),
            Err(other) => Outcome::Failed(format!("{}: the port panicked: {}", frame.place(), other.downcast_ref::<String>().cloned().or_else(|| other.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())),
        },
    }
}

fn state_same(frame: &Frame, what: &str, captured: V, mine: V) {
    if let Some(d) = diff(captured, mine) {
        fail(format!("{}: {what} at the end: {d} (capture | port)", frame.place()));
    }
}

fn ordnance_after(cx: &mut Replay, frame: &Frame) {
    cx.next_record();
    let mut a = Arena::default();
    let mine = cx.write_value(KIND_ORDNANCE, &mut a);
    state_same(frame, "the ordnance", frame.after(), a.view(&mine));
}

fn items_after(cx: &mut Replay, frame: &Frame, after: V) {
    cx.next_record();
    let mut a = Arena::default();
    let mine = cx.write_value(KIND_ITEMS, &mut a);
    state_same(frame, "the items", after, a.view(&mine));
}

fn opt_num(v: V) -> Option<f64> {
    v.opt_num()
}

fn replay_frame(capture: &Capture, frame: &Frame) -> Outcome {
    match frame.name.as_str() {
        "ordnance.update" => {
            if frame.arg(1).truthy() {
                return Outcome::Skipped("a page's update (drawn)".into());
            }
            let dt = frame.arg(0).num();
            let dt = if NUDGE.load(Ordering::Relaxed) { f64::from_bits(dt.to_bits() + 1) } else { dt };
            replay(capture, frame, |cx| cx.read_value(KIND_ORDNANCE, frame.before()).unwrap_or_else(|e| fail(e)), |o, _| o.update(dt), |cx, _, ()| ordnance_after(cx, frame))
        }
        "ordnance.fireRound" => replay(
            capture,
            frame,
            |cx| {
                cx.read_value(KIND_ORDNANCE, frame.before()).unwrap_or_else(|e| fail(e));
                cx.def_in("ordnance", frame.arg(0), "def").unwrap_or_else(|e| fail(e));
            },
            |o, cx| {
                let a = |i| frame.arg(i);
                let def = cx.def_in("ordnance", a(0), "def").unwrap();
                let owner = ordnance_owner(cx, a(3));
                let visual = a(4).vec3();
                let ignore_body = if a(5).is_nullish() { None } else { Some(cx.ref_in(KIND_BODY, a(5), "ignoreBody").unwrap_or_else(|e| fail(e))) };
                let emitter = if a(6).is_nullish() { BeamSource::Null } else { BeamSource::from_value(a(6), cx, "emitter").unwrap_or_else(|e| fail(e)) };
                o.fire_round(def, a(1).vec3().unwrap(), a(2).vec3().unwrap(), owner, visual, ignore_body, emitter, a(7).num(), opt_num(a(8)), opt_num(a(9)))
            },
            |cx, world, made| {
                ordnance_after(cx, frame);
                let result = frame.result();
                match made {
                    None => {
                        if !result.is_undefined() {
                            fail(format!("{}: fireRound returned {}, the port nothing", frame.place(), result.show()));
                        }
                    }
                    Some(shot) => {
                        let mut a = Arena::default();
                        let world: &LogWorld = world;
                        let w = world as *const LogWorld as *mut LogWorld;
                        // SAFETY: the world is done with its calls; this only builds values.
                        let mine = unsafe { (*w).shot_value(shot, &mut a) };
                        world.same_fields("fireRound's round", result, a.view(&mine));
                    }
                }
            },
        ),
        "items.update" => {
            let dt = frame.arg(0).num();
            let targets = frame.arg(1).len();
            replay(capture, frame, |cx| cx.read_value(KIND_ITEMS, frame.before().get("items")).unwrap_or_else(|e| fail(e)), |o, _| o.items_update(dt, targets), |cx, _, ()| items_after(cx, frame, frame.after().get("items")))
        }
        "items.dispense" => replay(
            capture,
            frame,
            |cx| {
                cx.read_value(KIND_ITEMS, frame.before().get("items")).unwrap_or_else(|e| fail(e));
                cx.def_in("item", frame.arg(2), "def").unwrap_or_else(|e| fail(e));
            },
            |o, cx| {
                let a = |i| frame.arg(i);
                let weapon = a(0).str().unwrap_or_else(|| fail(format!("a dispenser weapon of {}", a(0).show()))).to_string();
                let def = cx.def_in("item", a(2), "def").unwrap();
                let owner = ItemOwner::from_value(a(6), cx, "owner").unwrap_or_else(|e| fail(e));
                o.dispense(&weapon, a(1).num(), def, a(3).vec3().unwrap(), a(4).vec3().unwrap(), a(5).num(), owner, a(7).num())
            },
            |cx, _, _| items_after(cx, frame, frame.after().get("items")),
        ),
        "items.dropFor" => {
            let drops_v = frame.arg(0);
            let classes = frame.arg(1);
            replay(
                capture,
                frame,
                |cx| {
                    cx.read_value(KIND_ITEMS, frame.before().get("items")).unwrap_or_else(|e| fail(e));
                    for d in drops_v.items() {
                        let def = d.get("class").str().map(|c| classes.get(c)).filter(|v| v.truthy());
                        if let Some(def) = def {
                            cx.def_in("item", def, "drop").unwrap_or_else(|e| fail(e));
                        }
                    }
                },
                |o, cx| {
                    let mut book = Book::default();
                    let drops: Vec<Drop> = drops_v
                        .items()
                        .map(|d| {
                            let def = d.get("class").str().map(|c| classes.get(c)).filter(|v| v.truthy());
                            Drop { def: def.map(|v| cx.def_in("item", v, "drop").unwrap()), probability: field_of(d.get("probability"), &mut book) }
                        })
                        .collect();
                    o.drop_for(&drops, frame.arg(2).vec3().unwrap(), frame.arg(3).vec3().unwrap(), frame.arg(4).num())
                },
                |cx, _, _| items_after(cx, frame, frame.after().get("items")),
            )
        }
        "items.blast" => {
            let Some(key) = frame.arg(0).handle_str("def") else {
                return Outcome::Skipped("a blast of no explosion's definition".into());
            };
            let key = key.to_string();
            replay(
                capture,
                frame,
                |cx| cx.read_value(KIND_ITEMS, frame.before().get("items")).unwrap_or_else(|e| fail(e)),
                |o, _| {
                    // The explosion's place among those the definitions name.
                    let id = book_id(o, &key);
                    o.blast(id, frame.arg(1).vec3().unwrap())
                },
                |cx, _, ()| items_after(cx, frame, frame.after().get("items")),
            )
        }
        "items.hurt" => replay(
            capture,
            frame,
            |cx| {
                cx.read_value(KIND_ITEMS, frame.before().get("items")).unwrap_or_else(|e| fail(e));
                Item::ref_in(frame.before().get("item"), cx, "item").unwrap_or_else(|e| fail(e));
            },
            |o, cx| {
                let r = Item::ref_in(frame.before().get("item"), cx, "item").unwrap();
                o.hurt(r, frame.arg(1).num())
            },
            |cx, _, ()| items_after(cx, frame, frame.after().get("items")),
        ),
        other => Outcome::Skipped(format!("no replay of {other}")),
    }
}

/// An explosion definition's index by its capture key, among those the
/// replay's definitions hold.
fn book_id(o: &mut Cx<LogWorld>, key: &str) -> DefId {
    // SAFETY: the book is read only while the frame replays.
    let book = unsafe { &*o.w.book };
    match book.index.get(key) {
        Some(&id) => DefId(id),
        None => fail(format!("the explosion {key} is no definition's")),
    }
}

const FRAMES: [&str; 7] = ["ordnance.update", "ordnance.fireRound", "items.update", "items.dispense", "items.dropFor", "items.blast", "items.hurt"];

fn run_corpus(names: &[&str]) -> Vec<Report> {
    let Some(corpus) = Corpus::from_env().expect("the corpus BFE_REPLAY_CORPUS names") else {
        eprintln!("no BFE_REPLAY_CORPUS: nothing replayed");
        return Vec::new();
    };
    std::panic::set_hook(Box::new(|_| {}));
    let reports = names.iter().map(|name| replay_corpus(&corpus, name, replay_frame).expect("the corpus reads")).collect();
    let _ = std::panic::take_hook();
    reports
}

#[test]
fn one_ulp_off_shows() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(corpus) = Corpus::from_env().expect("the corpus BFE_REPLAY_CORPUS names") else { return };
    std::panic::set_hook(Box::new(|_| {}));
    NUDGE.store(true, Ordering::Relaxed);
    let report = replay_corpus(&corpus, "ordnance.update", replay_frame).expect("the corpus reads");
    NUDGE.store(false, Ordering::Relaxed);
    let _ = std::panic::take_hook();
    eprintln!("{}", report.summary());
    // Every frame with anything in flight parts, by a field or an argument.
    assert!(!report.failures.is_empty(), "{}", report.summary());
    assert!(report.failures.iter().all(|f| !f.contains("panicked")), "{}", report.summary());
}

#[test]
fn rounds_and_items_replay_as_captured() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let only = std::env::var("BFE_REPLAY_FRAMES").ok();
    let names: Vec<&str> = FRAMES.iter().copied().filter(|n| only.as_deref().is_none_or(|o| o.split(',').any(|x| x == *n))).collect();
    let reports = run_corpus(&names);
    let mut failed = false;
    for r in &reports {
        eprintln!("{}", r.summary());
        failed |= !r.failures.is_empty();
    }
    assert!(!failed, "frames parted from their captures");
}
