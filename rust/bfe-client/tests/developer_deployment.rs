use bfe_client::{
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    local::{context::Native, launch::Prepared, runtime::Runtime, thread::PublicationSlot},
};
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_sim::{
    abi::context::Context,
    gen::HumanPhase,
    world::admit::{
        mission::{MissionPreferences, MissionSelection},
        Bots, Options,
    },
};

/// The opening Hero Assault battle these tests play.
const ENTRY: &str = "swbf2/tat2:gcw:eli";

fn prepared() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var_os("BFE_SWBF2_PREPARED").expect("BFE_SWBF2_PREPARED names the prepared 2005 content"),
    )
}

#[test]
#[ignore = "requires a prepared Hero Assault mission"]
fn connected_mission_player_returns_from_the_spawn_queue() {
    let root = prepared();
    let entry = bfe_shell::launch::parse_rotation(Some(ENTRY)).remove(0);
    let selection = MissionSelection {
        family: entry.family,
        map: entry.map,
        era: entry.era.unwrap(),
        mode: entry.mode,
        seed: 7,
    };
    let options = Options {
        family: selection.family,
        map: selection.map.clone(),
        mode: selection.mode.clone(),
        seed: selection.seed,
        side: "all".into(),
        bots: Bots::read("classic", "medium", "full"),
        ..Options::default()
    };
    let context = Context::new();
    let (prepared, _) = Prepared::developer_mission_in(
        context.clone(),
        &root,
        &options,
        &selection,
        MissionPreferences {
            bots_per_team: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    let mut slot = PublicationSlot::new_preallocated();
    let mut runtime = Runtime::new_into_slot(
        *prepared.engine,
        *prepared.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
        &mut slot,
    )
    .unwrap();
    let mut previous = None;
    let mut died = None;
    let mut returned = None;
    for tick in 1..=1400 {
        if tick == 300 {
            runtime.respawn_human().unwrap();
        }
        let command = ClientCommand {
            tick: tick as f64,
            sequence: tick as f64,
            generation: slot.infantry.map_or(0., |sample| sample.generation),
            ..Default::default()
        };
        runtime
            .submit(
                &command,
                &ClientLook::default(),
                &ClientFlightCommand::default(),
                runtime.tick(),
                None,
                false,
            )
            .unwrap();
        runtime.step_into_slot(&mut slot).unwrap();
        let (phase, post, class, body, life) = context
            .owned(|w| {
                let view = w.local_mission_connection().unwrap();
                let mut life = None;
                w.visit_infantry(|id, human, health, _, _| {
                    if human && health > 0. && view.unit.is_some_and(|unit| unit.slot == id.slot) {
                        life = Some(id);
                    }
                });
                (view.phase, view.post, view.class_name, view.unit, life)
            })
            .unwrap();
        if tick < 300 && phase == HumanPhase::Alive {
            assert!(post.is_some());
            previous = Some((class, body, life.expect("deployed human has a live body")));
        }
        if tick >= 300 && phase != HumanPhase::Alive {
            died.get_or_insert(tick);
        }
        if let (Some(dead), Some(life)) = (died, life) {
            if phase == HumanPhase::Alive {
                let (old_class, old_body, old_life) = previous.unwrap();
                assert_eq!((old_class, old_body), (class, body));
                assert!(post.is_some() && life.generation > old_life.generation);
                assert!(tick - dead >= 239, "personal respawn delay");
                returned = Some((dead, tick));
                break;
            }
        }
    }
    let (dead, returned) = returned.expect("connected player must redeploy in the mission");
    println!(
        "connected mission death={dead} returned={returned} gap={}",
        returned - dead
    );
    runtime.close();
}

fn admit_mission() -> (Context, Runtime<Native>, PublicationSlot) {
    let root = prepared();
    let entry = bfe_shell::launch::parse_rotation(Some(ENTRY)).remove(0);
    let selection = MissionSelection {
        family: entry.family,
        map: entry.map,
        era: entry.era.unwrap(),
        mode: entry.mode,
        seed: 7,
    };
    let options = Options {
        family: selection.family,
        map: selection.map.clone(),
        mode: selection.mode.clone(),
        seed: selection.seed,
        side: "all".into(),
        bots: Bots::read("classic", "medium", "full"),
        ..Options::default()
    };
    let context = Context::new();
    let (mut prepared, _) = Prepared::developer_mission_in(
        context.clone(),
        &root,
        &options,
        &selection,
        MissionPreferences { bots_per_team: Some(32), ..Default::default() },
    )
    .unwrap();
    prepared.engine.use_mission_spawn_screen();
    let mut slot = PublicationSlot::new_preallocated();
    let runtime = Runtime::new_into_slot(
        *prepared.engine,
        *prepared.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
        &mut slot,
    )
    .unwrap();
    (context, runtime, slot)
}

/// One tick of idle input; the connected player's view afterwards:
/// (phase, spawned_once, personal countdown in ticks, deployable now).
fn step(context: &Context, runtime: &mut Runtime<Native>, slot: &mut PublicationSlot, tick: u32) -> (HumanPhase, bool, Option<u32>, bool) {
    let command = ClientCommand {
        tick: tick as f64,
        sequence: tick as f64,
        generation: slot.infantry.map_or(0., |sample| sample.generation),
        ..Default::default()
    };
    runtime
        .submit(&command, &ClientLook::default(), &ClientFlightCommand::default(), runtime.tick(), None, false)
        .unwrap();
    runtime.step_into_slot(slot).unwrap();
    context
        .owned(|w| {
            let view = w.local_mission_connection().unwrap();
            (view.phase, view.spawned_once, w.local_spawn_due(0), w.mission_deployment_choice().is_some())
        })
        .unwrap()
}

/// The class names the admitted mission lists for team 1 or 2.
fn roster(context: &Context, team: u8) -> Vec<String> {
    let classes = context
        .owned(|w| {
            let view = w.mission_view().expect("a mission");
            view.host.setup.teams[&i32::from(team)].classes.iter().map(|c| c.class_name.clone()).collect::<Vec<_>>()
        })
        .unwrap();
    assert!(!classes.is_empty(), "team {team} lists no class");
    classes
}

struct Game {
    context: Context,
    runtime: Runtime<Native>,
    slot: PublicationSlot,
    ticks: u32,
}

impl Game {
    fn new() -> Self {
        let (context, runtime, slot) = admit_mission();
        Game { context, runtime, slot, ticks: 0 }
    }
    fn step(&mut self) -> (HumanPhase, bool, Option<u32>, bool) {
        self.ticks += 1;
        step(&self.context, &mut self.runtime, &mut self.slot, self.ticks)
    }
    fn wait(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step();
        }
    }
    fn roster(&self, team: u8) -> Vec<String> {
        roster(&self.context, team)
    }
    /// Choose a team and class as the spawn screen does, naming no post (the
    /// mission places the player), then give the battle a moment: whether the
    /// player has a body afterwards.
    fn deploy(&mut self, team: u8, class: &str) -> bool {
        self.runtime.spawn_human(team, class, "").unwrap();
        (0..30).any(|_| self.step().0 == HumanPhase::Alive)
    }
    fn die(&mut self) {
        self.runtime.respawn_human().unwrap();
        self.wait(2);
    }
    /// Wait until the spawn button lights again: whether a countdown showed on
    /// the way. Every countdown is a real number of ticks.
    fn until_ready(&mut self) -> bool {
        let mut counted = false;
        for _ in 0..3000 {
            let (_, _, due, ready) = self.step();
            assert!(due.is_none_or(|d| d < 60 * 60), "countdown {due:?}");
            counted |= due.is_some_and(|d| d > 0);
            if ready {
                return counted;
            }
        }
        panic!("the spawn button never lit again");
    }
}

#[test]
#[ignore = "requires a prepared Hero Assault mission"]
fn a_player_who_switches_team_before_any_life_deploys() {
    let mut game = Game::new();
    game.wait(10);
    let class = game.roster(2).remove(0);
    assert!(game.deploy(2, &class), "the player never deployed on team 2");
}

#[test]
#[ignore = "requires a prepared Hero Assault mission"]
fn a_waiting_player_sees_no_countdown_and_a_lit_button() {
    let mut game = Game::new();
    for _ in 0..1200 {
        let (_, _, due, ready) = game.step();
        assert_eq!(due, None);
        assert!(ready);
    }
}

#[test]
#[ignore = "requires a prepared Hero Assault mission"]
fn every_hero_is_given_to_the_player_whatever_the_bots_hold() {
    // The 2005 game limits a human only in a class with points to unlock, and
    // no hero of this mission has any: after the bots have taken their places,
    // every class of either team deploys.
    let mut game = Game::new();
    game.wait(1500);
    for team in [2u8, 1u8] {
        for class in game.roster(team) {
            assert!(game.step().3, "the spawn button is lit before a choice");
            assert!(game.deploy(team, &class), "team {team} class {class} was refused at tick {}", game.ticks);
            game.die();
            game.until_ready();
        }
    }
}

#[test]
#[ignore = "requires a prepared Hero Assault mission"]
fn a_player_can_change_teams_back_and_forth_between_lives() {
    let mut game = Game::new();
    game.wait(20);
    for team in [1, 2, 1, 2] {
        let class = game.roster(team).remove(0);
        assert!(game.deploy(team, &class), "no deployment on team {team} at tick {}", game.ticks);
        game.wait(60);
        game.die();
        assert!(game.until_ready(), "no countdown after a death on team {team}");
    }
}

#[test]
#[ignore = "requires a prepared Hero Assault mission"]
fn the_parked_places_stay_parked() {
    let mut game = Game::new();
    game.wait(2);
    let parked = game.context.owned(|w| w.parked_mission_places()).unwrap();
    assert!(parked > 0, "the mission parks no place for a player");
    for _ in 0..600 {
        game.step();
        assert_eq!(game.context.owned(|w| w.parked_mission_places()).unwrap(), parked, "tick {}", game.ticks);
    }
}
