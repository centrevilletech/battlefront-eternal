//! Every battle of every map in a content folder, each side it fields as
//! the side played first: the battles the content dump covers (the same list
//! tools/content-dump.mjs makes).

use crate::json;
use crate::mission::{battles_of, utf16_cmp};
use crate::value::Value;
use std::path::Path;

pub struct Planned {
    pub map: String,
    pub mission: Option<String>,
    pub side: String,
    pub query: String,
}

impl Planned {
    /// The dump's file name: <map>-<mission>-<side>.
    pub fn name(&self) -> String {
        format!("{}-{}-{}", self.map, self.mission.as_deref().unwrap_or("none"), self.side)
    }
}

pub fn plan(root: &Path, only_map: Option<&str>, only_mission: Option<&str>, only_side: Option<&str>) -> Vec<Planned> {
    let mut maps: Vec<String> = std::fs::read_dir(root.join("maps"))
        .map(|dir| dir.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    maps.retain(|m| root.join("maps").join(m).join("map.json").exists());
    maps.sort_by(|a, b| utf16_cmp(a, b));
    let mut out = Vec::new();
    for map in maps {
        if only_map.is_some_and(|m| m != map) {
            continue;
        }
        let Ok(bytes) = std::fs::read(root.join("maps").join(&map).join("map.json")) else { continue };
        let Ok(data) = json::parse_bytes(&bytes) else { continue };
        let list: Vec<(Option<String>, Vec<String>)> = if data.get("missions").map(Value::truthy).unwrap_or(false) {
            battles_of(&data)
                .into_iter()
                .filter(|b| b.playable)
                .map(|b| (Some(b.name), b.sides.as_array().unwrap_or(&[]).iter().filter_map(Value::as_str).map(str::to_string).collect()))
                .collect()
        } else {
            let mission = data.get("mission");
            let name = mission.and_then(|m| m.get("name")).and_then(Value::as_str).map(str::to_string);
            let sides = mission.and_then(|m| m.get("sides")).and_then(Value::as_array).unwrap_or(&[]).iter().filter_map(Value::as_str).map(str::to_string).collect();
            vec![(name, sides)]
        };
        for (mission, sides) in list {
            if only_mission.is_some() && mission.as_deref() != only_mission {
                continue;
            }
            let mut seen: Vec<String> = Vec::new();
            for side in sides {
                if seen.contains(&side) {
                    continue;
                }
                seen.push(side.clone());
                if only_side.is_some_and(|s| s != side) {
                    continue;
                }
                if !root.join("sides").join(&side).join("units.json").exists() {
                    continue;
                }
                let query = match &mission {
                    Some(m) => format!("mission={m}&side={side}"),
                    None => format!("side={side}"),
                };
                out.push(Planned { map: map.clone(), mission: mission.clone(), side, query });
            }
        }
    }
    out
}
