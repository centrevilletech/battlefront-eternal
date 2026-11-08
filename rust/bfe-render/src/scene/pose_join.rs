//! InstancePass.writeBody/writeMesh resolve retained scene identities here.
//! World, bind and ready skin bytes stay in renderer memory through drawing.
use super::{SceneRegistry,index};
use crate::math::Mat4;
use crate::scene::layout::scene_pose_ref as p;
use crate::scene_pose_layout::{instance_record as r,pose_instance_flags as flags};
use crate::scene_transform_layout::transform_state as t;
const IDENTITY:Mat4=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
fn retire(words:&mut[f64]){words[r::KIND]=0.;words[r::PROGRAM]=u32::MAX as f64;words[r::GEOMETRY]=u32::MAX as f64;}
impl SceneRegistry{
    // Authored bind matrices are admitted once. Native pose owners publish
    // their inverses at the same lazy placement boundary as their worlds.
    pub fn binding(&self,id:usize,generation:f64)->Option<(Mat4,Mat4)>{
        let reference=self.pose_ref(id,generation)?;
        let at=id.checked_mul(t::STRIDE)?;
        let bind=self.transforms.values.get(at+t::BIND..at+t::BIND+16)?.try_into().ok()?;
        let inverse=if reference[p::OWNER]==0.{
            self.transforms.values.get(at+t::BIND_INVERSE..at+t::BIND_INVERSE+16)?.try_into().ok()?
        }else{
            let owner=reference[p::OWNER] as usize;let model=reference[p::ID] as usize;
            let generation=reference[p::GENERATION] as u32;let node=reference[p::NODE_START] as usize;
            self.poses.node(owner,model,generation,node)?.inverse.unwrap_or(*self.poses.bind(owner,model,generation)?)
        };
        Some((bind,inverse))
    }
    // A malformed or stale identity retires its draw, rather than using the
    // previous occupant's transform. Counts never exceed either output bank.
    pub fn instances_frame(&mut self,input:&mut[f64],bones:&mut[f32],count:usize,base:usize)->usize{
        let count=count.min(input.len().saturating_sub(base)/r::STRIDE);let mut resolved=0;
        for i in 0..count{
            let at=base+i*r::STRIDE;let words=&mut input[at..at+r::STRIDE];
            let flags=words[r::SOURCE_FLAGS] as usize;if flags==0{continue;}
            let Some(id)=index(words[r::SOURCE_SLOT],self.nodes.len())else{retire(words);continue;};
            let generation=words[r::SOURCE_GENERATION];
            let Some(reference)=self.pose_ref(id,generation)else{retire(words);continue;};
            let Some(world)=self.world(id)else{retire(words);continue;};
            words[r::WORLD..r::WORLD+16].copy_from_slice(&world);
            if flags&flags::SKIN!=0{
                let Some((bind,inverse))=self.binding(id,generation)else{retire(words);continue;};
                words[r::BIND..r::BIND+16].copy_from_slice(&bind);
                words[r::BIND_INVERSE..r::BIND_INVERSE+16].copy_from_slice(&inverse);
                let max=bones.len()/16;
                let Some(start)=index(words[r::BONE_AT],max.saturating_add(1))else{retire(words);continue;};
                let Some(count)=index(words[r::BONES],max.saturating_sub(start).saturating_add(1))else{retire(words);continue;};
                if reference[p::OWNER]==0.{self.solve_skeleton(id,generation);}
                if reference[p::OWNER]!=0.&&count>reference[p::SKIN_COUNT] as usize{retire(words);continue;}
                let owner=reference[p::OWNER] as usize;let model=reference[p::ID] as usize;
                let generation=reference[p::GENERATION] as u32;let skin=reference[p::SKIN_START] as usize;
                for b in 0..count{
                    let Some(source)=(if owner==0{self.skeleton_skin(id,words[r::SOURCE_GENERATION],b)}else{self.poses.skin(owner,model,generation,skin+b)})else{retire(words);break;};
                    bones[(start+b)*16..(start+b+1)*16].copy_from_slice(source);
                }
            }else{
                words[r::BIND..r::BIND+16].copy_from_slice(&IDENTITY);
                words[r::BIND_INVERSE..r::BIND_INVERSE+16].fill(0.);
            }
            if words[r::KIND]!=0.{resolved+=1;}
        }
        resolved
    }
    // Resource admission is cold. Ready skin spans are addressed by logical
    // bone ids because different model arenas need not be contiguous.
    pub fn copy_skin(&mut self,id:usize,generation:f64,out:&mut[f32],count:usize,visibility:&[f64])->bool{
        let Some(reference)=self.pose_ref(id,generation)else{return false;};
        let owner=reference[p::OWNER] as usize;
        if owner==0&&!self.solve_skeleton_flags(id,generation,visibility){return false;}
        let model=reference[p::ID] as usize;let source_generation=reference[p::GENERATION] as u32;let start=reference[p::SKIN_START] as usize;
        let count=count.min(out.len()/16);
        for i in 0..count{
            let Some(skin)=(if owner==0{self.skeleton_skin(id,generation,i)}else{self.poses.skin(owner,model,source_generation,start+i)})else{return false;};
            out[i*16..(i+1)*16].copy_from_slice(skin);
        }true
    }
    pub fn admit_skin(&mut self,id:usize,generation:f64,count:usize,words:usize){
        let words=words.min(crate::actor_layout::limit::BONES*16);let mut output=std::mem::take(&mut self.skin_admit);output.resize(words,0.);output.fill(0.);
        self.copy_skin(id,generation,&mut output,count,&[]);self.skin_admit=output;
    }
    pub fn uniform_world(&self,id:usize,generation:f64,input:&mut[f64])->bool{
        use crate::cameras::layout::uniform_input as u;if input.len()<u::LEN{return false;}
        let Some(world)=self.pose_ref(id,generation).and_then(|_|self.world(id))else{return false;};input[u::WORLD..u::WORLD+16].copy_from_slice(&world);true
    }
    // Locked first-person masks use the same native world on both sides of
    // the history pair, without publishing it to a scene proxy.
    pub fn locked_motion(&self,input:&mut[f64],record:usize,id:usize,generation:f64){
        use crate::motion as m;
        let at=m::IN_RECORDS+record*m::REC_STRIDE;if at+m::REC_STRIDE>input.len(){return;}
        let Some(world)=self.pose_ref(id,generation).and_then(|_|self.world(id))else{return;};
        let words=&mut input[at..at+m::REC_STRIDE];
        for field in [m::REC_WORLD,m::REC_PREVIOUS_WORLD]{words[field..field+16].copy_from_slice(&world);}
        for field in [m::REC_BIND,m::REC_BIND_INVERSE,m::REC_PREVIOUS_BIND_INVERSE]{words[field..field+16].copy_from_slice(&IDENTITY);}
    }
    // Cold history admission copies the authored bind in native memory. A
    // borrowed WASM array must not be passed back through an allocating ABI.
    pub fn history_model(&self,history:&mut crate::pose_history::PoseHistory,skin:u32,id:usize,generation:f64)->u32{
        if self.pose_ref(id,generation).is_none(){return 0;}
        let at=id*t::STRIDE+t::BIND;let bind=self.transforms.values.get(at..at+16).and_then(|s|s.try_into().ok()).unwrap_or(IDENTITY);
        history.model(skin,if skin==0{&IDENTITY}else{&bind})
    }
    // MotionVectors.prepare selects the same retained current pose. Previous
    // displayed poses remain in PoseHistory and never return through a facade.
    pub fn history_source(&mut self,history:&mut crate::pose_history::PoseHistory,id:u32,slot:usize,generation:f64)->bool{
        let Some(reference)=self.pose_ref(slot,generation)else{return false;};
        let Some(world)=self.world(slot)else{return false;};
        let inverse=self.binding(slot,generation).map_or(IDENTITY,|(_,inverse)|inverse);
        let owner=reference[p::OWNER] as usize;let model=reference[p::ID] as usize;
        let source_generation=generation;
        if owner==0{self.solve_skeleton(slot,generation);}
        let generation=reference[p::GENERATION] as u32;let skin=reference[p::SKIN_START] as usize;
        let count=if owner==0{self.skeleton_skin(slot,source_generation,0).map_or(0,|_|usize::MAX)}else{reference[p::SKIN_COUNT] as usize};
        history.source(id,&world,&inverse,count,|i|if owner==0{self.skeleton_skin(slot,source_generation,i)}else{self.poses.skin(owner,model,generation,skin+i)})
    }
}
