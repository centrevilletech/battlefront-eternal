//! validateStartupClass's cold roster input. Keep every authored class,
//! including heroes: validation must reject an unavailable URL choice rather
//! than silently substitute a class from the spawn screen.
use super::super::{AdmitError, Stage};
use bfe_content::{battle::Battle, mission::Policy, value::Value};
use crate::gen::{F4RosterClasses, F4StartupClass, StrId};

/// The owning admission's interner supplies ids shared with later owners.
/// Its order follows putF4StartupClass: chosen name, then rosters 0, 1, 2.
pub fn input(battle: &Battle, intern: &mut impl FnMut(&str) -> StrId) -> Result<F4StartupClass, AdmitError> {
    from_policy(&battle.policy, battle.world.get("mission"), intern)
}

// policy.team is passed unchanged, including an invalid number: the native
// startup owner is the membership authority. Row construction only rejects
// a malformed authored roster that the generated string writer cannot read.
pub fn from_policy(policy: &Policy, mission: Option<&Value>, intern: &mut impl FnMut(&str) -> StrId) -> Result<F4StartupClass, AdmitError> {
    let class_name = intern(&policy.class_name);
    let mut rosters = Vec::with_capacity(3);
    for team in 0..3 {
        let source = mission.and_then(|m| m.get("teams")).and_then(|t| t.get(&team.to_string())).and_then(|t| t.get("classes"));
        let mut classes = Vec::new();
        for entry in source.and_then(Value::as_array).unwrap_or(&[]) {
            let name = entry.at(0).and_then(Value::as_str).ok_or_else(|| AdmitError::Content {
                stage: Stage::Startup, message: format!("team {team} has a class without a string name"),
            })?;
            classes.push(intern(name));
        }
        rosters.push(F4RosterClasses { classes });
    }
    Ok(F4StartupClass { class_name, team: policy.team, rosters })
}

/// validateStartupClass uses the same table as every later owner. The
/// generated writer's string visit order is retained on this one call.
pub fn build(cx: &mut super::Cx) -> Result<super::Calls, AdmitError> {
    let battle = cx.battle;
    let mut call = super::Call { row: vec![0.0; F4StartupClass::WORDS], ..Default::default() };
    let input = input(battle, &mut |text| cx.intern(&mut call, text))?;
    input.put(&mut call.row, &mut call.heap);
    Ok(vec![(Stage::Startup, "f4_startup_class", call)])
}
