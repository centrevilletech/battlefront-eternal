//! Invented-data tests for 2005 space weapon audio: FireSound, FireLoopSound,
//! FireSoundStop, OverheatSound pitch sweep, ReloadSound, WeaponChangeSound,
//! chaingun WarmUpSound + BarrelSound, hit/explosion/expiry/destruction links,
//! bounded in-flight ordnance loops, distance roll-off with ScaleSoundParameter,
//! and zero heap allocations during steady-state ticks.

use audio_control::engine::{Curve, Entry, Sample, Sound, NONE, SILENT};
use audio_control::layout::{class as c, emitter as e, frame as f};
use audio_control::phase::Phase;
use audio_control::swbf2::vehicles::ChaingunInput;
use audio_control::swbf2::weapons::{SpaceOrdnanceInput, SpaceWeaponInput, MAX_SPACE_ORDNANCE_LOOPS};
use audio_control::swbf2::{zero_hash, SoundParam, Swbf2Class};
use audio_control::{Collision, Hit, Point};
use bfe_family::GameFamily;
use bfe_test_alloc::Counting;

#[global_allocator]
static GLOBAL: Counting = Counting::new();

fn measure_allocs<F: FnOnce()>(f: F) -> u32 {
    bfe_test_alloc::measure_thread(f).1
}

struct NoCollision;
impl Collision for NoCollision {
    fn cast(&mut self, _from: Point, _direction: Point, _reach: f64) -> Option<Hit> {
        None
    }
}

fn plain_sound(sample: usize, min: f64, max: f64, mute: f64, looping: bool) -> Sound {
    Sound {
        kind: 1,
        bus: NONE,
        pitch: 1.0,
        pitch_dev: 0.0,
        gain: 0.9,
        gain_dev: 0.0,
        reverb: 0.0,
        looping,
        pan: 0.0,
        mode3d: true,
        doppler: true,
        bias: 0.0001,
        priority: 0.5,
        curve: Curve { roll_in: 0.0, min, mute, max, roll: 1.0 },
        probability: 1.0,
        interval: 0.0,
        interval_dev: 0.0,
        random_position: false,
        cycle: -2,
        next: f64::NEG_INFINITY,
        stream: NONE,
        entries: vec![Entry { id: sample, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
    }
}

const SND_BEAM_FIRE: usize = 0;
const SND_BEAM_LOOP: usize = 1;
const SND_OVERHEAT: usize = 2;
const SND_RELOAD: usize = 3;
const SND_CHANGE: usize = 4;
const SND_CHAIN_WARM: usize = 5;
const SND_CHAIN_BARREL: usize = 6;
const SND_IMPACT: usize = 7;
const SND_BLAST: usize = 8;
const SND_EXPIRE_BLAST: usize = 9;
const SND_CRAFT_DEATH: usize = 10;
const SND_PROP_DESTRUCT: usize = 11;
const SND_ROCKET_HUM: usize = 12;

const CLS_BEAM_GUN: usize = 0;
const CLS_CHAIN_GUN: usize = 1;
const CLS_ORD_LASER: usize = 2;
const CLS_EXP_HIT: usize = 3;
const CLS_EXP_EXPIRE: usize = 4;
const CLS_CRAFT: usize = 5;
const CLS_PROP: usize = 6;
const CLS_EXP_PROP: usize = 7;
const CLS_ROCKET_GUN: usize = 8;
const CLS_ORD_ROCKET: usize = 9;

fn build_fixture() -> Phase {
    let mut phase = Phase::new_for_family(11, GameFamily::Swbf2);
    phase.engine.samples.push(Sample {
        render_id: 0,
        duration: 2.0,
        rate: 48000.0,
        scale: 1.0,
        available: true,
    });
    let grp_weapons = zero_hash("synth_weapons_group");
    for s in 0..=SND_ROCKET_HUM {
        let looping = matches!(s, SND_BEAM_LOOP | SND_OVERHEAT | SND_CHAIN_WARM | SND_CHAIN_BARREL | SND_ROCKET_HUM);
        let (min, max, mute) = if s == SND_ROCKET_HUM { (15.0, 120.0, 120.0) } else { (20.0, 200.0, 200.0) };
        phase.engine.sounds.push(plain_sound(0, min, max, mute, looping));
        phase.swbf2_mut().unwrap().sound_groups.push(grp_weapons);
    }

    // Populate 10 synthetic classes.
    phase.classes = vec![[-1.0; c::STRIDE]; 10];
    phase.swbf2_mut().unwrap().classes = vec![Swbf2Class::default(); 10];

    // CLS_BEAM_GUN: FireSound, FireLoopSound, FireSoundStop=1, OverheatSound (pitch 0.5), ReloadSound, WeaponChangeSound
    phase.classes[CLS_BEAM_GUN][c::FIRE] = SND_BEAM_FIRE as f64;
    phase.classes[CLS_BEAM_GUN][c::FIRE_LOOP] = SND_BEAM_LOOP as f64;
    phase.classes[CLS_BEAM_GUN][c::FIRE_STOP] = 1.0;
    phase.classes[CLS_BEAM_GUN][c::OVERHEAT] = SND_OVERHEAT as f64;
    phase.classes[CLS_BEAM_GUN][c::OVERHEAT_PITCH] = 0.5;
    phase.classes[CLS_BEAM_GUN][c::RELOAD] = SND_RELOAD as f64;
    phase.classes[CLS_BEAM_GUN][c::CHANGE] = SND_CHANGE as f64;

    // CLS_CHAIN_GUN: no FireSound, only WarmUpSound + BarrelSound
    phase.classes[CLS_CHAIN_GUN][c::OVERHEAT] = SND_OVERHEAT as f64;
    {
        let sw = phase.swbf2_mut().unwrap();
        sw.classes[CLS_BEAM_GUN].ordnance_class = CLS_ORD_LASER;
        sw.classes[CLS_CHAIN_GUN].warm_up = SND_CHAIN_WARM;
        sw.classes[CLS_CHAIN_GUN].warm_up_time = 0.4;
        sw.classes[CLS_CHAIN_GUN].warm_up_start_pitch = 0.5;
        sw.classes[CLS_CHAIN_GUN].barrel = SND_CHAIN_BARREL;
        sw.classes[CLS_ORD_LASER].explosion_class = CLS_EXP_HIT;
        sw.classes[CLS_ORD_LASER].expire_explosion_class = CLS_EXP_EXPIRE;
        sw.classes[CLS_CRAFT].destruct_explosion_class = NONE;
        sw.classes[CLS_PROP].destruct_explosion_class = CLS_EXP_PROP;
        sw.classes[CLS_ROCKET_GUN].ordnance_class = CLS_ORD_ROCKET;
    }

    phase.classes[CLS_ORD_LASER][c::IMPACT] = SND_IMPACT as f64;
    phase.classes[CLS_EXP_HIT][c::EXPLOSION] = SND_BLAST as f64;
    phase.classes[CLS_EXP_EXPIRE][c::EXPLOSION] = SND_EXPIRE_BLAST as f64;
    phase.classes[CLS_CRAFT][c::DEATH] = SND_CRAFT_DEATH as f64;
    phase.classes[CLS_EXP_PROP][c::EXPLOSION] = SND_PROP_DESTRUCT as f64;
    phase.classes[CLS_ORD_ROCKET][c::ORDNANCE] = SND_ROCKET_HUM as f64;

    {
        let sw = phase.swbf2_mut().unwrap();
        for (name, id) in [
            ("inv_beam_gun", CLS_BEAM_GUN),
            ("inv_chain_gun", CLS_CHAIN_GUN),
            ("inv_ord_laser", CLS_ORD_LASER),
            ("inv_exp_hit", CLS_EXP_HIT),
            ("inv_exp_expire", CLS_EXP_EXPIRE),
            ("inv_craft", CLS_CRAFT),
            ("inv_prop", CLS_PROP),
            ("inv_exp_prop", CLS_EXP_PROP),
            ("inv_rocket_gun", CLS_ROCKET_GUN),
            ("inv_ord_rocket", CLS_ORD_ROCKET),
        ] {
            sw.register_class_hash(zero_hash(name), id);
        }
        sw.finalize_lookups();
    }

    // Emitter 1 at 30 m ahead of listener.
    phase.emitters[1][e::ID] = 1.0;
    phase.emitters[1][e::KIND] = 3.0;
    phase.emitters[1][e::X] = 0.0;
    phase.emitters[1][e::Y] = 0.0;
    phase.emitters[1][e::Z] = -30.0;
    phase.entity_count = 2;

    phase.started = true;
    phase.frame[f::RUNNING] = 1.0;
    phase.frame[f::RATE] = 48000.0;
    phase.frame[f::CHANNELS] = 2.0;
    phase.frame[f::MAX_VOICES] = 32.0;
    phase.frame[f::VIRTUAL_VOICES] = 64.0;
    phase.frame[f::EFFECTS_VOLUME] = 1.0;
    phase.frame[f::AMBIENCE_VOLUME] = 1.0;
    phase.frame[f::MUSIC_VOLUME] = 1.0;
    phase.frame[f::VOICE_VOLUME] = 1.0;
    phase.frame[f::INTERFACE_VOLUME] = 1.0;
    phase.frame[f::FZ] = -1.0;
    phase.frame[f::UY] = 1.0;
    phase.frame[f::OWN] = -1.0;
    phase.engine.running = true;
    phase.engine.listener.set([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    phase
}

fn step_tick(phase: &mut Phase, tick: usize) {
    let dt = 1.0 / 60.0;
    let now = (tick as f64 + 1.0) * dt;
    phase.frame[f::DT] = dt;
    phase.frame[f::NOW] = now;
    phase.frame[f::WALL_NOW] = now;
    phase.engine.commands.clear();
    phase.engine.placements.clear();
    phase.step(&mut NoCollision, &[]);
}

fn has_live_sound(phase: &Phase, sound_id: usize) -> bool {
    phase.engine.voices.iter().any(|v| !v.done && v.sound == sound_id)
}

#[test]
fn beam_chaingun_overheat_reload_change_hits_and_scale_parameter() {
    let mut phase = build_fixture();
    let beam_hash = zero_hash("inv_beam_gun");
    let chain_hash = zero_hash("inv_chain_gun");

    // 1. Switch to beam gun + fire: plays WeaponChangeSound, FireSound, and FireLoopSound.
    phase.push_space_weapon(SpaceWeaponInput {
        key: 10,
        owner: 1,
        class_hash: beam_hash,
        fired: true,
        firing: true,
        spinning: false,
        overheated: false,
        heat: 0.2,
        heat_threshold: 0.5,
        reloading: false,
        changed: true,
        own: false,
    });
    step_tick(&mut phase, 0);
    assert!(has_live_sound(&phase, SND_CHANGE), "WeaponChangeSound must play");
    assert!(has_live_sound(&phase, SND_BEAM_FIRE), "FireSound must play");
    assert!(has_live_sound(&phase, SND_BEAM_LOOP), "FireLoopSound must loop while firing");

    // 2. Stop firing: FireLoopSound AND FireSound (FireSoundStop=1) stop cleanly.
    phase.push_space_weapon(SpaceWeaponInput {
        key: 10,
        owner: 1,
        class_hash: beam_hash,
        fired: false,
        firing: false,
        spinning: false,
        overheated: false,
        heat: 0.2,
        heat_threshold: 0.5,
        reloading: false,
        changed: false,
        own: false,
    });
    step_tick(&mut phase, 1);
    assert!(!has_live_sound(&phase, SND_BEAM_LOOP), "FireLoopSound must stop when trigger released");
    assert!(!has_live_sound(&phase, SND_BEAM_FIRE), "FireSoundStop must stop FireSound on release");

    // 3. Overheat pitch sweeps linearly from base * OverheatSoundPitch (0.5) up toward base (1.0).
    phase.push_space_weapon(SpaceWeaponInput {
        key: 10,
        owner: 1,
        class_hash: beam_hash,
        fired: false,
        firing: false,
        spinning: false,
        overheated: true,
        heat: 1.0,
        heat_threshold: 0.5,
        reloading: false,
        changed: false,
        own: false,
    });
    step_tick(&mut phase, 2);
    let pitch_hot = phase.engine.voices.iter().find(|v| !v.done && v.sound == SND_OVERHEAT).expect("OverheatSound").pitch;
    assert!((pitch_hot - 0.75).abs() < 1e-6, "expected 0.5 + 0.5 * (0.5 / 1.0) = 0.75, got {pitch_hot}");

    phase.push_space_weapon(SpaceWeaponInput {
        key: 10,
        owner: 1,
        class_hash: beam_hash,
        fired: false,
        firing: false,
        spinning: false,
        overheated: true,
        heat: 0.5,
        heat_threshold: 0.5,
        reloading: false,
        changed: false,
        own: false,
    });
    step_tick(&mut phase, 3);
    let pitch_cooled = phase.engine.voices.iter().find(|v| !v.done && v.sound == SND_OVERHEAT).expect("OverheatSound").pitch;
    assert!((pitch_cooled - 1.0).abs() < 1e-6, "expected pitch 1.0 at threshold, got {pitch_cooled}");

    // End overheat, start reload.
    phase.push_space_weapon(SpaceWeaponInput {
        key: 10,
        owner: 1,
        class_hash: beam_hash,
        fired: false,
        firing: false,
        spinning: false,
        overheated: false,
        heat: 0.0,
        heat_threshold: 0.5,
        reloading: true,
        changed: false,
        own: false,
    });
    step_tick(&mut phase, 4);
    assert!(!has_live_sound(&phase, SND_OVERHEAT));
    assert!(has_live_sound(&phase, SND_RELOAD));

    // 4. Chaingun with no FireSound plays WarmUpSound while spinning and BarrelSound while firing.
    phase.push_space_weapon(SpaceWeaponInput {
        key: 11,
        owner: 1,
        class_hash: chain_hash,
        fired: false,
        firing: false,
        spinning: true,
        overheated: false,
        heat: 0.0,
        heat_threshold: 0.5,
        reloading: false,
        changed: false,
        own: false,
    });
    step_tick(&mut phase, 5);
    assert!(has_live_sound(&phase, SND_CHAIN_WARM), "chaingun WarmUpSound must start");
    assert!(!has_live_sound(&phase, SND_CHAIN_BARREL));

    phase.push_space_weapon(SpaceWeaponInput {
        key: 11,
        owner: 1,
        class_hash: chain_hash,
        fired: true,
        firing: true,
        spinning: true,
        overheated: false,
        heat: 0.1,
        heat_threshold: 0.5,
        reloading: false,
        changed: false,
        own: false,
    });
    step_tick(&mut phase, 6);
    assert!(has_live_sound(&phase, SND_CHAIN_WARM));
    assert!(has_live_sound(&phase, SND_CHAIN_BARREL), "chaingun BarrelSound must play while firing");

    // 5. Round hit, expiry explosion, and craft/prop destruction play linked sounds.
    phase.play_space_round_hit(beam_hash, [0.0, 0.0, -40.0]);
    phase.play_space_round_expired(beam_hash, [0.0, 0.0, -50.0]);
    phase.play_space_destruction(zero_hash("inv_craft"), [0.0, 0.0, -60.0]);
    phase.play_space_destruction(zero_hash("inv_prop"), [0.0, 0.0, -70.0]);
    step_tick(&mut phase, 7);
    assert!(has_live_sound(&phase, SND_IMPACT), "CollisionSound must play on hit");
    assert!(has_live_sound(&phase, SND_BLAST), "Explosion SoundProperty must play on hit");
    assert!(has_live_sound(&phase, SND_EXPIRE_BLAST), "ExplosionExpire SoundProperty must play");
    assert!(has_live_sound(&phase, SND_CRAFT_DEATH), "DeathSound must play on craft destruction");
    assert!(has_live_sound(&phase, SND_PROP_DESTRUCT), "ExplosionDestruct SoundProperty must play");

    // 6. ScaleSoundParameter extends MuteDistance so a 250 m hit (previously culled at 200 m) becomes audible.
    let culls_before = phase.engine.stats[5];
    phase.play_space_round_hit(beam_hash, [0.0, 0.0, -250.0]);
    assert!(phase.engine.stats[5] > culls_before, "250 m hit should cull before scaling");
    phase.scale_sound_parameter(zero_hash("synth_weapons_group"), SoundParam::MaxDistance, 2.0);
    phase.scale_sound_parameter(zero_hash("synth_weapons_group"), SoundParam::MuteDistance, 2.0);
    let started_before = phase.engine.stats[2];
    phase.play_space_round_hit(beam_hash, [0.0, 0.0, -250.0]);
    assert!(phase.engine.stats[2] > started_before, "250 m hit should play after MuteDistance x2");
}

#[test]
fn bounded_ordnance_loops_and_zero_steady_state_allocations() {
    let mut phase = build_fixture();
    let rocket_hash = zero_hash("inv_rocket_gun");
    let beam_hash = zero_hash("inv_beam_gun");
    // Prime one tick before counting allocations.
    step_tick(&mut phase, 0);

    // 24 rockets in flight (exceeds MAX_SPACE_ORDNANCE_LOOPS = 16); only 16 nearest play, zero heap allocations.
    let mut rockets = [SpaceOrdnanceInput { serial: 0, weapon_hash: rocket_hash, position: [0.0; 3], velocity: [0.0, 0.0, 80.0] }; 24];
    for (i, r) in rockets.iter_mut().enumerate() {
        r.serial = (i + 1) as u64;
        r.position = [0.0, 0.0, -10.0 - (i as f64) * 4.0];
    }

    let allocs = measure_allocs(|| {
        for tick in 1..=30 {
            phase.push_space_weapon(SpaceWeaponInput {
                key: 10,
                owner: 1,
                class_hash: beam_hash,
                fired: tick % 5 == 0,
                firing: true,
                spinning: false,
                overheated: false,
                heat: 0.1,
                heat_threshold: 0.5,
                reloading: false,
                changed: false,
                own: false,
            });
            phase.sync_space_ordnance(64, &rockets);
            if tick % 10 == 0 {
                phase.play_space_round_hit(beam_hash, [0.0, 0.0, -45.0]);
            }
            step_tick(&mut phase, tick);
        }
    });
    assert_eq!(allocs, 0, "steady-state space weapon & ordnance ticks must perform zero heap allocations");

    let live_rockets = phase.engine.voices.iter().filter(|v| !v.done && v.sound == SND_ROCKET_HUM).count();
    assert_eq!(live_rockets, MAX_SPACE_ORDNANCE_LOOPS, "ordnance flight loops must cap cleanly at 16");

    // Removing rockets stops all their flight loops cleanly.
    phase.sync_space_ordnance(64, &[]);
    step_tick(&mut phase, 31);
    assert!(!has_live_sound(&phase, SND_ROCKET_HUM));
    assert_ne!(SILENT, 0);
}

#[test]
fn dead_slot_sweep_stops_weapon_and_chaingun_loops_when_craft_disappears() {
    let mut phase = build_fixture();
    let beam_hash = zero_hash("inv_beam_gun");
    let chain_hash = zero_hash("inv_chain_gun");

    // Start beam FireLoopSound on slot 10 and chaingun WarmUpSound + BarrelSound on slot 11.
    phase.push_space_weapon(SpaceWeaponInput {
        key: 10,
        owner: 1,
        class_hash: beam_hash,
        fired: true,
        firing: true,
        spinning: false,
        overheated: false,
        heat: 0.2,
        heat_threshold: 0.5,
        reloading: false,
        changed: false,
        own: false,
    });
    phase.push_space_weapon(SpaceWeaponInput {
        key: 11,
        owner: 1,
        class_hash: chain_hash,
        fired: true,
        firing: true,
        spinning: true,
        overheated: false,
        heat: 0.1,
        heat_threshold: 0.5,
        reloading: false,
        changed: false,
        own: false,
    });
    step_tick(&mut phase, 0);
    assert!(has_live_sound(&phase, SND_BEAM_LOOP), "beam fire loop must be live");
    assert!(has_live_sound(&phase, SND_CHAIN_WARM), "chaingun warmup loop must be live");
    assert!(has_live_sound(&phase, SND_CHAIN_BARREL), "chaingun barrel loop must be live");

    // Craft/turret destroyed mid-loop: neither weapon slot is pushed on subsequent ticks.
    // The dead-slot sweep in swbf2_weapons must stop all active looping voices cleanly.
    for t in 1..=10 {
        step_tick(&mut phase, t);
    }
    assert!(
        !has_live_sound(&phase, SND_BEAM_LOOP),
        "beam fire loop must stop when its weapon slot disappears"
    );
    assert!(
        !has_live_sound(&phase, SND_CHAIN_WARM),
        "chaingun warmup loop must stop when its weapon slot disappears"
    );
    assert!(
        !has_live_sound(&phase, SND_CHAIN_BARREL),
        "chaingun barrel loop must stop when its weapon slot disappears"
    );
}

#[test]
fn chaingun_barrel_loop_applies_barrel_sound_fade_in_time() {
    let chain_hash = zero_hash("inv_chain_gun");

    // 1. SpaceWeaponInput path (weapons.rs): BarrelSoundFadeInTime = 0.2 s ramps gain_scale 0 -> 0.5 -> 1.0.
    let mut phase = build_fixture();
    phase.swbf2_mut().unwrap().classes[CLS_CHAIN_GUN].barrel_fade_in = 0.2;
    let push_space = |p: &mut Phase, firing: bool| {
        p.push_space_weapon(SpaceWeaponInput {
            key: 11,
            owner: 1,
            class_hash: chain_hash,
            fired: firing,
            firing,
            spinning: firing,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.5,
            reloading: false,
            changed: false,
            own: false,
        });
    };
    push_space(&mut phase, true);
    step_tick(&mut phase, 0);
    let g0 = phase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("barrel loop voice at tick 0")
        .gain_scale;
    assert!(
        g0 < 0.01,
        "space chaingun barrel loop must start faded in from 0.0 on tick 0, got {g0}"
    );
    for t in 1..=6 {
        push_space(&mut phase, true);
        step_tick(&mut phase, t);
    }
    let g_mid = phase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("barrel loop voice at 0.1 s")
        .gain_scale;
    assert!(
        (g_mid - 0.5).abs() < 0.02,
        "space chaingun barrel loop at 0.1 s of 0.2 s fade-in should be ~0.5, got {g_mid}"
    );
    for t in 7..=12 {
        push_space(&mut phase, true);
        step_tick(&mut phase, t);
    }
    let g_full = phase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("barrel loop voice at 0.2 s")
        .gain_scale;
    assert!(
        (g_full - 1.0).abs() < 1e-6,
        "space chaingun barrel loop at 0.2 s should reach 1.0, got {g_full}"
    );
    // Stop and restart: fade-in must reset to 0.0.
    for t in 13..=25 {
        push_space(&mut phase, false);
        step_tick(&mut phase, t);
    }
    assert!(!has_live_sound(&phase, SND_CHAIN_BARREL));
    push_space(&mut phase, true);
    step_tick(&mut phase, 26);
    let g_restart = phase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("restarted barrel loop voice")
        .gain_scale;
    assert!(
        g_restart < 0.01,
        "restarted space chaingun barrel loop must fade in again from 0.0, got {g_restart}"
    );

    // 2. ChaingunInput path (swbf2/vehicles.rs): same BarrelSoundFadeInTime = 0.2 s ramp.
    let mut vphase = build_fixture();
    vphase.swbf2_mut().unwrap().classes[CLS_CHAIN_GUN].barrel_fade_in = 0.2;
    let push_veh = |p: &mut Phase, firing: bool| {
        p.update_chaingun(ChaingunInput {
            weapon: 5,
            owner: 1,
            class_hash: chain_hash,
            spinning: firing,
            firing,
        });
    };
    push_veh(&mut vphase, true);
    step_tick(&mut vphase, 0);
    let vg0 = vphase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("vehicle chaingun barrel loop at tick 0")
        .gain_scale;
    assert!(
        vg0 < 0.01,
        "vehicle chaingun barrel loop must start faded in from 0.0 on tick 0, got {vg0}"
    );
    for t in 1..=6 {
        push_veh(&mut vphase, true);
        step_tick(&mut vphase, t);
    }
    let vg_mid = vphase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("vehicle chaingun barrel loop at 0.1 s")
        .gain_scale;
    assert!(
        (vg_mid - 0.5).abs() < 0.02,
        "vehicle chaingun barrel loop at 0.1 s of 0.2 s fade-in should be ~0.5, got {vg_mid}"
    );
    for t in 7..=12 {
        push_veh(&mut vphase, true);
        step_tick(&mut vphase, t);
    }
    let vg_full = vphase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("vehicle chaingun barrel loop at 0.2 s")
        .gain_scale;
    assert!(
        (vg_full - 1.0).abs() < 1e-6,
        "vehicle chaingun barrel loop at 0.2 s should reach 1.0, got {vg_full}"
    );
    for t in 13..=25 {
        push_veh(&mut vphase, false);
        step_tick(&mut vphase, t);
    }
    assert!(!has_live_sound(&vphase, SND_CHAIN_BARREL));
    push_veh(&mut vphase, true);
    step_tick(&mut vphase, 26);
    let vg_restart = vphase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHAIN_BARREL)
        .expect("restarted vehicle chaingun barrel loop")
        .gain_scale;
    assert!(
        vg_restart < 0.01,
        "restarted vehicle chaingun barrel loop must fade in again from 0.0, got {vg_restart}"
    );
}
