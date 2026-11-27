//! Admit the character assets and presentation tables for a battle.
pub mod player;
pub mod state;
pub use bfe_pose::character::*;
use bfe_content::Value;

pub struct Class {
    pub side: String,
    pub name: String,
    pub model: String,
    pub animation: Character,
    pub tables: state::Tables,
}

/// Build palettes only for bodies in the admitted mission. Every track resolves
/// to a named mesh joint or an authored joint of this character's animation rig.
pub fn admit(battle_content: &crate::BattleContent) -> Result<Vec<Class>, String> {
    let content = &battle_content.content;
    let Some(battle) = &content.battle.swbf2 else {
        return Ok(Vec::new());
    };
    let shared = Archive::new(&battle.common_anim_banks, &battle.common_anim_bin)?;
    let mut classes = Vec::new();
    for (side_name, meta, bytes) in &battle.side_anim_banks {
        let side = Archive::new(meta, bytes)?;
        let Some(definitions) = content
            .battle
            .sides
            .iter()
            .find(|s| &s.name == side_name)
            .and_then(|s| s.units.as_object())
        else {
            continue;
        };
        let has_combo = definitions.values().any(|def| {
            array(def, "weapons")
                .iter()
                .any(|w| !array(w, "comboBindings").is_empty())
        });
        let machines = if has_combo {
            bfe_content::json::parse_bytes(
                &battle_content
                    .textures
                    .read(&format!("sides/{side_name}/combos.json"))?,
            )
            .map_err(|e| e.to_string())?
        } else {
            Value::Null
        };
        for (name, def) in definitions.entries() {
            let model = field(def, "model");
            let body_key = format!("{side_name}/{model}");
            let Some((_, actor)) = content.actors.iter().find(|(k, _)| *k == body_key) else {
                continue;
            };
            let unit = format!("{side_name}/{name}");
            let banks = battle
                .unit_anim_banks
                .iter()
                .find(|(key, _)| key == &unit)
                .ok_or_else(|| format!("missing animation banks for {unit}"))?;
            let animation = Character::bind(name, def, &banks.1, actor, &side, &shared)
                .map_err(|e| format!("{side_name}/{name}: {e}"))?;
            let body_clips = content.body_clips.iter()
                .find(|(key, _)| key == &body_key)
                .map(|(_, clips)| clips.as_ref());
            let profile = stance::movement_clips(content, &unit, body_clips);
            let tables = state::Tables::with_roll_slot(
                &animation,
                def,
                battle.common_anim_banks.get("sets").unwrap_or(&Value::Null),
                &machines,
                Some(stance::RollSlot::movement(profile)),
            )?;
            classes.push(Class {
                tables,
                side: side_name.clone(),
                name: name.into(),
                model: body_key,
                animation,
            });
        }
    }
    Ok(classes)
}
