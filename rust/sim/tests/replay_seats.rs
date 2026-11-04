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

use bfe_sim::gen::{Body, Post, Replay, SeatLease, SeatRole, SeatUnit, Seats, Store, TurretSeats, Unit, Vehicle, KIND_BODY, KIND_POST, KIND_SEAT_LEASE, KIND_SEAT_UNIT, KIND_TURRET};
use bfe_sim::seats::bodies::CombatFlags;
use bfe_sim::seats::hulls::{HullSide, Hulls, RiderFacts, Vacate};
use bfe_sim::seats::boarding::Mount;
use bfe_sim::seats::network::{network_vehicle_input, SeatCommand, SeatKeys, SeatKeysOut};
use bfe_sim::seats::turrets::{CrewWorld, Crews, Let, Shot, TurretDesc, TurretRig, TurretSide};
use bfe_sim::seats::lease::{control_key, SeatControl, SeatIntent};
use bfe_sim::seats::{Attacker, Hit, Holder, Reason, SeatWorld, Seating};
use bfe_sim::store::Ref;
use bfe_sim::units::world::{CastHit, GatherPolygon, UnitsWorld};
use bfe_sim::vehicles::replay::body_of;
use sim_replay::{check_num, check_vec3, diff, replay_corpus, Arena, Call, Corpus, Frame, Log, Mismatch, Outcome, Value, V};
use std::collections::BTreeMap;
use three::{Quaternion, Vector3};

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
    // The labels' prefix: the hulls' seats' world ("seats") or the
    // crews' ("crews").
    prefix: &'static str,
    // Each turret of a crew frame by its place among the battle's turrets:
    // its handle, its target in the registry and its body, as captured.
    turrets: BTreeMap<usize, TurretFacts<'f>>,
    // The shots the last weapon update fired, and each round fired.
    shots: Vec<V<'f>>,
    rounds: Vec<V<'f>>,
    // Each lease's seat control (its SeatUnit's key), for what a kill did.
    lease_controls: BTreeMap<(u32, u32), (u32, u32)>,
    // Where a frame's team relations come from: the seats' world
    // ("seats.relation"), or the hulls' own (mountInReach reads
    // `vehicles.relation`).
    relation_label: &'static str,
}

struct TurretFacts<'f> {
    handle: V<'f>,
    target: V<'f>,
    body: V<'f>,
    ordnance: V<'f>,
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
        let name = match name.strip_prefix("seats.") {
            Some(rest) => format!("{}.{rest}", self.prefix),
            None => name.to_string(),
        };
        match self.log.call(&name) {
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
            Holder::Turret(i) => self.turrets.get(&i).is_some_and(|t| sim_replay::same(t.handle, v)),
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
            body: if body.is_nullish() { None } else { self.cx.ref_in::<Body>(KIND_BODY, body, "hit.body").ok() },
        })
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
        let label = self.relation_label;
        let Some(call) = self.call(label) else { return 0.0 };
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

    fn kill(&mut self, st: &mut Store, seating: Seating<'_>, unit: u32, attacker: Attacker) {
        let Some(call) = self.call("seats.kill") else { return };
        self.check_unit("kill unit", call.arg(0), Some(unit));
        let a = call.arg(1);
        let ok = match attacker {
            Attacker::Nobody => a.is_nullish(),
            Attacker::Player => a.str() == Some("player"),
            Attacker::Unit(id) => a.handle_num("unit") == Some(id as f64),
            Attacker::Hull(i) => a.handle_num("vehicle") == Some(i as f64),
            Attacker::Record(record) => { let mut arena = Arena::default(); let value = record.to_value(self.cx, &mut arena); sim_replay::same(a, arena.view(&value)) },
        };
        if !ok {
            self.fail(format!("kill attacker: the capture has {}, the port {attacker:?}", a.show()));
        }
        // The seats as the death's hooks left them, read over the port's.
        let after = call.extra();
        std::mem::swap(&mut self.cx.store, st);
        let read = match seating {
            Seating::Hulls(h) => load_hulls(self.cx, after, h),
            Seating::Crews(c) => {
                let entry = call.call.root().get("s");
                if !entry.is_undefined() {
                    let named = entry.get("seats").get("leases").items().map(|l| SeatLease::ref_in(l.get("$id"), self.cx, "kill entry lease")).collect::<Result<Vec<_>, _>>();
                    match named {
                        Ok(named) => {
                            let (arena, mine) = write_crews(self.cx, c, entry, entry, &named);
                            if let Some(d) = diff(entry, arena.view(&mine)) { self.fail(format!("crew kill entry {d} (capture | port)")); }
                        }
                        Err(e) => self.fail(format!("crew kill entry: {e}")),
                    }
                }
                load_crews(self.cx, after, c)
            }
        };
        std::mem::swap(&mut self.cx.store, st);
        if let Err(e) = read {
            self.fail(format!("reading the seats a kill left: {e}"));
        }
        self.lease_controls = lease_controls(st);
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

    fn controllable(&mut self, lease: Ref<SeatLease>, _index: usize, _role: SeatRole) -> Ref<SeatUnit> {
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

impl CrewWorld for Served<'_, '_> {
    fn update_lock(&mut self, turret: usize, dt: f64, origin: &Vector3, dir: &Vector3, lease: Ref<SeatLease>) {
        let Some(call) = self.call("seats.updateLock") else { return };
        self.check(check_num("updateLock dt", call.arg(1), dt));
        self.check(check_vec3("updateLock origin", call.arg(2), arr(origin)));
        self.check(check_vec3("updateLock dir", call.arg(3), arr(dir)));
        let (target, body) = match self.turrets.get(&turret) {
            Some(t) => (t.target, t.body),
            None => return self.fail(format!("updateLock: no turret {turret}")),
        };
        if !sim_replay::same(call.arg(4), target) {
            self.fail(format!("updateLock viewer: the capture has {}, the turret's target {}", call.arg(4).show(), target.show()));
        }
        if !sim_replay::same(call.arg(5), body) && !(call.arg(5).is_nullish() && body.is_nullish()) {
            self.fail(format!("updateLock ignoreBody: the capture has {}, the turret's body {}", call.arg(5).show(), body.show()));
        }
        self.check_lease("updateLock lease", call.arg(6), lease);
    }

    fn weapon_update(&mut self, _turret: usize, dt: f64, held: bool, pressed: bool, reload: bool, out: &mut Vec<Shot>) {
        let Some(call) = self.call("seats.weaponUpdate") else { return Vec::new() };
        self.check(check_num("weaponUpdate dt", call.arg(1), dt));
        if !call.arg(2).truthy() {
            self.fail("weaponUpdate selected: not true".into());
        }
        let trigger = call.arg(3);
        if trigger.get("held").opt_bool() != Some(held) || trigger.get("pressed").opt_bool() != Some(pressed) {
            self.fail(format!("weaponUpdate trigger: the capture has {}, the port held {held} pressed {pressed}", trigger.show()));
        }
        if call.arg(4).opt_bool() != Some(reload) {
            self.fail(format!("weaponUpdate reload: the capture has {}, the port {reload}", call.arg(4).show()));
        }
        self.shots = call.result().items().collect();
        out.extend(self.shots.iter().map(|s| Shot { yaw: s.get("yaw").opt_num().unwrap_or(0.0), pitch: s.get("pitch").opt_num().unwrap_or(0.0) }));
    }

    fn fire(&mut self, turret: usize, origin: &Vector3, direction: &Vector3, owner: Option<u32>, lease: Ref<SeatLease>, shot: usize) -> usize {
        let Some(call) = self.call("seats.fire") else { return 0 };
        let ordnance = self.turrets.get(&turret).map(|t| t.ordnance);
        if !ordnance.is_some_and(|o| sim_replay::same(o, call.arg(0))) {
            self.fail(format!("fire ordnance: the capture has {}", call.arg(0).show()));
        }
        self.check(check_vec3("fire origin", call.arg(1), arr(origin)));
        self.check(check_vec3("fire direction", call.arg(2), arr(direction)));
        self.check_unit("fire owner", call.arg(3), owner);
        let body = self.turrets.get(&turret).map(|t| t.body);
        if !body.is_some_and(|b| sim_replay::same(b, call.arg(4)) || b.is_nullish() && call.arg(4).is_nullish()) {
            self.fail(format!("fire ignoreBody: the capture has {}", call.arg(4).show()));
        }
        let _ = lease;
        match self.shots.get(shot) {
            Some(s) if sim_replay::same(*s, call.arg(6)) => {}
            _ => self.fail(format!("fire shot: the capture has {}", call.arg(6).show())),
        }
        self.rounds.push(call.result());
        self.rounds.len() - 1
    }

    fn bind_round(&mut self, round: usize, turret: usize, barrel: f64) {
        let Some(call) = self.call("seats.bindRound") else { return };
        match self.rounds.get(round) {
            Some(r) if sim_replay::same(*r, call.arg(0)) => {}
            _ => self.fail(format!("bindRound round: the capture has {}", call.arg(0).show())),
        }
        self.check_holder("bindRound mount", call.arg(1), Holder::Turret(turret));
        self.check(check_num("bindRound seat", call.arg(2), -1.0));
        self.check(check_num("bindRound weapon", call.arg(3), -1.0));
        self.check(check_num("bindRound barrel", call.arg(4), barrel));
    }

    fn sound(&mut self, turret: usize, shot: usize) {
        let Some(call) = self.call("seats.sound") else { return };
        if call.arg(0).str() != Some("turretFire") {
            self.fail(format!("sound type: the capture has {}", call.arg(0).show()));
        }
        self.check_holder("sound turret", call.arg(1), Holder::Turret(turret));
        match self.shots.get(shot) {
            Some(s) if sim_replay::same(*s, call.arg(2)) => {}
            _ => self.fail(format!("sound shot: the capture has {}", call.arg(2).show())),
        }
    }

    fn fired(&mut self, turret: usize, shot: usize, owner: Option<u32>, muzzle: &Vector3, direction: &Vector3) {
        let Some(call) = self.call("seats.fired") else { return };
        self.check_holder("fired turret", call.arg(0), Holder::Turret(turret));
        self.check(check_num("fired seat", call.arg(1), 0.0));
        match self.shots.get(shot) {
            Some(s) if sim_replay::same(*s, call.arg(2)) => {}
            _ => self.fail(format!("fired shot: the capture has {}", call.arg(2).show())),
        }
        self.check_unit("fired owner", call.arg(3), owner);
        self.check(check_vec3("fired muzzle", call.arg(4), arr(muzzle)));
        self.check(check_vec3("fired direction", call.arg(5), arr(direction)));
    }
}

/// Each lease's seat control key, as a frame starts.
fn lease_controls(st: &Store) -> BTreeMap<(u32, u32), (u32, u32)> {
    st.seat_lease.iter().filter_map(|(r, l)| l.controllable.map(|c| ((r.slot, r.gen), control_key(c)))).collect()
}

// ---- A frame's state.

fn attacker_of(v: V, cx: &mut Replay) -> Attacker {
    if v.is_nullish() {
        Attacker::Nobody
    } else if v.str() == Some("player") {
        Attacker::Player
    } else if let Some(id) = v.handle_num("unit") {
        Attacker::Unit(id as u32)
    } else if let Some(i) = v.handle_num("vehicle") {
        Attacker::Hull(i as usize)
    } else {
        Attacker::Record(bfe_sim::gen::Attacker::from_value(v, cx, "seat attacker").expect("combat attacker identity"))
    }
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
        // (Captures before c3 lack these three.)
        trick: v.get("trick").opt_bool().unwrap_or(false),
        immelmann: v.get("immelmann").opt_bool().unwrap_or(false),
        boost: v.get("boost").opt_bool().unwrap_or(false),
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
            ("trick", Value::Bool(i.trick)),
            ("immelmann", Value::Bool(i.immelmann)),
            ("boost", Value::Bool(i.boost)),
        ],
    )
}

/// The seats as a frame starts: read into `cx.store`; the hulls' side.
fn read_state(cx: &mut Replay, s: V) -> Result<Hulls, String> {
    let mut hulls = Hulls::default();
    load_hulls(cx, s, &mut hulls)?;
    Ok(hulls)
}

/// The seats in a frame's state shape, read into `cx.store` and over
/// `hulls` (the leases it made in the frame kept).
fn load_hulls(cx: &mut Replay, s: V, hulls: &mut Hulls) -> Result<(), String> {
    let system = s.get("system");
    if !system.is_nullish() {
        hulls.time = system.get("time").num();
        hulls.ticks = system.get("ticks").num();
    }
    cx.store.seats = Seats::from_value(s.get("seats"), cx, "seats")?;
    for (i, h) in s.get("hulls").items().enumerate() {
        let r = Vehicle::value_in(h.get("hull"), cx, &format!("hulls[{i}]"))?;
        let side = h.get("side");
        let mine = HullSide {
            index: h.get("index").num() as usize,
            vehicle: r,
            last_attacker: attacker_of(side.get("lastAttacker"), cx),
            hover: side.get("hover").bool(),
            animal: side.get("animal").bool(),
            turret_nodes: side.get("turretNodes").items().map(|x| x.bool()).collect(),
            weapons: side.get("weapons").items().map(|x| x.num() as usize).collect(),
        };
        match hulls.hulls.iter_mut().find(|x| x.index == mine.index) {
            Some(x) => *x = mine,
            None => hulls.hulls.push(mine),
        }
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
    Ok(())
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
        let attacker = match side.last_attacker {
            Attacker::Nobody => Value::Null,
            Attacker::Player => Value::Str("player".into()),
            Attacker::Unit(id) => Value::handle("unit", Value::Num(id as f64)),
            Attacker::Hull(i) => Value::handle("vehicle", Value::Num(i as f64)),
            Attacker::Record(record) => record.to_value(cx, &mut a),
        };
        let nodes = side.turret_nodes.iter().map(|&b| Value::Bool(b)).collect();
        let nodes = a.array(nodes);
        let weapons = side.weapons.iter().map(|&n| Value::Num(n as f64)).collect();
        let weapons = a.array(weapons);
        let side_value = a.object(
            "",
            vec![
                ("lastAttacker", attacker),
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
    let mut served = Served { log: Log::new(frame), cx: &mut cx, error: None, prefix: "seats", turrets: BTreeMap::new(), shots: Vec::new(), rounds: Vec::new(), lease_controls: lease_controls(&st), relation_label: "seats.relation" };
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
        "networkSeat.cycle" => {
            let Some(unit) = frame.arg(1).handle_num("unit") else { return Outcome::Failed(format!("{}: cycle's unit {}", frame.place(), frame.arg(1).show())) };
            let direction = frame.arg(2).opt_num().unwrap_or(1.0) as i32;
            result = Some(bfe_sim::seats::network::cycle_network_seat(&mut hulls, &mut st, &mut served, unit as u32, direction));
            *tally.entry(format!("cycle {}", if result == Some(true) { "moved" } else { "stayed" })).or_default() += 1;
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
        bfe_sim::seats::hulls::lease_views(&mut st, h.vehicle);
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

// ---- The turret crews' frames.

fn q4(v: V) -> Quaternion {
    match v.node() {
        Some(sim_replay::Node::Quat(q)) => Quaternion::new(q[0], q[1], q[2], q[3]),
        _ => panic!("not a Quaternion: {}", v.show()),
    }
}

fn pair(v: V) -> [f64; 2] {
    [v.at(0).num(), v.at(1).num()]
}

fn node_at(rig: &TurretRig, v: V) -> Option<three::NodeId> {
    v.opt_num().map(|i| rig.bones[i as usize])
}

/// A turret as a crew frame's state has it.
fn turret_of(cx: &mut Replay, t: V) -> Result<TurretSide, String> {
    let d = t.get("desc");
    let cam = d.get("camera");
    let body = d.get("body");
    let rig_v = t.get("rig");
    let mut root = [0.0; 16];
    for (i, x) in rig_v.get("root").items().enumerate() {
        root[i] = x.num();
    }
    let bones: Vec<_> = rig_v
        .get("bones")
        .items()
        .map(|b| {
            let parent = b.get("parent").num();
            (if parent < 0.0 { None } else { Some(parent as usize) }, vec3(b.get("position")), q4(b.get("quaternion")), vec3(b.get("scale")))
        })
        .collect();
    let world: Vec<[f64; 16]> = rig_v
        .get("world")
        .items()
        .map(|m| {
            let mut e = [0.0; 16];
            for (i, x) in m.items().enumerate() {
                e[i] = x.num();
            }
            e
        })
        .collect();
    let mut rig = TurretRig::new(&root, &bones, Some(&world));
    rig.drum = node_at(&rig, d.get("drum"));
    rig.gun = node_at(&rig, d.get("gun"));
    rig.seat = node_at(&rig, d.get("seat"));
    rig.fire_points = d.get("firePoints").items().map(|i| rig.bones[i.num() as usize]).collect();
    let yaw_limits = d.get("yawLimits");
    let lease = t.get("lease");
    let gunner = t.get("gunner");
    let weapon = t.get("weapon");
    Ok(TurretSide {
        index: t.get("index").num() as usize,
        turret: cx.ref_in(KIND_TURRET, t.get("turret"), "turret")?,
        alive: t.get("alive").bool(),
        lease: if lease.is_null() { None } else { Some(SeatLease::ref_in(lease, cx, "turret.lease")?) },
        gunner: if gunner.is_null() { None } else { Some(gunner.handle_num("unit").ok_or(format!("a gunner that is no unit: {}", gunner.show()))? as u32) },
        yaw: t.get("yaw").num(),
        pitch: t.get("pitch").num(),
        yaw_vel: t.get("yawVel").num(),
        pitch_vel: t.get("pitchVel").num(),
        gun_yaw: t.get("gunYaw").num(),
        gun_pitch: t.get("gunPitch").num(),
        barrel: t.get("barrel").num(),
        last_attacker: attacker_of(t.get("lastAttacker"), cx),
        sphere: (vec3(t.get("sphere").get("center")), t.get("sphere").get("radius").num()),
        weapon: !weapon.is_null(),
        missile_lock: !weapon.is_null() && weapon.get("missileLock").bool(),
        desc: TurretDesc {
            facing: d.get("facing").num(),
            position: vec3(d.get("position")),
            body: if body.is_nullish() { None } else { Some(cx.ref_in::<Body>(KIND_BODY, body, "body")?) },
            pitch_limits: pair(d.get("pitchLimits")),
            yaw_limits: if yaw_limits.is_null() { None } else { Some(pair(yaw_limits)) },
            wrap_yaw: d.get("wrapYaw").bool(),
            gun_pitch_limits: pair(d.get("gunPitchLimits")),
            gun_yaw_limits: pair(d.get("gunYawLimits")),
            turn_rate: d.get("turnRate").num(),
            filter: d.get("filter").num(),
            gun_pitch_rate: d.get("gunPitchRate").num(),
            gun_yaw_rate: d.get("gunYawRate").num(),
            yaw_factor: d.get("yawFactor").num(),
            pitch_factor: d.get("pitchFactor").num(),
            camera_eye: vec3(cam.get("eye")),
            camera_center: vec3(cam.get("center")),
            camera_offset: vec3(cam.get("offset")),
            camera_tilt: cam.get("tilt").num(),
            pivot: vec3(d.get("pivot")),
            up: vec3(d.get("up")),
            drum_parent_turn: if d.get("drumParentTurn").is_null() { None } else { Some(q4(d.get("drumParentTurn"))) },
            gun_bind: if d.get("gunBind").is_null() { None } else { Some(q4(d.get("gunBind"))) },
            rider: d.get("rider").bool(),
            // (Captures before c3 lack these two: the crews' replay reads neither.)
            shootable: d.get("shootable").opt_bool().unwrap_or(false),
            matrix: {
                let mut m = [0.0; 16];
                for (i, x) in d.get("matrix").items().enumerate() {
                    m[i] = x.num();
                }
                m
            },
        },
        rig,
    })
}

/// The crews as a frame starts: read into `cx.store`; the turrets.
fn read_crews(cx: &mut Replay, s: V) -> Result<Crews, String> {
    let mut crews = Crews { collide: s.get("collide").bool(), ..Crews::default() };
    load_crews(cx, s, &mut crews)?;
    Ok(crews)
}

/// The crews in a frame's state shape, read into `cx.store` and over
/// `crews` (the leases they made in the frame kept).
fn load_crews(cx: &mut Replay, s: V, crews: &mut Crews) -> Result<(), String> {
    cx.store.turret_seats = TurretSeats::from_value(s.get("turretSeats"), cx, "turretSeats")?;
    cx.store.seats = Seats::from_value(s.get("seats"), cx, "seats")?;
    for t in s.get("turrets").items() {
        let mine = turret_of(cx, t)?;
        match crews.turrets.iter_mut().find(|x| x.index == mine.index) {
            Some(x) => *x = mine,
            None => crews.turrets.push(mine),
        }
    }
    crews.turrets.sort_by_key(|t| t.index);
    for (i, u) in s.get("units").items().enumerate() {
        let r = Unit::value_in(u.get("unit"), cx, &format!("units[{i}]"))?;
        let c = u.get("combat");
        crews.riders.insert(
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
        crews.controls.insert(control_key(unit), SeatControl { driven: c.get("driven").bool(), intent: intent_of(c.get("intent")) });
    }
    Ok(())
}

fn mat_value(a: &mut Arena, m: &three::Matrix4) -> Value {
    let items = m.elements.iter().map(|&x| Value::Num(x)).collect();
    a.array(items)
}

/// The crews as the port ends the frame, in the capture's shape: each
/// turret's entry value with what the crews change made again (its lease
/// and gunner, aim, gun's angles, barrel, its bones' turns and world
/// matrices).
fn write_crews(cx: &mut Replay, crews: &mut Crews, before: V, after: V, named: &[Ref<SeatLease>]) -> (Arena, Value) {
    let mut a = Arena::default();
    let turret_seats = cx.store.turret_seats.clone();
    let ts = turret_seats.to_value(cx, &mut a);
    let mut leases = cx.store.turret_seats.leases.clone();
    for &r in named {
        if !leases.contains(&r) {
            leases.push(r);
        }
    }
    cx.store.seats.leases = leases.clone();
    let seats = cx.store.seats.clone().to_value(cx, &mut a);
    let mut turrets = Vec::new();
    for t in after.get("turrets").items() {
        let index = t.get("index").num() as usize;
        let Some(mine) = crews.turrets.iter_mut().find(|x| x.index == index) else {
            turrets.push(Value::Undefined);
            continue;
        };
        let Some(entry) = before.get("turrets").items().find(|x| x.get("index").num() as usize == index) else {
            turrets.push(Value::Undefined);
            continue;
        };
        let v = a.copy(entry);
        let lease = match mine.lease {
            Some(r) => SeatLease::ref_out(r, cx, &mut a),
            None => Value::Null,
        };
        a.assign(&v, "lease", lease);
        let gunner = match mine.gunner {
            Some(id) => Value::handle("unit", Value::Num(id as f64)),
            None => Value::Null,
        };
        a.assign(&v, "gunner", gunner);
        for (key, x) in [("yaw", mine.yaw), ("pitch", mine.pitch), ("yawVel", mine.yaw_vel), ("pitchVel", mine.pitch_vel), ("gunYaw", mine.gun_yaw), ("gunPitch", mine.gun_pitch), ("barrel", mine.barrel)] {
            a.assign(&v, key, Value::Num(x));
        }
        let mut bones = Vec::new();
        let mut world = Vec::new();
        for (i, b) in entry.get("rig").get("bones").items().enumerate() {
            let id = mine.rig.bones[i];
            let n = mine.rig.tree.node(id);
            let bv = a.copy(b);
            let p = a.vec3([n.position.x, n.position.y, n.position.z]);
            a.assign(&bv, "position", p);
            let q = a.quat([n.quaternion.x, n.quaternion.y, n.quaternion.z, n.quaternion.w]);
            a.assign(&bv, "quaternion", q);
            let sc = a.vec3([n.scale.x, n.scale.y, n.scale.z]);
            a.assign(&bv, "scale", sc);
            bones.push(bv);
            let m = n.matrix_world;
            world.push(mat_value(&mut a, &m));
        }
        let rig = a.copy(entry.get("rig"));
        let bones = a.array(bones);
        a.assign(&rig, "bones", bones);
        let world = a.array(world);
        a.assign(&rig, "world", world);
        a.assign(&v, "rig", rig);
        turrets.push(v);
    }
    let turrets = a.array(turrets);
    let mut units = Vec::new();
    for u in after.get("units").items() {
        let id = u.get("unit").get("$id").handle_num("unit").unwrap() as u32;
        let r = Ref::new(id, 0);
        if cx.store.unit.get(r).is_none() {
            units.push(Value::Undefined);
            continue;
        }
        let unit = Unit::value_out(r, cx, &mut a);
        let f = crews.riders.get(&id).copied().unwrap_or_default();
        let combat = a.object(
            "",
            vec![
                ("zoomStep", Value::Num(f.combat.zoom_step)),
                ("zoom", Value::Num(f.combat.zoom)),
                ("firstPerson", Value::Bool(f.combat.first_person)),
                ("blowApart", Value::Bool(f.combat.blow_apart)),
            ],
        );
        units.push(a.object("", vec![("unit", unit), ("combat", combat), ("noEnter", Value::Num(f.no_enter))]));
    }
    let units = a.array(units);
    let mut controls = Vec::new();
    for &r in &leases {
        let Some(c) = cx.store.seat_lease.get(r).and_then(|l| l.controllable) else { continue };
        let lease = SeatLease::ref_out(r, cx, &mut a);
        let control = crews.controls.get(&control_key(c)).cloned().unwrap_or_default();
        let intent = intent_value(&mut a, &control.intent);
        controls.push(a.object("", vec![("lease", lease), ("driven", Value::Bool(control.driven)), ("intent", intent)]));
    }
    let controls = a.array(controls);
    let v = a.object(
        "",
        vec![("turretSeats", ts), ("collide", Value::Bool(crews.collide)), ("seats", seats), ("turrets", turrets), ("units", units), ("controls", controls)],
    );
    (a, v)
}

fn turret_place(crews: &Crews, cx: &mut Replay, v: V) -> Option<usize> {
    let r = cx.ref_in::<bfe_sim::gen::Turret>(KIND_TURRET, v, "turret").ok()?;
    crews.turrets.iter().position(|t| t.turret == r)
}

/// One crew frame: its state read, its method run, its end compared.
fn replay_crew_frame(frame: &Frame, name: &str, tally: &mut BTreeMap<String, usize>) -> Outcome {
    let mut cx = Replay::default();
    let before = frame.before();
    let mut crews = match read_crews(&mut cx, before) {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("{}: reading the state: {e}", frame.place())),
    };
    let mut facts = BTreeMap::new();
    for t in before.get("turrets").items() {
        let w = t.get("weapon");
        facts.insert(t.get("index").num() as usize, TurretFacts { handle: t.get("turret"), target: t.get("aiTarget"), body: t.get("desc").get("body"), ordnance: if w.is_null() { w } else { w.get("ordnance") } });
    }
    let mut named: Vec<Ref<SeatLease>> = Vec::new();
    let mut st = std::mem::take(&mut cx.store);
    let mut served = Served { log: Log::new(frame), cx: &mut cx, error: None, prefix: "crews", turrets: facts, shots: Vec::new(), rounds: Vec::new(), lease_controls: lease_controls(&st), relation_label: "seats.relation" };
    let mut result: Option<bool> = None;
    match name {
        "turretSeats.update" => {
            let n = st.turret_seats.leases.len();
            crews.update(&mut st, &mut served, frame.arg(0).num());
            *tally.entry(format!("update with {} crews", n.min(3))).or_default() += 1;
        }
        "turretSeats.man" => {
            let Some(unit) = frame.arg(0).handle_num("unit") else { return Outcome::Failed(format!("{}: man's unit {}", frame.place(), frame.arg(0).show())) };
            let Some(t) = turret_place(&crews, served.cx, frame.arg(1)) else { return Outcome::Failed(format!("{}: man's turret {}", frame.place(), frame.arg(1).show())) };
            result = Some(crews.man(&mut st, &mut served, unit as u32, t));
            *tally.entry(format!("man {}", if result == Some(true) { "taken" } else { "refused" })).or_default() += 1;
        }
        "turretSeats.vacate" => {
            let r = match SeatLease::ref_in(frame.arg(0), served.cx, "vacate lease") {
                Ok(r) => r,
                Err(e) => return Outcome::Failed(format!("{}: {e}", frame.place())),
            };
            named.push(r);
            let o = frame.arg(1);
            let mut how = Let::default();
            if !o.is_nullish() {
                if !o.get("placement").is_undefined() {
                    how.placement = o.get("placement").truthy();
                }
                if !o.get("post").is_nullish() {
                    how.post = served.cx.ref_in::<Post>(KIND_POST, o.get("post"), "post").ok();
                }
                how.kill = o.get("kill").truthy();
                if !o.get("attacker").is_undefined() {
                    how.attacker = attacker_of(o.get("attacker"), served.cx);
                }
                how.reason = match o.get("reason").str() {
                    None | Some("exit") => Reason::Exit,
                    Some("killed") => Reason::Killed,
                    Some(r) => return Outcome::Failed(format!("{}: a reason {r}", frame.place())),
                };
            }
            crews.vacate(&mut st, &mut served, r, how);
            *tally.entry(format!("vacate {}{}", how.reason.name(), if how.kill { " kill" } else { "" })).or_default() += 1;
        }
        "turretSeats.destroyed" => {
            let Some(t) = turret_place(&crews, served.cx, frame.arg(0)) else { return Outcome::Failed(format!("{}: destroyed's turret {}", frame.place(), frame.arg(0).show())) };
            crews.destroyed(&mut st, &mut served, t);
        }
        "turretSeats.requestExit" | "turretSeats.leave" => {
            let Some(unit) = frame.arg(0).handle_num("unit") else { return Outcome::Failed(format!("{}: unit {}", frame.place(), frame.arg(0).show())) };
            let o = frame.arg(1);
            let post = if o.is_nullish() || o.get("post").is_nullish() { None } else { served.cx.ref_in::<Post>(KIND_POST, o.get("post"), "post").ok() };
            let placement = if o.is_nullish() || o.get("placement").is_undefined() { true } else { o.get("placement").truthy() };
            result = Some(if name == "turretSeats.leave" { crews.leave(&mut st, &mut served, unit as u32, placement, post) } else { crews.request_exit(&mut st, unit as u32, post, placement) });
        }
        _ => return Outcome::Skipped(format!("{name}: not a crew frame")),
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
    cx.store = st;
    let after = frame.after();
    // A lease the port made that no call named takes the handle the capture
    // gives the lease at its place among the crews' leases.
    for r in crews.made.clone() {
        if let Some(at) = cx.store.turret_seats.leases.iter().position(|x| *x == r) {
            let v = after.get("seats").get("leases").at(at);
            if !v.is_undefined() {
                let _ = cx.stored_in(KIND_SEAT_LEASE, v.get("$id"), "made", |_, _| (r.slot, r.gen));
            }
        }
    }
    let (a, mine) = write_crews(&mut cx, &mut crews, before, after, &named);
    match diff(after, a.view(&mine)) {
        None => Outcome::Same,
        Some(d) => Outcome::Failed(format!("{}: crews {d} (capture | port)", frame.place())),
    }
}

#[test]
fn turret_crews_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    for name in ["turretSeats.update", "turretSeats.man", "turretSeats.vacate", "turretSeats.destroyed", "turretSeats.requestExit", "turretSeats.leave"] {
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        let report = replay_corpus(&corpus, name, |_capture, frame| replay_crew_frame(frame, name, &mut tally)).unwrap();
        eprintln!("{}", report.summary());
        for (what, n) in &tally {
            eprintln!("  {what}: {n}");
        }
        report.assert_clean();
    }
}

#[test]
fn turret_crews_reject_state_and_kill_entry_mutations() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let (mut state, mut entry) = (false, false);
    for capture in corpus.captures() { let capture = capture.unwrap();
        for name in ["turretSeats.update", "turretSeats.vacate", "turretSeats.destroyed"] {
            for frame in capture.frames(name) { let frame = frame.unwrap();
                assert!(matches!(replay_crew_frame(&frame, name, &mut BTreeMap::new()), Outcome::Same), "unmodified crew frame");
                if !state {
                    let mut changed = frame.clone(); let id = changed.after().get("turretSeats").node_id().unwrap();
                    if let sim_replay::Node::Object { fields, .. } = &mut changed.exit.arena.nodes[id as usize] {
                        fields.iter_mut().find(|(k,_)| k == "ticks").unwrap().1 = Value::Num(987654.0);
                    }
                    assert!(matches!(replay_crew_frame(&changed, name, &mut BTreeMap::new()), Outcome::Failed(_)), "wrong crew clock accepted"); state = true;
                }
                if !entry { for (index, event) in frame.events.iter().enumerate() {
                    let sim_replay::Event::Call(call) = event else { continue };
                    if call.name != "crews.kill" { continue; }
                    let Some(id) = call.call.root().get("s").get("units").at(0).get("unit").node_id() else { continue };
                    let mut changed = frame.clone();
                    let sim_replay::Event::Call(call) = &mut changed.events[index] else { unreachable!() };
                    if let sim_replay::Node::Object { fields, .. } = &mut call.call.arena.nodes[id as usize] {
                        fields.iter_mut().find(|(k,_)| k == "tickCount").unwrap().1 = Value::Num(987654.0);
                    }
                    assert!(matches!(replay_crew_frame(&changed, name, &mut BTreeMap::new()), Outcome::Failed(_)), "wrong callback entry accepted"); entry = true; break;
                } }
                if state && entry { return; }
            }
        }
    }
    panic!("missing crew mutation coverage: state={state}, kill entry={entry}");
}

// ---- A network human's mouse and keys in a hull's seat.

fn replay_network_seat(frame: &Frame, tally: &mut BTreeMap<String, usize>) -> Outcome {
    let before = frame.before();
    let mut cx = Replay::default();
    let r = match Vehicle::value_in(before.get("hull"), &mut cx, "hull") {
        Ok(r) => r,
        Err(e) => return Outcome::Failed(format!("{}: reading the hull: {e}", frame.place())),
    };
    let mut v = cx.store.vehicle.get(r).unwrap().clone();
    let index = frame.arg(1).num() as usize;
    let k = before.get("keys");
    let keys = SeatKeys { forward: k.get("forward").num(), right: k.get("right").num(), turn: k.get("turn").num(), space: k.get("space").bool() };
    let c = frame.arg(3);
    let fs = c.get("flightSettings");
    let command = SeatCommand {
        look: c.get("look").items().map(|l| (l.at(0).num(), l.at(1).num())).collect(),
        mouse_scale: c.get("mouseScale").opt_num(),
        invert_mouse: c.get("invertMouse").truthy(),
        flight_sensitivity: if fs.is_nullish() { None } else { fs.get("sensitivity").opt_num() },
        flight_invert: !fs.is_nullish() && fs.get("invert").truthy(),
    };
    let out = network_vehicle_input(&mut v, index, &keys, &command);
    let result = frame.result();
    let same_result = match out {
        Some(SeatKeysOut::Flight) => {
            *tally.entry("flight (keys the flight package's)".into()).or_default() += 1;
            true
        }
        Some(SeatKeysOut::Drive { forward, right, turn, brake }) => {
            *tally.entry("drive".into()).or_default() += 1;
            sim_replay::same_number(result.get("forward").num(), forward)
                && sim_replay::same_number(result.get("right").num(), right)
                && sim_replay::same_number(result.get("turn").num(), turn)
                && result.get("brake").opt_bool() == Some(brake)
        }
        Some(SeatKeysOut::Turret) => {
            *tally.entry("turret".into()).or_default() += 1;
            result.get("forward").num() == 0.0 && result.get("right").num() == 0.0 && result.get("turn").num() == 0.0 && result.get("brake").opt_bool() == Some(false)
        }
        None => result.is_nullish(),
    };
    if !same_result {
        return Outcome::Failed(format!("{}: the keys: the capture has {}, the port {out:?}", frame.place(), result.show()));
    }
    *cx.store.vehicle.get_mut(r).unwrap() = v;
    let mut a = Arena::default();
    let hull = Vehicle::value_out(r, &mut cx, &mut a);
    let keys = a.copy(k);
    let after = frame.after();
    let mine = a.object("", vec![("index", Value::Num(after.get("index").num())), ("hull", hull), ("keys", keys)]);
    match diff(after, a.view(&mine)) {
        None => Outcome::Same,
        Some(d) => Outcome::Failed(format!("{}: seat {d} (capture | port)", frame.place())),
    }
}

#[test]
fn network_seats_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let report = replay_corpus(&corpus, "networkVehicle.input", |_capture, frame| replay_network_seat(frame, &mut tally)).unwrap();
    eprintln!("{}", report.summary());
    for (what, n) in &tally {
        eprintln!("  {what}: {n}");
    }
    report.assert_clean();
}

// ---- What a soldier's use key gets him into.

// The reach query's casts are the hulls' collision's own ('collision.cast').
struct ReachCasts<'s, 'f, 'c>(&'s mut Served<'f, 'c>);

impl SeatWorld for ReachCasts<'_, '_, '_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<Ref<Body>>) -> Option<Hit> {
        let s = &mut *self.0;
        let call = s.call("collision.cast")?;
        s.check(check_vec3("reach cast origin", call.arg(0), arr(origin)));
        s.check(check_vec3("reach cast direction", call.arg(1), arr(direction)));
        s.check(check_num("reach cast far", call.arg(2), far));
        s.check_body("reach cast ignoreBody", call.arg(4), ignore);
        s.hit(call.result())
    }
    fn overlaps_sphere(&mut self, c: &Vector3, r: f64, i: Option<Ref<Body>>) -> bool {
        self.0.overlaps_sphere(c, r, i)
    }
    fn raycast_units(&mut self, o: &Vector3, d: &Vector3, f: f64, e: Option<u32>) -> Option<f64> {
        self.0.raycast_units(o, d, f, e)
    }
    fn relation(&mut self, a: f64, b: f64) -> f64 {
        self.0.relation(a, b)
    }
    fn stand_up(&mut self, u: &mut Unit, x: f64, y: f64, z: f64, yaw: f64) {
        self.0.stand_up(u, x, y, z, yaw)
    }
    fn kill(&mut self, st: &mut Store, seating: Seating<'_>, unit: u32, attacker: Attacker) {
        self.0.kill(st, seating, unit, attacker)
    }
    fn rider_drop(&mut self, h: Holder, i: usize, u: u32) -> Option<Vector3> {
        self.0.rider_drop(h, i, u)
    }
    fn sync_rider(&mut self, l: Ref<SeatLease>, u: &mut Unit) {
        self.0.sync_rider(l, u)
    }
    fn controller_state(&mut self, u: Option<u32>) -> Option<f64> {
        self.0.controller_state(u)
    }
    fn controllable(&mut self, l: Ref<SeatLease>, i: usize, r: SeatRole) -> Ref<SeatUnit> {
        self.0.controllable(l, i, r)
    }
    fn boarded(&mut self, u: u32, h: Holder, i: usize) {
        self.0.boarded(u, h, i)
    }
    fn unboarded(&mut self, u: Option<u32>, h: Holder, i: usize, p: Option<Ref<Post>>, r: Reason) {
        self.0.unboarded(u, h, i, p, r)
    }
    fn moved(&mut self, u: Option<u32>, h: Holder, i: usize) {
        self.0.moved(u, h, i)
    }
}

fn replay_reach(frame: &Frame, tally: &mut BTreeMap<String, usize>) -> Outcome {
    let mut cx = Replay::default();
    let before = frame.before();
    let hulls = match read_state(&mut cx, before) {
        Ok(h) => h,
        Err(e) => return Outcome::Failed(format!("{}: reading the state: {e}", frame.place())),
    };
    let mut crews = Crews::default();
    for t in before.get("turrets").items() {
        match turret_of(&mut cx, t) {
            Ok(t) => crews.turrets.push(t),
            Err(e) => return Outcome::Failed(format!("{}: reading a turret: {e}", frame.place())),
        }
    }
    crews.turrets.sort_by_key(|t| t.index);
    let Some(unit) = frame.arg(1).handle_num("unit") else { return Outcome::Failed(format!("{}: reach's unit {}", frame.place(), frame.arg(1).show())) };
    let person = frame.arg(2).truthy();
    let st = std::mem::take(&mut cx.store);
    let mut served = Served {
        log: Log::new(frame),
        cx: &mut cx,
        error: None,
       
        prefix: "seats",
        turrets: BTreeMap::new(),
        shots: Vec::new(),
        rounds: Vec::new(),
        lease_controls: lease_controls(&st),
        relation_label: "vehicles.relation",
    };
    let mine = bfe_sim::seats::boarding::mount_in_reach(&mut ReachCasts(&mut served), &st, &hulls, &crews, unit as u32, person);
    if let Some(e) = served.error.take() {
        return Outcome::Failed(e);
    }
    if let Err(e) = served.log.finish() {
        return Outcome::Failed(e.0);
    }
    let r = frame.result();
    let theirs = if r.is_nullish() {
        None
    } else if let Some(i) = r.get("vehicle").handle_num("vehicle") {
        Some(Mount::Hull(i as usize))
    } else if !r.get("turret").is_nullish() {
        crews.turrets.iter().find(|t| cx.ref_in::<bfe_sim::gen::Turret>(KIND_TURRET, r.get("turret"), "turret").ok() == Some(t.turret)).map(|t| Mount::Turret(t.index))
    } else {
        return Outcome::Failed(format!("{}: reach answered {}", frame.place(), r.show()));
    };
    let what = match mine {
        Some(Mount::Hull(_)) => "a hull",
        Some(Mount::Turret(_)) => "a turret",
        None => "nothing",
    };
    *tally.entry(format!("reaches {what}")).or_default() += 1;
    if theirs != mine {
        return Outcome::Failed(format!("{}: reach: the capture has {}, the port {mine:?}", frame.place(), r.show()));
    }
    Outcome::Same
}

#[test]
fn reach_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let report = replay_corpus(&corpus, "boardingReach.mount", |_capture, frame| replay_reach(frame, &mut tally)).unwrap();
    eprintln!("{}", report.summary());
    for (what, n) in &tally {
        eprintln!("  {what}: {n}");
    }
    report.assert_clean();
}

#[test]
fn hull_seats_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    for name in ["vehicles.latchCrews", "vehicles.board", "vehicles.vacate", "vehicles.destroyedCrew", "vehicles.requestExit", "networkSeat.cycle"] {
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
