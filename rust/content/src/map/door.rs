//! Class-authored 2005 door settings attached to a placed instance at admission.
use super::{GameFamily, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct DoorAnimation {
    pub bank: String,
    pub clip: String,
    pub trigger_joint: String,
    pub trigger_radius: f64,
}

impl DoorAnimation {
    /// A map-local bank can be bound immediately. Other packages may supply their own banks.
    pub fn has_bank(&self, banks: Option<&Value>) -> bool {
        banks
            .and_then(|m| m.get("banks"))
            .and_then(Value::as_object)
            .is_some_and(|table| {
                table
                    .entries()
                    .any(|(name, _)| name.eq_ignore_ascii_case(&self.bank))
            })
    }
}

pub(super) fn for_instance(
    family: GameFamily,
    base: &str,
    class: &str,
    classes: Option<&Value>,
    placed: &[(String, String)],
) -> Result<Option<DoorAnimation>, String> {
    if family != GameFamily::Swbf2 {
        return Ok(None);
    }
    let definition = classes
        .and_then(|c| c.get("entc"))
        .and_then(Value::as_object)
        .and_then(|t| {
            t.get(class).or_else(|| {
                t.entries()
                    .find(|(n, _)| n.eq_ignore_ascii_case(class))
                    .map(|(_, v)| v)
            })
        });
    let base = if base.is_empty() {
        definition
            .and_then(|d| d.get("base"))
            .and_then(Value::as_str)
            .unwrap_or("")
    } else {
        base
    };
    if !base.eq_ignore_ascii_case("door") {
        return Ok(None);
    }
    let props = definition
        .and_then(|d| d.get("properties"))
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    let property = |name: &str| {
        placed
            .iter()
            .rev()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .or_else(|| {
                props.iter().rev().find_map(|p| {
                    let p = p.as_array()?;
                    p.first()?
                        .as_str()?
                        .eq_ignore_ascii_case(name)
                        .then(|| p.get(1)?.as_str())
                        .flatten()
                })
            })
    };
    let Some(bank) = property("AnimationName") else {
        return Ok(None);
    };
    let clip = property("Animation")
        .filter(|s| !s.is_empty())
        .ok_or("door bank has no animation")?;
    let mut trigger = property("AnimationTrigger")
        .ok_or("door bank has no trigger")?
        .split_ascii_whitespace();
    let joint = trigger.next().ok_or("missing door trigger joint")?;
    let radius = trigger
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|r| r.is_finite() && *r > 0.)
        .ok_or("invalid door trigger radius")?;
    if trigger.next().is_some() || bank.is_empty() {
        return Err("invalid door animation declaration".into());
    }
    Ok(Some(DoorAnimation {
        bank: bank.to_owned(),
        clip: clip.to_owned(),
        trigger_joint: joint.to_owned(),
        trigger_radius: radius,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_door_binding_uses_the_placed_override_and_rejects_bad_triggers() {
        let json = r#"{"entc":{"Gate":{"base":"door","properties":[["AnimationName","shutter_bank"],["Animation","slide"],["AnimationTrigger","latch 2"]]}}}"#;
        let classes = crate::json::parse(json).unwrap();
        let read = |family, placed: &[(String, String)]| {
            for_instance(family, "", "gate", Some(&classes), placed)
        };
        let placed = [("AnimationTrigger".into(), "sensor 3.5".into())];
        let binding = read(GameFamily::Swbf2, &placed).unwrap().unwrap();
        assert_eq!(binding.bank, "shutter_bank");
        assert_eq!(binding.clip, "slide");
        assert_eq!(
            (binding.trigger_joint.as_str(), binding.trigger_radius),
            ("sensor", 3.5)
        );
        for invalid in ["sensor", "sensor -1", "sensor NaN", "sensor 2 extra"] {
            assert!(read(
                GameFamily::Swbf2,
                &[("AnimationTrigger".into(), invalid.into())]
            )
            .is_err());
        }
        assert_eq!(read(GameFamily::Swbf1, &placed).unwrap(), None);
    }
}
