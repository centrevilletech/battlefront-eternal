//! PlayerCombat.aimBasis and eye from combat.js, shared by unit display,
//! immediate command preview and speculative shot publication.
use crate::cameras::math::{Math, cross, plus, add, sub};
use three::Vector3 as V;

#[derive(Clone, Copy, Default)]
pub struct Basis { pub origin:V, pub pivot:V, pub left:V, pub up:V, pub direction:V }
impl Basis {
    pub fn new<M:Math>(position:V,yaw:f64,pitch:f64,posture:usize,air:bool) -> Self {
        // Authored gun offsets use the model's positive-left yaw frame.
        let posture=if air {0}else{posture.min(2)};
        let direction=V::new(M::sin(yaw)*M::cos(pitch),M::sin(pitch),M::cos(yaw)*M::cos(pitch));
        let left=V::new(M::cos(yaw),0.0,-M::sin(yaw));
        let up=cross(direction,left);
        let [px,py,pz]=[[0.06,1.7,0.0],[-0.1,1.2,0.0],[0.06,0.6,0.4]][posture];
        let pivot=V::new(position.x+left.x*px+M::sin(yaw)*pz,position.y+py,position.z+left.z*px+M::cos(yaw)*pz);
        let [gx,gy,gz]=[[-0.12,-0.1,0.25],[-0.12,-0.05,0.25],[-0.12,-0.2,0.15]][posture];
        // Keep the three sequential addScaledVector operations: reassociating
        // their sums changes distant gun origins in the last bit.
        let origin=plus(plus(plus(pivot,direction,gz),left,gx),up,gy);
        Self{origin,pivot,left,up,direction}
    }
    pub fn eye(&self,feet:V,position:V,posture:usize,air:bool,zoom:f64)->V {
        let eye=if !air&&posture==2 {plus(self.pivot,self.left,-0.12)}else{self.origin};
        // SoldierCamera consumes smoothed feet independently of body position.
        // Subtract and add separately, as PlayerCombat.eye did before setback.
        plus(add(sub(eye,position),feet),self.direction,if zoom>1.0{-0.6}else{-0.3})
    }
    pub fn words(&self)->[f64;15] {
        let [a,b,c,d,e]=[self.origin,self.pivot,self.left,self.up,self.direction];
        [a.x,a.y,a.z,b.x,b.y,b.z,c.x,c.y,c.z,d.x,d.y,d.z,e.x,e.y,e.z]
    }
}
