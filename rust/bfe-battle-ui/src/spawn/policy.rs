//! Startup spawn policy and URL/parameter resolution (`src/spawn-screen/policy.js`).
//!
//! Decides whether a battle opens on the Classic spawn screen or bypasses it into
//! an immediate spawn (`unit=`, `side=`, `spawn=`, `spawnscreen=0|1`), and picks
//! the initial faction side, team number (`1` or `2`), and soldier class name so
//! the first character body can be preloaded before the world starts ticking.

use crate::shapes::GameFamily;

/// Historical default unit class when no mission or URL override names one
/// (`src/spawn-screen/policy.js` `DEFAULT_UNIT`).
pub const DEFAULT_UNIT: &str = "rep_inf_clone_trooper";

/// A team's roster definition from `map.json` (`teams[1]`, `teams[2]`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissionTeamSpec {
    pub name: String,
    pub hero: Option<String>,
    pub classes: Vec<String>,
}

/// Mission roster metadata read by [`spawn_policy`] (`src/spawn-screen/policy.js`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissionSpec {
    pub family: GameFamily,
    pub sides: Vec<String>,
    pub team1: Option<MissionTeamSpec>,
    pub team2: Option<MissionTeamSpec>,
}

impl MissionSpec {
    /// Returns the team specification for team `1` or `2`.
    #[inline]
    pub fn team(&self, team: u8) -> Option<&MissionTeamSpec> {
        match team {
            1 => self.team1.as_ref(),
            2 => self.team2.as_ref(),
            _ => None,
        }
    }
}

/// Immediate auto-spawn choice when the spawn screen is bypassed (`auto` in
/// `src/spawn-screen/policy.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoSpawnPick {
    pub team: u8,
    pub class_name: String,
    pub post: Option<String>,
}

/// Resolved startup spawn policy (`src/spawn-screen/policy.js` `spawnPolicy`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnPolicy {
    /// Whether the spawn screen opens at the start of the battle.
    pub screen: bool,
    /// Faction prefix to load first (`"rep"`, `"cis"`, `"all"`, `"imp"`, `"gar"`, etc.).
    pub side: String,
    /// Team number (`1` or `2`) owning `class_name`.
    pub team: u8,
    /// Initial soldier class name.
    pub class_name: String,
    /// Optional command post name from `spawn=`.
    pub post: Option<String>,
    /// Present when `screen == false` so `HumanSpawn` can spawn immediately.
    pub auto: Option<AutoSpawnPick>,
}

/// Parsed URL query or parameter bag passed to [`spawn_policy`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnParams {
    pub unit: Option<String>,
    pub side: Option<String>,
    pub spawn: Option<String>,
    pub spawnscreen: Option<String>,
    pub network: Option<String>,
}

impl SpawnParams {
    /// Parses a URL or query string (`"?map=nab2&side=rep&unit=..."` or `"unit=..."`)
    /// into [`SpawnParams`].
    pub fn from_query(query: &str) -> Self {
        let mut out = Self::default();
        let q = query
            .split_once('?')
            .map_or(query, |(_, tail)| tail)
            .trim_start_matches('?');
        for pair in q.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let val = (!v.is_empty()).then(|| v.to_string());
            match k {
                "unit" => out.unit = val,
                "side" => out.side = val,
                "spawn" => out.spawn = val,
                "spawnscreen" => out.spawnscreen = val,
                "network" => out.network = val,
                _ => {}
            }
        }
        out
    }
}

/// Returns the non-hero playable class names of a team in roster order
/// (`src/spawn-screen/policy.js` `playableClasses`).
///
/// The hero unit (`rep_inf_macewindu`, `all_inf_lukeskywalkersnow`, etc.) is
/// never selectable on the spawn screen even when listed in `classes`.
pub fn playable_classes(team: Option<&MissionTeamSpec>) -> Vec<String> {
    let Some(team) = team else {
        return Vec::new();
    };
    let hero = team.hero.as_deref();
    team.classes
        .iter()
        .filter(|name| !name.is_empty() && Some(name.as_str()) != hero)
        .cloned()
        .collect()
}

/// Finds which team (`1` or `2`) owns `class_name`, including team heroes;
/// falls back to team `1` when neither team lists the class
/// (`src/spawn-screen/policy.js` `teamOfClass`).
pub fn team_of_class(mission: Option<&MissionSpec>, class_name: &str) -> u8 {
    let Some(mission) = mission else {
        return 1;
    };
    for t in [1u8, 2u8] {
        let Some(team) = mission.team(t) else {
            continue;
        };
        if team.hero.as_deref() == Some(class_name)
            || playable_classes(Some(team)).iter().any(|c| c == class_name)
        {
            return t;
        }
    }
    1
}

/// Returns the faction prefix (`"rep"`, `"cis"`, `"all"`, `"imp"`, etc.) before
/// the first `'_'` in `class_name`.
#[inline]
fn class_prefix(class_name: &str) -> &str {
    class_name
        .split_once('_')
        .map_or(class_name, |(prefix, _)| prefix)
}

/// Resolves the faction side (`"rep"`, `"cis"`, `"all"`, `"imp"`, etc.) for
/// `class_name` in `mission` (`src/spawn-screen/policy.js` `sideOfClass`).
pub fn side_of_class(mission: Option<&MissionSpec>, class_name: &str) -> String {
    let prefix = class_prefix(class_name);
    let Some(mission) = mission else {
        return if prefix.is_empty() {
            "rep".to_string()
        } else {
            prefix.to_string()
        };
    };
    for t in [1u8, 2u8] {
        let Some(team) = mission.team(t) else {
            continue;
        };
        if team.hero.as_deref() == Some(class_name)
            || playable_classes(Some(team)).iter().any(|c| c == class_name)
        {
            return prefix.to_string();
        }
    }
    if mission.sides.iter().any(|s| s == prefix) {
        return prefix.to_string();
    }
    mission
        .sides
        .first()
        .cloned()
        .unwrap_or_else(|| "rep".to_string())
}

/// Returns whether `mission` actually includes faction `side` on either team's
/// playable roster or hero.
fn mission_has_side(mission: Option<&MissionSpec>, side: &str) -> bool {
    let Some(mission) = mission else {
        return false;
    };
    let prefix = format!("{side}_");
    for t in [1u8, 2u8] {
        let Some(team) = mission.team(t) else {
            continue;
        };
        if playable_classes(Some(team))
            .iter()
            .any(|c| c.starts_with(&prefix))
            || team
                .hero
                .as_deref()
                .is_some_and(|h| h.starts_with(&prefix))
        {
            return true;
        }
    }
    false
}

#[inline]
fn non_empty(opt: &Option<String>) -> Option<&str> {
    opt.as_deref().filter(|s| !s.is_empty())
}

/// Resolves the startup spawn policy from URL query string `query` and `mission`
/// (`src/spawn-screen/policy.js` `spawnPolicy`).
pub fn spawn_policy_from_query(query: &str, mission: Option<&MissionSpec>) -> SpawnPolicy {
    spawn_policy(&SpawnParams::from_query(query), mission)
}

/// Resolves the startup spawn policy from structured [`SpawnParams`] and `mission`
/// (`src/spawn-screen/policy.js` `spawnPolicy`).
///
/// Rules preserved from JavaScript:
/// - In network games (`network=1`), stale `unit=` and `spawn=` query parameters
///   are ignored so the client always opens the spawn screen unless `spawnscreen=0`
///   is explicitly passed.
/// - `side=` picks that faction's first non-hero class in the mission; if the
///   mission lacks that faction, offline play keeps `DEFAULT_UNIT` and `side=`,
///   whereas online play falls back to the mission's default class and its own side.
/// - Without overrides, the screen opens on `DEFAULT_UNIT` if either team has it
///   (e.g. Republic on Theed where CIS is team 1 and Republic is team 2), or else
///   team 1's first playable class (e.g. Empire on Hoth).
pub fn spawn_policy(params: &SpawnParams, mission: Option<&MissionSpec>) -> SpawnPolicy {
    let online = matches!(non_empty(&params.network), Some("1" | "true"));
    let unit = if online {
        None
    } else {
        non_empty(&params.unit)
    };
    let side_param = non_empty(&params.side);
    let post = if online {
        None
    } else {
        non_empty(&params.spawn).map(str::to_string)
    };
    let forced = match non_empty(&params.spawnscreen) {
        Some("1" | "true") => Some(true),
        Some("0" | "false") => Some(false),
        _ => None,
    };
    let screen =
        forced.unwrap_or_else(|| online || (unit.is_none() && side_param.is_none() && post.is_none()));

    let first_of_side = |s: &str| -> Option<String> {
        let m = mission?;
        let prefix = format!("{s}_");
        for t in [1u8, 2u8] {
            for c in playable_classes(m.team(t)) {
                if c.starts_with(&prefix) {
                    return Some(c);
                }
            }
        }
        None
    };

    let default_class = || -> String {
        if !screen {
            return DEFAULT_UNIT.to_string();
        }
        let Some(m) = mission else {
            return DEFAULT_UNIT.to_string();
        };
        let t1 = playable_classes(m.team(1));
        let t2 = playable_classes(m.team(2));
        if t1.iter().any(|c| c == DEFAULT_UNIT) || t2.iter().any(|c| c == DEFAULT_UNIT) {
            return DEFAULT_UNIT.to_string();
        }
        t1.into_iter()
            .next()
            .or_else(|| t2.into_iter().next())
            .unwrap_or_else(|| DEFAULT_UNIT.to_string())
    };

    let class_name = if let Some(u) = unit {
        u.to_string()
    } else if let Some(sp) = side_param {
        if let Some(found) = first_of_side(sp) {
            found
        } else if !online {
            DEFAULT_UNIT.to_string()
        } else {
            default_class()
        }
    } else {
        default_class()
    };

    let team = team_of_class(mission, &class_name);
    let side = if let Some(sp) = side_param {
        if !online || mission_has_side(mission, sp) {
            sp.to_string()
        } else {
            side_of_class(mission, &class_name)
        }
    } else {
        side_of_class(mission, &class_name)
    };

    let auto = (!screen).then(|| AutoSpawnPick {
        team,
        class_name: class_name.clone(),
        post: post.clone(),
    });

    SpawnPolicy {
        screen,
        side,
        team,
        class_name,
        post,
        auto,
    }
}
