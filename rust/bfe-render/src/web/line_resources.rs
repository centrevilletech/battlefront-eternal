// LinesPass publishes one bounded view packet; descriptors survive context
// restoration through the same semantic admission journal as static meshes.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    pub fn line_clipped(&self)->f64{self.line_resources.clipped as f64}
    pub fn line_input_ptr(&mut self)->u32{self.line_resources.input.as_mut_ptr() as u32}
    pub fn line_input_len(&self)->u32{self.line_resources.input.len() as u32}
    pub fn line_data_ptr(&mut self)->u32{self.line_resources.data.as_mut_ptr() as u32}
    pub fn line_data_len(&self)->u32{self.line_resources.data.len() as u32}
    // The utility program is separate. Colour, topology, capacity and raster
    // defaults are selected by the semantic cable/helper kind in Rust.
    pub fn line_source(&mut self,program:u32,kind:u32,capacity:u32)->Result<Option<u32>,JsValue>{
        self.remember_line_source(program, kind, capacity); // recovery
 self.line_resources.admit(&mut self.r.core,program,kind,capacity as usize).map_err(|e|JsValue::from_str(&e))
    }
    pub fn line_frame(&mut self,view:u32){self.line_resources.frame(&mut self.r.core,&mut self.r.frames,view as usize);}
}
