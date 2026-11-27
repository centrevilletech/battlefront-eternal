//! Synthetic tests for 2005 voice limits, distance-aware virtual voice stealing,
//! layered group bounds, 6-stream budget, out-of-range layered pre-culling, and
//! zero steady-state heap allocations under combined space + hero combat load.

use audio_control::engine::{
    Ask, Curve, Entry, Sample, Sound, EFFECTS, EXPLOSIONS, FOLEY, MUSIC, NONE, SILENT, VOICE,
    WEAPONS,
};
use audio_control::layers::{Definition, Layer};
use audio_control::layout::{class as c, emitter as e, frame as f};
use audio_control::phase::Phase;
use audio_control::streams::{Options as StreamOptions, Segment, Stream};
use audio_control::swbf2::powers::{
    PowerAudioEvent, PowerAudioKind, PowerHoldInput, SaberThrowInput,
};
use audio_control::swbf2::saber::SaberEvent;
use audio_control::swbf2::weapons::{SpaceAudioEvent, SpaceOrdnanceInput, SpaceWeaponInput};
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

fn plain_3d_sound(sample_id: usize, gain: f64, min: f64, max: f64, mute: f64, looping: bool) -> Sound {
    Sound {
        kind: 1,
        bus: NONE,
        pitch: 1.0,
        pitch_dev: 0.0,
        gain,
        gain_dev: 0.0,
        reverb: 0.0,
        looping,
        pan: 0.0,
        mode3d: true,
        doppler: false,
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
        entries: vec![Entry { id: sample_id, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
    }
}

fn layered_parent(kind: usize) -> Sound {
    Sound {
        kind,
        bus: NONE,
        pitch: 1.0,
        pitch_dev: 0.0,
        gain: 1.0,
        gain_dev: 0.0,
        reverb: 0.0,
        looping: false,
        pan: 0.0,
        mode3d: true,
        doppler: false,
        bias: 0.5,
        priority: 0.5,
        curve: Curve { roll_in: 0.0, min: 0.0, mute: 1.0, max: 1.0, roll: 0.0 },
        probability: 1.0,
        interval: 0.0,
        interval_dev: 0.0,
        random_position: false,
        cycle: -2,
        next: f64::NEG_INFINITY,
        stream: NONE,
        entries: Vec::new(),
    }
}

fn setup_phase(family: GameFamily) -> Phase {
    let mut phase = Phase::new_for_family(7, family);
    phase.engine.samples.push(Sample {
        render_id: 0,
        duration: 1.5,
        rate: 48000.0,
        scale: 1.0,
        available: true,
    });
    // Sound 0: close-range craft engine loop
    phase.engine.sounds.push(plain_3d_sound(0, 0.9, 10.0, 200.0, 200.0, true));
    // Sound 1: laser bolt fire (audible to 600 m)
    phase.engine.sounds.push(plain_3d_sound(0, 0.85, 25.0, 600.0, 600.0, false));
    // Sound 2: explosion layer child (audible to 300 m)
    phase.engine.sounds.push(plain_3d_sound(0, 0.9, 20.0, 300.0, 300.0, false));
    // Sound 3: 4-layer explosion parent (kind = 2)
    phase.engine.sounds.push(layered_parent(2));
    // Sound 4: parameterized engine parent (kind = 3)
    phase.engine.sounds.push(layered_parent(3));

    phase.layers.definitions.resize_with(5, || Definition { layers: Vec::new() });
    phase.layers.definitions[3] = Definition {
        layers: (0..4)
            .map(|i| Layer { sound: 2, delay: i as f64 * 0.05, deviation: 0.0, bindings: Vec::new() })
            .collect(),
    };
    phase.layers.definitions[4] = Definition {
        layers: vec![Layer { sound: 0, delay: 0.0, deviation: 0.0, bindings: Vec::new() }],
    };

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

#[test]
fn swbf1_keeps_legacy_scoring_disabled_and_swbf2_enables_it() {
    let legacy = Phase::new(1);
    let swbf1 = Phase::new_for_family(1, GameFamily::Swbf1);
    let swbf2 = Phase::new_for_family(1, GameFamily::Swbf2);
    assert!(!legacy.engine.swbf2_voice_scoring);
    assert!(!swbf1.engine.swbf2_voice_scoring);
    assert!(swbf2.engine.swbf2_voice_scoring);
    assert_eq!(legacy.streams.max_active, 0);
    assert_eq!(swbf1.streams.max_active, 0);
    assert_eq!(swbf2.streams.max_active, 6);
}

#[test]
fn swbf2_distant_fire_does_not_steal_close_voices() {
    let mut phase = setup_phase(GameFamily::Swbf2);
    // Start a player engine close by plus 63 moderately close sounds (filling 64 virtual voices).
    let mut eng_ask = Ask::new(0, FOLEY);
    eng_ask.position = Some([0.0, 0.0, -5.0]);
    let engine_voice = phase.play(eng_ask);
    assert_ne!(engine_voice, SILENT);

    let mut mid_voices = Vec::new();
    for i in 0..63 {
        let mut ask = Ask::new(1, WEAPONS);
        ask.position = Some([0.0, 0.0, -40.0 - (i as f64) * 0.5]);
        let h = phase.play(ask);
        assert_ne!(h, SILENT);
        mid_voices.push(h);
    }
    step_tick(&mut phase, 0);
    assert!(phase.playing(engine_voice));

    // Distant fire at 550 m has very low distance gain and must be dropped rather than stealing.
    let drops_before = phase.engine.stats[4];
    for _ in 0..20 {
        let mut far = Ask::new(1, WEAPONS);
        far.position = Some([0.0, 0.0, -550.0]);
        assert_eq!(phase.play(far), SILENT);
    }
    assert_eq!(phase.engine.stats[4], drops_before + 20);
    assert!(phase.playing(engine_voice), "distant fire must never steal nearby engine");

    // A close gun at 5 m scores higher than the farthest 71 m voice and steals cleanly.
    let mut close = Ask::new(1, WEAPONS);
    close.position = Some([0.0, 0.0, -5.0]);
    let close_voice = phase.play(close);
    assert_ne!(close_voice, SILENT);
    assert!(phase.playing(engine_voice));
}

#[test]
fn swbf2_thirty_craft_ten_round_salvo_storm_stays_bounded() {
    let mut phase = setup_phase(GameFamily::Swbf2);
    // Start the player's parameterized engine right next to the listener.
    let mut own_eng = Ask::new(4, FOLEY);
    own_eng.position = Some([0.0, 0.0, -3.0]);
    let own_engine = phase.parameterized(own_eng);
    assert_ne!(own_engine, SILENT);
    step_tick(&mut phase, 0);

    // 30 craft firing 10-round salvos at 10 Hz across 3 seconds (180 ticks = 9000 rounds + blasts),
    // plus distant explosions past MuteDistance (should pre-cull without allocating layer groups).
    for tick in 1..=180 {
        if tick % 6 == 0 {
            for craft in 0..30 {
                let dist = 50.0 + (craft as f64) * 18.0;
                for round in 0..10 {
                    let mut fire = Ask::new(1, WEAPONS);
                    fire.position = Some([(round as f64 - 5.0) * 0.5, 0.0, -dist]);
                    let _ = phase.play(fire);
                }
                let mut blast = Ask::new(3, EXPLOSIONS);
                blast.position = Some([0.0, 0.0, -dist]);
                let _ = phase.play(blast);
            }
            // Extra distant layered explosions well beyond child MuteDistance (300 m).
            for _ in 0..20 {
                let mut far_blast = Ask::new(3, EXPLOSIONS);
                far_blast.position = Some([0.0, 0.0, -900.0]);
                assert_eq!(phase.play(far_blast), SILENT);
            }
        }
        // Periodic close player shot must keep sounding despite the 30-craft storm.
        if tick % 15 == 0 {
            let mut own_shot = Ask::new(1, EFFECTS);
            own_shot.position = Some([0.0, 0.0, -4.0]);
            assert_ne!(phase.play(own_shot), SILENT, "close player gun starved at tick {tick}");
        }

        step_tick(&mut phase, tick);

        let real_voices = phase.engine.voices.iter().filter(|v| !v.done && v.real).count();
        let virtual_voices = phase.engine.voices.iter().filter(|v| !v.done).count();
        let non_param_groups = phase.layers.active_non_param_count();
        assert!(real_voices <= 32, "real voices exceeded 32: {real_voices} at tick {tick}");
        assert!(virtual_voices <= 64, "virtual voices exceeded 64: {virtual_voices} at tick {tick}");
        assert!(non_param_groups <= 64, "non-param layer groups exceeded 64: {non_param_groups} at tick {tick}");
        assert!(phase.playing(own_engine), "player parameterized engine starved at tick {tick}");
    }

    assert!(phase.engine.stats[5] > 0, "expected distant layered blasts to be pre-culled");
    assert!(phase.engine.stats[4] > 0, "expected excess storm sounds to drop cleanly");
}

#[test]
fn swbf2_combined_space_hero_and_stream_storm_is_zero_alloc_and_bounded() {
    let mut phase = setup_phase(GameFamily::Swbf2);
    // Sound 5: hero power / choke / saber / ordnance one-shot & loop sounds.
    phase.engine.sounds.push(plain_3d_sound(0, 0.85, 15.0, 120.0, 120.0, false)); // 5: power cast
    phase.engine.sounds.push(plain_3d_sound(0, 0.80, 15.0, 100.0, 100.0, true));  // 6: saber hum / twirl / choke
    phase.engine.sounds.push(plain_3d_sound(0, 0.85, 20.0, 200.0, 200.0, true));  // 7: ordnance loop
    for i in 0..phase.engine.sounds.len() {
        phase.swbf2_mut().unwrap().sound_groups.push(i as u32);
    }

    // Register 8 stream definitions and stream sounds (indices 8..16) to test the 6-stream cap.
    for s in 0..8 {
        phase.streams.definitions.push(Stream { slots: 2, channels: 2 });
        phase.streams.segments.push(Segment {
            stream: s,
            duration: 30.0,
            channels: 2,
            variants: [NONE; 4],
        });
        let mut ss = plain_3d_sound(s, 0.7, 10.0, 100.0, 100.0, true);
        ss.kind = 4;
        ss.mode3d = false;
        ss.stream = s;
        phase.engine.sounds.push(ss);
        phase.swbf2_mut().unwrap().sound_groups.push(100 + s as u32);
    }
    phase.streams.configure();

    // Classes: 0=space gun, 1=space ord, 2=hero power, 3=hero blade, 4=hero throw, 5=hero throw ord.
    phase.classes = vec![[-1.0; c::STRIDE]; 6];
    phase.swbf2_mut().unwrap().classes = vec![Swbf2Class::default(); 6];
    phase.classes[0][c::FIRE] = 1.0;
    phase.classes[1][c::ORDNANCE] = 7.0;
    phase.classes[1][c::IMPACT] = 2.0;
    phase.classes[2][c::FIRE] = 5.0;
    phase.classes[4][c::FIRE] = 5.0;
    phase.classes[5][c::ORDNANCE] = 6.0;
    {
        let sw = phase.swbf2_mut().unwrap();
        sw.classes[0].ordnance_class = 1;
        sw.classes[1].explosion_class = 1;
        sw.classes[3].saber_turn_on = 5;
        sw.classes[3].saber_turn_off = 5;
        sw.classes[3].saber_on_hum = 6;
        sw.classes[3].saber_hit = 5;
        sw.classes[3].saber_deflect = 5;
        sw.classes[4].ordnance_class = 5;
        for (name, id) in [
            ("inv_gun", 0),
            ("inv_ord", 1),
            ("inv_power", 2),
            ("inv_blade", 3),
            ("inv_throw", 4),
            ("inv_throw_ord", 5),
        ] {
            sw.register_class_hash(zero_hash(name), id);
        }
        sw.finalize_lookups();
    }

    // Pre-populate 30 craft emitters (1..=30) + 4 hero emitters (31..=34).
    for id in 1..=34usize {
        phase.emitters[id][e::ID] = id as f64;
        phase.emitters[id][e::KIND] = if id <= 30 { 3.0 } else { 1.0 };
        phase.emitters[id][e::ALIVE] = 1.0;
        phase.emitters[id][e::GENERATION] = 1.0;
        phase.emitters[id][e::Z] = -(15.0 + (id as f64) * 12.0);
    }
    phase.entity_count = 35;

    // Warm up one tick, start own engine, ignite 4 hero sabers, and attempt 8 streams (capped at 6).
    let mut own_eng = Ask::new(4, FOLEY);
    own_eng.position = Some([0.0, 0.0, -3.0]);
    let own_engine = phase.parameterized(own_eng);
    assert_ne!(own_engine, SILENT);

    let blade_hash = zero_hash("inv_blade");
    let gun_hash = zero_hash("inv_gun");
    let power_hash = zero_hash("inv_power");
    let throw_hash = zero_hash("inv_throw");
    for hero in 31..=34usize {
        phase.push_saber_event(SaberEvent::SetBladeActive {
            owner: hero,
            weapon_class_hash: blade_hash,
            active: true,
        });
    }
    let mut started_streams = 0usize;
    for s in 0..8usize {
        let mut opt = StreamOptions::new(8 + s, if s == 0 { MUSIC } else { VOICE });
        opt.ask.looping = Some(true);
        if phase.streams.play(&mut phase.engine, opt) != SILENT {
            started_streams += 1;
        }
    }
    assert_eq!(started_streams, 6, "2005 stream channels must cap at 6");
    assert_eq!(phase.streams.active_count(), 6);
    step_tick(&mut phase, 0);

    // Preallocate fixed stack arrays so the test harness itself performs 0 heap allocations.
    let mut ord_buf = [SpaceOrdnanceInput {
        serial: 0,
        weapon_hash: gun_hash,
        position: [0.0; 3],
        velocity: [0.0, 0.0, -40.0],
    }; 40];
    let mut hold_buf = [PowerHoldInput {
        key: 0,
        emitter: NONE,
        weapon_hash: power_hash,
        caster_pos: [0.0; 3],
        target_pos: [0.0; 3],
    }; 4];
    let mut throw_buf = [SaberThrowInput {
        key: 0,
        owner: NONE,
        weapon_hash: throw_hash,
        position: [0.0; 3],
        velocity: [0.0, 0.0, -20.0],
    }; 4];

    let mut peak_real = 0usize;
    let mut peak_virtual = 0usize;
    let mut peak_groups = 0usize;
    let mut peak_streams = 0usize;
    let mut close_shots_ok = 0usize;

    let allocs = measure_allocs(|| {
        for tick in 1..=180usize {
            let salvo = tick % 6 == 0;
            for craft in 0..30usize {
                let eid = craft + 1;
                let dist = 30.0 + (craft as f64) * 16.0;
                phase.push_space_weapon(SpaceWeaponInput {
                    key: craft,
                    owner: eid,
                    class_hash: gun_hash,
                    fired: salvo,
                    firing: salvo,
                    spinning: false,
                    overheated: false,
                    heat: 0.2,
                    heat_threshold: 0.8,
                    reloading: false,
                    changed: false,
                    own: false,
                });
                if salvo {
                    for r in 0..9 {
                        let mut fire = Ask::new(1, WEAPONS);
                        fire.position = Some([(r as f64 - 4.0) * 0.5, 0.0, -dist]);
                        let _ = phase.play(fire);
                    }
                    let mut blast = Ask::new(3, EXPLOSIONS);
                    blast.position = Some([0.0, 0.0, -dist]);
                    let _ = phase.play(blast);
                    phase.push_space_audio_event(SpaceAudioEvent::RoundHit {
                        weapon_hash: gun_hash,
                        position: [0.0, 0.0, -dist],
                    });
                }
            }
            if salvo {
                // Distant layered explosions beyond MuteDistance must pre-cull without allocating.
                for _ in 0..16 {
                    let mut far = Ask::new(3, EXPLOSIONS);
                    far.position = Some([0.0, 0.0, -900.0]);
                    assert_eq!(phase.play(far), SILENT);
                }
            }
            for (idx, item) in ord_buf.iter_mut().enumerate() {
                item.serial = (idx + 1) as u64;
                item.position = [0.0, 0.0, -(20.0 + (idx as f64) * 5.0)];
            }
            phase.queue_space_ordnance(60, &ord_buf);

            // Hero powers, choke holds, and thrown sabers every tick.
            if tick % 12 == 0 {
                for h in 0..4usize {
                    phase.push_power_event(PowerAudioEvent {
                        caster_key: (h + 1) as u64,
                        emitter: 31 + h,
                        weapon_hash: power_hash,
                        target_weapon_hash: if tick % 24 == 0 { 0 } else { blade_hash },
                        kind: if tick % 24 == 0 { PowerAudioKind::Cast } else { PowerAudioKind::Blocked },
                        caster_pos: [0.0, 0.0, -(15.0 + (h as f64) * 8.0)],
                        target_pos: Some([0.0, 0.0, -10.0]),
                    });
                }
            }
            let holds_len = if (tick % 40) < 20 { 4 } else { 0 };
            for h in 0..holds_len {
                hold_buf[h] = PowerHoldInput {
                    key: 100 + h as u64,
                    emitter: 31 + h,
                    weapon_hash: power_hash,
                    caster_pos: [0.0, 0.0, -(15.0 + (h as f64) * 8.0)],
                    target_pos: [0.0, 0.0, -8.0],
                };
            }
            phase.queue_power_holds(120, &hold_buf[..holds_len]);

            let throws_len = if (tick % 50) < 25 { 4 } else { 0 };
            for h in 0..throws_len {
                throw_buf[h] = SaberThrowInput {
                    key: 200 + h as u64,
                    owner: 31 + h,
                    weapon_hash: throw_hash,
                    position: [0.0, 1.0, -(10.0 + (tick % 25) as f64)],
                    velocity: [0.0, 0.0, -20.0],
                };
            }
            phase.queue_saber_throws(160, &throw_buf[..throws_len]);

            // Extra stream open attempts beyond 6 must be rejected with 0 allocations.
            let mut extra_stream = StreamOptions::new(15, VOICE);
            extra_stream.ask.looping = Some(true);
            assert_eq!(phase.streams.play(&mut phase.engine, extra_stream), SILENT);

            // Close player shot must never starve.
            if tick % 15 == 0 {
                let mut close = Ask::new(1, EFFECTS);
                close.position = Some([0.0, 0.0, -4.0]);
                assert_ne!(phase.play(close), SILENT, "close player shot starved at tick {tick}");
                close_shots_ok += 1;
            }

            step_tick(&mut phase, tick);

            let real = phase.engine.voices.iter().filter(|v| !v.done && v.real).count();
            let virt = phase.engine.voices.iter().filter(|v| !v.done).count();
            let groups = phase.layers.active_non_param_count();
            let streams = phase.streams.active_count();
            peak_real = peak_real.max(real);
            peak_virtual = peak_virtual.max(virt);
            peak_groups = peak_groups.max(groups);
            peak_streams = peak_streams.max(streams);
            assert!(real <= 32, "real voices exceeded 32: {real} at tick {tick}");
            assert!(virt <= 64, "virtual voices exceeded 64: {virt} at tick {tick}");
            assert!(groups <= 64, "non-param groups exceeded 64: {groups} at tick {tick}");
            assert!(streams <= 6, "active streams exceeded 6: {streams} at tick {tick}");
            assert!(phase.playing(own_engine), "player engine starved at tick {tick}");
        }
    });

    assert_eq!(allocs, 0, "battle loop performed {allocs} heap allocations under full storm");
    assert_eq!(close_shots_ok, 12);
    assert!(peak_real <= 32 && peak_real > 0);
    assert!(peak_virtual <= 64 && peak_virtual > 0);
    assert!(peak_groups <= 64 && peak_groups > 0);
    assert_eq!(peak_streams, 6);
    assert!(phase.engine.stats[5] > 0, "expected distant layered blasts pre-culled");
    assert!(phase.engine.stats[4] > 0, "expected excess storm sounds dropped cleanly");
}

#[test]
fn swbf2_own_weapon_fire_and_fresh_close_voices_survive_saturated_pool() {
    use audio_control::layout::weapon as w;

    // 1. Fresh close voice started in a saturated tick must not be stolen by a distant voice
    // later in the same tick (score must be initialized on creation, not left at 0.0).
    let mut phase = setup_phase(GameFamily::Swbf2);
    for i in 0..64 {
        let mut far = Ask::new(1, WEAPONS);
        far.position = Some([0.0, 0.0, -400.0 - i as f64]);
        assert_ne!(phase.play(far), SILENT);
    }
    step_tick(&mut phase, 0);

    let mut close = Ask::new(1, WEAPONS);
    close.position = Some([0.0, 0.0, -5.0]);
    let close_handle = phase.play(close);
    assert_ne!(close_handle, SILENT, "close voice must steal a 400 m voice");

    let mut late_far = Ask::new(1, WEAPONS);
    late_far.position = Some([0.0, 0.0, -500.0]);
    assert_eq!(
        phase.play(late_far),
        SILENT,
        "distant voice later in the same tick must be dropped, not steal a fresh close voice"
    );
    assert!(
        phase.playing(close_handle),
        "fresh close voice must survive later distant voices in the same tick"
    );

    // 2. Local player's own weapon fire must not be refused, stolen, or demoted below real
    // slots even when all 64 voices are louder, higher-priority sounds right at the listener.
    let mut phase = setup_phase(GameFamily::Swbf2);
    // Sound 5: loud high-priority nearby blast (gain = 1.0, priority = 1.0)
    let mut loud = plain_3d_sound(0, 1.0, 50.0, 500.0, 500.0, false);
    loud.priority = 1.0;
    phase.engine.sounds.push(loud); // index 5
    // Sound 6: quieter local player weapon fire (gain = 0.45, priority = 0.5)
    let mut own_gun = plain_3d_sound(0, 0.45, 25.0, 400.0, 400.0, false);
    own_gun.priority = 0.5;
    phase.engine.sounds.push(own_gun); // index 6

    phase.classes = vec![[-1.0; c::STRIDE]; 1];
    phase.classes[0][c::FIRE] = 6.0;
    phase.emitters[0][e::ID] = 0.0;
    phase.emitters[0][e::KIND] = 1.0;
    phase.emitters[0][e::ALIVE] = 1.0;
    phase.emitters[0][e::GENERATION] = 1.0;
    phase.emitters[0][e::Z] = -2.0;
    phase.entity_count = 1;
    phase.frame[f::OWN] = 0.0;
    phase.weapons[0][w::CLASS] = 0.0;
    phase.weapons[0][w::OWNER] = 0.0;
    phase.weapon_count = 1;

    for _ in 0..64 {
        let mut ask = Ask::new(5, EXPLOSIONS);
        ask.position = Some([0.0, 0.0, -2.0]);
        assert_ne!(phase.play(ask), SILENT);
    }
    step_tick(&mut phase, 0);

    // Fire player's own quieter gun under a completely full pool of gain=1.0 blasts.
    phase.fire_weapon(0, false, true);
    let own_handle = phase.player_weapon_state[0].v1;
    assert_ne!(own_handle, SILENT, "player own weapon fire must not be refused under full pool");
    assert!(phase.playing(own_handle), "player own weapon voice must be live");

    // Spam 64 more loud blasts in the same tick and across the next 5 ticks.
    for _ in 0..64 {
        let mut ask = Ask::new(5, EXPLOSIONS);
        ask.position = Some([0.0, 0.0, -2.0]);
        let _ = phase.play(ask);
    }
    assert!(phase.playing(own_handle), "player own weapon voice must not be stolen in the same tick");

    for tick in 1..=5 {
        step_tick(&mut phase, tick);
        assert!(phase.playing(own_handle), "player own weapon voice lost at tick {tick}");
        let is_real = phase
            .engine
            .voices
            .iter()
            .any(|v| !v.done && v.sound == 6 && v.real);
        assert!(is_real, "player own weapon voice must hold a real voice slot at tick {tick}");
        for _ in 0..32 {
            let mut ask = Ask::new(5, EXPLOSIONS);
            ask.position = Some([0.0, 0.0, -2.0]);
            let _ = phase.play(ask);
        }
    }

    // 3. SpaceWeaponInput with own=true also stays protected under a full close pool.
    phase.classes.push([-1.0; c::STRIDE]);
    phase.classes[1][c::FIRE] = 6.0;
    phase.swbf2_mut().unwrap().classes = vec![Swbf2Class::default(); 2];
    phase.swbf2_mut().unwrap().register_class_hash(zero_hash("inv_own_fighter_gun"), 1);
    phase.swbf2_mut().unwrap().finalize_lookups();
    phase.push_space_weapon(SpaceWeaponInput {
        key: 77,
        owner: 0,
        class_hash: zero_hash("inv_own_fighter_gun"),
        fired: true,
        firing: true,
        spinning: false,
        overheated: false,
        heat: 0.0,
        heat_threshold: 0.8,
        reloading: false,
        changed: false,
        own: true,
    });
    step_tick(&mut phase, 6);
    let own_space_real = phase
        .engine
        .voices
        .iter()
        .filter(|v| !v.done && v.sound == 6 && v.real)
        .count();
    assert!(own_space_real >= 2, "own fighter weapon fire must start and hold a real voice slot");

    // 4. 2004 (Swbf1) leaves legacy pool scoring untouched and ignores Ask.protected.
    let mut legacy = setup_phase(GameFamily::Swbf1);
    let mut loud_legacy = plain_3d_sound(0, 1.0, 50.0, 500.0, 500.0, false);
    loud_legacy.priority = 1.0;
    legacy.engine.sounds.push(loud_legacy); // index 5
    let mut quiet_legacy = plain_3d_sound(0, 0.45, 25.0, 400.0, 400.0, false);
    quiet_legacy.priority = 0.2;
    legacy.engine.sounds.push(quiet_legacy); // index 6
    for _ in 0..64 {
        let mut ask = Ask::new(5, EXPLOSIONS);
        ask.position = Some([0.0, 0.0, -2.0]);
        assert_ne!(legacy.play(ask), SILENT);
    }
    step_tick(&mut legacy, 0);
    let mut ask = Ask::new(6, WEAPONS);
    ask.position = Some([0.0, 0.0, -2.0]);
    ask.protected = true;
    // In 2004, estimate = (1 - 0.0001) * 1.0 + 0.0001 * 0.2 = 0.99992 <= existing 1.0 -> refused.
    assert_eq!(legacy.play(ask), SILENT, "2004 Swbf1 must ignore Ask.protected");
}

#[test]
fn swbf2_saturated_pool_does_not_re_request_loops_every_tick() {
    use audio_control::layout::turret as t;
    use audio_control::swbf2::ambience::{RegionShape, TriggerRegion};
    use audio_control::swbf2::vehicles::{BuildingEmitter, ChaingunInput};

    let mut phase = setup_phase(GameFamily::Swbf2);
    // Sound 5: loud close blocker loop (gain = 1.0, priority = 1.0)
    let mut blocker = plain_3d_sound(0, 1.0, 50.0, 500.0, 500.0, true);
    blocker.priority = 1.0;
    phase.engine.sounds.push(blocker); // index 5
    // Sound 6: quieter distant loop used by all 2005 loop controllers (gain = 0.5, mute = 300 m)
    let mut far_loop = plain_3d_sound(0, 0.5, 10.0, 300.0, 300.0, true);
    far_loop.priority = 0.3;
    phase.engine.sounds.push(far_loop); // index 6

    // Classes: 0=space beam+chaingun, 1=vehicle chaingun, 2=building, 3=blade, 4=throw, 5=throw_ord, 6=turret
    phase.classes = vec![[-1.0; c::STRIDE]; 7];
    phase.classes[0][c::FIRE_LOOP] = 6.0;
    phase.classes[5][c::ORDNANCE] = 6.0;
    phase.classes[6][c::YAW] = 6.0;
    {
        let sw = phase.swbf2_mut().unwrap();
        sw.classes = vec![Swbf2Class::default(); 7];
        sw.classes[0].warm_up = 6;
        sw.classes[0].barrel = 6;
        sw.classes[1].warm_up = 6;
        sw.classes[1].barrel = 6;
        sw.classes[2].building_sound = 6;
        sw.classes[3].saber_on_hum = 6;
        sw.classes[4].ordnance_class = 5;
        for (name, id) in [
            ("inv_sat_space_wpn", 0),
            ("inv_sat_veh_chain", 1),
            ("inv_sat_bldg", 2),
            ("inv_sat_blade", 3),
            ("inv_sat_throw", 4),
            ("inv_sat_throw_ord", 5),
            ("inv_sat_turret", 6),
        ] {
            sw.register_class_hash(zero_hash(name), id);
        }
        sw.vehicles.buildings.push(BuildingEmitter {
            name_hash: zero_hash("inv_sat_bldg_inst"),
            class: 2,
            position: [0.0, 0.0, -100.0],
            alive: true,
            damaged: false,
            in_range: false,
            voice: SILENT,
            retry: Default::default(),
        });
        sw.ambience.triggers.push(TriggerRegion {
            hash: zero_hash("inv_sat_trig"),
            matrix: [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, -100.0, 1.0,
            ],
            size: [200.0, 200.0, 200.0],
            shape: RegionShape::Box,
            sound: 6,
            enabled: true,
            was_active: false,
            voice: SILENT,
            is_stream: false,
            retry: Default::default(),
        });
        sw.finalize_lookups();
    }

    // Emitters at 100 m (within 300 m mute, lower score than the 64 close blockers).
    for (id, kind) in [(1usize, 3.0), (2usize, 1.0), (3usize, 1.0)] {
        phase.emitters[id][e::ID] = id as f64;
        phase.emitters[id][e::KIND] = kind;
        phase.emitters[id][e::ALIVE] = 1.0;
        phase.emitters[id][e::GENERATION] = 1.0;
        phase.emitters[id][e::Z] = -100.0;
    }
    phase.entity_count = 4;

    // Turret 0 at 100 m.
    phase.turrets[0][t::ID] = 0.0;
    phase.turrets[0][t::CLASS] = 6.0;
    phase.turrets[0][t::OWNER] = -1.0;
    phase.turrets[0][t::Z] = -100.0;
    phase.turrets[0][t::HELD] = 1.0;
    phase.turrets[0][t::ALIVE] = 1.0;
    phase.turrets[0][t::YAW_TOP] = 1.5;
    phase.turrets[0][t::PITCH_TOP] = 1.5;
    phase.turrets[0][t::PRESENT] = 1.0;
    phase.turret_count = 1;

    // Fill all 64 virtual voices with loud close loops at the listener.
    let mut blockers = Vec::new();
    for _ in 0..64 {
        let mut ask = Ask::new(5, EXPLOSIONS);
        ask.position = Some([0.0, 0.0, -2.0]);
        let h = phase.play(ask);
        assert_ne!(h, SILENT);
        blockers.push(h);
    }
    step_tick(&mut phase, 0);

    // Tick 1: state-change entry for all 2005 loop controllers (each attempts once and is refused).
    let space_wpn = zero_hash("inv_sat_space_wpn");
    let veh_chain = zero_hash("inv_sat_veh_chain");
    let blade_h = zero_hash("inv_sat_blade");
    let throw_h = zero_hash("inv_sat_throw");
    let throw_input = [SaberThrowInput {
        key: 901,
        owner: 2,
        weapon_hash: throw_h,
        position: [0.0, 1.0, -100.0],
        velocity: [0.0, 0.0, -10.0],
    }];

    phase.push_saber_event(SaberEvent::SetBladeActive {
        owner: 3,
        weapon_class_hash: blade_h,
        active: true,
    });
    for tick in 1..=60usize {
        phase.turrets[0][t::YAW] = (tick as f64) * 0.05;
        phase.push_space_weapon(SpaceWeaponInput {
            key: 50,
            owner: 1,
            class_hash: space_wpn,
            fired: false,
            firing: true,
            spinning: true,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.8,
            reloading: false,
            changed: false,
            own: false,
        });
        phase.update_chaingun(ChaingunInput {
            weapon: 60,
            owner: 1,
            class_hash: veh_chain,
            spinning: true,
            firing: true,
        });
        phase.queue_saber_throws(100, &throw_input);
        step_tick(&mut phase, tick);

        if tick == 2 {
            // Snapshot stats after initial state-change ticks (turret initializes on tick 0,
            // detects turning on tick 1/2). From tick 3..=60 zero retries may occur.
        }
    }
    // Check steady-state ticks 61..=120 (1.0 s) while pool remains saturated.
    // Bounded recovery may retry at most on the per-slot cadence (never every tick),
    // and never starts or steals while all 64 blockers outscore the held loops.
    let started_before = phase.engine.stats[2];
    let stolen_before = phase.engine.stats[3];
    let dropped_before = phase.engine.stats[4];
    let mut ticks_with_drops = 0usize;
    for tick in 61..=120usize {
        let d_tick_before = phase.engine.stats[4];
        phase.turrets[0][t::YAW] = (tick as f64) * 0.05;
        phase.push_space_weapon(SpaceWeaponInput {
            key: 50,
            owner: 1,
            class_hash: space_wpn,
            fired: false,
            firing: true,
            spinning: true,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.8,
            reloading: false,
            changed: false,
            own: false,
        });
        phase.update_chaingun(ChaingunInput {
            weapon: 60,
            owner: 1,
            class_hash: veh_chain,
            spinning: true,
            firing: true,
        });
        phase.queue_saber_throws(100, &throw_input);
        step_tick(&mut phase, tick);
        if phase.engine.stats[4] > d_tick_before {
            ticks_with_drops += 1;
        }
    }
    let dropped_delta = phase.engine.stats[4] - dropped_before;
    assert_eq!(
        phase.engine.stats[2] - started_before,
        0,
        "steady-state held loops must not start voices while outscored under saturation"
    );
    assert_eq!(
        phase.engine.stats[3] - stolen_before,
        0,
        "steady-state held loops must not steal voices while outscored under saturation"
    );
    assert_eq!(
        dropped_delta,
        0,
        "steady-state outscored saturated loops must not hammer Engine::play or inflate dropped churn (got {dropped_delta} drops over 60 ticks)"
    );
    assert_eq!(
        ticks_with_drops,
        0,
        "steady-state outscored saturated loops must have zero drop ticks (got {ticks_with_drops} ticks with drops out of 60)"
    );

    // After the pool clears, held loops must recover WITHOUT releasing and re-triggering state.
    for h in blockers {
        phase.stop(h, 0.01);
    }
    for tick in 121..=140usize {
        phase.turrets[0][t::YAW] = (tick as f64) * 0.05;
        phase.push_space_weapon(SpaceWeaponInput {
            key: 50,
            owner: 1,
            class_hash: space_wpn,
            fired: false,
            firing: true,
            spinning: true,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.8,
            reloading: false,
            changed: false,
            own: false,
        });
        phase.update_chaingun(ChaingunInput {
            weapon: 60,
            owner: 1,
            class_hash: veh_chain,
            spinning: true,
            firing: true,
        });
        phase.queue_saber_throws(100, &throw_input);
        step_tick(&mut phase, tick);
    }
    let live_after = phase.engine.voices.iter().filter(|v| !v.done && v.sound == 6).count();
    assert_eq!(
        live_after, 10,
        "all 10 refused 2005 loop controllers must recover once pool capacity opens without re-triggering state"
    );
}

#[test]
fn swbf2_stolen_loops_recover_when_pool_frees_without_retrigger() {
    use audio_control::layout::turret as t;
    use audio_control::swbf2::ambience::{RegionShape, TriggerRegion};
    use audio_control::swbf2::vehicles::{BuildingEmitter, ChaingunInput};

    let mut phase = setup_phase(GameFamily::Swbf2);
    // Sound 5: loud close blocker loop (gain = 1.0, priority = 1.0)
    let mut blocker = plain_3d_sound(0, 1.0, 50.0, 500.0, 500.0, true);
    blocker.priority = 1.0;
    phase.engine.sounds.push(blocker); // index 5
    // Sound 6: quieter distant 3D loop (gain = 0.5, mute = 300 m)
    let mut far_loop = plain_3d_sound(0, 0.5, 10.0, 300.0, 300.0, true);
    far_loop.priority = 0.3;
    phase.engine.sounds.push(far_loop); // index 6
    // Sound 7: one-shot marker for blade turn-on / toss launch (must NOT replay on loop recovery)
    let mut oneshot_marker = plain_3d_sound(0, 0.5, 10.0, 300.0, 300.0, false);
    oneshot_marker.priority = 0.3;
    phase.engine.sounds.push(oneshot_marker); // index 7
    // Sound 8: quieter 2D cockpit loop (gain = 0.25, priority = 0.1, outscored by blockers)
    let mut cockpit_loop = plain_3d_sound(0, 0.25, 10.0, 300.0, 300.0, true);
    cockpit_loop.mode3d = false;
    cockpit_loop.priority = 0.1;
    phase.engine.sounds.push(cockpit_loop); // index 8

    // Classes:
    // 0 = space beam + chaingun, 1 = vehicle chaingun, 2 = building,
    // 3 = blade, 4 = blade toss weapon, 5 = toss ordnance, 6 = turret,
    // 7 = cockpit craft, 8 = loop power hold, 9 = rocket weapon, 10 = rocket ordnance
    phase.classes = vec![[-1.0; c::STRIDE]; 11];
    phase.classes[0][c::FIRE_LOOP] = 6.0;
    phase.classes[4][c::FIRE] = 7.0;
    phase.classes[5][c::ORDNANCE] = 6.0;
    phase.classes[6][c::YAW] = 6.0;
    phase.classes[8][c::FIRE_LOOP] = 6.0;
    phase.classes[10][c::ORDNANCE] = 6.0;
    {
        let sw = phase.swbf2_mut().unwrap();
        sw.classes = vec![Swbf2Class::default(); 11];
        sw.classes[0].warm_up = 6;
        sw.classes[0].barrel = 6;
        sw.classes[1].warm_up = 6;
        sw.classes[1].barrel = 6;
        sw.classes[2].building_sound = 6;
        sw.classes[3].saber_turn_on = 7;
        sw.classes[3].saber_on_hum = 6;
        sw.classes[4].ordnance_class = 5;
        sw.classes[7].cockpit_1p = 8;
        sw.classes[9].ordnance_class = 10;
        for (name, id) in [
            ("inv_stl_space_wpn", 0),
            ("inv_stl_veh_chain", 1),
            ("inv_stl_bldg", 2),
            ("inv_stl_blade", 3),
            ("inv_stl_toss", 4),
            ("inv_stl_toss_ord", 5),
            ("inv_stl_turret", 6),
            ("inv_stl_craft", 7),
            ("inv_stl_hold_loop", 8),
            ("inv_stl_rocket", 9),
            ("inv_stl_rocket_ord", 10),
        ] {
            sw.register_class_hash(zero_hash(name), id);
        }
        sw.vehicles.buildings.push(BuildingEmitter {
            name_hash: zero_hash("inv_stl_bldg_inst"),
            class: 2,
            position: [0.0, 0.0, -100.0],
            alive: true,
            damaged: false,
            in_range: false,
            voice: SILENT,
            retry: Default::default(),
        });
        sw.ambience.triggers.push(TriggerRegion {
            hash: zero_hash("inv_stl_trig"),
            matrix: [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, -100.0, 1.0,
            ],
            size: [200.0, 200.0, 200.0],
            shape: RegionShape::Box,
            sound: 6,
            enabled: true,
            was_active: false,
            voice: SILENT,
            is_stream: false,
            retry: Default::default(),
        });
        sw.finalize_lookups();
    }

    for (id, kind) in [(1usize, 3.0), (2usize, 1.0), (3usize, 1.0)] {
        phase.emitters[id][e::ID] = id as f64;
        phase.emitters[id][e::KIND] = kind;
        phase.emitters[id][e::ALIVE] = 1.0;
        phase.emitters[id][e::GENERATION] = 1.0;
        phase.emitters[id][e::Z] = -100.0;
    }
    phase.entity_count = 4;

    phase.turrets[0][t::ID] = 0.0;
    phase.turrets[0][t::CLASS] = 6.0;
    phase.turrets[0][t::OWNER] = -1.0;
    phase.turrets[0][t::Z] = -100.0;
    phase.turrets[0][t::HELD] = 1.0;
    phase.turrets[0][t::ALIVE] = 1.0;
    phase.turrets[0][t::YAW_TOP] = 1.5;
    phase.turrets[0][t::PITCH_TOP] = 1.5;
    phase.turrets[0][t::PRESENT] = 1.0;
    phase.turret_count = 1;

    let space_wpn = zero_hash("inv_stl_space_wpn");
    let veh_chain = zero_hash("inv_stl_veh_chain");
    let blade_h = zero_hash("inv_stl_blade");
    let toss_h = zero_hash("inv_stl_toss");
    let craft_h = zero_hash("inv_stl_craft");
    let hold_h = zero_hash("inv_stl_hold_loop");
    let rocket_h = zero_hash("inv_stl_rocket");

    let toss_input = [SaberThrowInput {
        key: 902,
        owner: 2,
        weapon_hash: toss_h,
        position: [0.0, 1.0, -100.0],
        velocity: [0.0, 0.0, -10.0],
    }];
    let hold_input = [PowerHoldInput {
        key: 903,
        emitter: 2,
        weapon_hash: hold_h,
        caster_pos: [0.0, 0.0, -100.0],
        target_pos: [0.0, 0.0, -90.0],
    }];
    let ord_input = [SpaceOrdnanceInput {
        serial: 904,
        weapon_hash: rocket_h,
        position: [0.0, 0.0, -100.0],
        velocity: [0.0, 0.0, -20.0],
    }];

    // Phase 1: start all 13 loop controllers cleanly in an empty pool.
    phase.set_own_mount_swbf2(Some(craft_h), true);
    phase.push_saber_event(SaberEvent::SetBladeActive {
        owner: 3,
        weapon_class_hash: blade_h,
        active: true,
    });
    for tick in 0..=4usize {
        phase.turrets[0][t::YAW] = (tick as f64) * 0.05;
        phase.push_space_weapon(SpaceWeaponInput {
            key: 50,
            owner: 1,
            class_hash: space_wpn,
            fired: false,
            firing: true,
            spinning: true,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.8,
            reloading: false,
            changed: false,
            own: false,
        });
        phase.update_chaingun(ChaingunInput {
            weapon: 60,
            owner: 1,
            class_hash: veh_chain,
            spinning: true,
            firing: true,
        });
        phase.queue_saber_throws(100, &toss_input);
        phase.queue_power_holds(140, &hold_input);
        phase.queue_space_ordnance(180, &ord_input);
        step_tick(&mut phase, tick);
    }
    let live_loops = phase
        .engine
        .voices
        .iter()
        .filter(|v| !v.done && (v.sound == 6 || v.sound == 8))
        .count();
    assert_eq!(live_loops, 13, "all 13 loop voices must be live before saturation");

    // Phase 2: saturate with 64 loud close blockers so every quieter loop is stolen.
    let mut blockers = Vec::new();
    for _ in 0..64 {
        let mut ask = Ask::new(5, EXPLOSIONS);
        ask.position = Some([0.0, 0.0, -2.0]);
        let h = phase.play(ask);
        assert_ne!(h, SILENT);
        blockers.push(h);
    }
    for tick in 5..=14usize {
        phase.turrets[0][t::YAW] = (tick as f64) * 0.05;
        phase.push_space_weapon(SpaceWeaponInput {
            key: 50,
            owner: 1,
            class_hash: space_wpn,
            fired: false,
            firing: true,
            spinning: true,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.8,
            reloading: false,
            changed: false,
            own: false,
        });
        phase.update_chaingun(ChaingunInput {
            weapon: 60,
            owner: 1,
            class_hash: veh_chain,
            spinning: true,
            firing: true,
        });
        phase.queue_saber_throws(100, &toss_input);
        phase.queue_power_holds(140, &hold_input);
        phase.queue_space_ordnance(180, &ord_input);
        step_tick(&mut phase, tick);
    }
    let live_saturated = phase
        .engine
        .voices
        .iter()
        .filter(|v| !v.done && (v.sound == 6 || v.sound == 8))
        .count();
    assert_eq!(live_saturated, 0, "all quieter loops must be stolen under 64 close blockers");

    // Phase 3: release blockers while keeping every controller active (no state change).
    for h in blockers {
        phase.stop(h, 0.01);
    }
    let marker_before = phase
        .engine
        .voices
        .iter()
        .filter(|v| !v.done && v.sound == 7)
        .count();
    for tick in 15..=35usize {
        phase.turrets[0][t::YAW] = (tick as f64) * 0.05;
        phase.push_space_weapon(SpaceWeaponInput {
            key: 50,
            owner: 1,
            class_hash: space_wpn,
            fired: false,
            firing: true,
            spinning: true,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.8,
            reloading: false,
            changed: false,
            own: false,
        });
        phase.update_chaingun(ChaingunInput {
            weapon: 60,
            owner: 1,
            class_hash: veh_chain,
            spinning: true,
            firing: true,
        });
        phase.queue_saber_throws(100, &toss_input);
        phase.queue_power_holds(140, &hold_input);
        phase.queue_space_ordnance(180, &ord_input);
        step_tick(&mut phase, tick);
    }
    let live_recovered = phase
        .engine
        .voices
        .iter()
        .filter(|v| !v.done && (v.sound == 6 || v.sound == 8))
        .count();
    assert_eq!(
        live_recovered, 13,
        "all 13 stolen 2005 loops must recover once pool capacity frees without re-triggering"
    );
    let marker_after = phase
        .engine
        .voices
        .iter()
        .filter(|v| !v.done && v.sound == 7)
        .count();
    assert!(
        marker_after <= marker_before,
        "loop recovery must not replay blade turn-on or throw launch one-shots"
    );
}

#[test]
fn swbf2_saturated_loop_recovers_on_cadence_when_it_outranks_lowest_voice() {
    let mut phase = setup_phase(GameFamily::Swbf2);
    // Sound 5: movable blocker loop following emitter 2
    let mut blocker = plain_3d_sound(0, 1.0, 50.0, 500.0, 500.0, true);
    blocker.priority = 0.5;
    phase.engine.sounds.push(blocker); // index 5
    // Sound 6: beam loop following emitter 1 at 20 m
    let mut beam = plain_3d_sound(0, 0.8, 50.0, 500.0, 500.0, true);
    beam.priority = 0.5;
    phase.engine.sounds.push(beam); // index 6

    phase.classes = vec![[-1.0; c::STRIDE]; 1];
    phase.classes[0][c::FIRE_LOOP] = 6.0;
    {
        let sw = phase.swbf2_mut().unwrap();
        sw.classes = vec![Swbf2Class::default(); 1];
        sw.register_class_hash(zero_hash("inv_cadence_beam"), 0);
        sw.finalize_lookups();
    }
    phase.emitters[1][e::ID] = 1.0;
    phase.emitters[1][e::KIND] = 3.0;
    phase.emitters[1][e::ALIVE] = 1.0;
    phase.emitters[1][e::GENERATION] = 1.0;
    phase.emitters[1][e::Z] = -20.0;
    phase.emitters[2][e::ID] = 2.0;
    phase.emitters[2][e::KIND] = 3.0;
    phase.emitters[2][e::ALIVE] = 1.0;
    phase.emitters[2][e::GENERATION] = 1.0;
    phase.emitters[2][e::Z] = -2.0;
    phase.entity_count = 3;

    // Fill all 64 virtual voices with blockers on emitter 2 (close -> score ~ 1.0).
    for _ in 0..64 {
        let mut ask = Ask::new(5, EXPLOSIONS);
        ask.emitter = 2;
        assert_ne!(phase.play(ask), SILENT);
    }
    step_tick(&mut phase, 0);

    let wpn = zero_hash("inv_cadence_beam");
    let push_beam = |p: &mut Phase| {
        p.push_space_weapon(SpaceWeaponInput {
            key: 88,
            owner: 1,
            class_hash: wpn,
            fired: false,
            firing: true,
            spinning: false,
            overheated: false,
            heat: 0.0,
            heat_threshold: 0.8,
            reloading: false,
            changed: false,
            own: false,
        });
    };

    // Tick 1: beam starts firing while blockers are closer -> refused.
    push_beam(&mut phase);
    step_tick(&mut phase, 1);
    assert_eq!(
        phase.engine.voices.iter().filter(|v| !v.done && v.sound == 6).count(),
        0,
        "beam loop must be refused when outscored by 64 closer blockers"
    );

    // Move blockers far away (300 m) while keeping all 64 active (pool stays saturated!).
    phase.emitters[2][e::Z] = -300.0;
    push_beam(&mut phase);
    step_tick(&mut phase, 2);
    // On tick 2 (1 tick after refusal, still saturated), must NOT have retried early.
    assert_eq!(
        phase.engine.voices.iter().filter(|v| !v.done && v.sound == 6).count(),
        0,
        "saturated loop must wait for its bounded cadence rather than retrying on the next tick"
    );

    // Within 12 ticks (~0.20 s > 0.10 s cadence), the beam retries, outscores the far blockers,
    // and claims a voice even though the pool stayed at 64/64.
    let mut recovered_at = None;
    for tick in 3..=15usize {
        push_beam(&mut phase);
        step_tick(&mut phase, tick);
        if phase.engine.voices.iter().any(|v| !v.done && v.sound == 6) {
            recovered_at = Some(tick);
            break;
        }
    }
    assert!(
        recovered_at.is_some_and(|t| (4..=12).contains(&t)),
        "saturated loop that now outranks the lowest active voice must recover on its bounded cadence (got {recovered_at:?})"
    );
}
