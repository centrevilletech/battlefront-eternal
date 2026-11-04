//! Live turret crews. Each command has its own store and turret sides;
//! publication copies only its rows into the shared store before an
//! import, and callback effects are read back after it returns.
use crate::abi::sim;
use crate::gen::*;
use crate::seats::{self, Attacker as By, Hit, Holder, Reason, SeatWorld, Seating};
use crate::seats::bodies::CombatFlags;
use crate::seats::hulls::RiderFacts;
use crate::seats::lease::{control_key, SeatControl, SeatIntent};
use crate::seats::turrets::{CrewWorld, Crews, Let, Shot};
use crate::store::Ref;
use three::Vector3;

fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
fn a3(v: &Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
fn reason(v: SeatReason) -> Reason { match v { SeatReason::Exit => Reason::Exit, SeatReason::Stale => Reason::Stale, SeatReason::Killed => Reason::Killed, SeatReason::Destroyed => Reason::Destroyed, SeatReason::Displaced => Reason::Displaced } }
fn why(v: Reason) -> SeatReason { match v { Reason::Exit => SeatReason::Exit, Reason::Stale => SeatReason::Stale, Reason::Killed => SeatReason::Killed, Reason::Destroyed => SeatReason::Destroyed, Reason::Displaced => SeatReason::Displaced } }
fn attacker(v: By) -> Attacker { match v {
    By::Nobody => Attacker::Null, By::Player => Attacker::Player, By::Unit(n) => Attacker::Unit(seats::unit_ref(n)), By::Record(v) => v,
    By::Hull(_) => unreachable!("live crews carry the combat schema's attacker references"),
} }
fn turret(holder: Holder) -> f64 { match holder { Holder::Turret(n) => n as f64, Holder::Hull(_) => unreachable!("turret crew holder") } }
fn intent(i: SeatAiIntent) -> SeatIntent {
    SeatIntent { forward: i.forward, right: i.right, turn: i.turn, pitch: i.pitch, jump: i.jump, crouch: i.crouch, prone: i.prone, r#use: i.r#use,
        exit: i.exit, buttons: i.buttons, reload: i.reload, next_primary: i.next_primary, previous_primary: i.previous_primary,
        next_secondary: i.next_secondary, zoom: i.zoom, aim: i.aim.map(v3), brake: i.brake, up: i.up, down: i.down, trick: i.trick, immelmann: i.immelmann, boost: i.boost }
}
fn intent_record(i: &SeatIntent) -> SeatAiIntent {
    SeatAiIntent { forward: i.forward, right: i.right, turn: i.turn, pitch: i.pitch, jump: i.jump, crouch: i.crouch, prone: i.prone, r#use: i.r#use,
        exit: i.exit, buttons: i.buttons, reload: i.reload, next_primary: i.next_primary, previous_primary: i.previous_primary,
        next_secondary: i.next_secondary, zoom: i.zoom, aim: i.aim.as_ref().map(a3), brake: i.brake, up: i.up, down: i.down, trick: i.trick, immelmann: i.immelmann, boost: i.boost }
}
fn read(value: PcState) -> (Store, Crews) {
    let mut st = Store::default();
    { let global = &sim().store;
      for r in value.units { *st.unit.ensure(r) = global.unit.get(r).expect("crew unit row").clone(); }
      for r in value.rows { *st.seat_lease.ensure(r) = global.seat_lease.get(r).expect("crew lease row").clone(); }
    }
    st.seats.leases = st.seat_lease.iter().map(|(r,_)| r).collect();
    st.turret_seats = TurretSeats { time: value.time, ticks: value.ticks, leases: value.leases };
    for r in &value.riders {
        let combat = &mut st.unit.get_mut(r.unit).expect("crew combat flags").combat;
        combat.dead = r.dead; combat.mounted = r.mounted;
        combat.gun_underwater = r.gun_underwater; combat.aim_override = r.aim_override.clone();
    }
    let crews = Crews { collide: value.collide,
        turrets: value.turrets.into_iter().map(|v| {
            let settings = super::turrets_module::settings(v.state.index as usize);
            let mut t = super::turrets::side(v.state, &settings);
            t.turret = v.turret; t.alive = v.alive; t.lease = v.lease; t.gunner = v.gunner.map(|u| u.slot);
            t.last_attacker = By::Record(v.attacker); t.sphere = (v3(v.center), v.radius); t.weapon = v.weapon; t.missile_lock = v.missile_lock;
            t.desc.body = v.body; t.desc.rider = v.rider; t.desc.shootable = v.shootable; t
        }).collect(),
        riders: value.riders.into_iter().map(|r| (r.unit.slot, RiderFacts { no_enter: r.no_enter,
            combat: CombatFlags { zoom_step: r.zoom_step, zoom: r.zoom, first_person: r.first_person, blow_apart: r.blow_apart } })).collect(),
        controls: value.controls.into_iter().map(|v| (control_key(v.seat_unit), SeatControl { driven: v.driven, intent: intent(v.intent) })).collect(),
        ..Crews::default() };
    (st, crews)
}
fn record(st: &Store, crews: &Crews, latched: Vec<Ref<SeatUnit>>) -> PcState {
    PcState { time: st.turret_seats.time, ticks: st.turret_seats.ticks, leases: st.turret_seats.leases.clone(),
        rows: st.seat_lease.iter().map(|(r,_)| r).collect(), units: st.unit.iter().map(|(r,_)| r).collect(),
        turrets: crews.turrets.iter().map(|t| PcTurret { state: super::turrets::state(t), turret: t.turret, alive: t.alive, lease: t.lease,
            gunner: t.gunner.map(seats::unit_ref), attacker: attacker(t.last_attacker), center: a3(&t.sphere.0), radius: t.sphere.1, weapon: t.weapon,
            missile_lock: t.missile_lock, body: t.desc.body, rider: t.desc.rider, shootable: t.desc.shootable }).collect(),
        riders: crews.riders.iter().map(|(&unit, r)| {
            let combat = &st.unit.get(seats::unit_ref(unit)).expect("crew combat flags").combat;
            PcRider { unit: seats::unit_ref(unit), no_enter: r.no_enter,
                zoom_step: r.combat.zoom_step, zoom: r.combat.zoom, first_person: r.combat.first_person, blow_apart: r.combat.blow_apart,
                dead: combat.dead, mounted: combat.mounted, gun_underwater: combat.gun_underwater, aim_override: combat.aim_override.clone() }
        }).collect(),
        controls: crews.controls.iter().map(|(&(slot, gen), c)| SeatAiControl { seat_unit: Ref::new(slot, gen), driven: c.driven, intent: intent_record(&c.intent) }).collect(),
        collide: crews.collide, latched }
}
fn write(st: &Store) {
    let global = &mut sim().store;
    for (r, u) in st.unit.iter() { *global.unit.ensure(r) = u.clone(); }
    for (r, lease) in st.seat_lease.iter() {
        *global.seat_lease.ensure(r) = lease.clone();
        if !global.seats.leases.contains(&r) { global.seats.leases.push(r); }
    }
    global.turret_seats = st.turret_seats.clone();
}
#[derive(Default)]
struct JsWorld { latched: usize }
impl JsWorld {
    fn args(&self, kind: PcActionKind) -> PcAction { PcAction { kind, ..PcAction::default() } }
    fn unit(&mut self, kind: PcActionKind, unit: u32, yaw: f64, point: [f64;3]) {
        import_pc_action(&PcAction { unit: Some(seats::unit_ref(unit)), yaw, point, ..self.args(kind) });
    }
}
impl SeatWorld for JsWorld {
    fn publish_crews(&mut self, st: &Store, crews: &Crews) {
        write(st); let pending = crews.latched[self.latched..].to_vec(); self.latched = crews.latched.len();
        import_pc_action(&PcAction { state: Some(record(st, crews, pending)), ..self.args(PcActionKind::Publish) });
    }
    fn resume_crews(&mut self, st: &mut Store, crews: &mut Crews) {
        let state = import_pc_action(&self.args(PcActionKind::Resume)).state.expect("crew callback state");
        let (next, mut next_crews) = read(state); next_crews.latched = std::mem::take(&mut crews.latched);
        next_crews.made = std::mem::take(&mut crews.made); *st = next; *crews = next_crews;
    }
    fn crew_lease(&mut self, st: &mut Store, lease: SeatLease) -> Ref<SeatLease> {
        let made = PcNewLease { unit: lease.unit, generation: lease.generation, human: lease.human, team: lease.team, vehicle: lease.vehicle,
            turret: lease.turret, index: lease.index, role: lease.role, since: lease.since, tick: lease.tick };
        let r = import_pc_action(&PcAction { made: Some(made), ..self.args(PcActionKind::MakeLease) }).lease.expect("new crew lease");
        *st.seat_lease.ensure(r) = lease; r
    }
    fn crew_posed(&mut self, lease: Ref<SeatLease>) { import_pc_action(&PcAction { lease: Some(lease), ..self.args(PcActionKind::Pose) }); }
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<Ref<Body>>) -> Option<Hit> {
        let h = import_pc_action(&PcAction { point: a3(origin), direction: a3(direction), value: far, body: ignore, ..self.args(PcActionKind::Cast) }).hit?;
        h.found.then(|| Hit { distance: h.distance, point: v3(h.point), normal: v3(h.normal), body: h.body })
    }
    fn overlaps_sphere(&mut self, center: &Vector3, radius: f64, ignore: Option<Ref<Body>>) -> bool {
        import_pc_action(&PcAction { point: a3(center), value: radius, body: ignore, ..self.args(PcActionKind::Overlaps) }).flag
    }
    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64, exclude: Option<u32>) -> Option<f64> {
        import_pc_action(&PcAction { point: a3(origin), direction: a3(direction), value: far, unit: exclude.map(seats::unit_ref), ..self.args(PcActionKind::Raycast) }).value
    }
    fn relation(&mut self, _: f64, _: f64) -> f64 { unreachable!("turret crews do not query alliances") }
    fn stand_up(&mut self, unit: &mut Unit, x: f64, y: f64, z: f64, yaw: f64) {
        let UnitBody::Soldier(s) = &mut unit.soldier else { unreachable!("a turret gunner has a soldier") };
        s.spawn(x, y, z, yaw, &mut Ground(self));
    }
    fn kill(&mut self, _: &mut Store, _: Seating<'_>, unit: u32, by: By) {
        import_pc_action(&PcAction { unit: Some(seats::unit_ref(unit)), attacker: attacker(by), ..self.args(PcActionKind::Kill) });
    }
    fn rider_drop(&mut self, holder: Holder, index: usize, unit: u32) -> Option<Vector3> {
        let r = import_pc_action(&PcAction { turret: turret(holder), index: index as f64, unit: Some(seats::unit_ref(unit)), ..self.args(PcActionKind::RiderDrop) });
        r.flag.then(|| v3(r.point))
    }
    fn sync_rider(&mut self, _: Ref<SeatLease>, _: &mut Unit) { unreachable!("turret crew poses its own body") }
    fn controller_state(&mut self, unit: Option<u32>) -> Option<f64> { import_pc_action(&PcAction { unit: unit.map(seats::unit_ref), ..self.args(PcActionKind::ControllerState) }).value }
    fn controllable(&mut self, lease: Ref<SeatLease>, _: usize, _: SeatRole) -> Ref<SeatUnit> { import_pc_action(&PcAction { lease: Some(lease), ..self.args(PcActionKind::Controllable) }).control.expect("seat control") }
    fn boarded(&mut self, unit: u32, holder: Holder, index: usize) { import_pc_action(&PcAction { unit: Some(seats::unit_ref(unit)), turret: turret(holder), index: index as f64, ..self.args(PcActionKind::Boarded) }); }
    fn unboarded(&mut self, unit: Option<u32>, holder: Holder, index: usize, post: Option<Ref<Post>>, reason: Reason) {
        import_pc_action(&PcAction { unit: unit.map(seats::unit_ref), turret: turret(holder), index: index as f64, post, reason: why(reason), ..self.args(PcActionKind::Unboarded) });
    }
    fn moved(&mut self, _: Option<u32>, _: Holder, _: usize) { unreachable!("a turret has one seat") }
    fn seated(&mut self, unit: u32) { self.unit(PcActionKind::Seated, unit, 0.0, [0.0;3]); }
    fn unseated(&mut self, unit: u32, yaw: f64) { self.unit(PcActionKind::Unseated, unit, yaw, [0.0;3]); }
    fn placed(&mut self, unit: u32, yaw: f64) { self.unit(PcActionKind::Placed, unit, yaw, [0.0;3]); }
    fn dropped(&mut self, unit: u32, at: &Vector3) { self.unit(PcActionKind::Dropped, unit, 0.0, a3(at)); }
    fn seat_policy(&mut self, holder: Holder, index: usize, player: bool, random: bool) {
        import_pc_action(&PcAction { turret: turret(holder), index: index as f64, flag: player, second: random, ..self.args(PcActionKind::SeatPolicy) });
    }
}
impl CrewWorld for JsWorld {
    fn update_lock(&mut self, turret: usize, dt: f64, origin: &Vector3, direction: &Vector3, lease: Ref<SeatLease>) {
        import_pc_action(&PcAction { turret: turret as f64, value: dt, point: a3(origin), direction: a3(direction), lease: Some(lease), ..self.args(PcActionKind::Lock) });
    }
    fn weapon_update(&mut self, turret: usize, dt: f64, held: bool, pressed: bool, reload: bool) -> Vec<Shot> {
        import_pc_action(&PcAction { turret: turret as f64, value: dt, flag: held, second: pressed, third: reload, ..self.args(PcActionKind::Weapon) }).shots.into_iter().map(|s| Shot { yaw: s.yaw, pitch: s.pitch }).collect()
    }
    fn fire(&mut self, turret: usize, origin: &Vector3, direction: &Vector3, owner: Option<u32>, lease: Ref<SeatLease>, shot: usize) -> usize {
        import_pc_action(&PcAction { turret: turret as f64, point: a3(origin), direction: a3(direction), unit: owner.map(seats::unit_ref), lease: Some(lease), shot: shot as f64, ..self.args(PcActionKind::Fire) }).value.expect("round index") as usize
    }
    fn bind_round(&mut self, round: usize, turret: usize, barrel: f64) { import_pc_action(&PcAction { index: round as f64, turret: turret as f64, value: barrel, ..self.args(PcActionKind::Bind) }); }
    fn sound(&mut self, turret: usize, shot: usize) { import_pc_action(&PcAction { turret: turret as f64, shot: shot as f64, ..self.args(PcActionKind::Sound) }); }
    fn fired(&mut self, turret: usize, shot: usize, owner: Option<u32>, muzzle: &Vector3, direction: &Vector3) {
        import_pc_action(&PcAction { turret: turret as f64, shot: shot as f64, unit: owner.map(seats::unit_ref), point: a3(muzzle), direction: a3(direction), ..self.args(PcActionKind::Fired) });
    }
}
#[no_mangle]
pub extern "C" fn crew_words(which: u32) -> u32 { (match which { 0 => PcInput::WORDS, 1 => PcResult::WORDS, _ => 0 }) as u32 }
#[no_mangle]
pub extern "C" fn crew_run() -> *const f64 {
    let (row, heap) = sim().calls.args(); let input = PcInput::take(row, heap);
    let (mut st, mut crews) = read(input.state); let mut world = JsWorld::default(); let mut flag = false; let mut lease = None;
    let unit = input.unit.map(|u| u.slot); let turret = (input.turret >= 0.0).then_some(input.turret as usize);
    match input.op {
        PcOperation::SeatOf => lease = unit.and_then(|u| crews.seat_of(&st, u)),
        PcOperation::CanMan => flag = unit.zip(turret).is_some_and(|(u,t)| crews.can_man(&st, u, t)),
        PcOperation::Man => if let Some((u,t)) = unit.zip(turret) { flag = crews.man(&mut st, &mut world, u, t); },
        PcOperation::RequestExit => if let Some(u) = unit { flag = crews.request_exit(&mut st, u, input.post, input.placement); },
        PcOperation::Leave => if let Some(u) = unit { flag = crews.leave(&mut st, &mut world, u, input.placement, input.post); },
        PcOperation::Vacate => crews.vacate(&mut st, &mut world, input.lease.expect("vacated lease"), Let { placement: input.placement, post: input.post, kill: input.kill, attacker: By::Record(input.attacker), reason: reason(input.reason) }),
        PcOperation::Destroyed => if let Some(t) = turret { crews.destroyed(&mut st, &mut world, t); },
        PcOperation::Update => crews.update(&mut st, &mut world, input.dt),
        PcOperation::Drive => crews.drive(&mut st, &mut world, input.lease.expect("driven lease"), input.dt),
        PcOperation::Pose => { let lease = input.lease.expect("posed lease"); crews.pose(&mut st, lease); world.publish_crews(&st, &crews); world.crew_posed(lease); world.resume_crews(&mut st, &mut crews); },
    }
    write(&st); let state = record(&st, &crews, crews.latched[world.latched..].to_vec());
    let result = PcResult { state, flag, lease, dt: input.dt };
    let sim = sim(); sim.calls.result(PcResult::WORDS, |row, heap| result.put(row, heap)); sim.calls.block.ptr()
}

// Soldier.spawn uses only its ground cast when a crew places a rider.
struct Ground<'a>(&'a mut JsWorld);
impl crate::units::world::UnitsWorld for Ground<'_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<crate::units::world::CastHit> {
        let h = import_pc_action(&PcAction { point: a3(origin), direction: a3(direction), value: far, flag: true, ..self.0.args(PcActionKind::Cast) }).hit?;
        h.found.then(|| crate::units::world::CastHit { point: v3(h.point), normal: v3(h.normal), distance: h.distance, terrain: h.terrain })
    }
    fn gather(&mut self, _: u32, _: &Vector3, _: f64, _: usize, _: &mut Vec<crate::units::world::GatherPolygon>) { unreachable!("spawn ground") }
    fn seat_exposed(&mut self, _: u32, _: &Unit) -> bool { unreachable!("spawn ground") }
    fn rider_shapes(&mut self, _: u32, _: &Unit, _: &mut [crate::units::shapes::HitShape]) -> usize { unreachable!("spawn ground") }
    fn held_shapes(&mut self, _: u32, _: &Unit, _: &mut [crate::units::shapes::HitShape]) -> usize { unreachable!("spawn ground") }
    fn target_alive(&mut self, _: u32) -> bool { unreachable!("spawn ground") }
    fn target_dead(&mut self, _: u32) -> bool { unreachable!("spawn ground") }
    fn target_sphere(&mut self, _: u32) -> crate::units::shapes::Bounds { unreachable!("spawn ground") }
    fn move_soldier(&mut self, _: u32, _: &mut Unit, _: f64) { unreachable!("spawn ground") }
    fn damage(&mut self, _: u32, _: &mut Unit, _: f64) { unreachable!("spawn ground") }
    fn aim(&mut self, _: u32, _: &mut Unit, _: f64) { unreachable!("spawn ground") }
    fn has_remote_step(&mut self, _: u32, _: &Unit) -> bool { unreachable!("spawn ground") }
    fn remote_step(&mut self, _: u32, _: &mut Unit, _: f64, _: bool) { unreachable!("spawn ground") }
    fn combat_reset(&mut self, _: u32, _: &mut Unit) { unreachable!("spawn ground") }
    fn combat_update(&mut self, _: u32, _: &mut Unit, _: f64, _: f64) { unreachable!("spawn ground") }
    fn flush_muzzles(&mut self, _: u32, _: &mut Unit) { unreachable!("spawn ground") }
    fn clear_charge(&mut self, _: u32, _: &mut Unit) { unreachable!("spawn ground") }
    fn death_blast(&mut self, _: u32, _: &mut Unit) { unreachable!("spawn ground") }
    fn blow_apart(&mut self, _: u32, _: &mut Unit, _: f64) { unreachable!("spawn ground") }
    fn let_go_of_character(&mut self, _: u32, _: &mut Unit) { unreachable!("spawn ground") }
    fn release(&mut self, _: u32, _: &Unit) { unreachable!("spawn ground") }
}
