//! CameraOwner.sky and BattleFrame.mirror retain sky placement in native state.
//! A mirror restores authored positions without solving them before the next view.
use super::{Cameras,layout::{limit as l,role as r}};
use crate::scene::SceneRegistry;
impl Cameras{
 pub fn sky_nodes(&mut self,nodes:&[u32],single:u32,generation:u32){
  let count=(nodes.len()/2).min(l::SKY);if nodes.len()%2!=0||nodes.len()/2>count{self.clipped=true;}
  self.sky_nodes.resize(count,[0;2]);for i in 0..count{self.sky_nodes[i]=[nodes[i*2],nodes[i*2+1]];}
  self.sky_single=(generation!=0).then_some([single,generation]);
 }
 pub fn scene_sky(&mut self,scene:&mut SceneRegistry,role:usize,follow:bool){
  if role>=r::COUNT{self.clipped=true;return;}self.sky(role,follow);
  for (i,&[id,generation]) in self.sky_nodes.iter().enumerate(){scene.transform_position(id as usize,generation as f64,[self.sky[i*3],self.sky[i*3+1],self.sky[i*3+2]]);}
  if (!follow||self.sky_nodes.is_empty())&&self.sky_single.is_some(){let [id,generation]=self.sky_single.unwrap();let p=self.cameras[role].p;scene.transform_position(id as usize,generation as f64,[p.x,p.y,p.z]);}
 }
 pub fn save_sky(&mut self,scene:&SceneRegistry){
  for (i,&[id,generation]) in self.sky_nodes.iter().enumerate(){self.sky_saved[i]=scene.transforms.position(id as usize,generation as f64).unwrap_or([0.;3]);}
  if let Some([id,generation])=self.sky_single{self.sky_saved[l::SKY]=scene.transforms.position(id as usize,generation as f64).unwrap_or([0.;3]);}
 }
 pub fn restore_sky(&mut self,scene:&mut SceneRegistry){
  for (i,&[id,generation]) in self.sky_nodes.iter().enumerate(){scene.transform_position(id as usize,generation as f64,self.sky_saved[i]);}
  if let Some([id,generation])=self.sky_single{scene.transform_position(id as usize,generation as f64,self.sky_saved[l::SKY]);}
 }
}
