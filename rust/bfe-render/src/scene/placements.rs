//! PageUnits.placeShown resolves authored visibility before native placement.
//! Mirror/shadow requests preserve the same root boundary as the colour view.
use super::{SceneRegistry,NONE,row,flags,bits,index};
use crate::scene::layout::scene_pose_ref as p;
use crate::scene_pose_layout::pose_limits as limit;
impl SceneRegistry{
 pub fn place_frame(&mut self,root:usize,generation:f64,owner:usize,maximum:usize)->usize{
  if self.pose_ref(root,generation).is_none(){return 0;}
  let maximum=maximum.min(limit::MODELS);let mut count=0;
  self.transforms.place_stack.clear();self.transforms.place_stack.push((root,0.,0.));
  while let Some((id,_,_))=self.transforms.place_stack.pop(){
   let node=&self.nodes[id];let flags=bits(node.data[row::FLAGS]);
   if flags&flags::ALIVE==0||flags&flags::VISIBLE==0{continue;}
   let reference=&node.data[row::POSE..row::POSE+p::STRIDE];
   if reference[p::OWNER]==owner as f64{
    if let Some(model)=index(reference[p::ID],maximum){
     if self.poses.is_root(owner,model,reference[p::GENERATION] as u32,reference[p::NODE_START] as usize)&&count<self.place_output.len(){self.place_output[count]=model as u32;count+=1;}
    }
   }
   self.order.clear();let mut child=node.first;
   while child!=NONE{if self.nodes[child].data[row::PARENT_GENERATION]==node.data[row::GENERATION]{self.order.push(child);}child=self.nodes[child].next;}
   for &child in self.order.iter().rev(){self.transforms.place_stack.push((child,0.,0.));}
  }
  // Unit row ids follow admission order. Reparenting can change scene order
  // without changing the producer's original placeShown iteration order.
  self.place_output[..count].sort_unstable();count
 }
}
