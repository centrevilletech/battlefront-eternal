use wasm_bindgen::prelude::*;

// CPU-only capture/fixture entry runs the production history producer without
// creating a GL context. Shader replays alone cannot validate temporal resets.
#[wasm_bindgen]
pub struct BfePoseHistory { pub(crate) inner:crate::pose_history::PoseHistory }
#[wasm_bindgen]
impl BfePoseHistory {
    #[wasm_bindgen(constructor)]
    pub fn new()->Self {Self{inner:Default::default()}}
    pub fn flyer_frame(&mut self)->u32{self.inner.flyer()}
    pub fn flyer_sample_frame(&mut self,id:u32,stepped:bool)->u32{self.inner.flyer_frame(id,stepped)}
    pub fn sequence(&self)->u32{self.inner.sequence()}
    pub fn skin_frame(&mut self,count:u32)->u32 {self.inner.skin(count as usize)}
    pub fn skin_start(&self,id:u32)->u32 {self.inner.skin_start(id) as u32}
    pub fn model_frame(&mut self,skin:u32,bind:&[f64])->u32 {self.inner.model(skin,bind)}
    pub fn reskin_frame(&mut self,id:u32,skin:u32){self.inner.reskin(id,skin);}
    pub fn reset_frame(&mut self,id:u32){self.inner.reset(id);}
    pub fn release_frame(&mut self,id:u32){self.inner.release(id);}
    pub fn poses_ptr(&mut self)->u32 {self.inner.poses.as_mut_ptr() as u32}
    pub fn poses_len(&self)->u32 {self.inner.poses.len() as u32}
    pub fn selected_ptr(&mut self)->u32 {self.inner.selected.as_mut_ptr() as u32}
    pub fn selected_len(&self)->u32 {self.inner.selected.len() as u32}
    pub fn records_ptr(&mut self)->u32 {self.inner.records.as_mut_ptr() as u32}
    pub fn records_len(&self)->u32 {self.inner.records.len() as u32}
    pub fn current_ptr(&mut self)->u32 {self.inner.current.as_mut_ptr() as u32}
    pub fn current_len(&self)->u32 {self.inner.current.len() as u32}
    pub fn prepare_frame(&mut self,count:u32){self.inner.prepare(count as usize);}
    pub fn write_frame(&mut self,count:u32,input:&mut[f64],bones:&mut[f32],previous:&mut[f32]){
        self.inner.write(count as usize,input,bones,previous);
    }
    pub fn commit_frame(&mut self){self.inner.commit();}
}

