//! Character-bound stance and movement action slots. All lookups run at admission.
use super::Character;
use bfe_content::{clips::Clips, content::Content, zero_hash};

/// Class clips take precedence over the body's animation binding.
pub fn movement_clips<'a>(
    content: &'a Content,
    unit: &str,
    body: Option<&'a Clips>,
) -> Option<&'a Clips> {
    content
        .unit_clips
        .iter()
        .find(|(name, _)| name == unit)
        .map(|(_, clips)| clips.as_ref())
        .or(body)
}

#[derive(Clone, Copy)]
pub struct BoundClip {
    pub frames: usize,
    crc: u32,
    bank: Option<usize>,
}

impl BoundClip {
    pub fn in_rig(self, c: &Character) -> Option<usize> {
        c.banks
            .iter()
            .enumerate()
            .filter(|(i, _)| self.bank.is_none_or(|b| b == *i))
            .filter_map(|(_, bank)| c.clip(bank, self.crc))
            .find(|&i| c.clips[i].frames == self.frames)
    }
}

pub struct Dive {
    pub form: String,
    pub layers: [Option<BoundClip>; 3],
}

#[derive(Clone, Copy)]
pub enum RollSlot<'a> {
    /// A live movement profile supplies the clip that owns the action window.
    Movement(Option<BoundClip>),
    /// Standalone animation without a movement profile uses its bound stance.
    Equipped(&'a Character, &'a [String]),
}

impl RollSlot<'_> {
    /// Read the same movement slot once for admission by the mover and the display.
    pub fn movement(clips: Option<&Clips>) -> Self {
        Self::Movement(
            clips
                .and_then(|clips| clips.swbf2_slot("diveforward"))
                .map(|clip| BoundClip {
                    crc: zero_hash::crc(&clip.name),
                    frames: clip.frames_f64() as usize,
                    bank: None,
                }),
        )
    }

    pub fn resolve(self, direction: &str) -> Option<Dive> {
        match self {
            Self::Movement(key) => Some(Dive {
                form: "diveforward".into(),
                layers: [Some(key?), None, None],
            }),
            Self::Equipped(c, chain) => {
                for form in [
                    format!("roll{direction}"),
                    format!("roll_{direction}"),
                    format!("dive{direction}"),
                    "diveforward".into(),
                ] {
                    let parts = layers(c, chain, &form, false, false);
                    if parts.iter().any(Option::is_some) {
                        return Some(Dive {
                            form,
                            layers: parts.map(|part| {
                                part.map(|id| {
                                    let clip = &c.clips[id];
                                    BoundClip {
                                        crc: clip.crc,
                                        frames: clip.frames,
                                        bank: Some(clip.bank),
                                    }
                                })
                            }),
                        });
                    }
                }
                None
            }
        }
    }
}

/// Stance inheritance is scoped to the packages bound to this character.
pub fn stance_chain(stance: &str, parent: Option<&str>) -> Vec<String> {
    let mut chain = vec![stance.to_ascii_lowercase()];
    if let Some(parent) = parent.filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(stance)) {
        chain.push(parent.to_ascii_lowercase());
    }
    loop {
        let next = match chain.last().unwrap().as_str() {
            "rifle" => break,
            "pistol" | "melee" => "tool",
            _ => "rifle",
        };
        if chain.iter().any(|s| s == next) {
            break;
        }
        chain.push(next.into());
    }
    chain
}

pub fn layers(
    c: &Character,
    chain: &[String],
    form: &str,
    reaction: bool,
    gait: bool,
) -> [Option<usize>; 3] {
    let mut result = [None; 3];
    let mut in_place = None;
    for (depth, stance) in chain.iter().enumerate() {
        for name in &c.banks {
            let prefix = name
                .rsplit_once('_')
                .filter(|(_, n)| n.parse::<u16>().is_ok())
                .map_or(name.as_str(), |(p, _)| p);
            let qualified = format!("{prefix}_{stance}");
            for root in std::iter::once(qualified.as_str()).chain((depth == 0).then_some(prefix)) {
                let lookup = |suffix: &str| {
                    c.clip(name, zero_hash::crc(&format!("{root}_{form}{suffix}")))
                        .filter(|&id| !reaction || c.clips[id].frames > 1)
                };
                let base = lookup("_full").or_else(|| lookup(""));
                let lower = lookup("_lower");
                let upper = lookup("_upper");
                if gait {
                    if let Some(stride) = lower.or(base).filter(|&i| c.clips[i].moves_legs(&c.rig))
                    {
                        let layers = if result[2].is_some() {
                            [None, Some(stride), result[2]]
                        } else {
                            [base, lower, upper]
                        };
                        if c.clips[stride].speed > 1e-4 {
                            return layers;
                        }
                        // Keep a genuinely moving in-place gait if the whole
                        // chain lacks a measured stride for this form.
                        in_place.get_or_insert(layers);
                    }
                    result[2] = result[2].or(upper).or(base);
                } else {
                    result = [result[0].or(base), result[1].or(lower), result[2].or(upper)];
                    if result[0].is_some() || (result[1].is_some() && result[2].is_some()) {
                        return result;
                    }
                }
            }
        }
    }
    in_place.unwrap_or(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Archive;
    use bfe_content::{actor::build_actor, json, Value};
    use serde_json::json;

    #[test]
    fn movement_and_standalone_rolls_use_their_own_slot() {
        let actor = build_actor(&json::parse(r#"{"bones":[],"parts":[]}"#).unwrap(), &[]).unwrap();
        let child = "chalk_tool_diveforward";
        let fixed = "chalk_rifle_diveforward";
        let descriptor = |name, frames| {
            json!({"crc":zero_hash::crc_hex(name),
            "frames":frames,"offset":0,"flags":0,"tracks":[]})
        };
        let meta = json::parse(
            &json!({"fps":12,"banks":{"chalk":{"clips":[
            descriptor(child,9),descriptor(fixed,6)]}}})
            .to_string(),
        )
        .unwrap();
        let archive = Archive::new(&meta, &[]).unwrap();
        let c = Character::bind(
            "courier",
            &Value::Null,
            &["chalk".into()],
            &actor,
            &archive,
            &archive,
        )
        .unwrap();
        let chain = stance_chain("lantern", Some("pistol"));
        assert_eq!(chain, ["lantern", "pistol", "tool", "rifle"]);
        let pose = RollSlot::Equipped(&c, &chain).resolve("left").unwrap();
        assert_eq!(pose.form, "diveforward");
        let key = pose.layers[0].unwrap();
        assert_eq!(key.frames, 9);
        assert_eq!(key.in_rig(&c), c.named_clip(child));

        let moves = json::parse(
            &json!({"fps":12,"clips":{(fixed):{
            "frames":6,"offset":0,"tracks":[],"travel":[0,0,0]}}})
            .to_string(),
        )
        .unwrap();
        let clips = bfe_content::clips::load_clips(&moves, &Default::default(), None).unwrap();
        let live = RollSlot::movement(Some(&clips))
            .resolve("left")
            .unwrap()
            .layers[0]
            .unwrap();
        assert_eq!(live.frames, 6);
        assert_eq!(live.in_rig(&c), c.named_clip(fixed));
        assert!(RollSlot::movement(None).resolve("forward").is_none());
    }
}
