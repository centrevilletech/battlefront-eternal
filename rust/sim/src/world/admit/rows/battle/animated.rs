//! The animated props (props.js AnimatedProp: Tatooine's sarlacc tentacles,
//! Bespin's doors) as F4Live births them (f4-live.js bindAnimated) and as
//! LiveObjects constructed them: the model's bone rig placed by the
//! instance, its clips, triggers and grabs from the instance's animation
//! record, the native pose accumulators the page's Pose made, and the
//! grip classes every soldier class binds to each of them (prepareGrips).
use super::super::super::{spawn, AdmitError, Stage};
use super::super::{Call, Cx};
use crate::gen::{F4AnimationSetup, F4AnimatedBirth, F4GripClass, F4Handle, PaBind, PaBone, PaClip, PaGrab, PaPose, PaSettings, PaState, PaTrigger};
use crate::store::{DefId, Ref};
use crate::world::owner::World;
use bfe_content::{clips::Clip, js::to_number, value::Value};
use three::{Matrix4, TransformNode, TransformTree, Vector3};

const STAGE: Stage = Stage::LiveTail;
fn error(message: impl Into<String>) -> AdmitError { AdmitError::Content { stage: STAGE, message: message.into() } }

/// The instances LiveObjects turns into AnimatedProps, in prop order:
/// `animatedprop` and `door` bases whose model exists (props.js 1053).
pub fn instances(cx: &Cx) -> Vec<usize> {
    let Some(data) = cx.battle.props.as_ref() else { return Vec::new() };
    let models = data.get("models");
    data.get("instances").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate().filter(|(_, desc)| {
        matches!(desc.get("base").and_then(Value::as_str), Some("animatedprop" | "door"))
            && desc.get("model").and_then(Value::as_str).and_then(|m| models.and_then(|ms| ms.get(m))).is_some_and(Value::truthy)
    }).map(|(i, _)| i).collect()
}

/// The pose the page's `new Pose(bones, root)` made for the prop at its
/// construction (anim-kernel.js newPose): the bone count and the dummy
/// root's index, 0 when the rig has none. Called by the prop constructor
/// before the prop's body, as LiveObjects does; the id is kept on
/// `cx.ids.animated_poses` for the birth.
pub fn pose(world: &mut World, cx: &mut Cx, instance: usize) -> Result<(u32, Vec<(Stage, &'static str, Call)>), AdmitError> {
    let (count, root) = {
        let model = model(cx, instance)?;
        let bones = model.get("bones").and_then(Value::as_array).ok_or_else(|| error("an animated prop has no bones"))?;
        let root = bones.iter().position(|b| {
            let name = b.get("name").and_then(Value::as_str).unwrap_or("").to_lowercase();
            name == "dummyroot" || name == "dummy_root"
        }).unwrap_or(0);
        (bones.len(), root)
    };
    let id = world.pose.pose_new(count as u32, root as i32);
    cx.returned.push(("pose_new", id as f64));
    cx.ids.animated_poses.push((instance, id));
    // Pose.refresh's accumulator views are reads of module memory the
    // host repeats only when memory grew: no call of the sequence's.
    let calls = vec![(Stage::Fleet, "pose_new", Call { args: vec![count as f64, root as f64], ..Default::default() })];
    Ok((id, calls))
}

fn model<'a>(cx: &'a Cx, instance: usize) -> Result<&'a Value, AdmitError> {
    let data = cx.battle.props.as_ref().ok_or_else(|| error("no props"))?;
    let desc = data.get("instances").and_then(|i| i.at(instance)).ok_or_else(|| error("an animated prop names no instance"))?;
    let name = desc.get("model").and_then(Value::as_str).ok_or_else(|| error("an animated prop has no model"))?;
    data.get("models").and_then(|m| m.get(name)).ok_or_else(|| error(format!("animated prop model {name} is missing")))
}

/// f4-live.js `clip`: a clip's frames, rate, root motion flag, track bones
/// and sampled data, as the extractor wrote them.
fn pa_clip(clip: &Clip) -> PaClip {
    PaClip { frames: clip.frames_f64(), fps: clip.fps, root_motion: clip.root_motion, tracks: clip.tracks.clone(),
        data: clip.data().iter().map(|v| f64::from(*v)).collect() }
}
/// props.js `phase(index)`: a fixed point of the idle for each prop.
fn phase(index: usize) -> f64 { ((((index as u64 + 1) * 2654435761) & 0xffff_ffff) as f64) / 4294967296.0 }
/// props.js `length(clip)`: seconds, 0 without a clip.
fn length(clip: Option<&Clip>) -> f64 { clip.map_or(0.0, |c| c.frames_f64() / c.fps) }
fn m16(m: &Matrix4) -> [f64; 16] { m.elements }

/// AnimatedProp's construction and F4Live.bindAnimated's birth row for the
/// prop at `instance`, the `index`-th animated prop: the rig from the
/// model's bones under the placed root (buildRig), the clips by slot name,
/// the triggers' world points, the grabs, the stand pose, and the pose
/// accumulators as the kernel holds them now.
pub fn birth(world: &mut World, cx: &mut Cx, instance: usize, index: usize, pose_id: u32, body: Option<Ref<crate::gen::Body>>, interns: &mut Call) -> Result<F4AnimationSetup, AdmitError> {
    let data = cx.battle.props.clone().ok_or_else(|| error("no props"))?;
    let desc = data.get("instances").and_then(|i| i.at(instance)).ok_or_else(|| error("an animated prop names no instance"))?.clone();
    let model = model(cx, instance)?.clone();
    let model_name = desc.get("model").and_then(Value::as_str).unwrap_or("");
    let records = model.get("bones").and_then(Value::as_array).ok_or_else(|| error("an animated prop has no bones"))?;
    let placement: [f64; 16] = desc.get("matrix").and_then(Value::as_array).filter(|m| m.len() == 16)
        .map(|m| std::array::from_fn(|i| to_number(m.get(i)))).ok_or_else(|| error("an animated prop has no placement matrix"))?;
    // buildRig: a bone per record, decomposed from its local matrix, under
    // its parent or the placed root.
    let mut tree = TransformTree::new();
    let root = tree.add_node(TransformNode::default());
    let mut bones = Vec::with_capacity(records.len()); let mut names = Vec::with_capacity(records.len()); let mut parents = Vec::with_capacity(records.len());
    for record in records {
        let local: [f64; 16] = record.get("local").and_then(Value::as_array).filter(|m| m.len() == 16)
            .map(|m| std::array::from_fn(|i| to_number(m.get(i)))).ok_or_else(|| error("an animated prop bone has no local matrix"))?;
        let mut node = TransformNode::default(); node.decompose_from(&Matrix4 { elements: local });
        bones.push(tree.add_node(node));
        names.push(record.get("name").and_then(Value::as_str).unwrap_or("").to_lowercase());
        let p = to_number(record.get("parent"));
        if p >= 0.0 && (p.fract() != 0.0 || p >= records.len() as f64) { return Err(error("an animated prop bone has an invalid parent")); }
        parents.push(if p >= 0.0 { p } else { -1.0 });
    }
    for (i, &p) in parents.iter().enumerate() { tree.add_child(if p < 0.0 { root } else { bones[p as usize] }, bones[i]); }
    { let node = tree.node_mut(root); node.matrix.elements = placement; node.matrix_auto_update = false; }
    // LiveObjects.look adds the placed root to the scene: its world matrix
    // is the identity parent's product, which turns a placed -0 into +0.
    let scene = tree.add_node(TransformNode::default());
    tree.add_child(scene, root);
    tree.update_matrix_world(scene, true);
    let position = [placement[12], placement[13], placement[14]];
    // Object.fromEntries keeps the last bone of a repeated name.
    let named = |name: Option<&Value>| -> Option<usize> {
        let text = match name { Some(Value::String(s)) => s.to_string(), Some(Value::Number(n)) => bfe_content::js::number_to_string(*n), _ => String::new() };
        names.iter().rposition(|n| *n == text.to_lowercase())
    };
    let anim = desc.get("animation").cloned().unwrap_or(Value::Null);
    let door = desc.get("base").and_then(Value::as_str) == Some("door");
    let clips = cx.content.prop_clips.iter().find(|(name, _)| name == model_name).and_then(|(_, clips)| clips.as_ref());
    let slot_names: Vec<String> = match anim.get("clips").and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        None => [anim.get("idle"), anim.get("trigger").and_then(|t| t.get("action"))].into_iter().flatten().filter(|v| v.truthy()).filter_map(|v| v.as_str().map(str::to_string)).collect(),
    };
    let slots: Vec<Option<&Clip>> = slot_names.iter().map(|name| clips.and_then(|c| c.get(name))).collect();
    let idle = slots.first().copied().flatten();
    let slot_of = |action: Option<&Value>| -> f64 { action.and_then(Value::as_str).and_then(|a| slot_names.iter().position(|n| n == a)).map_or(-1.0, |i| i as f64) };
    let trigger_list: Vec<Value> = match anim.get("triggers").and_then(Value::as_array) {
        Some(list) => list.to_vec(),
        None => anim.get("trigger").filter(|t| t.truthy()).cloned().into_iter().collect(),
    };
    let mut triggers = Vec::with_capacity(trigger_list.len());
    for trigger in &trigger_list {
        let mut point = Vector3::new(position[0], position[1], position[2]);
        if let Some(node) = named(trigger.get("node")) { tree.get_world_position(bones[node], &mut point); }
        let sound = trigger.get("sound").filter(|s| s.truthy()).and_then(Value::as_str).map(|s| cx.intern(interns, s));
        triggers.push(PaTrigger { slot: slot_of(trigger.get("action")), point: [point.x, point.y, point.z],
            radius: to_number(trigger.get("radius")), sound, opens: trigger.get("action").and_then(Value::as_str) == Some("open") });
    }
    let door_clip = if door { clips.and_then(|c| c.get("open")).or(idle) } else { None };
    let mut grabs = Vec::new();
    for grab in anim.get("attach").and_then(Value::as_array).unwrap_or(&[]) {
        let clip = grab.get("clip").and_then(Value::as_str).ok_or_else(|| error("an animated prop grab names no clip"))?;
        grabs.push(PaGrab { slot: slot_of(grab.get("action")), window: to_number(grab.get("window")), radius: to_number(grab.get("radius")),
            node: named(grab.get("node")).map(|i| i as f64), clip: cx.intern(interns, clip) });
    }
    let kill_sound = anim.get("killSound").filter(|s| s.truthy()).and_then(Value::as_str).map(|s| cx.intern(interns, s));
    let idle_delay = anim.get("idleDelay").filter(|v| v.truthy()).map(|v| to_number(Some(v))).unwrap_or(0.0);
    let bind: Vec<PaBind> = bones.iter().map(|&b| { let n = tree.node(b); PaBind { q: [n.quaternion.x, n.quaternion.y, n.quaternion.z, n.quaternion.w], p: [n.position.x, n.position.y, n.position.z] } }).collect();
    let settings = PaSettings { door, clips: slots.iter().map(|c| c.map(pa_clip)).collect(), door_clip: door_clip.map(pa_clip),
        door_duration: door_clip.map_or(0.0, |c| c.duration), idle_delay, open_slots: slot_names.iter().map(|n| n == "open").collect(),
        triggers, grabs, position, kill_sound, root: placement, parents, bind };
    // The stand pose from bind, then the clocks: the idle starts at the
    // prop's own point unless the class says fromStart.
    let mut stand_q = Vec::with_capacity(bones.len() * 4); let mut stand_t = Vec::with_capacity(bones.len() * 3);
    for &b in &bones { let n = tree.node(b); stand_q.extend([n.quaternion.x as f32 as f64, n.quaternion.y as f32 as f64, n.quaternion.z as f32 as f64, n.quaternion.w as f32 as f64]); stand_t.extend([n.position.x as f32 as f64, n.position.y as f32 as f64, n.position.z as f32 as f64]); }
    let time = if anim.get("fromStart").is_some_and(Value::truthy) { 0.0 } else { phase(index) * length(idle) };
    let pose = {
        let q: Vec<f64> = crate::pose::pose_q_in(&mut world.pose, pose_id).iter().map(|v| f64::from(*v)).collect();
        let t: Vec<f64> = crate::pose::pose_t_in(&mut world.pose, pose_id).iter().map(|v| f64::from(*v)).collect();
        let w: Vec<f64> = crate::pose::pose_w_in(&mut world.pose, pose_id).iter().map(|v| f64::from(*v)).collect();
        // Pose.root is the dummy root's index, 0 without one.
        let root_index = names.iter().position(|n| n == "dummyroot" || n == "dummy_root").unwrap_or(0) as f64;
        Some(PaPose { root: root_index, q, t, w })
    };
    let bone_rows: Vec<PaBone> = bones.iter().map(|&b| { let n = tree.node(b); PaBone { p: [n.position.x, n.position.y, n.position.z],
        q: [n.quaternion.x, n.quaternion.y, n.quaternion.z, n.quaternion.w], s: [n.scale.x, n.scale.y, n.scale.z], world: m16(&n.matrix_world) } }).collect();
    // `p.body?.enabled ?? null`: CollisionWorld.addBody enables a body and
    // nothing toggles an animated prop's before its birth.
    let body_enabled = body.map(|_| true);
    let state = PaState { index: index as f64, slot: -1.0, time, weight: 1.0, r#loop: false, hold: 0.0, started: 0.0, start_time: time,
        victim: None, openness: 0.0, body_enabled, stand_q, stand_t, pose, root_world: m16(&tree.node(root).matrix_world), bones: bone_rows };
    Ok(F4AnimationSetup { prop: Ref::new(index as u32, 1), body, birth: F4AnimatedBirth { settings, state } })
}

/// F4Live.prepareGrips for one soldier class and one animated prop: the
/// clip each grab names in the class's shared bank, as the kernel numbers
/// it (one id per clip object, registered here when the bank's clip is
/// new), null where the bank lacks it.
pub fn grip(world: &mut World, cx: &mut Cx, class_name: &str, def: DefId, handle: &F4Handle, grabs: &[String], calls: &mut Vec<(Stage, &'static str, Call)>) -> Result<F4GripClass, AdmitError> {
    let parts = super::super::super::bodies::prepare::parts(cx, class_name)?;
    let bank_key = spawn::clip_bank_key(&parts, 0);
    let mut clips = Vec::with_capacity(grabs.len());
    for grab in grabs {
        // The class's animator bank: a droideka's walker clips, else the
        // soldier bank (a missing clip grips as null).
        clips.push(match parts.walker.unwrap_or(parts.shared).get(grab) {
            Some(clip) => Some(spawn::register_clip(world, cx, clip, &bank_key, calls)?),
            None => None,
        });
    }
    Ok(F4GripClass { def, animated: handle.clone(), clips })
}

/// The grab clip names of a prop, in grab order, for the grips.
pub fn grab_clips(cx: &Cx, instance: usize) -> Vec<String> {
    cx.battle.props.as_ref().and_then(|p| p.get("instances")).and_then(|i| i.at(instance)).and_then(|d| d.get("animation"))
        .and_then(|a| a.get("attach")).and_then(Value::as_array).unwrap_or(&[]).iter()
        .filter_map(|g| g.get("clip").and_then(Value::as_str).map(str::to_string)).collect()
}

