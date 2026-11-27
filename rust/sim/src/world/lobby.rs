//! The battle's score table without its world: what a client that sits the
//! battle out is shown. It reads the same roster, tallies and team counts the
//! in-battle scoreboard does; a player's kills are his kills less twice his
//! team kills, and his points are his command post points.
use crate::battle::spawning::tickets;
use crate::world::owner::World;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LobbyTeam { pub team: u8, pub label: String, pub score: i32, pub limit: u32 }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LobbyRow { pub slot: u16, pub team: u8, pub name: String, pub kills: i32, pub deaths: u32, pub points: i32, pub human: bool }

/// `phase` 0 before play, 1 in play, 2 at the end; `winner` 0 while unset or
/// drawn, else the winning team.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LobbyTable { pub tick: u32, pub phase: u8, pub winner: u8, pub teams: Vec<LobbyTeam>, pub rows: Vec<LobbyRow> }

fn clamp_i32(n: f64) -> i32 { if n.is_finite() { n.clamp(i32::MIN as f64, i32::MAX as f64) as i32 } else { 0 } }
fn text(w: &World, id: Option<crate::store::StrId>, bytes: usize) -> String {
    let Some(id) = id else { return String::new() };
    let mut out = String::new();
    for ch in char::decode_utf16(w.sim.strings.units(id).iter().copied()).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)) {
        if out.len() + ch.len_utf8() > bytes { break; }
        out.push(ch);
    }
    out
}

/// The table of the world's battle at authority tick `tick`: the two armies
/// and every other team a roster row belongs to (the locals of a map), in team
/// order, then at most 256 roster rows in roster order. A team's score is its
/// reinforcement count for the two armies and zero for any other; a row of no
/// team (0) belongs to none of them.
pub fn table(w: &World, tick: u32) -> LobbyTable {
    let store = &w.sim.store;
    let battle = &store.battle;
    let phase = if battle.game_over || battle.finished { 2 } else if tick > 0 { 1 } else { 0 };
    let winner = match battle.loser { Some(loser) if battle.over && (loser == 1.0 || loser == 2.0) => (3.0 - loser) as u8, _ => 0 };
    let mut rows = Vec::with_capacity(battle.characters.len().min(256));
    let mut fielded = [false; 8];
    fielded[1] = true;
    fielded[2] = true;
    for (slot, &reference) in battle.characters.iter().take(256).enumerate() {
        let Some(c) = store.character.get(reference) else { continue };
        let team = if (0.0..=7.0).contains(&c.team) { c.team as u8 } else { 0 };
        fielded[usize::from(team)] = true;
        rows.push(LobbyRow { slot: slot as u16, team, name: text(w, c.name, 32), kills: clamp_i32(c.kills - 2.0 * c.team_kills),
            deaths: if c.deaths.is_finite() { c.deaths.clamp(0.0, u32::MAX as f64) as u32 } else { 0 }, points: clamp_i32(c.captures), human: c.human });
    }
    let mut teams = Vec::with_capacity(4);
    for team in (1u8..=7).filter(|&t| fielded[usize::from(t)]) {
        let label = store.spawning.teams.get(team as usize).map_or_else(String::new, |t| text(w, t.name, 64));
        let score = if team <= 2 { clamp_i32(tickets(store, f64::from(team))) } else { 0 };
        teams.push(LobbyTeam { team, label, score, limit: 0 });
    }
    LobbyTable { tick, phase, winner, teams, rows }
}

/// The body of the sit-out notice record (kind 26, revision 1): the match, the
/// mode it plays and why this client sits it out.
pub fn sit_out_body(match_token: &str, mode: &str, reason: &str) -> serde_json::Value {
    serde_json::json!({"match": match_token, "mode": mode, "reason": reason})
}

/// The body of a lobby board record (kind 27, revision 1): the complete table
/// of one match, each replacing the previous one. `pings` gives a human row's
/// ping by its roster slot; a bot or an unmeasured player has none.
pub fn board_body(match_token: &str, table: &LobbyTable, pings: impl Fn(u16) -> Option<u16>) -> serde_json::Value {
    use serde_json::json;
    let teams: Vec<serde_json::Value> = table.teams.iter().map(|t| json!([t.team, t.label, t.score, t.limit])).collect();
    let rows: Vec<serde_json::Value> = table.rows.iter().map(|r| json!([r.slot, r.team, r.name, r.kills, r.deaths, r.points, r.human,
        if r.human { pings(r.slot) } else { None }])).collect();
    json!({"match": match_token, "tick": table.tick, "phase": table.phase, "winner": table.winner, "teams": teams, "rows": rows})
}

/// The roster slot of a network player's character, which the table's rows
/// are keyed by: how a host joins a row with that player's connection. None
/// while the player has no character on rows 0 to 255.
pub fn player_slot(w: &World, player: u32) -> Option<u16> {
    let handle = w.input.players.pool.get(player as usize)?.character.as_ref()?;
    let reference = crate::battle::f4_core::resolve_character_ref(&w.battle, &w.sim.store, handle)?;
    w.sim.store.battle.characters.iter().take(256).position(|&r| r == reference).map(|slot| slot as u16)
}
