//! The 2004 game's Conquest scoring, countdowns and end sequence.

use super::{advance_lifecycle, begin_end, stats, RulesWorld, NO_POST};
use crate::gen::{Character, Store};
use crate::store::{Nullable, Ref};

/// The living soldiers of a team, heroes excepted: the spawn manager's
/// count, else the Characters' own.
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

/// update(dt). `stats_active(him)`: whether his tallies run
/// this tick (battle.statsActive).
pub fn update<W: RulesWorld + ?Sized>(s: &mut Store, dt: f64, strings: &dyn stats::ContextText<W>, stats_active: impl Fn(Ref<Character>, &Character) -> bool, world: &mut W) {
    if !advance_lifecycle(s, dt, strings, stats_active, world) {
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
        begin_end(s, loser, world);
    }
}

/// score(team): the ValueBleed of each post it holds (its
/// command vehicles' too), plus 1 per neutral post.
pub fn score<W: RulesWorld + ?Sized>(s: &mut Store, team: f64, world: &mut W) -> f64 {
    let (_, owned) = world.owned_summary(s, team);
    owned + world.owned_summary(s, 0.0).0 as f64
}

/// updateTeam(team, dt).
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

/// updateLocals(team): a local team friendly with exactly
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
