//! The battle's rules (src/battle.js): who fights for each team (its
//! Characters), their reinforcements, the bleed of reinforcements from
//! holding fewer posts, how a team loses, and each Character's scores.
//! Teams 1 and 2 are the armies; 3 to 7 the locals, who neither bleed nor
//! count down.
//!
//! The state is the store's (rust/sim/schema/battle.mjs): the `battle`
//! singleton and its Characters, by reference.

use super::posts;
use super::stats::{self, Seen};
use super::world::BattleWorld;
use crate::gen::{Battle, BattleStats, Character, KillRing, Store, TeamDesc};
use crate::store::{IntObj, Nullable, Ref, Strings, TypedArray};

/// GAME_OVER_DELAY: s from a team's last reinforcement to the game-over flag.
pub const GAME_OVER_DELAY: f64 = 2.0;
/// END_SEQUENCE: s from the game-over flag to the end-of-match screens.
pub const END_SEQUENCE: f64 = 7.0;
/// The no-post countdown, s.
pub const NO_POST: f64 = 20.0;

/// The services used by the rules and tallies. Native whole-battle hosts
/// use BattleWorld below; the live module can also call these rules while
/// the command posts and bodies still belong to JavaScript.
pub trait RulesWorld {
    fn stats_active(&mut self, _s: &Store, _r: Ref<Character>, _c: &Character, active: bool) -> bool { active }

    /// Owned worlds move prepared storage into a lazy record at this slot.
    /// Capture hosts retain recordOf's ordinary first-use construction.
    fn prepare_record(&mut self, _s: &mut Store, _character: Ref<Character>) {}
    fn relation(&mut self, a: f64, b: f64) -> f64;
    fn unit_alive(&mut self, s: &Store, unit: Ref<crate::gen::Unit>) -> bool;
    fn observe(&mut self, s: &Store, character: Ref<Character>, unit: Option<Ref<crate::gen::Unit>>) -> Option<Seen>;
    fn living(&mut self, s: &Store, team: f64) -> Option<f64>;
    fn owned_values(&mut self, s: &mut Store, team: f64) -> Vec<Option<f64>>;
    fn battle_over(&mut self, s: &mut Store, loser: f64);
    /// Native owners can sum their placed/mobile posts directly. The legacy
    /// foreign host still supplies its value list while its callers migrate.
    fn owned_summary(&mut self, s: &mut Store, team: f64) -> (usize, f64) {
        let values = self.owned_values(s, team);
        let mut sum = 0.0;
        for value in &values { sum += value.unwrap_or(10.0); }
        (values.len(), sum)
    }
}

impl<T: BattleWorld + ?Sized> RulesWorld for T {
    fn relation(&mut self, a: f64, b: f64) -> f64 { BattleWorld::relation(self, a, b) }
    fn unit_alive(&mut self, _s: &Store, unit: Ref<crate::gen::Unit>) -> bool { BattleWorld::unit_alive(self, unit) }
    fn observe(&mut self, _s: &Store, character: Ref<Character>, unit: Option<Ref<crate::gen::Unit>>) -> Option<Seen> { BattleWorld::observe(self, character, unit) }
    fn living(&mut self, _: &Store, team: f64) -> Option<f64> { BattleWorld::living(self, team) }
    fn owned_values(&mut self, s: &mut Store, team: f64) -> Vec<Option<f64>> {
        posts::owned(s, team, self).into_iter().map(|post| posts::value_bleed(s, post)).collect()
    }
    fn battle_over(&mut self, _: &mut Store, loser: f64) { BattleWorld::battle_over(self, loser); }
}

/// new Character(index, team, human) (battle.js:52): his name a human's
/// ("Player 1"), else none until his roster names him.
pub fn new_character(index: f64, team: f64, human: bool, strings: &mut Strings) -> Character {
    Character {
        index,
        team,
        human,
        name: human.then(|| strings.intern_str(&super::names::human_name(0.0))),
        out_of_bounds: -1.0,
        ..Default::default()
    }
}

/// new BattleStats() (battle-stats.js:123): an empty ring.
pub fn new_stats() -> BattleStats {
    BattleStats { time: 0.0, ring: KillRing { killers: vec![None; super::stats::RING_SIZE], times: TypedArray::default(), slots: TypedArray::default(), head: 0.0, tail: 0.0 } }
}

/// new Battle({ teams, posts, own, foe }) (battle.js:92), its Characters
/// none yet.
pub fn new_battle(teams: IntObj<TeamDesc>, own: f64, foe: f64) -> Battle {
    let reinforcements: IntObj<f64> = teams.iter().map(|(t, v)| (t, v.reinforcements.unwrap_or(if t >= 3.0 { 2147483647.0 } else { 0.0 }))).collect();
    let team_ids: Vec<f64> = teams.keys().collect();
    let no_post = team_ids.iter().map(|t| (*t, NO_POST)).collect();
    let bleed_rules = teams
        .iter()
        .map(|(t, v)| {
            let mut rules = v.bleed.clone().unwrap_or_default();
            // [...rules].sort((a, b) => a[0] - b[0]): a stable sort.
            jsrt::sort::js_sort(&mut rules, &mut |a: &[f64; 2], b: &[f64; 2]| a[0] - b[0]);
            (t, rules)
        })
        .collect();
    Battle {
        own,
        foe,
        start: reinforcements.clone(),
        reinforcements,
        team_ids,
        no_post,
        victory_clock: Nullable::Null,
        countdown: Nullable::Null,
        loser: None,
        bleed_rules,
        teams,
        stats: new_stats(),
        ..Default::default()
    }
}

/// spend(team, count) (battle.js:159): a death costs the team a
/// reinforcement.
pub fn spend(battle: &mut Battle, team: f64, count: f64) {
    let left = battle.reinforcements.at(team).unwrap_or(count);
    battle.reinforcements.set(team, v8math::max(left - count, 0.0));
}

/// credit(victim, killer, relation) (battle.js:170).
pub fn credit(s: &mut Store, victim: Option<Ref<Character>>, killer: Option<Ref<Character>>, relation: f64) {
    if s.battle.game_over {
        return;
    }
    if let Some(k) = killer {
        if relation != 0.0 {
            let c = s.character.get_mut(k).expect("a killer");
            c.kills += 1.0;
            if relation > 0.0 {
                c.team_kills += 1.0;
            }
        }
    }
    stats::kill(&mut s.battle.stats, &mut s.character, killer, victim, relation);
    if let Some(v) = victim {
        s.character.get_mut(v).expect("a victim").deaths += 1.0;
    }
    stats::death(&mut s.character, victim);
}

/// headshot(credited) (battle.js:181).
pub fn headshot(s: &mut Store, credited: Option<Ref<Character>>) {
    if !s.battle.game_over {
        stats::headshot(&mut s.character, credited);
    }
}

/// vehicleDestroyed(destroyer) (battle.js:186).
pub fn vehicle_destroyed(s: &mut Store, destroyer: Option<Ref<Character>>) {
    if !s.battle.game_over {
        stats::vehicle_destroyed(&mut s.character, destroyer);
    }
}

/// point(character) (battle.js:192): a command post point.
pub fn point(s: &mut Store, character: Option<Ref<Character>>) {
    let Some(r) = character else { return };
    if s.battle.game_over {
        return;
    }
    let c = s.character.get_mut(r).expect("a Character");
    c.captures = ((jsrt::conv::to_int32(c.captures + 1.0)) & 0xffff) as f64;
}

/// postDestroyed(destroyer, relation) (battle.js:201).
pub fn post_destroyed(s: &mut Store, destroyer: Option<Ref<Character>>, relation: f64) {
    if relation < 1.0 {
        point(s, destroyer);
    }
}

/// The living soldiers of a team, heroes excepted: the spawn manager's
/// count, else the Characters' own (battle.js:111).
pub fn living<W: RulesWorld + ?Sized>(s: &Store, team: f64, world: &mut W) -> f64 {
    if let Some(n) = world.living(s, team) {
        return n;
    }
    let mut n = 0.0;
    for &r in &s.battle.characters {
        let c = s.character.get(r).expect("a Character of the battle");
        if c.team == team && c.hero != Some(true) && c.unit.is_some_and(|u| world.unit_alive(s, u)) {
            n += 1.0;
        }
    }
    n
}

/// update(dt) (battle.js:205). `stats_active(him)`: whether his tallies run
/// this tick (battle.statsActive).
pub fn update<W: RulesWorld + ?Sized>(s: &mut Store, dt: f64, strings: &dyn stats::Text, stats_active: impl Fn(Ref<Character>, &Character) -> bool, world: &mut W) {
    if !s.battle.game_over {
        s.battle.stats.time += dt;
        for i in 0..s.battle.characters.len() {
            let r = s.battle.characters[i];
            let c = s.character.get(r).expect("a Character of the battle");
            if !world.stats_active(s, r, c, stats_active(r, c)) { continue; }
            world.prepare_record(s, r);
            let c = s.character.get_mut(r).expect("active statistics Character");
            stats::record_of(c);
            let unit = c.unit;
            let visible = stats::truthy(strings, c.class_name) || unit.is_some();
            let seen = if visible { world.observe(s, r, unit) } else { None };
            stats::tick_character(dt, s.character.get_mut(r).unwrap(), strings, seen);
        }
    }
    let b = &mut s.battle;
    if b.over {
        b.over_time += dt;
        if b.over_time >= GAME_OVER_DELAY {
            b.game_over = true;
        }
        if b.over_time >= GAME_OVER_DELAY + END_SEQUENCE {
            b.finished = true;
        }
        return;
    }
    if s.command_posts.posts.is_empty() {
        return;
    }
    for i in 0..s.battle.team_ids.len() {
        let team = s.battle.team_ids[i];
        update_team(s, team, dt, world);
    }
    // What the HUD shows: the main team counting down with no posts.
    let mut stranded = None;
    for team in [s.battle.own, s.battle.foe] {
        let score = score(s, team, world);
        if score == 0.0 || score.is_nan() {
            stranded = Some(team);
            break;
        }
    }
    let b = &mut s.battle;
    b.victory_clock = match stranded {
        // `stranded ? this.noPost[stranded] : null` (no team is 0).
        Some(t) if t != 0.0 && !t.is_nan() => match b.no_post.at(t) {
            Some(v) => Nullable::Value(v),
            None => Nullable::Undefined,
        },
        _ => Nullable::Null,
    };
    b.countdown = b.victory_clock;
    b.losing = stranded == Some(b.own);
    let loser = [b.foe, b.own].into_iter().find(|t| b.reinforcements.at(*t).unwrap_or(1.0) <= 0.0);
    if let Some(loser) = loser.filter(|t| *t != 0.0 && !t.is_nan()) {
        b.over = true;
        b.loser = Some(loser);
        b.countdown = Nullable::Null;
        world.battle_over(s, loser);
    }
}

/// score(team) (battle.js:237): the ValueBleed of each post it holds (its
/// command vehicles' too), plus 1 per neutral post.
pub fn score<W: RulesWorld + ?Sized>(s: &mut Store, team: f64, world: &mut W) -> f64 {
    let (_, owned) = world.owned_summary(s, team);
    owned + world.owned_summary(s, 0.0).0 as f64
}

/// updateTeam(team, dt) (battle.js:242).
pub fn update_team<W: RulesWorld + ?Sized>(s: &mut Store, team: f64, dt: f64, world: &mut W) {
    if team > 2.0 {
        update_locals(s, team, world);
        return;
    }
    let owned = world.owned_summary(s, team).0;
    let score = score(s, team, world);
    let b = &mut s.battle;
    let rate = b.bleed_rules.get(team).filter(|r| !r.is_empty())
        .map(|rules| rules.iter().find(|r| score <= r[0]).map(|r| r[1]).unwrap_or(0.0));
    if let Some(rate) = rate {
        b.bleed.set(team, rate);
        // `(this.bleedClock[team] || 0) + this.bleed[team] * dt`
        let clock = b.bleed_clock.at(team).filter(|c| *c != 0.0 && !c.is_nan()).unwrap_or(0.0);
        b.bleed_clock.set(team, clock + rate * dt);
        if b.bleed_clock.at(team).unwrap() >= 4.0 {
            *b.bleed_clock.get_mut(team).unwrap() -= 4.0;
            if b.reinforcements.at(team).is_some_and(|r| r > 0.0) {
                *b.reinforcements.get_mut(team).unwrap() -= 1.0;
            }
        }
    }
    if score != 0.0 && !score.is_nan() {
        if owned > 0 {
            b.no_post.set(team, NO_POST);
        }
    } else {
        let left = b.no_post.at(team).unwrap_or(f64::NAN) - dt;
        b.no_post.set(team, left);
        if left <= 0.0 {
            b.reinforcements.set(team, 0.0);
            b.start.set(team, 0.0);
        }
    }
}

/// updateLocals(team) (battle.js:268): a local team friendly with exactly
/// one army follows it out of the battle.
pub fn update_locals<W: RulesWorld + ?Sized>(s: &mut Store, team: f64, world: &mut W) {
    let mut ally = 0.0;
    for army in [1.0, 2.0] {
        if world.relation(team, army) > 0.0 {
            ally = if ally != 0.0 { -1.0 } else { army };
        }
    }
    if ally > 0.0 && s.battle.teams.get(ally).is_some() && s.battle.reinforcements.at(ally).unwrap_or(0.0) <= living(s, ally, world) {
        s.battle.reinforcements.set(team, 0.0);
    }
}
