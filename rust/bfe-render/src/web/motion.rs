// The motion pass's bindings (motion.rs): the frame's values and records the
// page writes each frame at motion_input_ptr, the drawn rigs' bones now and
// last frame at motion_bones_ptr and motion_previous_ptr, and the meshes that
// draw their own motion (motion_extra).
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

pub(crate) mod cpu;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn motion_history_flyer_frame(&mut self)->u32{self.r.cameras.history.flyer()}
    pub fn motion_history_flyer_sample_frame(&mut self,id:u32,stepped:bool)->u32{self.r.cameras.history.flyer_frame(id,stepped)}
    // MotionPass.draw consumes the camera publication and temporal state in
    // their native banks, after jitter and before Upscaler.restore.
    pub fn motion_camera_frame(&mut self,role:u32,view:u32){
        use crate::cameras::layout::{camera as C,role as R};
        use crate::post_chain::layout::state as S;
        use crate::motion as M;
        let role=role as usize;let view=view as usize;
        if role>=R::COUNT||view>=R::COUNT{return;}
        let source=&self.r.cameras.output;let out=&mut self.r.motion.input;
        for (at,from) in [(M::IN_PROJECTION,role*C::LEN+C::PROJECTION),(M::IN_VIEW,role*C::LEN+C::INVERSE),
            (M::IN_VIEW_PROJECTION,view*C::LEN+C::PROJECTION),(M::IN_VIEW_VIEW,view*C::LEN+C::INVERSE)]{
            out[at..at+16].copy_from_slice(&source[from..from+16]);
        }
        out[M::IN_CURRENT..M::IN_CURRENT+16].copy_from_slice(&self.r.post_chain.state[S::CURRENT..S::CURRENT+16]);
        out[M::IN_PREVIOUS..M::IN_PREVIOUS+16].copy_from_slice(&self.r.post_chain.state[S::PREVIOUS..S::PREVIOUS+16]);
    }
    // History lives with retained camera state across graphics reconstruction.
    pub fn motion_history_sequence(&self)->u32{self.r.cameras.history.sequence()}
    pub fn motion_history_skin_frame(&mut self,count:u32)->u32 {self.r.cameras.history.skin(count as usize)}
    pub fn motion_history_skin_start(&self,id:u32)->u32 {self.r.cameras.history.skin_start(id) as u32}
    pub fn motion_history_model_frame(&mut self,skin:u32,bind:&[f64])->u32 {self.r.cameras.history.model(skin,bind)}
    pub fn motion_history_reskin_frame(&mut self,id:u32,skin:u32){self.r.cameras.history.reskin(id,skin);}
    pub fn motion_history_reset_frame(&mut self,id:u32){self.r.cameras.history.reset(id);}
    pub fn motion_history_release_frame(&mut self,id:u32){self.r.cameras.history.release(id);}
    pub fn motion_history_poses_ptr(&mut self)->u32 {self.r.cameras.history.poses.as_mut_ptr() as u32}
    pub fn motion_history_poses_len(&self)->u32 {self.r.cameras.history.poses.len() as u32}
    pub fn motion_history_selected_ptr(&mut self)->u32 {self.r.cameras.history.selected.as_mut_ptr() as u32}
    pub fn motion_history_selected_len(&self)->u32 {self.r.cameras.history.selected.len() as u32}
    pub fn motion_history_records_ptr(&mut self)->u32 {self.r.cameras.history.records.as_mut_ptr() as u32}
    pub fn motion_history_records_len(&self)->u32 {self.r.cameras.history.records.len() as u32}
    pub fn motion_history_current_ptr(&mut self)->u32 {self.r.cameras.history.current.as_mut_ptr() as u32}
    pub fn motion_history_current_len(&self)->u32 {self.r.cameras.history.current.len() as u32}
    pub fn motion_history_prepare_frame(&mut self,count:u32){self.r.cameras.history.prepare(count as usize);}
    pub fn motion_history_write_frame(&mut self,count:u32){
        self.r.cameras.history.write(count as usize,&mut self.r.motion.input,&mut self.r.motion.bones,&mut self.r.motion.previous);
    }
    pub fn motion_history_commit_frame(&mut self){self.r.cameras.history.commit();}
    pub fn motion_input_ptr(&mut self) -> u32 { self.r.motion.input.as_mut_ptr() as u32 }
    pub fn motion_input_len(&self) -> u32 { self.r.motion.input.len() as u32 }
    pub fn motion_bones_ptr(&mut self) -> u32 { self.r.motion.bones.as_mut_ptr() as u32 }
    pub fn motion_bones_len(&self) -> u32 { self.r.motion.bones.len() as u32 }
    pub fn motion_previous_ptr(&mut self) -> u32 { self.r.motion.previous.as_mut_ptr() as u32 }
    pub fn motion_previous_len(&self) -> u32 { self.r.motion.previous.len() as u32 }
    // A record's size and where the records start in the input (f64 slots).
    pub fn motion_record_stride(&self) -> u32 { crate::motion::REC_STRIDE as u32 }
    pub fn motion_records_at(&self) -> u32 { crate::motion::IN_RECORDS as u32 }
    // The meshes that draw their own motion (the sky's ships): one registered
    // a mesh (program, geometry, its instances' attributes: [location,
    // columns, rows] each, for up to max instances), and each frame's at
    // motion_extras_ptr (f64) with their instances' values at
    // motion_extra_values_ptr (f32).
    pub fn motion_extra(&mut self, program: u32, geometry: u32, max: u32, layout: &[u32]) -> u32 {
        self.remember_motion_extra(program, geometry, max, layout); // recovery
 self.r.motion.add_extra(&mut self.r.core, program, geometry, max, layout)
    }
    pub fn motion_extras_ptr(&mut self) -> u32 { self.r.motion.extras_input.as_mut_ptr() as u32 }
    pub fn motion_extras_len(&self) -> u32 { self.r.motion.extras_input.len() as u32 }
    pub fn motion_extra_values_ptr(&mut self) -> u32 { self.r.motion.extra_values.as_mut_ptr() as u32 }
    pub fn motion_extra_values_len(&self) -> u32 { self.r.motion.extra_values.len() as u32 }
}
