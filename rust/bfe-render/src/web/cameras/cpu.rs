//! CPU-only uniform fixtures use the same retained native transform producer.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct BfeUniformTransforms{inner:crate::math::UniformTransforms}
#[wasm_bindgen]
impl BfeUniformTransforms{
    #[wasm_bindgen(constructor)]
    pub fn new()->Self{Self{inner:Default::default()}}
    pub fn input_ptr(&self)->u32{self.inner.input.as_ptr() as u32}
    pub fn input_len(&self)->u32{self.inner.input.len() as u32}
    pub fn output_ptr(&self)->u32{self.inner.output.as_ptr() as u32}
    pub fn output_len(&self)->u32{self.inner.output.len() as u32}
    pub fn frame(&mut self,uv:bool){self.inner.frame(uv);}
    pub fn mirrored(&self)->bool{
        use crate::cameras::layout::uniform_input as I;
        crate::math::determinant_mirrors(&self.inner.input[I::WORLD..I::WORLD+16].try_into().unwrap_or([0.;16]))
    }
}

// CPU fixtures replay the production marker without a graphics context.
#[wasm_bindgen]
pub struct BfeDepthMarker{inner:crate::cameras::depth_marker::DepthMarker,screen:crate::math::Mat4}
#[wasm_bindgen]
impl BfeDepthMarker{
    #[wasm_bindgen(constructor)]
    pub fn new()->Self{Self{inner:Default::default(),screen:[0.;16]}}
    pub fn key_ptr(&self)->u32{self.inner.key.as_ptr() as u32}
    pub fn key_len(&self)->u32{self.inner.key.len() as u32}
    pub fn output_ptr(&self)->u32{self.inner.output.as_ptr() as u32}
    pub fn output_len(&self)->u32{self.inner.output.len() as u32}
    pub fn screen_ptr(&self)->u32{self.screen.as_ptr() as u32}
    pub fn screen_len(&self)->u32{self.screen.len() as u32}
    pub fn begin_camera(&mut self,cameras:&[f64],role:u32){
        use crate::cameras::layout::{camera as c,role as r};let role=role as usize;if role>=r::COUNT||cameras.len()<(role+1)*c::LEN{return;}
        let row=&cameras[role*c::LEN..(role+1)*c::LEN];let projection=row[c::PROJECTION..c::PROJECTION+16].try_into().unwrap_or([0.;16]);let view=row[c::INVERSE..c::INVERSE+16].try_into().unwrap_or([0.;16]);self.screen=crate::math::mul(&projection,&view);self.inner.begin(&self.screen);
    }
    pub fn request(&mut self)->bool{self.inner.request()}
    pub fn finish(&mut self){self.inner.finish();}
}
