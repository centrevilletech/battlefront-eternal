//! Doors and animated map objects use the shared prop presentation clock.
use super::receiver::Receiver;
use crate::render_bridge::Bridge;
use bfe_content::{odf::num, Value};
use bfe_content_native::BattleContent;
use bfe_pose::rig::hull::HullRig;
use bfe_presentation::{
    layout::{prop_frame as f, prop_input as p, prop_limits as lim, prop_output as o, trap_input as t},
    network_layout::{net_kind as k, net_record as r},
    props::{PropInput, Props, TrapDesc, TriggerInput},
};
use bfe_render::instances as draw;
use three::{Matrix4, Vector3};
struct View {
    rig: HullRig,
    inverses: Vec<Matrix4>,
    skin: Vec<f32>,
    rows: Vec<[f64; draw::REC_STRIDE]>,
    bone: usize,
    death_camera: bool,
}
struct TrapView {
    building: usize,
    view: View,
    pad_rows: Vec<[f64; draw::REC_STRIDE]>,
    pad_bones: Vec<f32>,
    phase: u8,
    started: f64,
}
#[derive(Clone, Copy)]
struct Holding { id: u32, generation: u32, grab: usize, since: f64 }
struct Rider { handle: usize, birth: Option<(u32, u32, u64)> }
pub struct Animated {
    owner: Box<Props>,
    views: Vec<View>,
    traps: Vec<TrapView>,
    holds: Vec<Option<Holding>>,
    grab_clips: Vec<Vec<String>>,
    held_clips: Vec<Option<usize>>,
    riders: Vec<Option<Rider>>,
    rider_clips: std::collections::BTreeMap<usize, Vec<(String, usize, u32)>>,
}
impl Animated {
    pub fn new(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
    ) -> Result<Self, String> {
        let mut owner = Box::new(Props::new());
        let mut views = Vec::new();
        let mut traps = Vec::new();
        let mut grab_clips = Vec::new();
        let Some(data) = content.content.battle.props.as_ref() else {
            return Ok(Self { owner, views, traps, holds: vec![], grab_clips, held_clips: vec![], riders: vec![], rider_clips: Default::default() });
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
            let mut ids = vec![usize::MAX; rig.tree.nodes.len()];
            let mut nodes = Vec::new();
            super::turrets::copy_nodes(&rig.tree, rig.root, -1, &mut nodes, &mut ids);
            let grabs = anim.get("attach").and_then(Value::as_array).unwrap_or(&[]);
            grab_clips.push(grabs.iter().map(|g|g.get("clip").and_then(Value::as_str).unwrap_or("").to_string()).collect());
            owner.add_prop(PropInput {
                scene: bfe_presentation::scene::Input { nodes, bones: rig.bones.iter().map(|id|ids[*id as usize]).collect(), parent: None },
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
                grab_nodes: grabs.iter().map(|g| rig.named(g.get("node").and_then(Value::as_str)).map_or(-1,|id|ids[id as usize] as i32)).collect(),
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
                death_camera: anim.get("deathCamera").is_some_and(Value::truthy),
                skin: vec![0.; inverses.len() * 16],
                rig,
                inverses,
                rows: admission.records,
                bone: owner.bone_start(false, index),
            });
        }
        for (source, desc) in data.get("instances").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
            if desc.get("base").and_then(Value::as_str) != Some("trap") { continue; }
            let Some(name) = desc.get("model").and_then(Value::as_str) else { continue; };
            let Some(model) = data.get("models").and_then(|m| m.get(name)) else { continue; };
            let def = bfe_pose::rig::content::hull_records(&Value::Null, model, &[], vec![], false);
            let root = def.bones.iter().position(|b| matches!(b.name.to_lowercase().as_str(), "dummyroot" | "dummy_root")).unwrap_or(0);
            let mut rig = HullRig::new(&def);
            let inverses = rig.bones.iter().chain(std::iter::once(&rig.static_bone)).map(|id| {
                let mut m = rig.tree.node(*id).matrix_world; m.invert(); m
            }).collect::<Vec<_>>();
            let initial = rig.bones.iter().flat_map(|id| {
                let n = rig.tree.node(*id);
                [n.quaternion.x,n.quaternion.y,n.quaternion.z,n.quaternion.w,n.position.x,n.position.y,n.position.z]
            }).collect();
            let matrix = desc.get("matrix").and_then(Value::as_array).ok_or("trap has no placement")?;
            for i in 0..16 { rig.tree.node_mut(rig.root).matrix.elements[i] = matrix[i].as_f64().unwrap_or(0.); }
            rig.tree.node_mut(rig.root).matrix_auto_update = false;
            let settings = desc.get("trap").unwrap_or(&Value::Null);
            let bank = content.content.prop_clips.iter().find(|(n,_)| n == name).and_then(|(_,c)| c.as_ref());
            let mut load = |key| settings.get(key).and_then(Value::as_str).and_then(|n| bank.and_then(|b| b.get(n))).map(|c| {
                (owner.load_clip(c.frames_f64() as usize,c.fps,c.root_motion,c.tracks.iter().map(|n|*n as i32).collect(),c.data().to_vec()),c.frames_f64()/30.)
            });
            let trigger = load("trigger");
            let build = load("build");
            let index = owner.add_trap(TrapDesc { scene: Default::default(), root: root as i32, initial,
                trigger_clip: trigger.map(|c|c.0), build_clip: build.map(|c|c.0), trigger_length: trigger.map_or(0.,|c|c.1) });
            let admission = crate::static_scene::props(bridge,content,textures,
                bfe_content_native::placed::admit_look(content,source,0)?,bridge.shadow_cascades())?;
            let pad = crate::static_scene::props(bridge,content,textures,
                bfe_content_native::placed::admit_trap_pad(content,source)?,bridge.shadow_cascades())?;
            let building = bfe_sim::world::admit::rows::props::live_sources(&content.content.battle).iter().position(|id|*id==source).ok_or("trap has no building id")?;
            traps.push(TrapView { building, view: View { death_camera: false, skin: vec![0.; inverses.len()*16], rig, inverses,
                rows: admission.records, bone: owner.bone_start(true,index) }, pad_rows: pad.records, pad_bones: pad.bones,
                phase: 0, started: 0. });
        }
        let holds = vec![None; views.len()];
        let held_clips = vec![None; views.len()];
        Ok(Self { owner, views, traps, holds, grab_clips, held_clips, riders: vec![], rider_clips: Default::default() })
    }
    pub fn bind_actor(&mut self, units: &bfe_presentation::units::Units, actor: usize, model: usize, names: &[(String, f64)]) {
        if self.grab_clips.iter().all(Vec::is_empty) { return; }
        if !self.rider_clips.contains_key(&model) {
            let mut clips = Vec::new();
            for (index, (name, _)) in names.iter().enumerate() {
                if !self.grab_clips.iter().flatten().any(|n| n == name) { continue; }
                if let Some(c) = units.rider_clip(actor, index) {
                    let handle = self.owner.load_clip(c.frames,c.fps,c.root_motion,c.tracks.clone(),c.data.clone());
                    clips.push((name.clone(),index,handle));
                }
            }
            self.rider_clips.insert(model,clips);
        }
        if let Some((skeleton, rig)) = units.rider_source(actor) {
            let handle = self.owner.add_unit_rider(skeleton,rig);
            self.riders.resize_with(self.riders.len().max(actor+1),||None);
            self.riders[actor] = Some(Rider { handle, birth: None });
        }
    }
    pub fn frame(&mut self, receiver: &Receiver, tick: f64, dt: f64, units: &bfe_presentation::units::Units,
        resolve: impl Fn(u32,u32)->Option<(usize,usize)>) {
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
            if field.row[r::KIND] != k::SUMMARY as f64 { continue; }
            if field.row[r::SCHEMA] == 10. {
                if let Ok(s) = serde_json::from_slice::<[f64;4]>(&field.bytes[..field.length]) {
                    if let Some(trap) = self.traps.get_mut(s[0] as usize) {
                        trap.phase = s[1] as u8;
                        trap.started = s[2];
                        let row = &mut self.owner.trap_input[s[0] as usize * t::STRIDE..];
                        row[t::PHASE] = s[1]; row[t::STARTED] = s[2];
                    }
                }
            }
            if field.row[r::SCHEMA] != 9. {
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
                self.holds[i] = (s[4] > 0. && s[6] >= 0.).then_some(Holding { id: s[4] as u32, generation: s[5] as u32, grab: s[6] as usize, since: s[7] });
            }
        }
        for (index, hold) in self.holds.iter().enumerate() {
            let row = &mut self.owner.prop_input[index*p::STRIDE..];
            row[p::VICTIM] = 0.;
            self.held_clips[index] = None;
            let Some(hold) = hold else { continue };
            let Some((actor,model)) = resolve(hold.id,hold.generation) else { continue };
            let Some(name) = self.grab_clips[index].get(hold.grab) else { continue };
            let Some((_,clip,handle)) = self.rider_clips.get(&model).and_then(|c|c.iter().find(|(n,_,_)|n==name)) else { continue };
            let Some(rider) = self.riders.get_mut(actor).and_then(Option::as_mut) else { continue };
            row[p::VICTIM] = 1.; row[p::GRAB] = hold.grab as f64; row[p::SINCE] = hold.since;
            row[p::RIDER] = rider.handle as f64; row[p::CLIP] = *handle as f64;
            self.held_clips[index] = Some(*clip);
            let birth = (hold.id,hold.generation,hold.since.to_bits());
            if rider.birth != Some(birth) {
                if let Some((_,rig)) = units.rider_source(actor) { self.owner.refresh_unit_rider(rider.handle,rig); }
                rider.birth = Some(birth);
            }
        }
        self.owner.frame();
        for view in self.views.iter_mut().chain(self.traps.iter_mut().map(|t| &mut t.view)) {
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
    pub fn held(&self, id: u32, generation: u32) -> Option<([f64;16],f64,usize)> {
        let index = self.holds.iter().position(|h|h.is_some_and(|h| h.id == id && h.generation == generation))?;
        let clip = self.held_clips[index]?;
        let row = &self.owner.prop_output[index*o::STRIDE..];
        if row[o::HELD_PLACED] == 0. { return None; }
        Some((std::array::from_fn(|i|row[o::HELD_FRAME+i]),row[o::HELD_TIME],clip))
    }
    pub fn creature_holds(&self) -> impl Iterator<Item = (usize, u32, u32, usize, f64, Option<([f64; 16], f64, usize)>)> + '_ {
        self.holds.iter().enumerate().filter_map(|(index, hold)| {
            let h = hold.as_ref()?;
            Some((index, h.id, h.generation, h.grab, h.since, self.held(h.id, h.generation)))
        })
    }
    pub fn held_camera(&self, id: u32, generation: u32) -> Option<bool> {
        let index = self.holds.iter().position(|h| h.is_some_and(|h| h.id == id && h.generation == generation))?;
        Some(self.views[index].death_camera)
    }
    pub fn max_bones(&self) -> usize {
        self.views
            .iter()
            .chain(self.traps.iter().map(|t| &t.view))
            .map(|v| v.inverses.len())
            .max()
            .unwrap_or(1)
    }
    pub fn record_budget(&self) -> usize {
        self.views.iter().map(|v| v.rows.len()).sum::<usize>() + self.traps.iter().map(|t| t.view.rows.len() + if t.phase == 0 { t.pad_rows.len() } else { 0 }).sum::<usize>()
    }
    pub fn trap_states(&self) -> impl Iterator<Item=(usize,u8,f64)> + '_ {
        self.traps.iter().enumerate().map(|(i,t)|(i,t.phase,t.started))
    }
    pub fn unbuilt(&self, building: usize) -> bool {
        self.traps.iter().any(|t|t.building==building && t.phase==0)
    }
    pub fn append(
        &self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
    ) -> (usize, usize) {
        for view in self.views.iter().chain(self.traps.iter().map(|t| &t.view)) {
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
        for trap in self.traps.iter().filter(|t| t.phase == 0) {
            for recipe in &trap.pad_rows {
                let count = recipe[draw::REC_BONES] as usize;
                let source = recipe[draw::REC_BONE_AT] as usize;
                if records >= draw::MAX_RECORDS || bones + count > draw::MAX_BONES { continue; }
                let mut row = *recipe;
                row[draw::REC_BONE_AT] = bones as f64;
                bridge.r.core.bones[bones*16..(bones+count)*16].copy_from_slice(&trap.pad_bones[source*16..(source+count)*16]);
                let at = draw::IN_RECORDS + records*draw::REC_STRIDE;
                bridge.r.core.input[at..at+draw::REC_STRIDE].copy_from_slice(&row);
                records += 1; bones += count;
            }
        }
        (records, bones)
    }
}
