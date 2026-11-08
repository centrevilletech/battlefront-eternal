//! PageUnits publishes the matrices and ready skin bytes that drawing consumes.
//! Local bone TRS remains in its native owner for named gameplay readbacks.
use crate::scene_pose_layout::{pose_header as h,pose_world as w,pose_skin as s,pose_bind as b};
use crate::layout::{unit_node as n,unit_output as o};
const NONE:usize=usize::MAX;
pub struct Publisher {
    pub data:Vec<f64>,bone:Vec<usize>,world:Vec<bool>,
    admitted:Vec<bool>,pending:Vec<u32>,work:Vec<u32>,stamps:Vec<u64>,
    models:Vec<usize>,touched:Vec<bool>,bind:Vec<[f64;16]>,
    owner:u32,generation:u32,sequence:u64,
}
impl Publisher {
    pub fn new(_nodes:usize)->Self {
        Self{admitted:Vec::new(),pending:Vec::new(),work:Vec::new(),stamps:Vec::new(),data:vec![0.;h::STRIDE],bone:Vec::new(),world:Vec::new(),models:Vec::new(),
            touched:Vec::new(),bind:Vec::new(),owner:0,generation:1,sequence:0}
    }
    pub fn owner(&mut self,id:u32,generation:u32){
        if self.owner!=id||self.generation!=generation{self.sequence=0;}
        self.owner=id;self.generation=generation;
    }
    // Cold registration declares only world matrices with a render consumer.
    // A bone's ready skin row is independent of its world-matrix admission.
    pub fn targets(&mut self,start:usize,targets:&[u8],models:usize) {
        let end=start.saturating_add(targets.len()).min(crate::scene_pose_layout::pose_limits::SOURCE_NODES);
        if self.bone.len()<end{self.bone.resize(end,NONE);self.world.resize(end,false);}
        if self.admitted.len()<end{self.admitted.resize(end,false);self.stamps.resize(end,0);}
        self.work.reserve(end.saturating_sub(self.work.len()));
        for i in start..end{if !self.admitted[i]{self.pending.push(i as u32);self.admitted[i]=true;}}
        let models=models.max(self.bind.len());
        for (i,&target) in targets.iter().take(end.saturating_sub(start)).enumerate(){self.world[start+i]=target!=0;}
        if self.bind.len()<models{self.bind.resize(models,[f64::NAN;16]);self.touched.resize(models,false);}
        self.models.reserve(models.saturating_sub(self.models.len()));
        // Reserve by admitted content, not the maximum unit kernel arena.
        // Prefix lengths may shrink in a phase; capacity never grows there.
        let words=h::STRIDE+self.admitted.len()*(w::STRIDE+s::STRIDE)+models*b::STRIDE;
        if self.data.len()<words{self.data.resize(words,0.);}
    }
    pub fn bone(&mut self,node:usize,skin:usize){
        if self.bone.len()<=node{self.bone.resize(node+1,NONE);self.world.resize(node+1,false);}
        self.bone[node]=skin;
    }
    pub fn publish(&mut self,nodes:&[f64],changed:&[u32],count:usize,skin:&[f32],output:&[f64]){
        if self.owner==0{return;}
        self.sequence=(self.sequence+1).min(9_007_199_254_740_991);
        self.models.clear();self.work.clear();let mut worlds=0;let mut skins=0;
        // Admission supplies a one-time retained prefix, including actors
        // placed before registry attachment. No global node scan follows it.
        let count=count.min(changed.len());
        for &id in changed[..count].iter().chain(self.pending.iter()){
            let i=id as usize;if !self.admitted.get(i).copied().unwrap_or(false)||self.stamps[i]==self.sequence{continue;}
            // An unplaced actor still owns its cold bind pose. Match the old
            // world-version receipt: only a published native world may draw.
            if nodes.get(i*n::STRIDE+n::WORLD_VERSION).copied().unwrap_or(0.)==0.{continue;}
            self.stamps[i]=self.sequence;self.work.push(id);
        }
        self.pending.clear();
        for &index in &self.work{
            let index=index as usize;let at=index.saturating_mul(n::STRIDE);
            let Some(node)=nodes.get(at..at.saturating_add(n::STRIDE))else{continue;};
            if self.world.get(index).copied().unwrap_or(false){worlds+=1;}
            let bone=self.bone.get(index).copied().unwrap_or(NONE);
            if bone!=NONE && bone<skin.len()/16{skins+=1;}
            let model=node[n::ACTOR] as usize;
            if model<self.touched.len()&&!self.touched[model]{self.touched[model]=true;self.models.push(model);}
        }
        let mut binds=0;
        for &model in &self.models {
            let at=model*o::STRIDE+o::BIND_INVERSE;
            if let Some(inverse)=output.get(at..at+16){
                if inverse.iter().zip(&self.bind[model]).any(|(a,b)|a.to_bits()!=b.to_bits()){binds+=1;}
            }
        }
        let words=h::STRIDE+worlds*w::STRIDE+skins*s::STRIDE+binds*b::STRIDE;
        if words>self.data.len(){self.data[h::CLIPPED]+=1.;worlds=0;skins=0;binds=0;}
        self.data[h::OWNER]=self.owner as f64;self.data[h::GENERATION]=self.generation as f64;
        self.data[h::SEQUENCE]=self.sequence as f64;self.data[h::WORLDS]=worlds as f64;
        self.data[h::SKINS]=skins as f64;self.data[h::BINDS]=binds as f64;
        self.data[h::WORDS]=(h::STRIDE+worlds*w::STRIDE+skins*s::STRIDE+binds*b::STRIDE) as f64;
        let skin_start=h::STRIDE+worlds*w::STRIDE;let bind_start=skin_start+skins*s::STRIDE;
        let mut world_at=h::STRIDE;let mut skin_at=skin_start;
        if worlds+skins>0 {for &index in &self.work{
            let index=index as usize;let at=index.saturating_mul(n::STRIDE);
            let Some(node)=nodes.get(at..at.saturating_add(n::STRIDE))else{continue;};
            let model=node[n::ACTOR];
            if self.world.get(index).copied().unwrap_or(false){
                let r=&mut self.data[world_at..world_at+w::STRIDE];world_at+=w::STRIDE;
                r[w::INDEX]=index as f64;r[w::MODEL]=model;r[w::GENERATION]=self.generation as f64;
                r[w::WORLD..w::WORLD+16].copy_from_slice(&node[n::WORLD..n::WORLD+16]);
            }
            let bone=self.bone.get(index).copied().unwrap_or(NONE);
            if bone!=NONE&&bone<skin.len()/16{
                let r=&mut self.data[skin_at..skin_at+s::STRIDE];skin_at+=s::STRIDE;
                r[s::INDEX]=bone as f64;r[s::MODEL]=model;r[s::GENERATION]=self.generation as f64;
                // Copy f32 payload bits, preserving signed zeros and NaN bits.
                for k in 0..8{r[s::BITS+k]=f64::from_bits(skin[bone*16+k*2].to_bits() as u64|((skin[bone*16+k*2+1].to_bits() as u64)<<32));}
            }
        }}
        let mut at=bind_start;
        for &model in &self.models {
            self.touched[model]=false;
            if binds==0{continue;}
            let from=model*o::STRIDE+o::BIND_INVERSE;
            let Some(inverse)=output.get(from..from+16)else{continue;};
            if !inverse.iter().zip(&self.bind[model]).any(|(a,b)|a.to_bits()!=b.to_bits()){continue;}
            let r=&mut self.data[at..at+b::STRIDE];at+=b::STRIDE;
            r[b::MODEL]=model as f64;r[b::GENERATION]=self.generation as f64;
            r[b::INVERSE..b::INVERSE+16].copy_from_slice(inverse);self.bind[model].copy_from_slice(inverse);
        }
    }
}
