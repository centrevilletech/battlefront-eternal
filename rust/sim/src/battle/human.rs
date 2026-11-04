//! The player's spawn state machine (src/human-spawn.js). The world owns
//! bodies, rosters and the Classic spawn manager; this owns when a choice
//! may be made, the waiting choice and the events the screen receives.
//! Commands arrive after runtime/commands.js humanEntry has validated them.

use crate::gen::{HumanAuto, HumanPhase, HumanPick, HumanSpawn};
use crate::store::{StrId, Strings};

pub const RESPAWN_DELAY: f64 = 4.0;
pub const TIME_SLACK: f64 = 1e-6;

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub phase: HumanPhase,
    pub team: f64,
    pub class_name: StrId,
    pub class_index: f64,
    pub post: Option<StrId>,
    pub can_switch_teams: bool,
    pub over: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    AwaitingSpawn(State),
    Spawned { team: f64, class_name: StrId, post: Option<StrId> },
    TeamChanged { team: f64, previous: f64 },
    PlayerDied { team: f64, class_name: StrId },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Team(f64),
    Spawn(HumanPick),
    Suicide,
    Cancel,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Authority {
    pub phase: HumanPhase,
    pub class_name: Option<StrId>,
    pub post: Option<StrId>,
    pub spawned_once: bool,
    pub ended: bool,
    pub dead_time: f64,
    pub pending: Option<HumanPick>,
    pub auto: Option<HumanAuto>,
}

impl Command {
    /// humanEntry's team and name checks. Names count UTF-16 code units.
    pub fn team(team: f64) -> Option<Self> {
        (team.is_finite() && team.fract() == 0.0 && (1.0..=7.0).contains(&team)).then_some(Self::Team(team))
    }

    pub fn spawn(class_name: StrId, post: Option<StrId>, strings: &Strings) -> Option<Self> {
        let valid = |id| (1..=64).contains(&strings.units(id).len());
        (valid(class_name) && post.is_none_or(valid)).then_some(Self::Spawn(HumanPick { class_name, post }))
    }
}

/// The game's HumanSpawn world. Calls that can kill, spawn or change team
/// receive the state and event queue: the JavaScript callbacks synchronously
/// call died, spawned or teamChanged before returning to the state machine.
pub trait World {
    fn team(&mut self) -> f64;
    fn dead(&mut self) -> bool;
    fn over(&mut self) -> bool;
    fn roster(&mut self, team: f64) -> Vec<StrId>;
    fn classic(&mut self) -> bool;
    /// character?.spawn: whether he has a unit, and the spawn timer.
    fn character_spawn(&mut self) -> Option<(bool, f64)>;
    fn can_spawn(&mut self, class_name: StrId) -> bool;
    fn set_team(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, team: f64);
    fn spawn(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &HumanPick);
    fn ready(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &HumanPick) -> bool;
    fn kill(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>);
    fn cancel(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>);
    /// The warning when the roster or the world's loaded classes refuse.
    fn rejected(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, class_name: StrId, team: f64);
}

pub fn new(class_name: StrId, auto: Option<HumanAuto>, post: Option<StrId>) -> HumanSpawn {
    HumanSpawn {
        auto, class_name, post, phase: HumanPhase::Unspawned,
        can_switch_teams: true, spawned_once: false, dead_time: 0.0,
        pending: None, ended: false,
    }
}

pub fn state(s: &HumanSpawn, world: &mut dyn World) -> State {
    let team = world.team();
    let roster = world.roster(team);
    let index = roster.iter().position(|&name| name == s.class_name);
    State {
        phase: s.phase, team, class_name: s.class_name, class_index: index.unwrap_or(0) as f64,
        post: s.post, can_switch_teams: s.can_switch_teams, over: world.over(),
    }
}

/// flush's splice(0). The page dispatches these in order and keeps listener
/// errors out of the battle. Events queued by a listener wait for the next
/// flush, as they do in JavaScript.
pub fn flush(events: &mut Vec<Event>) -> Vec<Event> {
    std::mem::take(events)
}

fn truthy(id: Option<StrId>, strings: &Strings) -> Option<StrId> {
    id.filter(|&id| !strings.units(id).is_empty())
}

/// applyAuthoritative: accept the server's state, then return the events
/// that must be dispatched at once (there is no local world tick online).
pub fn apply_authoritative(
    s: &mut HumanSpawn, mut next: Authority, events: &mut Vec<Event>,
    strings: &Strings, world: &mut dyn World,
) -> Vec<Event> {
    next.class_name = truthy(next.class_name, strings);
    next.post = truthy(next.post, strings);
    apply_authoritative_normalized(s, next, events, world)
}

/// The module validates strings before calling the world, so an import
/// may reenter the module and intern more strings safely.
pub fn apply_authoritative_normalized(
    s: &mut HumanSpawn, next: Authority, events: &mut Vec<Event>, world: &mut dyn World,
) -> Vec<Event> {
    let was = s.phase;
    let team = world.team();
    if let Some(class_name) = next.class_name { s.class_name = class_name; }
    if next.post.is_some() { s.post = next.post; }
    s.phase = next.phase;
    s.spawned_once = next.spawned_once;
    s.ended = next.ended;
    s.dead_time = next.dead_time;
    s.pending = next.pending;
    s.auto = next.auto;
    if s.phase == HumanPhase::Alive {
        if was != HumanPhase::Alive {
            events.push(Event::Spawned { team, class_name: s.class_name, post: s.post });
        }
    } else if was == HumanPhase::Alive {
        events.push(Event::PlayerDied { team, class_name: s.class_name });
    }
    if matches!(s.phase, HumanPhase::Awaiting | HumanPhase::Unspawned)
        && !matches!(was, HumanPhase::Awaiting | HumanPhase::Unspawned)
    {
        events.push(Event::AwaitingSpawn(state(s, world)));
    }
    flush(events)
}

pub fn died(s: &mut HumanSpawn, events: &mut Vec<Event>, world: &mut dyn World) {
    s.phase = HumanPhase::Dying;
    s.dead_time = 0.0;
    events.push(Event::PlayerDied { team: world.team(), class_name: s.class_name });
}

pub fn spawned(
    s: &mut HumanSpawn, events: &mut Vec<Event>, class_name: StrId,
    post: Option<StrId>, strings: &Strings, world: &mut dyn World,
) {
    spawned_normalized(s, events, class_name, truthy(post, strings), world);
}

pub fn spawned_normalized(
    s: &mut HumanSpawn, events: &mut Vec<Event>, class_name: StrId,
    post: Option<StrId>, world: &mut dyn World,
) {
    let was = s.phase;
    s.class_name = class_name;
    if post.is_some() { s.post = post; }
    s.phase = HumanPhase::Alive;
    s.spawned_once = true;
    s.pending = None;
    if was != HumanPhase::Alive {
        events.push(Event::Spawned { team: world.team(), class_name, post });
    }
}

pub fn team_changed(s: &mut HumanSpawn, events: &mut Vec<Event>, team: f64, previous: f64, world: &mut dyn World) {
    let before = world.roster(previous).iter().position(|&name| name == s.class_name);
    let roster = world.roster(team);
    if !roster.contains(&s.class_name) && !roster.is_empty() {
        s.class_name = roster[before.unwrap_or(0).min(roster.len() - 1)];
    }
    events.push(Event::TeamChanged { team, previous });
}

pub fn update(s: &mut HumanSpawn, events: &mut Vec<Event>, dt: f64, commands: &[Command], world: &mut dyn World) {
    if world.dead() { s.dead_time += dt; }
    for command in commands { apply(s, events, command, world); }
    if s.auto.is_none() && !s.ended && world.over() {
        s.ended = true;
        s.pending = None;
        world.cancel(s, events);
    }
    update_phase(s, events, world);
    if s.pending.is_some() && matches!(s.phase, HumanPhase::Unspawned | HumanPhase::Awaiting) {
        realize(s, events, world);
    }
}

pub fn apply(s: &mut HumanSpawn, events: &mut Vec<Event>, entry: &Command, world: &mut dyn World) {
    if *entry == Command::Suicide {
        if !world.dead() { world.kill(s, events); }
        return;
    }
    if s.auto.is_some() || world.over() { return; }
    match entry {
        Command::Team(team) => {
            if world.dead() && *team != world.team() { world.set_team(s, events, *team); }
        }
        Command::Spawn(choice) => {
            if world.dead() { s.pending = Some(choice.clone()); }
        }
        Command::Cancel => {
            s.pending = None;
            world.cancel(s, events);
        }
        Command::Suicide => unreachable!(),
    }
}

pub fn update_phase(s: &mut HumanSpawn, events: &mut Vec<Event>, world: &mut dyn World) {
    let was = s.phase;
    s.phase = if !world.dead() {
        HumanPhase::Alive
    } else if !s.spawned_once {
        HumanPhase::Unspawned
    } else {
        let classic = world.classic();
        let character = world.character_spawn();
        let open = !world.over() && match (classic, character) {
            (true, Some((unit, timer))) => !unit && timer <= 0.0,
            _ => s.dead_time >= RESPAWN_DELAY - TIME_SLACK,
        };
        if open { HumanPhase::Awaiting } else { HumanPhase::Dying }
    };
    if s.phase == HumanPhase::Awaiting && was != HumanPhase::Awaiting {
        events.push(Event::AwaitingSpawn(state(s, world)));
    }
}

pub fn realize(s: &mut HumanSpawn, events: &mut Vec<Event>, world: &mut dyn World) {
    // Take a copy before calling the world: spawn may clear pending.
    let choice = s.pending.as_ref().expect("a waiting spawn choice").clone();
    let team = world.team();
    if !world.roster(team).contains(&choice.class_name) || !world.can_spawn(choice.class_name) {
        let team = world.team();
        world.rejected(s, events, choice.class_name, team);
        s.pending = None;
        return;
    }
    s.class_name = choice.class_name;
    if world.classic() {
        if world.ready(s, events, &choice) { s.pending = None; }
    } else {
        world.spawn(s, events, &choice);
    }
}
