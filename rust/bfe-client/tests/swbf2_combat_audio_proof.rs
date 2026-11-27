//! Real-content proof harness for 2005 (`GameFamily::Swbf2`) battle audio.
//! All tests are `#[ignore]` and read prepared 2005 content from `BFE_SWBF2_CONTENT_DIR`.
//! Zero retail game data is embedded here; every fighter, turret, weapon, and hero
//! power is discovered dynamically from the prepared battle content at test runtime.
use audio::{output::Output, FRAMES};
use audio_control::engine::NONE;
use audio_control::layout::*;
use audio_control::swbf2::powers::{
    PowerAudioEvent, PowerAudioKind, PowerHoldInput, SaberThrowInput,
};
use audio_control::swbf2::saber::SaberEvent;
use audio_control::swbf2::vehicles::{DoorEvent, VehicleEvent};
use audio_control::swbf2::weapons::{SpaceAudioEvent, SpaceOrdnanceInput, SpaceWeaponInput};
use audio_control::swbf2::zero_hash;
use audio_control::{Collision as SoundCollision, Hit, Point};
use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_client::local::launch::Prepared as BattlePrepared;
use bfe_client::local::runtime::Runtime as BattleRuntime;
use bfe_client::local::thread::PublicationSlot;
use bfe_client::online::sound_data::Prepared as SoundPrepared;
use bfe_client::online::sound_output::{build as build_output, AudioSettings};
use bfe_content::files::DirFiles;
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_content_native::Loader;
use bfe_sim::abi::context::Context;
use bfe_sim::space::flight::SpaceTrick;
use bfe_sim::space::objects::SpaceTurretPilotType;
use bfe_sim::space::SpaceEvent;
use bfe_sim::world::admit::mission::{MissionPreferences, MissionSelection};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const FRAMES_PER_TICK: usize = 800; // 48000 Hz / 60 Hz
const OWN: usize = bfe_presentation::network_layout::net_limits::SLOTS + 1;

struct NoCollision;
impl SoundCollision for NoCollision {
    fn cast(&mut self, _from: Point, _d: Point, _far: f64) -> Option<Hit> {
        None
    }
}

fn apply_batch(output: &mut Output, runtime: &audio_control::runtime::Runtime) {
    let commands = &runtime.phase.engine.commands;
    let placements = &runtime.phase.engine.placements;
    output.commands[..commands.len()].copy_from_slice(commands);
    output.apply(commands.len());
    for row in placements.chunks_exact(6) {
        let row: [f32; 6] = std::array::from_fn(|i| row[i] as f32);
        output.mixer.take(&row, false);
    }
}

fn render_rms(output: &mut Output, frames: usize) -> (f64, f32) {
    let mut sum_sq = 0.0_f64;
    let mut peak = 0.0_f32;
    let mut cursor = FRAMES;
    for _ in 0..frames {
        if cursor == FRAMES {
            output.process();
            cursor = 0;
        }
        let l = output.output[cursor];
        let r = output.output[FRAMES + cursor];
        cursor += 1;
        peak = peak.max(l.abs()).max(r.abs());
        sum_sq += f64::from(l) * f64::from(l) + f64::from(r) * f64::from(r);
    }
    let rms = (sum_sq / (frames * 2).max(1) as f64).sqrt();
    (rms, peak)
}

fn sound_name(names: &[String], id: usize) -> String {
    if id == NONE {
        "-".into()
    } else {
        names
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("#{id}"))
    }
}

fn sound_mute_distance(phase: &audio_control::phase::Phase, id: usize) -> f64 {
    let Some(s) = phase.engine.sounds.get(id) else {
        return 0.0;
    };
    if s.kind == 1 {
        return if s.mode3d { s.curve.mute } else { 0.0 };
    }
    if s.kind == 2 || s.kind == 3 {
        let Some(def) = phase.layers.definitions.get(id) else {
            return 0.0;
        };
        let mut m = 0.0_f64;
        for l in &def.layers {
            if let Some(c) = phase.engine.sounds.get(l.sound) {
                if c.mode3d {
                    m = m.max(c.curve.mute);
                }
            }
        }
        return m;
    }
    0.0
}

fn sound_has_samples(phase: &audio_control::phase::Phase, id: usize) -> bool {
    let Some(s) = phase.engine.sounds.get(id) else {
        return false;
    };
    if s.kind == 1 {
        return !s.entries.is_empty();
    }
    if s.kind == 2 || s.kind == 3 {
        return phase
            .layers
            .definitions
            .get(id)
            .is_some_and(|def| def.layers.iter().any(|l| sound_has_samples(phase, l.sound)));
    }
    s.kind == 4 && !s.entries.is_empty()
}

fn sound_is_3d(phase: &audio_control::phase::Phase, id: usize) -> bool {
    let Some(s) = phase.engine.sounds.get(id) else {
        return false;
    };
    if s.kind == 1 {
        return s.mode3d;
    }
    if s.kind == 2 || s.kind == 3 {
        return phase
            .layers
            .definitions
            .get(id)
            .is_some_and(|def| def.layers.iter().any(|l| phase.engine.sounds.get(l.sound).is_some_and(|c| c.mode3d)));
    }
    false
}

fn make_frame(wall: f64, dt: f64, listener_pos: Point, listener_fwd: Point) -> [f64; frame::STRIDE] {
    let mut f = [0.0; frame::STRIDE];
    f[frame::DT] = dt;
    f[frame::WALL_NOW] = wall;
    f[frame::NOW] = wall;
    f[frame::RATE] = 48000.0;
    f[frame::ENHANCED] = 0.0;
    f[frame::LIGHT] = 0.0;
    f[frame::CONFIGURATION] = 1.0;
    f[frame::RUNNING] = 1.0;
    f[frame::MAX_VOICES] = 32.0;
    f[frame::VIRTUAL_VOICES] = 64.0;
    f[frame::PX..frame::PX + 3].copy_from_slice(&listener_pos);
    f[frame::FX..frame::FX + 3].copy_from_slice(&listener_fwd);
    f[frame::UY] = 1.0;
    f[frame::TEAM] = 1.0;
    f[frame::HUMAN] = 1.0;
    f[frame::EFFECTS_VOLUME] = 1.0;
    f[frame::AMBIENCE_VOLUME] = 1.0;
    f[frame::MUSIC_VOLUME] = 1.0;
    f[frame::VOICE_VOLUME] = 1.0;
    f[frame::INTERFACE_VOLUME] = 1.0;
    f[frame::CHANNELS] = 2.0;
    f[frame::TICKETS1] = 100.0;
    f[frame::TICKETS2] = 100.0;
    f[frame::OWN] = OWN as f64;
    f
}

fn build_fresh_output(sp: &mut SoundPrepared) -> Output {
    let settings = AudioSettings {
        enhanced: false,
        output: "stereo".into(),
        dynamics: "normal".into(),
        mono: false,
        clarity: true,
    };
    let (mut output, _) = build_output(&settings, 2, 1 << 2, None, 1.0, None);
    output.take_sources(&mut sp.output);
    output
}

fn measure_weapon_at_distance(
    content: &bfe_content_native::BattleContent,
    weapon_name: &str,
    distance: f64,
) -> (u64, f64, f32) {
    let mut sp = SoundPrepared::new(content).unwrap();
    if let Some(sw) = sp.runtime.phase.swbf2_mut() {
        sw.vehicles.buildings.clear();
        sw.ambience.space_regions.clear();
        sw.ambience.triggers.clear();
    }
    let mut output = build_fresh_output(&mut sp);
    let mut col = NoCollision;
    let dt = 1.0 / 60.0;
    let whash = zero_hash(weapon_name);
    let wcls = sp.runtime.phase.swbf2_class_by_hash(whash);
    let is_chaingun = wcls.is_some_and(|cid| {
        sp.runtime
            .phase
            .swbf2()
            .and_then(|s| s.classes.get(cid))
            .is_some_and(|c| c.warm_up != NONE || c.barrel != NONE)
    });
    let has_fire_loop = wcls.is_some_and(|cid| sp.runtime.phase.sound(cid, class::FIRE_LOOP) != NONE);

    // Put source at (distance, 0, 0) on emitter 1.
    let mut em = [0.0; emitter::STRIDE];
    em[emitter::ID] = 1.0;
    em[emitter::KIND] = 3.0;
    em[emitter::ALIVE] = 1.0;
    em[emitter::GENERATION] = 1.0;
    em[emitter::X] = distance;
    sp.runtime.phase.observe_emitter(&em);

    // Silence all streams/music so we measure ONLY this weapon's PCM.
    let mut total_started = 0u64;
    let mut sum_sq = 0.0_f64;
    let mut peak = 0.0_f32;
    let ticks = 18;
    for t in 0..ticks {
        let wall = output.frame as f64 / 48000.0;
        sp.runtime.clear_output();
        let mut frame = make_frame(wall, dt, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        frame[frame::AMBIENCE_VOLUME] = 0.0;
        frame[frame::MUSIC_VOLUME] = 0.0;
        frame[frame::VOICE_VOLUME] = 0.0;
        sp.runtime.phase.frame.copy_from_slice(&frame);
        sp.runtime.events.clear();
        if t == 0 {
            let mut start = [0.0; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            sp.runtime.events.push(start);
        }
        sp.runtime.phase.push_space_weapon(SpaceWeaponInput {
            key: 1,
            owner: 1,
            class_hash: whash,
            fired: t == 1,
            firing: (1..=6).contains(&t) && (is_chaingun || has_fire_loop || t == 1),
            spinning: (1..=6).contains(&t) && is_chaingun,
            overheated: false,
            heat: 0.0,
            heat_threshold: 0.5,
            reloading: false,
            changed: false,
            own: false,
        });
        let before = sp.runtime.phase.engine.stats[2];
        sp.runtime.step(&mut col);
        total_started += sp.runtime.phase.engine.stats[2].saturating_sub(before);
        apply_batch(&mut output, &sp.runtime);
        let (r, p) = render_rms(&mut output, FRAMES_PER_TICK);
        sum_sq += r * r;
        peak = peak.max(p);
    }
    let rms = (sum_sq / ticks as f64).sqrt();
    (total_started, rms, peak)
}

fn measure_hit_at_distance(
    content: &bfe_content_native::BattleContent,
    weapon_name: &str,
    distance: f64,
) -> (u64, f64, f32) {
    let mut sp = SoundPrepared::new(content).unwrap();
    if let Some(sw) = sp.runtime.phase.swbf2_mut() {
        sw.vehicles.buildings.clear();
        sw.ambience.space_regions.clear();
        sw.ambience.triggers.clear();
    }
    let mut output = build_fresh_output(&mut sp);
    let mut col = NoCollision;
    let dt = 1.0 / 60.0;
    let whash = zero_hash(weapon_name);
    let mut total_started = 0u64;
    let mut sum_sq = 0.0_f64;
    let mut peak = 0.0_f32;
    let ticks = 18;
    for t in 0..ticks {
        let wall = output.frame as f64 / 48000.0;
        sp.runtime.clear_output();
        let mut frame = make_frame(wall, dt, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        frame[frame::AMBIENCE_VOLUME] = 0.0;
        frame[frame::MUSIC_VOLUME] = 0.0;
        frame[frame::VOICE_VOLUME] = 0.0;
        sp.runtime.phase.frame.copy_from_slice(&frame);
        sp.runtime.events.clear();
        if t == 0 {
            let mut start = [0.0; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            sp.runtime.events.push(start);
        }
        if t == 1 {
            sp.runtime
                .phase
                .push_space_audio_event(SpaceAudioEvent::RoundHit {
                    weapon_hash: whash,
                    position: [distance, 0.0, 0.0],
                });
        }
        let before = sp.runtime.phase.engine.stats[2];
        sp.runtime.step(&mut col);
        total_started += sp.runtime.phase.engine.stats[2].saturating_sub(before);
        apply_batch(&mut output, &sp.runtime);
        let (r, p) = render_rms(&mut output, FRAMES_PER_TICK);
        sum_sq += r * r;
        peak = peak.max(p);
    }
    let rms = (sum_sq / ticks as f64).sqrt();
    (total_started, rms, peak)
}

#[test]
#[ignore = "requires prepared 2005 content in BFE_SWBF2_CONTENT_DIR"]
fn spa6_every_weapon_and_turret_plays_with_rolloff() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("BFE_SWBF2_CONTENT_DIR");
    let map = std::env::var("BFE_SWBF2_SPACE_MAP").unwrap_or_else(|_| "spa6".into());
    let era = std::env::var("BFE_SWBF2_SPACE_ERA").unwrap_or_else(|_| "cw".into());
    let mode = std::env::var("BFE_SWBF2_SPACE_MODE").unwrap_or_else(|_| "ass".into());

    let selection = MissionSelection {
        family: bfe_content::GameFamily::Swbf2,
        map: map.clone(),
        era: era.clone(),
        mode: mode.clone(),
        seed: 1,
    };
    let side = if era == "cw" { "rep" } else { "all" };
    let mut admit_opts = bfe_sim::world::admit::Options::default();
    admit_opts.family = bfe_family::GameFamily::Swbf2;
    admit_opts.map = map.clone();
    admit_opts.side = side.into();
    admit_opts.mode = mode.clone();
    admit_opts.seed = 1;

    let ctx = Context::new();
    let files = DirFiles::from_resolved_root(Path::new(&root), bfe_family::GameFamily::Swbf2);
    let (_world, journal) = ctx
        .world_mission_content(&files, &admit_opts, &selection, MissionPreferences::default())
        .expect("admit space mission");

    let content = Loader::from_resolved_root(&root, bfe_family::GameFamily::Swbf2)
        .load_scripted_mission(&map, &journal.resources.query, &journal.resources.mission_json)
        .expect("load space content");
    let sp = SoundPrepared::new(&content).expect("prepare space sound");

    // Collect every fighter weapon of both fleets and every ship/chair/hangar turret weapon.
    let mut fighter_weapons: Vec<(String, String, u8, String)> = Vec::new();
    let mut turret_weapons: Vec<(String, String, String)> = Vec::new();
    let mut seen_turret_pairs = BTreeSet::<(String, String)>::new();
    let mut craft_classes = Vec::<String>::new();
    let mut door_classes = BTreeSet::<String>::new();
    let mut destruct_classes = BTreeSet::<String>::new();

    ctx.owned(|w| {
        let space = w.space().expect("space battlefield present");
        for p in &space.profiles {
            craft_classes.push(p.class_name.clone());
            for wpn in &p.weapons {
                fighter_weapons.push((
                    p.side.clone(),
                    p.class_name.clone(),
                    wpn.seat,
                    wpn.name.clone(),
                ));
            }
        }
        for b in &space.buildings {
            destruct_classes.insert(b.class.clone());
            if let Some(t) = &b.turret {
                let kind = match t.pilot_type {
                    SpaceTurretPilotType::Remote => "remote_chair",
                    SpaceTurretPilotType::SelfPiloted => "auto_turret",
                    SpaceTurretPilotType::Player => "player_turret",
                };
                if seen_turret_pairs.insert((b.class.clone(), t.weapon.name.clone())) {
                    turret_weapons.push((kind.into(), b.class.clone(), t.weapon.name.clone()));
                }
            }
        }
        for d in &space.doors {
            door_classes.insert(d.class.clone());
        }
    })
    .unwrap();

    assert!(
        !fighter_weapons.is_empty(),
        "expected fighter weapons on {map}"
    );
    assert!(
        !turret_weapons.is_empty(),
        "expected ship/chair turret weapons on {map}"
    );

    println!(
        "\n=== SPACE WEAPON AUDIT ({map} / {era} / {mode}): {} fighter mounts, {} turret types ===",
        fighter_weapons.len(),
        turret_weapons.len()
    );

    let mut unique_weapons: BTreeMap<String, (u64, f64, f64, f64, u64, f64)> = BTreeMap::new();
    let mut failures = Vec::<String>::new();

    let all_entries: Vec<(String, String, String)> = fighter_weapons
        .iter()
        .map(|(side, craft, seat, wpn)| (format!("{side}/{craft}[seat{seat}]"), craft.clone(), wpn.clone()))
        .chain(
            turret_weapons
                .iter()
                .map(|(kind, bldg, wpn)| (format!("{kind}/{bldg}"), bldg.clone(), wpn.clone())),
        )
        .collect();

    for (owner_label, _owner_cls, wpn) in &all_entries {
        let whash = zero_hash(wpn);
        let cid = sp.runtime.phase.swbf2_class_by_hash(whash);
        let Some(cid) = cid else {
            failures.push(format!("{owner_label} -> {wpn}: class NOT found in sound runtime"));
            println!("FAIL | {owner_label} | {wpn} | class missing");
            continue;
        };
        let phase = &sp.runtime.phase;
        let sw = phase.swbf2().unwrap();
        let ext = &sw.classes[cid];

        let fire_id = phase.sound(cid, class::FIRE);
        let loop_id = phase.sound(cid, class::FIRE_LOOP);
        let overheat_id = phase.sound(cid, class::OVERHEAT);
        let reload_id = phase.sound(cid, class::RELOAD);
        let change_id = phase.sound(cid, class::CHANGE);
        let warm_id = ext.warm_up;
        let barrel_id = ext.barrel;

        let ord_cls = ext.ordnance_class;
        let ord_col = if ord_cls != NONE { phase.sound(ord_cls, class::IMPACT) } else { NONE };
        let ord_hum = if ord_cls != NONE {
            let h = phase.sound(ord_cls, class::ORDNANCE);
            if h != NONE { h } else { phase.sound(ord_cls, class::ENGINE) }
        } else {
            NONE
        };
        let exp_cls = if ord_cls != NONE && sw.classes[ord_cls].explosion_class != NONE {
            sw.classes[ord_cls].explosion_class
        } else {
            ext.explosion_class
        };
        let exp_snd = if exp_cls != NONE {
            phase.sound(exp_cls, class::EXPLOSION)
        } else if ord_cls != NONE {
            phase.sound(ord_cls, class::DEATH)
        } else {
            NONE
        };
        let primary_snd = [fire_id, loop_id, barrel_id, warm_id]
            .into_iter()
            .find(|&id| id != NONE)
            .unwrap_or(NONE);

        let (started_near, near_rms, far_rms, beyond_rms, hit_started, hit_rms) =
            *unique_weapons.entry(wpn.clone()).or_insert_with(|| {
                let (sn, nr, _) = measure_weapon_at_distance(&content, wpn, 10.0);
                let mute = sound_mute_distance(phase, primary_snd);
                let is_3d = sound_is_3d(phase, primary_snd);
                let far_d = if is_3d && mute > 30.0 {
                    (mute * 0.75).max(80.0)
                } else {
                    150.0
                };
                let (_, fr, _) = measure_weapon_at_distance(&content, wpn, far_d);
                let (_, br, _) = if is_3d && mute > 0.0 {
                    measure_weapon_at_distance(&content, wpn, mute + 60.0)
                } else {
                    (0, 0.0, 0.0)
                };
                let (hs, hr, _) = measure_hit_at_distance(&content, wpn, 10.0);
                (sn, nr, fr, br, hs, hr)
            });

        let has_pcm = sound_has_samples(phase, primary_snd);
        let plays = primary_snd != NONE && has_pcm && started_near > 0 && near_rms > 1e-4;
        let is_3d = sound_is_3d(phase, primary_snd);
        let mute = sound_mute_distance(phase, primary_snd);
        let rolloff_ok = !has_pcm || !is_3d || (far_rms < near_rms && (mute <= 0.0 || beyond_rms < 1e-6));
        let hit_expected = (ord_col != NONE && sound_has_samples(phase, ord_col))
            || (exp_snd != NONE && sound_has_samples(phase, exp_snd));
        let hit_ok = !hit_expected || (hit_started > 0 && hit_rms > 1e-4);

        println!(
            "WPN | {owner_label} | {wpn} | fire={} loop={} warm={} barrel={} ovh={} rel={} chg={} | ord_col={} ord_hum={} exp={} | pcm={has_pcm} mute={mute:.1} 3d={is_3d} | near_rms={near_rms:.4} far_rms={far_rms:.4} beyond_rms={beyond_rms:.4} | hit_rms={hit_rms:.4} | plays={} rolloff={} hit={}",
            sound_name(&sp.sound_names, fire_id),
            sound_name(&sp.sound_names, loop_id),
            sound_name(&sp.sound_names, warm_id),
            sound_name(&sp.sound_names, barrel_id),
            sound_name(&sp.sound_names, overheat_id),
            sound_name(&sp.sound_names, reload_id),
            sound_name(&sp.sound_names, change_id),
            sound_name(&sp.sound_names, ord_col),
            sound_name(&sp.sound_names, ord_hum),
            sound_name(&sp.sound_names, exp_snd),
            if plays { "YES" } else if !has_pcm { "NO(sample_absent_in_bank)" } else { "NO" },
            if rolloff_ok { "OK" } else { "FAIL" },
            if hit_ok { "OK" } else { "FAIL" },
        );

        if has_pcm && !plays {
            failures.push(format!("{owner_label} -> {wpn}: did not play (primary={primary_snd}, started={started_near}, rms={near_rms})"));
        }
        if !rolloff_ok {
            failures.push(format!("{owner_label} -> {wpn}: rolloff failed (near={near_rms}, far={far_rms}, beyond={beyond_rms}, mute={mute})"));
        }
        if !hit_ok {
            failures.push(format!("{owner_label} -> {wpn}: hit/explosion failed (ord_col={ord_col}, exp_snd={exp_snd}, hit_started={hit_started}, hit_rms={hit_rms})"));
        }
    }

    // Verify craft engines, boost cues, tricks/flips, cockpit, destruction, and doors resolve.
    for craft in &craft_classes {
        let cid = sp
            .runtime
            .phase
            .swbf2_class_by_hash(zero_hash(craft))
            .unwrap_or_else(|| panic!("craft class {craft} missing"));
        let eng = sp.runtime.phase.sound(cid, class::ENGINE);
        let death = sp.runtime.phase.sound(cid, class::DEATH);
        let ext = &sp.runtime.phase.swbf2().unwrap().classes[cid];
        println!(
            "CRAFT | {craft} | engine={} death={} boost_cues={} trick={} flip={} cockpit_1p={} cockpit_3p={}",
            sound_name(&sp.sound_names, eng),
            sound_name(&sp.sound_names, death),
            ext.boost_cues.len(),
            sound_name(&sp.sound_names, ext.trick_sound),
            sound_name(&sp.sound_names, ext.flip_sound),
            sound_name(&sp.sound_names, ext.cockpit_1p),
            sound_name(&sp.sound_names, ext.cockpit_3p),
        );
        assert_ne!(eng, NONE, "craft {craft} missing EngineSound");
        assert_ne!(death, NONE, "craft {craft} missing DeathSound");
    }

    let mut doors_with_sound = 0usize;
    for d in &door_classes {
        if let Some(cid) = sp.runtime.phase.swbf2_class_by_hash(zero_hash(d)) {
            let ext = &sp.runtime.phase.swbf2().unwrap().classes[cid];
            if ext.door_open != NONE || ext.door_close != NONE {
                doors_with_sound += 1;
            }
            println!(
                "DOOR | {d} | open={} close={} locked={}",
                sound_name(&sp.sound_names, ext.door_open),
                sound_name(&sp.sound_names, ext.door_close),
                sound_name(&sp.sound_names, ext.door_locked),
            );
        }
    }
    println!("DOORS with authored open/close sound: {doors_with_sound}/{}", door_classes.len());

    let mut bldgs_with_destruct = 0usize;
    for b in &destruct_classes {
        if let Some(cid) = sp.runtime.phase.swbf2_class_by_hash(zero_hash(b)) {
            let ext = &sp.runtime.phase.swbf2().unwrap().classes[cid];
            let death = sp.runtime.phase.sound(cid, class::DEATH);
            let exp_snd = if ext.destruct_explosion_class != NONE {
                sp.runtime.phase.sound(ext.destruct_explosion_class, class::EXPLOSION)
            } else {
                NONE
            };
            if death != NONE || exp_snd != NONE || ext.damage_effect_sound != NONE {
                bldgs_with_destruct += 1;
            }
        }
    }
    println!(
        "BUILDINGS with destruction/damage audio: {bldgs_with_destruct}/{} | placed BuildingEmitters={}",
        destruct_classes.len(),
        sp.runtime.phase.swbf2().unwrap().vehicles.buildings.len()
    );
    assert!(
        bldgs_with_destruct > 0,
        "expected destructible ship buildings/turrets with destruction audio"
    );

    let _ = ctx.dispose();
    assert!(
        failures.is_empty(),
        "Space weapon audit failures:\n{}",
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires prepared 2005 content in BFE_SWBF2_CONTENT_DIR"]
fn spa6_live_battle_player_and_bots_bounded() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("BFE_SWBF2_CONTENT_DIR");
    let map = std::env::var("BFE_SWBF2_SPACE_MAP").unwrap_or_else(|_| "spa6".into());
    let era = std::env::var("BFE_SWBF2_SPACE_ERA").unwrap_or_else(|_| "cw".into());
    let mode = std::env::var("BFE_SWBF2_SPACE_MODE").unwrap_or_else(|_| "ass".into());

    std::env::set_var("BFE_DEV_SWBF2_SEATED", "1");
    let side = if era == "cw" { "rep" } else { "all" };
    let selection = MissionSelection {
        family: bfe_content::GameFamily::Swbf2,
        map: map.clone(),
        era: era.clone(),
        mode: mode.clone(),
        seed: 7,
    };
    let mut admit_opts = bfe_sim::world::admit::Options::default();
    admit_opts.family = bfe_family::GameFamily::Swbf2;
    admit_opts.map = map.clone();
    admit_opts.side = side.into();
    admit_opts.mode = mode;
    admit_opts.seed = 7;

    let ctx = Context::new();
    let (prepared, journal) = BattlePrepared::developer_mission_in(
        ctx.clone(),
        Path::new(&root),
        &admit_opts,
        &selection,
        MissionPreferences { direct_spawn: true, ..Default::default() },
    )
    .expect("admit space mission");
    let player = prepared.admission.handles.player;

    let content = Loader::from_resolved_root(&root, bfe_family::GameFamily::Swbf2)
        .load_scripted_mission(&map, &journal.resources.query, &journal.resources.mission_json)
        .expect("load space content");
    let mut sp = SoundPrepared::new(&content).expect("prepare space sound");
    let mut output = build_fresh_output(&mut sp);
    let mut col = NoCollision;

    let mut slot = PublicationSlot::new_preallocated();
    let mut rt = BattleRuntime::new_into_slot(
        *prepared.engine,
        *prepared.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
        &mut slot,
    )
    .expect("create battle runtime");

    let dt = 1.0 / 60.0;
    let hull_base = OWN + 1 + sp.stations.len();
    let ord_base = hull_base + 256;
    let mut player_fired = 0u64;
    let mut bot_fired = 0u64;
    let mut turret_fired = 0u64;
    let mut round_hits = 0u64;
    let mut destructions = 0u64;
    let mut peak_real = 0u64;
    let mut peak_virtual = 0u64;
    let mut peak_non_param = 0usize;
    let mut sum_sq = 0.0_f64;
    let mut peak_pcm = 0.0_f32;
    let mut hull_tricks = [SpaceTrick::None; 256];
    let mut ord_scratch = Vec::<SpaceOrdnanceInput>::with_capacity(256);

    let ticks = 420usize; // 7 seconds of real space combat
    for tick in 1..=ticks {
        let (local_state, local_id) = ctx
            .owned(|w| {
                w.space()
                    .and_then(|s| s.pilot_fighter(player))
                    .map(|f| (f.state, f.id))
            })
            .unwrap()
            .unzip();
        let local_gen = slot.infantry.map_or(1.0, |s| s.generation);
        let next = rt.tick() + 1.0;
        let cmd = ClientCommand {
            tick: next,
            sequence: next,
            generation: local_gen,
            look_count: 1,
            has_aspect: true,
            aspect: 16.0 / 9.0,
            has_mouse_scale: true,
            mouse_scale: 1.0,
            ..Default::default()
        };
        let flight = ClientFlightCommand {
            scale: 1.0,
            curve: 1.0,
            ..Default::default()
        };
        let look = ClientLook::default();
        if matches!(tick, 1 | 15 | 60 | 120 | 200) {
            println!("  tick {tick}: local_fighter={local_id:?} state={local_state:?} gen={local_gen}");
        }
        let landed = local_state == Some(bfe_sim::space::flight::SpaceFlightState::Landed);
        let flying = local_state == Some(bfe_sim::space::flight::SpaceFlightState::Flying);
        let controls = bfe_sim::units::infantry::Controls::default();
        let mut pilot = bfe_sim::space::local::PilotControls::default();
        pilot.flight.takeoff_or_land = landed && (tick % 10 == 5);
        pilot.flight.throttle = if flying { 1.0 } else { 0.0 };
        pilot.flight.fire_primary = flying && ((tick % 120) < 60);
        pilot.flight.fire_secondary = flying && (tick % 120 == 70 || tick % 120 == 71);
        pilot.flight.boost = flying && (180..=230).contains(&tick);
        pilot.flight.trick_roll = if flying && tick == 200 { 1 } else { 0 };
        rt.submit(&cmd, &look, &flight, rt.tick(), None, false)
            .unwrap();
        rt.infantry_input(next, controls).unwrap();
        rt.pilot_input(next, pilot).unwrap();
        rt.step_into_slot(&mut slot).unwrap();

        let wall = output.frame as f64 / 48000.0;
        sp.runtime.clear_output();
        sp.runtime.events.clear();
        if !sp.runtime.phase.started {
            let mut start = [0.0; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            sp.runtime.events.push(start);
        }

        let mut listener_pos = [0.0, 0.0, 0.0];
        let mut listener_fwd = [0.0, 0.0, 1.0];
        ctx.owned(|w| {
            let Some(space) = w.space() else { return };
            if let Some(f) = space.pilot_fighter(player) {
                listener_pos = f.position;
                listener_fwd = f.forward();
                sp.runtime
                    .phase
                    .set_own_mount_swbf2(Some(zero_hash(&space.profiles[f.profile_index].class_name)), true);
            }
            // Observe all live fighters and push active weapons.
            for f in space.fighters.iter().filter(|f| f.alive) {
                if f.id >= 256 {
                    continue;
                }
                let eid = hull_base + f.id;
                let profile = &space.profiles[f.profile_index];
                let chash = zero_hash(&profile.class_name);
                let mut row = [0.0; emitter::STRIDE];
                row[emitter::ID] = eid as f64;
                row[emitter::KIND] = 3.0;
                row[emitter::CLASS] = sp
                    .runtime
                    .phase
                    .swbf2_class_by_hash(chash)
                    .map_or(-1.0, |c| c as f64);
                row[emitter::ALIVE] = 1.0;
                row[emitter::GENERATION] = f64::from(f.generation);
                row[emitter::FLYING] = 1.0;
                row[emitter::PILOTED] = f.pilot.is_some() as u8 as f64;
                row[emitter::HEALTH] = f.health;
                row[emitter::MAX_HEALTH] = f.max_health;
                row[emitter::FLIGHT] = f64::from(f.state as u8 + 1);
                row[emitter::SPEED_LIMIT] = profile.max_speed;
                row[emitter::X..emitter::X + 3].copy_from_slice(&f.position);
                row[emitter::VX..emitter::VX + 3].copy_from_slice(&f.velocity);
                sp.runtime.phase.observe_emitter(&row);

                if hull_tricks[f.id] == SpaceTrick::None && f.trick != SpaceTrick::None {
                    let ev = match f.trick {
                        SpaceTrick::BarrelLeft | SpaceTrick::BarrelRight => {
                            VehicleEvent::Trick { owner: eid, class_hash: chash }
                        }
                        SpaceTrick::Loop | SpaceTrick::Immelmann => {
                            VehicleEvent::Flip { owner: eid, class_hash: chash }
                        }
                        SpaceTrick::None => unreachable!(),
                    };
                    sp.runtime.phase.push_vehicle_event(ev);
                }
                hull_tricks[f.id] = f.trick;

                let is_local = space.pilot_fighter(player).is_some_and(|lf| lf.id == f.id);
                for (wi, spec) in profile.weapons.iter().enumerate().take(5) {
                    let rt_w = &f.weapons[wi];
                    let fired = space.events.iter().any(|ev| {
                        matches!(*ev, SpaceEvent::WeaponFired { fighter: Some(fid), weapon_index, .. } if fid == f.id && weapon_index as usize == wi)
                    });
                    if fired {
                        if is_local {
                            player_fired += 1;
                        } else {
                            bot_fired += 1;
                        }
                    }
                    let seat_occ = f.occupant(spec.seat).is_some();
                    let reloading = rt_w.reload_timer > 0.0;
                    let spinning = seat_occ && rt_w.trigger_was_held && !rt_w.overheated && !reloading;
                    let firing = seat_occ && !rt_w.overheated && !reloading && (fired || rt_w.trigger_was_held || rt_w.clock.salvos > 0);
                    if fired || firing || spinning || rt_w.overheated || reloading || f.pilot.is_some() {
                        sp.runtime.phase.push_space_weapon(SpaceWeaponInput {
                            key: (f.id << 3) | wi,
                            owner: eid,
                            class_hash: zero_hash(&spec.name),
                            fired,
                            firing,
                            spinning,
                            overheated: rt_w.overheated,
                            heat: rt_w.heat,
                            heat_threshold: spec.heat_threshold,
                            reloading,
                            changed: false,
                            own: is_local,
                        });
                    }
                }
            }
            // Turrets.
            for (ti, b) in space.buildings.iter().filter(|b| b.turret.is_some()).enumerate().take(256) {
                let t = b.turret.as_ref().unwrap();
                let alive = w.space_building_snapshot(b.building_index).is_some_and(|s| s.alive);
                let fired = space.events.iter().any(|ev| {
                    matches!(*ev, SpaceEvent::WeaponFired { building: Some(bidx), .. } if bidx == b.building_index)
                });
                if fired {
                    turret_fired += 1;
                }
                let t_class = sp
                    .runtime
                    .phase
                    .swbf2_class_by_hash(zero_hash(&b.class))
                    .map_or(-1.0, |c| c as f64);
                let row = &mut sp.runtime.phase.turrets[ti];
                row[turret::ID] = ti as f64;
                row[turret::CLASS] = t_class;
                row[turret::OWNER] = -1.0;
                row[turret::X..turret::X + 3].copy_from_slice(&b.center);
                row[turret::ALIVE] = alive as u8 as f64;
                row[turret::HELD] = match t.pilot_type {
                    SpaceTurretPilotType::Remote => (alive && t.remote_operator.is_some()) as u8 as f64,
                    SpaceTurretPilotType::SelfPiloted => alive as u8 as f64,
                    SpaceTurretPilotType::Player => 0.0,
                };
                row[turret::YAW] = t.yaw;
                row[turret::PITCH] = t.pitch;
                row[turret::YAW_TOP] = t.turn_rate.max(0.1);
                row[turret::PITCH_TOP] = t.pitch_rate.max(0.1);
                row[turret::PRESENT] = 1.0;
                sp.runtime.phase.turret_count = sp.runtime.phase.turret_count.max(ti + 1);
                if alive {
                    let owner = sp.runtime.phase.turret_emitter_base + ti;
                    sp.runtime.phase.push_space_weapon(SpaceWeaponInput {
                        key: (1 << 16) | ti,
                        owner,
                        class_hash: zero_hash(&t.weapon.name),
                        fired,
                        firing: alive && !t.overheated && (fired || t.trigger_was_held || t.clock.salvos > 0),
                        spinning: alive && t.trigger_was_held && !t.overheated,
                        overheated: t.overheated,
                        heat: t.heat,
                        heat_threshold: t.weapon.heat_threshold,
                        reloading: false,
                        changed: false,
                        own: false,
                    });
                }
            }
            // Rounds & hits & expiries.
            ord_scratch.clear();
            if let Some(obs) = &space.ordnance.observations {
                for r in obs.rounds.iter().filter(|r| r.serial != 0 && r.tick == space.tick) {
                    if r.contact.is_some() {
                        round_hits += 1;
                        sp.runtime.phase.push_space_audio_event(SpaceAudioEvent::RoundHit {
                            weapon_hash: r.weapon,
                            position: r.position,
                        });
                    } else if !r.active {
                        sp.runtime.phase.push_space_audio_event(SpaceAudioEvent::RoundExpired {
                            weapon_hash: r.weapon,
                            position: r.position,
                        });
                    } else if ord_scratch.len() < ord_scratch.capacity() {
                        ord_scratch.push(SpaceOrdnanceInput {
                            serial: r.serial,
                            weapon_hash: r.weapon,
                            position: r.position,
                            velocity: r.velocity,
                        });
                    }
                }
            }
            sp.runtime.phase.queue_space_ordnance(ord_base, &ord_scratch);
            for ev in &space.events {
                match *ev {
                    SpaceEvent::FighterDestroyed { fighter, .. } => {
                        if let Some(f) = space.fighters.get(fighter) {
                            destructions += 1;
                            sp.runtime.phase.push_space_audio_event(SpaceAudioEvent::Destruction {
                                class_hash: zero_hash(&space.profiles[f.profile_index].class_name),
                                position: f.position,
                            });
                        }
                    }
                    SpaceEvent::BuildingDestroyed { building, .. } => {
                        if let Some(b) = space.buildings.iter().find(|b| b.building_index == building) {
                            destructions += 1;
                            sp.runtime.phase.push_space_audio_event(SpaceAudioEvent::Destruction {
                                class_hash: zero_hash(&b.class),
                                position: b.center,
                            });
                            sp.runtime.phase.set_building_alive_swbf2(zero_hash(&b.name), false);
                        }
                    }
                    SpaceEvent::DoorChanged { door, open, locked } => {
                        if let Some(d) = space.doors.get(door) {
                            let ch = zero_hash(&d.class);
                            let de = if open {
                                DoorEvent::Open { class_hash: ch, position: d.position }
                            } else if locked {
                                DoorEvent::Locked { class_hash: ch, position: d.position }
                            } else {
                                DoorEvent::Close { class_hash: ch, position: d.position }
                            };
                            sp.runtime.phase.push_door_event(de);
                        }
                    }
                    _ => {}
                }
            }
        })
        .unwrap();

        let frame = make_frame(wall, dt, listener_pos, listener_fwd);
        sp.runtime.phase.frame.copy_from_slice(&frame);
        sp.runtime.step(&mut col);
        apply_batch(&mut output, &sp.runtime);

        let real = sp.runtime.phase.engine.stats[0];
        let virt = sp.runtime.phase.engine.stats[1];
        let non_param = sp.runtime.phase.layers.active_non_param_count();
        peak_real = peak_real.max(real);
        peak_virtual = peak_virtual.max(virt);
        peak_non_param = peak_non_param.max(non_param);
        assert!(real <= 32, "real voices exceeded 32 at tick {tick}: {real}");
        assert!(virt <= 64, "virtual voices exceeded 64 at tick {tick}: {virt}");
        assert!(non_param <= 64, "non-param layer groups exceeded 64 at tick {tick}: {non_param}");

        let (r, p) = render_rms(&mut output, FRAMES_PER_TICK);
        sum_sq += r * r;
        peak_pcm = peak_pcm.max(p);
    }

    let rms = (sum_sq / ticks as f64).sqrt();
    let stats = sp.runtime.phase.engine.stats;
    println!(
        "SPA6 LIVE BATTLE ({ticks} ticks): player_fired={player_fired} bot_fired={bot_fired} turret_fired={turret_fired} round_hits={round_hits} destructions={destructions} | voices_started={} stolen={} dropped={} culled={} | peak_real={peak_real}/32 peak_virtual={peak_virtual}/64 peak_groups={peak_non_param}/64 | rms={rms:.4} peak_pcm={peak_pcm:.4}",
        stats[2], stats[3], stats[4], stats[5]
    );
    assert!(player_fired > 0, "player fighter must fire weapons");
    assert!(bot_fired > 0, "bot fighters must fire weapons");
    assert!(turret_fired > 0, "ship turrets must fire weapons");
    assert!(round_hits > 0, "space combat must produce round hits");
    assert!(stats[2] > 20, "expected active audio voices started");
    assert!(rms > 0.001 && peak_pcm > 0.01, "expected audible non-silent stereo PCM");
    rt.close();
}

fn prepare_isolated_sound(content: &bfe_content_native::BattleContent) -> (SoundPrepared, Output) {
    let mut sp = SoundPrepared::new(content).unwrap();
    if let Some(sw) = sp.runtime.phase.swbf2_mut() {
        sw.vehicles.buildings.clear();
        sw.ambience.space_regions.clear();
        sw.ambience.triggers.clear();
    }
    sp.runtime.phase.director.static_emitters.clear();
    sp.runtime.phase.director.static_voices.clear();
    sp.runtime.phase.director.regions.clear();
    sp.runtime.phase.director.region_voices.clear();
    let output = build_fresh_output(&mut sp);
    (sp, output)
}

fn step_isolated_ticks(
    sp: &mut SoundPrepared,
    output: &mut Output,
    ticks: usize,
    mut per_tick: impl FnMut(&mut SoundPrepared, usize),
) -> (u64, f64, f32) {
    let mut col = NoCollision;
    let dt = 1.0 / 60.0;
    let mut total_started = 0u64;
    let mut sum_sq = 0.0_f64;
    let mut peak = 0.0_f32;
    for t in 0..ticks {
        let wall = output.frame as f64 / 48000.0;
        sp.runtime.clear_output();
        let mut frame = make_frame(wall, dt, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        frame[frame::AMBIENCE_VOLUME] = 0.0;
        frame[frame::MUSIC_VOLUME] = 0.0;
        frame[frame::VOICE_VOLUME] = 0.0;
        sp.runtime.phase.frame.copy_from_slice(&frame);
        sp.runtime.events.clear();
        if !sp.runtime.phase.started {
            let mut start = [0.0; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            sp.runtime.events.push(start);
        }
        per_tick(sp, t);
        let before = sp.runtime.phase.engine.stats[2];
        sp.runtime.step(&mut col);
        total_started += sp.runtime.phase.engine.stats[2].saturating_sub(before);
        apply_batch(output, &sp.runtime);
        let (r, p) = render_rms(output, FRAMES_PER_TICK);
        sum_sq += r * r;
        peak = peak.max(p);
    }
    let rms = (sum_sq / ticks.max(1) as f64).sqrt();
    (total_started, rms, peak)
}

fn observe_soldier_emitter(sp: &mut SoundPrepared, eid: usize, x: f64) {
    let mut em = [0.0; emitter::STRIDE];
    em[emitter::ID] = eid as f64;
    em[emitter::KIND] = 1.0;
    em[emitter::ALIVE] = 1.0;
    em[emitter::GENERATION] = 1.0;
    em[emitter::X] = x;
    sp.runtime.phase.observe_emitter(&em);
}

#[test]
#[ignore = "requires prepared 2005 content in BFE_SWBF2_CONTENT_DIR"]
fn tat2_every_hero_power_and_offhand_plays() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("BFE_SWBF2_CONTENT_DIR");
    let map = std::env::var("BFE_SWBF2_HERO_MAP").unwrap_or_else(|_| "tat2".into());
    let era = std::env::var("BFE_SWBF2_HERO_ERA").unwrap_or_else(|_| "gcw".into());
    let mode = std::env::var("BFE_SWBF2_HERO_MODE").unwrap_or_else(|_| "eli".into());
    let side = if era == "cw" { "rep" } else { "all" };

    let selection = MissionSelection {
        family: bfe_content::GameFamily::Swbf2,
        map: map.clone(),
        era: era.clone(),
        mode: mode.clone(),
        seed: 1,
    };
    let mut admit_opts = bfe_sim::world::admit::Options::default();
    admit_opts.family = bfe_family::GameFamily::Swbf2;
    admit_opts.map = map.clone();
    admit_opts.side = side.into();
    admit_opts.mode = mode.clone();
    admit_opts.seed = 1;

    let ctx = Context::new();
    let files = DirFiles::from_resolved_root(Path::new(&root), bfe_family::GameFamily::Swbf2);
    let (_world, journal) = ctx
        .world_mission_content(&files, &admit_opts, &selection, MissionPreferences::default())
        .expect("admit tat2 hero mission");

    let content = Loader::from_resolved_root(&root, bfe_family::GameFamily::Swbf2)
        .load_scripted_mission(&map, &journal.resources.query, &journal.resources.mission_json)
        .expect("load tat2 content");
    let sp_ref = SoundPrepared::new(&content).expect("prepare tat2 sound");
    let defs = content.defs().expect("defs");

    // Discover every hero class and every weapon / power / offhand item dynamically.
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for cls in &defs.classes {
        if cls.record.get("base").and_then(bfe_content::Value::as_str) != Some("soldier") {
            continue;
        }
        let hero_name = defs.names.text(cls.name).to_string();
        let weapons = cls
            .record
            .get("weapons")
            .and_then(bfe_content::Value::as_array)
            .unwrap_or(&[]);
        for w in weapons {
            let Some(wname) = w.get("class").and_then(bfe_content::Value::as_str) else {
                continue;
            };
            let base = w.get("base").and_then(bfe_content::Value::as_str).unwrap_or("");
            let role = match bfe_sim::combat::powers::def::from_weapon(w) {
                Ok(Some(pdef)) => match pdef.effect {
                    bfe_sim::combat::powers::Effect::Cone(cone) => {
                        if cone.push > 0.0 {
                            "force_push"
                        } else if cone.push < 0.0 {
                            "force_pull"
                        } else {
                            "force_lightning"
                        }
                    }
                    bfe_sim::combat::powers::Effect::Aura(_) => "aura",
                    bfe_sim::combat::powers::Effect::Area(_) => "force_choke",
                },
                _ if base == "meleethrow" => "thrown_blade",
                _ if base == "melee" => "saber",
                _ => "gun_or_offhand",
            };
            rows.push((hero_name.clone(), role.to_string(), wname.to_string()));
            if role == "saber"
                && w.get("ExplosionName")
                    .and_then(bfe_content::Value::as_str)
                    .is_some_and(|s| !s.is_empty())
            {
                rows.push((hero_name.clone(), "combo_blast".to_string(), wname.to_string()));
            }
        }
    }

    assert!(!rows.is_empty(), "expected hero weapons on {map}");
    println!(
        "\n=== HERO POWER & WEAPON AUDIT ({map} / {era} / {mode}): {} hero weapon entries ===",
        rows.len()
    );

    let mut hero_saber_hash: BTreeMap<String, u32> = BTreeMap::new();
    for (hero, role, wpn) in &rows {
        if role == "saber" {
            hero_saber_hash.entry(hero.clone()).or_insert_with(|| zero_hash(wpn));
        }
    }
    let mut tested_cache: BTreeMap<(String, String), (f64, f64, f64, f64, String)> = BTreeMap::new();
    let mut failures = Vec::<String>::new();

    for (hero, role, wpn) in &rows {
        let whash = zero_hash(wpn);
        let Some(cid) = sp_ref.runtime.phase.swbf2_class_by_hash(whash) else {
            failures.push(format!("{hero} [{role}] {wpn}: class missing in sound runtime"));
            continue;
        };
        let phase = &sp_ref.runtime.phase;
        let sw = phase.swbf2().unwrap();
        let ext = &sw.classes[cid];

        let fire_id = phase.sound(cid, class::FIRE);
        let loop_id = phase.sound(cid, class::FIRE_LOOP);
        let ord_cls = ext.ordnance_class;
        let ord_hum = if ord_cls != NONE { phase.sound(ord_cls, class::ORDNANCE) } else { NONE };
        let ord_col = if ord_cls != NONE { phase.sound(ord_cls, class::IMPACT) } else { NONE };
        let ord_bounce = if ord_cls != NONE { phase.sound(ord_cls, class::BOUNCE) } else { NONE };
        let ord_tick = if ord_cls != NONE { phase.sound(ord_cls, class::TICK) } else { NONE };
        let exp_cls = if ord_cls != NONE && sw.classes[ord_cls].explosion_class != NONE {
            sw.classes[ord_cls].explosion_class
        } else {
            ext.explosion_class
        };
        let exp_snd = if exp_cls != NONE { phase.sound(exp_cls, class::EXPLOSION) } else { NONE };

        let (player_rms, bot_rms, hit_or_loop_rms, stop_tail_rms, note) = tested_cache
            .entry((role.clone(), wpn.clone()))
            .or_insert_with(|| match role.as_str() {
                "force_push" | "force_pull" | "aura" | "combo_blast" => {
                    // Player at OWN (0m) and bot at emitter 1 (10m).
                    let (_, p_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, OWN, 0.0);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_power_event(PowerAudioEvent {
                                    caster_key: 1,
                                    emitter: OWN,
                                    weapon_hash: whash,
                                    target_weapon_hash: 0,
                                    kind: PowerAudioKind::Cast,
                                    caster_pos: [0.0, 0.0, 0.0],
                                    target_pos: None,
                                });
                            }
                        })
                    };
                    let (_, b_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, 1, 10.0);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_power_event(PowerAudioEvent {
                                    caster_key: 2,
                                    emitter: 1,
                                    weapon_hash: whash,
                                    target_weapon_hash: 0,
                                    kind: PowerAudioKind::Cast,
                                    caster_pos: [10.0, 0.0, 0.0],
                                    target_pos: None,
                                });
                            }
                        })
                    };
                    (p_rms, b_rms, 0.0, 0.0, "cast".into())
                }
                "force_lightning" => {
                    let (_, p_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, OWN, 0.0);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_power_event(PowerAudioEvent {
                                    caster_key: 1,
                                    emitter: OWN,
                                    weapon_hash: whash,
                                    target_weapon_hash: 0,
                                    kind: PowerAudioKind::Cast,
                                    caster_pos: [0.0, 0.0, 0.0],
                                    target_pos: None,
                                });
                            }
                        })
                    };
                    let (_, b_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, 1, 10.0);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_power_event(PowerAudioEvent {
                                    caster_key: 2,
                                    emitter: 1,
                                    weapon_hash: whash,
                                    target_weapon_hash: 0,
                                    kind: PowerAudioKind::Cast,
                                    caster_pos: [10.0, 0.0, 0.0],
                                    target_pos: None,
                                });
                            }
                        })
                    };
                    let (_, h_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, 1, 10.0);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_power_event(PowerAudioEvent {
                                    caster_key: 2,
                                    emitter: 1,
                                    weapon_hash: whash,
                                    target_weapon_hash: 0,
                                    kind: PowerAudioKind::Hit,
                                    caster_pos: [10.0, 0.0, 0.0],
                                    target_pos: Some([5.0, 0.0, 0.0]),
                                });
                            }
                        })
                    };
                    (p_rms, b_rms, h_rms, 0.0, "cast+hit".into())
                }
                "force_choke" => {
                    let (_, p_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, OWN, 0.0);
                        let hold = [PowerHoldInput {
                            key: 1,
                            emitter: OWN,
                            weapon_hash: whash,
                            caster_pos: [0.0, 0.0, 0.0],
                            target_pos: [5.0, 0.0, 0.0],
                        }];
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            sp.runtime.phase.queue_power_holds(300, if t >= 1 { &hold } else { &[] });
                        })
                    };
                    let (mut sp, mut out) = prepare_isolated_sound(&content);
                    observe_soldier_emitter(&mut sp, 1, 10.0);
                    let hold = [PowerHoldInput {
                        key: 2,
                        emitter: 1,
                        weapon_hash: whash,
                        caster_pos: [10.0, 0.0, 0.0],
                        target_pos: [4.0, 0.0, 0.0],
                    }];
                    let (_, b_rms, _) = step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                        sp.runtime.phase.queue_power_holds(300, if t >= 1 { &hold } else { &[] });
                    });
                    // Release and verify silence after fade-out.
                    let _ = step_isolated_ticks(&mut sp, &mut out, 12, |sp, _| {
                        sp.runtime.phase.queue_power_holds(300, &[]);
                    });
                    let (_, tail_rms, _) = step_isolated_ticks(&mut sp, &mut out, 8, |sp, _| {
                        sp.runtime.phase.queue_power_holds(300, &[]);
                    });
                    (p_rms, b_rms, b_rms, tail_rms, "cast+stop".into())
                }
                "thrown_blade" => {
                    let (mut sp, mut out) = prepare_isolated_sound(&content);
                    observe_soldier_emitter(&mut sp, OWN, 0.0);
                    let (_, p_rms, _) = step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                        let tr = [SaberThrowInput {
                            key: 10,
                            owner: OWN,
                            weapon_hash: whash,
                            position: [t as f64 * 0.4, 1.0, 0.0],
                            velocity: [24.0, 0.0, 0.0],
                        }];
                        sp.runtime.phase.queue_saber_throws(340, if t >= 1 { &tr } else { &[] });
                    });
                    let (mut sp, mut out) = prepare_isolated_sound(&content);
                    observe_soldier_emitter(&mut sp, 1, 10.0);
                    // Fly for 50 ticks (0.83 s, past the ~0.65 s launch one-shot) so both launch
                    // and sustained spinning blade loop are measured before catching.
                    let (_, b_rms, _) = step_isolated_ticks(&mut sp, &mut out, 50, |sp, t| {
                        let tr = [SaberThrowInput {
                            key: 20,
                            owner: 1,
                            weapon_hash: whash,
                            position: [10.0 - (t.min(25) as f64) * 0.2, 1.0, 0.0],
                            velocity: [-12.0, 0.0, 0.0],
                        }];
                        sp.runtime.phase.queue_saber_throws(340, if t >= 1 { &tr } else { &[] });
                    });
                    let _ = step_isolated_ticks(&mut sp, &mut out, 12, |sp, _| {
                        sp.runtime.phase.queue_saber_throws(340, &[]);
                    });
                    let (_, tail_rms, _) = step_isolated_ticks(&mut sp, &mut out, 8, |sp, _| {
                        sp.runtime.phase.queue_saber_throws(340, &[]);
                    });
                    let blade_hash = hero_saber_hash.get(hero).copied().unwrap_or(0);
                    let (_, hit_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        step_isolated_ticks(&mut sp, &mut out, 14, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_saber_event(SaberEvent::Hit {
                                    owner: 1,
                                    weapon_class_hash: blade_hash,
                                    position: [5.0, 1.0, 0.0],
                                });
                            }
                        })
                    };
                    (p_rms, b_rms, hit_rms, tail_rms, "throw+twirl+hit+stop".into())
                }
                "saber" => {
                    let (_, p_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, OWN, 0.0);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_saber_event(SaberEvent::SetBladeActive {
                                    owner: OWN,
                                    weapon_class_hash: whash,
                                    active: true,
                                });
                            }
                        })
                    };
                    let (_, b_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, 1, 8.0);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_saber_event(SaberEvent::SetBladeActive {
                                    owner: 1,
                                    weapon_class_hash: whash,
                                    active: true,
                                });
                            }
                        })
                    };
                    let (_, h_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        step_isolated_ticks(&mut sp, &mut out, 14, |sp, t| {
                            if t == 1 {
                                sp.runtime.phase.push_saber_event(SaberEvent::Hit {
                                    owner: 1,
                                    weapon_class_hash: whash,
                                    position: [5.0, 1.0, 0.0],
                                });
                            }
                        })
                    };
                    (p_rms, b_rms, h_rms, 0.0, "ignite+hum+hit".into())
                }
                _ => {
                    // Gun / offhand weapon: player FIRE + bot ACTOR_FIRE + hit/exp/tick if authored.
                    let repair = fire_id == NONE && loop_id != NONE;
                    let measure_gun = |owner: usize, x: f64, is_own: bool| {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        observe_soldier_emitter(&mut sp, owner, x);
                        let mut wrow = [0.0; weapon::STRIDE];
                        wrow[weapon::ID] = owner as f64;
                        wrow[weapon::OWNER] = owner as f64;
                        wrow[weapon::CLASS] = cid as f64;
                        wrow[weapon::SELECTED] = 1.0;
                        wrow[weapon::FIRING] = if repair { 1.0 } else { 0.0 };
                        sp.runtime.phase.observe_weapon(&wrow);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                let mut ev = [0.0; event::STRIDE];
                                ev[event::KIND] = if is_own {
                                    event_kind::FIRE
                                } else {
                                    event_kind::ACTOR_FIRE
                                } as f64;
                                ev[event::OWNER] = owner as f64;
                                ev[event::WEAPON] = owner as f64;
                                ev[event::CLASS] = cid as f64;
                                ev[event::VALUE] = repair as u8 as f64;
                                sp.runtime.events.push(ev);
                            }
                        })
                    };
                    let (_, p_rms, _) = measure_gun(OWN, 0.0, true);
                    let (_, b_rms, _) = measure_gun(1, 10.0, false);
                    let (_, h_rms, _) = {
                        let (mut sp, mut out) = prepare_isolated_sound(&content);
                        step_isolated_ticks(&mut sp, &mut out, 18, |sp, t| {
                            if t == 1 {
                                if ord_col != NONE {
                                    let mut ev = [0.0; event::STRIDE];
                                    ev[event::KIND] = event_kind::IMPACT as f64;
                                    ev[event::CLASS] = ord_cls as f64;
                                    ev[event::PLACED] = 1.0;
                                    ev[event::X] = 8.0;
                                    sp.runtime.events.push(ev);
                                }
                                if exp_snd != NONE {
                                    let mut ev = [0.0; event::STRIDE];
                                    ev[event::KIND] = event_kind::EXPLOSION as f64;
                                    ev[event::CLASS] = exp_cls as f64;
                                    ev[event::PLACED] = 1.0;
                                    ev[event::X] = 8.0;
                                    sp.runtime.events.push(ev);
                                }
                                if ord_tick != NONE {
                                    sp.runtime.phase.play_class(
                                        ord_cls,
                                        class::TICK,
                                        audio_control::engine::WEAPONS,
                                        NONE,
                                        Some([8.0, 0.0, 0.0]),
                                        Some(false),
                                    );
                                }
                            }
                        })
                    };
                    (p_rms, b_rms, h_rms, 0.0, "gun/offhand".into())
                }
            })
            .clone();

        let expects_cast = match role.as_str() {
            "combo_blast" => exp_snd != NONE && sound_has_samples(phase, exp_snd),
            "saber" => ext.saber_turn_on != NONE || ext.saber_on_hum != NONE,
            "thrown_blade" => fire_id != NONE || ord_hum != NONE,
            _ => {
                (fire_id != NONE && sound_has_samples(phase, fire_id))
                    || (loop_id != NONE && sound_has_samples(phase, loop_id))
            }
        };
        let expects_hit = match role.as_str() {
            "force_lightning" => exp_snd != NONE && sound_has_samples(phase, exp_snd),
            "thrown_blade" => true,
            "saber" => ext.saber_hit != NONE && sound_has_samples(phase, ext.saber_hit),
            "gun_or_offhand" => {
                (ord_col != NONE && sound_has_samples(phase, ord_col))
                    || (exp_snd != NONE && sound_has_samples(phase, exp_snd))
                    || (ord_tick != NONE && sound_has_samples(phase, ord_tick))
            }
            _ => false,
        };
        let ok_cast = !expects_cast || (player_rms > 1e-4 && bot_rms > 1e-4);
        let ok_hit = !expects_hit || hit_or_loop_rms > 1e-4;
        let ok_stop = !matches!(role.as_str(), "force_choke" | "thrown_blade") || stop_tail_rms < 1e-4;

        println!(
            "HERO_WPN | {hero:22} | {role:15} | {wpn:34} | fire={} loop={} ord={} col={} bnc={} tick={} exp={} | p_rms={player_rms:.4} b_rms={bot_rms:.4} hit_rms={hit_or_loop_rms:.4} tail={stop_tail_rms:.5} | {note} | {}",
            sound_name(&sp_ref.sound_names, fire_id),
            sound_name(&sp_ref.sound_names, loop_id),
            sound_name(&sp_ref.sound_names, ord_hum),
            sound_name(&sp_ref.sound_names, ord_col),
            sound_name(&sp_ref.sound_names, ord_bounce),
            sound_name(&sp_ref.sound_names, ord_tick),
            sound_name(&sp_ref.sound_names, exp_snd),
            if ok_cast && ok_hit && ok_stop { "PLAYS=YES" } else { "PLAYS=FAIL" }
        );

        if !(ok_cast && ok_hit && ok_stop) {
            failures.push(format!(
                "{hero} [{role}] {wpn}: ok_cast={ok_cast} (p={player_rms:.4}, b={bot_rms:.4}) ok_hit={ok_hit} ({hit_or_loop_rms:.4}) ok_stop={ok_stop} ({stop_tail_rms:.5})"
            ));
        }
    }

    let _ = ctx.dispose();
    assert!(
        failures.is_empty(),
        "Mos Eisley hero power/weapon audit failures:\n{}",
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires prepared 2005 content in BFE_SWBF2_CONTENT_DIR"]
fn tat2_live_battle_player_and_bots_bounded() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("BFE_SWBF2_CONTENT_DIR");
    let map = std::env::var("BFE_SWBF2_HERO_MAP").unwrap_or_else(|_| "tat2".into());
    let era = std::env::var("BFE_SWBF2_HERO_ERA").unwrap_or_else(|_| "gcw".into());
    let mode = std::env::var("BFE_SWBF2_HERO_MODE").unwrap_or_else(|_| "eli".into());
    let side = if era == "cw" { "rep" } else { "all" };

    let selection = MissionSelection {
        family: bfe_content::GameFamily::Swbf2,
        map: map.clone(),
        era: era.clone(),
        mode: mode.clone(),
        seed: 7,
    };
    let mut admit_opts = bfe_sim::world::admit::Options::default();
    admit_opts.family = bfe_family::GameFamily::Swbf2;
    admit_opts.map = map.clone();
    admit_opts.side = side.into();
    admit_opts.mode = mode;
    admit_opts.seed = 7;

    let ctx = Context::new();
    let (prepared, journal) = BattlePrepared::developer_mission_in(
        ctx.clone(),
        Path::new(&root),
        &admit_opts,
        &selection,
        MissionPreferences { direct_spawn: true, ..Default::default() },
    )
    .expect("admit tat2 mission");

    let content = Loader::from_resolved_root(&root, bfe_family::GameFamily::Swbf2)
        .load_scripted_mission(&map, &journal.resources.query, &journal.resources.mission_json)
        .expect("load tat2 content");
    let mut sp = SoundPrepared::new(&content).expect("prepare tat2 sound");
    let mut output = build_fresh_output(&mut sp);
    let mut col = NoCollision;

    let mut slot = PublicationSlot::new_preallocated();
    let mut rt = BattleRuntime::new_into_slot(
        *prepared.engine,
        *prepared.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
        &mut slot,
    )
    .expect("create tat2 runtime");

    let dt = 1.0 / 60.0;
    let mut power_seq = 0u32;
    let mut contact_seq = 0u32;
    let mut player_power_casts = 0u64;
    let mut bot_power_casts = 0u64;
    let mut power_hits = 0u64;
    let mut choke_hold_ticks = 0u64;
    let mut throw_ticks = 0u64;
    let mut saber_contacts = 0u64;
    let mut peak_bot_sabers = 0usize;
    let mut peak_real = 0u64;
    let mut peak_virtual = 0u64;
    let mut peak_groups = 0usize;
    let mut sum_sq = 0.0_f64;
    let mut peak_pcm = 0.0_f32;
    let mut tracked_sabers: BTreeMap<(u32, u32), (usize, u32, u32, u8, bool)> = BTreeMap::new();
    let mut holds_buf = Vec::with_capacity(32);
    let mut throws_buf = Vec::with_capacity(32);

    let ticks = 1800usize; // 30 seconds of Mos Eisley hero assault so both sides engage in force combat
    for tick in 1..=ticks {
        let local_gen = slot.infantry.map_or(1.0, |s| s.generation);
        let next = rt.tick() + 1.0;
        let cast_pulse = tick % 90 == 30 || tick % 90 == 70;
        let hold_secondary = (30..50).contains(&(tick % 90)) || (70..85).contains(&(tick % 90));
        let cycle_secondary = tick % 180 == 25;
        let held_bits = (1u32 << 0) // ClientHeld::Forward
            | if hold_secondary { 1u32 << 7 } else { 0 }; // ClientHeld::Secondary
        let action_bits = if cast_pulse { 1u32 << 5 } else { 0 } // ClientAction::FireSecondary
            | if cycle_secondary { 1u32 << 9 } else { 0 }; // ClientAction::NextSecondary
        let cmd = ClientCommand {
            tick: next,
            sequence: next,
            generation: local_gen,
            held: held_bits,
            actions: action_bits,
            secondary_count: if cast_pulse { 1 } else { 0 },
            look_count: 1,
            has_aspect: true,
            aspect: 16.0 / 9.0,
            has_mouse_scale: true,
            mouse_scale: 1.0,
            ..Default::default()
        };
        let flight = ClientFlightCommand { scale: 1.0, curve: 1.0, ..Default::default() };
        let look = ClientLook::default();
        let mut controls = bfe_sim::units::infantry::Controls::default();
        controls.offhand_pressed = cast_pulse;
        controls.melee_pressed = if tick % 45 == 10 { 1 } else { 0 };
        controls.melee_held = if (10..20).contains(&(tick % 45)) { 1 } else { 0 };

        rt.submit(&cmd, &look, &flight, rt.tick(), None, false).unwrap();
        rt.infantry_input(next, controls).unwrap();
        rt.step_into_slot(&mut slot).unwrap();

        let wall = output.frame as f64 / 48000.0;
        sp.runtime.clear_output();
        sp.runtime.events.clear();
        if !sp.runtime.phase.started {
            let mut start = [0.0; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            sp.runtime.events.push(start);
        }

        let mut listener_pos = [0.0, 1.6, 0.0];
        let listener_fwd = [0.0, 0.0, 1.0];
        let local_slot_id = slot.infantry.map_or(u32::MAX, |s| s.id);
        let to_eid = |s: u32| if s == local_slot_id { OWN } else { (s as usize % 250) + 1 };
        if let Some(snap) = slot.hero_audio.as_deref() {
            let mut observe = |eid: usize, gen_val: u32, pos: [f64; 3]| {
                let mut row = [0.0; emitter::STRIDE];
                row[emitter::ID] = eid as f64;
                row[emitter::KIND] = 1.0;
                row[emitter::ALIVE] = 1.0;
                row[emitter::GENERATION] = f64::from(gen_val);
                row[emitter::X..emitter::X + 3].copy_from_slice(&pos);
                sp.runtime.phase.observe_emitter(&row);
            };
            for s in snap.sabers() {
                let eid = if s.human {
                    listener_pos = [s.position[0], s.position[1] + 1.6, s.position[2]];
                    OWN
                } else {
                    to_eid(s.owner_slot)
                };
                observe(eid, s.owner_generation, s.position);
            }
            for ev in snap.power_events_since(power_seq) {
                observe(to_eid(ev.caster_slot), ev.caster_generation, ev.caster_pos);
                if !ev.bot {
                    listener_pos = [ev.caster_pos[0], ev.caster_pos[1] + 1.6, ev.caster_pos[2]];
                }
            }
            for h in snap.holds() {
                observe(to_eid(h.caster_slot), h.caster_generation, h.caster_pos);
            }
        }

        if let Some(contacts) = slot.contacts.as_deref() {
            use bfe_sim::units::infantry::contact::{Cause, Outcome};
            let to_eid = |s: u32| if s == local_slot_id { OWN } else { (s as usize % 250) + 1 };
            let lookup_blade = |s_slot: u32, s_gen: u32| -> Option<u32> {
                slot.hero_audio
                    .as_deref()
                    .and_then(|snap| {
                        snap.throws()
                            .find(|t| t.owner_slot == s_slot && t.owner_generation == s_gen)
                            .map(|t| t.saber_weapon)
                            .or_else(|| {
                                snap.sabers()
                                    .find(|s| s.owner_slot == s_slot && s.owner_generation == s_gen)
                                    .map(|s| s.weapon_hash)
                            })
                    })
                    .or_else(|| tracked_sabers.get(&(s_slot, s_gen)).map(|t| t.1))
            };
            for c_ev in contacts.since(contact_seq) {
                contact_seq = c_ev.sequence;
                match (c_ev.cause, c_ev.outcome) {
                    (Cause::Saber { weapon, .. }, Outcome::Hit) => {
                        saber_contacts += 1;
                        let attacker = c_ev.source.map_or(NONE, |src| to_eid(src.slot));
                        sp.runtime.phase.push_saber_event(SaberEvent::Hit {
                            owner: attacker,
                            weapon_class_hash: weapon,
                            position: c_ev.point,
                        });
                    }
                    (Cause::Throw { .. }, Outcome::Hit) => {
                        saber_contacts += 1;
                        let attacker = c_ev.source.map_or(NONE, |src| to_eid(src.slot));
                        let weapon = c_ev
                            .source
                            .and_then(|src| lookup_blade(src.slot, src.generation))
                            .unwrap_or(0);
                        sp.runtime.phase.push_saber_event(SaberEvent::Hit {
                            owner: attacker,
                            weapon_class_hash: weapon,
                            position: c_ev.point,
                        });
                    }
                    (_, Outcome::Blocked) => {
                        saber_contacts += 1;
                        let defender = to_eid(c_ev.target.slot);
                        if let Some(weapon_class_hash) =
                            lookup_blade(c_ev.target.slot, c_ev.target.generation)
                        {
                            sp.runtime.phase.push_saber_event(SaberEvent::Deflect {
                                owner: defender,
                                weapon_class_hash,
                                position: c_ev.point,
                            });
                        } else {
                            sp.runtime.phase.push_saber_event(SaberEvent::Block {
                                owner: defender,
                                sound_hash: 0,
                                position: c_ev.point,
                            });
                        }
                    }
                    (_, Outcome::Deflected) => {
                        if let Some(weapon_class_hash) =
                            lookup_blade(c_ev.target.slot, c_ev.target.generation)
                        {
                            saber_contacts += 1;
                            sp.runtime.phase.push_saber_event(SaberEvent::Deflect {
                                owner: to_eid(c_ev.target.slot),
                                weapon_class_hash,
                                position: c_ev.point,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(snap) = slot.hero_audio.as_deref() {
            use bfe_sim::combat::powers::PowerEventKind;
            let to_eid = |s: u32| if s == local_slot_id { OWN } else { (s as usize % 250) + 1 };

            for ev in snap.power_events_since(power_seq) {
                power_seq = ev.sequence;
                let kind = match ev.kind {
                    PowerEventKind::Cast => {
                        if ev.bot { bot_power_casts += 1; } else { player_power_casts += 1; }
                        PowerAudioKind::Cast
                    }
                    PowerEventKind::Hit => { power_hits += 1; PowerAudioKind::Hit }
                    PowerEventKind::Thrown => { power_hits += 1; PowerAudioKind::Thrown }
                    PowerEventKind::Blocked => PowerAudioKind::Blocked,
                    PowerEventKind::Held => PowerAudioKind::Held,
                    PowerEventKind::Buffed => PowerAudioKind::Buffed,
                };
                sp.runtime.phase.push_power_event(PowerAudioEvent {
                    caster_key: (u64::from(ev.caster_slot) << 32) | u64::from(ev.caster_generation),
                    emitter: to_eid(ev.caster_slot),
                    weapon_hash: ev.weapon,
                    target_weapon_hash: ev.target_weapon,
                    kind,
                    caster_pos: ev.caster_pos,
                    target_pos: ev.target_pos,
                });
            }

            holds_buf.clear();
            for h in snap.holds() {
                choke_hold_ticks += 1;
                holds_buf.push(PowerHoldInput {
                    key: (u64::from(h.caster_slot) << 32) | u64::from(h.caster_generation),
                    emitter: to_eid(h.caster_slot),
                    weapon_hash: h.weapon,
                    caster_pos: h.caster_pos,
                    target_pos: h.target_pos,
                });
            }
            sp.runtime.phase.queue_power_holds(300, &holds_buf);

            throws_buf.clear();
            for tr in snap.throws() {
                throw_ticks += 1;
                throws_buf.push(SaberThrowInput {
                    key: (u64::from(tr.owner_slot) << 32) | u64::from(tr.launched),
                    owner: to_eid(tr.owner_slot),
                    weapon_hash: tr.weapon,
                    position: tr.position,
                    velocity: tr.velocity,
                });
            }
            sp.runtime.phase.queue_saber_throws(340, &throws_buf);

            let mut alive_keys = BTreeSet::new();
            let mut bot_sabers_now = 0usize;
            for s in snap.sabers() {
                if !s.human {
                    bot_sabers_now += 1;
                }
                let k = (s.owner_slot, s.owner_generation);
                alive_keys.insert(k);
                let eid = to_eid(s.owner_slot);
                match tracked_sabers.get(&k).copied() {
                    None => {
                        if s.blades_off != u8::MAX {
                            sp.runtime.phase.push_saber_event(SaberEvent::SetBladeActive {
                                owner: eid,
                                weapon_class_hash: s.weapon_hash,
                                active: true,
                            });
                        }
                        sp.runtime.phase.push_saber_event(SaberEvent::ComboState {
                            owner: eid,
                            combo_hash: s.combo_hash,
                            state_hash: s.state_hash,
                        });
                    }
                    Some((_, _, prev_state, prev_off, prev_thrown)) => {
                        if prev_off != s.blades_off {
                            sp.runtime.phase.push_saber_event(SaberEvent::SetBladeActive {
                                owner: eid,
                                weapon_class_hash: s.weapon_hash,
                                active: s.blades_off != u8::MAX,
                            });
                        }
                        if prev_thrown != s.thrown {
                            sp.runtime.phase.push_saber_event(SaberEvent::SetThrown {
                                owner: eid,
                                thrown: s.thrown,
                            });
                        }
                        if prev_state != s.state_hash {
                            sp.runtime.phase.push_saber_event(SaberEvent::ComboState {
                                owner: eid,
                                combo_hash: s.combo_hash,
                                state_hash: s.state_hash,
                            });
                        }
                    }
                }
                tracked_sabers.insert(k, (eid, s.weapon_hash, s.state_hash, s.blades_off, s.thrown));
            }
            peak_bot_sabers = peak_bot_sabers.max(bot_sabers_now);
            let gone: Vec<_> = tracked_sabers
                .keys()
                .copied()
                .filter(|k| !alive_keys.contains(k))
                .collect();
            for k in gone {
                if let Some((eid, wpn, _, _, _)) = tracked_sabers.remove(&k) {
                    sp.runtime.phase.push_saber_event(SaberEvent::Extinguish {
                        owner: eid,
                        weapon_class_hash: wpn,
                    });
                }
            }
        }

        let frame = make_frame(wall, dt, listener_pos, listener_fwd);
        sp.runtime.phase.frame.copy_from_slice(&frame);
        sp.runtime.step(&mut col);
        apply_batch(&mut output, &sp.runtime);

        let real = sp.runtime.phase.engine.stats[0];
        let virt = sp.runtime.phase.engine.stats[1];
        let groups = sp.runtime.phase.layers.active_non_param_count();
        peak_real = peak_real.max(real);
        peak_virtual = peak_virtual.max(virt);
        peak_groups = peak_groups.max(groups);
        assert!(real <= 32, "real voices exceeded 32 at tick {tick}: {real}");
        assert!(virt <= 64, "virtual voices exceeded 64 at tick {tick}: {virt}");
        assert!(groups <= 64, "non-param groups exceeded 64 at tick {tick}: {groups}");

        let (r, p) = render_rms(&mut output, FRAMES_PER_TICK);
        sum_sq += r * r;
        peak_pcm = peak_pcm.max(p);
    }

    let rms = (sum_sq / ticks as f64).sqrt();
    let stats = sp.runtime.phase.engine.stats;
    println!(
        "TAT2 LIVE BATTLE ({ticks} ticks): player_power_casts={player_power_casts} bot_power_casts={bot_power_casts} power_hits={power_hits} choke_hold_ticks={choke_hold_ticks} throw_ticks={throw_ticks} saber_contacts={saber_contacts} peak_bot_sabers={peak_bot_sabers} | voices_started={} stolen={} dropped={} culled={} | peak_real={peak_real}/32 peak_virtual={peak_virtual}/64 peak_groups={peak_groups}/64 | rms={rms:.4} peak_pcm={peak_pcm:.4}",
        stats[2], stats[3], stats[4], stats[5]
    );
    assert!(peak_bot_sabers > 0, "expected live bot saber heroes");
    assert!(bot_power_casts > 0, "expected bot hero power casts in live battle");
    assert!(player_power_casts > 0 || throw_ticks > 0, "expected player hero power or saber throw");
    assert!(stats[2] > 20, "expected active audio voices started");
    assert!(rms > 0.001 && peak_pcm > 0.01, "expected audible non-silent stereo PCM");
    rt.close();
}
