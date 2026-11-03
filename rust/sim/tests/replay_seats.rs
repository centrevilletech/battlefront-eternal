// The seats (src/seats.js, src/boarding.js, the hulls' seat methods of
// src/vehicles.js), replayed against every seat frame of the corpus
// BFE_REPLAY_CORPUS names (tools/sim-capture.mjs --specs seats). Each frame
// starts from the seats as captured (tools/sim-capture/seats.mjs: the leases
// that hold a seat, the hulls with someone aboard, the units in them, the
// seats the AI drives); its calls across the seats' boundary
// (src/seats-world.js) are served from the capture with every argument
// checked bit for bit; the seats it ends with are compared with the
// capture's. Without a corpus the test passes without replaying.
#![cfg(feature = "replay")]

use bfe_sim::gen::{Body, Post, Replay, SeatLease, SeatUnit, Seats, Store, Unit, Vehicle, KIND_BODY, KIND_POST, KIND_SEAT_LEASE, KIND_SEAT_UNIT};
use bfe_sim::seats::bodies::CombatFlags;
use bfe_sim::seats::hulls::{FlightState, HullSide, Hulls, RiderFacts, Vacate};
use bfe_sim::seats::lease::{control_key, SeatControl, SeatIntent};
use bfe_sim::seats::{Attacker, Hit, Holder, Reason, SeatWorld};
use bfe_sim::store::{Ref, Slab};
use bfe_sim::units::world::{CastHit, GatherPolygon, UnitsWorld};
use bfe_sim::vehicles::replay::body_of;
use sim_replay::{check_num, check_vec3, diff, replay_corpus, Arena, Call, Corpus, Frame, Log, Mismatch, Outcome, Value, V};
use std::collections::BTreeMap;
use three::Vector3;

fn vec3(v: V) -> Vector3 {
    let a = v.vec3().unwrap_or_else(|| panic!("not a Vector3: {}", v.show()));
    Vector3::new(a[0], a[1], a[2])
}

fn arr(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

// A frame's calls, served from its capture. `cx` maps the capture's
// handles to the store's slots (the store itself is the port's while the
// frame runs).
struct Served<'f, 'c> {
    log: Log<'f>,
    cx: &'c mut Replay,
    error: Option<String>,
    // What a kill or a posed rider left the units as.
    pending_units: Vec<(u32, V<'f>)>,
}

impl<'f, 'c> Served<'f, 'c> {
    fn fail(&mut self, what: String) {
        if self.error.is_none() {
            self.error = Some(format!("{}: {what}", self.log.frame.place()));
        }
    }

    fn call(&mut self, name: &str) -> Option<&'f Call> {
        if self.error.is_some() {
            return None;
        }
        match self.log.call(name) {
            Ok(call) => Some(call),
            Err(e) => {
                self.error = Some(e.0);
                None
            }
        }
    }

    fn check(&mut self, result: Result<(), Mismatch>) {
        if let Err(e) = result {
            self.fail(e.0);
        }
    }

    // A body named by a call: the store's reference, as the capture named
    // the body it stands for.
    fn check_body(&mut self, what: &str, v: V, mine: Option<Ref<Body>>) {
        let theirs = if v.is_nullish() { None } else { Some(body_of(v)) };
        let mine_serial = mine.map(|r| match self.cx.ref_out(KIND_BODY, r) {
            Value::Handle(_, key) => match *key {
                Value::Num(n) => n,
                other => panic!("a body's handle key: {other:?}"),
            },
            other => panic!("a body's handle: {other:?}"),
        });
        if theirs.map(|n| n as f64) != mine_serial {
            self.fail(format!("{what}: the capture has {}, the port {mine_serial:?}", v.show()));
        }
    }

    fn check_unit(&mut self, what: &str, v: V, mine: Option<u32>) {
        let theirs = v.handle_num("unit").map(|n| n as u32);
        if theirs != mine {
            self.fail(format!("{what}: the capture has {}, the port {mine:?}", v.show()));
        }
    }

    fn check_holder(&mut self, what: &str, v: V, mine: Holder) {
        let ok = match mine {
            Holder::Hull(i) => v.handle_num("vehicle") == Some(i as f64),
            Holder::Turret(i) => v.handle_num("turret") == Some(i as f64),
        };
        if !ok {
            self.fail(format!("{what}: the capture has {}, the port {mine:?}", v.show()));
        }
    }

    // A lease named by a call: one the port made in this frame takes the
    // handle the capture gives it the first time a call names it.
    fn check_lease(&mut self, what: &str, v: V, mine: Ref<SeatLease>) {
        let theirs = self.cx.stored_in(KIND_SEAT_LEASE, v, what, |_, _| (mine.slot, mine.gen)).ok().map(|(slot, gen)| Ref::new(slot, gen));
        if theirs != Some(mine) {
            self.fail(format!("{what}: the capture has {}, the port {mine:?}", v.show()));
        }
    }

    fn hit(&mut self, r: V) -> Option<Hit> {
        if r.is_nullish() {
            return None;
        }
        let body = r.get("body");
        Some(Hit {
            distance: r.get("distance").num(),
            point: vec3(r.get("point")),
            normal: vec3(r.get("normal")),
            terrain: r.get("terrain").truthy(),
            body: if body.is_nullish() { None } else { Some(body_of(body)) },
        })
    }

    // The units a call left as the capture has them: read into the units
    // when it returns.
    fn units_after(&mut self, call: &'f Call) {
        for u in call.extra().get("units").items() {
            let id = u.get("unit").get("$id").handle_num("unit").expect("a unit's handle") as u32;
            self.pending_units.push((id, u));
        }
    }

    fn apply_units(&mut self, units: &mut Slab<Unit>) {
        for (id, v) in std::mem::take(&mut self.pending_units) {
            let r = Ref::new(id, 0);
            if units.get(r).is_none() {
                continue;
            }
            match Unit::from_value(v.get("unit"), self.cx, "unit") {
                Ok(u) => *units.get_mut(r).unwrap() = u,
                Err(e) => self.fail(format!("reading a unit a call left: {e}")),
            }
        }
    }
}

// Soldier.spawn's cast, as the units' boundary (the port runs the units'
// spawn with the capture serving its collision).
struct SpawnCasts<'s, 'f, 'c>(&'s mut Served<'f, 'c>);

impl UnitsWorld for SpawnCasts<'_, '_, '_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
        let s = &mut *self.0;
        let call = s.call("collision.cast")?;
        s.check(check_vec3("spawn cast origin", call.arg(0), arr(origin)));
        s.check(check_vec3("spawn cast direction", call.arg(1), arr(direction)));
        s.check(check_num("spawn cast far", call.arg(2), far));
        let r = call.result();
        if r.is_nullish() {
            return None;
        }
        Some(CastHit { point: vec3(r.get("point")), normal: vec3(r.get("normal")), distance: r.get("distance").num(), terrain: r.get("terrain").truthy() })
    }
    fn gather(&mut self, _id: u32, _c: &Vector3, _r: f64, _cache: usize, _out: &mut Vec<GatherPolygon>) {
        unreachable!("a spawn gathers nothing")
    }
    fn seat_exposed(&mut self, _id: u32, _unit: &Unit) -> bool {
        unreachable!()
    }
    fn rider_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [bfe_sim::units::shapes::HitShape]) -> usize {
        unreachable!()
    }
    fn held_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [bfe_sim::units::shapes::HitShape]) -> usize {
        unreachable!()
    }
    fn target_alive(&mut self, _id: u32) -> bool {
        unreachable!()
    }
    fn target_dead(&mut self, _id: u32) -> bool {
        unreachable!()
    }
    fn target_sphere(&mut self, _id: u32) -> bfe_sim::units::shapes::Bounds {
        unreachable!()
    }
    fn move_soldier(&mut self, _id: u32, _unit: &mut Unit, _h: f64) {
        unreachable!()
    }
    fn damage(&mut self, _id: u32, _unit: &mut Unit, _amount: f64) {
        unreachable!()
    }
    fn aim(&mut self, _id: u32, _unit: &mut Unit, _h: f64) {
        unreachable!()
    }
    fn has_remote_step(&mut self, _id: u32, _unit: &Unit) -> bool {
        unreachable!()
    }
    fn remote_step(&mut self, _id: u32, _unit: &mut Unit, _h: f64, _active: bool) {
        unreachable!()
    }
    fn combat_reset(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!()
    }
    fn combat_update(&mut self, _id: u32, _unit: &mut Unit, _h: f64, _stick: f64) {
        unreachable!()
    }
    fn flush_muzzles(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!()
    }
    fn clear_charge(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!()
    }
    fn death_blast(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!()
    }
    fn blow_apart(&mut self, _id: u32, _unit: &mut Unit, _health: f64) {
        unreachable!()
    }
    fn let_go_of_character(&mut self, _id: u32, _unit: &mut Unit) {
        unreachable!()
    }
    fn release(&mut self, _id: u32, _unit: &Unit) {
        unreachable!()
    }
}

impl SeatWorld for Served<'_, '_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<Ref<Body>>) -> Option<Hit> {
        let call = self.call("seats.cast")?;
        self.check(check_vec3("cast origin", call.arg(0), arr(origin)));
        self.check(check_vec3("cast direction", call.arg(1), arr(direction)));
        self.check(check_num("cast far", call.arg(2), far));
        self.check_body("cast ignoreBody", call.arg(4), ignore);
        self.hit(call.result())
    }

    fn overlaps_sphere(&mut self, center: &Vector3, radius: f64, ignore: Option<Ref<Body>>) -> bool {
        let Some(call) = self.call("seats.overlapsSphere") else { return false };
        self.check(check_vec3("overlapsSphere center", call.arg(0), arr(center)));
        self.check(check_num("overlapsSphere radius", call.arg(1), radius));
        self.check_body("overlapsSphere ignoreBody", call.arg(2), ignore);
        call.result().truthy()
    }

    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64, exclude: Option<u32>) -> Option<f64> {
        let call = self.call("seats.raycast")?;
        self.check(check_vec3("raycast origin", call.arg(0), arr(origin)));
        self.check(check_vec3("raycast direction", call.arg(1), arr(direction)));
        self.check(check_num("raycast far", call.arg(2), far));
        self.check_unit("raycast exclude", call.arg(3), exclude);
        let r = call.result();
        if r.is_nullish() {
            None
        } else {
            Some(r.get("distance").num())
        }
    }

    fn relation(&mut self, a: f64, b: f64) -> f64 {
        let Some(call) = self.call("seats.relation") else { return 0.0 };
        self.check(check_num("relation a", call.arg(0), a));
        self.check(check_num("relation b", call.arg(1), b));
        call.result().num()
    }

    fn stand_up(&mut self, unit: &mut Unit, x: f64, y: f64, z: f64, yaw: f64) {
        let bfe_sim::gen::UnitBody::Soldier(s) = &mut unit.soldier else {
            self.fail("a droideka stood up out of a seat".into());
            return;
        };
        s.spawn(x, y, z, yaw, &mut SpawnCasts(self));
    }

    fn kill(&mut self, units: &mut Slab<Unit>, unit: u32, attacker: Attacker) {
        let Some(call) = self.call("seats.kill") else { return };
        self.check_unit("kill unit", call.arg(0), Some(unit));
        let a = call.arg(1);
        let ok = match attacker {
            Attacker::Nobody => a.is_nullish(),
            Attacker::Player => a.str() == Some("player"),
            Attacker::Unit(id) => a.handle_num("unit") == Some(id as f64),
            Attacker::Hull(i) => a.handle_num("vehicle") == Some(i as f64),
        };
        if !ok {
            self.fail(format!("kill attacker: the capture has {}, the port {attacker:?}", a.show()));
        }
        self.units_after(call);
        self.apply_units(units);
    }

    fn rider_drop(&mut self, holder: Holder, index: usize, unit: u32) -> Option<Vector3> {
        let call = self.call("seats.riderDrop")?;
        self.check_holder("riderDrop holder", call.arg(0), holder);
        self.check(check_num("riderDrop index", call.arg(1), index as f64));
        self.check_unit("riderDrop unit", call.arg(2), Some(unit));
        let r = call.result();
        if r.is_nullish() {
            None
        } else {
            Some(vec3(r))
        }
    }

    fn sync_rider(&mut self, lease: Ref<SeatLease>, unit: &mut Unit) {
        let Some(call) = self.call("vehicles.syncRider") else { return };
        self.check_lease("syncRider lease", call.arg(0), lease);
        // The unit in that lease, as the rider's pose left it.
        let posed = call.extra().get("units").items().map(|u| u.get("unit")).find(|u| {
            !u.get("seat").is_nullish() && SeatLease::ref_in(u.get("seat"), self.cx, "seat").ok() == Some(lease)
        });
        match posed {
            Some(v) => match Unit::from_value(v, self.cx, "unit") {
                Ok(u) => *unit = u,
                Err(e) => self.fail(format!("reading the posed rider: {e}")),
            },
            None => self.fail("syncRider: the capture has no unit in the lease".into()),
        }
    }

    fn controller_state(&mut self, unit: Option<u32>) -> Option<f64> {
        let call = self.call("seats.controllerState")?;
        self.check_unit("controllerState unit", call.arg(0), unit);
        call.result().opt_num()
    }

    fn controllable(&mut self, lease: Ref<SeatLease>) -> Ref<SeatUnit> {
        let Some(call) = self.call("seats.controllable") else { return Ref::new(u32::MAX, 0) };
        self.check_lease("controllable lease", call.arg(0), lease);
        match self.cx.ref_in(KIND_SEAT_UNIT, call.result(), "controllable") {
            Ok(r) => r,
            Err(e) => {
                self.fail(format!("controllable: {e}"));
                Ref::new(u32::MAX, 0)
            }
        }
    }

    fn boarded(&mut self, unit: u32, holder: Holder, index: usize) {
        let Some(call) = self.call("seats.boarded") else { return };
        self.check_unit("boarded unit", call.arg(0), Some(unit));
        self.check_holder("boarded holder", call.arg(1), holder);
        self.check(check_num("boarded seat", call.arg(2), index as f64));
    }

    fn unboarded(&mut self, unit: Option<u32>, holder: Holder, index: usize, post: Option<Ref<Post>>, reason: Reason) {
        let Some(call) = self.call("seats.unboarded") else { return };
        self.check_unit("unboarded unit", call.arg(0), unit);
        self.check_holder("unboarded holder", call.arg(1), holder);
        self.check(check_num("unboarded seat", call.arg(2), index as f64));
        let theirs = call.arg(3);
        let ok = match post {
            None => theirs.is_nullish(),
            Some(p) => self.cx.ref_in::<Post>(KIND_POST, theirs, "post").ok() == Some(p),
        };
        if !ok {
            self.fail(format!("unboarded post: the capture has {}, the port {post:?}", theirs.show()));
        }
        if call.arg(4).str() != Some(reason.name()) {
            self.fail(format!("unboarded reason: the capture has {}, the port {}", call.arg(4).show(), reason.name()));
        }
    }

    fn moved(&mut self, unit: Option<u32>, holder: Holder, index: usize) {
        let Some(call) = self.call("seats.moved") else { return };
        self.check_unit("moved unit", call.arg(0), unit);
        self.check_holder("moved holder", call.arg(1), holder);
        self.check(check_num("moved seat", call.arg(2), index as f64));
    }
}

// ---- A frame's state.

fn attacker_of(v: V) -> Attacker {
    if v.is_nullish() {
        Attacker::Nobody
    } else if v.str() == Some("player") {
        Attacker::Player
    } else if let Some(id) = v.handle_num("unit") {
        Attacker::Unit(id as u32)
    } else if let Some(i) = v.handle_num("vehicle") {
        Attacker::Hull(i as usize)
    } else {
        panic!("an attacker that is none of nobody, the player, a unit or a hull: {}", v.show())
    }
}

fn opt_num(v: V) -> Option<f64> {
    v.opt_num()
}

fn intent_of(v: V) -> SeatIntent {
    let aim = v.get("aim");
    SeatIntent {
        forward: v.get("forward").num(),
        right: v.get("right").num(),
        turn: v.get("turn").num(),
        pitch: v.get("pitch").num(),
        jump: v.get("jump").bool(),
        crouch: v.get("crouch").bool(),
        prone: v.get("prone").bool(),
        r#use: v.get("use").bool(),
        exit: v.get("exit").bool(),
        buttons: [v.get("buttons").at(0).num(), v.get("buttons").at(1).num()],
        reload: v.get("reload").bool(),
        next_primary: v.get("nextPrimary").bool(),
        previous_primary: v.get("previousPrimary").bool(),
        next_secondary: v.get("nextSecondary").bool(),
        zoom: v.get("zoom").bool(),
        aim: if aim.is_null() { None } else { Some(vec3(aim)) },
        brake: v.get("brake").bool(),
        up: v.get("up").num(),
        down: v.get("down").num(),
    }
}

fn intent_value(a: &mut Arena, i: &SeatIntent) -> Value {
    let buttons = a.array(vec![Value::Num(i.buttons[0]), Value::Num(i.buttons[1])]);
    let aim = match &i.aim {
        Some(v) => a.vec3(arr(v)),
        None => Value::Null,
    };
    a.object(
        "",
        vec![
            ("forward", Value::Num(i.forward)),
            ("right", Value::Num(i.right)),
            ("turn", Value::Num(i.turn)),
            ("pitch", Value::Num(i.pitch)),
            ("jump", Value::Bool(i.jump)),
            ("crouch", Value::Bool(i.crouch)),
            ("prone", Value::Bool(i.prone)),
            ("use", Value::Bool(i.r#use)),
            ("exit", Value::Bool(i.exit)),
            ("buttons", buttons),
            ("reload", Value::Bool(i.reload)),
            ("nextPrimary", Value::Bool(i.next_primary)),
            ("previousPrimary", Value::Bool(i.previous_primary)),
            ("nextSecondary", Value::Bool(i.next_secondary)),
            ("zoom", Value::Bool(i.zoom)),
            ("aim", aim),
            ("brake", Value::Bool(i.brake)),
            ("up", Value::Num(i.up)),
            ("down", Value::Num(i.down)),
        ],
    )
}

/// The seats as a frame starts: read into `cx.store`; the hulls' side.
fn read_state(cx: &mut Replay, s: V) -> Result<Hulls, String> {
    let mut hulls = Hulls::default();
    let system = s.get("system");
    if !system.is_nullish() {
        hulls.time = system.get("time").num();
        hulls.ticks = system.get("ticks").num();
    }
    cx.store.seats = Seats::from_value(s.get("seats"), cx, "seats")?;
    for (i, h) in s.get("hulls").items().enumerate() {
        let r = Vehicle::value_in(h.get("hull"), cx, &format!("hulls[{i}]"))?;
        let side = h.get("side");
        let stick = side.get("stick");
        hulls.hulls.push(HullSide {
            index: h.get("index").num() as usize,
            vehicle: r,
            team: opt_num(side.get("team")),
            flight_state: side.get("flightState").str().map(|n| FlightState::named(n).expect("a flight state")),
            last_attacker: attacker_of(side.get("lastAttacker")),
            stick: if stick.is_null() { None } else { Some((stick.at(0).num(), stick.at(1).num())) },
            hover: side.get("hover").bool(),
            animal: side.get("animal").bool(),
            turret_nodes: side.get("turretNodes").items().map(|x| x.bool()).collect(),
            weapons: side.get("weapons").items().map(|x| x.num() as usize).collect(),
        });
    }
    hulls.hulls.sort_by_key(|h| h.index);
    for (i, u) in s.get("units").items().enumerate() {
        let r = Unit::value_in(u.get("unit"), cx, &format!("units[{i}]"))?;
        let c = u.get("combat");
        hulls.riders.insert(
            r.slot,
            RiderFacts {
                combat: CombatFlags { zoom_step: c.get("zoomStep").num(), zoom: c.get("zoom").num(), first_person: c.get("firstPerson").bool(), blow_apart: c.get("blowApart").bool() },
                no_enter: u.get("noEnter").num(),
            },
        );
    }
    for c in s.get("controls").items() {
        let lease = SeatLease::ref_in(c.get("lease"), cx, "controls.lease")?;
        let unit = cx.store.seat_lease.get(lease).and_then(|l| l.controllable).ok_or("a control of a lease without one")?;
        hulls.controls.insert(control_key(unit), SeatControl { driven: c.get("driven").bool(), intent: intent_of(c.get("intent")) });
    }
    Ok(hulls)
}

/// The leases the capture lists at the frame's end: the hulls' seats' (hull
/// by hull, seat by seat), then the turrets' as they were, then those the
/// frame names that hold none.
fn exit_leases(st: &Store, hulls: &Hulls, entry: &[Ref<SeatLease>], named: &[Ref<SeatLease>]) -> Vec<Ref<SeatLease>> {
    let mut out = Vec::new();
    for h in &hulls.hulls {
        for seat in &st.vehicle.get(h.vehicle).unwrap().seats {
            if let Some(r) = seat.lease {
                out.push(r);
            }
        }
    }
    for &r in entry {
        if st.seat_lease.get(r).is_some_and(|l| l.turret.is_some()) && !out.contains(&r) {
            out.push(r);
        }
    }
    for &r in named {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// The seats as the port ends the frame, in the capture's shape: the units
/// and hulls the capture lists, by their handles.
fn write_state(cx: &mut Replay, hulls: &Hulls, leases: &[Ref<SeatLease>], after: V) -> (Arena, Value) {
    let mut a = Arena::default();
    let mut fields: Vec<(&str, Value)> = Vec::new();
    let system = if after.get("system").is_nullish() {
        Value::Null
    } else {
        a.object("", vec![("time", Value::Num(hulls.time)), ("ticks", Value::Num(hulls.ticks))])
    };
    fields.push(("system", system));
    cx.store.seats.leases = leases.to_vec();
    let seats = cx.store.seats.clone();
    let seats = seats.to_value(cx, &mut a);
    fields.push(("seats", seats));
    let mut hull_values = Vec::new();
    for h in after.get("hulls").items() {
        let index = h.get("index").num() as usize;
        let Some(side) = hulls.hulls.iter().find(|s| s.index == index) else {
            hull_values.push(Value::Undefined);
            continue;
        };
        let hull = Vehicle::value_out(side.vehicle, cx, &mut a);
        let team = side.team.map_or(Value::Undefined, Value::Num);
        let flight = match side.flight_state {
            None => Value::Null,
            Some(f) => Value::Str(format!("{f:?}")),
        };
        let attacker = match side.last_attacker {
            Attacker::Nobody => Value::Null,
            Attacker::Player => Value::Str("player".into()),
            Attacker::Unit(id) => Value::handle("unit", Value::Num(id as f64)),
            Attacker::Hull(i) => Value::handle("vehicle", Value::Num(i as f64)),
        };
        let stick = match side.stick {
            None => Value::Null,
            Some((x, y)) => a.array(vec![Value::Num(x), Value::Num(y)]),
        };
        let nodes = side.turret_nodes.iter().map(|&b| Value::Bool(b)).collect();
        let nodes = a.array(nodes);
        let weapons = side.weapons.iter().map(|&n| Value::Num(n as f64)).collect();
        let weapons = a.array(weapons);
        let side_value = a.object(
            "",
            vec![
                ("team", team),
                ("flightState", flight),
                ("lastAttacker", attacker),
                ("stick", stick),
                ("hover", Value::Bool(side.hover)),
                ("animal", Value::Bool(side.animal)),
                ("turretNodes", nodes),
                ("weapons", weapons),
            ],
        );
        let v = a.object("", vec![("index", Value::Num(index as f64)), ("hull", hull), ("side", side_value)]);
        hull_values.push(v);
    }
    let hull_values = a.array(hull_values);
    fields.push(("hulls", hull_values));
    let mut unit_values = Vec::new();
    for u in after.get("units").items() {
        let id = u.get("unit").get("$id").handle_num("unit").unwrap() as u32;
        let r = Ref::new(id, 0);
        if cx.store.unit.get(r).is_none() {
            unit_values.push(Value::Undefined);
            continue;
        }
        let unit = Unit::value_out(r, cx, &mut a);
        let f = hulls.riders.get(&id).copied().unwrap_or_default();
        let combat = a.object(
            "",
            vec![
                ("zoomStep", Value::Num(f.combat.zoom_step)),
                ("zoom", Value::Num(f.combat.zoom)),
                ("firstPerson", Value::Bool(f.combat.first_person)),
                ("blowApart", Value::Bool(f.combat.blow_apart)),
            ],
        );
        let v = a.object("", vec![("unit", unit), ("combat", combat), ("noEnter", Value::Num(f.no_enter))]);
        unit_values.push(v);
    }
    let unit_values = a.array(unit_values);
    fields.push(("units", unit_values));
    let mut controls = Vec::new();
    for &r in leases {
        let Some(c) = cx.store.seat_lease.get(r).and_then(|l| l.controllable) else { continue };
        let lease = SeatLease::ref_out(r, cx, &mut a);
        let control = hulls.controls.get(&control_key(c)).cloned().unwrap_or_default();
        let intent = intent_value(&mut a, &control.intent);
        controls.push(a.object("", vec![("lease", lease), ("driven", Value::Bool(control.driven)), ("intent", intent)]));
    }
    let controls = a.array(controls);
    fields.push(("controls", controls));
    let v = a.object("", fields);
    (a, v)
}

/// vacate's options (`{ placement, promote, post, policy, force, reason }`).
fn vacate_of(cx: &mut Replay, v: V) -> Vacate {
    let mut how = Vacate::default();
    if v.is_nullish() {
        return how;
    }
    if !v.get("placement").is_undefined() {
        how.placement = v.get("placement").truthy();
    }
    if !v.get("promote").is_undefined() {
        how.promote = Some(v.get("promote").truthy());
    }
    let post = v.get("post");
    if !post.is_nullish() {
        how.post = cx.ref_in::<Post>(KIND_POST, post, "post").ok();
    }
    how.safe = v.get("policy").str() == Some("safe");
    how.force = v.get("force").truthy();
    how.reason = match v.get("reason").str() {
        None | Some("exit") => Reason::Exit,
        Some("stale") => Reason::Stale,
        Some("killed") => Reason::Killed,
        Some("destroyed") => Reason::Destroyed,
        Some("displaced") => Reason::Displaced,
        Some(r) => panic!("a reason to leave a seat: {r}"),
    };
    how
}

fn place_of(hulls: &Hulls, v: V) -> Option<usize> {
    let index = v.handle_num("vehicle")? as usize;
    hulls.hulls.iter().position(|h| h.index == index)
}

/// One hull seat frame: its state read, its method run, its end compared.
fn replay_hull_frame(frame: &Frame, name: &str, tally: &mut BTreeMap<String, usize>) -> Outcome {
    let mut cx = Replay::default();
    let mut hulls = match read_state(&mut cx, frame.before()) {
        Ok(h) => h,
        Err(e) => return Outcome::Failed(format!("{}: reading the state: {e}", frame.place())),
    };
    let entry: Vec<Ref<SeatLease>> = cx.store.seats.leases.clone();
    let mut named: Vec<Ref<SeatLease>> = Vec::new();
    let mut st = std::mem::take(&mut cx.store);
    let mut served = Served { log: Log::new(frame), cx: &mut cx, error: None, pending_units: Vec::new() };
    let result: Option<bool>;
    match name {
        "vehicles.latchCrews" => {
            hulls.latch_crews(&mut st, &mut served);
            result = None;
        }
        "vehicles.board" => {
            let Some(unit) = frame.arg(0).handle_num("unit") else { return Outcome::Failed(format!("{}: board's unit {}", frame.place(), frame.arg(0).show())) };
            let Some(hull) = place_of(&hulls, frame.arg(1)) else { return Outcome::Failed(format!("{}: board's hull {}", frame.place(), frame.arg(1).show())) };
            let seat = frame.arg(2).opt_num().unwrap_or(-1.0) as i32;
            result = Some(hulls.board(&mut st, &mut served, unit as u32, hull, seat));
            *tally.entry(format!("board {}", if result == Some(true) { "taken" } else { "refused" })).or_default() += 1;
        }
        "vehicles.vacate" => {
            let r = match SeatLease::ref_in(frame.arg(0), served.cx, "vacate lease") {
                Ok(r) => r,
                Err(e) => return Outcome::Failed(format!("{}: {e}", frame.place())),
            };
            named.push(r);
            let how = vacate_of(served.cx, frame.arg(1));
            result = Some(hulls.vacate(&mut st, &mut served, r, how));
            *tally.entry(format!("vacate {} {}", how.reason.name(), if result == Some(true) { "out" } else { "refused" })).or_default() += 1;
        }
        "vehicles.destroyedCrew" => {
            let Some(hull) = place_of(&hulls, frame.arg(0)) else { return Outcome::Failed(format!("{}: destroyedCrew's hull {}", frame.place(), frame.arg(0).show())) };
            hulls.destroyed_crew(&mut st, &mut served, hull);
            result = None;
        }
        "vehicles.requestExit" => {
            let Some(unit) = frame.arg(0).handle_num("unit") else { return Outcome::Failed(format!("{}: requestExit's unit {}", frame.place(), frame.arg(0).show())) };
            let o = frame.arg(1);
            let post = if o.is_nullish() || o.get("post").is_nullish() { None } else { served.cx.ref_in::<Post>(KIND_POST, o.get("post"), "post").ok() };
            let promote = if o.is_nullish() || o.get("promote").is_undefined() { None } else { Some(o.get("promote").truthy()) };
            let placement = if o.is_nullish() || o.get("placement").is_undefined() { true } else { o.get("placement").truthy() };
            result = Some(hulls.request_exit(&mut st, unit as u32, post, promote, placement));
        }
        _ => return Outcome::Skipped(format!("{name}: not a hull seat frame")),
    }
    if let Some(e) = served.error.take() {
        return Outcome::Failed(e);
    }
    if let Err(e) = served.log.finish() {
        return Outcome::Failed(e.0);
    }
    if let Some(mine) = result {
        if frame.result().opt_bool() != Some(mine) {
            return Outcome::Failed(format!("{}: returned {}, the port {mine}", frame.place(), frame.result().show()));
        }
    }
    let leases = exit_leases(&st, &hulls, &entry, &named);
    for h in &hulls.hulls {
        bfe_sim::seats::hulls::lease_paths(&mut st, h.vehicle);
    }
    cx.store = st;
    // A lease the port made that no call named takes the handle the
    // capture gives the lease at its place among the leases.
    let after = frame.after();
    for r in hulls.made.clone() {
        if let Some(at) = leases.iter().position(|x| *x == r) {
            let v = after.get("seats").get("leases").at(at);
            if !v.is_undefined() {
                let _ = cx.stored_in(KIND_SEAT_LEASE, v.get("$id"), "made", |_, _| (r.slot, r.gen));
            }
        }
    }
    let (a, mine) = write_state(&mut cx, &hulls, &leases, after);
    match diff(after, a.view(&mine)) {
        None => Outcome::Same,
        Some(d) => Outcome::Failed(format!("{}: seats {d} (capture | port)", frame.place())),
    }
}

#[test]
fn hull_seats_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    for name in ["vehicles.latchCrews", "vehicles.board", "vehicles.vacate", "vehicles.destroyedCrew", "vehicles.requestExit"] {
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        let report = replay_corpus(&corpus, name, |_capture, frame| replay_hull_frame(frame, name, &mut tally)).unwrap();
        eprintln!("{}", report.summary());
        for (what, n) in &tally {
            eprintln!("  {what}: {n}");
        }
        report.assert_clean();
    }
}

// (A seat unit's and a body's handles are the store's refs to kinds the
// store only names: their types are named here to keep them in use.)
#[allow(dead_code)]
fn _kinds(_: Ref<SeatUnit>, _: Ref<Body>) {}
