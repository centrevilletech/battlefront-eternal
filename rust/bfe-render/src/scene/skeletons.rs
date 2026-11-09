//! Cold skeleton admission retains authored inverses and qualified bone ids.
//! Generic utility rigs use native worlds; unit/hull/prop skins arrive ready.
use super::{SceneRegistry,NONE};
use crate::math::Mat4;
use crate::scene_pose_layout::pose_limits as limit;
struct Skeleton{generation:f64,start:usize,bones:Vec<(usize,f64,Mat4,usize)>}
#[derive(Default)]
pub struct Skeletons{bindings:Vec<Skeleton>,by_node:Vec<usize>,pub values:Vec<f32>}
impl SceneRegistry{
    pub fn skeleton_register(&mut self,id:usize,generation:f64,bones:&[u32],generations:&[u32],inverses:&[f64],controls:&[u32])->bool{
        if self.pose_ref(id,generation).is_none()||bones.len()!=generations.len()||inverses.len()!=bones.len()*16||controls.len()!=bones.len()
            ||bones.iter().zip(generations).any(|(&id,&generation)|self.pose_ref(id as usize,generation as f64).is_none()){
            self.rejected+=1;return false;
        }
        if self.skeletons.by_node.len()<=id{self.skeletons.by_node.resize(id+1,NONE);}
        let existing=self.skeletons.by_node[id];
        let old=self.skeletons.bindings.get(existing).map_or(0,|s|s.bones.len());
        let total=self.skeletons.values.len()/16-old;
        if bones.len()>limit::BONES.saturating_sub(total){self.rejected+=1;return false;}
        let start=if let Some(skeleton)=self.skeletons.bindings.get(existing){skeleton.start}else{self.skeletons.values.len()};
        // Cold pooled-rig replacement compacts the retained arena. No external
        // pointer names these spans; live frames only resolve qualified ids.
        if existing!=NONE{
            self.skeletons.values.splice(start..start+old*16,std::iter::repeat_n(0.,bones.len()*16));
            for (slot,skeleton) in self.skeletons.bindings.iter_mut().enumerate(){if slot!=existing&&skeleton.start>=start+old*16{skeleton.start=skeleton.start-old*16+bones.len()*16;}}
        }else{self.skeletons.values.resize(start+bones.len()*16,0.);}
        let bones=bones.iter().zip(generations).zip(inverses.as_chunks::<16>().0).zip(controls).map(|(((&id,&generation),&inverse),&control)|(id as usize,generation as f64,inverse,control as usize)).collect();
        let skeleton=Skeleton{generation,start,bones};
        if existing==NONE{self.skeletons.by_node[id]=self.skeletons.bindings.len();self.skeletons.bindings.push(skeleton);}else{self.skeletons.bindings[existing]=skeleton;}true
    }
    // Selection has already advanced the owning lazy placement boundary.
    // Solve only the generic mesh being requested; this does not place actors.
    pub fn solve_skeleton(&mut self,id:usize,generation:f64)->bool{self.solve_skeleton_flags(id,generation,&[])}
    pub fn solve_skeleton_flags(&mut self,id:usize,generation:f64,visibility:&[f64])->bool{
        let Some(&binding)=self.skeletons.by_node.get(id)else{return false;};
        let Some(skeleton)=self.skeletons.bindings.get(binding)else{return false;};
        if skeleton.generation!=generation{return false;}let start=skeleton.start;let count=skeleton.bones.len();
        for i in 0..count{
            let (bone,generation,inverse,control)=self.skeletons.bindings[binding].bones[i];
            if self.pose_ref(bone,generation).is_none(){return false;}
            let Some(world)=self.world(bone)else{return false;};
            // CrowdShadows used a zero inverse for a hidden source. Multiply
            // by that matrix too, preserving the old signed-zero f32 bytes.
            let inverse=if control==0||visibility.get(control-1).is_some_and(|v|*v!=0.){inverse}else{[0.;16]};
            let matrix=crate::math::mul(&world,&inverse);
            for (k, &m_val) in matrix.iter().enumerate(){self.skeletons.values[start+i*16+k]=m_val as f32;}
        }true
    }
    pub fn skeleton_count(&self,id:usize,generation:f64)->Option<usize>{
        let binding=*self.skeletons.by_node.get(id)?;let skeleton=self.skeletons.bindings.get(binding)?;
        (skeleton.generation==generation).then_some(skeleton.bones.len())
    }
    pub fn skeleton_skin(&self,id:usize,generation:f64,index:usize)->Option<&[f32]>{
        let binding=*self.skeletons.by_node.get(id)?;let skeleton=self.skeletons.bindings.get(binding)?;
        if skeleton.generation!=generation||index>=skeleton.bones.len(){return None;}
        let at=skeleton.start+index*16;self.skeletons.values.get(at..at+16)
    }
}
