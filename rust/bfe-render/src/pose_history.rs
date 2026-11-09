//! MotionVectors.prepare/commit retain temporal pose state in native storage.
//! Admission may grow banks; a displayed frame only writes reserved spans.
use crate::cameras::layout::{history_pose as p, limit as l,history_receipt as receipt};
use crate::math::Mat4;
use crate::motion as m;

const IDENTITY: Mat4 = [1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
const NONE: usize = usize::MAX;

struct Model {
    alive: bool, skin: usize, seen: Option<u64>,
    bind: Mat4, previous: Mat4, inverse: Mat4,
}
struct Skin { start: usize, count: usize, frame: Option<u64>, written:u64 }
struct Flyer { checked:Option<u64>, still:bool }

#[derive(Default)]
pub struct PoseHistory {
    pub poses: Vec<f64>, pub selected: Vec<u32>, pub records: Vec<u32>,
    pub current: Vec<f32>, previous: Vec<f32>,
    models: Vec<Model>, skins: Vec<Skin>, flyers:Vec<Flyer>, prepared: usize, frame: u64, writes:u64,
    pub clipped: u64,
}

impl PoseHistory {
    pub fn sequence(&self)->u32{self.frame as u32}

    // SkyFlyers.motionFrame consumed the stepped fact once per displayed
    // history frame. A failed frame must leave later stepped facts pending.
    pub fn flyer(&mut self)->u32{
        if self.flyers.len()>=l::SKY{self.clipped+=1;return 0;}
        self.flyers.push(Flyer{checked:None,still:true});self.flyers.len() as u32
    }
    // Bit zero is the still uniform; bit one acknowledges consuming the
    // producer's stepped fact. Repeated meshes cannot consume it twice.
    pub fn flyer_frame(&mut self,id:u32,stepped:bool)->u32{
        let Some(flyer)=self.flyers.get_mut(id.wrapping_sub(1) as usize)else{return receipt::STILL as u32;};
        let fresh=flyer.checked!=Some(self.frame);
        if fresh{flyer.checked=Some(self.frame);flyer.still = !stepped;}
        (if flyer.still{receipt::STILL as u32}else{0})|if fresh{receipt::CONSUMED as u32}else{0}
    }

    // SceneRegistry supplies the current retained pose directly. Validate the
    // full skin span before changing a model, including a cold rig replacement.
    pub fn source<'a>(&mut self,id:u32,world:&Mat4,inverse:&Mat4,count:usize,mut skin:impl FnMut(usize)->Option<&'a[f32]>)->bool{
        let Some(model)=self.models.get(id.wrapping_sub(1) as usize).filter(|m|m.alive)else{return false;};
        let skinned=model.skin!=NONE;
        let target=if !skinned{None}else{self.skins.get(model.skin)};
        if let Some(target)=target{
            if count<target.count||(0..target.count).any(|i|skin(i).is_none_or(|s|s.len()!=16)){self.clipped+=1;return false;}
            for i in 0..target.count{
                let Some(source)=skin(i)else{return false;};
                self.current[target.start+i*16..target.start+(i+1)*16].copy_from_slice(source);
            }
        }
        let at=(id as usize-1)*p::LEN;
        self.poses[at+p::WORLD..at+p::WORLD+16].copy_from_slice(world);
        self.poses[at+p::INVERSE..at+p::INVERSE+16].copy_from_slice(if skinned{inverse}else{&IDENTITY});true
    }

    // A skeleton's identity is independent of its meshes. Shared rigs commit
    // once even when several groups or held parts draw from the same skin.
    pub fn skin(&mut self, count: usize) -> u32 {
        if count > l::HISTORY_BONES.saturating_sub(self.current.len()/16) {
            self.clipped += 1; return 0;
        }
        let id = self.skins.len(); let start = self.current.len();
        self.current.resize(start + count*16, 0.);
        self.previous.resize(start + count*16, 0.);
        self.skins.push(Skin {start, count, frame: None,written:0}); (id+1) as u32
    }
    pub fn skin_start(&self, id: u32) -> usize {
        self.skins.get(id.wrapping_sub(1) as usize).map_or(0, |s| s.start)
    }
    pub fn model(&mut self, skin: u32, bind: &[f64]) -> u32 {
        if self.models.len() >= l::HISTORY_MODELS || (skin != 0 && skin as usize > self.skins.len()) {
            self.clipped += 1; return 0;
        }
        let mut matrix = IDENTITY;
        if bind.len() == 16 { matrix.copy_from_slice(bind); }
        let id = self.models.len();
        self.models.push(Model {alive:true, skin:if skin==0 {NONE}else{skin as usize-1}, seen:None,
            bind:matrix, previous:IDENTITY, inverse:IDENTITY});
        self.poses.resize((id+1)*p::LEN, 0.); self.selected.resize(id+1, 0);
        // Record ids follow the existing motion pass capacity, not the number
        // of historical registrations accumulated during pooled lifetimes.
        self.records.resize(m::MAX_RECORDS, 0); (id+1) as u32
    }
    pub fn reset(&mut self, id: u32) {
        if let Some(model)=self.models.get_mut(id.wrapping_sub(1) as usize) {model.seen=None;}
    }
    // A cold skeleton replacement gets its own current/previous skin span.
    // Pose continuity remains a mesh decision, independent of skin storage.
    pub fn reskin(&mut self,id:u32,skin:u32){
        if skin==0 || skin as usize>self.skins.len(){self.clipped+=1;return;}
        if let Some(model)=self.models.get_mut(id.wrapping_sub(1) as usize){model.skin=skin as usize-1;}
    }
    pub fn release(&mut self, id: u32) {
        if let Some(model)=self.models.get_mut(id.wrapping_sub(1) as usize) {model.alive=false;model.seen=None;}
    }
    // MotionVectors.prepare used the previous displayed frame, including a
    // long pause with no commits. Squared distance preserves JS f64 order.
    pub fn prepare(&mut self, count: usize) {
        self.prepared=count.min(self.selected.len());
        if count>self.prepared {self.clipped+=1;}
        for ordinal in 0..self.prepared {
            let id=self.selected[ordinal].wrapping_sub(1) as usize;
            let Some(model)=self.models.get_mut(id).filter(|m|m.alive) else {continue;};
            let at=id*p::LEN; let world=&self.poses[at+p::WORLD..at+p::WORLD+16];
            let dx=model.previous[12]-world[12];let dy=model.previous[13]-world[13];let dz=model.previous[14]-world[14];
            let fresh=model.seen!=Some(self.frame.wrapping_sub(1)) || dx*dx+dy*dy+dz*dz>64.;
            if fresh {
                model.previous.copy_from_slice(world);
                model.inverse.copy_from_slice(&self.poses[at+p::INVERSE..at+p::INVERSE+16]);
            }
            if let Some(skin)=self.skins.get(model.skin) {
                if fresh || skin.frame!=Some(self.frame.wrapping_sub(1)) {
                    let span=skin.start..skin.start+skin.count*16;
                    self.previous[span.clone()].copy_from_slice(&self.current[span]);
                }
            }
        }
    }
    // MotionPass.draw consumes the native history directly. No previous
    // matrix, inverse or skin is copied back into a Three object or texture.
    pub fn write(&mut self, count: usize, input: &mut [f64], bones: &mut [f32], previous: &mut [f32]) {
        self.writes=self.writes.wrapping_add(1).max(1);
        let count=count.min(self.records.len()).min(input.len().saturating_sub(m::IN_RECORDS)/m::REC_STRIDE);
        for ordinal in 0..count {
            let id=self.records[ordinal].wrapping_sub(1) as usize;
            let Some(model)=self.models.get(id).filter(|m|m.alive) else {continue;};
            let at=id*p::LEN;let row=m::IN_RECORDS+ordinal*m::REC_STRIDE;
            input[row+m::REC_WORLD..row+m::REC_WORLD+16].copy_from_slice(&self.poses[at+p::WORLD..at+p::WORLD+16]);
            input[row+m::REC_PREVIOUS_WORLD..row+m::REC_PREVIOUS_WORLD+16].copy_from_slice(&model.previous);
            input[row+m::REC_BIND..row+m::REC_BIND+16].copy_from_slice(&model.bind);
            input[row+m::REC_BIND_INVERSE..row+m::REC_BIND_INVERSE+16].copy_from_slice(&self.poses[at+p::INVERSE..at+p::INVERSE+16]);
            input[row+m::REC_PREVIOUS_BIND_INVERSE..row+m::REC_PREVIOUS_BIND_INVERSE+16].copy_from_slice(&model.inverse);
            if let Some(skin)=self.skins.get_mut(model.skin) {
                if skin.written==self.writes{continue;}skin.written=self.writes;
                let offset=input[row+m::REC_BONE_AT];
                if !offset.is_finite() || offset<0. || offset.fract()!=0. || offset>m::MAX_BONES as f64 {self.clipped+=1;continue;}
                let start=offset as usize*16;let length=skin.count*16;
                if start>bones.len() || start>previous.len() || length>bones.len().saturating_sub(start) || length>previous.len().saturating_sub(start) {self.clipped+=1;continue;}
                bones[start..start+length].copy_from_slice(&self.current[skin.start..skin.start+length]);
                previous[start..start+length].copy_from_slice(&self.previous[skin.start..skin.start+length]);
            }
        }
    }
    // A successful complete frame alone advances temporal state. Several
    // prepared meshes sharing a skin must not advance that skin twice.
    pub fn commit(&mut self) {
        for ordinal in 0..self.prepared {
            let id=self.selected[ordinal].wrapping_sub(1) as usize;
            let Some(model)=self.models.get_mut(id).filter(|m|m.alive) else {continue;};
            let at=id*p::LEN;
            model.previous.copy_from_slice(&self.poses[at+p::WORLD..at+p::WORLD+16]);
            model.inverse.copy_from_slice(&self.poses[at+p::INVERSE..at+p::INVERSE+16]);model.seen=Some(self.frame);
            if let Some(skin)=self.skins.get_mut(model.skin) {
                if skin.frame!=Some(self.frame) {
                    let span=skin.start..skin.start+skin.count*16;
                    self.previous[span.clone()].copy_from_slice(&self.current[span]);skin.frame=Some(self.frame);
                }
            }
        }
        self.prepared=0;self.frame=self.frame.wrapping_add(1);
    }
}
