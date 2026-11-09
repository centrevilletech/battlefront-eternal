//! CrowdShadows.fill/rebuild/pack resource ownership. Cold body/weapon sources
//! compile into one retained skin geometry; live visibility only changes the
//! existing index prefix, without Geometry/BufferAttribute shells or allocation.
use crate::actor_resources::{Geometry,Stream,Attribute,Facts};
use crate::actor_layout::{crowd as L,crowd_input as I};

pub struct Span {pub first:usize,pub count:usize}
pub struct Crowd {pub source:Option<u32>,pub spans:Vec<Span>,pub input:Vec<f64>,pub full:Vec<u32>,pub drawn:usize,pub vertices:usize,pub vertex_capacity:usize,pub index_capacity:usize,pub bones:usize,pub clipped:u64}
impl Default for Crowd{fn default()->Self{Self{source:None,spans:Vec::with_capacity(L::SPANS),input:vec![0.;I::LEN],full:vec![],drawn:0,vertices:0,vertex_capacity:0,index_capacity:0,bones:0,clipped:0}}}
impl Crowd {
    pub fn geometry()->Geometry{Geometry{streams:vec![Stream::Float(vec![]),Stream::Unsigned(vec![]),Stream::Float(vec![])],attributes:vec![Attribute{name:"position",stream:0,size:3,stride:0,offset:0},Attribute{name:"skinIndex",stream:1,size:4,stride:0,offset:0},Attribute{name:"skinWeight",stream:2,size:4,stride:0,offset:0}],index:vec![],facts:Facts{bounds:[0.;10],textures:vec![],groups:vec![],count:0,index_width:4,index_count:0}}}
    // Reuse the original 1.5 growth and zero padding, bounded above by the
    // content-sized ceiling. Growth is admission work, never frame selection.
    pub fn append(&mut self,target:&mut Geometry,parts:&[(&Geometry,usize,Option<[f64;16]>,usize)],bones:usize)->Option<(Vec<u32>,bool)>{
        // Imported bone/group identities are bounded before addition or the
        // span walk, including a cold admission arriving during a live frame.
        if parts.len()>L::SPANS||parts.iter().any(|(_,bone,_,group)|*bone>=L::BONES||*group>=L::SPANS){self.clipped=self.clipped.saturating_add(1);return None;}
        let added=parts.iter().try_fold(0usize,|n,(g,_,_,_)|n.checked_add(g.facts.count))?;let indices=parts.iter().try_fold(0usize,|n,(g,_,_,_)|n.checked_add(g.index.len()))?;
        let spans=parts.iter().map(|p|p.3).max().map_or(Some(1),|n|n.checked_add(1))?;
        if self.vertices.checked_add(added).is_none_or(|n|n>L::VERTICES)||self.full.len().checked_add(indices).is_none_or(|n|n>L::INDICES)||self.spans.len()+spans>L::SPANS||bones>L::BONES{self.clipped=self.clipped.saturating_add(1);return None;}
        let vertices=self.vertices+added;let total=self.full.len()+indices;
        let grow=vertices>self.vertex_capacity||total>self.index_capacity||bones>self.bones;
        if grow{self.vertex_capacity=self.vertex_capacity.max(((vertices as f64)*1.5).ceil() as usize).clamp(1024, L::VERTICES);self.index_capacity=self.index_capacity.max(((total as f64)*1.5).ceil() as usize).clamp(1024, L::INDICES);self.bones=self.bones.max(((bones as f64)*1.5).ceil() as usize).clamp(32, L::BONES);
            if let Stream::Float(v)=&mut target.streams[0]{v.resize(self.vertex_capacity*3,0.);}
            if let Stream::Unsigned(v)=&mut target.streams[1]{v.resize(self.vertex_capacity*4,0);}
            if let Stream::Float(v)=&mut target.streams[2]{v.resize(self.vertex_capacity*4,0.);}
            target.index.resize(self.index_capacity,0);target.index.fill(0);
        }
        let first_span=self.spans.len();let mut span_first=self.full.len();let mut group=0;
        // body positions/joints are already immutable native streams. Weapon
        // positions use the hand-local authored matrix in f64 before Float32
        // assignment, matching fill's Vector3.applyMatrix4 operation order.
        for &(source,bone,matrix,part_group) in parts{
            while part_group>group{self.spans.push(Span{first:span_first,count:self.full.len()-span_first});span_first=self.full.len();group+=1;}
            let base=self.vertices;let p=source.attributes.iter().find(|a|a.name=="position")?;let Stream::Float(data)=&source.streams[p.stream]else{return None};let stride=if p.stride==0{3}else{p.stride as usize/4};
            let joint=source.attributes.iter().find(|a|a.name=="skinIndex");
            for i in 0..source.facts.count{let at=i*stride+p.offset as usize/4;let mut point=[data[at] as f64,data[at+1] as f64,data[at+2] as f64];if let Some(m)=matrix{let [x,y,z]=point;let w=1./(m[3]*x+m[7]*y+m[11]*z+m[15]);point=[(m[0]*x+m[4]*y+m[8]*z+m[12])*w,(m[1]*x+m[5]*y+m[9]*z+m[13])*w,(m[2]*x+m[6]*y+m[10]*z+m[14])*w];}
                let index=base+i;if let Stream::Float(out)=&mut target.streams[0]{for k in 0..3{out[index*3+k]=point[k] as f32;}}
                let source_joint=if matrix.is_none(){joint.and_then(|a|match &source.streams[a.stream]{Stream::Unsigned(v)=>v.get(i*4).copied(),_=>None}).unwrap_or(0) as usize}else{0};
                if let Stream::Unsigned(out)=&mut target.streams[1]{out[index*4]=(bone+source_joint) as u16;}
                if let Stream::Float(out)=&mut target.streams[2]{out[index*4]=1.;}
            }
            self.full.extend(source.index.iter().map(|i|base as u32+*i));self.vertices+=source.facts.count;
        }
        self.spans.push(Span{first:span_first,count:self.full.len()-span_first});target.facts.count=self.vertex_capacity;target.facts.index_count=target.index.len();self.input[I::DIRTY]=1.;Some(((first_span..self.spans.len()).map(|i|i as u32).collect(),grow))
    }
    // Visibility facts are paired by their admitted span id. Prefix compaction
    // preserves body/weapon admission order and leaves the old tail untouched,
    // exactly as CrowdShadows.pack did after a weapon swap or range crossing.
    pub fn pack(&mut self,target:&mut Geometry)->bool{if self.input[I::DIRTY]==0.{return false;}self.input[I::DIRTY]=0.;let mut n=0;for(id,span)in self.spans.iter().enumerate(){if self.input[I::FLAGS+id]==0.{continue;}target.index[n..n+span.count].copy_from_slice(&self.full[span.first..span.first+span.count]);n+=span.count;}self.drawn=n;true}
}
