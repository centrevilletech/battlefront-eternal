//! Temporal poses retain entity generations across visibility compaction.
use crate::{core::{Core,MultiDraw},instances as I,motion as M};
use glow::HasContext;
use std::collections::HashMap;
type Key=(u64,u32,u32,u32);
/// Slots reserved at the end of `MotionPass` for first-person view masks so
/// heavy moving scenes cannot starve first-person velocity locking.
pub const VIEW_RESERVE: usize = 16;
fn key(row:&[f64])->Option<Key>{(row[I::REC_MOTION_ID]>0.).then(||(row[I::REC_MOTION_ID]as u64,row[I::REC_MOTION_GENERATION]as u32,row[I::REC_MOTION_PART]as u32,row[I::REC_GEOMETRY]as u32))}
fn source_limit(extended: bool) -> usize {
    if extended { I::MAX_RECORDS } else { M::INITIAL_RECORDS }
}
pub struct NativeMotion {
    programs:[u32;3],
    rows:Vec<[f64;I::REC_STRIDE]>,
    pending:Vec<[f64;I::REC_STRIDE]>,
    bones:Vec<f32>,
    pending_bones:Vec<f32>,
    lookup:HashMap<Key,usize>,
    pub count:usize,
}
impl NativeMotion {
    pub fn new<C:HasContext,D:MultiDraw>(core:&mut Core<C,D>)->Result<Self,String>{
        Self::with_source_limit(core, M::INITIAL_RECORDS)
    }
    /// Reserve history for the source records that this pass can admit.
    pub fn for_pass<C: HasContext, D: MultiDraw>(core: &mut Core<C, D>, pass: &M::MotionPass<C>) -> Result<Self, String> {
        Self::with_source_limit(core, source_limit(pass.extended))
    }
    fn with_source_limit<C: HasContext, D: MultiDraw>(core: &mut Core<C, D>, records: usize) -> Result<Self, String> {
        let mut programs=[0;3];for(i,key)in ["effects/motion-rigid","effects/motion-skinned","effects/motion-locked"].iter().enumerate(){programs[i]=core.add_owned_program(key)?;}
        Ok(Self {
            programs,
            rows: Vec::with_capacity(records),
            pending: Vec::with_capacity(records),
            bones: Vec::with_capacity(I::MAX_BONES * 16),
            pending_bones: Vec::with_capacity(I::MAX_BONES * 16),
            lookup: HashMap::with_capacity(records),
            count: 0,
        })
    }
    /// Prepares moving-object motion proxies and returns how many moving proxies
    /// had to be omitted because the motion record or bone bank was full.
    pub fn prepare<C:HasContext,D:MultiDraw>(&mut self,core:&Core<C,D>,motion:&mut M::MotionPass<C>,records:usize,valid:bool)->usize{
        self.count=0;self.lookup.clear();self.pending.clear();self.pending_bones.clear();
        for(i,row)in self.rows.iter().enumerate(){if let Some(key)=key(row){self.lookup.insert(key,i);}}
        let input_limit = source_limit(motion.extended);
        let reserve = if motion.extended { VIEW_RESERVE } else { 0 };
        let scene_limit=motion.record_capacity().saturating_sub(reserve);
        let bone_limit=motion.bone_capacity();
        let mut history_end = 0;
        let mut bone=0;
        let mut dropped=0;
        for i in 0..records.min(input_limit){
            let row:&[f64;I::REC_STRIDE]=core.input[I::IN_RECORDS+i*I::REC_STRIDE..][..I::REC_STRIDE].try_into().unwrap();
            let count=row[I::REC_BONES]as usize;
            let start=row[I::REC_BONE_AT]as usize;
            let Some(end) = start.checked_add(count).filter(|&end| end <= core.bones.len()/16 && end <= I::MAX_BONES) else {continue};
            let first = start * 16;
            let Some(geometry)=core.geometries.get(row[I::REC_GEOMETRY]as usize)else{continue;};
            let previous=if valid{
                key(row)
                    .and_then(|k|self.lookup.get(&k))
                    .map(|&idx|&self.rows[idx])
                    .filter(|p|p[I::REC_BONES]==row[I::REC_BONES])
            }else{None};
            let previous=previous.unwrap_or(row);
            let previous_at=previous[I::REC_BONE_AT]as usize*16;
            let current=&core.bones[first..first+count*16];
            let last=if std::ptr::eq(previous,row){current}else{self.bones.get(previous_at..previous_at+count*16).unwrap_or(current)};
            if motion.extended || bone+count<=bone_limit {
                self.pending.push(*row);
                if count != 0 { history_end = history_end.max(end * 16); }
            }
            // The depth reconstruction already supplies camera motion. An
            // unchanged object needs no overriding velocity proxy.
            if row[I::REC_WORLD..I::REC_WORLD+16] == previous[I::REC_WORLD..I::REC_WORLD+16]
                && row[I::REC_BIND..I::REC_BIND+16] == previous[I::REC_BIND..I::REC_BIND+16]
                && row[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE+16] == previous[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE+16]
                && current == last { continue; }
            if self.count>=scene_limit || bone+count>bone_limit {
                dropped+=1;
                continue;
            }
            let output=&mut motion.input[M::IN_RECORDS+self.count*M::REC_STRIDE..][..M::REC_STRIDE];output.fill(0.);
            output[M::REC_KIND]=M::KIND_PROXY;output[M::REC_PROGRAM]=self.programs[usize::from(row[I::REC_KIND]==1.||row[I::REC_KIND]==I::KIND_BLENDED)]as f64;
            output[M::REC_GEOMETRY]=row[I::REC_GEOMETRY];output[M::REC_COUNT]=geometry.index_count as f64;output[M::REC_SIDE]=(row[I::REC_FLAGS]as u32&3)as f64;
            output[M::REC_BONE_AT]=bone as f64;output[M::REC_PREVIOUS_AT]=bone as f64;
            for(to,from)in [(M::REC_WORLD,I::REC_WORLD),(M::REC_BIND,I::REC_BIND),(M::REC_BIND_INVERSE,I::REC_BIND_INVERSE)]{output[to..to+16].copy_from_slice(&row[from..from+16]);}
            output[M::REC_PREVIOUS_WORLD..M::REC_PREVIOUS_WORLD+16].copy_from_slice(&previous[I::REC_WORLD..I::REC_WORLD+16]);output[M::REC_PREVIOUS_BIND_INVERSE..M::REC_PREVIOUS_BIND_INVERSE+16].copy_from_slice(&previous[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE+16]);
            motion.bones[bone*16..(bone+count)*16].copy_from_slice(current);
            motion.previous[bone*16..(bone+count)*16].copy_from_slice(last);
            bone+=count;self.count+=1;
        }
        // Keep source offsets. Overlapping palettes reference the same snapshot.
        self.pending_bones.extend_from_slice(&core.bones[..history_end]);
        motion.input[M::IN_BONES]=bone as f64;motion.input[M::IN_PREVIOUS_BONES]=bone as f64;
        dropped
    }
    /// Appends first-person locked velocity masks into the reserved tail and
    /// returns how many parts were omitted if total capacity was exceeded.
    pub fn view<C:HasContext,D:MultiDraw>(&mut self,core:&Core<C,D>,motion:&mut M::MotionPass<C>,records:usize)->usize{
        let limit=motion.record_capacity();
        let input_limit = source_limit(motion.extended);
        let mut dropped=0;
        for i in 0..records.min(input_limit){
            let row=&core.input[I::IN_RECORDS+i*I::REC_STRIDE..][..I::REC_STRIDE];let Some(g)=core.geometries.get(row[I::REC_GEOMETRY]as usize)else{continue;};
            if self.count>=limit{dropped+=1;continue;}
            let out=&mut motion.input[M::IN_RECORDS+self.count*M::REC_STRIDE..][..M::REC_STRIDE];out.fill(0.);out[M::REC_KIND]=M::KIND_LOCKED;out[M::REC_PROGRAM]=self.programs[2]as f64;out[M::REC_GEOMETRY]=row[I::REC_GEOMETRY];out[M::REC_COUNT]=g.index_count as f64;out[M::REC_SIDE]=2.;
            out[M::REC_WORLD..M::REC_WORLD+16].copy_from_slice(&row[I::REC_WORLD..I::REC_WORLD+16]);self.count+=1;
        }
        dropped
    }
    pub fn complete(&mut self,success:bool){if success{std::mem::swap(&mut self.rows,&mut self.pending);std::mem::swap(&mut self.bones,&mut self.pending_bones);}else{self.rows.clear();self.bones.clear();}}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn moving_pose_keeps_identity_but_a_new_life_drops_history(){
        let mut row=[0.;I::REC_STRIDE];row[I::REC_MOTION_ID]=8589934600.;row[I::REC_MOTION_GENERATION]=2.;row[I::REC_GEOMETRY]=7.;
        let previous=key(&row);row[I::REC_WORLD+12]=10.;assert_eq!(key(&row),previous);
        row[I::REC_MOTION_GENERATION]=3.;assert_ne!(key(&row),previous);
        row[I::REC_MOTION_ID]=0.;assert!(key(&row).is_none());
    }
}
