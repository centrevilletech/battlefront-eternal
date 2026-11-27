use bfe_wide::Fnv;
use bfe_client::local::{launch::Prepared, runtime::Runtime, thread::PublicationSlot};
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};

#[test]
#[ignore = "requires prepared 2005 missions"]
fn developer_local_missions_publish() {
    use bfe_sim::world::admit::{mission::*, Bots, Options};
    let root = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
    let playlist = bfe_shell::launch::parse_rotation(Some("swbf2/tat2:gcw:eli,swbf2/spa6:cw:ass"));
    assert_eq!(playlist.len(), 2);
    for (entry, side) in playlist.iter().zip(["all", "rep"]) {
        let map = entry.map.as_str();
        let options = Options {
            family: entry.family, mode: entry.mode.clone(), practice_target: None,
            map: map.into(),
            seed: 59,
            side: side.into(),
            unit: None,
            enhanced: false,
            heroes: false,
            name: None,
            bots: Bots::read("classic", "medium", "full"),
        };
        let (prepared, journal) = Prepared::developer_playlist_entry(
            &root,
            &options,
            entry,
            MissionPreferences {
                bots_per_team: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(journal.selection.family.as_str(), entry.family.as_str());
        assert_eq!(journal.selection.map, entry.map);
        assert_eq!(Some(&journal.selection.era), entry.era.as_ref());
        assert_eq!(journal.selection.mode, entry.mode);
        assert_eq!(journal.selection.seed, options.seed);
        assert_eq!(prepared.admission.query, journal.resources.query);
        assert!(prepared.admission.query.contains("&mission="));
        assert!(journal.resources.admission_digest.starts_with("bfe-admission-sha256-"));
        assert!(!prepared.admission.publication.is_empty());
        let files = bfe_content::files::DirFiles::from_resolved_root(&root, bfe_family::GameFamily::Swbf2);
        let (replay, admission) = journal.replay(&files).unwrap();
        assert_eq!(admission.publication, prepared.admission.publication);
        assert!(replay.mission_state().is_some());
        drop(replay);
        let mut rt = Runtime::new(
            *prepared.engine,
            *prepared.admission,
            CLOCK_START_MS,
            CLOCK_UNIX_MS,
        )
        .unwrap();
        let mut slot = PublicationSlot::new_preallocated();
        for _ in 0..30 {
            rt.step_into_slot(&mut slot).unwrap();
        }
        println!(
            "Local {map} ticks={} admission={}",
            rt.tick(),
            journal.resources.admission_digest
        );
        assert_eq!(rt.tick(), 30.0);
        rt.close();
    }
}

/// Play an input recording twice through the same authority as Instant Action.
/// Prepared game files, the selected class and the recording are supplied locally.
#[test]
#[ignore = "requires a prepared mission and a fixed-tick input recording"]
fn recorded_match_repeats() {
    use bfe_client::local::mission_input;
    use bfe_sim::{abi::context::Context, world::admit::{mission::*, Bots, Options}};
    use std::path::PathBuf;
    let Some(path) = std::env::var_os("BFE_MATCH_INPUT") else { return };
    let frames = mission_input::read(&PathBuf::from(path)).unwrap();
    let root = PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
    let seed = std::env::var("BFE_MATCH_SEED").unwrap().parse().unwrap();
    let selection = MissionSelection {
        family: bfe_family::GameFamily::Swbf2, map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed,
    };
    let ai = std::env::var("BFE_MATCH_AI").unwrap_or_else(|_| "classic".into());
    let difficulty = std::env::var("BFE_MATCH_DIFFICULTY").unwrap_or_else(|_| "medium".into());
    let mut bots = Bots::read(&ai, &difficulty, "full");
    if let Ok(value) = std::env::var("BFE_MATCH_FRIENDLY_FIRE") {
        bots.friendly_fire = value
            .parse::<bool>()
            .expect("BFE_MATCH_FRIENDLY_FIRE must be true or false");
    }
    let preferences = MissionPreferences {
        bots_per_team: std::env::var("BFE_MATCH_AI_UNITS_PER_TEAM").ok()
            .map(|value| value.parse().expect("BFE_MATCH_AI_UNITS_PER_TEAM must be a count")),
        ..Default::default()
    };
    let options = Options { family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(),
        side: "all".into(), unit: Some(std::env::var("BFE_MATCH_UNIT").unwrap()), seed, enhanced: ai == "enhanced",
        bots, ..Default::default() };
    let mut first = Vec::new();
    for run in 1..=2 {
        let context = Context::new();
        let (prepared, _) = Prepared::developer_mission_in(context.clone(), &root, &options, &selection,
            preferences).unwrap();
        let mut slot = PublicationSlot::new_preallocated();
        let mut runtime = Runtime::new_into_slot(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot).unwrap();
        let mut trace = (run == 1).then(||std::env::var_os("BFE_BENCH_MATCH_TRACE")).flatten()
            .map(|p| std::io::BufWriter::new(std::fs::File::create(p).unwrap()));
        let mut digest = Fnv::new();
        let mut checkpoints = Vec::new();
        let mut deployed = false;
        for (index, frame) in frames.iter().enumerate() {
            let tick = index + 1;
            frame.submit(&mut runtime, slot.infantry.map_or(0., |s| s.generation)).unwrap();
            runtime.step_into_slot(&mut slot).unwrap();
            let score = slot.mission_score.unwrap();
            if !deployed && slot.infantry.is_some() {
                deployed = context.owned(|w| {
                    w.local_mission_connection().is_some_and(|c| c.spawned_once)
                }).unwrap();
            }
            if let Some(file) = &mut trace {
                use std::io::Write;
                writeln!(file, "{tick}\t{}\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}",score.script_tick,
                    score.teams[0],score.teams[1],score.target.unwrap_or_default(),score.outcome,
                    score.over,score.game_over,score.finished,score.faulted).unwrap();
            }
            assert!(!score.faulted, "mission callback fault at tick {tick}");
            digest.bytes(&slot.bytes);
            for word in slot.block.words.iter().chain(&slot.block.heap) {
                digest.bytes(&word.to_le_bytes());
            }
            if tick % 600 == 0 || tick == frames.len() || score.over {
                let rules = context.owned(|w| w.mission_digest().unwrap()).unwrap();
                let digest = digest.finish();
                println!("match digest {run} {tick} {digest:016x} {rules:016x} {:?} {:?}", score.teams, score.outcome);
                checkpoints.push((tick, digest, rules, score));
            }
            if score.over {
                let Some(bfe_sim::battle::script::state::CompactOutcome::Victory(team @ 1..=2)) = score.outcome else {
                    panic!("expected a team victory at tick {tick}: {score:?}");
                };
                assert!(score.teams[team as usize - 1] >= score.target.unwrap());
                assert_eq!(tick, frames.len(), "recording must end on the deciding tick");
                break;
            }
        }
        assert!(deployed, "the recorded match never fielded the connected player");
        assert_eq!(runtime.refusals(), 0);
        runtime.close();
        if run == 1 { first = checkpoints; } else { assert_eq!(first, checkpoints); }
    }
    let (tick, _, _, score) = first.last().unwrap();
    assert!(score.over && score.outcome.is_some(), "recording ended at tick {tick} with {score:?}");
}

#[test]
#[ignore = "requires a prepared mission with a playable class and a friendly command post"]
fn a_connected_choice_deploys_at_the_selected_mission_post() {
    use bfe_sim::{abi::context::Context, world::admit::{mission::*, Bots, Options}};
    let (Some(root), Ok(unit), Ok(post)) = (
        std::env::var_os("BFE_SWBF2_PREPARED"),
        std::env::var("BFE_MATCH_UNIT"),
        std::env::var("BFE_MATCH_POST"),
    ) else { return };
    let selection = MissionSelection {
        family: bfe_family::GameFamily::Swbf2,
        map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed: 7,
    };
    let options = Options {
        family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(),
        side: "all".into(), unit: Some(unit.clone()), seed: selection.seed,
        bots: Bots::read("classic", "medium", "full"), ..Default::default()
    };
    let context = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(
        context.clone(), std::path::Path::new(&root), &options, &selection,
        MissionPreferences::default(),
    ).unwrap();
    let mut slot = PublicationSlot::new_preallocated();
    let mut runtime = Runtime::new_into_slot(
        *prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot,
    ).unwrap();
    runtime.spawn_human(1, &unit, &post).unwrap();
    let expected_post = context.intern(&post.encode_utf16().collect::<Vec<_>>()).unwrap();
    let expected_class = context.intern(&unit.encode_utf16().collect::<Vec<_>>()).unwrap();
    for _ in 0..600 {
        let generation = slot.infantry.map_or(0., |s| s.generation);
        bfe_client::local::mission_input::Frame::default().submit(&mut runtime, generation).unwrap();
        runtime.step_into_slot(&mut slot).unwrap();
        let view = context.owned(|w| w.local_mission_connection().cloned()).unwrap().unwrap();
        if view.spawned_once {
            assert_eq!(view.class_name, Some(expected_class));
            assert_eq!(view.post, Some(expected_post));
            assert_eq!(runtime.refusals(), 0);
            runtime.close();
            return;
        }
    }
    panic!("the connected player never deployed at the selected post");
}

#[test]
#[ignore = "requires a prepared character with an offhand returning weapon"]
fn an_offhand_press_launches_and_returns_the_equipped_blade() {
    use bfe_sim::{abi::context::Context, units::infantry::Controls, world::admit::{mission::*, Bots, Options}};
    let (Some(root),Ok(unit)) = (std::env::var_os("BFE_SWBF2_PREPARED"),std::env::var("BFE_MATCH_UNIT")) else { return };
    let selection = MissionSelection { family:bfe_family::GameFamily::Swbf2,map:"tat2".into(),mode:"eli".into(),era:"gcw".into(),seed:7 };
    let options = Options {family:selection.family,map:selection.map.clone(),mode:selection.mode.clone(),side:"all".into(),
        unit:Some(unit),seed:selection.seed,bots:Bots::read("classic","medium","full"),..Default::default()};
    let ctx = Context::new();
    let (prepared,_) = Prepared::developer_mission_in(ctx.clone(),std::path::Path::new(&root),&options,&selection,MissionPreferences::default()).unwrap();
    let mut slot = PublicationSlot::new_preallocated();
    let mut runtime = Runtime::new_into_slot(*prepared.engine,*prepared.admission,CLOCK_START_MS,CLOCK_UNIX_MS,&mut slot).unwrap();
    let mut baseline = None;
    let mut launch = None;
    let mut caught = None;
    for n in 1..=500 {
        let input = Controls {offhand_pressed:n==90,..Default::default()};
        use bfe_client::generated_input::{ClientCommand,ClientFlightCommand,ClientLook};
        let command = ClientCommand {tick:f64::from(n),sequence:f64::from(n),generation:slot.infantry.map_or(0.,|s|s.generation),
            has_mouse_scale:true,mouse_scale:1.,has_aspect:true,aspect:16./9.,look_count:1,..Default::default()};
        let flight=ClientFlightCommand{scale:1.,curve:1.,..Default::default()};
        runtime.submit(&command,&ClientLook::default(),&flight,runtime.tick(),None,false).unwrap();
        runtime.infantry_input(f64::from(n),input).unwrap();
        runtime.step_into_slot(&mut slot).unwrap();
        let Some(sample) = slot.infantry else { continue };
        let Some(saber) = sample.saber else { continue };
        if n == 89 { baseline=Some((sample.energy,saber.state_hash)); }
        if let Some((pos,angle)) = saber.thrown {
            let (full,_) = baseline.unwrap();
            assert!(sample.energy < full);
            assert_eq!(sample.blades_off,0);
            assert!(pos.into_iter().all(f64::is_finite) && angle.is_finite());
            launch.get_or_insert(n);
        } else if launch.is_some() {
            caught = Some(n);
            break;
        } else if (90..100).contains(&n) {
            assert_eq!(sample.saber.unwrap().state_hash,baseline.unwrap().1,"offhand raised guard");
        }
    }
    assert_eq!(runtime.refusals(),0);
    runtime.close();
    assert!(launch.is_some_and(|n| n >=90));
    assert!(caught>launch);
    println!("equipped blade launch={launch:?} catch={caught:?}");
}

#[test]
#[ignore = "requires a prepared character with an offhand returning weapon"]
fn throwing_while_blocking_exits_guard_and_hands_over_to_catch_without_gap() {
    use bfe_sim::{abi::context::Context, units::infantry::Controls, world::admit::{mission::*, Bots, Options}};
    let (Some(root), Ok(unit)) = (std::env::var_os("BFE_SWBF2_PREPARED"), std::env::var("BFE_MATCH_UNIT")) else { return };
    let selection = MissionSelection { family: bfe_family::GameFamily::Swbf2, map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed: 7 };
    let options = Options {
        family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(), side: "all".into(),
        unit: Some(unit), seed: selection.seed, bots: Bots::read("classic", "medium", "full"), ..Default::default()
    };
    let ctx = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(ctx.clone(), std::path::Path::new(&root), &options, &selection, MissionPreferences::default()).unwrap();
    let mut slot = PublicationSlot::new_preallocated();
    let mut runtime = Runtime::new_into_slot(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot).unwrap();
    let mut idle_combo = None;
    let mut guard_combo = None;
    let mut launch = None;
    let mut last_flight = None;
    let mut caught = None;
    let mut catch_ended = None;
    let mut min_flight_energy = f64::INFINITY;
    let mut energy_at_launch = None;
    for n in 1..=500u32 {
        let input = Controls {
            block: n == 70,
            block_pressed: n == 70,
            offhand_pressed: n == 95,
            ..Default::default()
        };
        use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
        let command = ClientCommand {
            tick: f64::from(n), sequence: f64::from(n), generation: slot.infantry.map_or(0., |s| s.generation),
            has_mouse_scale: true, mouse_scale: 1., has_aspect: true, aspect: 16. / 9., look_count: 1, ..Default::default()
        };
        let flight = ClientFlightCommand { scale: 1., curve: 1., ..Default::default() };
        runtime.submit(&command, &ClientLook::default(), &flight, runtime.tick(), None, false).unwrap();
        runtime.infantry_input(f64::from(n), input).unwrap();
        runtime.step_into_slot(&mut slot).unwrap();
        let Some(sample) = slot.infantry else { continue };
        let Some(saber) = sample.saber else { continue };
        assert_eq!(sample.blades_off, 0, "tick {n}: blade unexpectedly turned off");
        if n == 69 {
            idle_combo = sample.combo;
        }
        if n == 85 {
            guard_combo = sample.combo;
            assert_ne!(guard_combo, idle_combo, "toggle block must enter DEFLECT");
        }
        if n == 95 {
            assert_eq!(sample.combo, idle_combo, "throw press must exit DEFLECT immediately");
        }
        if saber.thrown.is_some() {
            assert_eq!(sample.combo, idle_combo, "tick {n}: must not stay in DEFLECT during flight");
            launch.get_or_insert(n);
            last_flight = Some(n);
            energy_at_launch.get_or_insert(sample.energy);
            min_flight_energy = min_flight_energy.min(sample.energy);
        } else if launch.is_some() && caught.is_none() {
            caught = Some(n);
            let catch_combo = sample.combo;
            assert_eq!(last_flight, Some(n - 1), "catch must follow last flight tick with zero gap");
            assert_ne!(catch_combo, idle_combo, "catch tick must enter CATCH_SABRE");
            assert_ne!(catch_combo, guard_combo, "catch tick must not be DEFLECT");
        } else if let Some(c) = caught {
            if n > c && sample.combo == idle_combo {
                catch_ended = Some(n);
                break;
            }
        }
    }
    assert_eq!(runtime.refusals(), 0);
    runtime.close();
    assert!(launch.is_some_and(|l| l > 95));
    assert!(caught.is_some_and(|c| c > launch.unwrap()));
    assert!(catch_ended.is_some_and(|e| e > caught.unwrap()));
    assert!(
        min_flight_energy >= energy_at_launch.unwrap() - 1e-6,
        "energy drained during flight after exiting DEFLECT: launch={energy_at_launch:?} min={min_flight_energy}"
    );
}

/// What a client draws of a saber fight comes from the battle's own publications, the
/// ones an online client receives too: every melee soldier's pose arrives as its world
/// field and replays the battle's state and clip time, and every blade hit, block or
/// deflection arrives as one named effect at the contact's point.
#[test]
#[ignore = "requires prepared Hero Assault content"]
fn melee_poses_and_saber_contact_effects_travel_with_the_publications() {
    use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
    use bfe_presentation::network_layout::{net_kind as k, net_record as r};
    use bfe_sim::{
        abi::context::Context,
        units::infantry::contact::{Cause, Outcome},
        wire::melee::{Display, MeleePose, ID_BASE, SCHEMA},
        world::admit::{mission::*, Bots, Options},
    };
    use std::collections::BTreeMap;
    let (Some(root), Ok(unit)) = (std::env::var_os("BFE_SWBF2_PREPARED"), std::env::var("BFE_MATCH_UNIT")) else { return };
    let ticks: u32 = std::env::var("BFE_ROSTER_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(3600);
    let selection = MissionSelection { family: bfe_family::GameFamily::Swbf2, map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed: 7 };
    let options = Options { family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(), side: "all".into(),
        unit: Some(unit), seed: selection.seed, bots: Bots::read("classic", "medium", "full"), ..Default::default() };
    let ctx = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(ctx.clone(), std::path::Path::new(&root), &options, &selection, MissionPreferences::default()).unwrap();
    let mut runtime = Runtime::new(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();
    let mut poses: BTreeMap<(u32, u32), MeleePose> = BTreeMap::new();
    let (mut checked, mut swinging, mut contacts, mut shown) = (0, 0, 0, 0);
    let (mut missing, mut wrong_state, mut wrong_clock, mut unshown) = (0, 0, 0, 0);
    let (mut turned, mut wrong_turn) = (0, 0);
    let mut kinds: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let (mut effects, mut expected) = (Vec::new(), Vec::new());
    let mut cursor = 0;
    for _ in 0..=ticks {
        let tick = runtime.tick();
        for field in runtime.receiver().publications.iter() {
            *kinds.entry((field.row[r::KIND] as u32, field.row[r::SCHEMA] as u32)).or_insert(0usize) += 1;
            let bytes = &field.bytes[..field.length];
            if field.row[r::KIND] == f64::from(k::SUMMARY) && field.row[r::SCHEMA] == SCHEMA {
                let display = Display::decode(bytes).expect("a valid combat display");
                if let Some(pose) = display.melee {
                    poses.insert(((field.row[r::ID] - ID_BASE) as u32, field.row[r::GENERATION] as u32), pose);
                }
            } else if field.row[r::KIND] == f64::from(k::COMBAT) && field.row[r::SCHEMA] == 8. {
                let event: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                if event["kind"] == "effect" {
                    let at: Vec<f64> = event["position"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
                    effects.push((tick, event["name"].as_str().unwrap().to_string(), at));
                }
            }
        }
        runtime.receiver_mut().publications.clear();
        ctx.owned(|w| {
            w.visit_melee(|life, _, state, _, attacking| {
                checked += 1;
                swinging += usize::from(attacking);
                let Some(pose) = poses.get(&(life.slot, life.generation)) else {
                    missing += 1;
                    if missing < 4 { println!("no pose for {life:?} at {tick}"); }
                    return;
                };
                if pose.state != state.state.0 {
                    wrong_state += 1;
                    if wrong_state < 6 { println!("state of {life:?} at {tick}: {} vs {}", pose.state, state.state.0); }
                } else if (pose.anim_time(tick) - state.anim_time).abs() >= 1e-6 {
                    wrong_clock += 1;
                    if wrong_clock < 6 { println!("clip time of {life:?} at {tick}: {} vs {}", pose.anim_time(tick), state.anim_time); }
                }
                let turn = w.melee_swing_turn(life).unwrap();
                turned += usize::from(turn != 0.);
                if (pose.swing_turn - turn).abs() >= 1e-6 {
                    wrong_turn += 1;
                    if wrong_turn < 6 { println!("swing turn of {life:?} at {tick}: {} vs {turn}", pose.swing_turn); }
                }
            });
            let ring = w.infantry_contacts().unwrap();
            for c in ring.since(cursor) {
                let named = match (c.cause, c.outcome) {
                    (Cause::Saber { .. }, Outcome::Hit) => true,
                    (Cause::Saber { .. } | Cause::Throw { .. } | Cause::Bolt { .. } | Cause::Shot { .. }, Outcome::Blocked | Outcome::Deflected) => true,
                    _ => false,
                };
                if named {
                    contacts += 1;
                    expected.push((tick, c));
                }
            }
            cursor = ring.latest;
        }).unwrap();
        let command = ClientCommand { tick: tick + 1., sequence: tick + 1., generation: runtime.receiver().own.generation,
            has_mouse_scale: true, mouse_scale: 1., has_aspect: true, aspect: 16. / 9., look_count: 1, ..Default::default() };
        let flight = ClientFlightCommand { scale: 1., curve: 1., ..Default::default() };
        runtime.step(&command, &ClientLook::default(), &flight).unwrap();
    }
    runtime.close();
    // An effect the battle emits as it records the contact reaches the display with that
    // tick's publication or the next one.
    let mut lag = 0.;
    for (at, c) in &expected {
        let found = effects.iter().position(|(arrived, name, point)| !name.is_empty()
            && (0. ..=2.).contains(&(arrived - at))
            && point.iter().zip(c.point).all(|(a, b)| (a - b).abs() < 1e-6));
        match found {
            Some(found) => { lag = f64::max(lag, effects[found].0 - at); effects.swap_remove(found); shown += 1; }
            None => {
                unshown += 1;
                if unshown < 6 {
                    let near = effects.iter().map(|(t, n, p)| (p.iter().zip(c.point).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt(), t - at, n.clone()))
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    println!("no effect for {:?} {:?} at {at}; nearest {near:?}", c.cause, c.outcome);
                }
            }
        }
    }
    println!("effect lag at most {lag} ticks");
    println!("melee poses checked={checked} attacking={swinging} missing={missing} wrong_state={wrong_state} wrong_clock={wrong_clock} turned={turned} wrong_turn={wrong_turn} saber contacts={contacts} effects shown={shown} unshown={unshown}");
    println!("received kinds/schemas {kinds:?} refused={}", runtime.receiver().publications.refused);
    assert!(checked > 0 && swinging > 0 && contacts > 0);
    assert_eq!((missing, wrong_state, wrong_clock, wrong_turn, unshown), (0, 0, 0, 0, 0));
}

#[test]
#[ignore = "requires a prepared character whose DEFLECT locks the offhand trigger"]
fn throwing_while_blocking_is_refused_when_guard_locks_offhand() {
    use bfe_sim::{abi::context::Context, units::infantry::Controls, world::admit::{mission::*, Bots, Options}};
    let (Some(root), Ok(unit)) = (std::env::var_os("BFE_SWBF2_PREPARED"), std::env::var("BFE_MATCH_UNIT")) else { return };
    let side = std::env::var("BFE_MATCH_SIDE").unwrap_or_else(|_| "all".into());
    let selection = MissionSelection { family: bfe_family::GameFamily::Swbf2, map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed: 7 };
    let options = Options {
        family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(), side,
        unit: Some(unit), seed: selection.seed, bots: Bots::read("classic", "medium", "full"), ..Default::default()
    };
    let ctx = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(ctx.clone(), std::path::Path::new(&root), &options, &selection, MissionPreferences::default()).unwrap();
    let mut slot = PublicationSlot::new_preallocated();
    let mut runtime = Runtime::new_into_slot(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot).unwrap();
    let mut idle_combo = None;
    let mut guard_combo = None;
    let mut energy_before_press = None;
    for n in 1..=160u32 {
        let input = Controls {
            block: n == 70,
            block_pressed: n == 70,
            offhand_pressed: n == 95,
            ..Default::default()
        };
        use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
        let command = ClientCommand {
            tick: f64::from(n), sequence: f64::from(n), generation: slot.infantry.map_or(0., |s| s.generation),
            has_mouse_scale: true, mouse_scale: 1., has_aspect: true, aspect: 16. / 9., look_count: 1, ..Default::default()
        };
        let flight = ClientFlightCommand { scale: 1., curve: 1., ..Default::default() };
        runtime.submit(&command, &ClientLook::default(), &flight, runtime.tick(), None, false).unwrap();
        runtime.infantry_input(f64::from(n), input).unwrap();
        runtime.step_into_slot(&mut slot).unwrap();
        let Some(sample) = slot.infantry else { continue };
        let Some(saber) = sample.saber else { continue };
        if n == 69 {
            idle_combo = sample.combo;
        }
        if n == 85 {
            guard_combo = sample.combo;
            assert_ne!(guard_combo, idle_combo, "toggle block must enter DEFLECT");
        }
        if n == 94 {
            energy_before_press = Some(sample.energy);
        }
        if n >= 95 {
            assert_eq!(sample.combo, guard_combo, "tick {n}: locked DEFLECT must stay active on throw press");
            assert!(saber.thrown.is_none(), "tick {n}: throw must not launch while DEFLECT locks offhand");
            if n == 95 {
                let before = energy_before_press.unwrap();
                assert!(before - sample.energy < 2.0, "tick 95: refused throw must not debit throw stamina ({before} -> {})", sample.energy);
            }
        }
    }
    assert_eq!(runtime.refusals(), 0);
    runtime.close();
}

/// A thrown saber reaches the display the way every other viewer gets it: as the hero's world
/// field in the battle's publications, in Instant Action and online alike. A viewer that
/// replays the flight from that field with the battle's own rule sees the blade where the
/// battle has it, tick for tick, and the field is sent once per press and once per launch.
#[test]
#[ignore = "requires a prepared character with an offhand returning weapon"]
fn a_thrown_saber_travels_with_the_publications_and_replays_where_the_battle_flies_it() {
    use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
    use bfe_presentation::network_layout::{net_kind as k, net_record as r};
    use bfe_sim::{
        abi::context::Context,
        units::infantry::Controls,
        wire::thrown::{Replay, Throw, ID_BASE, SCHEMA},
        world::admit::{mission::*, Bots, Options},
    };
    let (Some(root), Ok(unit)) = (std::env::var_os("BFE_SWBF2_PREPARED"), std::env::var("BFE_MATCH_UNIT")) else { return };
    let selection = MissionSelection { family: bfe_family::GameFamily::Swbf2, map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed: 7 };
    let options = Options { family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(), side: "all".into(),
        unit: Some(unit), seed: selection.seed, bots: Bots::read("classic", "medium", "full"), ..Default::default() };
    let ctx = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(ctx.clone(), std::path::Path::new(&root), &options, &selection, MissionPreferences::default()).unwrap();
    let mut runtime = Runtime::new(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();
    let mut heard: Vec<(f64, Throw)> = Vec::new();
    let mut replay: Option<Replay> = None;
    let (mut matched, mut worst, mut sim_out, mut replay_out, mut ended) = (0, 0f64, 0, 0, None);
    for n in 1..=420u32 {
        let command = ClientCommand { tick: f64::from(n), sequence: f64::from(n), generation: runtime.receiver().own.generation,
            has_mouse_scale: true, mouse_scale: 1., has_aspect: true, aspect: 16. / 9., look_count: 1, ..Default::default() };
        let flight = ClientFlightCommand { scale: 1., curve: 1., ..Default::default() };
        runtime.submit(&command, &ClientLook::default(), &flight, runtime.tick(), None, false).unwrap();
        runtime.infantry_input(f64::from(n), Controls { offhand_pressed: n == 90, ..Default::default() }).unwrap();
        runtime.step(&command, &ClientLook::default(), &flight).unwrap();
        let own = runtime.receiver().own;
        for field in runtime.receiver().publications.iter() {
            if field.row[r::KIND] == f64::from(k::SUMMARY) && field.row[r::SCHEMA] == SCHEMA && field.row[r::ID] == ID_BASE + own.id {
                let throw = Throw::decode(&field.bytes[..field.length]).expect("a valid throw");
                heard.push((own.tick, throw));
                if let Some(launch) = throw.launch {
                    if replay.as_ref().map_or(true, |old| old.launch().tick != launch.tick) {
                        replay = Some(Replay::new(launch));
                    }
                } else {
                    replay = None;
                }
            }
        }
        runtime.receiver_mut().publications.clear();
        let sim = ctx.owned(|w| w.local_infantry_sample()).unwrap().and_then(|s| s.saber).and_then(|s| s.thrown);
        let seen = replay.as_mut().and_then(|r| r.at(own.tick, own.position, own.yaw));
        sim_out += usize::from(sim.is_some());
        replay_out += usize::from(seen.is_some());
        if let (Some((position, _)), Some(seen)) = (sim, seen) {
            let d = (0..3).map(|i| (position[i] - seen.position[i]).powi(2)).sum::<f64>().sqrt();
            worst = worst.max(d);
            matched += 1;
        }
        if replay.as_ref().is_some_and(Replay::is_finished) && ended.is_none() {
            ended = Some(n);
        }
    }
    runtime.close();
    println!("fields heard {} sim ticks out {sim_out} replay ticks out {replay_out} matched {matched} worst {worst:.5} m, replay finished at {ended:?}", heard.len());
    // The idle hero, then one field at the press, one at the launch and one at the catch:
    // nothing in between, however long the blade flies.
    assert_eq!(heard.len(), 4, "{heard:?}");
    assert_eq!(heard[0].1, Throw::IDLE);
    assert!(heard[1].1.press.is_some() && heard[1].1.launch.is_none());
    assert!(heard[2].1.launch.is_some());
    assert_eq!(heard[3].1, Throw::IDLE);
    assert_eq!(heard[2].1.launch.unwrap().tick, heard[1].1.press.unwrap() + 17.);
    assert!(matched > 40, "the replay and the battle fly together for a while");
    assert!(worst < 0.01, "the replay stays on the battle's blade: {worst} m");
    assert!(sim_out.abs_diff(replay_out) <= 1, "both end together: {sim_out} vs {replay_out}");
}

/// The connected player supplies no pilot or infantry input. A fleet pilot must
/// score through a physical hit and the mission must decide the result.
#[test]
#[ignore = "requires prepared Space Assault content"]
fn scripted_flight_can_defeat_an_idle_player() {
    use bfe_sim::{
        abi::context::Context,
        space::SpaceEvent,
        world::admit::{mission::{MissionPreferences, MissionSelection}, Options},
    };
    let root = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
    let options = Options {
        family: bfe_content::GameFamily::Swbf2, map: "spa6".into(), mode: "ass".into(),
        side: "rep".into(), seed: 7, ..Default::default()
    };
    let selection = MissionSelection {
        family: options.family, map: options.map.clone(), era: options.era().into(),
        mode: options.mode.clone(), seed: options.seed,
    };
    let preferences = MissionPreferences {
        direct_spawn: true, assault_target: Some(1), bots_per_team: Some(0),
    };
    let context = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(context.clone(), &root, &options, &selection, preferences).unwrap();
    let player = prepared.admission.handles.player;
    context.owned(|w| w.start_local_space_developer(player, false, false, true)).unwrap();
    let mut runtime = Runtime::new(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();
    let mut slot = PublicationSlot::new_preallocated();
    let mut pilots = std::collections::BTreeSet::new();
    let mut credited_kill = false;
    let mut shots = 0;
    let mut result = None;
    for tick in 1..=36_000 {
        runtime.step_into_slot(&mut slot).unwrap();
        context.owned(|w| {
            let space = w.space().unwrap();
            for f in &space.fighters {
                if let Some(pilot) = f.pilot.filter(|p| !p.is_human && f.team == 2) {
                    pilots.insert((pilot.owner.slot, pilot.owner.gen));
                }
            }
            for event in &space.events {
                let owner = match event {
                    SpaceEvent::BuildingDestroyed { attacker_unit, .. } |
                    SpaceEvent::FighterDestroyed { attacker_unit, .. } => *attacker_unit,
                    SpaceEvent::WeaponFired { fighter: Some(id), .. } if space.fighters[*id].team == 2 => {
                        shots += 1;
                        None
                    }
                    _ => None,
                };
                if owner.is_some_and(|id| pilots.contains(&(id.slot, id.gen))) {
                    println!("credited flight kill, tick {tick}: {event:?}");
                    credited_kill = true;
                }
            }
            let view = w.mission_view().unwrap();
            let score = view.score();
            if tick % 3600 == 0 || score.finished || score.over {
                println!("tick={tick} score={score:?} launches={} weapon_events={shots}", pilots.len());
            }
            assert_eq!(view.host.diagnostics.callback_errors, 0);
            assert_eq!(view.host.diagnostics.callbacks_dropped, 0);
            if score.over { result = Some(score); }
        }).unwrap();
        if result.is_some() { break; }
    }
    runtime.close();
    let result = result.expect("idle flight match did not finish");
    assert!(credited_kill && result.teams[1] >= result.target.unwrap() && result.teams[1] > result.teams[0]);
}

/// Keep the ordinary mission population and the developer flight crew in the same world.
#[test]
#[ignore = "requires prepared Space Assault content"]
fn populated_flight_match_runs_for_ten_minutes() {
    populated_flight(true);
}

#[test]
#[ignore = "requires prepared Space Assault content"]
fn populated_mission_pilots_fly_for_ten_minutes() {
    populated_flight(false);
}

fn populated_flight(scripted: bool) {
    use bfe_sim::{
        abi::context::Context,
        space::flight::SpaceFlightState,
        world::admit::{mission::{MissionPreferences, MissionSelection}, Options},
    };
    use std::collections::{BTreeMap, BTreeSet};
    let root = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
    // BFE_SPACE_SEED picks another battle of the same mission (7 by default).
    let seed = std::env::var("BFE_SPACE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(7);
    let options = Options {
        family: bfe_content::GameFamily::Swbf2, map: "spa6".into(), mode: "ass".into(),
        side: "rep".into(), seed, ..Default::default()
    };
    let selection = MissionSelection {
        family: options.family, map: options.map.clone(), era: options.era().into(),
        mode: options.mode.clone(), seed: options.seed,
    };
    let context = Context::new();
    let preferences = MissionPreferences { bots_per_team: Some(32), direct_spawn: true, ..Default::default() };
    let (prepared, _) = Prepared::developer_mission_in(context.clone(), &root, &options, &selection, preferences).unwrap();
    if scripted {
        let player = prepared.admission.handles.player;
        context.owned(|w| w.start_local_space_developer(player, false, false, true)).unwrap();
    }
    let mut slot = PublicationSlot::new_preallocated();
    let mut runtime = Runtime::new_into_slot(*prepared.engine, *prepared.admission,
        CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot).unwrap();
    let mut rosters = [0; 2];
    let mut origins = BTreeMap::new();
    let mut walked = [BTreeSet::new(), BTreeSet::new()];
    let mut farthest = [0.0_f64; 2];
    let mut normal_seat_ticks = 0;
    let mut normal_flight_ticks = 0;
    let mut scripted_flight_ticks = 0;
    let mut truncations = 0;
    let mut tally = FlightTally::default();
    for tick in 1..=36_000 {
        let started = std::time::Instant::now();
        runtime.step_into_slot(&mut slot).unwrap();
        tally.time(tick, started.elapsed());
        context.owned(|world| {
            let space = world.space().unwrap();
            tally.observe(space);
            truncations += usize::from(space.soldier_targets_truncated);
            for fighter in &space.fighters {
                for pilot in std::iter::once(fighter.pilot).chain(fighter.gunners).flatten() {
                    if pilot.is_human { continue; }
                    let owner = world.space_launch_snapshot(pilot.owner).unwrap();
                    let airborne = fighter.alive && fighter.state != SpaceFlightState::Landed;
                    if owner.character.is_some() {
                        normal_seat_ticks += 1;
                        normal_flight_ticks += usize::from(airborne);
                    } else {
                        scripted_flight_ticks += usize::from(airborne);
                    }
                }
            }
            if tick == 1 || tick % 30 == 0 {
                let mut present = [0; 2];
                for (team, body) in world.space_bot_roster() {
                    let Some(side) = team.checked_sub(1).filter(|&s| s < 2) else { continue };
                    present[side] += 1;
                    let Some(key) = body else { continue };
                    let r = key.reference();
                    let life = (r.slot, r.gen, key.life.to_bits());
                    let Some(position) = world.space_unit_position(r) else { continue };
                    let origin = origins.entry(life).or_insert(position);
                    let metres = (position[0] - origin[0]).hypot(position[2] - origin[2]);
                    farthest[side] = farthest[side].max(metres);
                    if metres >= 2. { walked[side].insert(life); }
                }
                for (count, now) in rosters.iter_mut().zip(present) { *count = (*count).max(now); }
            }
            if tick % 3600 == 0 {
                let mission = world.mission_view().unwrap();
                tally.minute(tick, mission.score().teams);
                println!("tick={tick} points={:?} roster={rosters:?} walked={:?} farthest_m={farthest:?} ordinary_seat_ticks={normal_seat_ticks} ordinary_airborne_ticks={normal_flight_ticks} scripted_airborne_ticks={scripted_flight_ticks} truncated={truncations} callback_errors={} dropped={}",
                    mission.score().teams, walked.each_ref().map(|s| s.len()),
                    mission.host.diagnostics.callback_errors, mission.host.diagnostics.callbacks_dropped);
            }
        }).unwrap();
    }
    runtime.close();
    tally.summary();
    assert_eq!(rosters, [31, 32]);
    assert!(walked.iter().all(|lives| !lives.is_empty()));
    if scripted {
        assert!(scripted_flight_ticks > 0);
        assert_eq!(normal_flight_ticks, 0);
    } else {
        assert!(normal_flight_ticks > 0);
        assert_eq!(scripted_flight_ticks, 0);
    }
    assert_eq!(truncations, 0);
}

#[test]
#[ignore = "requires prepared Space Assault content"]
fn authored_start_survives_scripted_admission_and_replay() {
    use bfe_sim::{
        abi::context::Context,
        world::admit::{mission::{MissionPreferences, MissionSelection}, Options},
    };
    let root = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
    let options = Options {
        family: bfe_content::GameFamily::Swbf2, map: "spa6".into(), mode: "ass".into(),
        side: "rep".into(), seed: 7, ..Default::default()
    };
    let map: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(root.join("maps").join(&options.map).join("map.json")).unwrap(),
    ).unwrap();
    let selection = MissionSelection { family: options.family, map: options.map.clone(),
        era: options.era().into(), mode: options.mode.clone(), seed: options.seed };
    let context = Context::new();
    // A base starting node exists regardless of the active mission layer.
    let (name, start) = map["spawns"].as_object().unwrap().iter()
        .find_map(|(name, nodes)| Some((name, nodes[0].as_array()?)).filter(|(_, p)| p.len() >= 4))
        .unwrap();
    context.owned(|w| w.select_local_spawn_path(name.clone())).unwrap();
    let preferences = MissionPreferences { direct_spawn: true, bots_per_team: Some(0), ..Default::default() };
    let (prepared, journal) = Prepared::developer_mission_in(context.clone(), &root, &options, &selection, preferences).unwrap();
    let position = context.owned(|w| {
        let unit = w.space_player_unit(prepared.admission.handles.player).unwrap();
        w.space_unit_position(unit.reference()).unwrap()
    }).unwrap();
    for i in [0, 2] {
        assert!((position[i] - start[i].as_f64().unwrap()).abs() < 0.25);
    }
    journal.replay(&bfe_content::files::DirFiles::from_resolved_root(&root, bfe_family::GameFamily::Swbf2)).unwrap();
}

/// What a ten-minute flight battle did, by side: craft aloft, craft losses by
/// what destroyed them, ship systems lost and hull contacts.
#[derive(Default)]
struct FlightTally {
    aloft: [usize; 2],
    peak_aloft: [usize; 2],
    losses: std::collections::BTreeMap<(usize, &'static str), u32>,
    systems: [Vec<(u32, String)>; 2],
    contacts: [u32; 2],
    fatal_contacts: [u32; 2],
    takeoffs: [u32; 2],
    landings: [u32; 2],
    slowest: (f64, u32),
    tick: u32,
}

impl FlightTally {
    fn time(&mut self, tick: u32, elapsed: std::time::Duration) {
        self.tick = tick;
        let ms = elapsed.as_secs_f64() * 1e3;
        if ms > self.slowest.0 { self.slowest = (ms, tick); }
    }

    fn observe(&mut self, space: &bfe_sim::space::SpaceBattlefield) {
        use bfe_sim::space::{flight::SpaceFlightState, SpaceBodyOwner, SpaceEvent, SpaceImpactCause, SpaceImpactTarget};
        let side = |team: i32| usize::try_from(team - 1).ok().filter(|&s| s < 2);
        self.aloft = [0; 2];
        for f in space.fighters.iter().filter(|f| f.alive && f.state != SpaceFlightState::Landed && f.pilot.is_some()) {
            if let Some(s) = side(f.team) { self.aloft[s] += 1; }
        }
        for s in 0..2 { self.peak_aloft[s] = self.peak_aloft[s].max(self.aloft[s]); }
        for i in &space.impacts {
            let (SpaceImpactCause::Contact { .. }, SpaceImpactTarget::Fighter { index, .. }) = (i.cause, i.target) else { continue };
            let Some(s) = space.fighters.get(index).and_then(|f| side(f.team)) else { continue };
            self.contacts[s] += 1;
            self.fatal_contacts[s] += u32::from(i.destroyed);
        }
        for e in space.events() {
            match *e {
                SpaceEvent::FighterTakeoff { fighter, .. } | SpaceEvent::FighterLanded { fighter, .. } => {
                    let Some(s) = space.fighters.get(fighter).and_then(|f| side(f.team)) else { continue };
                    if matches!(e, SpaceEvent::FighterTakeoff { .. }) { self.takeoffs[s] += 1 } else { self.landings[s] += 1 }
                }
                SpaceEvent::FighterDestroyed { fighter, team, .. } => {
                    let Some(s) = side(team) else { continue };
                    let impact = space.impacts.iter().find(|i| i.destroyed
                        && matches!(i.target, SpaceImpactTarget::Fighter { index, .. } if index == fighter));
                    let cause = match impact.map(|i| (i.cause, i.emitter, i.team)) {
                        Some((SpaceImpactCause::Contact { body }, ..)) => match space.body_owner(body) {
                            SpaceBodyOwner::Fighter(_) => "collision with a craft",
                            _ => "collision with a ship",
                        },
                        Some((SpaceImpactCause::Flight, ..)) => "flight loss",
                        Some((SpaceImpactCause::Explosion, ..)) => "object explosion",
                        Some((_, _, attacker)) if attacker == team => "own fire",
                        Some((_, Some(bfe_sim::space::combat::EmittingBody::Fighter { .. }), _)) => "enemy craft",
                        Some((_, Some(bfe_sim::space::combat::EmittingBody::Building { index, .. }), _)) => {
                            match space.buildings.iter().find(|a| a.building_index == index).and_then(|a| a.turret.as_ref()) {
                                Some(t) if t.pilot_type == bfe_sim::space::objects::SpaceTurretPilotType::SelfPiloted => "enemy ship gun",
                                Some(_) => "enemy turret seat",
                                None => "enemy ship object",
                            }
                        }
                        _ => "unattributed",
                    };
                    *self.losses.entry((s, cause)).or_default() += 1;
                }
                SpaceEvent::BuildingDestroyed { building, team, .. } => {
                    let Some(s) = side(team) else { continue };
                    if let Some(a) = space.buildings.iter().find(|a| a.building_index == building) {
                        if a.lock_on_enabled || a.ai_damage_threshold > 0.0 {
                            self.systems[s].push((self.tick, a.name.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn losses(&self, side: usize) -> String {
        let mut text = String::new();
        for ((s, cause), n) in &self.losses {
            if *s == side { text.push_str(&format!("{cause}:{n};")); }
        }
        text
    }

    fn minute(&mut self, tick: u32, points: [u32; 2]) {
        println!("MINUTE tick={tick} points={points:?} aloft_now={:?} aloft_peak={:?} systems_lost=[{},{}] contacts={:?} fatal_contacts={:?} republic_losses={} cis_losses={}",
            self.aloft, self.peak_aloft, self.systems[0].len(), self.systems[1].len(), self.contacts, self.fatal_contacts,
            self.losses(0), self.losses(1));
        self.peak_aloft = [0; 2];
    }

    fn summary(&self) {
        for (side, name) in ["republic", "cis"].into_iter().enumerate() {
            println!("SYSTEMS {name} lost={} {:?}", self.systems[side].len(), self.systems[side]);
        }
        println!("FLIGHTS takeoffs={:?} landings={:?}", self.takeoffs, self.landings);
        println!("SLOWEST runtime step {:.3} ms at tick {}", self.slowest.0, self.slowest.1);
    }
}

#[test]
#[ignore = "requires the prepared mission_setup_fixture.py test copy"]
fn an_unusable_local_mission_returns_a_player_message_on_load_or_first_step() {
    use bfe_sim::world::admit::{mission::MissionPreferences, Bots, Options};
    let root = std::path::PathBuf::from(std::env::var_os("BFE_SETUP_TEST_CONTENT").unwrap());
    let options = Options { family: bfe_content::GameFamily::Swbf2, mode: "eli".into(), map: "tat2".into(), side: "all".into(), seed: 59,
        bots: Bots::read("classic", "medium", "full"), ..Default::default() };
    let selection = bfe_sim::world::admit::mission::MissionSelection { family: bfe_content::GameFamily::Swbf2,
        map: options.map.clone(), era: "gcw".into(), mode: "eli".into(), seed: options.seed };
    let common: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("common/common.json")).unwrap()).unwrap();
    let label = |key| common["strings"][bfe_content::zero_hash::fnv1a_hex(key)].as_str().unwrap();
    let expected = format!("{} ({}) could not start. The battle ended in a draw.",
        label("mapname.name.tat2"), label("modename.name.elimination"));
    for count in [1, 2] {
        let admission = Prepared::developer_mission(&root, &options, &selection, MissionPreferences {
            bots_per_team: Some(count), ..Default::default() });
        let message = if count == 1 {
            admission.err().unwrap()
        } else {
            let (prepared, _) = admission.unwrap();
            let mut runtime = Runtime::new(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();
            runtime.step_into_slot(&mut PublicationSlot::new_preallocated()).unwrap_err()
        };
        assert!(message.contains(&expected), "{message}");
        assert!(!message.contains("tat2g_eli"));
    }
}

/// Every power event a viewer draws (a cast, a hit, a throw, a block) that the battle logs
/// reaches the decoded publications the runtime's client receives: the one way an Instant
/// Action client and a server's clients get them. Burns travel the same way with their effect
/// and time left; the battle does not log them, so they are checked on their own.
#[test]
#[ignore = "requires prepared Hero Assault content"]
fn power_events_travel_with_the_publications() {
    use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
    use bfe_presentation::network_layout::{net_kind as k, net_record as r};
    use bfe_sim::{abi::context::Context, combat::powers::{powers_in, PowerEventKind as K}, world::admit::{mission::*, Bots, Options}};
    let (Some(root), Ok(unit)) = (std::env::var_os("BFE_SWBF2_PREPARED"), std::env::var("BFE_MATCH_UNIT")) else { return };
    let ticks: u32 = std::env::var("BFE_ROSTER_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(3600);
    let selection = MissionSelection { family: bfe_family::GameFamily::Swbf2, map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed: 7 };
    let options = Options { family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(), side: "all".into(),
        unit: Some(unit), seed: selection.seed, bots: Bots::read("classic", "medium", "full"), ..Default::default() };
    let ctx = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(ctx.clone(), std::path::Path::new(&root), &options, &selection, MissionPreferences::default()).unwrap();
    let mut runtime = Runtime::new(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();
    let action = |kind: K| match kind { K::Cast => "cast", K::Hit => "hit", K::Thrown => "thrown", K::Blocked => "blocked", _ => "" };
    type Row = (String, u32, (u32, u32), Option<(u32, u32)>);
    let (mut logged, mut published): (Vec<Row>, Vec<Row>) = (Vec::new(), Vec::new());
    let (mut cursor, mut bytes, mut events, mut burns) = (0, 0usize, 0usize, 0usize);
    for _ in 0..=ticks {
        let tick = runtime.tick();
        for field in runtime.receiver().publications.iter() {
            if field.row[r::KIND] != f64::from(k::COMBAT) || field.row[r::SCHEMA] != 8. {
                continue;
            }
            let event: serde_json::Value = serde_json::from_slice(&field.bytes[..field.length]).unwrap();
            events += 1;
            if event["kind"] != "power" {
                continue;
            }
            bytes += field.length;
            let id = |key: &str| event[key].as_f64().map(|v| v as u32);
            if event["action"] == "burn" {
                assert!(event["name"].is_string() && event["duration"].as_f64().is_some_and(|d| d > 0.), "a burn names its effect and time");
                assert!(id("targetId").is_some(), "a burn names whom it burns");
                burns += 1;
                continue;
            }
            assert!(event["position"].is_array(), "a published power event names where its caster stood");
            published.push((event["action"].as_str().unwrap().to_string(), id("weaponId").unwrap(),
                (id("ownerId").unwrap(), id("ownerGeneration").unwrap()), id("targetId").zip(id("targetGeneration"))));
        }
        runtime.receiver_mut().publications.clear();
        ctx.owned(|w| {
            let Some(powers) = powers_in(w) else { return };
            for ev in powers.log().since(cursor) {
                if !action(ev.kind).is_empty() {
                    logged.push((action(ev.kind).to_string(), ev.weapon, (ev.caster.slot, ev.caster.life as u32),
                        ev.target.map(|t| (t.slot, t.life as u32))));
                }
            }
            cursor = powers.log().latest;
        }).unwrap();
        let command = ClientCommand { tick: tick + 1., sequence: tick + 1., generation: runtime.receiver().own.generation,
            has_mouse_scale: true, mouse_scale: 1., has_aspect: true, aspect: 16. / 9., look_count: 1, ..Default::default() };
        let flight = ClientFlightCommand { scale: 1., curve: 1., ..Default::default() };
        runtime.step(&command, &ClientLook::default(), &flight).unwrap();
    }
    runtime.close();
    let count = |list: &Vec<Row>, a: &str| list.iter().filter(|e| e.0 == a).count();
    println!("POWER EVENTS ticks={ticks} combat_events={events} logged={} published={} cast={} hit={} thrown={} blocked={} burns={burns} payload_bytes={bytes}",
        logged.len(), published.len(), count(&published, "cast"), count(&published, "hit"), count(&published, "thrown"),
        count(&published, "blocked"));
    assert!(!logged.is_empty(), "the bots used their powers");
    logged.sort();
    published.sort();
    assert_eq!(logged, published, "every drawn power event, and nothing else, travels");
}

/// Instant Action Space Assault drives the same shared pilot and terminal seat
/// controller as online: deploy on the hangar deck, board a parked fighter,
/// take off, fire, return to the hangar and land, disembark, and operate an
/// interior chair turret.
#[test]
#[ignore = "requires prepared Space Assault content"]
fn instant_action_space_sortie_boards_flies_fires_lands_and_uses_chair_gun() {
    use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
    use bfe_sim::{
        abi::context::Context,
        space::{
            flight::{SpaceFlightState, SpacePilotInput},
            local::PilotControls,
            SpaceEvent,
        },
        units::infantry::Controls,
        world::admit::{
            mission::{MissionPreferences, MissionSelection},
            Options,
        },
    };
    let root = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
    let options = Options {
        family: bfe_content::GameFamily::Swbf2,
        map: "spa6".into(),
        mode: "ass".into(),
        side: "rep".into(),
        seed: 7,
        ..Default::default()
    };
    let selection = MissionSelection {
        family: options.family,
        map: options.map.clone(),
        era: options.era().into(),
        mode: options.mode.clone(),
        seed: options.seed,
    };
    let preferences = MissionPreferences {
        direct_spawn: true,
        assault_target: None,
        bots_per_team: Some(0),
    };
    let context = Context::new();
    let (prepared, _) =
        Prepared::developer_mission_in(context.clone(), &root, &options, &selection, preferences)
            .unwrap();
    let player_handle = prepared.admission.handles.player;
    let mut slot = PublicationSlot::new_preallocated();
    let player_yaw = |context: &Context, slot_id: u32| -> f64 {
        use bfe_sim::gen::{BlockHeader as B, Unit, UnitBody};
        let units = context.export(u32::from(bfe_sim::gen::KIND_UNIT), 1).unwrap();
        let stride = units[B::STRIDE] as usize;
        let rows = units[B::ROWS] as usize;
        let heap_at = units[B::HEAP] as usize;
        let heap = &units[heap_at..heap_at + units[B::HEAP_WORDS] as usize];
        for i in 0..rows {
            let row = &units[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
            if row[0] as u32 == slot_id {
                let mut u = Unit::default();
                u.take_full(row, heap);
                if let UnitBody::Soldier(s) = u.soldier {
                    return s.aim_yaw;
                }
            }
        }
        0.0
    };
    let player_slot_id = prepared.admission.player_id as u32;
    let mut runtime = Runtime::new_into_slot(
        *prepared.engine,
        *prepared.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
        &mut slot,
    )
    .unwrap();
    let mut seq = 1.0f64;
    let step_with = |runtime: &mut Runtime<_>,
                     slot: &mut PublicationSlot,
                     seq: &mut f64,
                     held: u32,
                     look_x: f64,
                     pilot: PilotControls| {
        let gen = slot.infantry.map_or(1.0, |s| s.generation);
        let cmd = ClientCommand {
            tick: runtime.tick() + 1.0,
            sequence: *seq,
            generation: gen,
            held,
            has_mouse_scale: true,
            mouse_scale: 1.0,
            has_aspect: true,
            aspect: 16.0 / 9.0,
            look_count: 1,
            ..Default::default()
        };
        let flight = ClientFlightCommand {
            scale: 1.0,
            curve: 1.0,
            ..Default::default()
        };
        runtime
            .submit(
                &cmd,
                &ClientLook { x: look_x, y: 0.0 },
                &flight,
                runtime.tick(),
                None,
                false,
            )
            .unwrap();
        runtime.infantry_input(*seq, Controls::default()).unwrap();
        runtime.pilot_input(*seq, pilot).unwrap();
        *seq += 1.0;
        runtime.step_into_slot(slot).unwrap();
    };
    let forward_bit = 1u32 << bfe_client::generated_input::ClientHeld::Forward as u8;
    let sprint_bit = 1u32 << bfe_client::generated_input::ClientHeld::Sprint as u8;

    // Wait for deployment on the hangar deck.
    for _ in 0..180 {
        step_with(&mut runtime, &mut slot, &mut seq, 0, 0.0, PilotControls::default());
        let spawned = context
            .owned(|w| w.space_player_unit(player_handle).and_then(|k| w.space_unit_position(k.reference())).is_some())
            .unwrap();
        if spawned {
            break;
        }
    }
    let _deployed = context
        .owned(|w| w.space_player_unit(player_handle).and_then(|k| w.space_unit_position(k.reference())))
        .unwrap()
        .expect("local Space pilot must be connected and spawned");

    // Walk on foot to the nearest friendly parked craft and board via the shared driver.
    let mut pad_pos = [0.0; 3];
    for step in 0..900u32 {
        let pos = context
            .owned(|w| w.space_player_unit(player_handle).and_then(|k| w.space_unit_position(k.reference())))
            .unwrap()
            .unwrap();
        let yaw = player_yaw(&context, player_slot_id);
        let target = context
            .owned(|w| {
                let s = w.space().unwrap();
                let id = s.nearest_parked_fighter(pos, 1, f64::INFINITY)?;
                Some(s.fighters[id].position)
            })
            .unwrap()
            .expect("parked friendly fighter");
        pad_pos = target;
        let (dx, dz) = (target[0] - pos[0], target[2] - pos[2]);
        let dist = (dx * dx + dz * dz).sqrt();
        let mut diff = dx.atan2(dz) - yaw;
        while diff > std::f64::consts::PI {
            diff -= std::f64::consts::TAU;
        }
        while diff < -std::f64::consts::PI {
            diff += std::f64::consts::TAU;
        }
        let look_x = (-diff * 120.0).clamp(-80.0, 80.0);
        let held = if dist > 2.5 { forward_bit | sprint_bit } else { 0 };
        step_with(
            &mut runtime,
            &mut slot,
            &mut seq,
            held,
            look_x,
            PilotControls {
                use_pressed: dist < 9.0 && step % 6 == 0,
                ..Default::default()
            },
        );
        if context
            .owned(|w| w.space().and_then(|s| s.pilot_fighter(player_handle)).is_some())
            .unwrap()
        {
            break;
        }
    }
    let boarded = context
        .owned(|w| w.space().and_then(|s| s.pilot_fighter(player_handle).map(|f| (f.id, f.state))))
        .unwrap()
        .expect("pressing use beside a parked fighter must board it in Instant Action");
    assert_eq!(boarded.1, SpaceFlightState::Landed);

    // Take off and fly out while firing primary weapons.
    step_with(
        &mut runtime,
        &mut slot,
        &mut seq,
        0,
        0.0,
        PilotControls {
            flight: SpacePilotInput { takeoff_or_land: true, throttle: 1.0, ..Default::default() },
            ..Default::default()
        },
    );
    let mut flew = false;
    let mut fired = false;
    for _ in 0..180 {
        step_with(
            &mut runtime,
            &mut slot,
            &mut seq,
            0,
            0.0,
            PilotControls {
                flight: SpacePilotInput { throttle: 1.0, fire_primary: true, ..Default::default() },
                ..Default::default()
            },
        );
        context
            .owned(|w| {
                let s = w.space().unwrap();
                if let Some(f) = s.pilot_fighter(player_handle) {
                    if f.state == SpaceFlightState::Flying {
                        flew = true;
                    }
                }
                if s.events.iter().any(|e| matches!(e, SpaceEvent::WeaponFired { .. })) {
                    fired = true;
                }
            })
            .unwrap();
        if flew && fired {
            break;
        }
    }
    assert!(flew, "local fighter must reach Flying state");
    assert!(fired, "local fighter firing must emit WeaponFired");

    // Return into the hangar landing zone, request landing, wait until Landed, and disembark.
    context
        .owned(|w| {
            let s = w.space_mut().unwrap();
            let id = s.pilot_seat(player_handle).unwrap().fighter;
            let f = &mut s.fighters[id];
            f.position = [pad_pos[0], pad_pos[1] + 5.0, pad_pos[2]];
            f.previous_position = f.position;
            f.airspeed = 10.0;
            f.velocity = [0.0; 3];
        })
        .unwrap();
    step_with(
        &mut runtime,
        &mut slot,
        &mut seq,
        0,
        0.0,
        PilotControls {
            flight: SpacePilotInput { takeoff_or_land: true, ..Default::default() },
            ..Default::default()
        },
    );
    let mut landed = false;
    for _ in 0..240 {
        step_with(&mut runtime, &mut slot, &mut seq, 0, 0.0, PilotControls::default());
        if context
            .owned(|w| w.space().and_then(|s| s.pilot_fighter(player_handle)).map(|f| f.state))
            .unwrap()
            == Some(SpaceFlightState::Landed)
        {
            landed = true;
            break;
        }
    }
    assert!(landed, "local fighter must land cleanly");
    step_with(
        &mut runtime,
        &mut slot,
        &mut seq,
        0,
        0.0,
        PilotControls { use_pressed: true, ..Default::default() },
    );
    assert!(
        context.owned(|w| w.space().and_then(|s| s.pilot_fighter(player_handle)).is_none()).unwrap(),
        "pressing use after landing must disembark back on foot"
    );

    // Walk up the hangar stairs through the proximity door to a linked chair terminal and board it.
    let waypoints: &[[f64; 2]] = &[
        [-434.0, -1238.0],
        [-432.0, -1250.5],
        [-437.9, -1257.7],
        [-435.5, -1268.0],
        [-438.5, -1273.5],
    ];
    let mut wp_idx = 0usize;
    for step in 0..900u32 {
        let pos = context
            .owned(|w| w.space_player_unit(player_handle).and_then(|k| w.space_unit_position(k.reference())))
            .unwrap()
            .unwrap();
        let yaw = player_yaw(&context, player_slot_id);
        let target_xz = if wp_idx < waypoints.len() {
            let wp = waypoints[wp_idx];
            let d = ((wp[0] - pos[0]).powi(2) + (wp[1] - pos[2]).powi(2)).sqrt();
            if d < 2.2 {
                wp_idx += 1;
            }
            wp
        } else {
            context
                .owned(|w| {
                    let s = w.space().unwrap();
                    s.buildings
                        .iter()
                        .filter(|b| {
                            b.terminal
                                .as_ref()
                                .is_some_and(|t| t.occupant.is_none() && t.remote_turret.is_some())
                                && b.center[2] < -1150.0
                        })
                        .min_by(|a, b| {
                            let da = (a.center[0] - pos[0]).powi(2) + (a.center[2] - pos[2]).powi(2);
                            let db = (b.center[0] - pos[0]).powi(2) + (b.center[2] - pos[2]).powi(2);
                            da.total_cmp(&db)
                        })
                        .map(|b| [b.center[0], b.center[2]])
                })
                .unwrap()
                .unwrap()
        };
        let (dx, dz) = (target_xz[0] - pos[0], target_xz[1] - pos[2]);
        let dist = (dx * dx + dz * dz).sqrt();
        let mut diff = dx.atan2(dz) - yaw;
        while diff > std::f64::consts::PI {
            diff -= std::f64::consts::TAU;
        }
        while diff < -std::f64::consts::PI {
            diff += std::f64::consts::TAU;
        }
        let look_x = (-diff * 120.0).clamp(-80.0, 80.0);
        let try_board = wp_idx >= waypoints.len() && dist < 8.0 && step % 6 == 0;
        let held = if dist > 1.8 { forward_bit | sprint_bit } else { 0 };
        step_with(
            &mut runtime,
            &mut slot,
            &mut seq,
            held,
            look_x,
            PilotControls { use_pressed: try_board, ..Default::default() },
        );
        if context
            .owned(|w| w.space().and_then(|s| s.pilot_terminal(player_handle)).is_some())
            .unwrap()
        {
            break;
        }
    }
    let initial_yaw = context
        .owned(|w| {
            let (_, gun) = w.space()?.pilot_terminal(player_handle)?;
            Some(gun.turret.as_ref()?.yaw)
        })
        .unwrap()
        .expect("pressing use beside a terminal chair must enter it in Instant Action");
    let mut turret_fired = false;
    for _ in 0..30 {
        step_with(
            &mut runtime,
            &mut slot,
            &mut seq,
            0,
            0.0,
            PilotControls {
                turret_look: [0.05, 0.02],
                flight: SpacePilotInput { fire_primary: true, ..Default::default() },
                ..Default::default()
            },
        );
        if context
            .owned(|w| w.space().unwrap().events.iter().any(|e| matches!(e, SpaceEvent::WeaponFired { .. })))
            .unwrap()
        {
            turret_fired = true;
        }
    }
    let after_yaw = context
        .owned(|w| w.space()?.pilot_terminal(player_handle).and_then(|(_, g)| Some(g.turret.as_ref()?.yaw)))
        .unwrap()
        .expect("still seated in terminal chair");
    assert!((after_yaw - initial_yaw).abs() > 1e-3, "chair gun must slew on turret_look");
    assert!(turret_fired, "chair gun must fire");
    step_with(
        &mut runtime,
        &mut slot,
        &mut seq,
        0,
        0.0,
        PilotControls { use_pressed: true, ..Default::default() },
    );
    assert!(context.owned(|w| w.space().unwrap().pilot_terminal(player_handle).is_none()).unwrap());
    let _ = player_handle;
    runtime.close();
}
