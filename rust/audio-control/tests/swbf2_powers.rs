//! Invented-data tests for 2005 hero powers, continuous choke holds, thrown
//! lightsabers, combo blasts, bot lightsabers, and zero steady-state heap
//! allocations.

use audio_control::engine::{Curve, Entry, Sample, Sound, NONE, SILENT};
use audio_control::layout::{class as c, emitter as e, frame as f};
use audio_control::phase::Phase;
use audio_control::swbf2::powers::{
    PowerAudioEvent, PowerAudioKind, PowerHoldInput, SaberThrowInput,
};
use audio_control::swbf2::saber::SaberEvent;
use audio_control::swbf2::{zero_hash, Swbf2Class};
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

const SND_PUSH: usize = 0;
const SND_PULL: usize = 1;
const SND_CHOKE: usize = 2;
const SND_LIGHTNING_FIRE: usize = 3;
const SND_LIGHTNING_HIT: usize = 4;
const SND_AURA: usize = 5;
const SND_THROW_LAUNCH: usize = 6;
const SND_THROW_TWIRL: usize = 7;
const SND_BLAST_SLAM: usize = 8;
const SND_SABER_ON: usize = 9;
const SND_SABER_OFF: usize = 10;
const SND_SABER_HUM: usize = 11;
const SND_SABER_HIT: usize = 12;
const SND_SABER_BLOCK: usize = 13;

const CLS_PUSH: usize = 0;
const CLS_PULL: usize = 1;
const CLS_CHOKE: usize = 2;
const CLS_LIGHTNING: usize = 3;
const CLS_LIGHTNING_ORD: usize = 4;
const CLS_LIGHTNING_EXP: usize = 5;
const CLS_AURA: usize = 6;
const CLS_THROW: usize = 7;
const CLS_THROW_ORD: usize = 8;
const CLS_BLADE: usize = 9;
const CLS_BLAST_EXP: usize = 10;

fn build_fixture() -> Phase {
    let mut phase = Phase::new_for_family(19, GameFamily::Swbf2);
    phase.engine.samples.push(Sample {
        render_id: 0,
        duration: 1.5,
        rate: 48000.0,
        scale: 1.0,
        available: true,
    });
    for s in 0..=SND_SABER_BLOCK {
        let looping = matches!(s, SND_THROW_TWIRL | SND_SABER_HUM);
        phase.engine.sounds.push(plain_sound(0, 15.0, 80.0, 80.0, looping));
        phase.swbf2_mut().unwrap().sound_groups.push(zero_hash("inv_hero_grp"));
    }

    phase.classes = vec![[-1.0; c::STRIDE]; 11];
    phase.swbf2_mut().unwrap().classes = vec![Swbf2Class::default(); 11];

    phase.classes[CLS_PUSH][c::FIRE] = SND_PUSH as f64;
    phase.classes[CLS_PULL][c::FIRE] = SND_PULL as f64;
    phase.classes[CLS_CHOKE][c::FIRE] = SND_CHOKE as f64;
    phase.classes[CLS_LIGHTNING][c::FIRE] = SND_LIGHTNING_FIRE as f64;
    phase.classes[CLS_LIGHTNING_EXP][c::EXPLOSION] = SND_LIGHTNING_HIT as f64;
    phase.classes[CLS_AURA][c::FIRE] = SND_AURA as f64;
    phase.classes[CLS_THROW][c::FIRE] = SND_THROW_LAUNCH as f64;
    phase.classes[CLS_THROW_ORD][c::ORDNANCE] = SND_THROW_TWIRL as f64;
    phase.classes[CLS_BLAST_EXP][c::EXPLOSION] = SND_BLAST_SLAM as f64;

    {
        let sw = phase.swbf2_mut().unwrap();
        sw.classes[CLS_LIGHTNING].ordnance_class = CLS_LIGHTNING_ORD;
        sw.classes[CLS_LIGHTNING_ORD].explosion_class = CLS_LIGHTNING_EXP;
        sw.classes[CLS_THROW].ordnance_class = CLS_THROW_ORD;
        sw.classes[CLS_BLADE].explosion_class = CLS_BLAST_EXP;
        sw.classes[CLS_BLADE].saber_turn_on = SND_SABER_ON;
        sw.classes[CLS_BLADE].saber_turn_off = SND_SABER_OFF;
        sw.classes[CLS_BLADE].saber_on_hum = SND_SABER_HUM;
        sw.classes[CLS_BLADE].saber_hit = SND_SABER_HIT;
        sw.classes[CLS_BLADE].saber_deflect = SND_SABER_BLOCK;

        for (name, id) in [
            ("inv_push", CLS_PUSH),
            ("inv_pull", CLS_PULL),
            ("inv_choke", CLS_CHOKE),
            ("inv_lightning", CLS_LIGHTNING),
            ("inv_lightning_ord", CLS_LIGHTNING_ORD),
            ("inv_lightning_exp", CLS_LIGHTNING_EXP),
            ("inv_aura", CLS_AURA),
            ("inv_throw", CLS_THROW),
            ("inv_throw_ord", CLS_THROW_ORD),
            ("inv_blade", CLS_BLADE),
            ("inv_blast_exp", CLS_BLAST_EXP),
        ] {
            sw.register_class_hash(zero_hash(name), id);
        }
        sw.finalize_lookups();
    }

    // Emitter 1 (player at origin) and emitter 2 (bot 10 m ahead).
    for (id, z) in [(1usize, 0.0), (2usize, -10.0)] {
        phase.emitters[id][e::ID] = id as f64;
        phase.emitters[id][e::KIND] = 1.0;
        phase.emitters[id][e::ALIVE] = 1.0;
        phase.emitters[id][e::GENERATION] = 1.0;
        phase.emitters[id][e::Z] = z;
    }
    phase.entity_count = 3;

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
    phase.frame[f::OWN] = 1.0;
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

fn live_count(phase: &Phase, snd: usize) -> usize {
    phase.engine.voices.iter().filter(|v| !v.done && v.sound == snd).count()
}

#[test]
fn push_pull_lightning_aura_blast_and_block_play_for_player_and_bots() {
    let mut phase = build_fixture();

    // Player Push, Bot Pull, Bot Aura.
    phase.push_power_event(PowerAudioEvent {
        caster_key: 1,
        emitter: 1,
        weapon_hash: zero_hash("inv_push"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Cast,
        caster_pos: [0.0, 0.0, 0.0],
        target_pos: None,
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 2,
        emitter: 2,
        weapon_hash: zero_hash("inv_pull"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Cast,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: None,
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 2,
        emitter: 2,
        weapon_hash: zero_hash("inv_aura"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Cast,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: None,
    });
    step_tick(&mut phase, 0);
    assert_eq!(live_count(&phase, SND_PUSH), 1, "push cast plays");
    assert_eq!(live_count(&phase, SND_PULL), 1, "bot pull cast plays");
    assert_eq!(live_count(&phase, SND_AURA), 1, "bot aura cast plays");

    // Lightning cast + hit + blocked.
    phase.push_power_event(PowerAudioEvent {
        caster_key: 2,
        emitter: 2,
        weapon_hash: zero_hash("inv_lightning"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Cast,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: None,
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 2,
        emitter: 2,
        weapon_hash: zero_hash("inv_lightning"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Hit,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: Some([0.0, 0.0, -3.0]),
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 2,
        emitter: 2,
        weapon_hash: zero_hash("inv_lightning"),
        target_weapon_hash: zero_hash("inv_blade"),
        kind: PowerAudioKind::Blocked,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: Some([0.0, 0.0, -2.0]),
    });
    step_tick(&mut phase, 1);
    assert_eq!(live_count(&phase, SND_LIGHTNING_FIRE), 1, "lightning fire plays");
    assert_eq!(live_count(&phase, SND_LIGHTNING_HIT), 1, "lightning impact plays");
    assert_eq!(live_count(&phase, SND_SABER_BLOCK), 1, "power block plays");

    // Combo blast (Cast + Hit on victim): explosion plays ONCE on Cast, not doubled by Hit.
    phase.push_power_event(PowerAudioEvent {
        caster_key: 1,
        emitter: 1,
        weapon_hash: zero_hash("inv_blade"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Cast,
        caster_pos: [0.0, 0.0, 0.0],
        target_pos: None,
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 1,
        emitter: 1,
        weapon_hash: zero_hash("inv_blade"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Thrown,
        caster_pos: [0.0, 0.0, 0.0],
        target_pos: Some([0.0, 0.0, -3.0]),
    });
    step_tick(&mut phase, 2);
    assert_eq!(live_count(&phase, SND_BLAST_SLAM), 1, "combo blast plays once on Cast");
}

#[test]
fn choke_plays_once_as_authored_without_double_cast_and_stops_on_release() {
    let mut phase = build_fixture();
    let choke_hash = zero_hash("inv_choke");
    let hold = [PowerHoldInput {
        key: 42,
        emitter: 2,
        weapon_hash: choke_hash,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: [0.0, 0.0, -4.0],
    }];

    // Acquire tick emits BOTH Cast and active hold: must start ONE non-looping voice, not two.
    phase.push_power_event(PowerAudioEvent {
        caster_key: 42,
        emitter: 2,
        weapon_hash: choke_hash,
        target_weapon_hash: 0,
        kind: PowerAudioKind::Cast,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: None,
    });
    phase.queue_power_holds(40, &hold);
    step_tick(&mut phase, 0);
    assert_eq!(live_count(&phase, SND_CHOKE), 1, "choke starts one voice");
    let choke_voice = phase
        .engine
        .voices
        .iter()
        .find(|v| !v.done && v.sound == SND_CHOKE)
        .expect("live choke voice");
    assert!(
        !choke_voice.looping,
        "choke FireSound must respect authored Looping=0 instead of forcing a loop"
    );
    assert_eq!(phase.swbf2().unwrap().powers.active_hold_count(), 1);

    // Sustain for 10 ticks (0.17 s < 1.5 s sample): still that single voice.
    let starts_after_acquire = phase.engine.stats[2];
    for t in 1..=10 {
        phase.queue_power_holds(40, &hold);
        step_tick(&mut phase, t);
    }
    assert_eq!(live_count(&phase, SND_CHOKE), 1);
    assert_eq!(phase.engine.stats[2], starts_after_acquire);

    // Early release: voice fades out and stops cleanly.
    phase.queue_power_holds(40, &[]);
    step_tick(&mut phase, 11);
    assert_eq!(phase.swbf2().unwrap().powers.active_hold_count(), 0);
    for t in 12..=25 {
        phase.queue_power_holds(40, &[]);
        step_tick(&mut phase, t);
    }
    assert_eq!(live_count(&phase, SND_CHOKE), 0, "choke stopped cleanly after early release");

    // Holding past the 1.5 s sample duration (110 ticks = 1.83 s) must let the one-shot finish
    // naturally without restarting or repeating.
    let hold2 = [PowerHoldInput { key: 43, ..hold[0] }];
    let starts_before_long = phase.engine.stats[2];
    for t in 26..=136 {
        phase.queue_power_holds(40, &hold2);
        step_tick(&mut phase, t);
    }
    assert_eq!(
        phase.engine.stats[2] - starts_before_long,
        1,
        "held choke must start its authored one-shot once and never repeat"
    );
    assert_eq!(
        live_count(&phase, SND_CHOKE),
        0,
        "authored non-looping choke sample must end naturally past its duration"
    );
}

#[test]
fn thrown_blade_plays_launch_and_moving_spin_pauses_hand_hum_without_turnoff() {
    let mut phase = build_fixture();
    let blade = zero_hash("inv_blade");
    let throw_wpn = zero_hash("inv_throw");

    // Ignite bot saber on emitter 2.
    phase.push_saber_event(SaberEvent::SetBladeActive {
        owner: 2,
        weapon_class_hash: blade,
        active: true,
    });
    step_tick(&mut phase, 0);
    assert_eq!(live_count(&phase, SND_SABER_ON), 1);
    assert_eq!(live_count(&phase, SND_SABER_HUM), 1);

    // Launch thrown blade: pauses hand hum WITHOUT TurnOffSound, starts launch + spin loop.
    let thrown = [SaberThrowInput {
        key: 99,
        owner: 2,
        weapon_hash: throw_wpn,
        position: [0.0, 1.0, -12.0],
        velocity: [0.0, 0.0, -25.0],
    }];
    phase.push_saber_event(SaberEvent::SetThrown { owner: 2, thrown: true });
    phase.queue_saber_throws(100, &thrown);
    step_tick(&mut phase, 1);
    assert_eq!(live_count(&phase, SND_SABER_OFF), 0, "throw must not play TurnOffSound");
    assert_eq!(live_count(&phase, SND_THROW_LAUNCH), 1, "launch plays inv_throw launch cue");
    assert_eq!(live_count(&phase, SND_THROW_TWIRL), 1, "in-flight plays inv_throw_ord spin loop");
    assert_eq!(phase.swbf2().unwrap().sabers.hum_voice(2), SILENT);
    // Verify the live engine voice for the in-hand hum actually stopped, not only the bank field.
    for t in 2..=6 {
        phase.queue_saber_throws(100, &thrown);
        step_tick(&mut phase, t);
    }
    assert_eq!(
        live_count(&phase, SND_SABER_HUM),
        0,
        "in-hand blade hum voice must actually stop while thrown"
    );
    assert_eq!(live_count(&phase, SND_THROW_TWIRL), 1, "spin loop stays live in flight");

    // Catch saber: stops twirl and resumes hand hum WITHOUT bogus TurnOnSound.
    let on_before = phase.engine.stats[2];
    phase.push_saber_event(SaberEvent::SetThrown { owner: 2, thrown: false });
    phase.queue_saber_throws(100, &[]);
    step_tick(&mut phase, 7);
    assert_eq!(phase.swbf2().unwrap().powers.active_throw_count(), 0);
    assert_ne!(phase.swbf2().unwrap().sabers.hum_voice(2), SILENT, "hand hum resumes on catch");
    assert_eq!(live_count(&phase, SND_SABER_HUM), 1, "live in-hand hum voice resumes on catch");
    assert_eq!(phase.engine.stats[2] - on_before, 1, "only hum restarts, no TurnOnSound");
    for t in 8..=18 {
        phase.queue_saber_throws(100, &[]);
        step_tick(&mut phase, t);
    }
    assert_eq!(live_count(&phase, SND_THROW_TWIRL), 0, "twirl stops on catch");
}

#[test]
fn steady_state_hero_power_and_saber_ticks_perform_zero_heap_allocations() {
    let mut phase = build_fixture();
    let blade = zero_hash("inv_blade");
    let push = zero_hash("inv_push");
    let choke = zero_hash("inv_choke");
    let throw_wpn = zero_hash("inv_throw");

    // Warm up voices/buffers.
    phase.push_saber_event(SaberEvent::SetBladeActive { owner: 1, weapon_class_hash: blade, active: true });
    phase.push_saber_event(SaberEvent::SetBladeActive { owner: 2, weapon_class_hash: blade, active: true });
    step_tick(&mut phase, 0);

    let allocs = measure_allocs(|| {
        for tick in 1..=60 {
            if tick % 10 == 0 {
                phase.push_power_event(PowerAudioEvent {
                    caster_key: 1,
                    emitter: 1,
                    weapon_hash: push,
                    target_weapon_hash: 0,
                    kind: PowerAudioKind::Cast,
                    caster_pos: [0.0, 0.0, 0.0],
                    target_pos: None,
                });
            }
            let holds = if (10..30).contains(&tick) {
                &[PowerHoldInput {
                    key: 2,
                    emitter: 2,
                    weapon_hash: choke,
                    caster_pos: [0.0, 0.0, -10.0],
                    target_pos: [0.0, 0.0, -2.0],
                }][..]
            } else {
                &[]
            };
            phase.queue_power_holds(40, holds);
            let throws = if (20..40).contains(&tick) {
                &[SaberThrowInput {
                    key: 7,
                    owner: 1,
                    weapon_hash: throw_wpn,
                    position: [0.0, 1.0, -(tick as f64)],
                    velocity: [0.0, 0.0, -20.0],
                }][..]
            } else {
                &[]
            };
            phase.queue_saber_throws(100, throws);
            step_tick(&mut phase, tick);
        }
    });
    assert_eq!(allocs, 0, "steady-state hero power audio must perform zero heap allocations");
}

#[test]
fn blocked_power_and_saber_contacts_use_contact_class_not_first_table_entry() {
    let mut phase = build_fixture();
    // Append a second blade class with distinct hit/deflect sounds and a bare class
    // with no saber sounds, AFTER CLS_BLADE (an earlier blade in the table).
    let snd_hit_b = phase.engine.sounds.len();
    phase.engine.sounds.push(plain_sound(0, 15.0, 80.0, 80.0, false));
    phase.swbf2_mut().unwrap().sound_groups.push(zero_hash("inv_hero_grp"));
    let snd_block_b = phase.engine.sounds.len();
    phase.engine.sounds.push(plain_sound(0, 15.0, 80.0, 80.0, false));
    phase.swbf2_mut().unwrap().sound_groups.push(zero_hash("inv_hero_grp"));

    let cls_blade_b = phase.classes.len();
    phase.classes.push([-1.0; c::STRIDE]);
    let mut ext_b = Swbf2Class::default();
    ext_b.saber_hit = snd_hit_b;
    ext_b.saber_deflect = snd_block_b;
    ext_b.saber_on_hum = SND_SABER_HUM;
    phase.swbf2_mut().unwrap().classes.push(ext_b);

    let cls_bare = phase.classes.len();
    phase.classes.push([-1.0; c::STRIDE]);
    phase.swbf2_mut().unwrap().classes.push(Swbf2Class::default());

    {
        let sw = phase.swbf2_mut().unwrap();
        sw.register_class_hash(zero_hash("inv_blade_b"), cls_blade_b);
        sw.register_class_hash(zero_hash("inv_bare"), cls_bare);
        sw.finalize_lookups();
    }

    // 1. Bare class or unknown contact must stay silent instead of playing CLS_BLADE's sounds.
    phase.push_saber_event(SaberEvent::Hit {
        owner: NONE,
        weapon_class_hash: zero_hash("inv_bare"),
        position: [0.0, 1.0, -5.0],
    });
    phase.push_saber_event(SaberEvent::Hit {
        owner: NONE,
        weapon_class_hash: 0,
        position: [0.0, 1.0, -5.0],
    });
    phase.push_saber_event(SaberEvent::Block {
        owner: NONE,
        sound_hash: 0,
        position: [0.0, 1.0, -5.0],
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 2,
        emitter: 2,
        weapon_hash: zero_hash("inv_lightning"),
        target_weapon_hash: 0,
        kind: PowerAudioKind::Blocked,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: Some([0.0, 0.0, -2.0]),
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 2,
        emitter: 2,
        weapon_hash: zero_hash("inv_lightning"),
        target_weapon_hash: zero_hash("inv_bare"),
        kind: PowerAudioKind::Blocked,
        caster_pos: [0.0, 0.0, -10.0],
        target_pos: Some([0.0, 0.0, -2.0]),
    });
    step_tick(&mut phase, 0);
    assert_eq!(
        live_count(&phase, SND_SABER_HIT),
        0,
        "missing/bare saber hit must not fall back to an earlier blade in the table"
    );
    assert_eq!(
        live_count(&phase, SND_SABER_BLOCK),
        0,
        "missing saber/power block must not fall back to an earlier blade in the table"
    );

    // 2. Active second blade class on emitter 2 resolves to blade B's sounds, not blade A's.
    phase.push_saber_event(SaberEvent::SetBladeActive {
        owner: 2,
        weapon_class_hash: zero_hash("inv_blade_b"),
        active: true,
    });
    phase.push_saber_event(SaberEvent::Hit {
        owner: 2,
        weapon_class_hash: zero_hash("inv_blade_b"),
        position: [0.0, 1.0, -5.0],
    });
    phase.push_saber_event(SaberEvent::Block {
        owner: 2,
        sound_hash: 0,
        position: [0.0, 1.0, -5.0],
    });
    phase.push_power_event(PowerAudioEvent {
        caster_key: 1,
        emitter: 1,
        weapon_hash: zero_hash("inv_lightning"),
        target_weapon_hash: zero_hash("inv_blade_b"),
        kind: PowerAudioKind::Blocked,
        caster_pos: [0.0, 0.0, 0.0],
        target_pos: Some([0.0, 0.0, -5.0]),
    });
    step_tick(&mut phase, 1);
    assert_eq!(live_count(&phase, snd_hit_b), 1, "hit must play blade B's HitSound");
    assert_eq!(
        live_count(&phase, snd_block_b),
        2,
        "saber block and blocked power must both play blade B's DeflectSound"
    );
    assert_eq!(live_count(&phase, SND_SABER_HIT), 0, "blade A hit must not play");
    assert_eq!(live_count(&phase, SND_SABER_BLOCK), 0, "blade A block must not play");
}
