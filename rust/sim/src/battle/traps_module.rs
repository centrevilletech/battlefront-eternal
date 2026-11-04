//! Trap commands own their input state and settings throughout each call.
//! Nothing in Sim is borrowed while a world callback can enter the module.
use super::rules_module::ModuleText;
use super::traps::{Phase, Settings, Trap, World};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{Ref, StrId};

impl From<TrPhase> for Phase {
    fn from(p: TrPhase) -> Self { match p { TrPhase::Down => Self::Down, TrPhase::Reset => Self::Reset, TrPhase::Armed => Self::Armed, TrPhase::Swing => Self::Swing } }
}
impl From<Phase> for TrPhase {
    fn from(p: Phase) -> Self { match p { Phase::Down => Self::Down, Phase::Reset => Self::Reset, Phase::Armed => Self::Armed, Phase::Swing => Self::Swing } }
}
impl From<TrState> for Trap {
    fn from(s: TrState) -> Self { Self { built: s.built, health: s.health, phase: s.phase.into(), started: s.started, time: s.time, sprung: s.sprung, tick: s.tick, crushed: s.crushed } }
}
impl From<&Trap> for TrState {
    fn from(s: &Trap) -> Self { Self { built: s.built, health: s.health, phase: s.phase.into(), started: s.started, time: s.time, sprung: s.sprung, tick: s.tick, crushed: s.crushed } }
}
impl From<TrSettings> for Settings {
    fn from(s: TrSettings) -> Self { Self { max_health: s.max_health, reach: s.reach, min_radius: s.min_radius, death_time: s.death_time,
        build_frames: s.build_frames, trigger_frames: s.trigger_frames, trigger_team: s.trigger_team, sound: s.sound,
        death_effect: s.death_effect, point: s.point, effect_at: s.effect_at } }
}

struct JsWorld { index: f64 }
impl JsWorld {
    fn query(&self, op: TrQueryOp, vehicle: Option<Ref<Vehicle>>) -> TrQueryResult { import_traps_query(&TrQueryArgs { op, index: self.index, vehicle }) }
    fn action(&self, trap: &mut Trap, op: TrActionOp, vehicle: Option<Ref<Vehicle>>, number: f64, flag: bool, name: Option<StrId>, position: [f64; 3]) {
        let result = import_traps_action(&TrActionArgs { op, index: self.index, state: (&*trap).into(), vehicle, number, flag, name, position });
        *trap = result.state.into();
    }
}
impl World for JsWorld {
    type Vehicle = Option<Ref<Vehicle>>;
    fn alive(&mut self, vehicle: Self::Vehicle) -> bool { vehicle.is_some() && self.query(TrQueryOp::Alive, vehicle).flag }
    fn sphere(&mut self, vehicle: Self::Vehicle) -> ([f64; 3], f64) { let r = self.query(TrQueryOp::Sphere, vehicle); (r.position, r.number) }
    fn side(&mut self, vehicle: Self::Vehicle) -> Option<StrId> { self.query(TrQueryOp::Side, vehicle).name }
    fn clear_attacker(&mut self, trap: &mut Trap, vehicle: Self::Vehicle) { self.action(trap, TrActionOp::ClearAttacker, vehicle, 0.0, false, None, [0.0; 3]); }
    fn set_health(&mut self, trap: &mut Trap, vehicle: Self::Vehicle, health: f64) { self.action(trap, TrActionOp::SetHealth, vehicle, health, false, None, [0.0; 3]); }
    fn set_dying(&mut self, trap: &mut Trap, vehicle: Self::Vehicle, time: f64) { self.action(trap, TrActionOp::SetDying, vehicle, time, false, None, [0.0; 3]); }
    fn hide_root(&mut self, trap: &mut Trap, vehicle: Self::Vehicle) { self.action(trap, TrActionOp::HideRoot, vehicle, 0.0, false, None, [0.0; 3]); }
    fn show(&mut self, trap: &mut Trap, built: bool) { self.action(trap, TrActionOp::Show, None, 0.0, built, None, [0.0; 3]); }
    fn sound(&mut self, trap: &mut Trap, name: StrId, at: [f64; 3]) { self.action(trap, TrActionOp::Sound, None, 0.0, false, Some(name), at); }
    fn effect(&mut self, trap: &mut Trap, name: StrId, at: [f64; 3]) { self.action(trap, TrActionOp::Effect, None, 0.0, false, Some(name), at); }
}

#[no_mangle]
pub extern "C" fn traps_words(which: u32) -> u32 { (match which { 0 => TrInput::WORDS, 1 => TrResult::WORDS, _ => 0 }) as u32 }

#[no_mangle]
pub extern "C" fn traps_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); TrInput::take(row, heap) };
    let mut trap: Trap = input.state.into();
    let settings: Settings = input.settings.into();
    let mut world = JsWorld { index: input.index };
    let mut flag = false;
    match input.op {
        TrOperation::SetHealth => trap.set_health(input.value, &settings, &mut world),
        TrOperation::Begin => trap.begin(input.phase.into(), input.started),
        TrOperation::Step => trap.step(input.dt, input.tick, &settings, &ModuleText, &input.vehicles, &mut world),
        TrOperation::Crush => trap.crush(&settings, &ModuleText, &input.vehicles, &mut world),
        TrOperation::Catches => flag = trap.catches(&settings, input.vehicle, &mut world),
        TrOperation::StateRecord => (),
    }
    let result = TrResult { state: (&trap).into(), flag, record: trap.state_record() };
    let sim = sim();
    sim.calls.result(TrResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}
