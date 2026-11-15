//! Doors and animated map objects use the shared prop presentation clock.
use super::receiver::Receiver;
use crate::render_bridge::Bridge;
use bfe_content::{odf::num, Value};
use bfe_content_native::BattleContent;
use bfe_pose::rig::hull::HullRig;
use bfe_presentation::{
    layout::{prop_frame as f, prop_input as p, prop_limits as lim},
    network_layout::{net_kind as k, net_record as r},
    props::{PropInput, Props, TriggerInput},
};
use bfe_render::instances as draw;
use three::{Matrix4, Vector3};
struct View {
    rig: HullRig,
    inverses: Vec<Matrix4>,
    skin: Vec<f32>,
    rows: Vec<[f64; draw::REC_STRIDE]>,
    bone: usize,
}
pub struct Animated {
    owner: Box<Props>,
    views: Vec<View>,
}
impl Animated {
    pub fn new(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
    ) -> Result<Self, String> {
        let mut owner = Box::new(Props::new());
        let mut views = Vec::new();
        let Some(data) = content.content.battle.props.as_ref() else {
            return Ok(Self { owner, views });
        };
        for (source, desc) in data
            .get("instances")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            if !matches!(
                desc.get("base").and_then(Value::as_str),
                Some("animatedprop" | "door")
            ) {
                continue;
            }
            let Some(name) = desc.get("model").and_then(Value::as_str) else {
                continue;
            };
            let Some(model) = data.get("models").and_then(|v| v.get(name)) else {
                continue;
            };
            let def = bfe_pose::rig::content::hull_records(&Value::Null, model, &[], vec![], false);
            let root = def
                .bones
                .iter()
                .position(|b| matches!(b.name.to_lowercase().as_str(), "dummyroot" | "dummy_root"))
                .unwrap_or(0);
            let mut rig = HullRig::new(&def);
            let inverses = rig
                .bones
                .iter()
                .chain(std::iter::once(&rig.static_bone))
                .map(|id| {
                    let mut m = rig.tree.node(*id).matrix_world;
                    m.invert();
                    m
                })
                .collect::<Vec<_>>();
            let initial = rig
                .bones
                .iter()
                .flat_map(|id| {
                    let n = rig.tree.node(*id);
                    [
                        n.quaternion.x,
                        n.quaternion.y,
                        n.quaternion.z,
                        n.quaternion.w,
                        n.position.x,
                        n.position.y,
                        n.position.z,
                    ]
                })
                .collect::<Vec<_>>();
            let matrix = desc
                .get("matrix")
                .and_then(Value::as_array)
                .ok_or("animated prop has no placement")?;
            for i in 0..16 {
                rig.tree.node_mut(rig.root).matrix.elements[i] = matrix[i].as_f64().unwrap_or(0.);
            }
            rig.tree.node_mut(rig.root).matrix_auto_update = false;
            rig.tree.update_matrix_world(rig.root, true);
            let anim = desc.get("animation").unwrap_or(&Value::Null);
            let names = if let Some(a) = anim.get("clips").and_then(Value::as_array) {
                a.iter().filter_map(Value::as_str).collect::<Vec<_>>()
            } else {
                [
                    anim.get("idle"),
                    anim.get("trigger").and_then(|v| v.get("action")),
                ]
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect()
            };
            let bank = content
                .content
                .prop_clips
                .iter()
                .find(|(n, _)| n == name)
                .and_then(|(_, c)| c.as_ref());
            let mut clips = std::collections::BTreeMap::new();
            if let Some(bank) = bank {
                for c in &bank.list {
                    clips.insert(
                        c.name.as_str(),
                        owner.load_clip(
                            c.frames_f64() as usize,
                            c.fps,
                            c.root_motion,
                            c.tracks.iter().map(|n| *n as i32).collect(),
                            c.data().to_vec(),
                        ),
                    );
                }
            }
            let slots = names
                .iter()
                .map(|n| clips.get(n).copied())
                .collect::<Vec<_>>();
            let door = desc.get("base").and_then(Value::as_str) == Some("door");
            let door_clip = if door {
                clips
                    .get("open")
                    .copied()
                    .or_else(|| slots.first().copied().flatten())
            } else {
                None
            };
            let duration = bank
                .and_then(|b| {
                    b.get("open")
                        .or_else(|| names.first().and_then(|n| b.get(n)))
                })
                .map_or(0., |c| c.duration);
            let triggers = anim
                .get("triggers")
                .and_then(Value::as_array)
                .map(|a| a.iter().collect::<Vec<_>>())
                .unwrap_or_else(|| anim.get("trigger").into_iter().collect());
            let mut points = Vec::new();
            let mut opens = false;
            for trigger in triggers {
                let point = rig.tree.node(rig.root).matrix_world;
                let mut at =
                    Vector3::new(point.elements[12], point.elements[13], point.elements[14]);
                if let Some(id) = rig.named(trigger.get("node").and_then(Value::as_str)) {
                    rig.tree.get_world_position(id, &mut at);
                }
                let action = trigger.get("action").and_then(Value::as_str).unwrap_or("");
                opens |= action == "open";
                points.push(TriggerInput {
                    slot: names
                        .iter()
                        .position(|n| *n == action)
                        .map_or(-1, |i| i as i32),
                    point: [at.x, at.y, at.z],
                    radius: num(trigger.get("radius"), 0.),
                    sound: trigger.get("sound").is_some_and(Value::truthy),
                });
            }
            let index = views.len();
            let phase = (((index as u64 + 1) * 2654435761) & 0xffff_ffff) as f64 / 4294967296.;
            let time = if anim.get("fromStart").is_some_and(Value::truthy) {
                0.
            } else {
                phase
                    * bank
                        .and_then(|b| names.first().and_then(|n| b.get(n)))
                        .map_or(0., |c| c.frames_f64() / c.fps)
            };
            owner.add_prop(PropInput {
                scene: Default::default(),
                root: root as i32,
                initial: initial.clone(),
                bind: initial,
                door,
                door_clip,
                door_duration: duration,
                slots,
                open_slots: names.iter().map(|n| *n == "open").collect(),
                triggers: points,
                opens,
                body: desc.get("collision").is_some_and(Value::truthy),
                idle_delay: num(anim.get("idleDelay"), 0.),
                grabs: anim
                    .get("attach")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty()),
                grab_nodes: vec![],
                time,
            });
            let admission = crate::static_scene::props(
                bridge,
                content,
                textures,
                bfe_content_native::placed::admit_look(content, source, 0)?,
                bridge.shadow_cascades(),
            )?;
            views.push(View {
                skin: vec![0.; inverses.len() * 16],
                rig,
                inverses,
                rows: admission.records,
                bone: owner.bone_start(false, index),
            });
        }
        Ok(Self { owner, views })
    }
    pub fn frame(&mut self, receiver: &Receiver, tick: f64, dt: f64) {
        self.owner.input[f::VERSION] = bfe_presentation::layout::VERSION as f64;
        self.owner.input[f::DT] = dt;
        self.owner.input[f::TICK] = tick;
        let mut count = 0;
        for row in receiver
            .active()
            .filter(|v| matches!(v[r::KIND] as u32, k::SOLDIER | k::DROIDEKA) && v[r::DEAD] == 0.)
        {
            if count == lim::FEET {
                break;
            }
            let at = f::STRIDE + count * 3;
            self.owner.input[at..at + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            let at = Props::bodies_at() + count * 4;
            self.owner.input[at..at + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            self.owner.input[at + 3] = 1.;
            count += 1;
        }
        self.owner.input[f::FEET] = count as f64;
        self.owner.input[f::BODIES] = count.min(lim::BODIES) as f64;
        for field in receiver.publications.iter() {
            if field.row[r::KIND] != k::SUMMARY as f64 || field.row[r::SCHEMA] != 9. {
                continue;
            }
            if let Ok(s) = serde_json::from_slice::<[f64; 8]>(&field.bytes[..field.length]) {
                let i = s[0] as usize;
                if i >= self.views.len() {
                    continue;
                }
                let a = &mut self.owner.prop_input[i * p::STRIDE..];
                a[p::WORLD] = 1.;
                a[p::SLOT] = s[1];
                a[p::STARTED] = s[2];
                a[p::START_TIME] = s[3];
            }
        }
        self.owner.frame();
        for view in &mut self.views {
            for (i, id) in view.rig.bones.iter().enumerate() {
                let row = &self.owner.bones[(view.bone + i) * 7..];
                let n = view.rig.tree.node_mut(*id);
                n.quaternion.set(row[0], row[1], row[2], row[3]);
                n.position.set(row[4], row[5], row[6]);
            }
            view.rig.tree.update_matrix_world(view.rig.root, true);
            for (i, id) in view
                .rig
                .bones
                .iter()
                .chain(std::iter::once(&view.rig.static_bone))
                .enumerate()
            {
                let mut m = view.rig.tree.node(*id).matrix_world;
                m.multiply(&view.inverses[i]);
                for j in 0..16 {
                    view.skin[i * 16 + j] = m.elements[j] as f32;
                }
            }
        }
    }
    pub fn max_bones(&self) -> usize {
        self.views
            .iter()
            .map(|v| v.inverses.len())
            .max()
            .unwrap_or(1)
    }
    pub fn record_budget(&self) -> usize {
        self.views.iter().map(|v| v.rows.len()).sum()
    }
    pub fn append(
        &self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
    ) -> (usize, usize) {
        for view in &self.views {
            for recipe in &view.rows {
                let count = view.inverses.len();
                if records >= draw::MAX_RECORDS || bones + count > draw::MAX_BONES {
                    continue;
                }
                let mut row = *recipe;
                row[draw::REC_BONES] = count as f64;
                row[draw::REC_BONE_AT] = bones as f64;
                bridge.r.core.bones[bones * 16..(bones + count) * 16].copy_from_slice(&view.skin);
                let at = draw::IN_RECORDS + records * draw::REC_STRIDE;
                bridge.r.core.input[at..at + draw::REC_STRIDE].copy_from_slice(&row);
                records += 1;
                bones += count;
            }
        }
        (records, bones)
    }
}
