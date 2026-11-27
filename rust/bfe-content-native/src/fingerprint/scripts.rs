use bfe_content::Value;
use std::collections::BTreeSet;

/// Map metadata already covers the ordered requests. Also hash the files a
/// prepared 2005 map can execute, using only paths within its family root.
pub(super) fn paths(world: &Value, map: &str) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    let prefix = format!("maps/{map}/scripts/");
    if let Some(missions) = world.get("missions").and_then(Value::as_object) {
        for (name, mission) in missions.entries() {
            if mission.get("scripts").is_none() && mission.get("scriptManifest").is_none() {
                continue;
            }
            let manifest = mission.get("scriptManifest").and_then(Value::as_str).unwrap_or("");
            let stem = manifest.strip_prefix(&prefix).and_then(|s| s.strip_suffix(".json")).unwrap_or("");
            if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
                return Err(format!("invalid script manifest path for {name}"));
            }
            paths.insert(format!("/{manifest}"));
            let records = mission.get("scripts").and_then(Value::as_array)
                .ok_or_else(|| format!("invalid script records for {name}"))?;
            for record in records {
                if matches!(record.get("file"), Some(Value::Null)) && record.get("error").and_then(Value::as_str).is_some() {
                    continue;
                }
                let hash = record.get("sha256").and_then(Value::as_str).unwrap_or("");
                if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                    return Err(format!("invalid script digest for {name}"));
                }
                let path = format!("scripts/{hash}.luac");
                if record.get("file").and_then(Value::as_str) != Some(&path) {
                    return Err(format!("invalid script body path for {name}"));
                }
                paths.insert(format!("/{path}"));
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::super::{digest, hash, manifest};
    use bfe_content::files::MemFiles;
    use serde_json::json;

    const BODY: &[u8] = include_bytes!("../../../../tests/fixtures/mission-scripts/bobbin.luac");

    #[test]
    fn hashes_manifests_and_distinct_compiled_bodies() {
        let mut files = MemFiles::default();
        let sha = digest(BODY);
        let path = format!("scripts/{sha}.luac");
        let doc = "maps/spool/scripts/cw-weave.json";
        let row = json!({"name":"Bobbin","file":path,"length":BODY.len(),"sha256":sha});
        let chain = json!({"scriptManifest":doc, "scripts":[row, row, {"file":null, "error":"absent"}]});
        let mut world = json!({"family":"swbf2", "missions":{"work":chain}});
        let map = "maps/spool/map.json";
        let query = "map=spool";
        files.insert(map, world.to_string().into_bytes());
        files.insert(&path, BODY.to_vec());
        files.insert(doc, br#"{"version":1}"#.to_vec());
        let rows = manifest(&files, query).unwrap();
        assert_eq!(rows.iter().filter(|(k, _)| k.contains("/scripts/")).count(), 2);
        assert!(rows.contains(&(format!("/{path}"), Some(sha))));
        let intact = hash(&files, query).unwrap();
        files.insert(&path, include_bytes!("../../../../tests/fixtures/mission-scripts/weft.luac").to_vec());
        assert_ne!(hash(&files, query).unwrap(), intact);
        files.insert(&path, BODY.to_vec());
        files.insert(doc, br#"{"version":2}"#.to_vec());
        assert_ne!(hash(&files, query).unwrap(), intact);
        files.files.remove(&path);
        assert!(manifest(&files, query).unwrap().contains(&(format!("/{path}"), Some("missing".into()))));
        world["missions"]["work"]["scripts"][0]["file"] = json!("../other");
        files.insert(map, world.to_string().into_bytes());
        assert!(manifest(&files, query).unwrap_err().contains("script body path"));
        world["family"] = json!("swbf1");
        files.insert(map, world.to_string().into_bytes());
        assert!(!manifest(&files, query).unwrap().iter().any(|(p, _)| p.contains("/scripts/")));
    }
}
