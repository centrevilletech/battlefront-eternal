//! The launcher's rules for a roster whose classes open and close, a personal
//! countdown, and a mission that places a player who names no post. Every name
//! and number here is invented.
use super::controller::SpawnScreenController;
use super::model::*;

struct Port {
    human: HumanSpawnFacts,
    rosters: [Vec<SpawnClassEntry>; 2],
    posts: Vec<SpawnPostEntry>,
    wait: Option<f64>,
    places: bool,
    requested: Vec<SpawnRequest>,
}

fn class(name: &str, open: bool) -> SpawnClassEntry {
    SpawnClassEntry {
        class_name: name.into(),
        display_name: name.to_uppercase(),
        weapons: vec!["PEBBLE".into()],
        eligible: open,
    }
}

impl Port {
    fn new() -> Self {
        Self {
            human: HumanSpawnFacts::default(),
            rosters: [
                ["wren", "lark", "kite", "rook"].map(|n| class(n, true)).to_vec(),
                ["moth", "gnat", "flea"].map(|n| class(n, true)).to_vec(),
            ],
            posts: vec![SpawnPostEntry {
                name: "ford".into(),
                display_name: "FORD".into(),
                position: [3.0, 0.0, -9.0],
                owner: 1,
                eligible: true,
                vehicles: Vec::new(),
            }],
            wait: None,
            places: false,
            requested: Vec::new(),
        }
    }
    fn close(&mut self, team: u8, index: usize, open: bool) {
        self.rosters[usize::from(team == 2)][index].eligible = open;
    }
}

impl SpawnPort for Port {
    fn human_state(&self) -> HumanSpawnFacts {
        self.human.clone()
    }
    fn team_facts(&self, _team: u8) -> Option<SpawnTeamFacts> {
        Some(SpawnTeamFacts::default())
    }
    fn roster(&self, team: u8) -> Vec<SpawnClassEntry> {
        self.rosters[usize::from(team == 2)].clone()
    }
    fn posts(&self, team: u8) -> Vec<SpawnPostEntry> {
        self.posts
            .iter()
            .map(|p| SpawnPostEntry {
                eligible: p.eligible && p.owner == team,
                ..p.clone()
            })
            .collect()
    }
    fn request(&mut self, choice: &SpawnRequest) {
        self.requested.push(choice.clone());
    }
    fn suicide(&mut self) {}
    fn cancel(&mut self) {}
    fn camera_shots(&self) -> Vec<CameraShot> {
        Vec::new()
    }
    fn set_view_shot(&mut self, _shot: Option<CameraShot>) {}
    fn class_open(&self, team: u8, index: usize) -> bool {
        self.rosters[usize::from(team == 2)]
            .get(index)
            .is_some_and(|c| c.eligible)
    }
    fn deploy_wait(&self) -> Option<f64> {
        self.wait
    }
    fn post_required(&self) -> bool {
        !self.places
    }
}

fn opened(port: &Port) -> SpawnModel {
    let mut model = SpawnModel::new();
    model.open(port, OpenReason::Start);
    model
}

#[test]
fn keys_and_clicks_pass_over_a_closed_class() {
    let mut port = Port::new();
    port.close(1, 1, false);
    let mut model = opened(&port);
    assert!(!model.select_class(&port, 1));
    assert!(model.next_class(&port));
    assert_eq!(model.class_index, 2);
    assert!(model.previous_class(&port));
    assert_eq!(model.class_index, 0);
    assert!(model.previous_class(&port));
    assert_eq!(model.class_index, 3);
}

#[test]
fn the_screen_opens_and_switches_team_onto_an_open_class() {
    let mut port = Port::new();
    port.close(1, 0, false);
    port.close(2, 0, false);
    port.close(2, 1, false);
    let mut model = opened(&port);
    assert_eq!(model.class_index, 1);
    assert!(model.switch_team(&port, TeamSwitchSource::Key, 0.0));
    assert_eq!((model.team, model.class_index), (2, 2));
    assert!(model.drain_events().any(|e| e
        == SpawnEvent::Class {
            index: 2,
            class_name: "flea".into()
        }));
}

#[test]
fn the_selection_leaves_a_class_that_fills_and_waits_when_none_is_open() {
    let mut port = Port::new();
    let mut model = opened(&port);
    assert!(model.select_class(&port, 3));
    assert!(!model.check_class(&port));
    port.close(1, 3, false);
    assert!(model.check_class(&port));
    assert_eq!(model.class_index, 0);
    for i in 0..4 {
        port.close(1, i, false);
    }
    assert!(!model.check_class(&port));
    assert!(model.commit(&port).is_none());
    port.close(1, 2, true);
    assert!(model.check_class(&port));
    assert_eq!(model.class_index, 2);
    assert_eq!(model.commit(&port).map(|r| r.class_name), Some("kite".into()));
}

#[test]
fn a_countdown_holds_the_deployment_until_it_ends() {
    let mut port = Port::new();
    port.wait = Some(1.75);
    let mut ctrl = SpawnScreenController::new(600.0);
    ctrl.open(&mut port, OpenReason::Death, 600.0);
    assert!(ctrl.spawn(&mut port).is_none());
    assert!(port.requested.is_empty());
    port.wait = Some(0.0);
    assert_eq!(ctrl.spawn(&mut port).map(|r| r.post), Some("ford".into()));
}

#[test]
fn a_mission_that_places_the_player_deploys_without_a_post() {
    let mut port = Port::new();
    port.posts.clear();
    let mut model = opened(&port);
    assert_eq!(model.post, None);
    assert!(model.commit(&port).is_none());
    port.places = true;
    let request = model.commit(&port).expect("a placed deployment");
    assert_eq!((request.class_name.as_str(), request.post.as_str()), ("wren", ""));
}

#[test]
fn the_controller_moves_off_a_class_that_fills_while_it_is_open() {
    let mut port = Port::new();
    let mut ctrl = SpawnScreenController::new(600.0);
    ctrl.open(&mut port, OpenReason::Start, 600.0);
    ctrl.update(1.0 / 60.0, &mut port, 600.0, 18.0, false, 0.5);
    port.close(1, 0, false);
    ctrl.sounds.clear();
    ctrl.update(1.0 / 60.0, &mut port, 600.0, 18.0, false, 0.5);
    assert_eq!(ctrl.model.class_index, 1);
    assert_eq!(ctrl.sounds, [super::view::SpawnSound::SpawnPointChange]);
}
