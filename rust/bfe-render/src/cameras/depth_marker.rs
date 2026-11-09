//! ParticleDepth.place/request retain the depth-copy marker in camera storage.
//! The resource host supplies keys; projection, ordering and placement stay here.
use crate::{math::{self,Mat4},cameras::layout::{depth_key as k,depth_marker as o}};
const ID:Mat4=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
pub struct DepthMarker{pub key:[f64;k::LEN],pub output:[f64;o::LEN],screen:Mat4,earliest:[f64;7],requests:u32}
impl Default for DepthMarker{fn default()->Self{Self{key:[0.;k::LEN],output:[0.;o::LEN],screen:ID,earliest:[0.;7],requests:0}}}
impl DepthMarker{
 pub fn begin(&mut self,screen:&Mat4){self.screen = *screen;self.requests=0;self.output[o::VISIBLE]=0.;self.output[o::REQUESTS]=0.;}
 // Reverse painter order includes group, explicit order, centre depth and id.
 // Comparisons deliberately retain JS's NaN and signed-zero behaviour.
 pub fn request(&mut self)->bool{
  let key=&self.key;let s=&self.screen;let z=s[2]*key[k::CENTER]+s[6]*key[k::CENTER+1]+s[10]*key[k::CENTER+2]+s[14]*1.;let e=&self.earliest;
  let before=if key[k::GROUP]!=e[0]{key[k::GROUP]<e[0]}else if key[k::ORDER]!=e[1]{key[k::ORDER]<e[1]}else if z!=e[2]{z>e[2]}else{key[k::ID]<e[3]};
  if self.requests==0||before{self.earliest=[key[k::GROUP],key[k::ORDER],z,key[k::ID],key[k::CENTER],key[k::CENTER+1],key[k::CENTER+2]];}
  self.requests=self.requests.saturating_add(1);true
 }
 // Doubling the rounding-sized step reproduces the old Vector4 operations.
 // A degenerate view publishes the original first-in-list identity marker.
 pub fn finish(&mut self){
  self.output[o::REQUESTS]=self.requests as f64;if self.requests==0{return;}
  let e=self.earliest;let [z,cx,cy,cz]=[e[2],e[4],e[5],e[6]];let s=&self.screen;
  let [gx,gy,gz]=[s[2],s[6],s[10]];let g2=gx*gx+gy*gy+gz*gz;let mut placed=false;let mut matrix=ID;
  if g2>0.&&z.is_finite()&&(cx+cy+cz).is_finite(){
   let sum=(gx*cx).abs()+(gy*cy).abs()+(gz*cz).abs()+s[14].abs();let mut step=f64::EPSILON*if sum.is_nan(){f64::NAN}else{sum.max(f64::MIN_POSITIVE)};
   for _ in 0..64{
    let amount=step/g2;matrix=ID;matrix[12]=cx+gx*amount;matrix[13]=cy+gy*amount;matrix[14]=cz+gz*amount;
    // Even the identity terms are evaluated: their additions determine the
    // sign of zero before the second transform in Vector4.applyMatrix4.
    let point=math::transform4(&matrix,[0.,0.,0.,1.]);let projected=math::transform4(s,point);
    if projected[2]>z{placed=true;break;}step*=2.;
   }
  }
  if placed{self.output[o::GROUP]=e[0];self.output[o::ORDER]=e[1];}else{matrix=ID;self.output[o::GROUP] = -f64::MAX;self.output[o::ORDER]=0.;}
  self.output[o::MATRIX..o::MATRIX+16].copy_from_slice(&matrix);self.output[o::VISIBLE]=1.;
 }
}
