//! Battle and era selection from a world's mission sub-levels (`src/mission.js`).
//!
//! This module wraps `bfe_content::mission` (`battles_of`, `pick_mission`,
//! `select_mission`, `select_ai_mission`, `spawn_policy`) and provides
//! `serde_json::Value` and [`BattleRequest`] helpers so callers in the shell,
//! tests, and native setup can resolve which mission a battle request
//! (`mission`, `era`, `side`) plays.
//!
//! Key rules from `src/mission.js`:
//! - `battles_of` returns every battle on a map in UTF-16 name order, marking
//!   the base `"world"` sub-level `playable = false` and historical campaign
//!   variants (`historical = true`).
//! - `pick_mission` honors an explicit `mission` first (including historical
//!   variants when explicitly named), then `era` (`cw` or `gcw`), then `side`,
//!   and defaults to Clone Wars (`cw`) before other eras, never picking a
//!   historical variant unless explicitly named.

pub use bfe_content::mission::{
    battles_of, era_of_side, pick_mission, playable_classes, select_ai_mission, select_mission,
    side_of_class, spawn_policy, team_of_class, utf16_cmp, BattleInfo, Policy, Query, DEFAULT_ERA,
    DEFAULT_UNIT, ERAS,
};

use crate::launch::BattleRequest;
use bfe_content::value::{Object, Value};
use std::rc::Rc;

/// Converts a `serde_json::Value` into a `bfe_content::Value` so JSON manifests
/// (`map.json` / `world.json` and `ai-mission.json`) loaded by `serde_json` can
/// be queried with `bfe_content::mission` without re-parsing raw JSON text.
/// Numbers that cannot be represented as finite `f64` fall back to `0.0`, and
/// arrays/objects are wrapped in reference-counted `bfe_content` containers.
pub fn to_content_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::str(s),
        serde_json::Value::Array(arr) => {
            Value::Array(Rc::new(arr.iter().map(to_content_value).collect()))
        }
        serde_json::Value::Object(obj) => {
            let mut map = Object::new();
            for (k, v) in obj {
                map.set(Rc::from(k.as_str()), to_content_value(v));
            }
            Value::Object(Rc::new(map))
        }
    }
}

/// Builds a [`Query`] from a [`BattleRequest`]'s non-empty `mission`, `era`,
/// and `side` fields, matching the `reader(query)` key lookup in `src/mission.js`:
/// - `mission=<name>` selects an explicit sub-level (including historical campaigns).
/// - `era=cw` or `era=gcw` selects that era's regular battle on the map.
/// - `side=<id>` selects the era of that army (`rep`/`cis` -> `cw`, `all`/`imp` -> `gcw`)
///   or, for a local faction, the regular battle whose `sides` list includes it.
pub fn query_for_request(request: &BattleRequest) -> Query {
    let mut parts = Vec::new();
    if let Some(m) = request.mission.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("mission={m}"));
    }
    if let Some(e) = request.era.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("era={e}"));
    }
    if let Some(s) = request.side.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("side={s}"));
    }
    Query::parse(&parts.join("&"))
}

/// Lists the battles on a map from a `serde_json::Value` (`world.json` / `map.json`),
/// sorted in UTF-16 code-unit order of sub-level name with `playable = !mission.world`.
/// Mirrors `battlesOf(world)` in `src/mission.js`.
pub fn battles_of_json(world: &serde_json::Value) -> Vec<BattleInfo> {
    let val = to_content_value(world);
    battles_of(&val)
}

/// Picks which mission sub-level a [`BattleRequest`] plays on `world`, ordering
/// `DEFAULT_ERA` (`"cw"`) ahead of other eras when no explicit override matches.
/// Mirrors `pickMission(world, query)` in `src/mission.js`.
pub fn pick_mission_for_request(world: &Value, request: &BattleRequest) -> Option<String> {
    let query = query_for_request(request);
    pick_mission(world, &query)
}

/// Picks which mission sub-level a [`Query`] plays on a `serde_json::Value` world,
/// returning `world.mission.name` for single-mission legacy imports without `missions`.
/// Mirrors `pickMission(world, query)` in `src/mission.js`.
pub fn pick_mission_json(world: &serde_json::Value, query: &Query) -> Option<String> {
    let val = to_content_value(world);
    pick_mission(&val, query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_world() -> serde_json::Value {
        json!({
            "name": "bes2",
            "missions": {
                "world": {
                    "mission": { "world": true }
                },
                "bes2a": {
                    "mission": {
                        "era": "gcw",
                        "historical": false,
                        "sides": ["all", "imp"]
                    }
                },
                "bes2a_h": {
                    "mission": {
                        "era": "gcw",
                        "historical": true,
                        "sides": ["all", "imp"]
                    }
                },
                "bes2r": {
                    "mission": {
                        "era": "cw",
                        "historical": false,
                        "sides": ["rep", "cis"]
                    }
                }
            }
        })
    }

    #[test]
    fn battles_of_marks_world_unplayable_and_identifies_historical_variants() {
        let world = sample_world();
        let playable: Vec<BattleInfo> = battles_of_json(&world)
            .into_iter()
            .filter(|b| b.playable && !b.historical)
            .collect();
        assert_eq!(playable.len(), 2);
        assert_eq!(playable[0].name, "bes2a");
        assert_eq!(playable[0].era.as_ref().and_then(Value::as_str), Some("gcw"));
        assert_eq!(playable[1].name, "bes2r");
        assert_eq!(playable[1].era.as_ref().and_then(Value::as_str), Some("cw"));
    }

    #[test]
    fn pick_mission_defaults_to_clone_wars_and_never_picks_historical_unless_named()
    -> Result<(), Box<dyn std::error::Error>> {
        let world = to_content_value(&sample_world());

        // Default query chooses Clone Wars (bes2r).
        assert_eq!(
            pick_mission(&world, &Query::default()).as_deref(),
            Some("bes2r")
        );

        // Explicit era=gcw chooses bes2a, skipping bes2a_h.
        assert_eq!(
            pick_mission(&world, &Query::parse("era=gcw")).as_deref(),
            Some("bes2a")
        );

        // Explicit side=imp chooses the GCW battle where imp is present.
        assert_eq!(
            pick_mission(&world, &Query::parse("side=imp")).as_deref(),
            Some("bes2a")
        );

        // Explicit mission=bes2a_h honors the named historical mission.
        assert_eq!(
            pick_mission(&world, &Query::parse("mission=bes2a_h")).as_deref(),
            Some("bes2a_h")
        );

        // BattleRequest helper resolves the same mission.
        let mut req = BattleRequest::single("bes2", Some("gcw"))?;
        assert_eq!(
            pick_mission_for_request(&world, &req).as_deref(),
            Some("bes2a")
        );
        req.mission = Some("bes2a_h".to_string());
        assert_eq!(
            pick_mission_for_request(&world, &req).as_deref(),
            Some("bes2a_h")
        );
        Ok(())
    }
}
