//! Unit.updateSpheres' on-foot primitives, shared by authority and replicas.
//! Geometry is independent of drawn animation; exposed and held bodies use
//! the rider sampler before falling back to these posture primitives.
use three::Vector3;
#[derive(Clone,Copy,Debug,PartialEq,Eq,Default)]
pub enum Kind {#[default] Sphere,Cylinder,Box,Capsule}
#[derive(Clone,Copy,Debug,Default)]
pub struct Part {pub kind:Kind,pub head:bool,pub at:[f64;3],pub radius:f64,pub half:f64,pub half_box:[f64;3]}
/// soldierShapes preserves class scale and the enlarged head's small lift.
pub fn parts(posture:u8,s:f64)->[Part;2]{
 let big=v8math::max(s,1.0);let lift=0.17*(big-1.0);
 let cylinder=|at,radius,half|Part{kind:Kind::Cylinder,at,radius,half,..Default::default()};
 let head=|at,radius|Part{head:true,at,radius,..Default::default()};
 match posture {
  2=>[Part{kind:Kind::Box,at:[0.0,0.2,-0.25*s],half_box:[0.25,0.2,0.87*s],..Default::default()},head([-0.07,0.28*s,0.75*s],0.17*big)],
  1=>[cylinder([0.0,0.6*s+lift,0.0],0.35,0.45*s),head([-0.12,1.2*s,0.25],0.17*big)],
  _=>[cylinder([0.0,0.8*s+lift,0.0],0.35,0.775*s),head([-0.09,1.65*s,0.16],0.17*big)]
 }
}
#[derive(Clone,Copy,Debug,Default)]
pub struct Shape {pub kind:Kind,pub head:bool,pub center:Vector3,pub radius:f64,pub half:f64,pub half_size:Vector3,pub yaw:f64}
/// The lazy cache stores complete fixed values, including signed-zero axes.
pub fn standing(position:Vector3,yaw:f64,posture:u8,scale:f64,mode:Option<f64>,ball_radius:f64,out:&mut [Shape;10])->usize{
 // Keep x as soldier-left and z as forward, in the original operation order.
 // Folded droidekas expose their configured ball; deployed ones use both
 // authored ordnance primitives, without a separate head multiplier.
 let sin=v8math::sin(yaw);let cos=v8math::cos(yaw);
 if let Some(mode)=mode {
  let rolled=mode>=11.0;let r=if rolled{ball_radius}else{0.65};
  out[0]=Shape{center:Vector3::new(position.x,position.y+r,position.z),radius:r,yaw,..Default::default()};
  if rolled{return 1;}
  out[1]=Shape{kind:Kind::Cylinder,center:Vector3::new(position.x+sin*0.506,position.y+1.0,position.z+cos*0.506),radius:0.8,half:1.0,yaw,..Default::default()};return 2;
 }
 for(k,p)in parts(posture,scale).iter().enumerate(){let[x,y,z]=p.at;
  out[k]=Shape{kind:p.kind,head:p.head,center:Vector3::new(position.x+cos*x+sin*z,position.y+y,position.z-sin*x+cos*z),radius:p.radius,half:p.half,half_size:Vector3::new(p.half_box[0],p.half_box[1],p.half_box[2]),yaw};
 }2
}
// Each owner keeps its own store layout. The shared broadphase reads those
// values through monomorphized accessors, without copying or allocating them.
pub trait BoundsShape {fn center(&self)->Vector3;fn reach(&self)->f64;}
impl BoundsShape for Shape {
 fn center(&self)->Vector3{self.center}
 fn reach(&self)->f64{match self.kind{Kind::Sphere=>self.radius,Kind::Cylinder=>v8math::hypot(self.radius,self.half),Kind::Box=>self.half_size.length(),Kind::Capsule=>self.half_size.length()+self.radius}}
}
// The mean of centres is deliberate: this is the original broadphase,
// rather than a minimum enclosing sphere with different query boundaries.
pub fn bounds<T:BoundsShape>(shapes:&[T])->(Vector3,f64){
 let mut center=Vector3::default();for s in shapes{center.add(&s.center());}center.divide_scalar(shapes.len()as f64);
 let mut radius=0.0;for s in shapes{radius=v8math::max(radius,center.distance_to(&s.center())+s.reach());}(center,radius)
}
