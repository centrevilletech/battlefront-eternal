//! Live building commands, with state held outside the instance while a
//! callback may reenter it. Geometry is immutable after configuration.
use super::props::{Building, Identity, Kind, Model, Settings, State, Station, Supply, Target, Weapon, World};
use crate::abi::sim;
use crate::gen::*;
use std::ptr::addr_of_mut;
use three::{Matrix4, Vector3};

static mut CONFIG: Vec<Settings> = Vec::new();
fn config() -> &'static mut Vec<Settings> { unsafe { &mut *addr_of_mut!(CONFIG) } }
fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
fn a3(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
impl From<&PbIdentity> for Identity {
    fn from(v: &PbIdentity) -> Self { match v.kind as u32 { 0 => Self::Undefined, 1 => Self::Null, 2 => Self::Object(v.id as usize), _ => panic!("prop identity") } }
}
impl From<Identity> for PbIdentity {
    fn from(v: Identity) -> Self { match v { Identity::Undefined => Self { kind: 0.0, id: 0.0 }, Identity::Null => Self { kind: 1.0, id: 0.0 }, Identity::Object(id) => Self { kind: 2.0, id: id as f64 } } }
}
impl From<PbState> for Building {
    fn from(v: PbState) -> Self { Self { state: match v.state { PbStateName::Unbuilt => State::Unbuilt, PbStateName::Built => State::Built, PbStateName::Destroyed => State::Destroyed },
        health: v.health, add_health: v.add_health, last_attacker: (&v.last_attacker).into(), target_generation: v.target_generation } }
}
impl From<&Building> for PbState {
    fn from(v: &Building) -> Self { Self { state: match v.state { State::Unbuilt => PbStateName::Unbuilt, State::Built => PbStateName::Built, State::Destroyed => PbStateName::Destroyed },
        health: v.health, add_health: v.add_health, last_attacker: v.last_attacker.into(), target_generation: v.target_generation } }
}
impl From<PbStation> for Station {
    fn from(v: PbStation) -> Self { Self { building: v.building.into(), active_mode: v.mode == PbMode::Active, active: v.active, pulse: v.pulse,
        served_player: v.served_player, served_health: v.served_health, served_ammo: v.served_ammo } }
}
impl From<&Station> for PbStation {
    fn from(v: &Station) -> Self { Self { building: (&v.building).into(), mode: if v.active_mode { PbMode::Active } else { PbMode::Waiting }, active: v.active, pulse: v.pulse,
        served_player: v.served_player, served_health: v.served_health, served_ammo: v.served_ammo } }
}
impl From<PbTarget> for Target {
    fn from(v: PbTarget) -> Self { Self { id: v.id as usize, position: v3(v.position), vehicle: v.vehicle, alive: v.alive, player: v.player, radius: v.radius,
        health: v.health, max_health: v.max_health, weapons: v.weapons.into_iter().map(|w| Weapon { bottomless: w.bottomless, ammo: w.ammo, clip: w.clip, per_shot: w.per_shot, reserve: w.reserve }).collect() } }
}
impl From<&Target> for PbTarget {
    fn from(v: &Target) -> Self { Self { id: v.id as f64, position: a3(v.position), vehicle: v.vehicle, alive: v.alive, player: v.player, radius: v.radius,
        health: v.health, max_health: v.max_health, weapons: v.weapons.iter().map(|w| PbWeapon { bottomless: w.bottomless, ammo: w.ammo, clip: w.clip, per_shot: w.per_shot, reserve: w.reserve }).collect() } }
}
impl From<PbSupply> for Supply {
    fn from(v: PbSupply) -> Self { Self { position: v3(v.position), radius: v.radius, delay: v.delay, health: v.health, ammo: v.ammo, vehicle_health: v.vehicle_health, vehicle_ammo: v.vehicle_ammo } }
}
pub(crate) fn settings(v: PbSettings) -> Settings {
    let mut matrix = Matrix4::default(); matrix.from_array(&v.matrix, 0);
    Settings { kind: match v.kind { PbKind::Building => Kind::Building, PbKind::Turret => Kind::Turret, PbKind::Objective => Kind::Objective },
        max_health: v.max_health, respawn_time: v.respawn_time, built_add_health: v.built_add_health, regenerates: v.regenerates,
        has_destroyed: v.has_destroyed, has_unbuilt: v.has_unbuilt, has_crew: false, has_explosion: v.has_explosion, has_damaged: false, matrix,
        models: v.models.into_iter().map(|v| v.map(|m| Model { sphere: m.sphere, bounds: m.low.zip(m.high).map(|(lo, hi)| [v3(lo), v3(hi)]) })).collect::<Vec<_>>().try_into().expect("three building looks") }
}
struct JsWorld { index: usize }
impl JsWorld {
    fn args(&self, kind: PbActionKind) -> PbAction { PbAction { kind, index: self.index as f64, ..PbAction::default() } }
    fn change(&mut self, kind: PbActionKind, state: &mut Building) {
        let result = import_pb_action(&PbAction { state: Some((&*state).into()), ..self.args(kind) });
        *state = result.state.expect("building callback state").into();
    }
}
impl World for JsWorld {
    fn show(&mut self, state: &Building) { import_pb_action(&PbAction { state: Some(state.into()), ..self.args(PbActionKind::Show) }); }
    fn crew_destroyed(&mut self, state: &mut Building) { self.change(PbActionKind::CrewDestroyed, state); }
    fn destroyed(&mut self, state: &mut Building) { self.change(PbActionKind::Destroyed, state); }
    fn explode(&mut self, state: &mut Building, point: Vector3) {
        let result = import_pb_action(&PbAction { state: Some((&*state).into()), point: a3(point), ..self.args(PbActionKind::Explode) });
        *state = result.state.expect("explosion callback state").into();
    }
    fn damaged(&mut self, state: &mut Building, amount: f64, attacker: Identity, hit: Identity) {
        let result = import_pb_action(&PbAction { state: Some((&*state).into()), amount, attacker: attacker.into(), hit: hit.into(), ..self.args(PbActionKind::Damaged) });
        *state = result.state.expect("damage callback state").into();
    }
    fn target_changed(&mut self, target: &Target) { import_pb_action(&PbAction { target: Some(target.into()), ..self.args(PbActionKind::TargetChanged) }); }
    fn served(&mut self, station: &Station, target: &mut Target, gave: u8) {
        let result = import_pb_action(&PbAction { station: Some(station.into()), target: Some((&*target).into()), gave: gave as f64, ..self.args(PbActionKind::Served) });
        *target = result.target.expect("served target").into();
    }
    fn serve_sound(&mut self, station: &Station, first: Option<usize>) {
        import_pb_action(&PbAction { station: Some(station.into()), first: first.map_or(-1.0, |i| i as f64), ..self.args(PbActionKind::ServeSound) });
    }
}
#[no_mangle]
pub extern "C" fn props_words(which: u32) -> u32 {
    (match which { 0 => PbConfiguration::WORDS, 1 => PbInput::WORDS, 2 => PbResult::WORDS, _ => 0 }) as u32
}
#[no_mangle]
pub extern "C" fn props_configure() {
    let (row, heap) = sim().calls.args(); let input = PbConfiguration::take(row, heap);
    *config() = input.buildings.into_iter().map(settings).collect();
}
#[no_mangle]
pub extern "C" fn props_run() -> *const f64 {
    let (row, heap) = sim().calls.args(); let input = PbInput::take(row, heap);
    let mut settings = config()[input.index as usize].clone(); settings.has_crew = input.crew; settings.has_damaged = input.damaged;
    let mut state: Building = input.state.into(); let mut station: Option<Station> = input.station.map(Into::into);
    let mut targets: Vec<Target> = input.targets.into_iter().map(Into::into).collect(); let mut world = JsWorld { index: input.index as usize };
    let mut number = 0.0; let mut point = Vector3::default();
    match input.op {
        PbOperation::SetHealth => state.set_health(input.value, &settings, &mut world),
        PbOperation::Damage => state.damage(input.value, (&input.attacker).into(), (&input.hit).into(), &settings, &mut world),
        PbOperation::Regenerate => state.regenerate(input.value, &settings, &mut world),
        PbOperation::Built => state.built(&settings, &mut world),
        PbOperation::Destroyed => state.destroyed(&settings, &mut world),
        PbOperation::Sphere => (point, number) = state.sphere(&settings),
        PbOperation::BlastSphere => (point, number) = state.blast_sphere(&settings),
        PbOperation::Serve => number = Station::serve(&input.supply.expect("station supply").into(), &mut targets[0]) as f64,
        PbOperation::Update => {
            let s = station.as_mut().expect("station state");
            let soldiers: Vec<_> = input.soldiers.iter().map(|n| *n as usize).collect(); let vehicles: Vec<_> = input.vehicles.iter().map(|n| *n as usize).collect();
            s.update(input.value, &settings, &input.supply.expect("station supply").into(), &mut targets, &soldiers, &vehicles, &mut world); state = s.building.clone();
        }
    }
    if let Some(s) = &mut station { s.building = state.clone(); }
    let result = PbResult { state: (&state).into(), station: station.as_ref().map(Into::into), targets: targets.iter().map(Into::into).collect(), number, point: a3(point) };
    let sim = sim(); sim.calls.result(PbResult::WORDS, |row, heap| result.put(row, heap)); sim.calls.block.ptr()
}
