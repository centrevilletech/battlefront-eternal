//! PlayerCombat.aimBasis and eye, shared by unit display,
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

/// The line a player's reticle sits on, as the 2004 game draws it: in third
/// person the chase camera's line along the aim, from where the camera rests
/// (`camera`, its section: never pulled in at a wall, never eased), started
/// level with the eye; with no camera (first person) the eye's own line, so
/// the first thing along it is what the reticle is on. A droideka's shots
/// converge on that first thing (zoomed too: from its eye). Returns the
/// line's start and direction.
pub fn camera_line<M:Math>(feet:V,yaw:f64,pitch:f64,eye:V,camera:Option<&crate::cameras::soldier::Section>)->(V,V) {
    use crate::cameras::math::{euler,rotate};
    let direction=V::new(M::sin(yaw)*M::cos(pitch),M::sin(pitch),M::cos(yaw)*M::cos(pitch));
    let Some(section)=camera else {return (eye,direction)};
    let pivot=crate::cameras::soldier::pivot::<M>(feet,yaw,crate::cameras::soldier::v(section.center));
    let q=euler::<M>(pitch-section.tilt,yaw+std::f64::consts::PI,0.0,false);
    let rest=add(pivot,rotate(crate::cameras::soldier::v(section.offset),q));
    let along=sub(eye,rest).dot(&direction);
    (plus(rest,direction,along),direction)
}

/// Where a soldier's crosshair ray starts in third person, and its
/// direction, as the 2004 game finds it: the camera's line (`section`, the
/// posture's camera, its ZOOM section when zoomed) level with the eye at the
/// section's EyePointOffset, then TrackOffset.z + 2 m on along the aim. The
/// ray looks 60 m from there; nearer things are passed over, so a crosshair
/// on the ground at a soldier's feet sends his shot almost along the aim.
pub fn crosshair_start<M:Math>(feet:V,yaw:f64,pitch:f64,section:&crate::cameras::soldier::Section)->(V,V) {
    let eye=crate::cameras::soldier::pivot::<M>(feet,yaw,crate::cameras::soldier::v(section.eye));
    let (line,direction)=camera_line::<M>(feet,yaw,pitch,eye,Some(section));
    (plus(line,direction,section.offset[2]+2.0),direction)
}

// Command preview and replay use this same gun/eye geometry. Collision is
// supplied afterwards so neither owner calls a foreign object per command.
#[derive(Clone,Copy,Default)]
pub struct View {
    pub position:V,pub feet:V,pub yaw:f64,pub pitch:f64,pub posture:usize,pub air:bool,
    pub walker:bool,pub rolled:bool,pub eye_height:f64,pub zoom:f64,
    pub camera:Option<V>,pub uses_eye:bool,pub track_offsets:[f64;3],
    /// The class's cameras (STAND, CROUCH, PRONE, then their ZOOM sections):
    /// with them the crosshair ray starts as the 2004 game starts it
    /// (`crosshair_start`), which the world's own held rounds follow too;
    /// without them, from the drawn `camera` and `track_offsets`.
    pub sections:Option<[crate::cameras::soldier::Section;6]>,
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
        let needs_reach=(self.camera.is_some()||self.sections.is_some())&&!self.uses_eye&&!self.walker;let mut from=V::default();
        let posture=if self.air{0}else{self.posture.min(2)};
        if let Some(sections)=self.sections.filter(|_|needs_reach){
            from=crosshair_start::<M>(self.position,self.yaw,self.pitch,&sections[posture+if self.zoom>1.0{3}else{0}]).0;
        }else if let Some(camera)=self.camera.filter(|_|needs_reach){
            let behind=sub(camera,basis.pivot).dot(&basis.direction);from=plus(camera,basis.direction,-behind);
            from=plus(from,basis.direction,self.track_offsets[posture]+2.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cameras::math::V8;
    #[test]
    fn converged_fire_matches_the_live_camera_ray_and_eye_views_keep_body_aim() {
        let mut view=View {position:V::new(-156.68114711235845,-50.04499710083008,87.94857544127457),
            camera:Some(V::new(-156.68114711235845,-47.69683062868016,91.40538227979053)),
            yaw:std::f64::consts::PI,pitch:-0.07,track_offsets:[3.5;3],..Default::default()};
        let mut aim=view.sample::<V8>();aim.resolve::<V8>(13.948355251622601);
        assert!((aim.yaw - -3.138465488710734).abs()<1e-12);
        assert!((aim.pitch - -0.0437119141364).abs()<1e-12);
        for posture in 0..3 {
            view.posture=posture;view.uses_eye=true;
            let mut eye=view.sample::<V8>();eye.resolve::<V8>(5.);
            assert_eq!((eye.yaw,eye.pitch),(view.yaw,view.pitch));
        }
        view.uses_eye=false;view.walker=true;
        assert!(!view.sample::<V8>().needs_reach);
    }
    #[test]
    fn a_walkers_line_is_the_chase_cameras_level_with_its_eye() {
        use crate::cameras::soldier::Section;
        // The droideka's STAND camera: its line runs 0.5 cos 5 + 4.5 sin 5
        // (0.890) m above the eye, square to the aim, whatever the aim.
        let stand=Section{eye:[0.;3],center:[0.,1.8,0.],offset:[0.,0.5,4.5],tilt:5f64.to_radians()};
        let rise=0.5*5f64.to_radians().cos()+4.5*5f64.to_radians().sin();
        for (yaw,pitch) in [(0.,0.),(1.2,0.3),(-2.5,-0.4)] {
            let (feet,eye)=(V::new(4.,-1.,7.),V::new(4.,0.8,7.));
            let (start,direction)=camera_line::<V8>(feet,yaw,pitch,eye,Some(&stand));
            let off=sub(start,eye);
            assert!((off.length()-rise).abs()<1e-12&&off.dot(&direction).abs()<1e-12);
            assert!(off.y>0.&&off.dot(&V::new(yaw.cos(),0.,-yaw.sin())).abs()<1e-12);
            assert_eq!(camera_line::<V8>(feet,yaw,pitch,eye,None),(eye,direction));
        }
    }
}
