//! Cold numeric construction of Vehicle's retained rig and seatView. Hosts
//! instantiate resource shells from these records; no browser rig math is
//! needed before the first native presentation frame.
use super::{content::hull_records, hull::HullRig};
use bfe_content::{json, odf::{num, setting, split_space, to_text}, value::Value};
use serde::Deserialize;
use serde_json::{json as record, Value as Record};
use three::{TransformNode, Vector3};
const DEG: f64 = std::f64::consts::PI / 180.0;
const PI: f64 = std::f64::consts::PI;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
// This descriptor exists only during resource creation. Strings and vectors
// resolve once; displayed frames carry numeric handles and fixed records.
struct Input {
    class: Record,
    rig: Record,
    muzzle_tips: Vec<(String, [f64; 3])>,
    animated: Vec<usize>,
    has_clips: bool,
    turn_rate: f64,
    #[serde(default)]
    wheel_radii: Vec<(String,f64)>,
    #[serde(default)]
    bounds: [f64;6],
}
// JSON records preserve f64 values, including signed zero. Only the final
// vertex attributes below round to their original Float32Array storage.
fn xyz(p: Vector3) -> [f64; 3] { [p.x, p.y, p.z] }
fn pose(n: &TransformNode) -> [f64; 10] {
    [n.position.x,n.position.y,n.position.z,n.quaternion.x,n.quaternion.y,n.quaternion.z,n.quaternion.w,n.scale.x,n.scale.y,n.scale.z]
}
// seatView accepts three finite components and ignores any trailing ones.
// Missing or malformed vectors retain the section-specific fallback.
fn vector(value: Option<&Value>) -> Option<[f64; 3]> {
    let value = value.filter(|v| !v.is_null())?;
    let text = to_text(Some(value));
    let parts: Vec<f64> = split_space(&text).iter().map(|p| bfe_content::js::string_to_number(p)).collect();
    (parts.len() >= 3 && parts[..3].iter().all(|v| v.is_finite())).then(|| [parts[0],parts[1],parts[2]])
}
// Limits use the first two finite components. The degree conversion stays
// outside parsing so every caller keeps the JavaScript operation order.
fn pair(value: Option<&Value>) -> Option<[f64; 2]> {
    let text = to_text(Some(value.filter(|v| v.truthy())?));
    let parts: Vec<f64> = split_space(&text).iter().map(|p| bfe_content::js::string_to_number(p)).collect();
    (parts.len() >= 2 && parts[..2].iter().all(|v| v.is_finite())).then(|| [parts[0],parts[1]])
}
/// seatView's camera parser also honors hashed tension keys, just as
/// flight/camera.js cameraTension does for the flyer control section.
fn view(s: Option<&Value>, index: usize, flying: bool, turn_rate: f64) -> Record {
    let key = |k: &str| s.and_then(|s| s.get(k));
    // EyePointOffset seeds TrackCenter; an explicit center then wins. Camera
    // offsets remain in their authored frame until the camera places them.
    let eye = vector(key("EyePointOffset")).unwrap_or([0.0;3]);
    let center = vector(key("TrackCenter")).unwrap_or(eye);
    let offset = vector(key("TrackOffset")).unwrap_or([0.0;3]);
    let rate = |k: &str| num(setting(s, k), 30.0);
    // MoveTension seeds all six directions. Each axis may replace one pair;
    // a single component means the same rate in both directions.
    let mut tension = [rate("MoveTension");6];
    for (axis, name) in ["MoveTensionX","MoveTensionY","MoveTensionZ"].iter().enumerate() {
        if let Some(value) = setting(s, name) {
            let text = to_text(Some(value));
            let values: Vec<f64> = split_space(&text).iter().map(|p| bfe_content::js::string_to_number(p)).collect();
            if let Some(&first) = values.first().filter(|v| v.is_finite()) {
                tension[axis*2] = first;
                tension[axis*2+1] = values.get(1).copied().filter(|v| v.is_finite()).unwrap_or(first);
            }
        }
    }
    // Body pitch is up positive. Turret sections author it down positive,
    // and a full-circle declaration leaves the engine's default bound.
    let pitch = pair(key("PitchLimits")).map(|p| p.map(|p| p*DEG));
    let pitch = if index == 0 { if flying { [-PI,PI] } else { pitch.unwrap_or([-0.5,0.5]) } }
        else { pitch.filter(|p| p[0] > -PI && p[1] < PI).map(|p| [-p[1],-p[0]]).unwrap_or([-1.5,1.5]) };
    let yaw = pair(key("YawLimits")).map(|p| p.map(|p| p*DEG));
    // Back-facing yaw arcs keep 0..2pi, instead of wrapping through the
    // front. Other complete arcs remain unlimited, as seatView did.
    let wrap = index > 0 && yaw.is_some_and(|y| y[0]>0.0 && y[0]<=PI && y[1]>=PI && y[1]<2.0*PI);
    let yaw = if index==0 {None} else {yaw.filter(|y| wrap || y[0]>-PI+1e-4 && y[1]<PI-1e-4)};
    record!({"eye":eye,"center":center,"offset":offset,"tilt":num(key("TiltValue"),0.0)*DEG,
        "aimTension":rate("AimTension"),"moveTension":tension,
        "firstFov":num(key("FirstPersonFOV"),48.0)*DEG,"thirdFov":num(key("ThirdPersonFOV"),48.0)*DEG,
        "yawFactor":num(key("YawTurnFactor"),1.0),"pitchFactor":num(key("PitchTurnFactor"),1.0),
        "turnRate":num(key("TurnRate"),if index>0 {2.0} else {turn_rate}),"pitchLimits":pitch,"yawLimits":yaw,"wrapYaw":wrap})
}
/// Turret's camera vectors follow authored property order, while tension
/// and lens parsing share seatView's numeric constructor.
pub fn turret_view(settings: Option<&Value>) -> Record {
    let mut result = view(settings, 1, false, 2.0);
    let key = |name: &str| settings.and_then(|s| s.get(name));
    let pitch = pair(key("PitchLimits")).unwrap_or([-45.0,45.0]);
    result["pitchLimits"] = record!([-pitch[1]*DEG, -pitch[0]*DEG]);
    result["turnRate"] = record!(num(key("MaxTurnSpeed"),2.0));
    result["filter"] = record!(num(key("TurnFilter"),10.0));
    result["eye"] = record!([0.0, 0.0, 0.0]);
    result["center"] = record!([0.0, 0.0, 0.0]);
    result["offset"] = record!([0.0, 0.0, 0.0]);
    if let Some(settings) = settings.and_then(Value::as_object) {
        for (key, value) in settings.entries() {
            if !matches!(key, "EyePointOffset" | "TrackCenter" | "TrackOffset") { continue; }
            let Some(v) = vector(Some(value)) else { continue; };
            match key {
                "EyePointOffset" => { result["eye"] = record!(v); result["center"] = record!(v); }
                "TrackCenter" => result["center"] = record!(v),
                _ => result["offset"] = record!(v),
            }
        }
    }
    result
}
/// Vehicle constructor, its marker/muzzle ordering, and seatView. The HullRig
/// builder is shared with authority; these records include the host-facing
/// aim rates and camera fields authority has no reason to publish.
pub fn vehicle(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let input: Input = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let class = json::parse(&input.class.to_string()).map_err(|e| e.to_string())?;
    let model = json::parse(&input.rig.to_string()).map_err(|e| e.to_string())?;
    let desc = hull_records(&class, &model, &input.muzzle_tips, input.animated, input.has_clips);
    let mut rig = HullRig::new(&desc);
    // Node ids include the root and static bone before muzzle markers. The
    // terminal identity scene is native bookkeeping, not a resource shell.
    let nodes: Vec<Record> = rig.tree.nodes.iter().take(rig.scene as usize).enumerate().map(|(i,n)| {
        record!({"parent":n.parent.filter(|&p| p!=rig.scene).map(|p|p as i32).unwrap_or(-1),"pose":pose(n),
            "name":if i>0 && i<=desc.bones.len() {desc.bones[i-1].name.as_str()} else {""}})
    }).collect();
    // Seat ordering is authored ordering, including empty control sections.
    // It must agree with replicated seat indices and the weapons they own.
    let class_seats = class.get("seats").and_then(Value::as_array).unwrap_or(&[]);
    let seats: Vec<Record> = rig.seats.iter().enumerate().map(|(i,seat)| {
        let definition = class_seats.get(i);
        let mut view = view(definition.and_then(|s|s.get("settings")),i,desc.flying,input.turn_rate);
        view["turret"] = record!(seat.turret);
        view["pivot"] = record!(xyz(seat.pivot)); view["up"] = record!(xyz(seat.up));
        view["rider"] = seat.rider.as_ref().map(|r| record!({"anim":r.anim,"clip":r.clip,"ninePose":r.nine_pose,"node":r.node,"shootable":r.shootable})).unwrap_or(Record::Null);
        // Weapon state and effects remain host resources. Native records
        // connect their aimers, barrels and fire nodes without name scans.
        let definitions = definition.and_then(|s| s.get("weapons")).and_then(Value::as_array).unwrap_or(&[]);
        let weapons: Vec<Record> = seat.weapons.iter().enumerate().map(|(w,weapon)| {
            let definitions = definitions.get(w).and_then(|w|w.get("aimers")).and_then(Value::as_array).unwrap_or(&[]);
            let aimers:Vec<Record> = weapon.aimers.iter().enumerate().map(|(a,aim)| {
                let key = |k:&str| definitions.get(a).and_then(|a|a.get(k));
                // Aimer limits have their own defaults and sign convention;
                // a seat's view limits must not be substituted here.
                let yaw = pair(key("AimerYawLimits")).unwrap_or([-30.0,30.0]).map(|v|v*DEG);
                record!({"bone":aim.bone,"owner":aim.owner,"yawLimits":yaw,
                    "pitchLimits":pair(key("AimerPitchLimits")).unwrap_or([-45.0,45.0]).map(|v|v*DEG),
                    "wrapYaw":yaw[0]>0.0 && yaw[0]<=PI && yaw[1]>=PI && yaw[1]<2.0*PI,
                    "pitchRate":num(key("AimerPitchRate"),1.0),"yawRate":num(key("AimerYawRate"),2.0)})
            }).collect();
            let muzzles:Vec<Record> = weapon.muzzles.iter().map(|m| record!({"fireNode":m.fire_node,"sourceAimer":m.source_aimer,"barrel":m.barrel,"rest":m.rest.map(xyz),"recoil":m.recoil})).collect();
            record!({"aimers":aimers,"muzzles":muzzles,"fire":weapon.fire})
        }).collect();
        record!({"view":view,"weapons":weapons})
    }).collect();
    // Skeleton bindings use recomposed bone matrices, rather than the raw
    // authored locals used for rigid vertex packing. This retains Three's
    // decomposition and attached-mode rounding at the resource boundary.
    let inverses:Vec<[f64;16]>=rig.bones.iter().chain(std::iter::once(&rig.static_bone)).map(|&id| {
        let mut matrix=rig.tree.node(id).matrix_world;matrix.invert();matrix.elements
    }).collect();
    // Wheel rims use local Y as their axle. Mirrored imports reverse it;
    // the rest-pose world point gives the wheel's side of the hull.
    let mut wheels=Vec::new();
    for name in ["wheel_left","wheel_right"] {
        let Some(index)=rig.by_name.get(name) else {continue;};let bone=rig.bones[index];
        let mut turn=three::Quaternion::default();rig.tree.get_world_quaternion(bone,&mut turn);
        let mut axis=Vector3::new(0.0,1.0,0.0);axis.apply_quaternion(&turn);
        let side=v8math::sign(axis.x*1.0+axis.y*0.0+axis.z*0.0);let side=if side==0.0 || side.is_nan() {1.0} else {side};
        let mut at=Vector3::default();rig.tree.get_world_position(bone,&mut at);
        let radius=input.wheel_radii.iter().find(|(key,_)|key==name).map(|(_,r)|*r).filter(|r|*r!=0.0 && !r.is_nan()).unwrap_or(4.0);
        wheels.push(record!({"bone":bone,"axis":[0.0*side,side,0.0*side],"x":at.x,"radius":radius}));
    }
    // A walker's tow legs are rest-pose geometry, not a frame's animation.
    // Resolve the same named suffix groups once while the rig is at bind.
    let mut leg_top=input.bounds[1]+(input.bounds[4]-input.bounds[1])*0.6;
    let mut legs:Vec<Record>=Vec::new();
    let walking=class.get("base").and_then(Value::as_str).is_some_and(|base|base=="walker" || base=="commandwalker");
    if walking {
        let mut pelvis=None;
        for (name,index) in &rig.by_name.entries {
            if name.ends_with("pelvis") {
                let mut at=Vector3::default();rig.tree.get_world_position(rig.bones[*index],&mut at);
                pelvis=Some(pelvis.map_or(at.y,|old:f64|v8math::min(old,at.y)));
            }
            for suffix in ["thigh","calf","foot"] {
                let Some(prefix)=name.strip_suffix(suffix) else {continue;};
                let prefix=prefix.strip_suffix('_').unwrap_or(prefix);
                let row=if let Some(i)=legs.iter().position(|leg|leg["name"]==prefix) {i} else {
                    legs.push(record!({"name":prefix,"thigh":null,"calf":null,"foot":null}));legs.len()-1
                };
                let mut at=Vector3::default();rig.tree.get_world_position(rig.bones[*index],&mut at);
                legs[row][suffix]=record!(xyz(at));break;
            }
        }
        if let Some(height)=pelvis {leg_top=height;}
        legs.retain(|leg|!leg["calf"].is_null() || !leg["thigh"].is_null());
    }
    serde_json::to_vec(&record!({"nodes":nodes,"seats":seats,"staticBone":rig.static_bone,"poseRoot":rig.pose_root,
        "inverses":inverses,"wheels":wheels,"legs":legs,"legTop":leg_top})).map_err(|e|e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
// Attribute arrays are resource inputs. Their material fields only group
// parts; no material object or renderer handle enters the numeric builder.
struct Part {
    data: Vec<f32>,
    index: Vec<u32>,
    texture: Option<Record>,
    flags: Option<u32>,
    scroll: Option<Record>,
    bone: Option<String>,
    joints: Option<Vec<u16>>,
    skin_joints: Option<Vec<u16>>,
    skin_weights: Option<Vec<f32>>,
}
#[derive(Deserialize)]
struct BatchInput { rig: Record, parts: Vec<Part>, separate: Vec<String> }
struct Batch { first: usize, attach: Option<String>, parts: Vec<usize> }
/// vehicleBatches: material grouping, bind-frame vertices, normals and skin
/// weights. The cold result is already rounded to the typed GPU attributes;
/// hosts only allocate resource shells and choose the authored material.
pub fn batches(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use three::Matrix4;
    let input: BatchInput = serde_json::from_slice(bytes).map_err(|e|e.to_string())?;
    let model = json::parse(&input.rig.to_string()).map_err(|e|e.to_string())?;
    let bones = bfe_content::derived::rig_bones(&model);
    let mut world: Vec<Matrix4> = vec![Matrix4::default();bones.len()+1];
    // Authored locals are used directly for rigid vertices. Recomposition
    // here would introduce rounding before the GPU attribute conversion.
    let mut ready = vec![false;bones.len()];
    fn resolve(index:usize, bones:&[bfe_content::derived::RigBone], world:&mut [Matrix4], ready:&mut [bool]) {
        if ready[index] { return; }
        // Mark before climbing, so malformed cyclic parents cannot recurse
        // forever. Authored rigs have an acyclic skeleton.
        ready[index]=true;
        let parent=bones[index].parent as i32;
        let local=Matrix4 {elements:bones[index].local.elements};
        if parent>=0 && (parent as usize)<bones.len() {
            resolve(parent as usize,bones,world,ready);
            let matrix=world[parent as usize];
            world[index].multiply_matrices(&matrix,&local);
        } else {world[index]=local;}
    }
    for i in 0..bones.len() {resolve(i,&bones,&mut world,&mut ready);}
    let named = |name:Option<&str>| bones.iter().rposition(|b|b.name.to_lowercase()==name.unwrap_or("").to_lowercase());
    // A separate drum or gun owns every part below it, including parts on
    // unnamed descendants. Other parts stay attached to the model root.
    let attach = |part:&Part| {
        let mut bone=named(part.bone.as_deref());
        for _ in 0..bones.len() {
            let Some(index)=bone else {break};
            let name=bones[index].name.to_lowercase();
            if input.separate.contains(&name) {return Some(name);}
            let parent=bones[index].parent as i32;
            bone=(parent>=0 && (parent as usize)<bones.len()).then_some(parent as usize);
        }
        None
    };
    // First appearance fixes draw order and the representative material.
    // Equal textures with different flags, scrolling or parents stay apart.
    let mut groups:Vec<Batch>=Vec::new();
    for (i,p) in input.parts.iter().enumerate() {
        let node=attach(p);
        if let Some(group)=groups.iter_mut().find(|g| {
            let first=&input.parts[g.first];g.attach==node && first.texture==p.texture && first.flags==p.flags && first.scroll==p.scroll
        }) {group.parts.push(i);} else {groups.push(Batch {first:i,attach:node,parts:vec![i]});}
    }
    let mut result=Vec::new();
    for group in groups {
        let count:usize=group.parts.iter().map(|&p|input.parts[p].data.len()/8).sum();
        // Each group is packed once. The final arrays have the same scalar
        // rounding and fourth influence lane as the old typed buffers.
        let mut positions=Vec::<f32>::with_capacity(count*3);
        let mut normals=Vec::<f32>::with_capacity(count*3);
        let mut uv=Vec::<f32>::with_capacity(count*2);
        let mut joints=vec![0u16;count*4];let mut weights=vec![0f32;count*4];
        let mut indices=Vec::<u32>::new();let mut vertex=0;
        for index in group.parts {
            let p=&input.parts[index];let base=vertex;
            let joint=named(p.bone.as_deref()).unwrap_or(bones.len());
            let matrix=&world[joint];
            for (i,data) in p.data.chunks_exact(8).enumerate() {
                let mut position=Vector3::new(data[0] as f64,data[1] as f64,data[2] as f64);
                let mut normal=Vector3::new(data[3] as f64,data[4] as f64,data[5] as f64);
                if p.joints.is_none() {position.apply_matrix4(matrix);normal.transform_direction(matrix);}
                positions.extend(xyz(position).map(|v|v as f32));normals.extend(xyz(normal).map(|v|v as f32));
                uv.extend_from_slice(&data[6..8]);
                joints[vertex*4]=p.joints.as_ref().and_then(|j|j.get(i)).copied().unwrap_or(joint as u16);weights[vertex*4]=1.0;
                // Soft skins carry three authored influences; the fourth lane
                // remains zero as the original typed arrays did.
                if let Some(skin)=&p.skin_joints {
                    for k in 0..3 {
                        joints[vertex*4+k]=skin.get(i*3+k).copied().unwrap_or(0);
                        weights[vertex*4+k]=p.skin_weights.as_ref().and_then(|w|w.get(i*3+k)).copied().unwrap_or(0.0);
                    }
                }
                vertex+=1;
            }
            indices.extend(p.index.iter().map(|&i|(base as u32).wrapping_add(i)));
        }
        // Bounds use the packed f32 positions, just as computeBoundingSphere
        // did. The radius allowance keeps animated barrels and feet visible.
        let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];
        for p in positions.chunks_exact(3) {for k in 0..3 {lo[k]=lo[k].min(p[k] as f64);hi[k]=hi[k].max(p[k] as f64);}}
        let center=std::array::from_fn::<_,3,_>(|k|if count==0 {0.0} else {(lo[k]+hi[k])*0.5});
        let mut radius=0.0_f64;
        for p in positions.chunks_exact(3) {let dx=p[0] as f64-center[0];let dy=p[1] as f64-center[1];let dz=p[2] as f64-center[2];radius=radius.max(dx*dx+dy*dy+dz*dz);}
        let casts=input.parts[group.first].flags.unwrap_or(1)&0x84==0;
        result.push(record!({"part":group.first,"attach":group.attach,"casts":casts,"positions":positions,"normals":normals,"uv":uv,
            "joints":joints,"weights":weights,"indices":indices,"center":center,"radius":radius.sqrt()*1.35}));
    }
    serde_json::to_vec(&result).map_err(|e|e.to_string())
}
