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

// Command preview and replay use this same gun/eye geometry. Collision is
// supplied afterwards so neither owner calls a foreign object per command.
#[derive(Clone,Copy,Default)]
pub struct View {
    pub position:V,pub feet:V,pub yaw:f64,pub pitch:f64,pub posture:usize,pub air:bool,
    pub walker:bool,pub rolled:bool,pub eye_height:f64,pub zoom:f64,
    pub camera:Option<V>,pub uses_eye:bool,pub track_offsets:[f64;3],
}
#[derive(Clone,Copy,Default)]
pub struct CommandAim {pub basis:Basis,pub eye:V,pub from:V,pub needs_reach:bool,pub yaw:f64,pub pitch:f64}
impl View {
    // page_body.aim preserves the third-person droideka's infantry pivot,
    // independently of the barrel and eye its displayed model uses.
    pub fn sample<M:Math>(&self)->CommandAim {
        let basis=Basis::new::<M>(self.position,self.yaw,self.pitch,self.posture,self.air);
        let eye=if self.walker{V::new(self.feet.x,self.feet.y+if self.rolled{1.0}else{self.eye_height},self.feet.z)}
            else{basis.eye(self.feet,self.position,self.posture,self.air,self.zoom)};
        let needs_reach=self.camera.is_some()&&!self.uses_eye;let mut from=V::default();
        if let Some(camera)=self.camera.filter(|_|needs_reach){
            let behind=sub(camera,basis.pivot).dot(&basis.direction);from=plus(camera,basis.direction,-behind);
            let posture=if self.air{0}else{self.posture.min(2)};from=plus(from,basis.direction,self.track_offsets[posture]+2.0);
        }
        CommandAim{basis,eye,from,needs_reach,yaw:self.yaw,pitch:self.pitch}
    }
}
impl CommandAim {
    // PlayerCombat.convergedAim adds reach before subtracting gun origin.
    // Normalize only that final difference, retaining close-range parallax.
    pub fn resolve<M:Math>(&mut self,reach:f64){
        if !self.needs_reach{return;}
        let mut direction=self.from;direction.add_scaled_vector(&self.basis.direction,reach);
        direction.sub(&self.basis.origin).normalize();self.yaw=M::atan2(direction.x,direction.z);
        self.pitch=M::asin(v8math::max(-1.0,v8math::min(1.0,direction.y)));
    }
}
