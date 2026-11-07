// The tree canopies pass's bindings (canopies.rs): its input, its copy of
// the leaves, and its mesh, registered once.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn canopies_input_ptr(&mut self) -> u32 { self.r.canopies.input.as_mut_ptr() as u32 }
    pub fn canopies_input_len(&self) -> u32 { self.r.canopies.input.len() as u32 }
    pub fn canopies_leaves_ptr(&mut self) -> u32 { self.r.canopies.leaves.as_mut_ptr() as u32 }
    pub fn canopies_leaves_len(&self) -> u32 { self.r.canopies.leaves.len() as u32 }
    // TreeCanopies.constructor's fixed card and instanced attribute descriptor
    // belong to Rust. The host sends only authored capacity; program policy stays native.
    pub fn canopies_mesh(&mut self, max: u32) -> Result<(), JsValue> {
        self.remember_canopies_mesh(max); // recovery
 if max as usize > crate::static_layout::CANOPIES_MAX_GROUPS * crate::static_layout::CANOPIES_LEAVES { return Err(JsValue::from_str("invalid canopy capacity or program")); }
        // The two settings variants share locations and geometry. Compile them
        // cold so switching shadows needs no frame allocation or host program.
        let mut programs=[0;2];
        for (i,key) in ["world/canopies-no-shadows","world/canopies"].iter().enumerate() {programs[i]=self.r.core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;}
        let spec=crate::programs::spec("world/canopies").map_err(|e|JsValue::from_str(&e))?;
        let core = &mut self.r.core;
        let geometry = core.add_geometry();
        let positions: [f32; 12] = [-0.5,-0.5,0., 0.5,-0.5,0., -0.5,0.5,0., 0.5,0.5,0.];
        let uvs: [f32; 8] = [0.,1., 1.,1., 0.,0., 1.,0.];
        for (name, data, size) in [("position", &positions[..], 3), ("uv", &uvs[..], 2)] {
            if let Some((_, location)) = spec.attributes.iter().find(|(n,_)| *n == name) {
                let buffer = core.geometry_buffer(geometry, crate::core::bytes(data));
                core.geometry_pointer(geometry, buffer, *location, size, glow::FLOAT, false, 0, 0);
            }
        }
        core.geometry_index(geometry, crate::core::bytes(&[0u16,1,2,2,1,3]), glow::UNSIGNED_SHORT);
        let mut locations = [0; 3];
        for (i, name) in ["leafCenter", "leafSize", "leafShade"].iter().enumerate() {
            locations[i] = spec.attributes.iter().find(|(n,_)| n == name).ok_or_else(|| JsValue::from_str("missing canopy attribute"))?.1;
        }
        self.r.canopies.set_mesh(core, geometry, &locations, &[3,2,3], max, programs);
        Ok(())
    }
}
