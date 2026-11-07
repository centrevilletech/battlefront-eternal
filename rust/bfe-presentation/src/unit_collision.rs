//! Page replicas' Unit.updateSpheres cache. Registry queries refresh stale
//! admitted bodies in one native phase, then read retained shape resources.
//! The authority uses the same on-foot tables and broadphase arithmetic.
use crate::{layout::{unit_collision_input as i,unit_collision_output as o,unit_shape as s,unit_shape_kind as k,unit_collision_limits as lim},unit_content::Content,rider_sources::{Bindings,Source}};
use bfe_pose::{display::DisplayRig,rig::{body_shapes::{self,Shape,Kind},rider::{self,RiderSkeleton,RiderScratch,RiderPoints,RiderSample}}};
use three::{Vector3,Quaternion,Matrix4};
// Each actor retains its lazy stamp, sample scratch and maximum limb set.
// A query changes values in these stores and never grows their capacity.
struct Cache {binding:f64,generation:f64,tick:f64,skeleton:RiderSkeleton,scratch:RiderScratch,q:Vec<Quaternion>,p:Vec<Vector3>,points:RiderPoints,rider:[rider::HitShape;lim::SHAPES],shapes:[Shape;lim::SHAPES]}
#[derive(Default)]
pub struct Collision {rows:Vec<Cache>,pub actors:Vec<u32>,pub input:Vec<f64>,pub output:Vec<f64>,pub shapes:Vec<f64>}
impl Collision {
 /// Only actor registration grows stores or creates a pose accumulator.
 pub fn actor(&mut self,skeleton:&bfe_pose::rig::skeleton::Skeleton){
  let skeleton=RiderSkeleton::new(skeleton);let n=skeleton.parent.len();let scratch=RiderScratch::new(&skeleton);
  self.rows.push(Cache{binding:f64::NAN,generation:f64::NAN,tick:f64::NAN,skeleton,scratch,q:vec![Quaternion::default();n],p:vec![Vector3::default();n],points:RiderPoints::default(),rider:[rider::HitShape::default();lim::SHAPES],shapes:[Shape::default();lim::SHAPES]});
  // The host submits a retained actor-id prefix, not the whole resource pool.
  self.actors.resize(self.rows.len(),0);
  self.input.resize(self.rows.len()*i::STRIDE,0.0);self.output.resize(self.rows.len()*o::STRIDE,0.0);self.shapes.resize(self.rows.len()*lim::SHAPES*s::STRIDE,0.0);
 }
 pub fn observe(&mut self,id:usize,content:&Content,rig:&DisplayRig,sources:&Bindings){
  let Some(cache)=self.rows.get_mut(id)else{return};let x=&self.input[id*i::STRIDE..][..i::STRIDE];let out=&mut self.output[id*o::STRIDE..][..o::STRIDE];out[o::CHANGED]=0.0;
  if x[i::ENABLED]==0.0||cache.binding==x[i::BINDING]&&cache.generation==x[i::GENERATION]&&cache.tick==x[i::TICK]{return;}
  // Binding changes distinguish a prepared class resource transferred to
  // the viewpoint from its old body, even if both have the same tick/life.
  cache.binding=x[i::BINDING];cache.generation=x[i::GENERATION];cache.tick=x[i::TICK];let mut count=0;
  let held=sources.held_source(x[i::SOURCE_PROP]);
  let seated=x[i::SEAT]!=0.0&&x[i::EXPOSED]!=0.0;
  // An absent clip or incomplete anatomical rig falls back to posture,
  // as the original lazy getter did; a store boundary never traps a frame.
  if seated||held.is_some(){
   let source=sources.live_source(x[i::SOURCE_RIG],x[i::SOURCE_SEAT],x[i::SOURCE_TURRET]);
   if let Some(clip)=if x[i::CLIP]>=0.0{content.clips.get(x[i::CLIP]as usize)}else{None}{
    // Unkeyed tracks read the current native local pose, while anatomical
    // bind lengths remain immutable in RiderSkeleton.
    for(index,&bone)in rig.bones.iter().enumerate(){cache.q[index]=rig.tree.node(bone).quaternion;cache.p[index]=rig.tree.node(bone).position;}
    let(pitch,turn,time,frame)=if let Some(held)=held {(0.0,0.0,Some(held.time),held.frame)}else if let Some(source)=source {
     let(pitch,turn,time)=sample(x,&source);let mut frame=source.frame;
     // An animal puts its pelvis, rather than its feet, on the saddle.
     if source.animal {
      let sample=RiderSample{clip:&clip.sample,pitch,turn,time};
      if rider::rider_points(&cache.skeleton,&cache.q,&cache.p,&sample,&mut cache.scratch,&mut cache.points,false){
       let mut offset=cache.points.pelvis;offset.apply_quaternion(&source.quaternion);let mut p=source.position;p.sub(&offset);frame.compose(&p,&source.quaternion,&Vector3::new(1.0,1.0,1.0));
      }
     }(pitch,turn,time,frame)
    }else{(0.0,0.0,None,Matrix4::default())};
    if held.is_some()||source.is_some(){
     let sample=RiderSample{clip:&clip.sample,pitch,turn,time};
     if rider::rider_points(&cache.skeleton,&cache.q,&cache.p,&sample,&mut cache.scratch,&mut cache.points,false){
      count=rider::rider_hit_shapes(&cache.points,&frame,Some(x[i::SCALE]),&mut cache.rider);
      for n in 0..count {let h=cache.rider[n];cache.shapes[n]=Shape{kind:if h.kind==rider::ShapeKind::Sphere{Kind::Sphere}else{Kind::Capsule},head:h.part==rider::ShapePart::Head,center:h.center,radius:h.radius,half:h.half,half_size:h.half_size,yaw:h.yaw};}
     }
    }
   }
  }
  // Posture precedence follows the old getter: dead, dive, air, posture.
  if count==0 {
   let posture=if x[i::DEAD]!=0.0{2}else if x[i::DIVE]!=0.0{1}else if x[i::AIR]!=0.0{0}else{x[i::POSTURE]as u8};
   count=body_shapes::standing(Vector3::new(x[i::POSITION],x[i::POSITION+1],x[i::POSITION+2]),x[i::YAW],posture,x[i::SCALE],(x[i::WALKER]!=0.0).then_some(x[i::MODE]),x[i::BALL_RADIUS],&mut cache.shapes);
  }
  let(center,radius)=body_shapes::bounds(&cache.shapes[..count]);out[o::COUNT]=count as f64;out[o::CENTER..o::CENTER+3].copy_from_slice(&[center.x,center.y,center.z]);out[o::RADIUS]=radius;out[o::CHANGED]=1.0;
  // Publish a fixed prefix. The page retains vectors and its result array
  // while native counts change between ordinary bodies and ten rider limbs.
  for n in 0..count {let h=cache.shapes[n];let at=(id*lim::SHAPES+n)*s::STRIDE;let row=&mut self.shapes[at..][..s::STRIDE];
   row[s::KIND]=match h.kind{Kind::Sphere=>k::SPHERE,Kind::Cylinder=>k::CYLINDER,Kind::Box=>k::BOX,Kind::Capsule=>k::CAPSULE}as f64;
   row[s::HEAD]=if h.head{1.0}else{0.0};row[s::CENTER..s::CENTER+3].copy_from_slice(&[h.center.x,h.center.y,h.center.z]);row[s::RADIUS]=h.radius;row[s::HALF]=h.half;row[s::HALF_SIZE..s::HALF_SIZE+3].copy_from_slice(&[h.half_size.x,h.half_size.y,h.half_size.z]);row[s::YAW]=h.yaw;
  }
 }
}
// riderSample's live sample is separate from the display lease's frozen
// nine-pose. Both read the same native vehicle/prop source and class clip.
fn sample(x:&[f64],source:&Source)->(f64,f64,Option<f64>){
 if x[i::TIMED]!=0.0{return(0.0,0.0,Some(if x[i::FIXED_TIME]!=0.0{x[i::TIME]}else if x[i::SOURCE_TURRET]>=0.0{0.0}else{source.age}));}
 if x[i::SOURCE_TURRET]>=0.0{return(-source.pitch/std::f64::consts::FRAC_PI_4,if source.turn_rate==0.0{0.0}else{source.yaw_rate/source.turn_rate},None);}
 if x[i::NINE]==0.0{return(0.0,0.0,None);}
 if x[i::SOURCE_SEAT]==0.0{return(-source.pitch/0.7,0.0,None);}
 // A bounded turret arc uses position; an unrestricted one uses speed.
 // Preserve wrap and normalization before blending the nine-pose.
 let[low,high]=source.pitch_limits;let pitch=if high>low{three::clamp(2.0*((high-source.pitch)/(high-low)-0.5),-1.0,1.0)}else{0.0};
 let turn=if let Some([low,high])=source.yaw_limits.filter(|l|l[1]-l[0]<1.5*std::f64::consts::PI){
  let mut yaw=v8math::atan2(v8math::sin(source.yaw-source.hull_yaw),v8math::cos(source.yaw-source.hull_yaw));if source.wrap&&yaw<0.0{yaw+=2.0*std::f64::consts::PI;}
  if high>low{three::clamp(2.0*((yaw-low)/(high-low)-0.5),-1.0,1.0)}else{0.0}
 }else{three::clamp(source.yaw_rate/v8math::max(source.turn_rate,1e-3),-1.0,1.0)};(pitch,turn,None)
}
