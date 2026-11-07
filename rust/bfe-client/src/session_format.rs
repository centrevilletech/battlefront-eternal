//! Current recordings use one retained battle-frame contract. Superseded
//! archives are rejected before content or resource-command admission.
use crate::render_bridge::Command;
use serde_json::Value;
pub fn admit(meta: &Value) -> Result<(), String> {
    if meta["version"].as_u64() != Some(2) {
        return Err("unsupported native session version; record a version-2 archive".into());
    }
    let renderer = meta["renderer"]
        .as_u64()
        .ok_or("missing session renderer")?;
    let calls = meta["calls"].as_array().ok_or("missing session calls")?;
    let mut battles = 0;
    for call in calls {
        if call["kind"] != "call" || call["owner"].as_u64() != Some(renderer) {
            continue;
        }
        let name = call["name"].as_str().ok_or("missing renderer method")?;
        if name == "battle_frame" {
            battles += 1;
        } else if Command::is_per_pass(name) {
            return Err(format!(
                "session version 2 requires battle_frame, found per-pass {name}"
            ));
        } else if !Command::is_known(name) {
            // Retired exports are absent from the generated command enum.
            // Reject them before content/resource admission, like any unknown API.
            return Err(format!("unknown renderer command {name}"));
        }
    }
    if battles == 0 {
        return Err("session version 2 contains no battle_frame".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn session(version: u32, names: &[&str]) -> Value {
        json!({"version":version,"renderer":7,"calls":names.iter().map(|name|json!({"kind":"call","owner":7,"name":name})).collect::<Vec<_>>()})
    }
    #[test]
    fn only_current_battle_frame_archives_are_admitted() {
        for names in [
            &["frame", "far_frame", "post_chain_frame"][..],
            &["battle_frame"][..],
        ] {
            assert!(admit(&session(1, names)).is_err());
        }
        assert!(admit(&session(
            2,
            &["frame_begin_frame", "frame_publish_frame", "battle_frame"]
        ))
        .is_ok());
        let api: Value = serde_json::from_str(include_str!(
            "../../../tools/native-session/renderer-api.json"
        ))
        .unwrap();
        for name in [
            "frame",
            "far_frame",
            "view_frame",
            "transparent_frame",
            "hud_frame",
            "interface_frame",
            "post_chain_frame",
        ] {
            assert!(!api["methods"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["name"] == name));
            assert!(admit(&session(2, &[name, "battle_frame"]))
                .unwrap_err()
                .contains(name));
        }
        assert!(admit(&session(1, &["battle_frame"])).is_err());
        assert!(admit(&session(2, &["frame_aux_frame"])).is_err());
        assert!(admit(&session(3, &["battle_frame"])).is_err());
        let mut meta = session(2, &["battle_frame"]);
        meta["calls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":"call","owner":8,"name":"frame"}));
        assert!(admit(&meta).is_ok());
    }
}
