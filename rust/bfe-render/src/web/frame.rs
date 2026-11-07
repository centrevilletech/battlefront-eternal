// Loaded battles hand one sealed packet bank to the native schedule. These
// scalar/pointer exports allocate no success result or JavaScript command list.
use super::BfeRenderer;
use crate::frame::{FrameTargets};
use crate::frame_plan::layout::binding as B;
use crate::resources::GpuResources;
use wasm_bindgen::prelude::*;
// Convert a resource fault only on failure; successful frame submission
// returns a scalar and leaves diagnostics in retained numeric rows.
fn failure(error:JsValue)->String {error.as_string().unwrap_or_else(||"Frame resource operation failed".into())}
impl FrameTargets for GpuResources {
    // Graphics.setRenderTarget supplied these coordinates. The existing owner
    // validates target generations and restores its own attachment storage.
    fn bind(&mut self,b:&[f64])->Result<(),String> {
        self.bind(b[B::TARGET] as i32,(b[B::FACE] as u32).min(5),b[B::LEVEL].max(0.).min(32.) as i32,
            b[B::X] as i32,b[B::Y] as i32,b[B::WIDTH].max(0.).min(32768.) as i32,b[B::HEIGHT].max(0.).min(32768.) as i32,
            b[B::SCISSOR] as i32,b[B::SCISSOR+1] as i32,b[B::SCISSOR+2].max(0.).min(32768.) as i32,b[B::SCISSOR+3].max(0.).min(32768.) as i32,b[B::CLIPPED]!=0.).map_err(failure)
    }
    fn resolve(&mut self,id:i32)->Result<(),String> {if id<0{Ok(())}else{self.resolve(id as u32).map_err(failure)}}
    fn copy(&mut self,from:i32,to:i32,mask:u32)->Result<(),String> {
        if from<0 || to<0 {return Ok(());}
        self.copy(from as u32,to as u32,mask).map_err(failure)
    }
    fn mipmap(&mut self,sample:i32)->Result<(),String> {if sample<0{Ok(())}else{self.mipmap(sample as u32).map_err(failure)}}
}
#[wasm_bindgen]
impl BfeRenderer {
    // Stable generated blocks separate camera/material facts from policy state.
    // The browser refreshes their views after any cold admission grows memory.
    pub fn frame_facts_ptr(&mut self)->u32 {self.r.frames.plan.facts.as_mut_ptr() as u32}
    pub fn frame_facts_len(&self)->u32 {self.r.frames.plan.facts.len() as u32}
    pub fn frame_state_ptr(&mut self)->u32 {self.r.frames.plan.state.as_mut_ptr() as u32}
    pub fn frame_state_len(&self)->u32 {self.r.frames.plan.state.len() as u32}
    // A diagnostic trace borrows the planner's fixed operation store. Reading
    // it never constructs a browser command stream or changes execution.
    pub fn frame_trace_ptr(&mut self)->u32 {self.r.frames.plan.trace.as_mut_ptr() as u32}
    pub fn frame_trace_len(&self)->u32 {self.r.frames.plan.trace.len() as u32}
    pub fn frame_bindings_ptr(&mut self)->u32 {self.r.frames.bindings.as_mut_ptr() as u32}
    pub fn frame_bindings_len(&self)->u32 {self.r.frames.bindings.len() as u32}
    pub fn frame_meta_ptr(&mut self)->u32 {self.r.frames.meta.as_mut_ptr() as u32}
    pub fn frame_meta_len(&self)->u32 {self.r.frames.meta.len() as u32}
    pub fn frame_lines_ptr(&mut self)->u32 {self.r.frames.lines.as_mut_ptr() as u32}
    pub fn frame_lines_len(&self)->u32 {self.r.frames.lines.len() as u32}
    pub fn frame_lines_publish_frame(&mut self,view:u32,count:u32) {
        if let Some(row)=self.r.frames.line_counts.get_mut(view as usize){*row=(count as usize).min(crate::frame_plan::layout::limit::LINES);}
    }
    pub fn frame_stats_ptr(&mut self)->u32 {self.r.frames.stats.as_mut_ptr() as u32}
    pub fn frame_stats_len(&self)->u32 {self.r.frames.stats.len() as u32}
    // Reserve before publishing live views. Later publications copy used
    // prefixes only, so multiple cameras cannot overwrite pending uniforms.
    pub fn frame_reserve_frame(&mut self) {self.r.frame_reserve();}
    pub fn frame_shadow_reserve_frame(&mut self) {self.r.frame_shadow_reserve();}
    pub fn frame_ui_slot_frame(&mut self,role:u32) {self.r.frames.ui_role=role as usize;}
    pub fn frame_overflow_frame(&mut self) {self.r.frames.mark_overflow();}
    pub fn frame_begin_frame(&mut self) {self.r.frame_begin();}
    // Selection reports which semantic inputs are needed before preparation.
    // It neither invokes a draw nor advances the successful-frame cadence.
    pub fn frame_select_frame(&mut self) {self.r.frames.plan.select();}
    pub fn frame_plan_frame(&mut self) {self.r.frames.plan.select();self.r.frames.plan.plan();}
    pub fn frame_publish_frame(&mut self,role:u32)->bool {self.r.frame_publish(role as usize)}
    pub fn frame_admit_interface_frame(&mut self,role:u32) {self.r.frame_admit_interface(role as usize);}
    pub fn battle_frame(&mut self,resources:&mut GpuResources)->bool {
        match self.r.battle_frame(resources) {Ok(())=>true,Err(error)=>{self.r.core.fault=Some(error);false}}
    }
    // Standalone menu/interface clients submit a single packet. Loaded battles
    // always use battle_frame, including their previews and overlays.
    pub fn frame_aux_frame(&mut self,resources:&mut GpuResources,role:u32)->bool {
        match self.r.frame_aux(resources,role as usize){Ok(())=>true,Err(error)=>{self.r.core.fault=Some(error);false}}
    }
    pub fn frame_error_frame(&mut self)->Option<String> {self.r.core.fault.take()}
    pub fn frame_cancel_frame(&mut self) {self.r.frames.collecting=false;self.r.post_chain.invalidate();}
}
