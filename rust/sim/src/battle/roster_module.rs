//! Native multiplayer roster commands on the battle's shared store.
use super::roster::{self, Players, PlayersWorld, RosterWorld};
use super::rules_module::{self, ModuleText};
use super::spawning;
use super::spawning_module::{self, JsDraws, JsSpawningWorld};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{Ref, StrId};

impl From<RoPlayers> for Players {
    fn from(p: RoPlayers) -> Self { Self { capacity: p.capacity, reserved: p.reserved } }
}
impl From<Players> for RoPlayers {
    fn from(p: Players) -> Self { Self { capacity: p.capacity, reserved: p.reserved } }
}

struct JsWorld { classic: bool }
impl JsWorld {
    fn swap(&self, s: &mut Store) {
        if self.classic { spawning_module::swap(s); } else { rules_module::swap(s); }
    }
    fn action(&self, s: &mut Store, args: RoActionArgs) -> RoActionResult {
        self.swap(s);
        let out = import_roster_action(&args);
        self.swap(s);
        out
    }
    fn named(&self, name: &[u16]) -> StrId { sim().strings.intern(roster::player_name(name)) }
}
impl RosterWorld for JsWorld {
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool { import_roster_query(&RoQueryArgs { unit }).alive }
    fn ai_detach(&mut self, s: &mut Store, character: Ref<Character>) { self.action(s, RoActionArgs { op: RoActionOp::Detach, character: Some(character), ..Default::default() }); }
    fn ai_attach(&mut self, s: &mut Store, character: Ref<Character>) { self.action(s, RoActionArgs { op: RoActionOp::Attach, character: Some(character), ..Default::default() }); }
    fn retire_unit(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>) { self.action(s, RoActionArgs { op: RoActionOp::Retire, character: Some(character), unit: Some(unit), ..Default::default() }); }
    fn bot_name(&mut self, s: &mut Store, team: usize) -> StrId { self.action(s, RoActionArgs { op: RoActionOp::BotName, team: team as f64, ..Default::default() }).name.expect("the team's next name") }
    fn player_name(&mut self, name: &[u16]) -> StrId { self.named(name) }
    fn insert(&mut self, s: &mut Store, team: f64, name: StrId) -> Option<Ref<Character>> { self.action(s, RoActionArgs { op: RoActionOp::Insert, team, name: Some(name), ..Default::default() }).character }
    fn activate(&mut self, s: &mut Store, character: Ref<Character>) { spawning::activate(s, character, &ModuleText, &mut JsSpawningWorld, &mut JsDraws); }
    fn pad_quotas(&mut self, s: &mut Store, team: usize) { spawning::pad_quotas(&mut s.spawning, team, &mut JsDraws); }
}
impl PlayersWorld for JsWorld {
    fn player_name(&mut self, name: &[u16]) -> StrId { self.named(name) }
    fn new_human(&mut self, players: &mut Players, s: &mut Store, team: f64) -> Ref<Character> {
        let out = self.action(s, RoActionArgs { op: RoActionOp::NewHuman, team, players: Some(players.clone().into()), ..Default::default() });
        *players = out.players.expect("the player's roster").into();
        out.character.expect("the new player")
    }
    fn remove_body(&mut self, players: &mut Players, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>) {
        let out = self.action(s, RoActionArgs { op: RoActionOp::RemoveBody, character: Some(character), unit: Some(unit), players: Some(players.clone().into()), ..Default::default() });
        *players = out.players.expect("the player's roster").into();
    }
}

#[no_mangle]
pub extern "C" fn roster_words(which: u32) -> u32 { (match which { 0 => RoInput::WORDS, 1 => RoResult::WORDS, _ => 0 }) as u32 }

#[no_mangle]
pub extern "C" fn roster_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); RoInput::take(row, heap) };
    let name = sim().strings.units(input.name).to_vec();
    let mut world = JsWorld { classic: input.classic };
    let mut players: Players = input.players.into();
    let mut s = Store::default();
    world.swap(&mut s);
    let mut out = RoResult::default();
    match input.op {
        RoOperation::Create => {
            if input.classic { roster::create_with(&mut s, input.army_size, input.local, &mut world); }
            else { players = Players::new(&mut s, input.army_size, input.has_ai); }
        }
        RoOperation::Reserve => {
            let r = if input.classic { roster::reserve_with(&mut s, input.team, input.player_id, &name, &mut world) }
                else { players.reserve(&mut s, input.team, input.player_id, &name, &mut world) };
            match r { Ok(character) => out.character = Some(character), Err(_) => out.full = true }
        }
        RoOperation::Release => {
            let character = input.character.expect("the player leaving");
            if input.classic { roster::release_with(&mut s, character, &mut world); }
            else { players.release(&mut s, character, &mut world); }
        }
        RoOperation::SwitchTeam => {
            let character = input.character.expect("the player switching teams");
            out.flag = if input.classic { roster::switch_team_with(&mut s, character, input.team, &mut world) }
                else { players.switch_team(&mut s, character, input.team) };
        }
        RoOperation::Counts => {
            out.counts = if input.classic { roster::counts(&s).into_iter().map(|(team, humans, total)| RoCount { team, humans, total: Some(total) }).collect() }
                else { [1.0, 2.0].into_iter().map(|team| RoCount { team, humans: players.humans(&s, team), total: None }).collect() };
        }
    }
    out.players = players.into();
    world.swap(&mut s);
    let sim = sim();
    sim.calls.result(RoResult::WORDS, |row, heap| out.put(row, heap));
    sim.calls.block.ptr()
}
