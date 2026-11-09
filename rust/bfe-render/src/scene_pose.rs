//! One qualified dirty block joins the presentation and renderer memories.
//! Drawable worlds, skin bytes and model bind inverses retain native ownership.
use crate::math::Mat4;
use crate::scene_pose_layout::{pose_world as w,pose_skin as s,pose_bind as b,pose_header as h,pose_limits as limit};
const NONE:usize=usize::MAX;
const IDENTITY:Mat4=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
#[derive(Clone,Copy)]pub struct Pose{pub world:Mat4,pub valid:bool,pub inverse:Option<Mat4>}
impl Default for Pose{fn default()->Self{Self{world:IDENTITY,valid:false,inverse:None}}}
#[derive(Clone,Copy)]struct Model{generation:u32,node_start:usize,node_count:usize,skin_start:usize,skin_count:usize,bind:Mat4,skin_head:usize}
impl Default for Model{fn default()->Self{Self{generation:0,node_start:0,node_count:0,skin_start:0,skin_count:0,bind:IDENTITY,skin_head:NONE}}}
#[derive(Default)]struct Owner{generation:u32,sequence:u64,nodes:Vec<usize>,skins:Vec<usize>,models:Vec<Model>}
pub struct ScenePoses {
 pub stream:Vec<f64>,pub stream_info:[f64;crate::scene_pose_layout::pose_tree_header::STRIDE],bindings:Vec<SkinBinding>,
 pub input:Vec<f64>,owners:Vec<Owner>,nodes:Vec<Pose>,pub skins:Vec<f32>,
 pub changed:Vec<usize>,pub heads:Vec<usize>,pub rejected:u64,pub clipped:u64,
}
impl Default for ScenePoses{
 fn default()->Self{Self{stream:Vec::new(),stream_info:[0.;crate::scene_pose_layout::pose_tree_header::STRIDE],bindings:Vec::new(),input:vec![0.;h::STRIDE],owners:Vec::new(),nodes:Vec::new(),skins:Vec::new(),changed:Vec::new(),heads:Vec::new(),rejected:0,clipped:0}}
}
fn index(value:f64,max:usize)->Option<usize>{(value.is_finite()&&value>=0.&&value.fract()==0.&&value<max as f64).then_some(value as usize)}
impl ScenePoses {
 // PageUnits.attach and rig admission reserve by admitted content. Source
 // kernel indices and renderer arena indices have independent capacities.
 #[allow(clippy::too_many_arguments)]
 pub fn register(&mut self,owner:usize,owner_generation:u32,id:usize,generation:u32,node_start:usize,node_count:usize,skin_start:usize,skin_count:usize)->bool {
  if owner==0||owner>=limit::OWNERS||id>=limit::MODELS||owner_generation==0||generation!=owner_generation
   ||node_start>limit::SOURCE_NODES||node_count>limit::SOURCE_NODES.saturating_sub(node_start)
   ||skin_start>limit::BONES||skin_count>limit::BONES.saturating_sub(skin_start){self.rejected+=1;return false;}
  if self.owners.len()<=owner{self.owners.resize_with(owner+1,Owner::default);}
  let o=&mut self.owners[owner];if owner_generation<o.generation{self.rejected+=1;return false;}
  if owner_generation!=o.generation{o.nodes.clear();o.skins.clear();o.models.clear();o.sequence=0;o.generation=owner_generation;}
  if o.models.len()<=id{o.models.resize(id+1,Model::default());}
  let previous=o.models[id];if generation<previous.generation{self.rejected+=1;return false;}
  if o.nodes.len()<node_start+node_count{o.nodes.resize(node_start+node_count,NONE);}
  if o.skins.len()<skin_start+skin_count{o.skins.resize(skin_start+skin_count,NONE);}
  let missing=o.nodes[node_start..node_start+node_count].iter().filter(|&&n|n==NONE).count();
  let skin_missing=o.skins[skin_start..skin_start+skin_count].iter().filter(|&&n|n==NONE).count();
  if self.nodes.len()+missing>limit::NODES||self.skins.len()/16+skin_missing>limit::BONES{self.clipped+=1;return false;}
  for slot in &mut o.nodes[node_start..node_start+node_count]{if *slot==NONE{*slot=self.nodes.len();self.nodes.push(Pose::default());self.heads.push(NONE);}}
  for slot in &mut o.skins[skin_start..skin_start+skin_count]{if *slot==NONE{*slot=self.skins.len()/16;self.skins.resize(self.skins.len()+16,0.);}}
  if generation!=previous.generation{for &slot in &o.nodes[node_start..node_start+node_count]{self.nodes[slot].valid=false;}}
  o.models[id]=Model{generation,node_start,node_count,skin_start,skin_count,bind:previous.bind,skin_head:if previous.generation==generation{previous.skin_head}else{NONE}};
  self.changed.reserve(self.nodes.len().saturating_sub(self.changed.len()));
  let words=h::STRIDE+self.nodes.len()*w::STRIDE+self.skins.len()/16*s::STRIDE+limit::MODELS*b::STRIDE;
  if self.input.len()<words{self.input.resize(words,0.);}true
 }
 fn model(&self,owner:usize,id:usize,generation:u32)->Option<&Model>{
  let o=self.owners.get(owner)?;let m=o.models.get(id)?;
  (generation!=0&&o.generation==generation&&m.generation==generation).then_some(m)
 }
 pub fn is_root(&self,owner:usize,id:usize,generation:u32,node:usize)->bool{self.model(owner,id,generation).is_some_and(|m|m.node_start==node)}
 pub fn slot(&self,owner:usize,id:usize,generation:u32,node:usize)->Option<usize>{
  let m=self.model(owner,id,generation)?;
  if node<m.node_start||node-m.node_start>=m.node_count{return None;}
  let slot=*self.owners[owner].nodes.get(node)?;(slot!=NONE).then_some(slot)
 }
 fn skin_slot(&self,owner:usize,id:usize,generation:u32,skin:usize)->Option<usize>{
  let m=self.model(owner,id,generation)?;
  if skin<m.skin_start||skin-m.skin_start>=m.skin_count{return None;}
  let slot=*self.owners[owner].skins.get(skin)?;(slot!=NONE).then_some(slot)
 }
 // Validate a whole phase before mutation. A delayed generation, oversized
 // prefix or bad skin cannot partially replace a live model's world/history.
 pub fn apply(&mut self)->usize {
  self.changed.clear();
  let Some(owner)=index(self.input[h::OWNER],self.owners.len())else{self.rejected+=1;return 0;};
  let sequence=self.input[h::SEQUENCE];let generation=self.owners[owner].generation;
  if self.input[h::GENERATION]!=generation as f64||!sequence.is_finite()||sequence<1.||sequence.fract()!=0.||sequence>9_007_199_254_740_991.||sequence<=self.owners[owner].sequence as f64{self.rejected+=1;return 0;}
  let count=|v:f64,max:usize|index(v,max.saturating_add(1));
  let Some(worlds)=count(self.input[h::WORLDS],self.nodes.len())else{self.clipped+=1;return 0;};
  let Some(skins)=count(self.input[h::SKINS],self.skins.len()/16)else{self.clipped+=1;return 0;};
  let Some(binds)=count(self.input[h::BINDS],limit::MODELS)else{self.clipped+=1;return 0;};
  let skin_at=h::STRIDE+worlds*w::STRIDE;let bind_at=skin_at+skins*s::STRIDE;let end=bind_at+binds*b::STRIDE;
  if end>self.input.len()||self.input[h::WORDS]!=end as f64{self.clipped+=1;return 0;}
  for i in 0..worlds{
   let r=&self.input[h::STRIDE+i*w::STRIDE..h::STRIDE+(i+1)*w::STRIDE];
   let valid=index(r[w::MODEL],limit::MODELS).zip(index(r[w::INDEX],limit::SOURCE_NODES))
    .and_then(|(model,node)|self.slot(owner,model,generation,node)).is_some()&&r[w::GENERATION]==generation as f64;
   if !valid{self.rejected+=1;return 0;}
  }
  for i in 0..skins{
   let r=&self.input[skin_at+i*s::STRIDE..skin_at+(i+1)*s::STRIDE];
   let valid=index(r[s::MODEL],limit::MODELS).zip(index(r[s::INDEX],limit::BONES))
    .and_then(|(model,skin)|self.skin_slot(owner,model,generation,skin)).is_some()&&r[s::GENERATION]==generation as f64;
   if !valid{self.rejected+=1;return 0;}
  }
  for i in 0..binds{
   let r=&self.input[bind_at+i*b::STRIDE..bind_at+(i+1)*b::STRIDE];
   if index(r[b::MODEL],limit::MODELS).and_then(|id|self.model(owner,id,generation)).is_none()||r[b::GENERATION]!=generation as f64{self.rejected+=1;return 0;}
  }
  self.owners[owner].sequence=sequence as u64;
  for i in 0..worlds{
   let at=h::STRIDE+i*w::STRIDE;let model=self.input[at+w::MODEL] as usize;let node=self.input[at+w::INDEX] as usize;
   let slot=self.owners[owner].nodes[node];debug_assert!(self.model(owner,model,generation).is_some());
   self.nodes[slot].world.copy_from_slice(&self.input[at+w::WORLD..at+w::WORLD+16]);self.nodes[slot].valid=true;self.changed.push(slot);
  }
  for i in 0..skins{
   let at=skin_at+i*s::STRIDE;let skin=self.input[at+s::INDEX] as usize;let slot=self.owners[owner].skins[skin]*16;
   // Raw words preserve the existing f32 skin publication, including -0.
   for k in 0..8{let bits=self.input[at+s::BITS+k].to_bits();self.skins[slot+k*2]=f32::from_bits(bits as u32);self.skins[slot+k*2+1]=f32::from_bits((bits>>32)as u32);}
  }
  for i in 0..binds{let at=bind_at+i*b::STRIDE;let model=self.input[at+b::MODEL] as usize;self.owners[owner].models[model].bind.copy_from_slice(&self.input[at+b::INVERSE..at+b::INVERSE+16]);}
  worlds+skins+binds
 }
 pub fn node(&self,owner:usize,id:usize,generation:u32,index:usize)->Option<&Pose>{
  let slot=self.slot(owner,id,generation,index)?;let pose=self.nodes.get(slot)?;pose.valid.then_some(pose)
 }
 pub fn arena_world(&self,slot:usize)->Option<&Mat4>{let pose=self.nodes.get(slot)?;pose.valid.then_some(&pose.world)}
 pub fn bind(&self,owner:usize,id:usize,generation:u32)->Option<&Mat4>{Some(&self.model(owner,id,generation)?.bind)}
 pub fn skin(&self,owner:usize,id:usize,generation:u32,index:usize)->Option<&[f32]>{let slot=self.skin_slot(owner,id,generation,index)?;self.skins.get(slot*16..slot*16+16)}
}

// Hull/prop phases already publish contiguous native records. Import those
// records once, then resolve ready skins without a Three skeleton update.
use crate::scene_pose_layout::{pose_tree_header as th,pose_tree_kind as tk,pose_tree_node as tn,pose_vehicle_node as vn};
struct SkinBinding{owner:usize,model:usize,generation:u32,skin:usize,node:usize,bind_index:usize,bones:Vec<usize>,inverses:Vec<Mat4>,next:usize}
impl ScenePoses{
 pub fn reserve_stream(&mut self,words:usize){
  let limit=limit::SOURCE_NODES*tn::STRIDE+limit::BONES*16;
  let words=words.min(limit);if self.stream.len()<words{self.stream.resize(words,0.);}
 }
 pub fn skin_offset(&self,owner:usize,id:usize,generation:u32,skin:usize)->Option<usize>{self.skin_slot(owner,id,generation,skin).map(|at|at*16)}
 // Authored inverse matrices and bone identities are cold content. Each
 // skinned mesh also declares where its native bind inverse is published.
 #[allow(clippy::too_many_arguments)]
 pub fn register_skin(&mut self,owner:usize,id:usize,generation:u32,skin:usize,node:usize,bind_index:usize,bones:&[u32],inverses:&[f64])->bool{
  let Some(model)=self.model(owner,id,generation).copied()else{self.rejected+=1;return false;};
  if bind_index>=model.node_count||bones.len()>limit::BONES||inverses.len()!=bones.len()*16||self.slot(owner,id,generation,node).is_none()
   ||bones.iter().any(|&n|self.slot(owner,id,generation,n as usize).is_none())
   ||(0..bones.len()).any(|i|self.skin_slot(owner,id,generation,skin+i).is_none()){self.rejected+=1;return false;}
  let inverse=inverses.as_chunks::<16>().0.to_vec();
  // Re-admitting a pooled mesh replaces its binding rather than leaving an
  // older inverse later in the model chain to overwrite the new skin.
  let mut existing=model.skin_head;
  for _ in 0..self.bindings.len(){
   if existing==NONE{break;}let binding=&mut self.bindings[existing];
   if binding.node==node&&binding.generation==generation{
    binding.skin=skin;binding.bind_index=bind_index;binding.bones=bones.iter().map(|&n|n as usize).collect();binding.inverses=inverse;return true;
   }existing=binding.next;
  }
  let index=self.bindings.len();self.bindings.push(SkinBinding{owner,model:id,generation,skin,node,bind_index,bones:bones.iter().map(|&n|n as usize).collect(),inverses:inverse,next:model.skin_head});
  self.owners[owner].models[id].skin_head=index;true
 }
 // Every imported count, field span and identity is qualified before any
 // pose changes. Stale generations leave the last successful phase intact.
 pub fn import_stream(&mut self)->usize{
  self.changed.clear();let a=&self.stream_info;
  let Some(owner)=index(a[th::OWNER],self.owners.len())else{self.rejected+=1;return 0;};
  let Some(id)=index(a[th::MODEL],limit::MODELS)else{self.rejected+=1;return 0;};
  let generation=a[th::GENERATION];if generation.fract()!=0.||generation<1.||generation>u32::MAX as f64{self.rejected+=1;return 0;}
  let generation=generation as u32;let Some(model)=self.model(owner,id,generation).copied()else{self.rejected+=1;return 0;};
  let Some(kind)=index(a[th::KIND],tk::VEHICLE+1)else{self.rejected+=1;return 0;};
  let stride=match kind{tk::TREE=>tn::STRIDE,tk::VEHICLE=>vn::STRIDE,_=>{self.rejected+=1;return 0;}};
  if a[th::COUNT]!=model.node_count as f64||model.node_count>self.stream.len()/stride{self.clipped+=1;return 0;}
  let Some(words)=index(a[th::WORDS],self.stream.len().saturating_add(1))else{self.clipped+=1;return 0;};
  if model.node_count>words/stride{self.clipped+=1;return 0;}
  // Qualify every trailing vehicle inverse before replacing any world. A
  // short packet cannot borrow the previous model's tail in the shared bank.
  if kind==tk::VEHICLE{
   let mut binding=model.skin_head;
   for _ in 0..self.bindings.len(){
    if binding==NONE{break;}let b=&self.bindings[binding];
    let start=model.node_count.saturating_mul(stride).saturating_add(b.bind_index.saturating_mul(16));
    if start>words||16>words-start{self.clipped+=1;return 0;}binding=b.next;
   }
  }
  let world=if kind==tk::TREE{tn::WORLD}else{vn::WORLD};let mut count=0;
  for i in 0..model.node_count{
   let Some(slot)=self.slot(owner,id,generation,model.node_start+i)else{self.rejected+=1;return 0;};
   let at=i*stride;let source=at+world;let pose=&mut self.nodes[slot];
   if kind==tk::TREE&&self.stream[at+tn::CHANGED]==0.&&pose.valid{continue;}
   if !pose.valid||(0..16).any(|k|pose.world[k].to_bits()!=self.stream[source+k].to_bits()){
    pose.world.copy_from_slice(&self.stream[source..source+16]);pose.valid=true;self.changed.push(slot);count+=1;
   }
   if kind==tk::TREE{
    let mut inverse=IDENTITY;inverse.copy_from_slice(&self.stream[at+tn::BIND_INVERSE..at+tn::BIND_INVERSE+16]);pose.inverse=Some(inverse);
   }
  }
  let mut binding=model.skin_head;
  for _ in 0..self.bindings.len(){
   if binding==NONE{break;}
   let b=&self.bindings[binding];
   if b.owner!=owner||b.model!=id||b.generation!=generation{break;}
   if kind==tk::VEHICLE{
    let start=model.node_count*stride+b.bind_index*16;
    if start+16<=self.stream.len(){let slot=self.owners[owner].nodes[b.node];let mut inverse=IDENTITY;inverse.copy_from_slice(&self.stream[start..start+16]);self.nodes[slot].inverse=Some(inverse);}
    else{self.clipped+=1;}
   }
   for i in 0..b.bones.len(){
    let slot=self.owners[owner].nodes[b.bones[i]];let target=self.owners[owner].skins[b.skin+i]*16;
    let matrix=crate::math::mul(&self.nodes[slot].world,&b.inverses[i]);
    // Skeleton.update converted only after world * authored inverse in f64.
    for (k, &m_val) in matrix.iter().enumerate(){self.skins[target+k]=m_val as f32;}
   }
   binding=b.next;
  }
  count
 }
}
