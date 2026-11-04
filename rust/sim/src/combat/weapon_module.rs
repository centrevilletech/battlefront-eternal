//! Ordinary authority weapons use the same core as combat and prediction.
//! Command-local values, including callback publications, never borrow Sim
//! while JavaScript can re-enter this or any other command.
use super::defs::K;
use super::weapons::{Admission, Aim, LockTarget, ShotKind, Trigger, WeaponDraws};
use crate::abi::sim;
use crate::gen::*;

struct Host { numeric_level: f64, numeric_seed: Option<f64> }
impl Host {
    fn action(&mut self, kind: WpCallback, weapon: &mut Weapon, first: bool) -> WpReply {
        let result = import_weapon_action(&WpAction { kind, state: WpState { weapon: weapon.clone(), numeric_level: self.numeric_level, numeric_seed: self.numeric_seed }, first });
        *weapon = result.state.weapon.clone();
        self.numeric_level = result.state.numeric_level;
        self.numeric_seed = result.state.numeric_seed;
        result
    }
}
impl WeaponDraws for Host {
    fn draw(&mut self, _: u32) -> f64 { unreachable!("ordinary RNG publishes its weapon") }
    fn legacy(&mut self, _: u32) -> f64 { unreachable!("ordinary RNG publishes its weapon") }
    fn seeded(&mut self, _: f64, _: f64) {}
    fn admit(&mut self, _: u32) -> Admission { unreachable!("ordinary admission publishes its weapon") }
    fn level_written(&mut self, _: u32, level: f64) { self.numeric_level = level; }
    fn draw_visible(&mut self, _: u32, state: &mut Weapon, first: bool) -> f64 { self.action(WpCallback::Random, state, first).value }
    fn legacy_visible(&mut self, _: u32, state: &mut Weapon) -> f64 { self.action(WpCallback::Legacy, state, false).value }
    fn admit_visible(&mut self, _: u32, state: &mut Weapon) -> Admission {
        let result = self.action(WpCallback::Admit, state, false);
        match result.admission { 0 => Admission::Yes, 1 => Admission::No, _ => Admission::Token(result.value) }
    }
    fn salvo_started(&mut self, _: u32, state: &Weapon) {
        self.numeric_seed = state.salvo_seed;
        self.action(WpCallback::Start, &mut state.clone(), false);
    }
    fn seeded_visible(&mut self, _: u32, state: &mut Weapon, first: bool, _: f64, _: f64) -> f64 {
        self.action(WpCallback::Seeded, state, first).value
    }
}
fn target(value: Option<WpTarget>) -> Option<LockTarget> {
    value.map(|v| LockTarget { id: v.id, generation: v.generation, alive: v.alive })
}
#[no_mangle]
pub extern "C" fn weapon_words(which: u32) -> u32 { (match which { 0 => WpInput::WORDS, 1 => WpOutput::WORDS, _ => 0 }) as u32 }
/// A retained closure's own cursor, independent of the weapon's numeric
/// salvoCount. Mulberry's increment makes this random access exact.
#[no_mangle]
pub extern "C" fn weapon_seed(seed: f64, index: f64) -> f64 {
    let state = jsrt::conv::to_uint32(seed).wrapping_add(0x6d2b79f5u32.wrapping_mul(jsrt::conv::to_uint32(index)));
    jsrt::rng::Mulberry32::new(state as f64).next_f64()
}
#[no_mangle]
pub extern "C" fn weapon_run() -> *const f64 {
    let (row, heap) = sim().calls.args();
    let input = WpInput::take(row, heap);
    let defs = super::module::defs();
    let mut weapon = input.state.weapon;
    let class = defs.weapons.class(weapon.def);
    let mut host = Host { numeric_level: input.state.numeric_level, numeric_seed: input.state.numeric_seed };
    let mut shots = Vec::new(); let mut flag = false; let mut spread = [0.0; 2];
    match input.op {
        WpOperation::Update => weapon.update(0, class, input.dt, input.selected,
            &Trigger { held: input.held, pressed: input.pressed, seed: input.seed, late: input.late }, input.reload,
            &Aim { stick: input.stick, posture: input.posture, zoomed: input.zoomed, repair_target: input.repair_target }, &mut host, &mut shots),
        WpOperation::Deselect => { weapon.deselect(); if !weapon.busy() { host.numeric_level = 0.0; } }
        WpOperation::Clear => weapon.clear_lock(),
        WpOperation::Lock => weapon.update_lock(class, input.dt, target(input.target)),
        WpOperation::Reload => flag = weapon.start_reload(class),
        WpOperation::Base => { weapon.base_level(); host.numeric_level = 0.0; }
        WpOperation::Spread => spread = weapon.spread(class, input.stick, input.posture, input.zoomed),
        WpOperation::Target => {
            let d = weapon.level(class);
            if d.get(K::Base).is_str("launcher") && d.get(K::Ordnance).get("base").is_str("missile") {
                let picked = host.action(WpCallback::Pick, &mut weapon, false);
                weapon.update_lock(class, input.dt, target(picked.target));
            }
        }
    }
    let shots = shots.into_iter().map(|s| {
        let mut result = WpShot::default();
        match s.kind {
            ShotKind::Round { yaw, pitch, charge, target_id, target_generation, admission } => {
                result.kind = WpShotKind::Round; result.yaw = yaw; result.pitch = pitch; result.charge = charge;
                result.target_id = target_id; result.target_generation = target_generation; result.admission = admission;
            }
            ShotKind::Repair => result.kind = WpShotKind::Repair,
            ShotKind::MeleeStart { swing } => { result.kind = WpShotKind::MeleeStart; result.swing = swing; }
            ShotKind::MeleeHit => result.kind = WpShotKind::MeleeHit,
            ShotKind::MeleeEnd => result.kind = WpShotKind::MeleeEnd,
        }
        result
    }).collect();
    let result = WpOutput { state: WpState { weapon, numeric_level: host.numeric_level, numeric_seed: host.numeric_seed }, shots, flag, spread };
    let sim = sim(); sim.calls.result(WpOutput::WORDS, |row, heap| result.put(row, heap)); sim.calls.block.ptr()
}
