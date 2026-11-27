//! A hero's guided rocket detonates through the local battle's host, boosted and not.
//!
//! The rocket's flight is the player's own: hold the primary button to boost,
//! press the secondary button to detonate. A rocket above its boost speed
//! threshold takes its critical explosion, a slower one its ordinary
//! explosion; the host must record both. The test is `#[ignore]` and reads
//! prepared 2005 content from `BFE_SWBF2_CONTENT_DIR`.
use bfe_client::generated_input::{ClientAction, ClientCommand, ClientFlightCommand, ClientHeld, ClientLook};
use bfe_client::local::launch::Prepared as BattlePrepared;
use bfe_client::local::runtime::Runtime as BattleRuntime;
use bfe_client::local::thread::PublicationSlot;
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_sim::abi::context::Context;
use bfe_sim::host::journal::Event;
use bfe_sim::world::admit::mission::{MissionPreferences, MissionSelection};
use std::path::Path;

/// One scripted flight: how many ticks the rocket boosts, and for how many of
/// them it climbs.
#[derive(Clone, Copy)]
struct Plan {
    boost_ticks: u32,
    climb_ticks: u32,
}

/// What a flight did.
#[derive(Default)]
struct Flight {
    /// The tick error the battle reported, with its tick.
    error: Option<(u32, String)>,
    /// The radius of each blast the rocket's detonation made.
    blasts: Vec<f64>,
    /// The admitted explosions' damage and outer radius, in index order.
    explosions: Vec<(f64, f64)>,
}

const LAUNCH: u32 = 150;

/// A hero of the rebel side whose gun is a guided rocket, found in the prepared
/// content: the hero is whichever unit carries a remote weapon with a guided
/// ordnance.
fn guided_rocket_hero(root: &str) -> String {
    let text = std::fs::read(Path::new(root).join("sides/all/units.json")).expect("read units");
    let units: serde_json::Value = serde_json::from_slice(&text).expect("parse units");
    units["units"]
        .as_object()
        .expect("units table")
        .iter()
        .find(|(name, unit)| {
            name.contains("hero")
                && unit["weapons"].as_array().is_some_and(|weapons| {
                    weapons.iter().any(|weapon| {
                        weapon["base"] == "remote" && weapon["ordnance"].get("GuidedMissile").is_some()
                    })
                })
        })
        .map(|(name, _)| name.clone())
        .expect("a hero with a guided rocket")
}

fn fly(root: &str, plan: Plan) -> Flight {
    let selection = MissionSelection {
        family: bfe_content::GameFamily::Swbf2,
        map: "tat2".into(),
        era: "gcw".into(),
        mode: "eli".into(),
        seed: 7,
    };
    let mut options = bfe_sim::world::admit::Options::default();
    options.family = bfe_family::GameFamily::Swbf2;
    options.map = "tat2".into();
    options.side = "all".into();
    options.mode = "eli".into();
    options.seed = 7;
    options.unit = Some(guided_rocket_hero(root));
    let ctx = Context::new();
    let (prepared, _journal) = BattlePrepared::developer_mission_in(
        ctx.clone(),
        Path::new(root),
        &options,
        &selection,
        MissionPreferences { direct_spawn: true, ..Default::default() },
    )
    .expect("admit");
    let mut flight = Flight {
        explosions: ctx
            .world_explosion_defs()
            .unwrap()
            .into_iter()
            .map(|(damage, _, outer)| (damage, outer))
            .collect(),
        ..Flight::default()
    };
    let mut slot = PublicationSlot::new_preallocated();
    let mut rt = BattleRuntime::new_into_slot(
        *prepared.engine,
        *prepared.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
        &mut slot,
    )
    .expect("runtime");

    let climb_until = LAUNCH + 6 + plan.climb_ticks;
    let detonate = LAUNCH + 6 + plan.boost_ticks;
    let end = detonate + 60;
    let mut seen = 0;
    let mut stop = 0;
    for tick in 1..=end {
        let mut held = 0u32;
        let mut actions = 0u32;
        if (LAUNCH..LAUNCH + 4).contains(&tick) {
            held |= 1 << ClientHeld::Secondary as u32;
        }
        if tick == LAUNCH {
            actions |= 1 << ClientAction::FireSecondary as u32;
        }
        if tick >= LAUNCH + 6 && tick < detonate {
            held |= 1 << ClientHeld::Primary as u32;
        }
        if tick == detonate {
            held |= 1 << ClientHeld::Secondary as u32;
            actions |= 1 << ClientAction::FireSecondary as u32;
        }
        let look = if (LAUNCH + 7..climb_until).contains(&tick) {
            ClientLook { y: -30.0, ..Default::default() }
        } else {
            ClientLook::default()
        };
        let next = rt.tick() + 1.0;
        let command = ClientCommand {
            tick: next,
            sequence: next,
            generation: slot.infantry.map_or(1.0, |s| s.generation),
            held,
            actions,
            look_count: 1,
            has_aspect: true,
            aspect: 16.0 / 9.0,
            has_mouse_scale: true,
            mouse_scale: 1.0,
            ..Default::default()
        };
        let flight_command = ClientFlightCommand { scale: 1.0, curve: 1.0, ..Default::default() };
        rt.submit(&command, &look, &flight_command, rt.tick(), None, false).unwrap();
        rt.infantry_input(next, Default::default()).unwrap();
        if let Err(error) = rt.step_into_slot(&mut slot) {
            flight.error = Some((tick, error));
            break;
        }
        ctx.world_host_journal(|journal| {
            if tick > LAUNCH {
                for record in journal.since(seen) {
                    match &record.event {
                        Event::Explosion { radius, .. } => flight.blasts.push(*radius),
                        _ => {}
                    }
                }
            }
            seen = journal.seq;
        })
        .unwrap();
        if !flight.blasts.is_empty() {
            stop += 1;
            if stop > 3 {
                break;
            }
        }
    }
    let _ = ctx.dispose();
    flight
}

/// The outer radius of the explosion a blast of `radius` made, with its damage.
fn damage_of(flight: &Flight, radius: f64) -> f64 {
    flight
        .explosions
        .iter()
        .filter(|&&(_, outer)| outer == radius)
        .map(|&(damage, _)| damage)
        .fold(f64::NAN, f64::max)
}

#[test]
#[ignore = "requires prepared 2005 content in BFE_SWBF2_CONTENT_DIR"]
fn a_boosted_and_an_unboosted_rocket_both_detonate() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("BFE_SWBF2_CONTENT_DIR");
    let plain = fly(&root, Plan { boost_ticks: 0, climb_ticks: 0 });
    let boosted = fly(&root, Plan { boost_ticks: 90, climb_ticks: 90 });
    for flight in [&plain, &boosted] {
        assert_eq!(flight.error, None, "the battle must go on after the blast");
        assert_eq!(flight.blasts.len(), 1, "one detonation, one blast");
    }
    let (small, large) = (plain.blasts[0], boosted.blasts[0]);
    assert!(large > small, "a boosted rocket's blast ({large}) is larger than a slow one's ({small})");
    // Each blast is an admitted explosion, and the boosted one hits harder.
    let (small_damage, large_damage) = (damage_of(&plain, small), damage_of(&boosted, large));
    assert!(small_damage > 0.0 && large_damage > small_damage, "damage {small_damage} against {large_damage}");
}
