//! Ordinary vehicle and turret weapons from actual headless battles.
//! Every timer, shot and state field is computed by combat::weapons;
//! only the captured RNG draws are served, in their original order.
#![cfg(feature = "replay")]
use bfe_sim::combat::defs::{Js, JsObj, WeaponClass};
use bfe_sim::combat::weapons::{Admission, Aim, LockTarget, Shot, ShotKind, Trigger, WeaponDraws};
use bfe_sim::gen::{Posture, Replay, Weapon};
use sim_replay::{check_num, check_value, replay_corpus, Arena, Capture, Corpus, Event, Frame, Log, Node, Outcome, Value, V};
use std::rc::Rc;

fn definition(capture: &Capture, value: V) -> Js {
    if let Some((kind, _)) = value.handle() {
        assert_eq!(kind, "def", "weapon content only contains definition handles");
        return definition(capture, capture.def_of(value).expect("captured weapon definition"));
    }
    if value.is_undefined() { return Js::Undefined; }
    if value.is_null() { return Js::Null; }
    if let Some(v) = value.opt_bool() { return Js::Bool(v); }
    if let Some(v) = value.opt_num() { return Js::Num(v); }
    if let Some(v) = value.str() { return Js::Str(v.into()); }
    if matches!(value.node(), Some(Node::Array(_))) {
        return Js::Arr(Rc::new(value.items().map(|v| definition(capture, v)).collect()));
    }
    Js::Obj(Rc::new(JsObj { fields: value.fields().map(|(k, v)| (k.into(), definition(capture, v))).collect(), def: None }))
}
#[derive(Clone, Copy)]
enum Mutation { None, Dt, Draw }
struct Draws<'f> { log: Log<'f>, failure: Option<String>, nudge: bool }
impl Draws<'_> {
    fn check(&mut self, result: Result<(), impl ToString>) { if let Err(e) = result { self.failure.get_or_insert_with(|| e.to_string()); } }
    fn number(&mut self, label: &str) -> f64 {
        match self.log.draw(label) { Ok(v) => v, Err(e) => { self.failure.get_or_insert_with(|| e.to_string()); 0.5 } }
    }
    fn nudge(&mut self, value: f64) -> f64 { if std::mem::take(&mut self.nudge) { 1.0 - value } else { value } }
}
impl WeaponDraws for Draws<'_> {
    fn draw(&mut self, _: u32) -> f64 {
        let label = match self.log.peek() {
            Some(Event::Draw(d)) if !d.stream.starts_with("seeded:") && d.stream != "Math.random" => d.stream.clone(),
            _ => String::from("ordinary weapon generator"),
        };
        let raw = self.number(&label);
        self.nudge((jsrt::conv::to_uint32(raw) >> 9) as f64 / 8388608.0)
    }
    fn legacy(&mut self, _: u32) -> f64 { let value = self.number("Math.random"); self.nudge(value) }
    fn seeded(&mut self, seed: f64, value: f64) {
        let expected = self.number(&format!("seeded:{seed}"));
        if value.to_bits() != expected.to_bits() { self.failure.get_or_insert_with(|| format!("seeded draw: {expected:?} != {value:?}")); }
    }
    fn seeded_visible(&mut self, _: u32, _: &mut Weapon, _: bool, seed: f64, value: f64) -> f64 { self.seeded(seed, value); self.nudge(value) }
    fn admit(&mut self, _: u32) -> Admission {
        self.failure.get_or_insert_with(|| "ordinary holder has an uncaptured admission callback".into()); Admission::No
    }
}
fn shot_value(shot: &Shot, frame: &Frame, arena: &mut Arena) -> Value {
    let owner = arena.copy(frame.receiver());
    let mut fields = vec![("weapon", owner)];
    match shot.kind {
        ShotKind::Round { yaw, pitch, charge, target_id, target_generation, admission } => {
            assert!(admission.is_none());
            fields.extend([("yaw", Value::Num(yaw)), ("pitch", Value::Num(pitch)), ("charge", Value::Num(charge)),
                ("targetId", Value::Num(target_id)), ("targetGeneration", Value::Num(target_generation))]);
        }
        ShotKind::Repair => fields.push(("repair", Value::Bool(true))),
        ShotKind::MeleeStart { .. } | ShotKind::MeleeHit | ShotKind::MeleeEnd => panic!("ordinary holder has an unexpected melee weapon"),
    }
    arena.object("", fields)
}
fn replay_frame(capture: &Capture, frame: &Frame, mutation: Mutation) -> Result<bool, String> {
    let before = frame.before();
    if before.get("skipped").truthy() { return Ok(false); }
    let mut cx = Replay::default();
    let mut weapon = Weapon::from_value(before.get("weapon"), &mut cx, "weapon")?;
    if weapon.level.to_bits() != before.get("definition").num().to_bits() { return Err("ordinary weapon has a separate numeric level".into()); }
    if weapon.salvo_seed.is_some() != before.get("seeded").bool() { return Err("ordinary weapon has a separately replaced seeded closure".into()); }
    let Js::Obj(class) = definition(capture, before.get("weapon").get("def")) else { return Err("weapon class is not an object".into()); };
    let class = WeaponClass::new(class);
    let mut draws = Draws { log: Log::new(frame), failure: None, nudge: matches!(mutation, Mutation::Draw) };
    let mut arena = Arena::default();
    let mut result = Value::Undefined;
    let mut dt = if matches!(frame.name.as_str(), "Weapon.update" | "Weapon.updateLock") { frame.arg(0).num() } else { 0.0 };
    if matches!(mutation, Mutation::Dt) { dt = f64::from_bits(dt.to_bits() + 1); }
    match frame.name.as_str() {
        "Weapon.update" => {
            let trigger = frame.arg(2); let aim = frame.arg(4);
            let seed = trigger.get("seed"); let late = trigger.get("late").opt_num().unwrap_or(0.0);
            let mut shots = Vec::new();
            weapon.update(0, &class, dt, frame.arg(1).truthy(), &Trigger { held: trigger.get("held").truthy(), pressed: trigger.get("pressed").truthy(),
                seed: (!seed.is_nullish()).then(|| seed.num()), late }, frame.arg(3).truthy(),
                &Aim { stick: aim.get("stick").opt_num().unwrap_or(0.0), posture: match aim.get("posture").str() { Some("stand") => Some(Posture::Stand), Some("crouch") => Some(Posture::Crouch), Some("prone") => Some(Posture::Prone), _ => None },
                    zoomed: aim.get("zoomed").truthy(), repair_target: aim.get("repairTarget").truthy() }, &mut draws, &mut shots);
            let shots = shots.iter().map(|s| shot_value(s, frame, &mut arena)).collect(); result = arena.array(shots);
        }
        "Weapon.deselect" => weapon.deselect(),
        "Weapon.clearLock" => weapon.clear_lock(),
        "Weapon.baseLevel" => weapon.base_level(),
        "Weapon.startReload" => result = Value::Bool(weapon.start_reload(&class)),
        "Weapon.updateLock" => {
            let target = before.get("target");
            weapon.update_lock(&class, dt, (!target.is_nullish()).then(|| LockTarget { id: target.get("id").num(), generation: target.get("generation").num(), alive: target.get("alive").bool() }));
        }
        "Weapon.spread" => {
            let posture = match frame.arg(1).str() { Some("stand") => Some(Posture::Stand), Some("crouch") => Some(Posture::Crouch), Some("prone") => Some(Posture::Prone), _ => None };
            result = arena.array(weapon.spread(&class, frame.arg(0).num(), posture, frame.arg(2).bool()).map(Value::Num).to_vec());
        }
        _ => return Err(format!("unknown weapon method {}", frame.name)),
    }
    draws.check(check_value("return", frame.result(), arena.view(&result)));
    let actual = weapon.to_value(&mut cx, &mut arena);
    draws.check(check_value("weapon", frame.after().get("weapon"), arena.view(&actual)));
    draws.check(check_num("definition", frame.after().get("definition"), weapon.level));
    let active = Value::Bool(weapon.salvo_seed.is_some());
    draws.check(check_value("seeded closure", frame.after().get("seeded"), arena.view(&active)));
    draws.log.finish().map_err(|e| e.to_string())?;
    if let Some(error) = draws.failure { return Err(error); }
    Ok(true)
}
fn report(method: &str, mutation: Mutation) -> Option<sim_replay::Report> {
    let corpus = Corpus::from_env().unwrap()?;
    Some(replay_corpus(&corpus, method, |capture, frame| match replay_frame(capture, frame, mutation) {
        Ok(true) => Outcome::Same, Ok(false) => Outcome::Skipped("weapon belongs to unit combat".into()), Err(e) => Outcome::Failed(e),
    }).unwrap())
}
#[test]
fn ordinary_holder_weapon_methods_replay_exactly() {
    let mut replayed = 0;
    for method in ["update", "deselect", "clearLock", "updateLock", "startReload", "baseLevel", "spread"] {
        let Some(report) = report(&format!("Weapon.{method}"), Mutation::None) else { return };
        println!("{}", report.summary()); replayed += report.replayed(); report.assert_clean();
    }
    assert!(replayed > 0, "the corpus must contain ordinary holder weapon methods");
}
#[test]
fn ordinary_weapon_replay_detects_changed_time_and_rng() {
    for mutation in [Mutation::Dt, Mutation::Draw] {
        let Some(report) = report("Weapon.update", mutation) else { return };
        println!("mutated ordinary update: {} of {} frames differ", report.failures.len(), report.replayed());
        assert!(!report.failures.is_empty(), "mutation was not exercised");
    }
}
