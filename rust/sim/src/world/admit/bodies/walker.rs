use super::{build::number, error};
use crate::gen::{BonePose, RigAnimator};
use crate::rig::{droideka::{self, Barrel, DroidekaClips, DroidekaRig, DroidekaState, Gun, SkinnedBody}, skeleton::{BoneDesc, Skeleton}};
use crate::world::admit::AdmitError;
use bfe_content::{actor::Actor, clips::Clips, derived::DroidekaGun, Value};
use three::{EulerOrder, Matrix4};

/// UnitMuzzles.attach's existing variable-width rig record. It is a cold
/// serialization of a prepared TransformTree, never a tick-time pose upload.
#[derive(Clone, Debug)]
pub struct Muzzles { pub words: Vec<f64> }

pub(super) fn build(actor: &Actor, clips: &Clips, guns: &[DroidekaGun], def: &Value)
    -> Result<(Muzzles, RigAnimator), AdmitError>
{
    if actor.skin_index.len() != actor.vertex_count() * 4 {
        return Err(error("droideka actor has an invalid skin index"));
    }
    let skeleton = Skeleton::actor(&actor.bones.iter().map(|bone| BoneDesc {
        name: bone.name.clone(), parent: bone.parent as i32, local: bone.local,
    }).collect::<Vec<_>>());
    let bank = clips.list.iter().map(|c| crate::pose::Clip {
        frames: c.frames_f64() as usize, fps: c.fps, root_motion: c.root_motion,
        tracks: c.tracks.iter().map(|&t| t as i32).collect(), data: c.data().to_vec(),
    }).collect::<Vec<_>>();
    let find = |name: &str| clips.list.iter().position(|c| c.name == name).map(|i| &bank[i]);
    let modes = DroidekaClips { by_mode: droideka::CLIPS.map(|name| name.and_then(find)) };
    let inverses = actor.bones.iter().map(|b| Matrix4 { elements: b.inverse.elements }).collect::<Vec<_>>();
    let skin = SkinnedBody { positions: &actor.positions, skin_index: &actor.skin_index,
        bind_matrix: Matrix4 { elements: actor.bind_matrix.elements }, bone_inverses: &inverses };
    let (_, center) = droideka::roll_axle(&skeleton, &skin, find("roll"), number(def, "BallRadius", 0.65));
    let guns = guns.iter().map(|gun| Gun { bone: gun.bone,
        barrels: gun.tips.as_ref().map_or_else(|| vec![Barrel::GunBone], |tips| tips.iter()
            .map(|p| Barrel::Marker(three::Vector3::new(p.x, p.y, p.z))).collect()) }).collect::<Vec<_>>();
    if guns.iter().any(|g| g.bone >= actor.bones.len()) { return Err(error("droideka gun has no bone")); }
    let mut rig = DroidekaRig::new(&skeleton, &guns, center);
    // Droideka.arm calls sampleMuzzles(0) before its shield is installed.
    // Initial clip/Float32 pose rounding therefore precedes F3 adoption.
    rig.sample_muzzles(0.0, &DroidekaState { speed: number(def, "AnimationSpeed", 1.0), ..Default::default() }, &modes);
    let a = &rig.animator;
    let animator = RigAnimator { facing: a.facing, legs: a.legs, mode: a.mode as f64,
        went_up_rolled: a.went_up_rolled, roll_origin: a.roll_origin, transition: a.transition,
        from: a.from_p.iter().zip(&a.from_q).map(|(p, q)| BonePose {
            p: [p.x, p.y, p.z], q: [q.x, q.y, q.z, q.w] }).collect(),
        pivot: [a.pivot.x, a.pivot.y, a.pivot.z], roll_center: [a.roll_center.x, a.roll_center.y, a.roll_center.z] };
    Ok((encode(&mut rig), animator))
}

// UnitMuzzles.attach and units/muzzles.rs define this existing ungenerated
// tree format: counts, bind poses, nodes, then the three pose arrays. Keep
// the writer next to construction; ordinary world ticks never encode it.
fn encode(rig: &mut DroidekaRig) -> Muzzles {
    const HEADER_WORDS: usize=4;
    const BIND_WORDS: usize=3+4;
    const NODE_WORDS: usize=1+3+4+3+4+16+16+3;
    const POSE_WORDS: usize=4+3+1;
    let a = &rig.animator;
    let n = rig.bones.len();
    let nodes=1+n+rig.barrels.iter().map(Vec::len).sum::<usize>();
    let total=HEADER_WORDS+a.spine.len()+1+rig.barrels.len()+n*(BIND_WORDS+POSE_WORDS)+nodes*NODE_WORDS;
    let mut words=Vec::with_capacity(total);
    words.extend([n as f64,a.root as f64,a.ball as f64,a.spine.len() as f64]);
    words.extend(a.spine.iter().map(|&i| i as f64));
    words.push(rig.barrels.len() as f64);
    words.extend(rig.barrels.iter().map(|b| b.len() as f64));
    for (p, q) in a.bind_p.iter().zip(&a.bind_q) { words.extend([p.x, p.y, p.z, q.x, q.y, q.z, q.w]); }
    // Rig construction inserts the root, bones and physical barrels in this
    // order, just as UnitMuzzles' nodes array. Parent ids name this order.
    for id in 0..nodes {
        let node = rig.tree.node_mut(id as u32);
        let e = node.rotation();
        let order = match e.order { EulerOrder::XYZ => 0, EulerOrder::YXZ => 1, EulerOrder::ZXY => 2,
            EulerOrder::ZYX => 3, EulerOrder::YZX => 4, EulerOrder::XZY => 5 };
        words.push(node.parent.map_or(-1.0, |p| p as f64));
        words.extend([node.position.x, node.position.y, node.position.z,
            node.quaternion.x, node.quaternion.y, node.quaternion.z, node.quaternion.w,
            node.scale.x, node.scale.y, node.scale.z, e.x, e.y, e.z, order as f64]);
        words.extend(node.matrix.elements);
        words.extend(node.matrix_world.elements);
        words.extend([u8::from(node.matrix_auto_update) as f64,
            u8::from(node.matrix_world_auto_update) as f64, u8::from(node.matrix_world_needs_update) as f64]);
    }
    words.extend(a.pose.q.iter().map(|&v| v as f64));
    words.extend(a.pose.t.iter().map(|&v| v as f64));
    words.extend(a.pose.w.iter().map(|&v| v as f64));
    debug_assert_eq!(words.len(),total,"hidden muzzle rig width");
    Muzzles { words }
}
