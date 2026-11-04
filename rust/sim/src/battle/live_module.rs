//! LiveObjects query commands, using the existing target adapters for
//! lifecycle effects and the collision world's own query implementation.
use super::live::{self, Damage, Definition, Health, Hit, Out, Queries, Target, World};
use super::props::{Identity, State};
use crate::abi::sim;
use crate::gen::*;
use crate::ordnance::defs::{ExplosionDef, Field};
use three::{Matrix4, Vector3};

fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
fn a3(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
fn field(v: PqValue) -> Field { match v.kind {
    PqValueKind::Undefined => Field::Undefined, PqValueKind::Null => Field::Null, PqValueKind::Boolean => Field::Bool(v.number != 0.0),
    PqValueKind::Number => Field::Num(v.number),
    PqValueKind::String => Field::Str(String::from_utf16(sim().strings.units(v.text)).expect("definition primitive string")),
} }
fn value(v: Field) -> PqValue {
    let (kind, number, text) = match v { Field::Undefined => (PqValueKind::Undefined, 0.0, String::new()), Field::Null => (PqValueKind::Null, 0.0, String::new()),
        Field::Bool(v) => (PqValueKind::Boolean, f64::from(v), String::new()), Field::Num(v) => (PqValueKind::Number, v, String::new()),
        Field::Str(v) => (PqValueKind::String, 0.0, v), _ => panic!("definition primitive") };
    PqValue { kind, number, text: sim().strings.intern_str(&text) }
}
fn definition(v: PqDefinition) -> Definition {
    Definition { repair: v.repair.into_iter().map(field).collect::<Vec<_>>().try_into().expect("four repair types"),
        target_vehicle: field(v.target_vehicle), target_neutral: field(v.target_neutral), target_enemy: field(v.target_enemy), target_friendly: field(v.target_friendly),
        range: field(v.range), angle: field(v.angle), shield_scale: field(v.shield_scale),
        explosion: ExplosionDef { damage: field(v.damage), damage_radius_inner: field(v.inner), damage_radius_outer: field(v.outer),
            scales: v.scales.into_iter().map(field).collect::<Vec<_>>().try_into().expect("five health scales") } }
}
fn target(v: PqTarget) -> Target {
    let mut matrix = Matrix4::default(); matrix.from_array(&v.matrix, 0);
    Target { alive: v.alive, state: v.state.map(|s| match s { PbStateName::Built => State::Built, PbStateName::Unbuilt => State::Unbuilt, PbStateName::Destroyed => State::Destroyed }),
        health_type: v.health_type.map(|h| match h { PqHealth::Building => Health::Building, PqHealth::Vehicle => Health::Vehicle, PqHealth::Rebuild => Health::Rebuild, PqHealth::Build => Health::Build }),
        health: v.health, max_health: v.max_health, team: v.team, body: v.body.map(|n| n as usize), gunner: v.gunner, position: v3(v.position), matrix }
}
impl From<PqState> for Queries { fn from(v: PqState) -> Self { Self { center: v3(v.center), toward: v3(v.toward) } } }
impl From<&Queries> for PqState { fn from(q: &Queries) -> Self { Self { center: a3(q.center), toward: a3(q.toward) } } }
struct JsWorld;
impl JsWorld {
    fn args(&self, kind: PqActionKind, index: usize) -> PqAction { PqAction { kind, index: index as f64, ..PqAction::default() } }
    fn effect(&mut self, q: &mut Queries, args: PqAction) -> PqActionResult {
        let result = import_pq_action(&PqAction { state: (&*q).into(), ..args });
        *q = result.state.clone().into(); result
    }
}
impl World for JsWorld {
    fn target(&mut self, index: usize) -> Target { target(import_pq_action(&self.args(PqActionKind::Target, index)).target.expect("query target")) }
    fn sphere(&mut self, index: usize, blast: bool, point: Vector3) -> (Vector3, f64) {
        let r = import_pq_action(&PqAction { blast, point: a3(point), ..self.args(PqActionKind::Sphere, index) }); (v3(r.point), r.number)
    }
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, ignore: Option<usize>, static_only: bool, out: Out) -> Option<Hit> {
        let r = import_pq_action(&PqAction { point: a3(origin), direction: a3(direction), far, ignore: ignore.map(|n| n as f64), static_only, lock: matches!(out, Out::Lock), ..self.args(PqActionKind::Cast, 0) });
        r.found.then(|| Hit { distance: r.hit_distance, owner: (r.hit_owner >= 0.0).then_some(r.hit_owner as usize) })
    }
    fn team_scale(&mut self, q: &mut Queries, index: usize) -> f64 { self.effect(q, self.args(PqActionKind::TeamScale, index)).number }
    fn damage(&mut self, q: &mut Queries, index: usize, amount: f64, owner: Identity, info: Option<Damage>) {
        let info = info.map(|v| PqDamage { direction: v.direction.map(a3), amount: v.amount, typed: v.typed, generic: value(v.generic) });
        self.effect(q, PqAction { amount, owner: owner.into(), info, ..self.args(PqActionKind::Damage, index) });
    }
    fn set_health(&mut self, q: &mut Queries, index: usize, health: Field) { self.effect(q, PqAction { value: value(health), ..self.args(PqActionKind::SetHealth, index) }); }
    fn may_enter(&mut self, q: &mut Queries, index: usize) -> bool { self.effect(q, self.args(PqActionKind::May, index)).flag }
}
#[no_mangle]
pub extern "C" fn live_words(which: u32) -> u32 { (match which { 0 => PqInput::WORDS, 1 => PqResult::WORDS, _ => 0 }) as u32 }
#[no_mangle]
pub extern "C" fn live_run() -> *const f64 {
    let (row, heap) = sim().calls.args(); let input = PqInput::take(row, heap);
    let def = definition(input.definition); let mut q: Queries = input.state.into(); let mut world = JsWorld;
    let one = (input.one >= 0.0).then_some(input.one as usize); let mut flag = false; let mut selected = None;
    let buildings: Vec<_> = input.buildings.into_iter().map(|n| n as usize).collect();
    match input.op {
        PqOperation::Blast => q.blast(&buildings, &def.explosion, v3(input.a), (&input.owner).into(), input.team_scale, input.collision, &mut world),
        PqOperation::Struck => q.struck(if input.building { one } else { None }, &def, (&input.owner).into(), input.team_scale, input.direction.map(v3), &mut world),
        PqOperation::Repairs => flag = live::repairs(&def, &world.target(one.expect("repair target")), input.team),
        PqOperation::Repair => flag = q.repair(one, &def, input.team, &mut world),
        PqOperation::LockOn => {
            let candidates = buildings.into_iter().chain(input.vehicles.into_iter().map(|n| n as usize)).collect::<Vec<_>>();
            selected = q.lock_on(&candidates, v3(input.a), v3(input.b), &def, one, input.held, input.team, input.collision, &mut world);
        }
        PqOperation::Enterable => selected = q.enterable(&input.turrets.into_iter().map(|n| n as usize).collect::<Vec<_>>(), v3(input.a), v3(input.b), input.may, &mut world),
    }
    let result = PqResult { state: (&q).into(), selected: selected.map_or(-1.0, |n| n as f64), flag };
    let sim = sim(); sim.calls.result(PqResult::WORDS, |row, heap| result.put(row, heap)); sim.calls.block.ptr()
}
