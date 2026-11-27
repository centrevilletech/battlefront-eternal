//! Synthetic tests for 2005 audio control features and zero-allocation stepping.
//! Every name, graph, region, and parameter here is invented for this test suite.

use audio_control::acoustics::Room;
use audio_control::engine::{Entry, Sample, Sound, NONE, SILENT};
use audio_control::layout::*;
use audio_control::runtime::Runtime;
use audio_control::swbf2::ambience::{
    blend_interior_room, RegionShape, SpaceAttenuation, SpacePreset, SpaceRegion, TriggerRegion,
};
use audio_control::swbf2::director::HeroVoKind;
use audio_control::swbf2::saber::SaberEvent;
use audio_control::swbf2::vehicles::{
    BoostCue, BuildingEmitter, ChaingunInput, DoorEvent, ProximityBinding, VehicleEvent,
};
use audio_control::swbf2::{
    zero_hash, ComboSoundSet, ComboSoundTrigger, ComboStateSounds, SoundParam, Swbf2Class, VoRule,
};
use audio_control::{Collision, Hit, Point};
use bfe_family::GameFamily;
use bfe_test_alloc::Counting;

#[global_allocator]
static GLOBAL: Counting = Counting::new();

fn measure_allocs<F: FnOnce()>(f: F) -> u32 {
    bfe_test_alloc::measure_thread(f).1
}

struct OpenRoom;
impl Collision for OpenRoom {
    fn cast(&mut self, from: Point, direction: Point, reach: f64) -> Option<Hit> {
        if direction[1] > 0.0 {
            let d = (20.0 - from[1]) / direction[1];
            if d >= 0.0 && d <= reach {
                return Some(Hit {
                    distance: d,
                    point: [from[0] + direction[0] * d, 20.0, from[2] + direction[2] * d],
                    terrain: false,
                });
            }
        }
        None
    }
}

fn plain_sound_row(gain: f64, looping: bool, mode3d: bool, max_dist: f64) -> [f64; sound::STRIDE] {
    let mut row = [f64::NAN; sound::STRIDE];
    row[sound::KIND] = 1.0;
    row[sound::BUS] = -1.0;
    row[sound::STREAM] = -1.0;
    row[sound::GAIN] = gain;
    row[sound::PITCH] = 1.0;
    row[sound::LOOP] = if looping { 1.0 } else { 0.0 };
    row[sound::MODE3D] = if mode3d { 1.0 } else { 0.0 };
    row[sound::DOPPLER] = 1.0;
    row[sound::MIN_DISTANCE] = 5.0;
    row[sound::MAX_DISTANCE] = max_dist;
    row[sound::MUTE_DISTANCE] = max_dist;
    row[sound::PROBABILITY] = 1.0;
    row
}

fn stream_sound_row(stream: usize, gain: f64, priority: f64, looping: bool) -> [f64; sound::STRIDE] {
    let mut row = [f64::NAN; sound::STRIDE];
    row[sound::KIND] = 4.0;
    row[sound::BUS] = -1.0;
    row[sound::STREAM] = stream as f64;
    row[sound::GAIN] = gain;
    row[sound::PITCH] = 1.0;
    row[sound::LOOP] = if looping { 1.0 } else { 0.0 };
    row[sound::MODE3D] = 0.0;
    row[sound::PRIORITY] = priority;
    row[sound::BIAS] = 0.5;
    row[sound::PROBABILITY] = 1.0;
    row
}

fn init_frame(rt: &mut Runtime, enhanced: bool) {
    let p = &mut rt.phase;
    p.started = true;
    p.engine.running = true;
    p.frame[frame::DT] = 1.0 / 60.0;
    p.frame[frame::RATE] = 48000.0;
    p.frame[frame::RUNNING] = 1.0;
    p.frame[frame::ENHANCED] = if enhanced { 1.0 } else { 0.0 };
    p.frame[frame::COLLISION] = 1.0;
    p.frame[frame::CHANNELS] = 2.0;
    p.frame[frame::MAX_VOICES] = 32.0;
    p.frame[frame::VIRTUAL_VOICES] = 64.0;
    p.frame[frame::TEAM] = 1.0;
    p.frame[frame::HUMAN] = 1.0;
    p.frame[frame::OWN] = 1.0;
    p.frame[frame::FZ] = -1.0;
    p.frame[frame::UY] = 1.0;
    p.frame[frame::EFFECTS_VOLUME] = 1.0;
    p.frame[frame::AMBIENCE_VOLUME] = 1.0;
    p.frame[frame::MUSIC_VOLUME] = 1.0;
    p.frame[frame::VOICE_VOLUME] = 1.0;
    p.frame[frame::INTERFACE_VOLUME] = 1.0;
    p.frame[frame::TICKETS1] = 100.0;
    p.frame[frame::TICKETS2] = 100.0;
}

fn step_n(rt: &mut Runtime, ticks: usize, t0: f64) -> f64 {
    let mut t = t0;
    let dt = 1.0 / 60.0;
    for _ in 0..ticks {
        t += dt;
        rt.clear_output();
        rt.phase.frame[frame::DT] = dt;
        rt.phase.frame[frame::NOW] = t;
        rt.phase.frame[frame::WALL_NOW] = t;
        rt.step(&mut OpenRoom);
    }
    t
}

fn make_synth_swbf2() -> Runtime {
    let mut rt = Runtime::new_for_family(42, GameFamily::Swbf2);
    let p = &mut rt.phase;

    // 10 synthetic PCM samples
    for i in 0..10 {
        p.engine.samples.push(Sample {
            render_id: i,
            duration: 0.4,
            rate: 48000.0,
            scale: 1.0,
            available: true,
        });
        p.engine.enhanced_samples.push(None);
    }

    // 3 streams: 0 = music, 1 = vo_primary (1 slot), 2 = vo_appended (bound to group 1)
    p.streams.definitions.push(audio_control::streams::Stream { slots: 1, channels: 2 });
    p.streams.definitions.push(audio_control::streams::Stream { slots: 1, channels: 1 });
    p.streams.definitions.push(audio_control::streams::Stream { slots: 1, channels: 1 });
    for s in 0..3 {
        p.streams.segments.push(audio_control::streams::Segment {
            stream: s,
            duration: 10.0,
            channels: if s == 0 { 2 } else { 1 },
            variants: [NONE; 4],
        });
    }
    p.streams.configure();
    p.swbf2_mut().unwrap().director.bind_stream_group(1, 1);
    p.swbf2_mut().unwrap().director.bind_stream_group(2, 1);

    // Sounds 0..20: plain FX
    // 0: blade_on, 1: blade_off, 2: blade_hum (loop), 3: blade_swing, 4: blade_hit, 5: blade_block, 6: blade_deflect
    // 7: boost_up, 8: boost_down, 9: trick, 10: flip, 11: cockpit_1p (loop), 12: cockpit_3p (loop)
    // 13: warm_up (loop), 14: barrel (loop), 15: bldg_hum (loop), 16: bldg_dmg, 17: door_open, 18: door_close, 19: door_locked
    // 20: engine_layer (loop), 21: engine_param (kind 3)
    // 22: music_stage0, 23: music_stage1, 24: music_stage2, 25: music_hero, 26: music_vehicle, 27: music_script_once (non-looping)
    // 28: vo_alpha (stream 1), 29: vo_beta (stream 2 appended to 1), 30: vo_hero_spawn (stream 1)
    // 31: vo_t1_defeat (stream 1), 32: vo_t1_victory (stream 1), 33: vo_t2_defeat (stream 2), 34: vo_t2_victory (stream 2)
    // 35: trigger_loop (loop)
    let looping_plain = [
        false, false, true, false, false, false, false, false, false, false, false, true, true, true, true, true,
        false, false, false, false, true,
    ];
    for (i, &is_loop) in looping_plain.iter().enumerate() {
        let mut row = plain_sound_row(0.8, is_loop, i != 11 && i != 12, 120.0);
        if i == 11 || i == 12 {
            row[sound::MODE3D] = 0.0;
        }
        p.engine.sounds.push(Sound::load(
            &row,
            vec![Entry { id: i % 10, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
        ));
        p.swbf2_mut().unwrap().sound_groups.push(if i == 16 { 0xAAAA_0001 } else { 0xBBBB_0002 });
    }
    // 21: parameterized engine
    let mut param_row = plain_sound_row(1.0, true, true, 180.0);
    param_row[sound::KIND] = 3.0;
    p.engine.sounds.push(Sound::load(&param_row, Vec::new()));
    p.swbf2_mut().unwrap().sound_groups.push(0xBBBB_0002);

    // 22..27: music streams
    for (prio, looping) in [(1.0, true), (1.0, true), (1.0, true), (2.0, true), (2.0, true), (2.0, false)] {
        let row = stream_sound_row(0, 0.9, prio, looping);
        p.engine.sounds.push(Sound::load(
            &row,
            vec![Entry { id: 0, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
        ));
        p.swbf2_mut().unwrap().sound_groups.push(0);
    }
    // 28..34: VO streams
    for st in [1, 2, 1, 1, 1, 2, 2] {
        let row = stream_sound_row(st, 0.9, 0.8, false);
        p.engine.sounds.push(Sound::load(
            &row,
            vec![Entry { id: st, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
        ));
        p.swbf2_mut().unwrap().sound_groups.push(0);
    }
    // 35: trigger region sound
    let trig_row = plain_sound_row(0.7, true, true, 90.0);
    p.engine.sounds.push(Sound::load(
        &trig_row,
        vec![Entry { id: 0, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
    ));
    p.swbf2_mut().unwrap().sound_groups.push(0);

    // Register names for hash-based APIs
    for (name, id) in [
        ("synth_swing", 3),
        ("synth_block", 5),
        ("synth_music_once", 27),
        ("synth_vo_alpha", 28),
        ("synth_vo_beta", 29),
        ("synth_vo_t1_defeat", 31),
        ("synth_vo_t1_victory", 32),
        ("synth_vo_t2_defeat", 33),
        ("synth_vo_t2_victory", 34),
    ] {
        p.sound_names.push((zero_hash(name), id));
    }

    // Layers definitions
    p.layers.definitions.resize_with(p.engine.sounds.len(), || audio_control::layers::Definition {
        layers: Vec::new(),
    });
    p.layers.definitions[21].layers.push(audio_control::layers::Layer {
        sound: 20,
        delay: 0.0,
        deviation: 0.0,
        bindings: vec![audio_control::layers::Binding {
            input: 0,
            pitch: true,
            points: vec![[0.0, 0.8], [1.0, 1.5]],
        }],
    });
    p.swbf2_mut().unwrap().vehicles.proximity_bindings.push(ProximityBinding {
        sound: 21,
        layer: 0,
        pitch: false,
        min_dist: 10.0,
        max_dist: 100.0,
        points: vec![[0.0, 0.25], [1.0, 1.0]],
    });

    // Music tracks: 0=stage0 (max_play 2.0s), 1=stage1, 2=stage2, 3=hero (prio 2), 4=vehicle (prio 2)
    p.director.tracks.push(audio_control::director::Track::new(22, 0.1, 0.2, 1.0, 0.05, 0.05));
    p.director.tracks.push(audio_control::director::Track::new(23, 0.1, 0.2, 1.0, 0.05, 0.05));
    p.director.tracks.push(audio_control::director::Track::new(24, 0.1, 0.2, 1.0, 0.05, 0.05));
    p.director.tracks.push(audio_control::director::Track::new(25, 0.1, 0.2, 2.0, 0.05, 0.05));
    p.director.tracks.push(audio_control::director::Track::new(26, 0.1, 0.2, 2.0, 0.05, 0.05));
    {
        let dir = &mut p.swbf2_mut().unwrap().director;
        dir.set_track_max_play(0, 2.0);
        for team in 1..=2 {
            dir.set_stage_slot(team, 0, 0, 1.0, true);
            dir.set_stage_slot(team, 1, 1, 0.99, true);
            dir.set_stage_slot(team, 2, 2, 0.10, true);
        }
    }

    // Classes:
    // 0: synth_actor (saber/chaingun/vehicle/building/door)
    // 1: synth_dmg_only (DamageEffectSound only, no BuildingSound)
    let mut cls_row = [-1.0; class::STRIDE];
    cls_row[class::ENGINE] = 21.0;
    p.classes.push(cls_row);
    p.classes.push([-1.0; class::STRIDE]);
    p.reaches.push(200.0);
    p.reaches.push(200.0);
    p.class_properties.push(Vec::new());
    p.class_properties.push(Vec::new());
    p.foot_definitions.push(Vec::new());
    p.foot_definitions.push(Vec::new());
    {
        let sw = p.swbf2_mut().unwrap();
        sw.classes.push(Swbf2Class {
            saber_turn_on: 0,
            saber_turn_off: 1,
            saber_on_hum: 2,
            saber_hit: 4,
            saber_deflect: 6,
            boost_cues: vec![
                BoostCue { sound: 7, threshold: 0.75, up: true },
                BoostCue { sound: 8, threshold: 0.5, up: false },
            ],
            trick_sound: 9,
            flip_sound: 10,
            cockpit_1p: 11,
            cockpit_3p: 12,
            warm_up: 13,
            warm_up_time: 0.5,
            warm_up_start_pitch: 0.5,
            warm_up_fade_in: 0.1,
            warm_up_fade_out: 0.1,
            barrel: 14,
            barrel_fade_in: 0.05,
            barrel_fade_out: 0.1,
            building_sound: 15,
            damage_effect_sound: 16,
            door_open: 17,
            door_close: 18,
            door_locked: 19,
            soldier_music: 3,
            rep_music: 4,
            cis_music: 4,
            hero_spawned: 30,
            hero_defeated: 30,
            hero_killer: 30,
            vo_unit_type: 99,
            vo_rules: vec![VoRule::new(28, zero_hash("synth_ev"), &[])],
            ..Swbf2Class::default()
        });
        sw.classes.push(Swbf2Class {
            damage_effect_sound: 16,
            ..Swbf2Class::default()
        });
        sw.register_class_hash(zero_hash("synth_actor"), 0);
        sw.register_class_hash(zero_hash("synth_dmg_only"), 1);

        // Combo machine `synth_combo`: state `synth_swing_a` triggers swing sound 3 after 0.1 s.
        let combo_hash = zero_hash("synth_combo");
        sw.combos.push(ComboSoundSet {
            combo_hash,
            states: vec![ComboStateSounds {
                state_hash: zero_hash("synth_swing_a"),
                triggers: vec![ComboSoundTrigger { sound: 3, delay_s: 0.1 }],
            }],
        });
        sw.register_combo_hash(combo_hash, 0);

        // Spaces: 0 = hall box, 1 = core cylinder; neighbor attenuation 0 -> 1 = 0.4
        sw.ambience.spaces.push(SpacePreset {
            hash: zero_hash("synth_hall"),
            send: 0.9,
            room: Room {
                room: 0.85,
                decay: 3.2,
                hf_ratio: 0.7,
                reflections: 0.4,
                reverb: 0.25,
                diffusion: 0.8,
                ..Room::default()
            },
            mask: 2047,
        });
        sw.ambience.spaces.push(SpacePreset {
            hash: zero_hash("synth_core"),
            send: 0.5,
            room: Room { room: 0.35, decay: 1.1, reflections: 0.3, reverb: 0.2, ..Room::default() },
            mask: 2047,
        });
        let mut box_mat = [0.0; 16];
        box_mat[0] = 1.0;
        box_mat[5] = 1.0;
        box_mat[10] = 1.0;
        box_mat[15] = 1.0;
        let mut cyl_mat = box_mat;
        cyl_mat[12] = 40.0;
        sw.ambience.space_regions.push(SpaceRegion {
            matrix: box_mat,
            size: [15.0, 10.0, 15.0],
            shape: RegionShape::Box,
            space: 0,
        });
        sw.ambience.space_regions.push(SpaceRegion {
            matrix: cyl_mat,
            size: [8.0, 12.0, 8.0],
            shape: RegionShape::Cylinder,
            space: 1,
        });
        sw.ambience.attenuations.push(SpaceAttenuation {
            from_space: 0,
            to_space: 1,
            gain: 0.4,
        });
        sw.ambience.triggers.push(TriggerRegion {
            hash: zero_hash("synth_trigger"),
            matrix: box_mat,
            size: [12.0, 8.0, 12.0],
            shape: RegionShape::Box,
            sound: 35,
            enabled: false,
            was_active: false,
            voice: SILENT,
            is_stream: false,
            retry: Default::default(),
        });

        let b0_hash = zero_hash("synth_reactor_a");
        let b1_hash = zero_hash("synth_panel_b");
        sw.vehicles.buildings.push(BuildingEmitter {
            name_hash: b0_hash,
            class: 0,
            position: [5.0, 0.0, -5.0],
            alive: true,
            damaged: false,
            in_range: false,
            voice: SILENT,
            retry: Default::default(),
        });
        sw.vehicles.buildings.push(BuildingEmitter {
            name_hash: b1_hash,
            class: 1,
            position: [6.0, 0.0, -4.0],
            alive: true,
            damaged: false,
            in_range: false,
            voice: SILENT,
            retry: Default::default(),
        });
        sw.register_building_hash(b0_hash, 0);
        sw.register_building_hash(b1_hash, 1);
        sw.finalize_lookups();
    }

    // Live hero emitter at slot 1, starfighter emitter at slot 2
    p.emitters[1][emitter::ID] = 1.0;
    p.emitters[1][emitter::KIND] = 1.0;
    p.emitters[1][emitter::ALIVE] = 1.0;
    p.emitters[1][emitter::GENERATION] = 1.0;
    p.emitters[1][emitter::CLASS] = 0.0;
    p.emitters[2][emitter::ID] = 2.0;
    p.emitters[2][emitter::KIND] = 3.0;
    p.emitters[2][emitter::ALIVE] = 1.0;
    p.emitters[2][emitter::GENERATION] = 1.0;
    p.emitters[2][emitter::CLASS] = 0.0;
    p.emitters[2][emitter::PILOTED] = 1.0;
    p.emitters[2][emitter::SPEED_LIMIT] = 100.0;
    p.emitters[2][emitter::X] = 20.0;
    p.entity_count = 3;

    init_frame(&mut rt, true);
    rt
}

#[test]
fn swbf1_family_seam_is_identical_and_ignores_swbf2_apis() {
    let mut a = Runtime::new(11);
    let mut b = Runtime::new_for_family(11, GameFamily::Swbf1);
    for rt in [&mut a, &mut b] {
        rt.phase.engine.samples.push(Sample {
            render_id: 0,
            duration: 0.5,
            rate: 48000.0,
            scale: 1.0,
            available: true,
        });
        rt.phase.engine.enhanced_samples.push(None);
        rt.phase.engine.sounds.push(Sound::load(
            &plain_sound_row(0.7, false, true, 80.0),
            vec![Entry { id: 0, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
        ));
        init_frame(rt, false);
    }
    b.phase.set_doppler_factor(0.4);
    b.phase.enable_hero_music(false);
    b.phase.push_saber_event(SaberEvent::Ignite {
        owner: 1,
        weapon_class_hash: zero_hash("synth_actor"),
    });
    assert_eq!(b.phase.family(), GameFamily::Swbf1);
    assert_eq!(b.phase.engine.doppler_factor, 1.0);
    assert!(b.phase.swbf2().is_none());

    a.phase.play(audio_control::engine::Ask::new(0, 0));
    b.phase.play(audio_control::engine::Ask::new(0, 0));
    step_n(&mut a, 10, 0.0);
    step_n(&mut b, 10, 0.0);
    assert_eq!(a.phase.engine.commands, b.phase.engine.commands);
    assert_eq!(a.phase.engine.placements, b.phase.engine.placements);
}

#[test]
fn saber_ignite_hum_delayed_combo_hit_block_deflect_and_extinguish() {
    let mut rt = make_synth_swbf2();
    let actor_h = zero_hash("synth_actor");
    rt.phase.push_saber_event(SaberEvent::Ignite { owner: 1, weapon_class_hash: actor_h });
    let t = step_n(&mut rt, 1, 0.0);
    assert!(rt.phase.swbf2().unwrap().sabers.blade_active(1));
    assert!(rt.phase.playing(rt.phase.swbf2().unwrap().sabers.hum_voice(1)));

    // Trigger delayed combo swing via machine + state hash (0.1 s = 6 ticks at 60 Hz).
    let before_starts = rt.phase.engine.stats[2];
    rt.phase.push_saber_event(SaberEvent::ComboState {
        owner: 1,
        combo_hash: zero_hash("synth_combo"),
        state_hash: zero_hash("synth_swing_a"),
    });
    let t = step_n(&mut rt, 3, t);
    assert_eq!(rt.phase.engine.stats[2], before_starts, "delayed swing must wait until 0.1 s elapses");
    let t = step_n(&mut rt, 4, t);
    assert!(rt.phase.engine.stats[2] > before_starts, "delayed swing must fire after 0.1 s");

    // Hit, block, deflect
    let hits_before = rt.phase.engine.stats[2];
    rt.phase.push_saber_event(SaberEvent::Hit { owner: 1, weapon_class_hash: actor_h, position: [1.0, 1.0, -2.0] });
    rt.phase.push_saber_event(SaberEvent::Block {
        owner: 1,
        sound_hash: zero_hash("synth_block"),
        position: [1.0, 1.0, -2.0],
    });
    rt.phase.push_saber_event(SaberEvent::Deflect { owner: 1, weapon_class_hash: actor_h, position: [1.0, 1.0, -2.0] });
    let t = step_n(&mut rt, 1, t);
    assert_eq!(rt.phase.engine.stats[2] - hits_before, 3);

    // Extinguish silences blade hum and plays TurnOffSound.
    let hum = rt.phase.swbf2().unwrap().sabers.hum_voice(1);
    rt.phase.push_saber_event(SaberEvent::Extinguish { owner: 1, weapon_class_hash: actor_h });
    step_n(&mut rt, 1, t);
    assert!(!rt.phase.swbf2().unwrap().sabers.blade_active(1));
    assert!(!rt.phase.playing(hum));
}

#[test]
fn starfighter_boost_proximity_tricks_cockpit_chaingun_buildings_doors_and_scaling() {
    let mut rt = make_synth_swbf2();
    let actor_h = zero_hash("synth_actor");
    rt.phase.set_doppler_factor(0.4);
    assert!((rt.phase.engine.doppler_factor - 0.4).abs() < 1e-12);
    rt.phase.scale_sound_parameter(0xAAAA_0001, SoundParam::MaxDistance, 5.0);
    assert!((rt.phase.engine.sounds[16].curve.max - 600.0).abs() < 1e-9);

    // Verify scaled Doppler still shifts a 200 m/s radial velocity at factor 0.4 (scaled = 80 m/s < 172 m/s).
    let vid = rt.phase.play(audio_control::engine::Ask {
        sound: 0,
        category: 0,
        emitter: NONE,
        position: Some([0.0, 0.0, -10.0]),
        looping: Some(true),
        gain: 1.0,
        pitch: 1.0,
        curve: None,
        protected: false,
    });
    let idx = (vid % audio_control::engine::CAPACITY as u64) as usize;
    rt.phase.engine.voices[idx].radial = 200.0;
    let shifted = rt.phase.engine.rate(idx);
    assert!(shifted > 1.20 && shifted < 1.26, "scaled 200 m/s * 0.4 must shift pitch, got {shifted}");
    rt.phase.stop(vid, 0.01);

    // Initial tick turns vehicle engine on and starts ambient building sound.
    let t = step_n(&mut rt, 2, 0.0);
    assert!(rt.phase.playing(rt.phase.swbf2().unwrap().vehicles.buildings[0].voice));
    // Building 1 has only DamageEffectSound (no ambient BuildingSound loop).
    assert_eq!(rt.phase.swbf2().unwrap().vehicles.buildings[1].voice, SILENT);

    // Cross boost-up threshold (0.75 * 100 = 75 m/s).
    let before = rt.phase.engine.stats[2];
    rt.phase.emitters[2][emitter::VX] = 85.0;
    let t = step_n(&mut rt, 1, t);
    assert!(rt.phase.engine.stats[2] > before, "boost up cue should fire on threshold crossing");

    // Cross boost-down threshold (0.5 * 100 = 50 m/s).
    let before = rt.phase.engine.stats[2];
    rt.phase.emitters[2][emitter::VX] = 30.0;
    let t = step_n(&mut rt, 1, t);
    assert!(rt.phase.engine.stats[2] > before, "boost down cue should fire on deceleration");

    // Destroying a building with NO ambient loop must still fire DamageEffectSound.
    let before = rt.phase.engine.stats[2];
    rt.phase.set_building_alive_swbf2(zero_hash("synth_panel_b"), false);
    let t = step_n(&mut rt, 1, t);
    assert_eq!(
        rt.phase.engine.stats[2] - before,
        1,
        "DamageEffectSound must play even when no BuildingSound loop was live"
    );

    // Trick, flip, cockpit, chaingun warmup/barrel, building 0 destruction, doors.
    let before = rt.phase.engine.stats[2];
    rt.phase.push_vehicle_event(VehicleEvent::Trick { owner: 2, class_hash: actor_h });
    rt.phase.push_vehicle_event(VehicleEvent::Flip { owner: 2, class_hash: actor_h });
    rt.phase.set_own_mount_swbf2(Some(actor_h), true);
    rt.phase.update_chaingun(ChaingunInput {
        weapon: 5,
        owner: 1,
        class_hash: actor_h,
        spinning: true,
        firing: true,
    });
    rt.phase.push_door_event(DoorEvent::Open { class_hash: actor_h, position: [2.0, 0.0, -2.0] });
    rt.phase.push_door_event(DoorEvent::Close { class_hash: actor_h, position: [2.0, 0.0, -2.0] });
    rt.phase.push_door_event(DoorEvent::Locked { class_hash: actor_h, position: [2.0, 0.0, -2.0] });
    rt.phase.set_building_alive_swbf2(zero_hash("synth_reactor_a"), false);
    step_n(&mut rt, 1, t);
    assert!(rt.phase.engine.stats[2] >= before + 7);
    assert_eq!(rt.phase.swbf2().unwrap().vehicles.buildings[0].voice, SILENT);
}

#[test]
fn enhanced_interior_reverb_blend_never_compounds_across_ticks() {
    let mut rt = make_synth_swbf2();
    let preset = rt.phase.swbf2().unwrap().ambience.spaces[0];

    // Step 50 ticks (spanning multiple 10-tick ray probes) inside space 0.
    let mut t = 0.0;
    for tick in 0..50 {
        t = step_n(&mut rt, 1, t);
        let measured = rt.phase.acoustics.room;
        let expected = blend_interior_room(measured, preset);
        // Published info::ROOM must equal the exact one-step blend of raw measured acoustics and preset.
        assert!(
            (rt.info[info::ROOM] - expected.room).abs() < 1e-12,
            "tick {tick}: room drifted: got {}, expected {}",
            rt.info[info::ROOM],
            expected.room
        );
        assert!(
            (rt.info[info::DECAY] - expected.decay).abs() < 1e-12,
            "tick {tick}: decay drifted: got {}, expected {}",
            rt.info[info::DECAY],
            expected.decay
        );
        assert!(
            (rt.info[info::REVERB] - expected.reverb).abs() < 1e-12,
            "tick {tick}: reverb drifted: got {}, expected {}",
            rt.info[info::REVERB],
            expected.reverb
        );
        assert!(
            (rt.info[info::DIFFUSION] - expected.diffusion).abs() < 1e-12,
            "tick {tick}: diffusion drifted: got {}, expected {}",
            rt.info[info::DIFFUSION],
            expected.diffusion
        );
        // Raw acoustics.room must NOT have drifted to the preset values (reverb != preset.reverb).
        assert!(
            (measured.reverb - preset.room.reverb).abs() > 0.05,
            "tick {tick}: raw acoustics.room.reverb was mutated toward preset ({})",
            measured.reverb
        );
    }

    // Leaving the space reverts immediately to raw measured acoustics and flags room change.
    rt.phase.frame[frame::PX] = 100.0;
    step_n(&mut rt, 1, t);
    assert_eq!(rt.phase.swbf2().unwrap().ambience.current_space, None);
    assert_eq!(rt.info[info::ROOM_CHANGED] as u32, 2047);
    assert!((rt.info[info::DECAY] - rt.phase.acoustics.room.decay).abs() < 1e-12);
}

#[test]
fn interior_spaces_cylinder_region_attenuation_and_trigger_regions() {
    let mut rt = make_synth_swbf2();
    let trig_h = zero_hash("synth_trigger");
    let t = step_n(&mut rt, 1, 0.0);
    assert_eq!(rt.phase.swbf2().unwrap().ambience.current_space, Some(0));
    assert_eq!(rt.phase.swbf2().unwrap().ambience.triggers[0].voice, SILENT);
    assert!(rt.info[info::DECAY] > 1.5, "published enhanced room should blend interior decay");
    assert!((rt.phase.swbf2().unwrap().ambience.voice_space_gain(Some(0), [40.0, 0.0, 0.0]) - 0.4).abs() < 1e-9);

    rt.phase.trigger_sound_region_enable(trig_h, true);
    let t = step_n(&mut rt, 1, t);
    assert!(rt.phase.playing(rt.phase.swbf2().unwrap().ambience.triggers[0].voice));

    // Move listener into cylinder space 1 (centered at x = 40, r = 8).
    rt.phase.frame[frame::PX] = 40.0;
    rt.phase.frame[frame::PZ] = 2.0;
    let t = step_n(&mut rt, 1, t);
    assert_eq!(rt.phase.swbf2().unwrap().ambience.current_space, Some(1));
    assert_eq!(rt.phase.swbf2().unwrap().ambience.triggers[0].voice, SILENT, "leaving trigger stops its sound");

    rt.phase.trigger_sound_region_enable(trig_h, false);
    rt.phase.frame[frame::PX] = 100.0;
    step_n(&mut rt, 1, t);
    assert_eq!(rt.phase.swbf2().unwrap().ambience.current_space, None);
}

#[test]
fn points_mode_music_sweeps_raw_fraction_stages_even_after_ticket_slot_reimport() {
    let mut rt = make_synth_swbf2();
    rt.phase.enable_hero_music(false);

    // Simulate production `Sound::frame` re-importing `MUSIC_SLOT` with START = 100.0.
    for (stage, amount) in [(0usize, 1.0), (1, 0.99), (2, 0.10)] {
        let mut row = [0.0; music_slot::STRIDE];
        row[music_slot::TEAM] = 1.0;
        row[music_slot::STAGE] = stage as f64;
        row[music_slot::TRACK] = stage as f64;
        row[music_slot::AMOUNT] = amount;
        row[music_slot::FRACTION] = 1.0;
        row[music_slot::START] = 100.0;
        rt.input[..music_slot::STRIDE].copy_from_slice(&row);
        rt.import(import::MUSIC_SLOT, music_slot::STRIDE);
    }

    // Progress 1.0 and 0.995 -> stage 0 (`track 0`).
    rt.phase.set_mode_score_progress(1.0, 1.0);
    let t = step_n(&mut rt, 2, 0.0);
    assert_eq!(rt.phase.director.stage, 0);
    assert!(rt.phase.director.tracks[0].playing());

    rt.phase.set_mode_score_progress(0.995, 0.995);
    let t = step_n(&mut rt, 10, t);
    assert_eq!(rt.phase.director.stage, 0);
    assert!(rt.phase.director.tracks[0].playing());

    // Cross 0.99 -> stage 1 (`track 1`), after `min_play` elapses.
    rt.phase.set_mode_score_progress(0.95, 0.95);
    let t = step_n(&mut rt, 20, t);
    assert_eq!(rt.phase.director.stage, 1);
    assert!(rt.phase.director.tracks[1].playing(), "stage 1 track must take over below 0.99");
    assert!(!rt.phase.director.tracks[0].playing());

    // Still stage 1 at 0.15.
    rt.phase.set_mode_score_progress(0.15, 0.50);
    let t = step_n(&mut rt, 10, t);
    assert_eq!(rt.phase.director.stage, 1);
    assert!(rt.phase.director.tracks[1].playing());

    // Cross 0.10 -> stage 2 (`track 2`).
    rt.phase.set_mode_score_progress(0.09, 0.09);
    step_n(&mut rt, 20, t);
    assert_eq!(rt.phase.director.stage, 2);
    assert!(rt.phase.director.tracks[2].playing(), "stage 2 track must take over below 0.10");
    assert!(!rt.phase.director.tracks[1].playing());
}

#[test]
fn asymmetric_team_progress_follows_last_team_wins_stage_rule() {
    // Points mode: listener on trailing team 1 (0.05), team 2 still at 1.0.
    // Team 2 is visited last (matching the shared director rule), so stage stays 0.
    let mut rt = make_synth_swbf2();
    rt.phase.enable_hero_music(false);
    rt.phase.frame[frame::TEAM] = 1.0;
    rt.phase.set_mode_score_progress(0.05, 1.0);
    let t = step_n(&mut rt, 2, 0.0);
    assert_eq!(rt.phase.director.stage, 0, "team 2 at 1.0 overwrites team 1 stage 2");

    // Flip asymmetry: team 1 at 1.0, team 2 at 0.05 -> team 2 visited last selects stage 2.
    rt.phase.set_mode_score_progress(1.0, 0.05);
    step_n(&mut rt, 20, t);
    assert_eq!(rt.phase.director.stage, 2, "team 2 at 0.05 selects stage 2");

    // Ticket mode: same last-team-wins rule with listener on trailing team 1.
    let mut rt_tickets = make_synth_swbf2();
    rt_tickets.phase.enable_hero_music(false);
    rt_tickets.phase.frame[frame::TEAM] = 1.0;
    let t = step_n(&mut rt_tickets, 1, 0.0);
    rt_tickets.phase.frame[frame::TICKETS1] = 5.0;
    rt_tickets.phase.frame[frame::TICKETS2] = 100.0;
    let t = step_n(&mut rt_tickets, 20, t);
    assert_eq!(rt_tickets.phase.director.stage, 0, "team 2 full tickets overwrites team 1 low tickets");

    rt_tickets.phase.frame[frame::TICKETS1] = 100.0;
    rt_tickets.phase.frame[frame::TICKETS2] = 5.0;
    step_n(&mut rt_tickets, 20, t);
    assert_eq!(rt_tickets.phase.director.stage, 2, "team 2 low tickets selects stage 2");
}

#[test]
fn low_reinforcements_vo_honors_distinct_listener_and_watched_teams() {
    let run_case = |listener: usize, t1: f64, t2: f64| -> Vec<usize> {
        let mut rt = make_synth_swbf2();
        rt.phase.frame[frame::TEAM] = listener as f64;
        // Register all 4 rows: listener 1 watches 1 (defeat) & 2 (victory); listener 2 watches 2 (defeat) & 1 (victory).
        rt.phase.set_low_reinforcements_vo(1, 0.10, zero_hash("synth_vo_t1_defeat"), true, 1);
        rt.phase.set_low_reinforcements_vo(2, 0.10, zero_hash("synth_vo_t1_victory"), true, 1);
        rt.phase.set_low_reinforcements_vo(2, 0.10, zero_hash("synth_vo_t2_defeat"), true, 2);
        rt.phase.set_low_reinforcements_vo(1, 0.10, zero_hash("synth_vo_t2_victory"), true, 2);

        // Tick 0 captures initial tickets (100, 100); nothing fires yet.
        let mut t = step_n(&mut rt, 1, 0.0);
        rt.phase.frame[frame::TICKETS1] = t1;
        rt.phase.frame[frame::TICKETS2] = t2;
        for _ in 0..2 {
            t = step_n(&mut rt, 1, t);
            let reqs = rt.phase.streams.requests.clone();
            for req in reqs {
                if req.operation == request_kind::LOAD {
                    let input = &mut rt.input[..ready::STRIDE];
                    input.fill(0.0);
                    input[ready::VOICE] = req.voice as f64;
                    input[ready::GENERATION] = req.generation as f64;
                    input[ready::PART] = req.part as f64;
                    input[ready::SAMPLE] = req.sample as f64;
                    input[ready::TIME] = t;
                    input[ready::WALL_NOW] = t;
                    input[ready::DURATION] = 2.0;
                    input[ready::CHANNELS] = 1.0;
                    rt.ready();
                }
            }
        }

        let mut started = Vec::new();
        for (sound, cat) in rt.phase.streams.census() {
            if cat == audio_control::engine::VOICE && sound != NONE {
                started.push(sound);
            }
        }
        for q in &rt.phase.swbf2().unwrap().director.logical_queues {
            for item in q.iter().flatten() {
                started.push(item.options.ask.sound);
            }
        }
        started
    };

    assert_eq!(run_case(1, 5.0, 80.0), vec![31], "team 1 low -> team 1 hears t1_defeat only");
    assert_eq!(run_case(1, 80.0, 5.0), vec![32], "team 2 low -> team 1 hears t1_victory only");
    assert_eq!(run_case(2, 80.0, 5.0), vec![33], "team 2 low -> team 2 hears t2_defeat only");
    assert_eq!(run_case(2, 5.0, 80.0), vec![34], "team 1 low -> team 2 hears t2_victory only");
}

#[test]
fn music_max_playback_time_hero_music_script_music_and_logical_vo_queue() {
    let mut rt = make_synth_swbf2();
    let actor_h = zero_hash("synth_actor");
    rt.phase.set_mode_score_progress(1.0, 1.0);
    rt.phase.enable_hero_music(false);
    rt.phase.set_own_hero_swbf2(Some(actor_h));

    // Stage 0 starts immediately (hero music disabled).
    let t = step_n(&mut rt, 1, 0.0);
    assert!(rt.phase.director.tracks[0].playing());
    assert!(!rt.phase.director.tracks[3].playing());

    // After 2.0 s (`MaxPlaybackTime`), stage 0 stops to idle.
    let t = step_n(&mut rt, 125, t);
    assert!(!rt.phase.director.tracks[0].playing(), "track 0 should stop at MaxPlaybackTime");

    // Enable hero music -> priority 2.0 hero track takes over.
    rt.phase.enable_hero_music(true);
    let t = step_n(&mut rt, 20, t);
    assert!(rt.phase.director.tracks[3].playing(), "hero music should start once enabled");

    // Non-looping PlayInGameMusic starts once and clears automatically if its stream ends.
    rt.phase.play_in_game_music(zero_hash("synth_music_once"));
    let t = step_n(&mut rt, 1, t);
    let script_voice = rt.phase.swbf2().unwrap().director.script_music_voice;
    assert_ne!(script_voice, SILENT);
    rt.phase.streams.stop(&mut rt.phase.engine, script_voice, 0.0);
    let t = step_n(&mut rt, 2, t);
    assert!(
        rt.phase.swbf2().unwrap().director.script_music_sound.is_none(),
        "non-looping script music must not restart forever once finished"
    );

    // Logical stream group sharing: queue two VO lines on stream 1 and appended stream 2.
    rt.phase.broadcast_voice_over(zero_hash("synth_vo_alpha"), 1);
    rt.phase.broadcast_voice_over(zero_hash("synth_vo_beta"), 1);
    let t = step_n(&mut rt, 1, t);
    let active_group1 = rt.phase.streams.active_in_group(|s| s == 1 || s == 2);
    assert_eq!(active_group1, 1, "appended stream must share the single playback slot");
    assert!(
        rt.phase.swbf2().unwrap().director.logical_queues[1][0].is_some(),
        "second line must wait in shared queue"
    );

    // Hero VO gating: disabled suppresses, enabled queues.
    rt.phase.enable_hero_vo(false);
    rt.phase.notify_hero_event(actor_h, HeroVoKind::Spawned);
    let t = step_n(&mut rt, 1, t);
    assert!(rt.phase.swbf2().unwrap().director.logical_queues[1][1].is_none());
    rt.phase.enable_hero_vo(true);
    rt.phase.notify_hero_event(actor_h, HeroVoKind::Spawned);
    step_n(&mut rt, 1, t);
    assert!(rt.phase.swbf2().unwrap().director.logical_queues[1][1].is_some());
}

#[test]
fn swbf2_steady_state_tick_makes_zero_heap_allocations() {
    let mut rt = make_synth_swbf2();
    let actor_h = zero_hash("synth_actor");
    let swing_h = zero_hash("synth_swing");
    let vo_h = zero_hash("synth_vo_alpha");
    rt.phase.set_mode_score_progress(0.8, 0.8);
    rt.phase.trigger_sound_region_enable(zero_hash("synth_trigger"), true);
    rt.phase.push_saber_event(SaberEvent::Ignite { owner: 1, weapon_class_hash: actor_h });
    rt.phase.set_own_mount_swbf2(Some(actor_h), true);
    // Warm up initial ticks.
    let mut t = step_n(&mut rt, 3, 0.0);

    let allocs = measure_allocs(|| {
        for i in 0..300 {
            rt.phase.emitters[2][emitter::VX] = if i % 20 < 10 { 85.0 } else { 30.0 };
            rt.phase.push_saber_event(SaberEvent::Swing { owner: 1, sound_hash: swing_h, delay_s: 0.05 });
            rt.phase.push_vehicle_event(VehicleEvent::Trick { owner: 2, class_hash: actor_h });
            rt.phase.push_door_event(DoorEvent::Open { class_hash: actor_h, position: [1.0, 0.0, -1.0] });
            rt.phase.update_chaingun(ChaingunInput {
                weapon: 5,
                owner: 1,
                class_hash: actor_h,
                spinning: true,
                firing: i % 2 == 0,
            });
            rt.phase.broadcast_voice_over(vo_h, 1);
            rt.phase.play_vo_tag(
                actor_h,
                zero_hash("synth_ev"),
                &[zero_hash("cond_a"), zero_hash("cond_b")],
                None,
                false,
            );
            t = step_n(&mut rt, 1, t);
        }
    });
    assert_eq!(allocs, 0, "2005 audio control step must perform zero heap allocations over 300 steady-state ticks");
}

#[test]
fn vo_tag_matches_multi_condition_rules_and_prefers_most_specific() {
    let mut rt = make_synth_swbf2();
    let actor_h = zero_hash("synth_actor");
    let ev_h = zero_hash("synth_sight");
    let c_a = zero_hash("tag_alpha");
    let c_b = zero_hash("tag_beta");
    let c_c = zero_hash("tag_gamma");

    // Authored order places broader rules before narrower 3-condition rules.
    rt.phase.swbf2_mut().unwrap().classes[0].vo_rules = vec![
        VoRule::new(28, ev_h, &[]),
        VoRule::new(29, ev_h, &[c_a]),
        VoRule::new(31, ev_h, &[c_a, c_b]),
        VoRule::new(32, ev_h, &[c_a, c_c]),
        VoRule::new(33, ev_h, &[c_a, c_b, c_c]),
    ];

    let pick = |rt: &mut Runtime, conds: &[u32]| -> usize {
        rt.phase.swbf2_mut().unwrap().director.logical_queues.fill([None; 4]);
        assert!(rt.phase.play_vo_tag(actor_h, ev_h, conds, None, false));
        rt.phase.swbf2().unwrap().director.logical_queues[1][0]
            .expect("VO line queued")
            .options
            .ask
            .sound
    };

    assert_eq!(pick(&mut rt, &[]), 28, "empty active set picks unconditional rule");
    assert_eq!(pick(&mut rt, &[c_a]), 29, "single condition picks 1-condition rule");
    assert_eq!(pick(&mut rt, &[c_a, c_b]), 31, "two conditions pick matching 2-condition rule");
    assert_eq!(
        pick(&mut rt, &[c_c, c_a]),
        32,
        "rules differing after first condition disambiguate regardless of caller order"
    );
    assert_eq!(
        pick(&mut rt, &[c_c, c_b, c_a]),
        33,
        "three active conditions pick 3-condition rule instead of being shadowed by broader rules"
    );

    // Equal-specificity ties keep authored order (rule 31 appears before rule 32).
    assert_eq!(
        pick(&mut rt, &[c_a, c_b, c_c]),
        33,
        "three-condition rule beats both two-condition rules"
    );
    rt.phase.swbf2_mut().unwrap().classes[0].vo_rules.pop();
    assert_eq!(
        pick(&mut rt, &[c_a, c_b, c_c]),
        31,
        "equal-specificity two-condition tie preserves authored order"
    );

    // Rules with 8 distinct conditions (plus duplicates) succeed; 9 distinct conditions are rejected.
    let eight = [1u32, 2, 3, 4, 5, 6, 7, 8, 1];
    let nine = [1u32, 2, 3, 4, 5, 6, 7, 8, 9];
    assert_eq!(VoRule::try_new(28, ev_h, &eight).map(|r| r.conditions().len()), Some(8));
    assert!(VoRule::try_new(28, ev_h, &nine).is_none());
}

#[test]
fn music_director_supports_track_ids_at_or_above_64() {
    let mut rt = make_synth_swbf2();
    rt.phase.enable_hero_music(false);
    // Push extra tracks so ambient stage 0 sits at index 66 and hero track sits at index 70.
    while rt.phase.director.tracks.len() <= 70 {
        let prio = if rt.phase.director.tracks.len() == 70 { 2.0 } else { 1.0 };
        rt.phase
            .director
            .tracks
            .push(audio_control::director::Track::new(22, 0.1, 0.2, prio, 0.05, 0.05));
    }
    for team in 1..=2 {
        rt.phase
            .swbf2_mut()
            .unwrap()
            .director
            .set_stage_slot(team, 0, 66, 1.0, true);
    }
    rt.phase.swbf2_mut().unwrap().classes[0].soldier_music = 70;
    rt.phase.set_mode_score_progress(1.0, 1.0);

    let t = step_n(&mut rt, 2, 0.0);
    assert!(rt.phase.director.tracks[66].playing(), "ambient track id 66 must start and play");

    rt.phase.set_own_hero_swbf2(Some(zero_hash("synth_actor")));
    rt.phase.enable_hero_music(true);
    step_n(&mut rt, 20, t);
    assert!(rt.phase.director.tracks[70].playing(), "hero track id 70 must preempt ambient track 66");
    assert!(!rt.phase.director.tracks[66].playing());
}

#[test]
fn swbf2_fraction_thresholds_use_true_starting_tickets_on_mid_battle_join() {
    let mut rt = make_synth_swbf2();
    rt.phase.enable_hero_music(false);
    rt.phase.frame[frame::TEAM] = 1.0;
    rt.phase.set_low_reinforcements_vo(1, 0.10, zero_hash("synth_vo_t1_defeat"), true, 1);

    // Join mid-battle at 75 / 150 tickets with true starting counts plumbed.
    rt.phase.set_starting_tickets(150.0, 150.0);
    rt.phase.frame[frame::TICKETS1] = 75.0;
    rt.phase.frame[frame::TICKETS2] = 75.0;
    let t = step_n(&mut rt, 2, 0.0);
    assert_eq!(rt.phase.director.stage, 1, "75/150 is below 0.99 -> stage 1");
    assert!(!rt.phase.swbf2().unwrap().director.low_reinforcements[0].fired);

    // 16 / 150 stays in stage 1 and does not fire 10% VO.
    rt.phase.frame[frame::TICKETS1] = 16.0;
    rt.phase.frame[frame::TICKETS2] = 16.0;
    let t = step_n(&mut rt, 2, t);
    assert_eq!(rt.phase.director.stage, 1);
    assert!(!rt.phase.swbf2().unwrap().director.low_reinforcements[0].fired);

    // 15 / 150 hits 10% of true start (150), advancing to stage 2 and firing low-reinforcements VO.
    rt.phase.frame[frame::TICKETS1] = 15.0;
    rt.phase.frame[frame::TICKETS2] = 15.0;
    step_n(&mut rt, 2, t);
    assert_eq!(rt.phase.director.stage, 2, "15/150 must enter 0.10 stage 2");
    assert!(
        rt.phase.swbf2().unwrap().director.low_reinforcements[0].fired,
        "15/150 must fire 0.10 low-reinforcements VO"
    );
}

#[test]
fn unplayable_script_music_leaves_ambient_running_even_with_long_min_inactive_time() {
    let mut rt = make_synth_swbf2();
    rt.phase.enable_hero_music(false);
    rt.phase.set_mode_score_progress(1.0, 1.0);
    // Realistic stock MinInactiveTime values (30 s and 90 s): stopping ambient would leave long silence.
    rt.phase.director.tracks[0] = audio_control::director::Track::new(22, 0.1, 30.0, 1.0, 0.05, 0.05);
    rt.phase.director.tracks[1] = audio_control::director::Track::new(23, 0.1, 90.0, 1.0, 0.05, 0.05);
    rt.phase.swbf2_mut().unwrap().director.set_track_max_play(0, f64::INFINITY);

    // Start ambient stage 0.
    let t = step_n(&mut rt, 2, 0.0);
    assert!(rt.phase.director.tracks[0].playing());

    // Case 1: non-stream FX sound is ignored without interrupting ambient.
    rt.phase.play_in_game_music(zero_hash("synth_swing"));
    let t = step_n(&mut rt, 2, t);
    assert!(rt.phase.swbf2().unwrap().director.script_music_sound.is_none());
    assert!(rt.phase.director.tracks[0].playing(), "non-stream request must leave ambient playing");

    // Case 2: stream sound with empty segment list is ignored without interrupting ambient.
    let empty_id = rt.phase.engine.sounds.len();
    let empty_row = stream_sound_row(0, 0.9, 2.0, false);
    rt.phase.engine.sounds.push(Sound::load(&empty_row, Vec::new()));
    rt.phase.swbf2_mut().unwrap().sound_groups.push(0);
    rt.phase.sound_names.push((zero_hash("synth_music_empty"), empty_id));

    rt.phase.play_in_game_music(zero_hash("synth_music_empty"));
    let t = step_n(&mut rt, 2, t);
    assert!(rt.phase.swbf2().unwrap().director.script_music_sound.is_none());
    assert!(
        rt.phase.director.tracks[0].playing(),
        "empty-segment script music must not stop running ambient for its 30 s MinInactiveTime"
    );

    // Case 3: stream sound on a 0-slot stream clears without stopping running ambient.
    let zero_stream = rt.phase.streams.definitions.len();
    rt.phase.streams.definitions.push(audio_control::streams::Stream { slots: 0, channels: 2 });
    let seg_zero = rt.phase.streams.segments.len();
    rt.phase.streams.segments.push(audio_control::streams::Segment {
        stream: zero_stream,
        duration: 10.0,
        channels: 2,
        variants: [NONE; 4],
    });
    rt.phase.streams.configure();
    let zero_id = rt.phase.engine.sounds.len();
    let zero_row = stream_sound_row(zero_stream, 0.9, 2.0, true);
    rt.phase.engine.sounds.push(Sound::load(
        &zero_row,
        vec![Entry { id: seg_zero, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
    ));
    rt.phase.swbf2_mut().unwrap().sound_groups.push(0);
    rt.phase.sound_names.push((zero_hash("synth_music_noslot"), zero_id));

    rt.phase.play_in_game_music(zero_hash("synth_music_noslot"));
    let t = step_n(&mut rt, 2, t);
    assert!(rt.phase.swbf2().unwrap().director.script_music_sound.is_none());
    assert!(
        rt.phase.director.tracks[0].playing(),
        "unopenable stream script music must not stop running ambient"
    );

    // Case 4: non-finite threshold in set_low_reinforcements_vo is ignored.
    let before_rules = rt.phase.swbf2().unwrap().director.low_reinforcements.len();
    rt.phase.set_low_reinforcements_vo(1, f64::NAN, zero_hash("synth_vo_t1_defeat"), true, 1);
    rt.phase.set_low_reinforcements_vo(1, f64::INFINITY, zero_hash("synth_vo_t1_defeat"), true, 1);
    assert_eq!(rt.phase.swbf2().unwrap().director.low_reinforcements.len(), before_rules);
    let _ = t;
}

#[test]
fn repeated_play_in_game_music_is_idempotent_and_failing_loop_does_not_retry() {
    let mut rt = make_synth_swbf2();
    rt.phase.enable_hero_music(false);
    rt.phase.set_mode_score_progress(1.0, 1.0);

    // Repeated calls every tick for the same playable music track must start once and keep the same voice.
    let mut t = 0.0;
    let mut first_voice = SILENT;
    let mut total_loads = 0usize;
    for _ in 0..600 {
        rt.phase.play_in_game_music(zero_hash("synth_music_once"));
        t = step_n(&mut rt, 1, t);
        for req in &rt.phase.streams.requests {
            if req.operation == request_kind::LOAD {
                total_loads += 1;
            }
        }
        let voice = rt.phase.swbf2().unwrap().director.script_music_voice;
        if first_voice == SILENT {
            assert_ne!(voice, SILENT);
            first_voice = voice;
        } else {
            assert_eq!(voice, first_voice, "repeated play_in_game_music must not stop and restart its voice");
        }
    }
    assert_eq!(total_loads, 1, "600 repeated play_in_game_music calls must issue only one stream load");

    // A looping script music request whose segment fails asynchronously must remember failure,
    // clear its script request, and never loop or exhaust voices.
    rt.phase.stop_in_game_music(0.0);
    let failing_seg = rt.phase.streams.segments.len();
    rt.phase.streams.segments.push(audio_control::streams::Segment {
        stream: 0,
        duration: 10.0,
        channels: 2,
        variants: [NONE; 4],
    });
    rt.phase.streams.configure();
    let loop_id = rt.phase.engine.sounds.len();
    let loop_row = stream_sound_row(0, 0.9, 2.0, true);
    rt.phase.engine.sounds.push(Sound::load(
        &loop_row,
        vec![Entry { id: failing_seg, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }],
    ));
    rt.phase.swbf2_mut().unwrap().sound_groups.push(0);
    rt.phase.sound_names.push((zero_hash("synth_music_loop_fail"), loop_id));

    rt.phase.play_in_game_music(zero_hash("synth_music_loop_fail"));
    let mut fail_loads = 0usize;
    for _ in 0..120 {
        t = step_n(&mut rt, 1, t);
        let reqs = rt.phase.streams.requests.clone();
        for req in reqs {
            if req.operation == request_kind::LOAD && req.segment == failing_seg {
                fail_loads += 1;
                assert!(rt.phase.streams.failed(&rt.phase.engine, req.voice, req.generation));
            }
        }
    }
    assert_eq!(fail_loads, 1, "failing looping script music must not retry after initial failure");
    assert!(
        rt.phase.swbf2().unwrap().director.script_music_sound.is_none(),
        "failing looping script music must clear its request"
    );
    assert!(
        rt.phase.director.tracks[0].playing(),
        "ambient music must resume after failing looping script music clears"
    );
}

#[test]
fn ambient_music_owns_its_slot_until_the_script_segment_starts() {
    for (blocked, fails) in [(true, false), (false, true), (false, false)] {
        let mut rt = make_synth_swbf2();
        rt.phase.enable_hero_music(false);
        rt.phase.set_mode_score_progress(1.0, 1.0);
        rt.phase.streams.definitions[0].slots = 1;
        rt.phase.swbf2_mut().unwrap().director.set_track_max_play(0, f64::INFINITY);
        let mut t = step_n(&mut rt, 1, 0.0);
        let ambient = *rt.phase.streams.requests.iter().find(|r| r.operation == request_kind::LOAD).unwrap();
        let p = &mut rt.phase;
        assert!(p.streams.ready(&mut p.engine, ambient.voice, ambient.generation, ambient.part, ambient.sample, 180.0, 2));
        let entry = &mut p.engine.sounds[27].entries[0];
        if blocked { entry.interval = 90.0; entry.next = 90.0; }
        p.play_in_game_music(zero_hash("synth_music_once"));
        t = step_n(&mut rt, 1, t);
        assert!(rt.phase.director.tracks[0].playing(), "script has not supplied PCM");
        let load = rt.phase.streams.requests.iter().find(|r| r.operation == request_kind::LOAD);
        assert_eq!(load.is_some(), !blocked);
        if let Some(&r) = load {
            let p = &mut rt.phase;
            if fails { assert!(p.streams.failed(&p.engine, r.voice, r.generation)); }
            else { assert!(p.streams.ready(&mut p.engine, r.voice, r.generation, r.part, r.sample, 180.0, 2)); }
        }
        step_n(&mut rt, 1, t);
        assert_eq!(rt.phase.director.tracks[0].playing(), blocked || fails);
    }
}

#[test]
fn a_held_script_request_does_not_reopen_a_failed_original() {
    let mut rt = make_synth_swbf2();
    rt.phase.enable_hero_music(false);
    rt.phase.set_mode_score_progress(1.0, 1.0);
    rt.phase.swbf2_mut().unwrap().director.set_track_max_play(0, f64::INFINITY);
    rt.phase.director.tracks[0] = audio_control::director::Track::new(22, 0.1, 90., 1., 0.05, 0.05);
    let original = rt.phase.streams.segments.len();
    rt.phase.streams.segments.push(audio_control::streams::Segment { stream: 0, duration: 180., channels: 2, variants: [NONE; 4] });
    rt.phase.streams.configure();
    rt.phase.engine.sounds[27].entries[0].id = original;
    let mut t = step_n(&mut rt, 1, 0.);
    let ambient = *rt.phase.streams.requests.iter().find(|r| r.operation == request_kind::LOAD).unwrap();
    assert!(rt.phase.streams.ready(&mut rt.phase.engine, ambient.voice, ambient.generation, ambient.part, ambient.sample, 180., 2));
    let mut attempts = 0;
    for _ in 0..3600 {
        rt.phase.play_in_game_music(zero_hash("synth_music_once"));
        t = step_n(&mut rt, 1, t);
        if let Some(req) = rt.phase.streams.requests.iter().find(|r| r.operation == request_kind::LOAD && r.segment == original) {
            attempts += 1;
            rt.input[..ready::STRIDE].fill(0.);
            rt.input[ready::VOICE] = req.voice as f64;
            rt.input[ready::GENERATION] = req.generation as f64;
            rt.input[ready::TIME] = t;
            rt.input[ready::WALL_NOW] = t;
            rt.input[ready::FAILED] = 1.;
            rt.ready();
            assert!(rt.phase.swbf2().unwrap().director.script_music_sound.is_none());
        }
        assert!(rt.phase.director.tracks[0].playing());
    }
    assert_eq!(attempts, 1);
    t = step_n(&mut rt, 1, t);
    rt.phase.play_in_game_music(zero_hash("synth_music_once"));
    step_n(&mut rt, 1, t);
    assert!(rt.phase.streams.requests.iter().any(|r| r.operation == request_kind::LOAD && r.segment == original));
}
