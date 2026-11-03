// The water pass's bindings (water.rs): its surfaces and rings, registered
// once, and the values and ring vertices the page writes each frame.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn water_input_ptr(&mut self) -> u32 { self.r.water.input.as_mut_ptr() as u32 }
    pub fn water_input_len(&self) -> u32 { self.r.water.input.len() as u32 }
    pub fn water_rings_position_ptr(&mut self) -> u32 { self.r.water.rings_position.as_mut_ptr() as u32 }
    pub fn water_rings_position_len(&self) -> u32 { self.r.water.rings_position.len() as u32 }
    pub fn water_rings_uv_ptr(&mut self) -> u32 { self.r.water.rings_uv.as_mut_ptr() as u32 }
    pub fn water_rings_uv_len(&self) -> u32 { self.r.water.rings_uv.len() as u32 }
    pub fn water_rings_alpha_ptr(&mut self) -> u32 { self.r.water.rings_alpha.as_mut_ptr() as u32 }
    pub fn water_rings_alpha_len(&self) -> u32 { self.r.water.rings_alpha.len() as u32 }
    pub fn water_surface(&mut self, geometry: u32, blend: &[u32], depth_func: u32, flags: u32, side: u32) -> u32 {
        self.r.water.add_surface(&self.r.core, geometry, blend, depth_func, flags, side)
    }
    pub fn water_rings(&mut self, index: &[u32], blend: &[u32], depth_func: u32, flags: u32, side: u32) -> u32 {
        self.r.water.add_rings(&self.r.core, index, blend, depth_func, flags, side)
    }
    // Instanced surfaces (the shore's): an instance set (sizes: floats an
    // instance, each attribute's), undefined when there is no room; its
    // values the page writes (f32).
    pub fn water_instances_ptr(&mut self) -> u32 { self.r.water.instances.as_mut_ptr() as u32 }
    pub fn water_instances_len(&self) -> u32 { self.r.water.instances.len() as u32 }
    pub fn water_set(&mut self, max: u32, sizes: &[u32]) -> Option<u32> { self.r.water.add_set(&self.r.core, max, sizes) }
    // Attribute k of a set read at `location` by a surface, which then draws
    // as many instances as the set holds.
    pub fn water_instanced(&mut self, surface: u32, set: u32, k: u32, location: u32) { self.r.water.instanced(&self.r.core, surface, set, k, location) }
}
