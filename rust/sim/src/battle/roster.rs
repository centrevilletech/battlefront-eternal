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
use crate::gen::{Character, Nullable, PlayerId, Store};
use crate::store::{Ref, Strings};

/// MAX_TEAM_SIZE (src/battle-limits.js).
pub const MAX_TEAM_SIZE: f64 = 32.0;

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
pub fn retire(s: &mut Store, r: Ref<Character>, world: &mut dyn BattleWorld) {
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
fn bot(s: &mut Store, r: Ref<Character>, naming: &mut Naming, strings: &mut Strings, world: &mut dyn BattleWorld, random: &mut dyn Draws) {
    let team = s.character.get(r).unwrap().team as usize;
    let name = spawning::bot_name(&s.spawning, team, naming, strings);
    let c = ch_mut(s, r);
    c.human = false;
    c.player_id = PlayerId::F64(-1.0);
    c.name = Some(name);
    reset(c);
    c.spawn.as_mut().expect("a Character the spawn manager made").timer = 0.0;
    spawning::activate(s, r, strings, world, random);
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
pub fn create(s: &mut Store, army_size: Option<f64>, local: Option<Ref<Character>>, naming: &mut Naming, strings: &mut Strings, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> Roster {
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
            let name = spawning::bot_name(&s.spawning, number, naming, strings);
            let r = spawning::insert(s, number as f64, false, -1.0, name, strings).expect("room in the team");
            let c = ch_mut(s, r);
            c.index = index;
            c.slot = Some(index);
            spawning::activate(s, r, strings, world, random);
            world.ai_attach(s, r);
        }
        spawning::pad_quotas(&mut s.spawning, number, random);
    }
    Roster { army_size }
}

/// available(team) (multiplayer-roster.js:89): the team's first bot whose
/// body is dead, else its first bot (a stable sort by whether it lives).
fn available(s: &Store, team: f64, world: &mut dyn BattleWorld) -> Option<Ref<Character>> {
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
pub fn reserve(s: &mut Store, team: f64, id: PlayerId, name: &[u16], strings: &mut Strings, world: &mut dyn BattleWorld) -> Result<Ref<Character>, &'static str> {
    let Some(r) = available(s, team, world) else { return Err("team full") };
    retire(s, r, world);
    let name = strings.intern(player_name(name));
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
pub fn release(s: &mut Store, r: Ref<Character>, naming: &mut Naming, strings: &mut Strings, world: &mut dyn BattleWorld, random: &mut dyn Draws) {
    retire(s, r, world);
    bot(s, r, naming, strings, world, random);
}

/// switchTeam(character, target) (multiplayer-roster.js:106): he trades
/// places with a bot of the other team; false when it has none.
#[allow(clippy::too_many_arguments)]
pub fn switch_team(s: &mut Store, r: Ref<Character>, target: f64, naming: &mut Naming, strings: &mut Strings, world: &mut dyn BattleWorld, random: &mut dyn Draws) -> bool {
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
    bot(s, replacement, naming, strings, world, random);
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
