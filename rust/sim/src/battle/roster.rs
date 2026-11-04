//! The multiplayer roster (src/runtime/multiplayer-roster.js) with the
//! Classic AI: network players take the places of bots without spending a
//! reinforcement, and a Character keeps its place in the battle's list for
//! the battle (the network names Characters by place). When the roster is
//! made, the local player's Character goes dormant, each army is trimmed to
//! its size (the Characters cut go dormant too) and filled with bots; a
//! player joining takes a bot's place (one whose body is dead first), one
//! leaving hands his place back to a new bot, and one switching teams trades
//! places with a bot of the other team.
//!
//! The state is the store's (rust/sim/schema/battle.mjs). What the roster
//! asks of the rest of the world (the AI's attach and detach, a body taken
//! out of the battle) goes through [`BattleWorld`].

use super::spawning::{self, Draws, Naming};
use super::world::BattleWorld;
use crate::gen::{Character, Nullable, PlayerId, Store, Unit};
use crate::store::{Ref, StrId, Strings};

/// MAX_TEAM_SIZE (src/battle-limits.js).
pub const MAX_TEAM_SIZE: f64 = 32.0;

/// Services used while the roster owns its team and Character state.
/// Name generators and Character construction can be native or supplied
/// by the existing spawn manager during the live transition.
pub trait RosterWorld {
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool;
    fn ai_detach(&mut self, s: &mut Store, character: Ref<Character>);
    fn ai_attach(&mut self, s: &mut Store, character: Ref<Character>);
    fn retire_unit(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>);
    fn bot_name(&mut self, s: &mut Store, team: usize) -> StrId;
    fn player_name(&mut self, name: &[u16]) -> StrId;
    fn insert(&mut self, s: &mut Store, team: f64, name: StrId) -> Option<Ref<Character>>;
    fn activate(&mut self, s: &mut Store, character: Ref<Character>);
    fn pad_quotas(&mut self, s: &mut Store, team: usize);
}

struct NativeWorld<'a> {
    world: &'a mut dyn BattleWorld,
    strings: &'a mut Strings,
    naming: Option<&'a mut Naming>,
    random: Option<&'a mut dyn Draws>,
}
impl RosterWorld for NativeWorld<'_> {
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool { self.world.unit_alive(unit) }
    fn ai_detach(&mut self, s: &mut Store, character: Ref<Character>) { self.world.ai_detach(s, character); }
    fn ai_attach(&mut self, s: &mut Store, character: Ref<Character>) { self.world.ai_attach(s, character); }
    fn retire_unit(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>) { self.world.retire_unit(s, character, unit); }
    fn bot_name(&mut self, s: &mut Store, team: usize) -> StrId { spawning::bot_name(&s.spawning, team, self.naming.as_deref_mut().expect("the roster's names"), self.strings) }
    fn player_name(&mut self, name: &[u16]) -> StrId { self.strings.intern(player_name(name)) }
    fn insert(&mut self, s: &mut Store, team: f64, name: StrId) -> Option<Ref<Character>> { spawning::insert(s, team, false, -1.0, name, self.strings) }
    fn activate(&mut self, s: &mut Store, character: Ref<Character>) { spawning::activate(s, character, self.strings, self.world, self.random.as_deref_mut().expect("the roster's draws")); }
    fn pad_quotas(&mut self, s: &mut Store, team: usize) { spawning::pad_quotas(&mut s.spawning, team, self.random.as_deref_mut().expect("the roster's draws")); }
}

/// A player's name as the roster keeps it: String(name).slice(0, 32), in
/// UTF-16 units.
pub fn player_name(name: &[u16]) -> &[u16] {
    &name[..name.len().min(32)]
}

fn ch_mut(s: &mut Store, r: Ref<Character>) -> &mut Character {
    s.character.get_mut(r).expect("a Character of the battle")
}

/// retire(character) (multiplayer-roster.js:29): out of his post's queue and
/// the AI, his body (if any) out of the battle, its team's living count down
/// if it lived.
pub fn retire<W: RosterWorld + ?Sized>(s: &mut Store, r: Ref<Character>, world: &mut W) {
    spawning::change_post(s, r, None);
    world.ai_detach(s, r);
    let c = s.character.get(r).expect("a Character of the battle");
    if let Some(unit) = c.unit {
        let (team, hero) = (c.team, c.hero == Some(true));
        if world.unit_alive(unit) && !hero {
            s.spawning.teams[team as usize].living -= 1.0;
        }
        world.retire_unit(s, r, unit);
    }
    ch_mut(s, r).unit = None;
}

/// reset(character) (multiplayer-roster.js:44): fresh scores.
fn reset(c: &mut Character) {
    c.kills = 0.0;
    c.team_kills = 0.0;
    c.deaths = 0.0;
    c.captures = 0.0;
    c.record = Nullable::Null;
    c.objective = crate::gen::PostRef::Null;
}

/// bot(character) (multiplayer-roster.js:49): a bot again, named by its
/// team, back in the AI's hands.
#[allow(clippy::too_many_arguments)]
fn bot<W: RosterWorld + ?Sized>(s: &mut Store, r: Ref<Character>, world: &mut W) {
    let team = s.character.get(r).unwrap().team as usize;
    let c = ch_mut(s, r);
    c.human = false;
    c.player_id = PlayerId::F64(-1.0);
    let name = world.bot_name(s, team);
    let c = ch_mut(s, r);
    c.name = Some(name);
    reset(c);
    c.spawn.as_mut().expect("a Character the spawn manager made").timer = 0.0;
    world.activate(s, r);
    world.ai_attach(s, r);
}

/// The roster's sizes (createMultiplayerRoster's armySize: a number, or
/// None for each army's mission size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roster {
    pub army_size: Option<f64>,
}

/// createMultiplayerRoster(game, armySize) (multiplayer-roster.js:5), with
/// the Classic AI: the local player's Character dormant, each army trimmed
/// and filled to its size with bots, their class targets padded.
#[allow(clippy::too_many_arguments)]
pub fn create_with<W: RosterWorld + ?Sized>(s: &mut Store, army_size: Option<f64>, local: Option<Ref<Character>>, world: &mut W) -> Roster {
    if let Some(dormant) = local {
        retire(s, dormant, world);
        let c = ch_mut(s, dormant);
        c.network_dormant = Some(true);
        let spawn = c.spawn.as_mut().expect("a Character the spawn manager made");
        spawn.ready = false;
        spawn.timer = f64::INFINITY;
        let team = c.team as usize;
        // splice(indexOf(dormant), 1): not found (-1) takes the last.
        let list = &mut s.spawning.teams[team].characters;
        match list.iter().position(|x| *x == dormant) {
            Some(i) => {
                list.remove(i);
            }
            None => {
                list.pop();
            }
        }
    }
    for number in [1usize, 2] {
        let unit_count = s.spawning.teams[number].unit_count;
        let capacity = army_size.unwrap_or_else(|| v8math::min(MAX_TEAM_SIZE, v8math::max(1.0, unit_count)));
        while s.spawning.teams[number].characters.len() as f64 > capacity {
            let r = s.spawning.teams[number].characters.pop().unwrap();
            retire(s, r, world);
            let c = ch_mut(s, r);
            c.network_dormant = Some(true);
            c.human = true;
            let spawn = c.spawn.as_mut().expect("a Character the spawn manager made");
            spawn.ready = false;
            spawn.timer = f64::INFINITY;
        }
        s.spawning.teams[number].capacity = capacity;
        s.spawning.teams[number].unit_count = capacity;
        while (s.spawning.teams[number].characters.len() as f64) < capacity {
            // Math.max(-1, ...indices) + 1
            let mut index = -1.0;
            for &c in &s.spawning.teams[number].characters {
                index = v8math::max(index, s.character.get(c).unwrap().index);
            }
            let index = index + 1.0;
            let name = world.bot_name(s, number);
            let r = world.insert(s, number as f64, name).expect("room in the team");
            let c = ch_mut(s, r);
            c.index = index;
            c.slot = Some(index);
            world.activate(s, r);
            world.ai_attach(s, r);
        }
        world.pad_quotas(s, number);
    }
    Roster { army_size }
}

/// available(team) (multiplayer-roster.js:89): the team's first bot whose
/// body is dead, else its first bot (a stable sort by whether it lives).
fn available<W: RosterWorld + ?Sized>(s: &Store, team: f64, world: &mut W) -> Option<Ref<Character>> {
    let mut bots: Vec<(Ref<Character>, f64)> = Vec::new();
    for &r in &s.spawning.teams[team as usize].characters {
        let c = s.character.get(r).unwrap();
        if !c.human {
            let alive = c.unit.is_some_and(|u| world.unit_alive(u));
            bots.push((r, if alive { 1.0 } else { 0.0 }));
        }
    }
    jsrt::sort::js_sort(&mut bots, &mut |a: &(Ref<Character>, f64), b: &(Ref<Character>, f64)| a.1 - b.1);
    bots.first().map(|(r, _)| *r)
}

/// reserve(team, id, name) (multiplayer-roster.js:92): a player takes a
/// bot's place; Err when the team has none.
#[allow(clippy::too_many_arguments)]
pub fn reserve_with<W: RosterWorld + ?Sized>(s: &mut Store, team: f64, id: PlayerId, name: &[u16], world: &mut W) -> Result<Ref<Character>, &'static str> {
    let Some(r) = available(s, team, world) else { return Err("team full") };
    retire(s, r, world);
    let name = world.player_name(name);
    let c = ch_mut(s, r);
    reset(c);
    c.human = true;
    c.hero = Some(false);
    c.player_id = id;
    c.name = Some(name);
    let spawn = c.spawn.as_mut().expect("a Character the spawn manager made");
    spawn.ready = false;
    spawn.timer = f64::INFINITY;
    Ok(r)
}

/// release(character) (multiplayer-roster.js:105): a player leaves; a bot
/// takes his place.
pub fn release_with<W: RosterWorld + ?Sized>(s: &mut Store, r: Ref<Character>, world: &mut W) {
    retire(s, r, world);
    bot(s, r, world);
}

/// switchTeam(character, target) (multiplayer-roster.js:106): he trades
/// places with a bot of the other team; false when it has none.
#[allow(clippy::too_many_arguments)]
pub fn switch_team_with<W: RosterWorld + ?Sized>(s: &mut Store, r: Ref<Character>, target: f64, world: &mut W) -> bool {
    let previous = s.character.get(r).unwrap().team;
    if target == previous {
        return true;
    }
    let Some(replacement) = available(s, target, world) else { return false };
    retire(s, replacement, world);
    spawning::change_post(s, r, None);
    let swap = |list: &mut Vec<Ref<Character>>, from: Ref<Character>, to: Ref<Character>| {
        // source[source.indexOf(character)] = replacement: not found writes
        // index -1, a property no loop reads (the list is unchanged).
        if let Some(i) = list.iter().position(|x| *x == from) {
            list[i] = to;
        }
    };
    swap(&mut s.spawning.teams[previous as usize].characters, r, replacement);
    swap(&mut s.spawning.teams[target as usize].characters, replacement, r);
    ch_mut(s, r).team = target;
    ch_mut(s, replacement).team = previous;
    bot(s, replacement, world);
    true
}

/// counts() (multiplayer-roster.js:120): each army's players and size.
pub fn counts(s: &Store) -> [(f64, f64, f64); 2] {
    [1.0, 2.0].map(|team| {
        let list = &s.spawning.teams[team as usize].characters;
        let humans = list.iter().filter(|r| s.character.get(**r).unwrap().human).count() as f64;
        (team, humans, list.len() as f64)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn create(s: &mut Store, army_size: Option<f64>, local: Option<Ref<Character>>, naming: &mut Naming, strings: &mut Strings, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> Roster {
    create_with(s, army_size, local, &mut NativeWorld { world, strings, naming: Some(naming), random: Some(random) })
}
pub fn reserve(s: &mut Store, team: f64, id: PlayerId, name: &[u16], strings: &mut Strings, world: &mut dyn BattleWorld) -> Result<Ref<Character>, &'static str> {
    reserve_with(s, team, id, name, &mut NativeWorld { world, strings, naming: None, random: None })
}
pub fn release(s: &mut Store, character: Ref<Character>, naming: &mut Naming, strings: &mut Strings, world: &mut dyn BattleWorld, random: &mut dyn Draws) {
    release_with(s, character, &mut NativeWorld { world, strings, naming: Some(naming), random: Some(random) });
}
#[allow(clippy::too_many_arguments)]
pub fn switch_team(s: &mut Store, character: Ref<Character>, target: f64, naming: &mut Naming, strings: &mut Strings, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> bool {
    switch_team_with(s, character, target, &mut NativeWorld { world, strings, naming: Some(naming), random: Some(random) })
}

/// A roster without Classic spawning. Only players consume its capacity;
/// disconnected slots are reused without growing the replicated list.
pub trait PlayersWorld {
    fn player_name(&mut self, name: &[u16]) -> StrId;
    fn new_human(&mut self, players: &mut Players, s: &mut Store, team: f64) -> Ref<Character>;
    fn remove_body(&mut self, players: &mut Players, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>);
}

#[derive(Clone, Debug, Default)]
pub struct Players {
    pub capacity: f64,
    pub reserved: Vec<Ref<Character>>,
}
impl Players {
    pub fn new(s: &mut Store, army_size: Option<f64>, has_ai: bool) -> Self {
        if !has_ai { s.battle.characters.retain(|r| s.character.get(*r).unwrap().human); }
        Self { capacity: army_size.unwrap_or(MAX_TEAM_SIZE), reserved: Vec::new() }
    }
    pub fn humans(&self, s: &Store, team: f64) -> f64 {
        self.reserved.iter().filter(|r| s.character.get(**r).unwrap().team == team).count() as f64
    }
    pub fn full(&self, s: &Store, team: f64) -> bool { self.humans(s, team) >= self.capacity }
    pub fn reserve<W: PlayersWorld + ?Sized>(&mut self, s: &mut Store, team: f64, id: PlayerId, name: &[u16], world: &mut W) -> Result<Ref<Character>, &'static str> {
        if self.full(s, team) { return Err("team full"); }
        let reuse = s.battle.characters.iter().copied().find(|r| {
            let c = s.character.get(*r).unwrap();
            c.human && c.team == 0.0 && !matches!(c.player_id, PlayerId::Undefined) && !self.reserved.contains(r)
        });
        let r = if let Some(r) = reuse {
            let c = ch_mut(s, r);
            c.team = team;
            reset(c);
            c.class_name = None;
            r
        } else { world.new_human(self, s, team) };
        let name = world.player_name(name);
        let c = ch_mut(s, r);
        c.name = Some(name);
        c.player_id = id;
        c.spawn = Some(crate::gen::Spawn { ready: false, timer: f64::INFINITY, post: crate::gen::PostRef::Undefined, ..Default::default() });
        if !self.reserved.contains(&r) { self.reserved.push(r); }
        Ok(r)
    }
    pub fn release<W: PlayersWorld + ?Sized>(&mut self, s: &mut Store, r: Ref<Character>, world: &mut W) {
        if let Some(unit) = s.character.get(r).unwrap().unit { world.remove_body(self, s, r, unit); }
        let c = ch_mut(s, r);
        c.unit = None;
        c.team = 0.0;
        self.reserved.retain(|v| *v != r);
    }
    pub fn switch_team(&self, s: &mut Store, r: Ref<Character>, team: f64) -> bool {
        if s.character.get(r).unwrap().team != team && self.full(s, team) { return false; }
        ch_mut(s, r).team = team;
        true
    }
}
