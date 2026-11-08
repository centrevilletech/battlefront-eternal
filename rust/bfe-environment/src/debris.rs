//! Debris.blowApart, update, bounce and rest retain their classic trajectory.
//! Named assets, casts, bounds queries and effect handles are host resources.
//! Fixed slots preserve identity when a burst fills the visual debris bank.
use crate::layout::{debris as lim,debris_launch as input,debris_piece as out,debris_event as event,debris_scratch as scratch};
use three::{Matrix4,Quaternion,Vector3};
const UP:Vector3=Vector3::new(0.,1.,0.);
const AXES:[Vector3;3]=[Vector3::new(1.,0.,0.),UP,Vector3::new(0.,0.,1.)];
#[derive(Clone,Copy)]
struct Settle {from:Quaternion,to:Quaternion,y0:f64,y1:f64,t:f64}
#[derive(Clone,Copy)]
struct Piece {
    position:Vector3,previous:Vector3,velocity:Vector3,q:Quaternion,omega:Vector3,
    hits:f64,lift:f64,thin:usize,resting:bool,flight:f64,left:f64,most:f64,
    trail:bool,smoke:bool,impact:bool,settle:Option<Settle>,shift:Option<Vector3>,impact_point:Vector3,
}
impl Default for Piece {fn default()->Self{Self{position:Vector3::default(),previous:Vector3::default(),velocity:Vector3::default(),q:Quaternion::default(),omega:Vector3::default(),hits:0.,lift:0.,thin:1,resting:false,flight:0.,left:10.,most:0.,trail:false,smoke:false,impact:false,settle:None,shift:None,impact_point:Vector3::default()}}}
pub struct Hit {pub point:Vector3,pub normal:Vector3}
// Event kinds describe browser I/O, never a host-supplied physics afterstate.
// Publish precedes each query/event so callbacks read the native position.
pub trait Host {
    fn cast(&mut self,slot:usize,start:Vector3,ray:Vector3,far:f64)->Option<Hit>;
    fn low(&mut self,slot:usize,q:Quaternion)->f64;
    fn event(&mut self,slot:usize,kind:f64,row:&[f64]);
}
pub struct Phase {
    pub input:Vec<f64>,pub output:Vec<f64>,pub scratch:[f64;scratch::STRIDE],pieces:Vec<Option<Piece>>,order:Vec<usize>,
}
impl Default for Phase{fn default()->Self{Self::new()}}
impl Phase {
    pub fn new()->Self{Self{scratch:[0.;scratch::STRIDE],input:vec![0.;input::STRIDE],output:vec![0.;lim::CAPACITY*out::STRIDE],pieces:vec![None;lim::CAPACITY],order:Vec::with_capacity(lim::CAPACITY)}}
    pub fn free(&self)->usize {self.pieces.iter().position(Option::is_none).unwrap_or(lim::CAPACITY)}
    fn publish(&mut self,id:usize,p:&Piece){let r=&mut self.output[id*out::STRIDE..(id+1)*out::STRIDE];
        let put=|r:&mut[f64],at:usize,v:Vector3|r[at..at+3].copy_from_slice(&[v.x,v.y,v.z]);
        put(r,out::POSITION,p.position);put(r,out::PREVIOUS,p.previous);put(r,out::VELOCITY,p.velocity);put(r,out::OMEGA,p.omega);
        r[out::QUATERNION..out::QUATERNION+4].copy_from_slice(&[p.q.x,p.q.y,p.q.z,p.q.w]);
        r[out::HITS]=p.hits;r[out::LIFT]=p.lift;r[out::THIN]=p.thin as f64;r[out::RESTING]=u8::from(p.resting) as f64;r[out::FLIGHT]=p.flight;r[out::LEFT]=p.left;
        r[out::HAS_SHIFT]=u8::from(p.shift.is_some()) as f64;if let Some(v)=p.shift{put(r,out::SHIFT,v);}
        put(r,out::IMPACT_POINT,p.impact_point);r[out::HAS_SETTLE]=u8::from(p.settle.is_some()) as f64;
        if let Some(s)=p.settle{r[out::SETTLE_FROM..out::SETTLE_FROM+4].copy_from_slice(&[s.from.x,s.from.y,s.from.z,s.from.w]);r[out::SETTLE_TO..out::SETTLE_TO+4].copy_from_slice(&[s.to.x,s.to.y,s.to.z,s.to.w]);r[out::SETTLE_Y]=s.y0;r[out::SETTLE_Y+1]=s.y1;r[out::SETTLE_T]=s.t;}
    }
    fn event(&mut self,id:usize,p:&Piece,kind:f64,host:&mut impl Host){self.publish(id,p);host.event(id,kind,&self.output[id*out::STRIDE..(id+1)*out::STRIDE]);}
    // Debris.blowApart resolves one model/bone pair at admission. The launch
    // result carries the recentering delta, avoiding host-side flight math.
    pub fn launch(&mut self,id:usize,host:&mut impl Host){if id>=lim::CAPACITY||self.pieces[id].is_some(){return;}
        let c=&self.input;let v=|at:usize|Vector3::new(c[at],c[at+1],c[at+2]);
        let matrix=Matrix4{elements:c[input::MATRIX..input::MATRIX+16].try_into().unwrap()};
        let center=c[input::CENTER]!=0.;let lo=v(input::BOUNDS);let hi=v(input::BOUNDS+3);let valid=!(hi.x<lo.x||hi.y<lo.y||hi.z<lo.z);
        let mut p=Piece{omega:v(input::OMEGA),most:c[input::COLLISIONS],trail:c[input::TRAIL]!=0.,smoke:c[input::SMOKE]!=0.,impact:c[input::IMPACT]!=0.,..Piece::default()};
        let(mut middle,mut size)=(Vector3::default(),Vector3::default());
        if center&&valid{middle.add_vectors(&lo,&hi).multiply_scalar(0.5);size.sub_vectors(&hi,&lo);p.lift=v8math::min(v8math::min(size.x,size.y),size.z)/2.;p.thin=if size.x<=size.y&&size.x<=size.z{0}else if size.y<=size.z{1}else{2};p.shift=Some(middle);}
        matrix.decompose(&mut p.position,&mut p.q,&mut Vector3::default());
        if center{p.position.copy(&middle.apply_matrix4(&matrix));p.position.y=v8math::max(p.position.y,c[input::FLOOR]+size.y/2.);}
        let mut d=Vector3::default();d.sub_vectors(&p.position,&v(input::ORIGIN));d.y+=0.25*d.length();if d.length_sq()<1e-8{d.set(0.,1.,0.);}
        p.resting=c[input::STILL]!=0.;if p.resting{p.left=f64::INFINITY;}else{let health=c[input::HEALTH];let factor=if health<=-50.{1.}else{v8math::min(0.1-(0.9*health)/50.,1.)};p.velocity=*d.normalize().multiply_scalar(c[input::SPEED]).add(&v(input::VELOCITY)).multiply_scalar(factor);}
        // The model children are CPU resource transforms. Return their
        // recentered coordinates with this same admission, rather than
        // running a second host geometry transform or one call per child.
        if let Some(shift)=p.shift {
            let count=(self.input[input::CHILD_COUNT] as usize).min(lim::CHILDREN);
            for child in self.input[input::CHILDREN..input::CHILDREN+count*3].chunks_exact_mut(3){child[0]-=shift.x;child[1]-=shift.y;child[2]-=shift.z;}
        }
        self.scratch[scratch::OUT..scratch::OUT+3].copy_from_slice(&[d.x,d.y,d.z]);
        p.previous=p.position;self.event(id,&p,event::ADMIT,host);self.pieces[id]=Some(p);self.order.push(id);
    }
    // Debris.rest preserves the current spinning pose as the interpolation
    // start. Bounds are queried at the target orientation, then restored.
    fn rest(&mut self,id:usize,p:&mut Piece,floor:f64,host:&mut impl Host){
        if p.lift>0.&&floor.is_finite(){let mut axis=AXES[p.thin.min(2)];axis.apply_quaternion(&p.q);let down=Vector3::new(0.,-1.,0.);let mut target=Quaternion::default();target.set_from_unit_vectors(&axis,if axis.y<0.{&down}else{&UP}).multiply(&p.q);
            self.publish(id,p);let low=host.low(id,target);p.settle=Some(Settle{from:p.q,to:target,y0:p.position.y,y1:p.position.y+floor-0.15-low,t:0.});}
        p.resting=true;p.velocity.set(0.,0.,0.);if p.trail{p.trail=false;self.event(id,p,event::STOP_TRAIL,host);}
    }
    fn bounce(&mut self,id:usize,p:&mut Piece,host:&mut impl Host){let mut ray=Vector3::default();ray.sub_vectors(&p.position,&p.previous);let length=ray.length();self.scratch[scratch::RAY..scratch::RAY+3].copy_from_slice(&[ray.x,ray.y,ray.z]);if length<1e-6{return;}ray.divide_scalar(length);let mut start=p.previous;start.add_scaled_vector(&ray,-0.05);
        self.scratch[scratch::RAY..scratch::RAY+3].copy_from_slice(&[ray.x,ray.y,ray.z]);self.scratch[scratch::START..scratch::START+3].copy_from_slice(&[start.x,start.y,start.z]);
        self.publish(id,p);let Some(hit)=host.cast(id,start,ray,length+0.1+p.lift)else{return;};let d=p.velocity.dot(&hit.normal);if d>=0.{return;}
        p.position.copy(&hit.point).add_scaled_vector(&hit.normal,v8math::max(0.05,p.lift));
        if p.hits<p.most&&p.impact{p.impact_point=hit.point;self.event(id,p,event::IMPACT,host);}p.hits+=1.;
        p.velocity.add_scaled_vector(&hit.normal,-d);let along=p.velocity.length();if along>0.{p.velocity.multiply_scalar(v8math::max(along-v8math::min(1.5755*-d,along),0.)/along);}p.velocity.add_scaled_vector(&hit.normal,0.37*-d);
        if p.hits>=p.most||p.velocity.length()<1.{self.rest(id,p,hit.point.y,host);}
    }
    // Debris.update runs one phase over retained slots. Compact only the
    // ordinal list after all deaths, preserving original callback order.
    pub fn update(&mut self,dt:f64,host:&mut impl Host){for i in 0..self.order.len(){let id=self.order[i];let Some(mut p)=self.pieces[id]else{continue;};
        if p.resting{p.left-=dt;if let Some(s)=&mut p.settle{if s.t<0.35{s.t=v8math::min(s.t+dt,0.35);let w=s.t/0.35;let ease=w*w*(3.-2.*w);p.q.slerp_quaternions(&s.from,&s.to,ease);p.position.y=s.y0+(s.y1-s.y0)*ease;if p.smoke{self.event(id,&p,event::SETTLE,host);}}}}
        else{p.flight+=dt;p.velocity.y+=-9.8*dt;p.previous=p.position;p.position.add_scaled_vector(&p.velocity,dt);for(axis,omega)in AXES.iter().zip([p.omega.x,p.omega.y,p.omega.z]){let mut q=Quaternion::default();q.set_from_axis_angle(axis,omega*dt);p.q.multiply(&q);}self.bounce(id,&mut p,host);self.event(id,&p,event::MOVE,host);if p.flight>20.{self.rest(id,&mut p,f64::NEG_INFINITY,host);}}
        self.publish(id,&p);self.pieces[id]=Some(p);
    }
    let mut count=0;for i in 0..self.order.len(){let id=self.order[i];let Some(p)=self.pieces[id]else{continue;};if p.left>0.{self.order[count]=id;count+=1;}else{self.event(id,&p,event::RETIRE,host);self.pieces[id]=None;}}self.order.truncate(count);}
    // Capture tools may issue an isolated contact or rest command. These
    // enter the same retained owner; ordinary game frames use update only.
    pub fn contact(&mut self,id:usize,host:&mut impl Host){if let Some(mut p)=self.pieces.get(id).copied().flatten(){self.bounce(id,&mut p,host);self.publish(id,&p);self.pieces[id]=Some(p);}}
    pub fn settle(&mut self,id:usize,floor:f64,host:&mut impl Host){if let Some(mut p)=self.pieces.get(id).copied().flatten(){self.rest(id,&mut p,floor,host);self.publish(id,&p);self.pieces[id]=Some(p);}}
    pub fn clear(&mut self,host:&mut impl Host){for i in 0..self.order.len(){let id=self.order[i];if let Some(p)=self.pieces[id].take(){self.event(id,&p,event::CLEAR,host);}}self.order.clear();}
}
#[cfg(target_arch="wasm32")]
mod exports {
    use super::*;use std::ptr::addr_of_mut;
    static mut STATE:Option<Phase>=None;
    fn state()->&'static mut Phase{unsafe{(*addr_of_mut!(STATE)).as_mut().expect("debris_new first")}}
    #[link(wasm_import_module="env")]extern "C"{
        fn host_debris_cast(slot:u32,ox:f64,oy:f64,oz:f64,dx:f64,dy:f64,dz:f64,far:f64)->u32;
        fn host_debris_hit(i:u32)->f64;
        fn host_debris_low(slot:u32,x:f64,y:f64,z:f64,w:f64)->f64;
        fn host_debris_event(slot:u32,kind:u32);
    }
    struct Browser;
    impl Host for Browser{
        fn cast(&mut self,id:usize,s:Vector3,r:Vector3,far:f64)->Option<Hit>{unsafe{if host_debris_cast(id as u32,s.x,s.y,s.z,r.x,r.y,r.z,far)==0{None}else{Some(Hit{point:Vector3::new(host_debris_hit(0),host_debris_hit(1),host_debris_hit(2)),normal:Vector3::new(host_debris_hit(3),host_debris_hit(4),host_debris_hit(5))})}}}
        fn low(&mut self,id:usize,q:Quaternion)->f64{unsafe{host_debris_low(id as u32,q.x,q.y,q.z,q.w)}}
        fn event(&mut self,id:usize,k:f64,_:&[f64]){unsafe{host_debris_event(id as u32,k as u32)}}
    }
    #[no_mangle]pub extern "C" fn debris_new(){unsafe{*addr_of_mut!(STATE)=Some(Phase::new())}}
    #[no_mangle]pub extern "C" fn debris_input()->*mut f64{state().input.as_mut_ptr()}
    #[no_mangle]pub extern "C" fn debris_output()->*const f64{state().output.as_ptr()}
    #[no_mangle]pub extern "C" fn debris_free()->u32{state().free() as u32}
    #[no_mangle]pub extern "C" fn debris_launch(id:u32){state().launch(id as usize,&mut Browser)}
    #[no_mangle]pub extern "C" fn debris_update(dt:f64){state().update(dt,&mut Browser)}
    #[no_mangle]pub extern "C" fn debris_scratch()->*const f64{state().scratch.as_ptr()}
    #[no_mangle]pub extern "C" fn debris_contact(slot:u32){state().contact(slot as usize,&mut Browser)}
    #[no_mangle]pub extern "C" fn debris_settle(slot:u32,floor:f64){state().settle(slot as usize,floor,&mut Browser)}
    #[no_mangle]pub extern "C" fn debris_clear(){state().clear(&mut Browser)}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]struct Scene {events:usize,hits:usize}
    impl Host for Scene {
        fn cast(&mut self,_:usize,s:Vector3,r:Vector3,far:f64)->Option<Hit>{
            if r.y>=0.||s.y<0.{return None;}let d=-s.y/r.y;if d>far{return None;}
            self.hits+=1;let mut point=s;point.add_scaled_vector(&r,d);Some(Hit{point,normal:UP})
        }
        fn low(&mut self,_:usize,_:Quaternion)->f64{0.}
        fn event(&mut self,_:usize,_:f64,_:&[f64]){self.events+=1;}
    }
    fn launch(phase:&mut Phase){phase.input[input::MATRIX..input::MATRIX+16].copy_from_slice(&Matrix4::default().elements);phase.input[input::MATRIX+13]=0.5;phase.input[input::ORIGIN+1]=0.5;phase.input[input::HEALTH]=-50.;phase.input[input::SPEED]=8.;phase.input[input::COLLISIONS]=3.;phase.input[input::TRAIL]=1.;}
    #[test]fn classic_flight_bounces_rests_and_retires(){let mut p=Phase::new();let mut scene=Scene::default();launch(&mut p);p.launch(0,&mut scene);assert_eq!(p.pieces[0].unwrap().velocity,Vector3::new(0.,8.,0.));
        for _ in 0..240{p.update(1./60.,&mut scene);}assert!(scene.hits>0);let piece=p.pieces[0].unwrap();assert!(piece.resting);assert_eq!(piece.velocity,Vector3::default());assert!(piece.position.y>=0.05);
        for _ in 0..900{p.update(1./60.,&mut scene);}assert!(p.order.is_empty());assert_eq!(p.free(),0);
    }
    #[test]fn a_full_paused_burst_keeps_slots_and_never_grows(){let mut p=Phase::new();let mut scene=Scene::default();launch(&mut p);p.input[input::STILL]=1.;
        let capacity=p.order.capacity();for id in 0..lim::CAPACITY{p.launch(id,&mut scene);}assert_eq!(p.free(),lim::CAPACITY);p.launch(usize::MAX,&mut scene);p.launch(0,&mut scene);
        for _ in 0..60{p.update(1.,&mut scene);}assert_eq!(p.order.len(),lim::CAPACITY);assert_eq!(p.order.capacity(),capacity);
        p.clear(&mut scene);assert!(p.order.is_empty());assert!(p.pieces.iter().all(Option::is_none));
        p.launch(0,&mut scene);assert_eq!(p.order,[0]);
    }
}
