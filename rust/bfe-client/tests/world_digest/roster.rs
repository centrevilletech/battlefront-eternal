use bfe_wide::Wide;
use super::*;
use bfe_sim::{abi::context::Context, units::infantry::contact::Cause, world::admit::{mission::*, Bots, Options as WorldOptions}};
use std::collections::BTreeSet;

/// Exercise the mission, every equipped soldier, and the native death-to-rules handover together.
#[test]
#[ignore = "requires prepared Hero Assault content"]
fn roster_fights_repeat() {
    let (Some(root), Ok(unit)) = (std::env::var_os("BFE_SWBF2_PREPARED"), std::env::var("BFE_MATCH_UNIT")) else { return };
    let root = std::path::PathBuf::from(root);
    let seed = 7;
    let ticks = std::env::var("BFE_ROSTER_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(18_000);
    let selection = MissionSelection { family: bfe_family::GameFamily::Swbf2, map: "tat2".into(),
        era: "gcw".into(), mode: "eli".into(), seed };
    let options = WorldOptions { family: selection.family, mode: selection.mode.clone(),
        map: selection.map.clone(), side: "all".into(), seed, unit: Some(unit),
        bots: Bots::read("classic", "medium", "full"), ..Default::default() };
    let mut first = None;
    for run in 1..=2 {
        let context = Context::new();
        let (prepared, _) = Prepared::developer_mission_in(context.clone(), &root, &options, &selection,
            MissionPreferences::default()).unwrap();
        let mut slot = PublicationSlot::new_preallocated();
        let mut rt = Runtime::new_into_slot(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot).unwrap();
        let mut digest = 0xcbf29ce484222325_u64;
        let (mut bodies, mut attacks, mut deaths, mut saber_deaths) = (0, 0, 0, 0);
        let mut swung = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut points = [0u32; 2];
        let mut peak = std::time::Duration::ZERO;
        for n in 1..=ticks {
            let at = Instant::now();
            rt.step_into_slot(&mut slot).unwrap();
            if n > 900 { peak = peak.max(at.elapsed()); }
            let score = slot.mission_score.unwrap();
            assert!(!score.faulted, "ordinary callback fault on tick {n}");
            context.owned(|w| {
                let (mut lives, mut active) = (0, 0);
                w.visit_melee(|id, weapon, state, energy, attacking| {
                    lives += 1;
                    if attacking { active += 1; swung.insert(id.slot); }
                    for value in [Wide::from_u32(id.slot.into()), Wide::from_u32(id.slab.into()), Wide::from_u32(id.generation.into()),
                        Wide::from_u32(weapon.into()), Wide::from_u32(state.state.0.into()),
                        Wide::from_f64_bits(state.eval_time), Wide::from_f64_bits(energy)] {
                        digest = mix(digest, &value.to_le_bytes());
                    }
                });
                bodies = bodies.max(lives);
                attacks = attacks.max(active);
                for record in &w.mission_view().unwrap().host.deaths {
                    let v = record.victim;
                    assert!(seen.insert((v.slot, v.slab, v.generation)), "duplicate native death {record:?}");
                    deaths += 1;
                    if matches!(record.cause, Some(Cause::Saber { .. })) {
                        assert!(record.killer.is_some());
                        saber_deaths += 1;
                    }
                    if let (Some(team @ 1..=2), enemy @ 1..=2) = (record.killer_team, record.victim_team) {
                        if team != enemy { points[team as usize-1] += 1; }
                    }
                    digest = mix(digest, &record.tick.to_le_bytes());
                    digest = mix(digest, &v.slot.to_le_bytes());
                    digest = mix(digest, &record.killer.map(|r| r.slot).unwrap_or(u32::MAX).to_le_bytes());
                }
            }).unwrap();
            assert_eq!(score.teams, points, "native enemy deaths and mission score differ on tick {n}");
            if n % 600 == 0 { println!("roster run={run} tick={n} score={points:?} hash={digest:016x} armed={bodies} simultaneous={attacks} attackers={} saber_deaths={saber_deaths}",swung.len()); }
            if score.over { break; }
        }
        // Through a battle's opening minute every team holds a place in each class, so a short run fields one of each
        // saber class of both rosters at a time.
        assert!(bodies >= 12 && !swung.is_empty() && deaths > 0);
        println!("roster result run={run} hash={digest:016x} armed={bodies} simultaneous={attacks} attackers={} deaths={deaths} saber_deaths={saber_deaths} worst_runtime_us={}",swung.len(),peak.as_micros());
        let proof = (digest, bodies, attacks, deaths, saber_deaths, points);
        if let Some(expected) = &first { assert_eq!(expected, &proof); }
        first = Some(proof);
        rt.close();
    }
}
