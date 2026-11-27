//! The hull or turret owns the exposed seat transform and names its pilot pose.
use super::Class;
use crate::online::replicas::Replicas;
use bfe_content_native::BattleContent;
use bfe_presentation::rider_sources::Source;
use three::Matrix4;

pub(super) struct Seat<'a> {
    names: [&'a str; 2],
    nine: bool,
    pub source: Source,
}
impl Seat<'_> {
    pub fn pose(&self, class: &Class) -> Option<(usize, f64)> {
        let id = self
            .names
            .iter()
            .filter(|s| !s.is_empty())
            .find_map(|s| class.animation.named_clip(s))?;
        let clip = &class.animation.clips[id];
        // Nine-pose aim sheets have a seated neutral sample at their centre.
        let time = if self.nine {
            (clip.frames.saturating_sub(1) / 2) as f64 / clip.fps
        } else if clip.duration() > 0. {
            self.source.age.rem_euclid(clip.duration())
        } else {
            0.
        };
        Some((id, time))
    }
}
pub(super) fn seat(
    resources: &Replicas,
    (id, generation, index): (u32, u32, usize),
) -> Option<Seat<'_>> {
    if let Some(rig) = resources.hulls.rig(id.into(), generation.into()) {
        let (clip, anim, nine) = resources.hulls.rider(rig, index)?;
        if clip.is_empty() && anim.is_empty() {
            return None;
        }
        let sources = resources.hulls.owner.sources.borrow();
        let source = sources.get(rig as f64, index as f64, sources.epoch)?;
        return source.shown.then_some(Seat {
            names: [clip, anim],
            nine,
            source,
        });
    }
    let turret = resources.turrets.source(id.into(), generation.into())?;
    let (clip, nine) = resources.turrets.rider(turret)?;
    let source = *resources
        .turrets
        .owner
        .sources
        .borrow()
        .turrets
        .get(turret)?;
    source.shown.then_some(Seat {
        names: [clip, ""],
        nine,
        source,
    })
}

/// An exposed remote turret chair's seat: its placed instance, the
/// `PilotPosition` node in the chair's frame and the `PilotAnimation` name.
pub(crate) struct ChairSeat {
    pub instance: usize,
    pub local: Matrix4,
    pub pose: String,
}

/// The 2005 game draws a chair's operator at the chair's `PilotPosition` in its
/// `PilotAnimation` when `IsPilotExposed` is set, as it does a vehicle's rider.
pub(crate) fn chair_seats(content: &BattleContent) -> Vec<ChairSeat> {
    let battle = &content.content.battle;
    let Some(data) = battle.props.as_ref() else {
        return Vec::new();
    };
    bfe_sim::world::admit::rows::props::live_sources(battle)
        .into_iter()
        .filter_map(|source| {
            let desc = data.get("instances")?.at(source)?;
            let (local, pose) = chair_seat(desc, data.get("models")?)?;
            Some(ChairSeat { instance: source, local, pose })
        })
        .collect()
}

/// A placed remote terminal's exposed seat in its own frame, and its pose name.
fn chair_seat(desc: &bfe_content::Value, models: &bfe_content::Value) -> Option<(Matrix4, String)> {
    use bfe_content::Value;
    if desc.get("base").and_then(Value::as_str) != Some("remoteterminal") {
        return None;
    }
    let settings = desc.get("settings")?;
    let text = |key: &str| settings.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
    if bfe_content::odf::num(settings.get("IsPilotExposed"), 0.) == 0. {
        return None;
    }
    let (node, pose) = (text("PilotPosition")?, text("PilotAnimation")?);
    let rig = models.get(desc.get("model").and_then(Value::as_str)?)?;
    let def = bfe_pose::rig::content::hull_records(&Value::Null, rig, &[], vec![], false);
    let mut rig = bfe_pose::rig::hull::HullRig::new(&def);
    rig.tree.update_matrix_world(rig.root, true);
    let id = rig.named(Some(node))?;
    Some((rig.tree.node(id).matrix_world, pose.to_string()))
}

/// A chair seat's pose for this class: the named clip, else the clip that
/// name takes in one of the class's banks ("perch" in bank "loam_3" is
/// "loam_perch").
pub(super) fn chair_clip(class: &Class, pose: &str) -> Option<usize> {
    class.animation.named_clip(pose).or_else(|| {
        class.animation.banks.iter().find_map(|bank| {
            let base = bank.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_');
            class.animation.named_clip(&format!("{base}_{pose}"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content::{actor::build_actor, json, zero_hash};
    use bfe_content_native::animation2::{state::Tables, Archive, Character};

    #[test]
    fn an_exposed_chair_seats_its_operator_at_its_pilot_node() {
        let models = json::parse(
            r#"{"stool_rig":{"bones":[
                {"name":"stool_root","parent":-1,"local":[0,0,1,0, 0,1,0,0, -1,0,0,0, 2,0,3,1]},
                {"name":"perch_node","parent":0,"local":[1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0.8,0.25,1]}]}}"#,
        )
        .unwrap();
        let chair = |base: &str, exposed: &str, pose: &str| {
            json::parse(&format!(
                r#"{{"base":"{base}","model":"stool_rig","settings":{{"PilotPosition":"PERCH_NODE","PilotAnimation":"{pose}","IsPilotExposed":"{exposed}"}}}}"#
            ))
            .unwrap()
        };
        let (local, pose) = chair_seat(&chair("remoteterminal", "1", "perch"), &models).unwrap();
        assert_eq!(pose, "perch");
        let at = [local.elements[12], local.elements[13], local.elements[14]];
        assert!(at.iter().zip([1.75, 0.8, 3.0]).all(|(a, b)| (a - b).abs() < 1e-9), "{at:?}");
        // The seat faces the way the chair does.
        assert!((local.elements[8] + 1.).abs() < 1e-9);
        // A hidden pilot, a chair without a pose and a prop are not seats.
        assert!(chair_seat(&chair("remoteterminal", "0", "perch"), &models).is_none());
        assert!(chair_seat(&chair("remoteterminal", "1", ""), &models).is_none());
        assert!(chair_seat(&chair("prop", "1", "perch"), &models).is_none());
    }

    fn class(bank: &str, clip: &str) -> Class {
        let mesh = json::parse(r#"{"bones":[{"name":"spine","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#).unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let frames = 9;
        let samples: Vec<f32> = (0..frames).flat_map(|i| [0f32, 0., 0., 1., 0., i as f32 / 30., 0.]).collect();
        let meta = json::parse(
            &serde_json::json!({"fps":30,"banks":{bank:{"clips":[{"crc":zero_hash::crc_hex(clip),"flags":0,"offset":0,"frames":frames,"tracks":[zero_hash::crc_hex("spine")]}]}}}).to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(r#"{"weapons":[{"AnimationBank":"test"}]}"#).unwrap();
        let animation = Character::bind("sitter", &def, &[bank.into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&animation, &def, &bfe_content::Value::Null, &bfe_content::Value::Null).unwrap();
        Class { side: String::new(), name: "sitter".into(), model: String::new(), animation, tables }
    }

    #[test]
    fn a_seat_pose_names_a_clip_of_the_soldiers_bank() {
        assert!(chair_clip(&class("loam_3", "loam_perch"), "perch").is_some());
        assert!(chair_clip(&class("loam", "perch"), "perch").is_some());
        assert!(chair_clip(&class("loam_3", "loam_kneel"), "perch").is_none());
    }
}
