//! Session versions describe the recorded submission contract. A newer
//! recording cannot silently resume legacy per-pass drawing after a failure.
use crate::render_bridge::Command;
use serde_json::Value;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    PerPass = 1,
    BattleFrame = 2,
}
impl Version {
    // Validate before admitting content or creating GPU resources. The API
    // digest remains a separate check: a format version does not translate
    // changed numeric layouts or make incompatible old banks safe to borrow.
    pub fn admit(meta: &Value) -> Result<Self, String> {
        let version = match meta["version"].as_u64() {
            Some(1) => Self::PerPass,
            Some(2) => Self::BattleFrame,
            _ => return Err("unsupported native session version".into()),
        };
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
                if version == Self::PerPass {
                    return Err("battle_frame requires session version 2".into());
                }
                battles += 1;
            } else if version == Self::BattleFrame && Command::is_per_pass(name) {
                return Err(format!(
                    "session version 2 requires battle_frame, found per-pass {name}"
                ));
            }
        }
        if version == Self::BattleFrame && battles == 0 {
            return Err("session version 2 contains no battle_frame".into());
        }
        Ok(version)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn session(version: u32, names: &[&str]) -> Value {
        json!({"version":version,"renderer":7,"calls":names.iter().map(|name|json!({"kind":"call","owner":7,"name":name})).collect::<Vec<_>>()})
    }
    #[test]
    fn recorded_version_selects_the_submission_contract_without_a_switch() {
        assert_eq!(
            Version::admit(&session(1, &["frame", "far_frame", "post_chain_frame"])).unwrap(),
            Version::PerPass
        );
        assert_eq!(
            Version::admit(&session(
                2,
                &["frame_begin_frame", "frame_publish_frame", "battle_frame"]
            ))
            .unwrap(),
            Version::BattleFrame
        );
        for name in [
            "frame",
            "far_frame",
            "view_frame",
            "transparent_frame",
            "hud_frame",
            "interface_frame",
            "post_chain_frame",
        ] {
            assert!(Version::admit(&session(2, &[name, "battle_frame"]))
                .unwrap_err()
                .contains(name));
        }
        assert!(Version::admit(&session(1, &["battle_frame"])).is_err());
        assert!(Version::admit(&session(2, &["frame_aux_frame"])).is_err());
        assert!(Version::admit(&session(3, &["battle_frame"])).is_err());
        let mut meta = session(2, &["battle_frame"]);
        meta["calls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":"call","owner":8,"name":"frame"}));
        assert!(Version::admit(&meta).is_ok());
    }
}
