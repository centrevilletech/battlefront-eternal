//! Immutable rider samples and source coordinates stay beside their native
//! hull/prop owners. JS names only resource handles and a source revision.
//! A lease retains the source visible at arrival, even if the next hull
//! phase advances before the unit phase consumes it.
use std::{cell::RefCell,rc::Rc};
use three::{Matrix4,Quaternion,Vector3};
use crate::layout::{unit_frame as f,unit_input as i,prop_output as po,prop_held as held};

// A source is a fixed numeric value. A lease can retain it independently
// of the source owner's next frame, without sharing a mutable rig scratch.
#[derive(Clone,Copy,Default)]
pub struct Source {
    pub frame:Matrix4,pub position:Vector3,pub velocity:Vector3,
    pub quaternion:Quaternion,pub age:f64,pub yaw:f64,pub pitch:f64,
    pub hull_yaw:f64,pub yaw_rate:f64,pub hull_yaw_rate:f64,
    pub speed:f64,pub hover_phase:f64,pub height:f64,pub animal:bool,pub shown:bool,
    pub turn_rate:f64,pub pitch_limits:[f64;2],pub yaw_limits:Option<[f64;2]>,pub wrap:bool,
}
// Four publications cover ordinary and predicted hull batches plus cold
// placement. A slow consumer keeps its own fixed birth sample once admitted.
const HISTORY:usize=4;
#[derive(Clone,Copy,Default)]
struct History {rows:[Source;HISTORY],epochs:[f64;HISTORY],count:usize,next:usize}
impl History {
    fn push(&mut self,epoch:f64,source:Source){self.rows[self.next]=source;self.epochs[self.next]=epoch;
        self.next=(self.next+1)%HISTORY;self.count=(self.count+1).min(HISTORY);}
    fn at(&self,epoch:f64)->Option<Source>{
        let mut chosen=None;let mut best=f64::NEG_INFINITY;
        for k in 0..self.count {if self.epochs[k]<=epoch&&self.epochs[k]>best {chosen=Some(self.rows[k]);best=self.epochs[k];}}
        // An overrun retains the oldest available source, never a frame trap.
        chosen.or_else(||(self.count>0).then(||self.rows[self.next%self.count]))
    }
}
#[derive(Default)]
pub struct Vehicles {ranges:Vec<std::ops::Range<usize>>,rows:Vec<History>,pub epoch:f64}
pub type SharedVehicles=Rc<RefCell<Vehicles>>;
impl Vehicles {
    /// Registration is the only operation that grows the source store.
    pub fn rig(&mut self,seats:usize){let start=self.rows.len();self.rows.resize(start+seats,History::default());self.ranges.push(start..start+seats);}
    pub fn publish(&mut self,rig:usize,seat:usize,source:Source){
        let Some(range)=self.ranges.get(rig)else{return};if seat>=range.len(){return;}
        self.rows[range.start+seat].push(self.epoch,source);
    }
    // Foreign resource handles never become unchecked store indices. The
    // nearest retained revision also bounds a delayed or excessive request.
    pub fn get(&self,rig:f64,seat:f64,epoch:f64)->Option<Source>{
        if !rig.is_finite()||rig<0.0||!seat.is_finite()||seat<0.0{return None;}
        let range=self.ranges.get(rig as usize)?;if seat as usize>=range.len(){return None;}
        self.rows[range.start+seat as usize].at(epoch)
    }
}
#[derive(Clone,Copy,Default)]
pub struct Held {pub placed:bool,pub time:f64,pub frame:Matrix4}
#[derive(Default)]
pub struct Props {pub turrets:Vec<Source>,pub held:Vec<Held>}
pub type SharedProps=Rc<RefCell<Props>>;
impl Props {
    /// The prop phase has already resolved root motion against its grab node.
    pub fn held(&mut self,index:usize,row:&[f64]){let Some(out)=self.held.get_mut(index)else{return};
        out.placed=row[po::HELD]==held::TIMED as f64&&row[po::HELD_PLACED]!=0.0;
        out.time=row[po::HELD_TIME];out.frame.elements.copy_from_slice(&row[po::HELD_FRAME..po::HELD_FRAME+16]);}
}
#[derive(Clone,Copy,Default)]
struct Birth {version:f64,rig:f64,seat:f64,generation:f64,source:Option<Source>}
#[derive(Default)]
pub struct Bindings {vehicles:Option<SharedVehicles>,props:Option<SharedProps>,births:Vec<Birth>}
impl Bindings {
    pub fn reserve(&mut self,count:usize){self.births.resize(count,Birth::default());}
    /// Cold owner links retain native memory, without retaining host objects.
    pub fn bind(&mut self,vehicles:&SharedVehicles,props:&SharedProps){self.vehicles=Some(vehicles.clone());self.props=Some(props.clone());}
    // Collision reads the latest native sample; display leases retain birth.
    // Both consumers copy fixed values before releasing the source borrow.
    pub fn live_source(&self,rig:f64,seat:f64,turret:f64)->Option<Source>{
        if turret>=0.0 {self.props.as_ref()?.borrow().turrets.get(turret as usize).copied()}
        else {let vehicles=self.vehicles.as_ref()?.borrow();vehicles.get(rig,seat,vehicles.epoch)}
    }
    pub fn held_source(&self,prop:f64)->Option<Held>{
        if !(prop>=0.0){return None;}self.props.as_ref()?.borrow().held.get(prop as usize).copied().filter(|h|h.placed)
    }
    pub fn fill(&mut self,input:&mut [f64]){
        // Unit collection already knows whether any body has a holder.
        // Ordinary infantry frames need no source borrow or second row walk.
        if input[f::SOURCES]==0.0 {return;}
        let (Some(vehicles),Some(props))=(&self.vehicles,&self.props)else{return};
        let vehicles=vehicles.borrow();let props=props.borrow();
        let count=(input[f::ACTORS]as usize).min(input.len().saturating_sub(f::STRIDE)/i::STRIDE);
        for n in 0..count {let row=&mut input[f::STRIDE+n*i::STRIDE..][..i::STRIDE];
            if row[i::SOURCE_PROP]<0.0&&row[i::RIDER_KIND]==0.0 {continue;}
            let Some(birth)=self.births.get_mut(row[i::ACTOR]as usize)else{continue};
            if row[i::SOURCE_PROP]>=0.0 {
                row[i::HELD]=0.0;
                if let Some(held)=props.held.get(row[i::SOURCE_PROP]as usize).filter(|s|s.placed){
                    row[i::HELD]=1.0;row[i::HELD_TIME]=held.time;row[i::HELD_MATRIX..i::HELD_MATRIX+16].copy_from_slice(&held.frame.elements);
                }
            }
            let turret=row[i::RIDER_KIND]==2.0;
            let current=if turret {if row[i::SOURCE_TURRET]>=0.0{props.turrets.get(row[i::SOURCE_TURRET]as usize).copied()}else{None}}
                else{vehicles.get(row[i::SOURCE_RIG],row[i::RIDER_SEAT],vehicles.epoch)};
            // A missing rig/seat leaves that body unplaced; another row can
            // still complete its frame and publish its existing resources.
            let Some(current)=current else{continue};
            // Identity, life and lease reset guard the immutable sample.
            // Re-reading a live source never advances its arrival snapshot.
            let key=(row[i::SOURCE_VERSION],row[i::SOURCE_RIG],row[i::RIDER_SEAT],row[i::GENERATION]);
            if row[i::RIDER_RESET]!=0.0||key!=(birth.version,birth.rig,birth.seat,birth.generation){
                birth.version=key.0;birth.rig=key.1;birth.seat=key.2;birth.generation=key.3;
                birth.source=if turret{Some(current)}else{vehicles.get(key.1,key.2,key.0)};
            }
            // Birth position and current drawn frame have separate clocks.
            // Prediction consumes the former; animation follows the latter.
            let old=birth.source.unwrap_or(current);
            row[i::RIDER_MATRIX..i::RIDER_MATRIX+16].copy_from_slice(&current.frame.elements);
            row[i::RIDER]=if current.shown&&(!turret||row[i::OWN]!=0.0||row[i::RIDER_LIVE]!=0.0){1.0}else{0.0};
            row[i::MOUNT_ENCLOSED]=if current.shown{0.0}else{1.0};
            row[i::RIDER_ANIMAL]=if current.animal{1.0}else{0.0};
            // Preserve the whole hull turn for an animal's pelvis offset.
            // Ordinary riders instead take the authored node matrix above.
            row[i::RIDER_QUATERNION..i::RIDER_QUATERNION+4].copy_from_slice(&[current.quaternion.x,current.quaternion.y,current.quaternion.z,current.quaternion.w]);
            vector(row,i::RIDER_VELOCITY,current.velocity);row[i::RIDER_SPEED]=current.speed;
            row[i::RIDER_AGE]=current.age;row[i::RIDER_HOVER_PHASE]=current.hover_phase;
            row[i::RIDER_YAW_RATE]=if current.animal{current.hull_yaw_rate}else{current.yaw_rate};
            row[i::RIDER_TURN_RATE]=current.turn_rate;
            row[i::RIDER_PITCH_LIMITS..i::RIDER_PITCH_LIMITS+2].copy_from_slice(&current.pitch_limits);
            row[i::RIDER_HAS_YAW_LIMITS]=if current.yaw_limits.is_some(){1.0}else{0.0};
            if let Some(limits)=current.yaw_limits{row[i::RIDER_YAW_LIMITS..i::RIDER_YAW_LIMITS+2].copy_from_slice(&limits);}
            row[i::RIDER_WRAP_YAW]=if current.wrap{1.0}else{0.0};
            // Hull pilot poses freeze at lease birth. Turret aim remains live.
            row[i::RIDER_YAW]=if turret{current.yaw}else{old.yaw};row[i::RIDER_PITCH]=if turret{current.pitch}else{old.pitch};
            row[i::RIDER_HULL_YAW]=old.hull_yaw;row[i::RIDER_SAMPLE_YAW_RATE]=old.yaw_rate;
            // Authored stand-ins hold an explicit frame. Ordinary timed
            // clips use the captured source age, including repeated draws.
            if row[i::RIDER_FIXED_TIME]==0.0{row[i::RIDER_TIME]=if turret{0.0}else{old.age};}
            row[i::LEASE_FACTS]=if turret{0.0}else{1.0};
            vector(row,i::LEASE_POSITION,old.position);vector(row,i::LEASE_VELOCITY,old.velocity);row[i::LEASE_HEIGHT]=old.height;
        }
    }
}
fn vector(row:&mut [f64],at:usize,v:Vector3){row[at..at+3].copy_from_slice(&[v.x,v.y,v.z]);}
