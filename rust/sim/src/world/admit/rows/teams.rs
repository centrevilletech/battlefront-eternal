//! missionTeams' two cold reads: map team names before Battle, then the
//! native roster after AI construction. The no-AI reader uses the authored
//! mission program, preserving calls and scalar tags in source order.
use super::super::{AdmitError, Stage};
use bfe_content::{battle::Battle, js::to_number, value::{Object, Value}};
use crate::gen::{AiClassScalar, AiMissionTeamsInput, AiRosterCall, StrId};
use std::rc::Rc;

// The roster scalar is an export argument, outside the generated input
// block. Keeping it beside the record prevents an accidental heap field.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub roster: u32,
    pub input: AiMissionTeamsInput,
}

/// game.js's first missionTeams call always marshals eight names. An empty
/// name is still interned, exactly as the generated string writer does.
pub fn names(battle: &Battle, intern: &mut impl FnMut(&str) -> StrId) -> Input {
    names_from_mission(battle.world.get("mission"), intern)
}
pub fn names_from_mission(mission: Option<&Value>, intern: &mut impl FnMut(&str) -> StrId) -> Input {
    let teams = mission.and_then(|m| m.get("teams"));
    Input { roster: 0, input: AiMissionTeamsInput { calls: Vec::new(), names: (0..8).map(|i| {
        let value = teams.and_then(|t| t.get(&i.to_string())).and_then(|t| t.get("name"));
        intern(value.and_then(Value::as_str).unwrap_or(""))
    }).collect() } }
}

/// Both Classic and Enhanced createNativeAI own a Spawning roster. Only
/// the off-AI branch transports the script again through missionTeams.
pub fn after_ai(battle: &Battle, has_ai: bool, intern: &mut impl FnMut(&str) -> StrId) -> Result<Input, AdmitError> {
    // An initialized roster already holds mission relations and factions;
    // sending its script again would create an independent team table.
    let calls = if has_ai { Vec::new() } else { mission_calls(battle.ai_mission.as_ref(), battle.world.get("mission"), intern)? };
    Ok(Input { roster: u32::from(has_ai), input: AiMissionTeamsInput { calls, names: Vec::new() } })
}

/// ai-world-load.js and the off-AI missionTeams reader select the same
/// script. A present empty calls array suppresses callsFromMapMission.
pub fn mission_calls(ai_mission: Option<&Value>, mission: Option<&Value>, intern: &mut impl FnMut(&str) -> StrId) -> Result<Vec<AiRosterCall>, AdmitError> {
    let selected = bfe_content::mission::select_ai_mission(ai_mission, mission);
    let held = selected.as_ref().and_then(|m| m.get("mission")).and_then(Value::as_str);
    let calls = held.and_then(|name| selected.as_ref()?.get("scripts")?.get(name)?.get("calls"));
    // Optional chaining and ?? distinguish a missing/null script from a
    // deliberately empty program. Do not use truthiness for this choice.
    let fallback;
    let calls = match calls.filter(|v| !matches!(v, Value::Null)) {
        Some(value) => value.as_array().ok_or_else(|| error("mission calls must be an array"))?,
        None => { fallback = calls_from_map(mission); &fallback }
    };
    calls.iter().map(|call| {
        let name = call.get("name").and_then(Value::as_str).ok_or_else(|| error("mission call name must be a string"))?;
        let name = intern(name);
        let args = call.get("args").and_then(Value::as_array).ok_or_else(|| error("mission call arguments must be an array"))?
            .iter().map(|v| scalar(v, intern)).collect::<Result<_, _>>()?;
        Ok(AiRosterCall { name, args,
            faction: call.get("faction").filter(|v| !matches!(v, Value::Null)).map(|v| to_number(Some(v))),
            mask: call.get("mask").filter(|v| !matches!(v, Value::Null)).map(|v| to_number(Some(v))) })
    }).collect()
}
fn error(message: &str) -> AdmitError { AdmitError::Content { stage: Stage::Teams, message: message.into() } }
// Mission arguments retain their JS type. Number conversion belongs to
// the native mission parser; doing it here would erase null and booleans.
fn scalar(value: &Value, intern: &mut impl FnMut(&str) -> StrId) -> Result<AiClassScalar, AdmitError> {
    Ok(match value {
        Value::Null => AiClassScalar::Null,
        Value::Bool(v) => AiClassScalar::Bool(*v),
        Value::Number(v) => AiClassScalar::F64(*v),
        Value::String(v) => AiClassScalar::Str(intern(v)),
        _ => return Err(error("mission argument must be a scalar")),
    })
}

/// Ports callsFromMapMission. Object.entries' numeric-key order comes from
/// content::Value; optional count zero is present, unlike a falsey name.
pub fn calls_from_map(mission: Option<&Value>) -> Vec<Value> {
    let mut calls = Vec::new();
    let mut push = |name: &str, args: Vec<Value>| {
        calls.push(Value::Object(Rc::new(Object::from_written(vec![
            (Rc::from("name"), Value::str(name)), (Rc::from("args"), Value::Array(Rc::new(args))),
        ]))));
    };
    if let Some(teams) = mission.and_then(|m| m.get("teams")).and_then(Value::as_object) {
        for (key, team) in teams.entries() {
            let n = Value::Number(to_number(Some(&Value::str(key))));
            if let Some(name) = team.get("name").filter(|v| v.truthy()) { push("SetTeamName", vec![n.clone(), name.clone()]); }
            for entry in team.get("classes").and_then(Value::as_array).unwrap_or(&[]) {
                push("AddUnitClass", vec![n.clone(), entry.at(0).cloned().unwrap_or(Value::Null), entry.at(1).cloned().unwrap_or(Value::Null)]);
            }
            if let Some(hero) = team.get("hero").filter(|v| v.truthy()) { push("SetHeroClass", vec![n.clone(), hero.clone()]); }
            // Zero counts still produce calls. Only absent/null values
            // suppress these authored overrides of the native defaults.
            for (key, call) in [("units", "SetUnitCount"), ("reinforcements", "SetReinforcementCount")] {
                if let Some(value) = team.get(key).filter(|v| !matches!(v, Value::Null)) { push(call, vec![n.clone(), value.clone()]); }
            }
        }
    }
    // Spawn delay is already an argument array, whereas attacking team is
    // one scalar wrapped as an argument array by callsFromMapMission.
    if let Some(value) = mission.and_then(|m| m.get("spawnDelay")).filter(|v| v.truthy()) {
        push("SetSpawnDelay", value.as_array().unwrap_or(&[]).to_vec());
    }
    if let Some(value) = mission.and_then(|m| m.get("attacker")).filter(|v| !matches!(v, Value::Null)) { push("SetAttackingTeam", vec![value.clone()]); }
    calls
}

/// game.js's pre-Battle reader uses the shared context's string namespace.
/// The roster reader is separate because AI construction runs between them.
pub fn build(cx: &mut super::Cx) -> Result<super::Calls, AdmitError> {
    let battle = cx.battle;
    let mut call = super::Call { args: vec![0.0], row: vec![0.0; AiMissionTeamsInput::WORDS], ..Default::default() };
    let input = names(battle, &mut |text| cx.intern(&mut call, text));
    input.input.put(&mut call.row, &mut call.heap);
    Ok(vec![(Stage::Teams, "ai_mission_teams", call)])
}

/// The sequence calls this after createNativeAI's roster has committed.
/// Both native AI flavours read that roster; the off-AI path replays calls.
pub fn after_build(cx: &mut super::Cx, has_ai: bool) -> Result<super::Calls, AdmitError> {
    let battle = cx.battle;
    let mut call = super::Call { args: vec![u32::from(has_ai) as f64], row: vec![0.0; AiMissionTeamsInput::WORDS], ..Default::default() };
    let input = after_ai(battle, has_ai, &mut |text| cx.intern(&mut call, text))?;
    input.input.put(&mut call.row, &mut call.heap);
    Ok(vec![(Stage::Ai, "ai_mission_teams", call)])
}
