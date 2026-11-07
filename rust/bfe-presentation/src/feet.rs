//! Visual Footfalls.update/fall/place and cycleHeights from vehicle-feet.js.
//! Immutable clips are sampled at resource birth. Frames only advance fixed
//! detectors and publish the authored primitive at the displayed matrix.
use crate::layout::{foot_frame as f,foot_input as i,foot_event as o,foot_limits as lim};
use audio_control::feet::{Foot,State};
use bfe_pose::{Clip,Pose};
use serde::Deserialize;
use three::{Vector3,Quaternion,Matrix4,TransformTree,TransformNode};

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Bone { parent:i32,scale:[f64;3],position:[f64;3],quaternion:[f64;4] }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Animation {frames:usize,fps:f64,root_motion:bool,tracks:Vec<i32>,data:Vec<f32>}
#[derive(Deserialize)]
pub struct Primitive {bone:i32,point:[f64;3],turn:Option<[f64;4]>}
#[derive(Deserialize)]
pub struct Content {bones:Vec<Bone>,root:i32,clips:Vec<Animation>,pub feet:Vec<Primitive>,length:f64,
    detection:bool,threshold:f64,hydraulic:f64,centred:bool,rate:f64}

// cycleHeights deliberately samples a separate tree. Its root is identity,
// its feet retain authored scales, and unused clip tracks never move it.
pub fn heights(content:&Content)->Vec<Vec<f64>> {
    let mut result=vec![Vec::new();content.feet.len()];
    if content.length<=0.0||content.clips.len()!=2||content.bones.is_empty(){return result;}
    let mut tree=TransformTree::new();let root=tree.add_node(TransformNode::default());
    let nodes:Vec<_>=content.bones.iter().map(|b|tree.add_node(TransformNode::placed(Vector3::default(),v(b.scale)))).collect();
    for (k,b) in content.bones.iter().enumerate(){tree.add_child(if b.parent>=0{nodes.get(b.parent as usize).copied().unwrap_or(root)}else{root},nodes[k]);}
    // A foot only depends on its ancestors; unrelated tracks must leave
    // their binds untouched, including clips with missing authored bones.
    let mut chain=vec![false;nodes.len()];
    for foot in &content.feet {let mut bone=foot.bone;let mut remaining=nodes.len();
        while bone>=0&&remaining>0 {let Some(b)=content.bones.get(bone as usize)else{break};chain[bone as usize]=true;bone=b.parent;remaining-=1;}}
    // Each authored clip excludes its repeated last key. The sampled f32
    // heights must round at the same point as cycleHeights' typed arrays.
    let mut pose=Pose::new(nodes.len(),content.root);
    for animation in &content.clips {
        let clip=Clip{frames:animation.frames,fps:animation.fps,root_motion:animation.root_motion,
            tracks:animation.tracks.iter().map(|&k|if k>=0&&chain.get(k as usize)==Some(&true){k}else{-1}).collect(),data:animation.data.clone()};
        for frame in 0..animation.frames.saturating_sub(1) {
            for (k,b) in content.bones.iter().enumerate(){let n=tree.node_mut(nodes[k]);n.position=v(b.position);n.set_quaternion(b.quaternion[0],b.quaternion[1],b.quaternion[2],b.quaternion[3]);}
            pose.clear();pose.add(&clip,frame as f64/clip.fps,1.0,false);pose.apply_out();
            for (k,&node) in nodes.iter().enumerate(){if pose.w[k]<=1e-6{continue;}let p=&pose.out[k*7..];let n=tree.node_mut(node);n.set_quaternion(p[0],p[1],p[2],p[3]);n.position=Vector3::new(p[4],p[5],p[6]);}
            tree.update_matrix_world(root,true);
            for (k,foot) in content.feet.iter().enumerate(){let node=if foot.bone>=0{nodes.get(foot.bone as usize).copied().unwrap_or(root)}else{root};let mut p=v(foot.point);p.apply_matrix4(&tree.node(node).matrix_world);result[k].push((p.y as f32)as f64);}
        }
    }
    result
}
fn v(p:[f64;3])->Vector3 {Vector3::new(p[0],p[1],p[2])}
// The hull phase is shared by its feet. Arming and fallback descent state
// are independent per primitive and reset on a new resource generation.
struct Body {start:usize,count:usize,phase:Option<f64>,walking:bool,generation:Option<f64>}
struct Detector {definition:Foot,state:State,turn:Option<Quaternion>}
pub struct Feet {bodies:Vec<Body>,detectors:Vec<Detector>,pub input:Vec<f64>,pub output:Vec<f64>}
impl Default for Feet {fn default()->Self{Self{bodies:Vec::new(),detectors:Vec::new(),input:vec![0.0;f::STRIDE],output:Vec::new()}}}
impl Feet {
    /// The returned first foot remains stable for the resource's lifetime.
    /// An excessive authored class is bounded at birth, never during a draw.
    pub fn add(&mut self,content:Content)->usize {
        let samples=heights(&content);let start=self.detectors.len();
        for (k,foot) in content.feet.iter().take(lim::PER_BODY).enumerate(){self.detectors.push(Detector{
            definition:Foot::new(foot.point,&samples[k],content.length,content.detection,content.threshold,content.hydraulic,content.centred,content.rate),
            state:State::default(),turn:foot.turn.map(|q|Quaternion::new(q[0],q[1],q[2],q[3]))});}
        self.bodies.push(Body{start,count:self.detectors.len()-start,phase:None,walking:false,generation:None});
        self.input.resize(f::STRIDE+self.detectors.len()*i::STRIDE,0.0);self.output.resize(self.detectors.len()*o::STRIDE,0.0);start
    }
    /// One block spans hulls and placed objects, in effect draw order. A foot
    /// can plant several times over a long frame; publish the multiplicity
    /// at the same drawn point without allocating an event for each crossing.
    pub fn frame(&mut self){let dt=self.input[f::DT];self.output.fill(0.0);
        for body in &mut self.bodies {
            if body.count==0{continue;}let row=&self.input[f::STRIDE+body.start*i::STRIDE..];if row[i::PRESENT]==0.0{continue;}
            if body.generation!=Some(row[i::GENERATION]){body.phase=None;body.walking=false;body.generation=Some(row[i::GENERATION]);}
            let phase=row[i::PHASE];let walking=row[i::BLEND]>0.01;
            for k in body.start..body.start+body.count {
                let x=&self.input[f::STRIDE+k*i::STRIDE..];let d=&mut self.detectors[k];d.state.reset_generation(x[i::GENERATION]);
                let mut matrix=Matrix4::default();matrix.elements.copy_from_slice(&x[i::MATRIX..i::MATRIX+16]);let mut point=v(d.definition.point);point.apply_matrix4(&matrix);
                // Stable crossing order comes from the shared audio detector.
                // A frame needs only down-event multiplicity for its one point.
                let mut events=[0u8;lim::EVENTS];let count=d.definition.events(&mut d.state,body.phase,body.walking,phase,walking,point.y,dt,&mut events);
                let out=&mut self.output[k*o::STRIDE..(k+1)*o::STRIDE];out[o::COUNT]=events[..count].iter().filter(|&&kind|kind==1).count()as f64;
                out[o::POSITION..o::POSITION+3].copy_from_slice(&[point.x,x[i::ROOT_Y],point.z]);
                // Authored primitive turns follow the full displayed matrix,
                // including parent scale, before their local turn is applied.
                if let Some(turn)=d.turn {let mut q=Quaternion::default();matrix.decompose(&mut Vector3::default(),&mut q,&mut Vector3::default());q.multiply(&turn);out[o::TURN..o::TURN+4].copy_from_slice(&[q.x,q.y,q.z,q.w]);out[o::HAS_TURN]=1.0;}
            }
            body.phase=Some(phase);body.walking=walking;
        }
    }
}
