//! Stage Ai on the owning world: createNativeAI (ai-world-load.js) piece by
//! piece against the string table each native birth leaves, then the
//! roster's missionTeams, Units.setTeams' relation table and the player's
//! combat settings game.js makes right after (combat.team, remotes, random,
//! each weapon's random), as the host records them before createWorldSim.
use super::super::{teams, units, Call, Calls, Cx};
use super::error;
use crate::gen::*;
use crate::world::admit::{dispatch, sequence, AdmitError, Stage};
use crate::world::owner::World;

fn dispatch_all(world: &mut World, cx: &mut Cx, calls: &mut Calls, batch: Calls) -> Result<(), AdmitError> {
    for (stage, name, call) in batch {
        sequence::args(world, &call)?;
        let answer = dispatch::call(world, stage, name, &call)?;
        dispatch::record(cx, stage, name, answer)?;
        calls.push((stage, name, call));
        cx.refresh_strings(&world.sim.strings);
    }
    Ok(())
}

pub fn run_stage(world: &mut World, cx: &mut Cx) -> Result<Calls, AdmitError> {
    super::observe(world, cx)?;
    let mut calls = super::run_initial(world, cx)?;
    // units.setTeams(backend.prepare('teams', [], {roster: true})).
    let after = teams::after_build(cx, true)?;
    dispatch_all(world, cx, &mut calls, after)?;
    // F3Registry.relations over Units.relation with the roster's table.
    let table = mission_teams(cx)?;
    let count = table.teams.len().max(6);
    let relation = |a: f64, b: f64| -> f64 {
        if a == b { return if a != 0.0 { 1.0 } else { 0.0 }; }
        if a == 0.0 || b == 0.0 { return 0.0; }
        table.teams.get(a as usize).and_then(|team| team.relations.get(b as usize).copied()).unwrap_or(-1.0)
    };
    let rows = units::relations(count, relation)?;
    let (_, name, call) = units::in_context(cx, |_, _| Ok(units::Entry::Relations(rows)))?;
    dispatch_all(world, cx, &mut calls, vec![(Stage::Ai, name, call)])?;
    // game.js: the player's combat after the teams are known.
    let viewpoint = cx.ids.registry.iter().find(|(key, _)| key == "viewpoint").map(|(_, id)| *id)
        .ok_or_else(|| error("viewpoint registry id is not assigned"))?;
    let team = cx.battle.policy.team;
    let weapons = cx.battle.soldier_def.get("weapons").and_then(bfe_content::Value::as_array).map_or(0, |w| w.len());
    let mut player = Vec::new();
    for (field, value) in [(F3CombatField::Team, team), (F3CombatField::Remotes, 1.0), (F3CombatField::Random, 1.0), (F3CombatField::Random, 1.0)] {
        player.push((Stage::Ai, "cmb_set", Call { args: vec![viewpoint, field as u32 as f64, value], ..Default::default() }));
    }
    for index in 0..weapons {
        player.push((Stage::Ai, "cmb_weapon_set", Call { args: vec![viewpoint, index as f64, F3WeaponField::Random as u32 as f64, 1.0], ..Default::default() }));
    }
    dispatch_all(world, cx, &mut calls, player)?;
    Ok(calls)
}

/// The roster's table as ai_mission_teams answered it: a block with the
/// header, the row and its heap.
fn mission_teams(cx: &Cx) -> Result<AiMissionTeams, AdmitError> {
    let words = cx.returned_row("ai_mission_teams").ok_or_else(|| error("missionTeams answered no table"))?;
    let header = crate::store::block::HEADER;
    let stride = *words.get(BlockHeader::STRIDE).ok_or_else(|| error("missionTeams block has no header"))? as usize;
    let start = words[BlockHeader::HEAP] as usize;
    let length = words[BlockHeader::HEAP_WORDS] as usize;
    if words.len() < header + stride || words.len() < start + length { return Err(error("missionTeams block is short")); }
    Ok(AiMissionTeams::take(&words[header..header + stride], &words[start..start + length]))
}
