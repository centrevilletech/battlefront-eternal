use bfe_battle_ui::battle2::*;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Recording<'a> {
    /// Texture handle names and image paths relative to the prepared family root.
    pub images: BTreeMap<&'a str, &'a str>,
    pub stages: Vec<Stage<'a>>,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Stage<'a> {
    pub name: &'a str,
    pub backdrop: &'a str,
    pub keys: Vec<&'a str>,
    tick: u32,
    phase: Phase,
    mode: Mode,
    player: u32,
    team: u8,
    map_name: Caption<'a>,
    map_image: &'a str,
    teams: [Team<'a>; 2],
    body: Option<Body<'a>>,
    vehicle: Option<Vehicle<'a>>,
    target: Option<Target<'a>>,
    #[serde(default)]
    aim: Aim<'a>,
    contacts: Vec<Contact<'a>>,
    systems: Vec<System<'a>>,
    objectives: Vec<Objective<'a>>,
    timers: Vec<Timer<'a>>,
    events: Vec<Event<'a>>,
    scores: Vec<ScoreRow<'a>>,
    #[serde(default)]
    leaving: bool,
}

impl Stage<'_> {
    pub fn view(&self) -> BattleView<'_> {
        BattleView {
            tick: self.tick,
            phase: self.phase,
            mode: self.mode,
            player: self.player,
            team: self.team,
            map_name: self.map_name,
            map_image: self.map_image,
            teams: self.teams,
            body: self.body,
            vehicle: self.vehicle,
            target: self.target,
            aim: self.aim,
            contacts: &self.contacts,
            systems: &self.systems,
            objectives: &self.objectives,
            timers: &self.timers,
            events: &self.events,
            scores: &self.scores,
            leaving: self.leaving,
        }
    }
}
