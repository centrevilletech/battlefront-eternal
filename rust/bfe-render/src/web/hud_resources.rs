// HudPass formerly constructed its own index/program bindings from a Three
// batch. Native descriptors now own those resources and map/mask policies.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    pub fn hud_descriptor_init(&mut self)->Result<(),JsValue>{
        self.remember_hud_descriptor_init(); // recovery
 let key="post-hud/hud";let program=self.r.core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;
        let mut index=Vec::with_capacity(crate::hud::QUADS*6);for i in 0..crate::hud::QUADS as u32{let a=i*4;index.extend([a,a+1,a+2,a+2,a+1,a+3]);}self.r.hud.init(&mut self.r.core,&index);
        let spec=crate::programs::spec(key).map_err(|e|JsValue::from_str(&e))?;let resources=&mut self.r.hud.resources;resources.program=Some(program);resources.slots=[-1;3];resources.cameras.clear();
        for(i,(name,_))in spec.uniforms.iter().enumerate(){match *name{"map"=>resources.slots[0]=i as i32,"mask"=>resources.slots[1]=i as i32,"masked"=>resources.slots[2]=i as i32,"projectionMatrix"=>resources.cameras.push((i,0)),"modelViewMatrix"=>resources.cameras.push((i,1)),_=>return Err(JsValue::from_str("unknown HUD uniform"))}}
        Ok(())
    }
    // Batch.material's semantic map/mask pair is resolved once. A full bank
    // skips later groups deterministically rather than stopping an overlay.
    pub fn hud_appearance(&mut self,map:i32,mask:i32,masked:bool,map_sample:i32,mask_sample:i32)->Option<u32>{
        self.remember_hud_appearance(map, mask, masked, map_sample, mask_sample); // recovery
 for(slot,source)in [(map,map_sample),(mask,mask_sample)]{if slot>=0&&source>=0{let pair=(slot as u32,source as u32);if !self.r.interface.resources.samples.contains(&pair){self.r.interface.resources.samples.push(pair);}}} let resources=&mut self.r.hud.resources;if resources.materials.len()>=crate::interface_resource_layout::limit::APPEARANCES{resources.clipped=resources.clipped.saturating_add(1);return None;}let id=resources.materials.len()as u32;resources.materials.push((map,mask,masked));Some(id)}
    // One publication expands the camera and every material run. The existing
    // HUD model still owns all battle state, glyphs and minimap quad generation.
    pub fn hud_resources_frame(&mut self,resources:&crate::resources::GpuResources,mode:i32,full:bool)->Result<(),JsValue>{
        self.ui_resources_frame(resources)?;
        use crate::hud::*;use crate::interface_resource_layout::hud as R;
        if mode>=0{self.hud_model_build(mode as u32,full);}
        let h=&mut self.r.hud;let Some(program)=h.resources.program else{return Ok(());};let mut view=[0.;16];view.copy_from_slice(&h.input[IN_CAMERA+R::VIEW..IN_CAMERA+R::VIEW+16]);let mut projection=[0.;16];projection.copy_from_slice(&h.input[IN_CAMERA+R::PROJECTION..IN_CAMERA+R::PROJECTION+16]);
        h.input[IN_CAMERA..IN_CAMERA+2*crate::post::UNIFORM].fill(0.);for i in 0..2{let at=IN_CAMERA+i*crate::post::UNIFORM;h.input[at]=-1.;if let Some(&(slot,kind))=h.resources.cameras.get(i){h.input[at]=slot as f64;h.input[at+1]=16.;h.input[at+2..at+18].copy_from_slice(if kind==0{&projection}else{&view});}}
        h.input[IN_PROGRAM]=program as f64;for i in 0..3{h.input[IN_SLOTS+i]=h.resources.slots[i]as f64;}
        let n=(h.input[IN_GROUPS]as usize).min(MAX_GROUPS);h.input[IN_GROUPS]=n as f64;for i in 0..n{let at=IN_FIRST_GROUP+i*GROUP;let first=(h.input[at]as usize).min(QUADS*6);h.input[at]=first as f64;h.input[at+1]=(h.input[at+1]as usize).min(QUADS*6-first)as f64;if h.input[at+2]!=crate::interface_resource_layout::kind::HUD as f64{continue;}if let Some(&(map,mask,masked))=h.resources.materials.get(h.input[at+3]as usize){h.input[at+2]=map as f64;h.input[at+3]=mask as f64;h.input[at+4]=u8::from(masked)as f64;}else{h.input[at+1]=0.;h.resources.clipped=h.resources.clipped.saturating_add(1);}}
        Ok(())
    }
}
