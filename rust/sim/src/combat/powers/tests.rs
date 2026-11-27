//! Invented powers and values exercising each rule of the module.
    use bfe_wide::{Fnv, Wide};

use super::cast::{PowerInput, PowerKit, PowerState, Request, HOLD_RECHECK_TICKS};
use super::def::{from_weapon, Effect, PowerDef};
use super::effects::{EffectTable, HOLD_RELEASE_TICKS};
use super::geometry::{self, AimFrame, Footing, Nearest, Reaction};
use super::lock::{LockCandidate, LockState, PROVISIONAL_LOCK_UNSEEN_TICKS};
use super::key::LifeKey;
use crate::movement::soldier::swbf2::{ControlSpeed, State, Tuning};
use bfe_content::{json, Value};

fn record(text: &str) -> Value {
    json::parse(text).expect("test record")
}

fn tuning(capacity: f64) -> Tuning {
    let row = |forward, strafe, turn| ControlSpeed { forward, strafe, turn };
    Tuning {
        stand: row(1.0, 1.0, 1.0),
        crouch: row(0.6, 0.6, 1.0),
        sprint: row(1.8, 0.4, 0.5),
        jump: row(0.05, 0.05, 0.5),
        jet: row(0.9, 0.7, 0.5),
        jetpack: None,
        roll: row(0.0, 0.0, 0.3),
        tumble: row(0.0, 0.0, 0.1),
        sprint_accelerate_time: 0.3,
        energy_capacity: capacity,
        energy_floor: 0.0,
        energy_restore: 7.0,
        energy_restore_idle: 11.0,
        energy_drain_sprint: 13.0,
        energy_min_sprint: 9.0,
        energy_cost_jump: 0.0,
        energy_cost_jump_sprint: None,
        energy_cost_roll: 17.0,
        jump_height: 2.5,
        jump_forward_speed_factor: 1.1,
        jump_strafe_speed_factor: 0.9,
        roll_speed_factor: 1.4,
        roll_frames: 18,
        recover_from_tumble: false,
        getup_front_seconds: 1.9,
        getup_back_seconds: 1.7,
    }
}

const SHOVE: &str = r#"{"class":"inv_power_shove","base":"cannon","slot":2,"channel":1,"TriggerSingle":"1",
 "InitialSalvoDelay":"0.25","ShotDelay":"0.4","EnergyDrain":"30","LockOnRange":"40",
 "ordnance":{"base":"emitterordnance","ConeLength":"12","ConeAngle":"90","MaxTargets":"3",
 "NoChaining":"1","Push":"9","AffectFriends":"0","Damage":"0"}}"#;

const TUG: &str = r#"{"class":"inv_power_tug","base":"cannon","slot":3,"channel":1,"InitialSalvoDelay":"0.2",
 "ShotDelay":"0.3","EnergyDrain":"25","ordnance":{"base":"emitterordnance","ConeLength":"30",
 "ConeAngle":"40","MaxTargets":"1","NoChaining":"1","Push":"-20","AffectFriends":"0"}}"#;

const STORM: &str = r#"{"class":"inv_power_storm","base":"cannon","slot":2,"channel":1,"TriggerSingle":"0",
 "InitialSalvoDelay":"0.3","ShotDelay":"0.2","EnergyDrain":"12","ordnance":{"base":"emitterordnance",
 "ConeLength":"9","ConeAngle":"60","MaxTargets":"5","NoChaining":"1","Damage":"40","MaxDamage":"70",
 "VehicleScale":"0.25"}}"#;

const RALLY: &str = r#"{"class":"inv_power_rally","base":"cannon","slot":4,"channel":1,"ammo":2,"RoundsPerClip":"1",
 "InitialSalvoDelay":"0.1","ShotDelay":"0.5","ordnance":{"base":"emitterordnance","Radius":"7",
 "ConeLength":"0","ConeAngle":"45","ApplyOnOwnerIfOnOthers":"1","AffectFriends":"1","AffectEnemies":"0",
 "BuffOffenseTimer":"5","BuffOffenseMult":"1.3","BuffHealthTimer":"3","BuffHealthRate":"8"}}"#;

const GRIP: &str = r#"{"class":"inv_power_grip","base":"areaeffectweapon","slot":3,"channel":1,"FireType":"Hold",
 "EffectType":"Choke","AreaType":"Ray","AreaRange":"14","AreaRadius":"8","AreaHeight":"3",
 "MaxTargets":"6","EffectDamage":"90","EffectStrength":"4","ThrustFactor":"0.7",
 "EnergyDrain":"22","EnergyDrainRate":"9","InitialSalvoDelay":"0.15"}"#;

const SLAM: &str = r#"{"Damage":"300","DamageRadiusInner":"1","DamageRadiusOuter":"6","Push":"9",
 "PushRadiusInner":"2","PushRadiusOuter":"5","HurtOwner":"0","DroidScale":"2"}"#;

const SLAM_BLADE: &str = r#"{"class":"inv_blade","base":"melee","slot":1,"ExplosionName":"inv_slam"}"#;

const WARD: &str = r#"{"class":"inv_power_ward","base":"cannon","slot":3,"channel":1,"ammo":1,"RoundsPerClip":"1",
 "InitialSalvoDelay":"0.1","ShotDelay":"0.5","ordnance":{"base":"emitterordnance","Radius":"8",
 "ConeLength":"0","ConeAngle":"45","ApplyOnOwnerIfOnOthers":"1","AffectFriends":"1","AffectEnemies":"0",
 "BuffDefenseTimer":"6","BuffDefenseMult":"0.25"}}"#;

fn def(text: &str) -> PowerDef {
    from_weapon(&record(text)).expect("valid power").expect("a power")
}

fn kit(defs: &[(&str, u32)]) -> PowerKit {
    let mut kit = PowerKit::default();
    for (text, slot) in defs {
        assert!(kit.push(*slot, def(text)));
    }
    kit
}

#[test]
fn records_become_cones_auras_and_held_areas_with_their_authored_numbers() {
    let shove = def(SHOVE);
    let Effect::Cone(cone) = shove.effect else { panic!("cone") };
    assert_eq!((shove.trigger.windup, shove.trigger.recovery, shove.trigger.single), (15, 24, true));
    assert_eq!((shove.cost.energy, shove.cost.on_press, shove.cost.rounds), (30.0, false, None));
    assert_eq!((cone.length, cone.max_targets, cone.push, shove.lock_range), (12.0, 3, 9.0, 40.0));
    assert!((cone.cos_half - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    assert!(!cone.relations.friends && cone.relations.enemies, "an unset key keeps its default");

    let storm = def(STORM);
    let Effect::Cone(cone) = storm.effect else { panic!("cone") };
    assert!(!storm.trigger.single);
    assert!(cone.relations.friends && cone.relations.enemies, "emitters reach friends unless told not to");
    assert_eq!(storm.scale(crate::combat::impacts::HealthScale::Vehicle), 0.25);
    assert_eq!(cone.damage_per_target(0.0, 3), 40.0, "uncharged damage is the floor");
    assert_eq!(cone.damage_per_target(1.0, 1), 70.0);
    assert_eq!(cone.damage_per_target(1.0, 7), 40.0);

    let rally = def(RALLY);
    let Effect::Aura(aura) = rally.effect else { panic!("aura") };
    assert_eq!(rally.cost.rounds, Some(2));
    assert_eq!((aura.radius, aura.owner_with_others), (7.0, true));
    assert_eq!(aura.offense.map(|b| (b.seconds, b.value)), Some((5.0, 1.3)));
    assert!(aura.defense.is_none());
    assert_eq!(aura.regen.map(|b| b.value), Some(8.0));

    let grip = def(GRIP);
    let Effect::Area(area) = grip.effect else { panic!("area") };
    assert!(grip.cost.on_press);
    assert_eq!((grip.cost.energy, grip.cost.rate), (22.0, 9.0));
    assert_eq!(area.max_targets, 4, "an area weapon keeps at most four targets");
    assert!((area.cos_half - v8math::cos(8.0f64.to_radians())).abs() < 1e-12);
}

#[test]
fn guns_blades_and_unsupported_area_forms_are_refused_or_ignored() {
    assert_eq!(from_weapon(&record(r#"{"class":"inv_gun","base":"cannon","ordnance":{"base":"bolt"}}"#)), Ok(None));
    assert_eq!(from_weapon(&record(r#"{"class":"inv_blade","base":"melee"}"#)), Ok(None));
    let chained = SHOVE.replace(r#""NoChaining":"1","#, "");
    assert!(from_weapon(&record(&chained)).is_err(), "a chaining emitter is refused");
    let cylinder = GRIP.replace(r#""AreaType":"Ray""#, r#""AreaType":"Cylinder""#);
    assert!(from_weapon(&record(&cylinder)).is_err());
    let negative = SHOVE.replace(r#""EnergyDrain":"30""#, r#""EnergyDrain":"-3""#);
    assert!(from_weapon(&record(&negative)).is_err());
    let unreadable = TUG.replace(r#""ConeLength":"30""#, r#""ConeLength":"far""#);
    assert!(from_weapon(&record(&unreadable)).is_err());
}

#[test]
fn an_emitter_on_the_primary_trigger_stays_a_gun() {
    let torch = STORM.replace(r#""channel":1,"#, r#""channel":0,"#);
    assert_eq!(from_weapon(&record(&torch)), Ok(None));
    let unset = STORM.replace(r#""channel":1,"#, "");
    assert_eq!(from_weapon(&record(&unset)), Ok(None), "a weapon without a channel is on the primary trigger");
    let class = format!(r#"{{"class":"inv_hero","weapons":[{},{}]}}"#, torch.replace(r#""slot":2"#, r#""slot":1"#), TUG);
    let p = super::world::Powers::new(4, [("inv_hero", &record(&class))], |_| None).unwrap();
    let kit = p.kit(p.kit_of("inv_hero").unwrap()).unwrap();
    assert_eq!(kit.iter().count(), 1, "only the offhand weapon joins the kit");
    assert_eq!((kit.index_of_slot(1), kit.index_of_slot(3)), (None, Some(0)));
}

fn press(index: u8) -> PowerInput {
    PowerInput { selected: Some(index), pressed: true, held: true }
}

fn hold(index: u8) -> PowerInput {
    PowerInput { selected: Some(index), pressed: false, held: true }
}

/// The trigger up with `index` still selected.
fn let_go(index: u8) -> PowerInput {
    PowerInput { selected: Some(index), ..PowerInput::default() }
}

#[test]
fn an_emitter_pays_and_fires_after_its_windup_then_waits_out_its_recovery() {
    let kit = kit(&[(SHOVE, 2)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    let first = s.step(&kit, press(0), true, &mut energy, &t);
    assert_eq!((first.request(), energy.energy), (None, 100.0), "nothing is spent on the press");
    let mut fired = None;
    for tick in 1..=40 {
        let step = s.step(&kit, PowerInput { selected: Some(0), ..PowerInput::default() }, true, &mut energy, &t);
        if step.request().is_some() {
            fired = Some((tick, step.request(), step.energy_spent));
        }
    }
    assert_eq!(fired, Some((15, Some(Request::Fire(0)), 30.0)), "the cast lands 15 ticks after the press");
    assert_eq!(energy.energy, 70.0);
    // Another press during the recovery is not remembered.
    let mut again = PowerState::new(&kit);
    again.step(&kit, press(0), true, &mut energy, &t);
    for _ in 0..15 {
        again.step(&kit, let_go(0), true, &mut energy, &t);
    }
    assert!(again.step(&kit, press(0), true, &mut energy, &t).request().is_none());
}

#[test]
fn a_held_repeating_emitter_casts_once_per_windup_and_recovery() {
    let kit = kit(&[(STORM, 2)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    let mut casts = Vec::new();
    for tick in 0..120 {
        let input = if tick == 0 { press(0) } else { hold(0) };
        if let Some(Request::Fire(_)) = s.step(&kit, input, true, &mut energy, &t).request() {
            casts.push(tick);
        }
    }
    // 18 ticks of windup then 12 of recovery: one cast every 30 ticks.
    assert_eq!(casts, vec![18, 48, 78, 108]);
    assert_eq!(energy.energy, 100.0 - 4.0 * 12.0);
    // Releasing during a windup still lets that cast land, then stops.
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    let mut released = Vec::new();
    for tick in 0..90 {
        let input = match tick {
            0 => press(0),
            1..=5 => hold(0),
            _ => PowerInput { selected: Some(0), ..PowerInput::default() },
        };
        if s.step(&kit, input, true, &mut energy, &t).request().is_some() {
            released.push(tick);
        }
    }
    assert_eq!(released, vec![18]);
}

#[test]
fn the_stamina_latch_refuses_a_cast_and_rounds_run_out() {
    let kit = kit(&[(SHOVE, 2), (RALLY, 4)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    energy.energy = 12.0;
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    let mut spent = 0.0;
    for _ in 0..20 {
        spent += s.step(&kit, let_go(0), true, &mut energy, &t).energy_spent;
    }
    assert_eq!(spent, 30.0, "one overdraw is allowed while the latch is clear");
    assert!(energy.exhausted && energy.energy == 0.0);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    let refused = (0..20).any(|_| s.step(&kit, let_go(0), true, &mut energy, &t).request().is_some());
    assert!(!refused, "a latched soldier cannot cast");

    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    let mut fired = 0;
    for _ in 0..3 {
        s.step(&kit, press(1), true, &mut energy, &t);
        for _ in 0..60 {
            fired += usize::from(s.step(&kit, let_go(1), true, &mut energy, &t).request().is_some());
        }
    }
    assert_eq!((fired, s.rounds(1), energy.energy), (2, 0, 100.0), "two rounds, no stamina");
}

#[test]
fn a_held_area_pays_on_the_press_acquires_after_its_windup_and_drains_while_held() {
    let kit = kit(&[(GRIP, 3)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    let first = s.step(&kit, press(0), true, &mut energy, &t);
    assert_eq!((first.energy_spent, energy.energy, first.casting), (22.0, 78.0, Some(0)));
    let mut acquired = None;
    for tick in 1..=9 {
        if let Some(r) = s.step(&kit, hold(0), true, &mut energy, &t).request() {
            acquired = Some((tick, r));
        }
    }
    assert_eq!(acquired, Some((9, Request::Acquire(0))));
    let mut rechecks = 0;
    for _ in 0..HOLD_RECHECK_TICKS * 2 {
        let step = s.step(&kit, hold(0), true, &mut energy, &t);
        assert_eq!((step.energy_rate, step.thrust), (Some(-9.0), 0.7));
        if let Some(Request::Sustain { recheck: true, .. }) = step.request() {
            rechecks += 1;
        }
    }
    assert_eq!(rechecks, 2);
    let released = s.step(&kit, PowerInput { selected: Some(0), ..PowerInput::default() }, true, &mut energy, &t);
    assert!(released.request().is_none() && s.holding().is_none());
    // Without a target the world releases it at once.
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    for _ in 0..9 {
        s.step(&kit, hold(0), true, &mut energy, &t);
    }
    s.release(&kit, 0, true);
    assert!(s.holding().is_none());
    // Releasing before the windup ends acquires nothing.
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    let early = (0..12).filter_map(|_| s.step(&kit, PowerInput { selected: Some(0), ..PowerInput::default() }, true, &mut energy, &t).request()).count();
    assert_eq!(early, 0);
}

#[test]
fn being_unable_to_act_cancels_a_windup_and_a_hold() {
    let kit = kit(&[(GRIP, 3), (SHOVE, 2)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(1), true, &mut energy, &t);
    assert!(s.step(&kit, hold(1), false, &mut energy, &t).request().is_none());
    assert_eq!(s.active(), None);
    let spent = (0..30).map(|_| s.step(&kit, hold(1), true, &mut energy, &t).energy_spent).sum::<f64>();
    assert_eq!(spent, 0.0, "the cancelled cast never fires");
}

#[test]
fn an_emitter_lands_after_a_release_but_not_after_the_wheel_leaves_it() {
    let kit = kit(&[(SHOVE, 2), (TUG, 3)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    let landed: Vec<_> = (1..=20).filter(|_| s.step(&kit, let_go(0), true, &mut energy, &t).request() == Some(Request::Fire(0))).collect();
    assert_eq!(landed.len(), 1, "letting go of the trigger keeps the cast");
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    s.step(&kit, let_go(0), true, &mut energy, &t);
    let after_wheel = (0..30).filter_map(|_| s.step(&kit, let_go(1), true, &mut energy, &t).request()).count();
    assert_eq!(after_wheel, 0, "the wheel drops a cast that was still winding up");
    assert!(!s.is_active(0));
}

#[test]
fn an_exhausted_soldier_starts_no_wind_up_and_casts_once_the_latch_clears_under_a_held_trigger() {
    let t = tuning(100.0);
    for (record, slot, single) in [(SHOVE, 2, true), (STORM, 2, false)] {
        let kit = kit(&[(record, slot)]);
        let windup = kit.get(0).unwrap().trigger.windup;
        let mut energy = State::new(&t);
        energy.energy = 0.0;
        energy.exhausted = true;
        let mut s = PowerState::new(&kit);
        for tick in 0..40 {
            let input = if tick == 0 { press(0) } else { hold(0) };
            let step = s.step(&kit, input, true, &mut energy, &t);
            assert!(step.request().is_none() && step.casting.is_none(), "no wind-up while exhausted, tick {tick}");
            assert!(s.active().is_none() && !s.spent(), "the trigger stays unspent while exhausted, tick {tick}");
        }
        energy.recover(50.0, &t);
        assert!(!energy.exhausted);
        let mut casts = Vec::new();
        for tick in 0..60u32 {
            if let Some(Request::Fire(_)) = s.step(&kit, hold(0), true, &mut energy, &t).request() {
                casts.push(tick);
            }
        }
        assert_eq!(casts.first(), Some(&windup), "a full wind-up after the latch clears");
        assert_eq!(casts.len() == 1, single, "a single-shot power casts once, a repeating one goes on");
    }
}

#[test]
fn a_held_area_holds_for_free_while_the_wheel_selects_a_weapon_that_is_no_power() {
    // The 2005 game: a choke held while the wheel selects a weapon that is not a power (a
    // returning blade, which needs a fresh press) keeps holding and hurting, with no drain
    // and no slow-down, until the trigger is let go, the target is lost or the soldier tires.
    let kit = kit(&[(GRIP, 3)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    for _ in 1..20 {
        s.step(&kit, hold(0), true, &mut energy, &t);
    }
    let paid = energy.energy;
    let other = PowerInput { selected: None, pressed: false, held: true };
    for tick in 0..600 {
        let step = s.step(&kit, other, true, &mut energy, &t);
        assert!(matches!(step.request(), Some(Request::Sustain { power: 0, .. })), "still holding on tick {tick}");
        assert_eq!((step.energy_rate, step.thrust), (None, 1.0), "free: no drain, no slow-down");
    }
    assert_eq!(energy.energy, paid, "nothing more is paid");
    s.step(&kit, PowerInput::default(), true, &mut energy, &t);
    assert!(!s.is_holding(0), "letting go ends it");
}

#[test]
fn a_held_area_goes_on_while_the_wheel_selects_a_repeating_cone_that_fires_from_the_held_trigger() {
    let kit = kit(&[(STORM, 2), (GRIP, 3)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(1), true, &mut energy, &t);
    for _ in 1..20 {
        s.step(&kit, hold(1), true, &mut energy, &t);
    }
    assert!(s.is_holding(1));
    let (mut casts, mut sustained) = (Vec::new(), 0);
    for tick in 20..120 {
        let step = s.step(&kit, hold(0), true, &mut energy, &t);
        assert_eq!((step.energy_rate, step.thrust), (None, 1.0), "only the selected power is asked for its rate and thrust");
        for request in step.requests() {
            match request {
                Request::Fire(0) => casts.push(tick),
                Request::Sustain { power: 1, .. } => sustained += 1,
                other => panic!("unexpected {other:?} on tick {tick}"),
            }
        }
    }
    assert_eq!(casts, vec![38, 68, 98], "the selected cone winds up from the held trigger and repeats");
    assert_eq!(sustained, 100, "the hold goes on on every tick after the switch");
    assert_eq!(energy.energy, 100.0 - 22.0 - 3.0 * 12.0);
    // Letting go ends the hold.
    s.step(&kit, PowerInput { selected: Some(0), ..PowerInput::default() }, true, &mut energy, &t);
    assert!(!s.is_holding(1));
}

#[test]
fn the_wheel_starts_a_held_area_from_a_trigger_that_no_single_cast_has_spent() {
    let kit = kit(&[(STORM, 2), (GRIP, 3), (SHOVE, 4)]);
    let t = tuning(100.0);
    // From a repeating cone that never spends the trigger: the area starts at once,
    // and the cone that was still winding up drops its cast.
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    for _ in 1..5 {
        s.step(&kit, hold(0), true, &mut energy, &t);
    }
    let switched = s.step(&kit, hold(1), true, &mut energy, &t);
    assert_eq!((switched.energy_spent, switched.casting), (22.0, Some(1)));
    let mut seen = Vec::new();
    for tick in 6..30 {
        seen.extend(s.step(&kit, hold(1), true, &mut energy, &t).requests().filter(|r| !matches!(r, Request::Sustain { .. })).map(|r| (tick, r)));
    }
    assert_eq!(seen, vec![(14, Request::Acquire(1))]);
    assert_eq!(energy.energy, 100.0 - 22.0, "the dropped cast costs nothing");
    // After a single-shot cast spent it: nothing starts until a release and a press.
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(2), true, &mut energy, &t);
    assert!(s.spent());
    for _ in 1..40 {
        let step = s.step(&kit, hold(1), true, &mut energy, &t);
        assert!(!s.is_active(1) && step.energy_spent == 0.0 || step.request() == Some(Request::Fire(2)));
    }
    s.step(&kit, PowerInput { selected: Some(1), ..PowerInput::default() }, true, &mut energy, &t);
    assert_eq!(s.step(&kit, press(1), true, &mut energy, &t).energy_spent, 22.0);
}

#[test]
fn a_single_area_that_lets_go_under_a_held_trigger_spends_it() {
    let kit = kit(&[(GRIP, 3)]);
    let t = tuning(100.0);
    let mut energy = State::new(&t);
    let mut s = PowerState::new(&kit);
    s.step(&kit, press(0), true, &mut energy, &t);
    for _ in 0..9 {
        s.step(&kit, hold(0), true, &mut energy, &t);
    }
    s.release(&kit, 0, true);
    assert!(s.spent() && !s.is_holding(0));
    let again = (0..30).map(|_| s.step(&kit, hold(0), true, &mut energy, &t).energy_spent).sum::<f64>();
    assert_eq!(again, 0.0, "a spent trigger starts nothing while it stays down");
    s.step(&kit, PowerInput { selected: Some(0), ..PowerInput::default() }, true, &mut energy, &t);
    assert_eq!(s.step(&kit, press(0), true, &mut energy, &t).energy_spent, 22.0);
}

#[test]
fn cone_membership_widens_near_the_caster_and_respects_the_far_edge() {
    let cone = match def(SHOVE).effect {
        Effect::Cone(c) => c,
        _ => unreachable!(),
    };
    let origin = [0.0, 1.0, 0.0];
    let axis = [0.0, 0.0, 1.0];
    assert!(geometry::in_cone(&cone, origin, axis, [0.0, 1.0, 11.9]).is_some());
    assert!(geometry::in_cone(&cone, origin, axis, [0.0, 1.0, 12.0]).is_none(), "the length is exclusive");
    // 60 degrees off the axis: outside a 90 degree cone at range, inside it close by.
    let (s, c) = (v8math::sin(1.05), v8math::cos(1.05));
    assert!(geometry::in_cone(&cone, origin, axis, [6.0 * s, 1.0, 6.0 * c]).is_none());
    assert!(geometry::in_cone(&cone, origin, axis, [2.0 * s, 1.0, 2.0 * c]).is_some());
    assert!(geometry::in_cone(&cone, origin, axis, [0.0, 1.0, -1.0]).is_none(), "nothing behind");
}

#[test]
fn an_aimed_ray_is_a_wedge_of_the_area_radius_with_a_slab_of_the_area_height() {
    let area = match def(GRIP).effect {
        Effect::Area(a) => a,
        _ => unreachable!(),
    };
    let frame = AimFrame::from_angles(0.0, 0.0);
    let o = [0.0, 1.0, 0.0];
    let ahead = geometry::in_ray(&area, o, &frame, [0.0, 1.0, 7.0], 0.4).unwrap();
    assert!((ahead - 49.0 / 196.0).abs() < 1e-12);
    assert!(geometry::in_ray(&area, o, &frame, [0.0, 1.0, 14.3], 0.4).is_some(), "the body radius extends the reach");
    assert!(geometry::in_ray(&area, o, &frame, [0.0, 1.0, 14.5], 0.4).is_none());
    assert!(geometry::in_ray(&area, o, &frame, [3.0, 1.0, 7.0], 0.4).is_none(), "outside the 8 degree wedge");
    assert!(geometry::in_ray(&area, o, &frame, [0.0, 3.0, 7.0], 0.4).is_none(), "above the 3 m slab");
    let left = AimFrame::from_angles(-std::f64::consts::FRAC_PI_2, 0.0);
    assert!(geometry::in_ray(&area, o, &left, [-7.0, 1.0, 0.0], 0.4).is_some());
}

#[test]
fn a_push_spreads_from_the_caster_and_a_pull_draws_far_targets_harder() {
    let shove = match def(SHOVE).effect {
        Effect::Cone(c) => c,
        _ => unreachable!(),
    };
    let tug = match def(TUG).effect {
        Effect::Cone(c) => c,
        _ => unreachable!(),
    };
    let caster = [0.0, 1.0, 0.0];
    let near = [0.0, 1.0, 4.0];
    let source = geometry::cone_push(&shove, caster, near).unwrap();
    assert_eq!((source.inner, source.outer, source.strength), (6.0, 24.0, 9.0));
    let (v, strength) = geometry::push_velocity(&source, near, 1.0).unwrap();
    assert_eq!(strength, 9.0, "full strength inside half the cone length");
    assert!((v[1] - 0.6 * 9.0).abs() < 1e-12 && v[2] > 8.9, "away and lifted");
    let (_, far) = geometry::push_velocity(&source, [0.0, 1.0, 15.5], 1.0).unwrap();
    assert!((far - 9.0 * (24.0 - 15.0) / 18.0).abs() < 1e-9);

    let pull_near = geometry::cone_push(&tug, caster, near).unwrap();
    let (toward, weak) = geometry::push_velocity(&pull_near, near, 0.0).unwrap();
    assert!(toward[2] < 0.0, "a pull draws the target toward the caster");
    let far_point = [0.0, 1.0, 25.0];
    let pull_far = geometry::cone_push(&tug, caster, far_point).unwrap();
    let (_, strong) = geometry::push_velocity(&pull_far, far_point, 0.0).unwrap();
    assert!((weak - 20.0 * 4.0 / 33.0).abs() < 1e-9 && (strong - 20.0 * 25.0 / 33.0).abs() < 1e-9);
}

#[test]
fn the_reaction_to_a_push_depends_on_footing_and_strength() {
    let up = [0.0, 1.0, 0.0];
    assert_eq!(geometry::reaction(Footing::Ground, [4.0, 2.4, 0.0], 4.0, up), Some(Reaction::Stagger));
    assert_eq!(geometry::reaction(Footing::Ground, [5.0, 3.1, 0.0], 5.0, up), Some(Reaction::Thrown));
    assert_eq!(geometry::reaction(Footing::Ground, [8.2, 0.0, 0.0], 1.0, up), Some(Reaction::Tumble), "fast flat push tumbles on the ground");
    assert_eq!(geometry::reaction(Footing::Ground, [1.0, 0.6, 0.0], 1.0, up), Some(Reaction::Moved));
    assert_eq!(geometry::reaction(Footing::Rolling, [5.0, 3.1, 0.0], 5.0, up), Some(Reaction::Thrown), "a roll does not protect");
    assert_eq!(geometry::reaction(Footing::Air, [4.0, 2.4, 0.0], 4.0, up), Some(Reaction::Stagger));
    assert_eq!(geometry::reaction(Footing::Air, [6.0, 3.6, 0.0], 6.0, up), Some(Reaction::Thrown));
    assert_eq!(geometry::reaction(Footing::Jet, [6.0, 3.6, 0.0], 6.0, up), Some(Reaction::Stagger));
    assert_eq!(geometry::reaction(Footing::Thrown, [9.0, 9.0, 0.0], 9.0, up), None);
    assert_eq!(geometry::reaction(Footing::Seated, [9.0, 9.0, 0.0], 9.0, up), None);
}

fn key(slot: u32, life: f64) -> LifeKey {
    LifeKey { slot, generation: 1, life }
}

#[test]
fn modifiers_refresh_only_when_longer_scale_damage_and_lapse() {
    let mut table = EffectTable::with_slots(4);
    let a = key(1, 3.0);
    let b = key(2, 5.0);
    let long = super::def::Buff { seconds: 2.0, value: 1.4 };
    let short = super::def::Buff { seconds: 1.0, value: 3.0 };
    assert!(table.give(a, Some(long), None, None));
    assert!(!table.give(a, Some(short), None, None), "a shorter modifier does not replace a longer one");
    assert!(table.give(b, None, Some(super::def::Buff { seconds: 1.0, value: 0.25 }), Some(super::def::Buff { seconds: 0.5, value: 12.0 })));
    assert_eq!(table.damage_scale(Some(a), Some(b)), 1.4 * 0.25);
    assert_eq!(table.damage_scale(Some(key(1, 4.0)), Some(b)), 0.25, "a new life keeps nothing");
    let mut healed = Vec::new();
    for _ in 0..60 {
        table.tick(1.0 / 60.0, |life, rate| healed.push((life.slot, rate)));
    }
    assert!((29..=30).contains(&healed.len()), "regeneration while its timer runs: {}", healed.len());
    assert_eq!(table.damage_scale(Some(a), Some(b)), 1.4);
    for _ in 0..61 {
        table.tick(1.0 / 60.0, |_, _| {});
    }
    assert_eq!(table.damage_scale(Some(a), Some(b)), 1.0);
    table.hold(b, a, [1.0, 0.0, 0.0], 0);
    for _ in 0..HOLD_RELEASE_TICKS - 1 {
        table.tick(1.0 / 60.0, |_, _| {});
    }
    assert!(table.held(b).is_some());
    table.tick(1.0 / 60.0, |_, _| {});
    assert!(table.held(b).is_none());
}

#[test]
fn a_lock_takes_the_best_aligned_enemy_in_range_and_lapses_out_of_sight() {
    let near = LockCandidate { life: key(1, 1.0), distance: 6.0, alignment: 0.95 };
    let aligned = LockCandidate { life: key(2, 1.0), distance: 20.0, alignment: 0.99 };
    let wide = LockCandidate { life: key(3, 1.0), distance: 3.0, alignment: 0.5 };
    let far = LockCandidate { life: key(4, 1.0), distance: 50.0, alignment: 1.0 };
    let chosen = LockState::choose(40.0, [near, aligned, wide, far].into_iter());
    assert_eq!(chosen.map(|c| c.life.slot), Some(2));
    let mut lock = LockState::default();
    lock.take(key(2, 1.0), 40.0);
    assert!(lock.keep(45.0, true));
    assert!(!lock.keep(51.0, true), "past the keep range");
    lock.take(key(2, 1.0), 40.0);
    for _ in 0..PROVISIONAL_LOCK_UNSEEN_TICKS {
        assert!(lock.keep(10.0, false));
    }
    assert!(!lock.keep(10.0, false));
}

#[test]
fn nearest_lists_keep_order_capacity_and_insertion_ties() {
    let mut n: Nearest<u8, 3> = Nearest::default();
    for (k, v) in [(5.0, 1), (2.0, 2), (5.0, 3), (9.0, 4), (1.0, 5)] {
        n.insert(k, v);
    }
    assert_eq!(n.iter().map(|(_, v)| v).collect::<Vec<_>>(), vec![5, 2, 1]);
}

mod live {
    //! The module against a real world store with invented soldiers.
    use super::*;
    use crate::combat::powers::world::{self, Forward, NoHost, PowerEventKind};
    use crate::gen::{Air, Member, Unit, UnitBody};
    use crate::units::f3;

    struct Arena {
        w: Box<crate::world::owner::World>,
        caster: LifeKey,
        others: Vec<LifeKey>,
    }

    fn soldier(team: f64, at: [f64; 3], yaw: f64) -> Unit {
        let mut u = Unit { generation: 1.0, team, alive: true, ..Default::default() };
        u.combat.health = 1000.0;
        u.combat.max_health = 1000.0;
        if let UnitBody::Soldier(s) = &mut u.soldier {
            s.position = at;
            s.aim_yaw = yaw;
            s.normal = [0.0, 1.0, 0.0];
            s.grounded = true;
        }
        u
    }

    /// A world of the 2005 game, whose damage path applies the timed modifiers.
    fn world_2005() -> Box<crate::world::owner::World> {
        let mut w = crate::world::owner::World::new();
        w.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        w
    }

    fn arena(class: &str, caster: Unit, others: Vec<Unit>) -> Arena {
        let mut w = world_2005();
        let slots = (others.len() + 2).max(8);
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        f3::reserve_native_slots_in(&mut w, slots);
        let build = |w: &mut crate::world::owner::World, u: Unit| {
            let k = LifeKey::from(&f3::with_owner_rows(w, |store, owner| owner.build(store, u)));
            if !w.sim.store.units.list.contains(&Member::Unit(k.reference())) {
                w.sim.store.units.list.push(Member::Unit(k.reference()));
            }
            k
        };
        let caster = build(&mut w, caster);
        let others = others.into_iter().map(|u| build(&mut w, u)).collect();
        let unit = record(class);
        world::install_in(&mut w, slots, [("inv_hero", &unit)], |name| (name == "inv_slam").then(|| record(SLAM))).unwrap();
        Arena { w, caster, others }
    }

    fn velocity(w: &crate::world::owner::World, k: LifeKey) -> ([f64; 3], Option<Air>) {
        match &k.unit(&w.sim.store).unwrap().soldier {
            UnitBody::Soldier(s) => (s.velocity, s.air),
            _ => unreachable!(),
        }
    }

    fn cast(a: &mut Arena, selected: u8, ticks: u32, energy: &mut State, t: &Tuning) {
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        for tick in 0..ticks {
            let input = Forward { power: PowerInput { selected: Some(selected), pressed: tick == 0, held: true }, ..Forward::default() };
            world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, input, energy, t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
    }

    fn hero(weapons: &[&str]) -> String {
        format!(r#"{{"class":"inv_hero","weapons":[{}]}}"#, weapons.join(","))
    }

    #[test]
    fn a_push_throws_enemies_in_its_cone_and_leaves_friends_and_bodies_behind_it() {
        let class = hero(&[SHOVE]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(2.0, [0.0, 0.0, 4.0], 0.0),
            soldier(1.0, [1.0, 0.0, 5.0], 0.0),
            soldier(2.0, [0.0, 0.0, -4.0], 0.0),
        ]);
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        cast(&mut a, 0, 20, &mut energy, &t);
        let (v, air) = velocity(&a.w, a.others[0]);
        assert!(v[2] > 5.0 && v[1] > 3.0 && air == Some(Air::Thrown), "thrown away from the caster: {v:?} {air:?}");
        assert_eq!(velocity(&a.w, a.others[1]).0, [0.0, 0.0, 0.0], "this push does not affect friends");
        assert_eq!(velocity(&a.w, a.others[2]).0, [0.0, 0.0, 0.0], "behind the caster");
        assert_eq!(energy.energy, 70.0);
        let log = world::powers_in(&a.w).unwrap().log();
        let kinds: Vec<_> = log.since(0).map(|e| e.kind).collect();
        assert_eq!(kinds, vec![world::PowerEventKind::Cast, world::PowerEventKind::Thrown]);
    }

    #[test]
    fn a_choke_holds_and_hurts_the_target_ahead_until_released() {
        let class = hero(&[GRIP]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 6.0], 0.0)]);
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        cast(&mut a, 0, 9 + 60, &mut energy, &t);
        let victim = a.others[0];
        assert!(world::held_in(&a.w, victim).is_some_and(|h| h.by == a.caster));
        let health = victim.unit(&a.w.sim.store).unwrap().combat.health;
        assert!((health - 910.0).abs() < 1e-6, "90 per second over the 60 held ticks: {health}");
        assert_eq!(world::powers_in(&a.w).unwrap().held_targets(a.caster), vec![victim]);
        assert_eq!(energy.energy, 78.0, "the movement step charges the held rate");
        // Released: the hold lapses after half a second.
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        for _ in 0..=HOLD_RELEASE_TICKS {
            world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, Forward::default(), &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        assert!(world::held_in(&a.w, victim).is_none());
    }

    #[test]
    fn a_choked_soldier_stays_held_while_the_wheel_brings_a_cone_that_strikes_it_too() {
        let class = hero(&[STORM, GRIP]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 6.0], 0.0)]);
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let victim = a.others[0];
        for tick in 0..140u32 {
            let selected = if tick < 30 { 1 } else { 0 };
            let input = Forward { power: PowerInput { selected: Some(selected), pressed: tick == 0, held: true }, ..Forward::default() };
            world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
            if tick >= 9 {
                assert!(world::held_in(&a.w, victim).is_some_and(|h| h.by == a.caster), "held on tick {tick}");
            }
        }
        let storm = def(STORM).weapon;
        let log = world::powers_in(&a.w).unwrap().log();
        let struck: Vec<_> = log.since(0).filter(|e| e.weapon == storm && e.kind == world::PowerEventKind::Hit).map(|e| e.target).collect();
        assert_eq!(struck, vec![Some(victim); 4], "four cone casts at 48, 78, 108 and 138 strike the held soldier");
        let lost = 1000.0 - health(&a.w, victim);
        assert!((lost - (131.0 * 1.5 + 4.0 * 40.0)).abs() < 1e-6, "the hold's 90 a second and every cast: {lost}");
    }

    #[test]
    fn the_hold_pose_comes_back_once_a_cone_ends_after_the_wheel_returns_to_the_held_area() {
        // Choke held, the wheel to lightning (its casts show), the wheel back to the
        // choke: once the lightning is done the caster shows the hold again, while the
        // victim is still held.
        let class = hero(&[STORM, GRIP]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 6.0], 0.0)]);
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let victim = a.others[0];
        let (storm, grip) = (def(STORM).weapon, def(GRIP).weapon);
        let mut back_at = None;
        for tick in 0..160u32 {
            let selected = if (30..90).contains(&tick) { 0 } else { 1 };
            let input = Forward { power: PowerInput { selected: Some(selected), pressed: tick == 0, held: true }, ..Forward::default() };
            world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
            let powers = world::powers_in(&a.w).unwrap();
            let pose = powers.cast_pose(a.caster).map(|(weapon, _, _, holding)| (weapon, holding));
            assert!(world::held_in(&a.w, victim).is_some_and(|h| h.by == a.caster) || tick < 9, "held on tick {tick}");
            if (50..90).contains(&tick) {
                assert_eq!(pose.map(|p| p.0), Some(storm), "the selected cone's pose on tick {tick}");
            }
            if tick >= 90 && back_at.is_none() && pose == Some((grip, true)) {
                back_at = Some(tick);
            }
            if let Some(at) = back_at {
                assert_eq!(pose, Some((grip, true)), "the hold pose stays from tick {at} on, tick {tick}");
            }
        }
        let at = back_at.expect("the hold pose never came back");
        assert!(at < 90 + 30, "back within a cone's recovery: tick {at}");
    }

    /// A host that reports one defender guarding and one soldier rolling.
    struct Host {
        guard: Option<(LifeKey, crate::combat::combo::DeflectDef)>,
        rolling: Option<LifeKey>,
        thrown: Vec<LifeKey>,
    }

    impl world::PowerHost for Host {
        fn footing(&mut self, _w: &crate::world::owner::World, life: LifeKey) -> Option<Footing> {
            (self.rolling == Some(life)).then_some(Footing::Rolling)
        }
        fn guard(&mut self, _w: &crate::world::owner::World, life: LifeKey) -> Option<crate::combat::combo::DeflectDef> {
            self.guard.filter(|(k, _)| *k == life).map(|(_, d)| d)
        }
        fn thrown(&mut self, _w: &mut crate::world::owner::World, life: LifeKey) {
            self.thrown.push(life);
        }
    }

    fn front_guard() -> crate::combat::combo::DeflectDef {
        crate::combat::combo::DeflectDef {
            min_angle_deg: -50.0,
            max_angle_deg: 50.0,
            energy_cost: 0.0,
            deflect_anim_start: 0,
            deflect_anim_len: 0,
            reaction_ids: [u16::MAX; 6],
        }
    }

    fn run(a: &mut Arena, host: &mut Host, presses: &[u32], ticks: u32, energy: &mut State, t: &Tuning, bot: bool) {
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        for tick in 0..ticks {
            let power = PowerInput { selected: Some(0), pressed: presses.contains(&tick), held: true };
            let input = Forward { power, bot, ..Forward::default() };
            world::forward_in(&mut a.w, host, a.caster.reference(), kit, input, energy, t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
    }

    #[test]
    fn a_guard_facing_the_caster_stops_a_push_and_one_facing_away_does_not() {
        let class = hero(&[SHOVE]);
        // One target faces the caster, the other has its back turned.
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(2.0, [-1.0, 0.0, 5.0], std::f64::consts::PI),
            soldier(2.0, [1.0, 0.0, 5.0], 0.0),
        ]);
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        let second = a.others[1];
        let mut host = Host { guard: Some((second, front_guard())), rolling: None, thrown: vec![] };
        run(&mut a, &mut host, &[0], 20, &mut energy, &t, false);
        let kinds: Vec<_> = world::powers_in(&a.w).unwrap().log().since(0).map(|e| (e.kind, e.target.map(|k| k.slot))).collect();
        assert!(kinds.contains(&(world::PowerEventKind::Thrown, Some(a.others[0].slot))), "no guard: thrown {kinds:?}");
        assert!(kinds.contains(&(world::PowerEventKind::Thrown, Some(second.slot))), "a guard behind does not face the push {kinds:?}");
        let mut b = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 5.0], std::f64::consts::PI)]);
        let mut energy = State::new(&t);
        let mut guarded = Host { guard: Some((b.others[0], front_guard())), rolling: None, thrown: vec![] };
        run(&mut b, &mut guarded, &[0], 20, &mut energy, &t, false);
        assert_eq!(velocity(&b.w, b.others[0]).0, [0.0, 0.0, 0.0], "the guard stopped it");
        let blocked = world::powers_in(&b.w).unwrap().log().since(0).any(|e| e.kind == world::PowerEventKind::Blocked);
        assert!(blocked && energy.energy == 70.0, "the caster still paid");
    }

    #[test]
    fn a_roll_is_pushed_but_not_choked_and_a_seated_soldier_is_never_pushed() {
        let t = tuning(100.0);
        let grip = hero(&[GRIP]);
        let mut a = arena(&grip, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 6.0], 0.0)]);
        let mut energy = State::new(&t);
        let mut host = Host { guard: None, rolling: Some(a.others[0]), thrown: vec![] };
        run(&mut a, &mut host, &[0], 40, &mut energy, &t, false);
        assert!(world::held_in(&a.w, a.others[0]).is_none(), "a rolling target is hurt but not held");
        assert!(a.others[0].unit(&a.w.sim.store).unwrap().combat.health < 1000.0);

        let shove = hero(&[SHOVE]);
        let mut b = arena(&shove, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 4.0], 0.0)]);
        let mut energy = State::new(&t);
        let mut host = Host { guard: None, rolling: Some(b.others[0]), thrown: vec![] };
        run(&mut b, &mut host, &[0], 20, &mut energy, &t, false);
        assert_eq!(velocity(&b.w, b.others[0]).1, Some(Air::Thrown));
        assert_eq!(host.thrown, vec![b.others[0]], "the movement is told to end the roll");

        let mut c = arena(&shove, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 4.0], 0.0)]);
        c.others[0].unit_mut(&mut c.w.sim.store).unwrap().held = crate::store::Nullable::Value(Default::default());
        let mut energy = State::new(&t);
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };
        run(&mut c, &mut host, &[0], 20, &mut energy, &t, false);
        assert_eq!(velocity(&c.w, c.others[0]).0, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn bots_skip_zero_damage_cones_and_area_powers_and_cast_damaging_cones_from_trigger() {
        let t = tuning(100.0);
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };

        // Zero-damage push, pull, and area choke are never cast by bots even with trigger input.
        let non_damaging = hero(&[SHOVE, TUG, GRIP]);
        let mut a = arena(&non_damaging, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 6.0], 0.0)]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let mut energy = State::new(&t);
        for tick in 0..60u32 {
            let selected = (tick / 20) as u8;
            let input = Forward { power: PowerInput { selected: Some(selected), pressed: tick % 20 == 0, held: true }, lock_pressed: false, bot: true };
            world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        let casts = world::powers_in(&a.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast).count();
        assert_eq!((casts, energy.energy), (0, 100.0));

        // A damaging cone is cast only when driven by the bot's weapon trigger, and resumes when held after recovery.
        let storm = hero(&[STORM]);
        let mut idle_bot = arena(&storm, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 5.0], 0.0)]);
        let kit = world::powers_in(&idle_bot.w).unwrap().kit_of("inv_hero").unwrap();
        let mut energy = State::new(&t);
        for _ in 0..60u32 {
            let input = Forward { power: PowerInput::default(), lock_pressed: false, bot: true };
            world::forward_in(&mut idle_bot.w, &mut host, idle_bot.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut idle_bot.w, 1.0 / 60.0);
        }
        assert_eq!(energy.energy, 100.0, "no autonomous cast without trigger");

        let mut triggered = arena(&storm, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 5.0], 0.0)]);
        let kit = world::powers_in(&triggered.w).unwrap().kit_of("inv_hero").unwrap();
        let mut energy = State::new(&t);
        energy.energy = 12.0;
        for tick in 0..120u32 {
            let input = Forward { power: PowerInput { selected: Some(0), pressed: tick == 0, held: true }, lock_pressed: false, bot: true };
            world::forward_in(&mut triggered.w, &mut host, triggered.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut triggered.w, 1.0 / 60.0);
            energy.recover(25.0 / 60.0, &t);
        }
        let casts = world::powers_in(&triggered.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast && e.bot).count();
        assert!(casts >= 2, "damaging cone casts from trigger and resumes while held after energy recovery: {casts}");
    }

    #[test]
    fn a_bot_whose_trigger_selects_another_power_while_a_cone_winds_up_starts_nothing() {
        let t = tuning(100.0);
        let class = hero(&[STORM, GRIP]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 5.0], 0.0)]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let mut energy = State::new(&t);
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };
        for tick in 0..60u32 {
            // The weapon AI holds the trigger on the cone, then its selection moves to the area power.
            let selected = if tick < 5 { 0 } else { 1 };
            let input = Forward { power: PowerInput { selected: Some(selected), pressed: tick == 0 || tick == 5, held: true }, lock_pressed: false, bot: true };
            world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        let p = world::powers_in(&a.w).unwrap();
        let grip = def(GRIP).weapon;
        assert!(p.log().since(0).all(|e| e.weapon != grip), "no area power started by a bot");
        assert!(p.state(a.caster).unwrap().active().is_none());
        assert_eq!(energy.energy, 100.0, "nothing was paid");
        assert!(world::held_in(&a.w, a.others[0]).is_none());
    }

    #[test]
    fn a_bot_neither_starts_nor_keeps_a_damaging_cone_that_reaches_no_enemy() {
        let t = tuning(100.0);
        let storm = hero(&[STORM]);
        let place = |a: &mut Arena, k: LifeKey, at: [f64; 3]| {
            if let Some(UnitBody::Soldier(s)) = k.unit_mut(&mut a.w.sim.store).map(|u| &mut u.soldier) {
                s.position = at;
            }
        };
        // Hold the trigger for 140 ticks while the enemy stands at `path(tick)`; the ticks of the casts.
        let run = |bot: bool, path: &dyn Fn(u32) -> [f64; 3]| {
            let mut a = arena(&storm, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, path(0), 0.0)]);
            let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
            let mut energy = State::new(&t);
            let mut host = Host { guard: None, rolling: None, thrown: vec![] };
            let mut casts = Vec::new();
            let mut cursor = 0;
            for tick in 0..140u32 {
                let enemy = a.others[0];
                place(&mut a, enemy, path(tick));
                let input = Forward { power: PowerInput { selected: Some(0), pressed: tick == 0, held: true }, lock_pressed: false, bot };
                world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, input, &mut energy, &t);
                world::finish_tick_in(&mut a.w, 1.0 / 60.0);
                let log = world::powers_in(&a.w).unwrap().log();
                casts.extend(log.since(cursor).filter(|e| e.kind == world::PowerEventKind::Cast).map(|_| tick));
                cursor = log.latest;
            }
            casts
        };
        let far = |_: u32| [0.0, 0.0, 20.0];
        let behind = |_: u32| [0.0, 0.0, -5.0];
        assert_eq!(run(true, &far), Vec::<u32>::new(), "an enemy beyond the reach");
        assert_eq!(run(true, &behind), Vec::<u32>::new(), "an enemy behind the caster");
        assert_eq!(run(false, &far), vec![18, 48, 78, 108, 138], "a player casts wherever the trigger says");
        // The enemy steps into reach at tick 30 and out again at tick 70: the casts begun
        // while it was there land, and the hold ends.
        let visit = |tick: u32| if (30..70).contains(&tick) { [0.0, 0.0, 6.0] } else { [0.0, 0.0, 20.0] };
        assert_eq!(run(true, &visit), vec![48, 78]);
    }

    #[test]
    fn bots_cast_an_aura_only_in_combat_with_at_least_three_friendly_soldiers_in_range() {
        let t = tuning(100.0);
        let rally = hero(&[RALLY]);
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };

        // Solo with enemy nearby: 1 friendly total (< 3) -> no cast.
        let mut solo = arena(&rally, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 5.0], 0.0)]);
        let mut energy = State::new(&t);
        run(&mut solo, &mut host, &[], 40, &mut energy, &t, true);
        assert_eq!(world::powers_in(&solo.w).unwrap().log().since(0).count(), 0);

        // One ally in range + self = 2 (< 3) -> no cast.
        let mut pair = arena(&rally, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [2.0, 0.0, 0.0], 0.0),
            soldier(2.0, [0.0, 0.0, 5.0], 0.0),
        ]);
        let mut energy = State::new(&t);
        run(&mut pair, &mut host, &[], 40, &mut energy, &t, true);
        assert_eq!(world::powers_in(&pair.w).unwrap().log().since(0).count(), 0);

        // Two allies in range + self = 3, but no enemy threat -> no cast.
        let mut peaceful = arena(&rally, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [2.0, 0.0, 0.0], 0.0),
            soldier(1.0, [-2.0, 0.0, 0.0], 0.0),
        ]);
        let mut energy = State::new(&t);
        run(&mut peaceful, &mut host, &[], 40, &mut energy, &t, true);
        assert_eq!(world::powers_in(&peaceful.w).unwrap().log().since(0).count(), 0);

        // Two allies in range + self = 3 with enemy threat -> casts and buffs.
        let mut squad = arena(&rally, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [2.0, 0.0, 0.0], 0.0),
            soldier(1.0, [-2.0, 0.0, 0.0], 0.0),
            soldier(2.0, [0.0, 0.0, 5.0], 0.0),
        ]);
        let mut energy = State::new(&t);
        run(&mut squad, &mut host, &[], 40, &mut energy, &t, true);
        let casts = world::powers_in(&squad.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast && e.bot).count();
        assert_eq!(casts, 1);
    }

    #[test]
    fn human_held_repeating_emitter_starts_from_idle_and_resumes_after_exhaustion() {
        let t = tuning(100.0);
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };

        // A single-trigger emitter (TriggerSingle 1) reads the trigger while it is down, as every
        // weapon on the channel does, and spends it as it starts: one cast, then nothing more
        // until the trigger is released and pressed again.
        let shove = hero(&[SHOVE]);
        let mut single = arena(&shove, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 5.0], 0.0)]);
        let kit = world::powers_in(&single.w).unwrap().kit_of("inv_hero").unwrap();
        let mut energy = State::new(&t);
        for _ in 0..30 {
            let input = Forward { power: PowerInput { selected: Some(0), pressed: false, held: true }, lock_pressed: false, bot: false };
            world::forward_in(&mut single.w, &mut host, single.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut single.w, 1.0 / 60.0);
        }
        let single_casts = world::powers_in(&single.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast).count();
        assert_eq!(single_casts, 1, "TriggerSingle=1 power casts once per press of the trigger");

        // A repeating emitter (TriggerSingle 0, Force lightning) starts from idle while held for a
        // human player, pauses when stamina exhausts, and resumes automatically once recovered.
        let storm = hero(&[STORM]);
        let mut a = arena(&storm, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 5.0], 0.0)]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let mut energy = State::new(&t);
        energy.energy = 10.0;
        let held_only = Forward { power: PowerInput { selected: Some(0), pressed: false, held: true }, lock_pressed: false, bot: false };
        // Cast 1 starts from idle with pressed=false, held=true and overdraws stamina into exhaustion.
        for _ in 0..25 {
            world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, held_only, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        let after_first = world::powers_in(&a.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast && !e.bot).count();
        assert_eq!(after_first, 1, "human holding TriggerSingle=0 should start casting from idle");
        assert!(energy.exhausted, "overdraw should latch exhaustion");

        // While exhausted with no recovery, holding cannot cast again.
        for _ in 0..40 {
            world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, held_only, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        let while_exhausted = world::powers_in(&a.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast && !e.bot).count();
        assert_eq!(while_exhausted, 1, "exhausted human cannot cast while latched");

        // Once stamina recovers above EnergyMinSprint, continuing to hold resumes casting without a fresh press.
        for _ in 0..90 {
            energy.recover(25.0 / 60.0, &t);
            world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, held_only, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        let after_recovery = world::powers_in(&a.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast && !e.bot).count();
        assert!(after_recovery >= 2, "human holding TriggerSingle=0 should resume after exhaustion clears, got {after_recovery}");
    }

    #[test]
    fn bot_aura_recheck_waits_full_cadence_before_polling_again() {
        let t = tuning(100.0);
        let rally = hero(&[RALLY]);
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };
        // Start with two nearby allies and an enemy outside threat range so tick 0 fails its check
        // and arms the full BOT_AURA_CHECK_TICKS cooldown.
        let mut a = arena(&rally, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [2.0, 0.0, 0.0], 0.0),
            soldier(1.0, [-2.0, 0.0, 0.0], 0.0),
            soldier(2.0, [0.0, 0.0, 40.0], 0.0),
        ]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let mut energy = State::new(&t);
        let input = Forward { power: PowerInput::default(), lock_pressed: false, bot: true };
        world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, input, &mut energy, &t);
        world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        assert!(world::powers_in(&a.w).unwrap().state(a.caster).unwrap().active().is_none());

        // Move the enemy into threat range on tick 1; the bot must not poll again for 12 ticks.
        if let UnitBody::Soldier(s) = &mut a.others[2].unit_mut(&mut a.w.sim.store).unwrap().soldier {
            s.position = [0.0, 0.0, 5.0];
        }
        for tick in 1..=12u32 {
            world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
            assert!(
                world::powers_in(&a.w).unwrap().state(a.caster).unwrap().active().is_none(),
                "bot must not recheck aura before the 12-tick cadence elapses (tick {tick})"
            );
        }
        // Tick 13 begins the aura windup and subsequent ticks land the cast.
        world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, input, &mut energy, &t);
        world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        assert!(world::powers_in(&a.w).unwrap().state(a.caster).unwrap().active().is_some());
        for _ in 0..10 {
            world::forward_in(&mut a.w, &mut host, a.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        let casts = world::powers_in(&a.w).unwrap().log().since(0).filter(|e| e.kind == world::PowerEventKind::Cast && e.bot).count();
        assert_eq!(casts, 1);
    }

    fn digest_of(w: &crate::world::owner::World, keys: &[LifeKey], energies: &[&State]) -> Wide {
        let mut h = Fnv::new();
        let mut eat = |bits: Wide| h.bytes(&bits.to_le_bytes());
        let p = world::powers_in(w).unwrap();
        let effects = world::effects_in(w);
        for k in keys {
            let u = k.unit(&w.sim.store).unwrap();
            eat(Wide::from_f64_bits(u.combat.health));
            if let UnitBody::Soldier(s) = &u.soldier {
                for v in s.velocity {
                    eat(Wide::from_f64_bits(v));
                }
                eat(Wide::from_u32(s.air.map_or(9, |a| a as u32)));
            }
            if let Some(r) = effects.get(*k) {
                for t in [r.offense, r.defense, r.regen] {
                    eat(Wide::from_f64_bits(t.seconds));
                    eat(Wide::from_f64_bits(t.value));
                }
                eat(Wide::from_u32(r.held.map_or(0, |held| u32::from(held.left))));
            }
            eat(Wide::from_u32(p.lock(*k).map_or(0, |l| l.slot + 1)));
        }
        for e in energies {
            eat(Wide::from_f64_bits(e.energy));
            eat(Wide::from_u32(u32::from(e.exhausted)));
        }
        for e in p.log().since(0) {
            eat(Wide::from_u32(e.sequence)
                .xor(Wide::from_u32(e.kind as u32).shl(40))
                .xor(Wide::from_u32(e.target.map_or(99, |k| k.slot)).shl(48))
                .xor(Wide::from_u32(u32::from(e.weapon))));
        }
        h.finish()
    }

    fn scripted_fight() -> Wide {
        let first = record(&hero(&[SHOVE, TUG, RALLY]).replace("inv_hero", "inv_first"));
        let second = record(&hero(&[STORM, GRIP]).replace("inv_hero", "inv_second"));
        let mut w = world_2005();
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        f3::reserve_native_slots_in(&mut w, 8);
        let mut keys = vec![];
        for u in [
            soldier(1.0, [0.0, 0.0, 0.0], 0.0),
            soldier(2.0, [0.5, 0.0, 9.0], std::f64::consts::PI),
            soldier(2.0, [1.5, 0.0, 4.0], 0.3),
            soldier(2.0, [-2.0, 0.0, 6.0], 2.0),
            soldier(1.0, [2.0, 0.0, 1.0], 0.0),
            soldier(1.0, [-1.0, 0.0, -2.0], 1.0),
        ] {
            let k = LifeKey::from(&f3::with_owner_rows(&mut w, |store, owner| owner.build(store, u)));
            if !w.sim.store.units.list.contains(&Member::Unit(k.reference())) {
                w.sim.store.units.list.push(Member::Unit(k.reference()));
            }
            keys.push(k);
        }
        world::install_in(&mut w, 8, [("inv_first", &first), ("inv_second", &second)], |_| None).unwrap();
        let p = world::powers_in(&w).unwrap();
        let (ka, kb) = (p.kit_of("inv_first").unwrap(), p.kit_of("inv_second").unwrap());
        let t = tuning(100.0);
        let (mut ea, mut eb) = (State::new(&t), State::new(&t));
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };
        for tick in 0..600u32 {
            let a = PowerInput { selected: Some(((tick / 90) % 3) as u8), pressed: tick % 90 == 5, held: tick % 90 < 30 };
            let b = PowerInput { selected: Some(if tick < 300 { 0 } else { 1 }), pressed: tick == 10 || tick == 320, held: (10..200).contains(&tick) || (320..420).contains(&tick) };
            world::forward_in(&mut w, &mut host, keys[0].reference(), ka, Forward { power: a, lock_pressed: tick == 2, bot: false }, &mut ea, &t);
            world::forward_in(&mut w, &mut host, keys[1].reference(), kb, Forward { power: b, ..Forward::default() }, &mut eb, &t);
            world::finish_tick_in(&mut w, 1.0 / 60.0);
            ea.recover(7.0 / 60.0, &t);
            eb.recover(7.0 / 60.0, &t);
        }
        digest_of(&w, &keys, &[&ea, &eb])
    }

    #[test]
    fn a_scripted_fight_of_every_power_replays_to_one_digest() {
        let first = scripted_fight();
        assert_eq!(first, scripted_fight());
        println!("POWERS_REPLAY {first:016x}");
    }

    #[test]
    fn a_blade_blast_falls_off_from_where_the_character_stands_and_spares_it() {
        let class = hero(&[SLAM_BLADE]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(2.0, [2.0, 0.0, 0.0], 0.0),
            soldier(2.0, [0.0, 0.0, 9.0], 0.0),
        ]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let blast = world::powers_in(&a.w).unwrap().kit(kit).unwrap().blast.unwrap();
        assert_eq!((blast.damage, blast.outer, blast.hurt_owner, blast.scales[2]), (300.0, 6.0, false, 2.0));
        world::play_explosion_in(&mut a.w, &mut NoHost, a.caster.reference(), kit);
        let near = a.others[0].unit(&a.w.sim.store).unwrap().combat.health;
        let [middle, radius, _] = crate::combat::combat::blast_body(crate::gen::Posture::Stand);
        let distance = v8math::hypot(2.0, middle) - 0.5 * radius;
        assert!((near - (1000.0 - 300.0 * (6.0 - distance) / 5.0)).abs() < 1e-9, "falloff from the inner to the outer radius: {near}");
        assert_eq!(velocity(&a.w, a.others[0]).1, Some(Air::Thrown));
        assert_eq!(a.others[1].unit(&a.w.sim.store).unwrap().combat.health, 1000.0, "out of reach");
        assert_eq!(a.caster.unit(&a.w.sim.store).unwrap().combat.health, 1000.0, "the character is spared");
        // The same blast with HurtOwner reaches its owner at full strength.
        let unit = record(&class);
        let mut w = crate::world::owner::World::new();
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        f3::reserve_native_slots_in(&mut w, 8);
        let me = LifeKey::from(&f3::with_owner_rows(&mut w, |store, owner| owner.build(store, soldier(1.0, [0.0, 0.0, 0.0], 0.0))));
        w.sim.store.units.list.push(Member::Unit(me.reference()));
        world::install_in(&mut w, 8, [("inv_hero", &unit)], |_| Some(record(&SLAM.replace(r#""HurtOwner":"0""#, r#""HurtOwner":"1""#)))).unwrap();
        let kit = world::powers_in(&w).unwrap().kit_of("inv_hero").unwrap();
        world::play_explosion_in(&mut w, &mut NoHost, me.reference(), kit);
        assert_eq!(me.unit(&w.sim.store).unwrap().combat.health, 700.0);
    }

    #[test]
    fn a_blast_publishes_its_cast_and_throws_with_the_casters_position_once() {
        let class = hero(&[SLAM_BLADE]);
        let mut a = arena(&class, soldier(1.0, [4.0, 1.5, -2.0], 0.0), vec![soldier(2.0, [6.0, 1.5, -2.0], 0.0)]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let weapon = world::powers_in(&a.w).unwrap().kit(kit).unwrap().blast_weapon;
        let mut out = Vec::with_capacity(4);
        world::drain_published_in(&mut a.w, &mut out);
        assert!(out.is_empty(), "nothing before the blast");
        world::play_explosion_in(&mut a.w, &mut NoHost, a.caster.reference(), kit);
        world::drain_published_in(&mut a.w, &mut out);
        let kinds: Vec<PowerEventKind> = out.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![PowerEventKind::Cast, PowerEventKind::Thrown]);
        assert!(out.iter().all(|e| e.weapon == weapon && e.caster == a.caster));
        assert_eq!(out[0].target, None);
        assert_eq!(out[1].target, Some(a.others[0]));
        assert_eq!(out[0].position, Some([4.0, 1.5, -2.0]), "where the caster stands");
        world::drain_published_in(&mut a.w, &mut out);
        assert!(out.is_empty(), "each event is published once");
    }

    #[test]
    fn an_aura_buffs_allies_in_range_and_the_caster_and_scales_their_damage() {
        let class = hero(&[RALLY]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [3.0, 0.0, 0.0], 0.0),
            soldier(1.0, [9.0, 0.0, 0.0], 0.0),
            soldier(2.0, [2.0, 0.0, 0.0], 0.0),
        ]);
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        cast(&mut a, 0, 8, &mut energy, &t);
        let effects = world::effects_in(&a.w);
        let ally = a.others[0];
        assert_eq!(effects.get(ally).map(|r| r.offense.value), Some(1.3));
        assert!(effects.get(a.caster).is_some(), "the caster shares it when it reached someone");
        assert!(effects.get(a.others[1]).is_none(), "out of range");
        assert!(effects.get(a.others[2]).is_none(), "enemies are not buffed");
        let attacker = crate::gen::Attacker::Unit(ally.reference());
        assert_eq!(world::damage_scale_in(&a.w, attacker, a.others[2]), Some(1.3));
        assert_eq!(world::damage_scale_in(&a.w, crate::gen::Attacker::Unit(a.others[2].reference()), ally), None);
    }

    #[test]
    fn a_character_without_powers_locks_with_the_longest_reach_of_its_weapons() {
        let carbine = r#"{"class":"inv_carbine","base":"cannon","slot":1,"channel":0,"LockOnRange":"30","ordnance":{"base":"bolt"}}"#;
        let mut a = arena(&hero(&[carbine]), soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(2.0, [0.5, 0.0, 40.0], 0.0),
            soldier(2.0, [3.0, 0.0, 20.0], 0.0),
        ]);
        let p = world::powers_in(&a.w).unwrap();
        let kit = p.kit_of("inv_hero").expect("a kit for the lock alone");
        assert_eq!(p.kit(kit).unwrap().iter().count(), 0);
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        let press = Forward { lock_pressed: true, ..Forward::default() };
        world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, press, &mut energy, &t);
        assert_eq!(world::powers_in(&a.w).unwrap().lock(a.caster), Some(a.others[1]), "the enemy past 30 m is out of reach");
        world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, press, &mut energy, &t);
        assert_eq!(world::powers_in(&a.w).unwrap().lock(a.caster), None, "a second press lets go");
    }

    fn health(w: &crate::world::owner::World, k: LifeKey) -> f64 {
        k.unit(&w.sim.store).unwrap().combat.health
    }

    /// Hold `selected` of `kit` for `ticks` ticks as `who`, pressing on the opening tick.
    fn press_as(a: &mut Arena, who: LifeKey, kit: u16, selected: u8, ticks: u32, energy: &mut State, t: &Tuning) {
        for tick in 0..ticks {
            let input = Forward { power: PowerInput { selected: Some(selected), pressed: tick == 0, held: true }, ..Forward::default() };
            world::forward_in(&mut a.w, &mut NoHost, who.reference(), kit, input, energy, t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
    }

    /// Let go of every trigger for `ticks` ticks.
    fn idle(a: &mut Arena, kit: u16, ticks: u32, energy: &mut State, t: &Tuning) {
        for _ in 0..ticks {
            world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, Forward::default(), energy, t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
    }

    /// What one cone, 30 held choke ticks and one blade blast take from the
    /// same enemy, with or without the caster's offence aura first.
    fn caster_hits(rallied: bool) -> [f64; 3] {
        let class = hero(&[SLAM_BLADE, STORM, GRIP, RALLY]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [-2.0, 0.0, 0.0], 0.0),
            soldier(2.0, [0.0, 0.0, 4.0], std::f64::consts::PI),
        ]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let t = tuning(400.0);
        let mut energy = State::new(&t);
        let (me, enemy) = (a.caster, a.others[1]);
        if rallied {
            press_as(&mut a, me, kit, 2, 8, &mut energy, &t);
            assert_eq!(world::effects_in(&a.w).get(a.caster).map(|r| r.offense.value), Some(1.3));
            // The next press waits out the aura's recovery.
            idle(&mut a, kit, 30, &mut energy, &t);
        }
        let mut lost = [0.0; 3];
        let before = health(&a.w, enemy);
        press_as(&mut a, me, kit, 0, 19, &mut energy, &t);
        lost[0] = before - health(&a.w, enemy);
        idle(&mut a, kit, 30, &mut energy, &t);
        let before = health(&a.w, enemy);
        press_as(&mut a, me, kit, 1, 9 + 30, &mut energy, &t);
        lost[1] = before - health(&a.w, enemy);
        idle(&mut a, kit, HOLD_RELEASE_TICKS + 2, &mut energy, &t);
        let before = health(&a.w, enemy);
        world::play_explosion_in(&mut a.w, &mut NoHost, a.caster.reference(), kit);
        lost[2] = before - health(&a.w, enemy);
        lost
    }

    #[test]
    fn an_offence_aura_scales_the_damage_of_every_power_its_soldier_casts() {
        let plain = caster_hits(false);
        let rallied = caster_hits(true);
        assert_eq!(plain[0], 40.0, "the uncharged cone");
        assert!(plain.iter().all(|l| *l > 0.0), "each power hurts: {plain:?}");
        for (p, r) in plain.iter().zip(rallied) {
            assert!((r - 1.3 * p).abs() < 1e-9, "offence x1.3 on every power: {plain:?} -> {rallied:?}");
        }
    }

    #[test]
    fn a_defence_aura_scales_the_power_damage_its_soldiers_take() {
        let class = hero(&[STORM, WARD]);
        let struck = |warded: bool| {
            let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
                soldier(2.0, [0.0, 0.0, 4.0], std::f64::consts::PI),
                soldier(2.0, [1.0, 0.0, 5.0], std::f64::consts::PI),
            ]);
            let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
            let t = tuning(100.0);
            let (mut ours, mut theirs) = (State::new(&t), State::new(&t));
            if warded {
                let target = a.others[0];
                press_as(&mut a, target, kit, 1, 8, &mut theirs, &t);
                let effects = world::effects_in(&a.w);
                assert_eq!(effects.get(target).map(|r| r.defense.value), Some(0.25), "the caster shares its ward");
                assert_eq!(effects.get(a.others[1]).map(|r| r.defense.value), Some(0.25));
            }
            let me = a.caster;
            press_as(&mut a, me, kit, 0, 19, &mut ours, &t);
            [health(&a.w, a.others[0]), health(&a.w, a.others[1])]
        };
        assert_eq!(struck(false), [960.0, 960.0]);
        assert_eq!(struck(true), [990.0, 990.0], "defence x0.25 on the cone's 40");
    }

    /// An aura and a choke run while `reserve_at` (if any) grows the tables.
    fn held_fight(reserve_at: Option<u32>) -> Wide {
        let class = hero(&[STORM, GRIP, RALLY]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [-2.0, 0.0, 0.0], 0.0),
            soldier(2.0, [0.0, 0.0, 6.0], std::f64::consts::PI),
        ]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let t = tuning(400.0);
        let mut energy = State::new(&t);
        for tick in 0..180u32 {
            // The aura, its recovery, then a choke held from tick 40 to 130.
            let (selected, pressed, held) = if tick < 8 { (2, tick == 0, true) } else if tick < 40 { (1, false, false) } else { (1, tick == 40, tick < 130) };
            let input = Forward { power: PowerInput { selected: Some(selected), pressed, held }, ..Forward::default() };
            if reserve_at == Some(tick) {
                let enemy = a.others[1];
                let effects = world::effects_in(&a.w).clone();
                let held_before = world::held_in(&a.w, enemy);
                let state = world::powers_in(&a.w).unwrap().state(a.caster).copied();
                assert!(effects.get(a.caster).is_some_and(|r| r.offense.active()), "the aura is running");
                assert!(held_before.is_some(), "the choke is holding");
                world::reserve_in(&mut a.w, 32);
                assert!(world::effects_in(&a.w).slots() >= 32);
                for k in [a.caster, a.others[0], enemy] {
                    assert_eq!(world::effects_in(&a.w).get(k), effects.get(k), "every row as it was");
                }
                assert_eq!(world::held_in(&a.w, enemy), held_before);
                assert_eq!(world::powers_in(&a.w).unwrap().state(a.caster).copied(), state);
            }
            world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        let mut keys = vec![a.caster];
        keys.extend(a.others.iter().copied());
        digest_of(&a.w, &keys, &[&energy])
    }

    #[test]
    fn a_reserve_keeps_a_running_aura_and_hold_and_the_fight_replays_unchanged() {
        assert_eq!(held_fight(Some(70)), held_fight(None));
    }

    #[test]
    fn caster_animations_track_instant_windup_choke_hold_and_repeating_casts() {
        const INSTANT_STORM: &str = r#"{"class":"inv_instant_storm","base":"cannon","slot":2,"channel":1,
         "TriggerSingle":"0","FireAnim":"0","InstantPlayFireAnim":"1",
         "InitialSalvoDelay":"0.1","ShotDelay":"0.05","EnergyDrain":"10",
         "ordnance":{"base":"emitterordnance","ConeLength":"12","ConeAngle":"60","MaxTargets":"3",
         "NoChaining":"1","Damage":"10"}}"#;
        const INSTANT_GRIP: &str = r#"{"class":"inv_instant_grip","base":"areaeffectweapon","slot":3,"channel":1,
         "FireType":"Hold","EffectType":"Choke","AreaType":"Ray","AreaRange":"14","AreaRadius":"8","AreaHeight":"3",
         "MaxTargets":"1","EffectDamage":"10","EffectStrength":"4","ThrustFactor":"0.7",
         "EnergyDrain":"10","EnergyDrainRate":"5","InitialSalvoDelay":"0.1",
         "FireAnim":"0","InstantPlayFireAnim":"1","SoldierAnimation":"choking"}"#;
        const INSTANT_WARD: &str = r#"{"class":"inv_instant_ward","base":"cannon","slot":4,"channel":1,
         "ammo":2,"RoundsPerClip":"1","FireAnim":"1","InstantPlayFireAnim":"1",
         "InitialSalvoDelay":"0.1","ShotDelay":"0.1",
         "ordnance":{"base":"emitterordnance","Radius":"8","ConeLength":"0","ConeAngle":"45",
         "ApplyOnOwnerIfOnOthers":"1","AffectFriends":"1","AffectEnemies":"0",
         "BuffDefenseTimer":"4","BuffDefenseMult":"0.5"}}"#;
        let class = hero(&[INSTANT_STORM, INSTANT_GRIP, INSTANT_WARD]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(2.0, [0.0, 0.0, 6.0], std::f64::consts::PI),
        ]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let t = tuning(400.0);
        let mut energy = State::new(&t);
        let mut frame = world::PowerFrame::with_capacity(8);
        let step = |a: &mut Arena, energy: &mut State, selected: u8, pressed: bool, held: bool, frame: &mut world::PowerFrame| {
            let input = Forward {
                power: PowerInput { selected: Some(selected), pressed, held },
                ..Forward::default()
            };
            world::forward_in(&mut a.w, &mut NoHost, a.caster.reference(), kit, input, energy, &t);
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
            world::powers_in(&a.w).unwrap().fill_frame(&a.w, frame);
        };
        // Repeating storm: starts on press, holds continuously across two casts without resetting elapsed_ticks.
        step(&mut a, &mut energy, 0, true, true, &mut frame);
        let p0 = *frame.unit(a.caster.slot, a.caster.life).expect("storm anim started on press");
        assert!(p0.holding && p0.fire_anim == 0 && p0.elapsed_ticks == 1);
        for _ in 0..16 {
            step(&mut a, &mut energy, 0, false, true, &mut frame);
        }
        let p1 = *frame.unit(a.caster.slot, a.caster.life).expect("storm anim stays active across repeat");
        assert!(p1.holding && p1.elapsed_ticks == 17, "elapsed should advance monotonically: {p1:?}");
        // Releasing storm stops holding.
        for _ in 0..8 {
            step(&mut a, &mut energy, 0, false, false, &mut frame);
        }
        let p2 = *frame.unit(a.caster.slot, a.caster.life).expect("storm follow-through");
        assert!(!p2.holding);
        // Choke: starts on press, holds victim and caster pose, ends caster pose on release.
        step(&mut a, &mut energy, 1, true, true, &mut frame);
        let c0 = *frame.unit(a.caster.slot, a.caster.life).expect("choke anim started on press");
        assert!(c0.holding && c0.elapsed_ticks == 1);
        for _ in 0..10 {
            step(&mut a, &mut energy, 1, false, true, &mut frame);
        }
        let c1 = *frame.unit(a.caster.slot, a.caster.life).expect("choke holding");
        let victim = *frame.unit(a.others[0].slot, a.others[0].life).expect("victim choked");
        assert!(c1.holding && victim.choked && victim.choke_anim == bfe_content::zero_hash::crc("human_choking"));
        step(&mut a, &mut energy, 1, false, false, &mut frame);
        assert!(frame.unit(a.caster.slot, a.caster.life).is_none(), "choke release clears held caster pose");
        // Finish the one-tick recovery before starting the next power.
        step(&mut a, &mut energy, 2, false, false, &mut frame);
        // Gun-hero offhand buff uses FireAnim=1 and starts on press without holding.
        step(&mut a, &mut energy, 2, true, false, &mut frame);
        let w0 = *frame.unit(a.caster.slot, a.caster.life).expect("ward anim started");
        assert_eq!((w0.fire_anim, w0.holding, w0.elapsed_ticks), (1, false, 1));
    }

    #[test]
    fn finish_tick_damages_all_35_burning_soldiers_without_capping() {
        let class = hero(&[STORM]);
        let victims: Vec<Unit> = (0..35)
            .map(|i| soldier(2.0, [i as f64, 0.0, 4.0], 0.0))
            .collect();
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), victims);
        for &victim in &a.others {
            a.w.combat.power_effects.ignite(victim, a.caster, 60.0, 1.0, None);
        }
        for _ in 0..10 {
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
        }
        for (idx, &victim) in a.others.iter().enumerate() {
            let hp = health(&a.w, victim);
            assert!(
                (hp - 990.0).abs() < 1e-6,
                "burning victim {idx} should lose 10 hp over 10 ticks at 60 dps, got hp={hp}"
            );
        }
    }

    #[test]
    fn a_burn_publishes_its_effect_and_time_left_on_its_life_while_it_burns() {
        let class = hero(&[STORM]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [3.0, 0.0, 0.0], 0.0)]);
        let fx = a.w.sim.strings.intern_str("inv_cinders");
        a.w.combat.power_effects.ignite(a.others[0], a.caster, 30.0, 1.0, Some(fx));
        let (mut out, mut burns) = (Vec::with_capacity(8), Vec::new());
        for _ in 0..70 {
            world::finish_tick_in(&mut a.w, 1.0 / 60.0);
            world::drain_published_in(&mut a.w, &mut out);
            burns.extend(out.iter().map(|e| (e.caster, e.target, e.burn)));
        }
        let lefts: Vec<f64> = burns.iter().map(|b| b.2.unwrap().1).collect();
        assert_eq!(burns.len(), 4, "one each quarter of a second of its one-second burn: {lefts:?}");
        assert!(burns.iter().all(|b| b.0 == a.caster && b.1 == Some(a.others[0]) && b.2.unwrap().0 == fx));
        assert!((lefts[0] - (1.0 - 1.0 / 60.0)).abs() < 1e-9 && lefts.windows(2).all(|w| w[1] < w[0]), "{lefts:?}");
    }

    #[test]
    fn aura_play_effect_on_owner_emits_only_at_caster_not_allies() {
        const RALLY_FX: &str = r#"{"class":"inv_power_rally_fx","base":"cannon","slot":4,"channel":1,"ammo":2,"RoundsPerClip":"1",
         "InitialSalvoDelay":"0.1","ShotDelay":"0.5","ordnance":{"base":"emitterordnance","Radius":"7",
         "ConeLength":"0","ConeAngle":"45","ApplyOnOwnerIfOnOthers":"1","AffectFriends":"1","AffectEnemies":"0",
         "PlayEffectOnOwner":"inv_owner_aura_fx","BuffOffenseTimer":"5","BuffOffenseMult":"1.3"}}"#;
        let class = hero(&[RALLY_FX]);
        let mut a = arena(&class, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![
            soldier(1.0, [2.0, 0.0, 0.0], 0.0),
            soldier(1.0, [-2.0, 0.0, 0.0], 0.0),
        ]);
        let kit = world::powers_in(&a.w).unwrap().kit_of("inv_hero").unwrap();
        let t = tuning(100.0);
        let mut energy = State::new(&t);
        let caster = a.caster;
        press_as(&mut a, caster, kit, 0, 8, &mut energy, &t);
        let mut effects = Vec::new();
        f3::drain_swbf2_effects_into(&mut a.w, &mut effects);
        assert_eq!(effects.len(), 1, "PlayEffectOnOwner must emit once at the caster, not per ally");
        assert_eq!(effects[0].position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_powers_tick_saturates_at_the_bound_and_casts_keep_their_age() {
        let t = tuning(100.0);
        let mut host = Host { guard: None, rolling: None, thrown: vec![] };
        let shove = hero(&[SHOVE]);
        let mut a = arena(&shove, soldier(1.0, [0.0, 0.0, 0.0], 0.0), vec![soldier(2.0, [0.0, 0.0, 4.0], 0.0)]);
        a.w.combat.powers.as_mut().unwrap().start_tick_for_test(u32::MAX - 5);
        let mut energy = State::new(&t);
        run(&mut a, &mut host, &[0], 20, &mut energy, &t, false);
        assert_eq!(world::powers_in(&a.w).unwrap().tick_for_test(), u32::MAX, "the tick saturates");
    }
}
