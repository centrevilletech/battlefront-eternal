//! point-lights.js lightsOf/shown and Graphics.syncViewLights retain graph facts.
//! Discovery preserves the half-second cache, including detached cached lights.
use super::{SceneRegistry,NONE,row,flags,bits,index};
use crate::cameras::{Cameras,layout::{point_change as q,light as p,input as i,camera as c,lens as n,role as r,limit as l}};
#[derive(Clone,Copy,Default)]
struct Point{generation:f64,color:[f64;3],intensity:f64,range:f64}
struct Census{root:usize,generation:f64,at:Option<f64>,sources:Vec<(usize,f64)>}
#[derive(Clone,Copy)]
struct Proxy{slot:usize,generation:f64,shown:f64,layers:f64}
pub struct PointLights{
 pub changes:Vec<f64>,records:Vec<Point>,roots:Vec<Census>,by_root:Vec<usize>,
 stack:Vec<usize>,candidates:Vec<(usize,f64)>,proxies:Vec<Proxy>,proxy_root:usize,
}
impl PointLights{
 pub fn new(count:usize)->Self{Self{changes:vec![0.;count*q::LEN],records:vec![Point::default();count],roots:Vec::new(),by_root:vec![NONE;count],stack:Vec::with_capacity(count),candidates:Vec::with_capacity(l::RAW_LIGHTS),proxies:Vec::with_capacity(l::RAW_LIGHTS),proxy_root:NONE}}
 pub fn reserve(&mut self,count:usize){
  self.changes.resize(count*q::LEN,0.);self.records.resize(count,Point::default());self.by_root.resize(count,NONE);self.stack.reserve(count.saturating_sub(self.stack.len()));
  for root in &mut self.roots{root.sources.reserve(count.saturating_sub(root.sources.len()));}
 }
}
impl SceneRegistry{
 // Scene roots register before drawing, so a first view never grows a census.
 pub fn point_root(&mut self,id:usize,generation:f64){
  if id>=self.nodes.len()||generation<1.{self.rejected+=1;return;}
  let slot=self.points.by_root[id];if slot!=NONE{if self.points.roots[slot].generation!=generation{let root=&mut self.points.roots[slot];root.generation=generation;root.at=None;root.sources.clear();}return;}
  self.points.by_root[id]=self.points.roots.len();self.points.roots.push(Census{root:id,generation,at:None,sources:Vec::with_capacity(self.nodes.len())});
 }
 pub fn points_apply(&mut self,count:usize){
  for ordinal in 0..count.min(self.points.changes.len()/q::LEN){
   let a=&self.points.changes[ordinal*q::LEN..(ordinal+1)*q::LEN];
   let Some(id)=index(a[q::SLOT],self.nodes.len())else{self.rejected+=1;continue;};
   if a[q::GENERATION]<1.||a[q::GENERATION]!=self.nodes[id].data[row::GENERATION]{self.rejected+=1;continue;}
   self.points.records[id]=Point{generation:a[q::GENERATION],color:[a[q::COLOR],a[q::COLOR+1],a[q::COLOR+2]],intensity:a[q::INTENSITY],range:a[q::RANGE]};
  }
 }
 fn point_shown(&self,id:usize,root:usize)->bool{
  let mut at=id;
  for _ in 0..self.nodes.len(){if at==NONE||at==root{return true;}if bits(self.nodes[at].data[row::FLAGS])&flags::VISIBLE==0{return false;}at=self.nodes[at].parent;}
  false
 }
 // A scene's cached list is a stable depth-first traversal, independent of
 // current visibility. Hidden ancestors are tested later for each packed row.
 fn point_sources(&mut self,root:usize,generation:f64,now:f64)->Option<usize>{
  if self.pose_ref(root,generation).is_none(){return None;}
  let slot=*self.points.by_root.get(root)?;if slot==NONE{return None;}
  let source=&self.points.roots[slot];if source.root!=root||source.generation!=generation{return None;}
  if source.at.is_none_or(|at|now-at>500.){
   self.points.roots[slot].at=Some(now);self.points.roots[slot].sources.clear();self.points.stack.clear();self.points.stack.push(root);
   while let Some(id)=self.points.stack.pop(){
    let generation=self.nodes[id].data[row::GENERATION];
    if generation!=0.&&self.points.records[id].generation==generation{self.points.roots[slot].sources.push((id,generation));}
    self.order.clear();let mut child=self.nodes[id].first;
    while child!=NONE{self.order.push(child);child=self.nodes[child].next;}
    for &child in self.order.iter().rev(){self.points.stack.push(child);}
   }
  }Some(slot)
 }
 fn point_row(&self,id:usize,generation:f64,root:usize,out:&mut[f64])->bool{
  if self.pose_ref(id,generation).is_none(){return false;}
  let light=self.points.records[id];if light.generation!=generation{return false;}
  let Some(world)=self.world(id)else{return false;};
  out.fill(0.);out[p::POSITION..p::POSITION+3].copy_from_slice(&world[12..15]);out[p::COLOR..p::COLOR+3].copy_from_slice(&light.color);
  out[p::INTENSITY]=light.intensity;out[p::RANGE]=light.range;out[p::LAYERS]=self.nodes[id].data[row::LAYERS];out[p::SHOWN]=self.point_shown(id,root) as u8 as f64;true
 }
 // First-person sources are direct children, as syncViewLights selected them.
 // Only an identity/order change replaces their visibility/layer snapshots.
 fn point_proxies(&mut self,root:usize){
  self.points.candidates.clear();let mut id=self.nodes[root].first;
  while id!=NONE&&self.points.candidates.len()<l::RAW_LIGHTS{
   let generation=self.nodes[id].data[row::GENERATION];if generation!=0.&&self.points.records[id].generation==generation{self.points.candidates.push((id,generation));}id=self.nodes[id].next;
  }
  let changed=self.points.proxy_root!=root||self.points.proxies.len()!=self.points.candidates.len()||self.points.proxies.iter().zip(&self.points.candidates).any(|(p,&(id,generation))|p.slot!=id||p.generation!=generation);
  if changed{
   self.points.proxies.clear();self.points.proxy_root=root;
   for &(id,generation) in &self.points.candidates{let node=&self.nodes[id];self.points.proxies.push(Proxy{slot:id,generation,shown:(bits(node.data[row::FLAGS])&flags::VISIBLE!=0) as u8 as f64,layers:node.data[row::LAYERS]});}
  }
 }
 pub fn point_lights_frame(&mut self,cameras:&mut Cameras,root:usize,generation:f64,role:usize,source:usize,source_generation:f64,view:bool,now:f64,pack:usize){
  if role>=r::COUNT{return;}let Some(census)=self.point_sources(root,generation,now)else{return;};
  let mut count=0;
  for ordinal in 0..self.points.roots[census].sources.len(){
   if count==l::RAW_LIGHTS{break;}let (id,generation)=self.points.roots[census].sources[ordinal];
   if self.point_row(id,generation,root,&mut cameras.light_input[count*p::LEN..(count+1)*p::LEN]){count+=1;}
  }
  if view&&self.pose_ref(source,source_generation).is_some(){
   self.point_proxies(source);
   for proxy in &self.points.proxies{
    if count==l::RAW_LIGHTS{break;}let out=&mut cameras.light_input[count*p::LEN..(count+1)*p::LEN];
    if !self.point_row(proxy.slot,proxy.generation,source,out){continue;}
    // A proxy's visible bit only zeroes intensity; its admission snapshot
    // still controls layers and whether it participates in the view list.
    out[p::SHOWN]=(bits(self.nodes[proxy.slot].data[row::FLAGS])&flags::VISIBLE!=0) as u8 as f64;
    out[p::PROXY]=1.;out[p::INITIAL_SHOWN]=proxy.shown;out[p::INITIAL_LAYERS]=proxy.layers;count+=1;
   }
  }
  let camera=&cameras.output[role*c::LEN..(role+1)*c::LEN];
  cameras.input[i::LIGHT_COUNT]=count as f64;cameras.input[i::LIGHT_EYE..i::LIGHT_EYE+3].copy_from_slice(&camera[c::WORLD+12..c::WORLD+15]);
  cameras.input[i::LIGHT_FAR]=camera[c::LENS+n::FAR];
  // Layers are authored camera facts and remain in the semantic input bank.
  if let Some(world)=self.world(root){cameras.input[i::VIEW_WORLD..i::VIEW_WORLD+16].copy_from_slice(&world);}
  cameras.lights(pack);
 }
}
