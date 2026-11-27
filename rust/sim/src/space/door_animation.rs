//! Prepared trigger joints and opening clips for ship passage doors.
use crate::space::objects::{is_door, SpaceDoor};
use bfe_content::{
    clips::{load_clips_by_crc, Clip},
    map::MapAdmission,
    three::Matrix4,
    Value,
};
use std::{collections::HashMap, rc::Rc};

pub(crate) struct Binding {
    pub(super) instance: usize,
    trigger: [f64; 3],
    pub(super) animation: Option<Rc<Clip>>,
}

#[derive(Default)]
pub(crate) struct DoorSetup {
    pub bindings: Vec<Binding>,
    pub connections: Vec<super::passages::DoorConnection>,
}

pub(crate) fn load_for_battle(map: &MapAdmission) -> Result<DoorSetup, String> {
    let bindings = load(map)?;
    let connections = super::passages::connections(map, &bindings)?;
    Ok(DoorSetup { bindings, connections })
}

fn trigger_position(bones: &[Value], joint: &str, world: [f64; 16]) -> Result<[f64; 3], String> {
    let mut index = bones
        .iter()
        .position(|b| {
            b.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(joint))
        })
        .ok_or_else(|| format!("unknown door trigger joint {joint}"))?;
    let mut model = Matrix4::identity();
    for _ in 0..bones.len() {
        let bone = &bones[index];
        let values = bone
            .get("local")
            .and_then(Value::as_array)
            .filter(|v| v.len() == 16)
            .ok_or("invalid door trigger matrix")?;
        let mut m = [0.; 16];
        for (out, n) in m.iter_mut().zip(values) {
            *out = n
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("invalid door trigger matrix")?;
        }
        model = Matrix4::multiply_matrices(&Matrix4::from_array(&m), &model);
        let parent = bone
            .get("parent")
            .and_then(Value::as_f64)
            .ok_or("missing door joint parent")?;
        if parent == -1. {
            let m = Matrix4::multiply_matrices(&Matrix4::from_array(&world), &model).elements;
            return Ok([m[12], m[13], m[14]]);
        }
        if parent.fract() != 0. || !(0. ..bones.len() as f64).contains(&parent) {
            return Err("invalid door joint parent".into());
        }
        index = parent as usize;
    }
    Err("cyclic door trigger joints".into())
}

pub(crate) fn load(map: &MapAdmission) -> Result<Vec<Binding>, String> {
    let mut result = Vec::new();
    let mut cache = HashMap::new();
    let mut samples = None;
    for obj in &map.placed {
        if !is_door(obj, map.map_classes.as_ref()) {
            continue;
        }
        let declared = obj.door_animation.as_ref();
        let bones = obj
            .model
            .as_deref()
            .and_then(|model| map.props.as_ref()?.get("skeletons")?.get(model)?.as_array());
        let mut trigger = obj.position;
        let mut animation = None;
        if bones.is_none() && declared.is_some_and(|d| d.has_bank(map.map_anim_banks.as_ref())) {
            return Err(format!(
                "door {} has an animation bank but no skeleton",
                obj.name
            ));
        }
        if let (Some(binding), Some(bones)) = (declared, bones) {
            trigger = trigger_position(bones, &binding.trigger_joint, obj.matrix)?;
        }
        if let (Some(bones), Some(model), Some(binding), Some(meta)) =
            (bones, &obj.model, declared, map.map_anim_banks.as_ref())
        {
            let bank = binding.bank.as_str();
            let clip = binding.clip.as_str();
            let key = (
                model.clone(),
                bank.to_ascii_lowercase(),
                clip.to_ascii_lowercase(),
            );
            if !cache.contains_key(&key) {
                // A map may reference a side-provided bank. Only bind banks present in the map package.
                if binding.has_bank(Some(meta)) {
                    if samples.is_none() {
                        let bytes = map
                            .map_anim_bin
                            .as_ref()
                            .ok_or("declared door bank has no samples")?;
                        samples = Some(Rc::from(
                            bfe_content::bin_arrays::f32s(bytes).map_err(|e| e.to_string())?,
                        ));
                    }
                    let names = bones
                        .iter()
                        .map(|b| {
                            b.get("name")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .ok_or("missing door bone name")
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let alias = HashMap::from([(
                        bfe_content::zero_hash::crc_hex(clip),
                        vec![clip.to_owned()],
                    )]);
                    let (clips, _) = load_clips_by_crc(
                        meta,
                        samples.as_ref().unwrap(),
                        &[bank.to_owned()],
                        &names,
                        &alias,
                    )?;
                    let matched = clips
                        .list
                        .into_iter()
                        .find(|c| c.name == clip)
                        .ok_or_else(|| format!("door clip {clip} missing in bank {bank}"))?;
                    cache.insert(key.clone(), Rc::new(matched));
                }
            }
            animation = cache.get(&key).cloned();
        }
        result.push(Binding {
            instance: obj.index,
            trigger,
            animation,
        });
    }
    Ok(result)
}

pub(crate) fn attach(doors: &mut [SpaceDoor], bindings: Vec<Binding>) {
    for (door, binding) in doors.iter_mut().zip(bindings) {
        debug_assert_eq!(door.instance, binding.instance);
        door.trigger_position = binding.trigger;
        door.opening_clip = binding.animation;
    }
}
