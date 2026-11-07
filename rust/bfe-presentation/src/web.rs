// Browser binding owns only allocation at setup and addresses of fixed buffers.
// The native host uses Presentation directly, with identical frame records.
use super::*;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct PagePresentation {
    inner: Presentation,
}
#[wasm_bindgen]
impl PagePresentation {
    #[wasm_bindgen(constructor)]
    pub fn new(content: &str) -> Result<PagePresentation, JsValue> {
        let content =
            serde_json::from_str(content).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self {
            inner: Presentation::new(content, Limits::default()),
        })
    }
    /// Live effect ids the page may hold at once (id 0 is never issued).
    pub fn handles_len(&self) -> usize {
        self.inner.handles()
    }
    pub fn input_ptr(&self) -> *const f64 {
        self.inner.input.as_ptr()
    }
    pub fn input_len(&self) -> usize {
        self.inner.input.len()
    }
    pub fn header_ptr(&self) -> *const f64 {
        self.inner.header.as_ptr()
    }
    pub fn effects_ptr(&self) -> *const f64 {
        self.inner.effect_output.as_ptr()
    }
    pub fn effects_len(&self) -> usize {
        self.inner.effect_output.len()
    }
    pub fn draws_ptr(&self) -> *const f64 {
        self.inner.draws.as_ptr()
    }
    pub fn draws_len(&self) -> usize {
        self.inner.draws.len()
    }
    pub fn events_ptr(&self) -> *const f64 {
        self.inner.events.as_ptr()
    }
    pub fn events_len(&self) -> usize {
        self.inner.events.len()
    }
    pub fn present_frame(&mut self) {
        self.inner.frame();
    }
}

/// Thin browser binding for the unit phase. Registration owns allocations;
/// two record batches are split by the host camera phase, without re-entry.
#[wasm_bindgen]
pub struct PageUnits {
    inner: crate::units::Units,
}
#[wasm_bindgen]
impl PageUnits {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PageUnits {
        Self {
            inner: crate::units::Units::new(),
        }
    }
    pub fn preview() -> PageUnits {
        Self {
            inner: crate::units::Units::preview(),
        }
    }
    pub fn input_len(&self) -> usize {
        self.inner.input.len()
    }
    pub fn commands_len(&self) -> usize {
        self.inner.commands.len()
    }
    pub fn output_len(&self) -> usize {
        self.inner.output.len()
    }
    pub fn nodes_len(&self) -> usize {
        self.inner.nodes.len()
    }
    pub fn changed_len(&self) -> usize {
        self.inner.changed.len()
    }
    pub fn skin_len(&self) -> usize {
        self.inner.skin.len()
    }
    pub fn weapons_len(&self) -> usize {
        self.inner.weapon_input.len()
    }
    pub fn load_content(&mut self, data: &str) -> Result<usize, JsValue> {
        let input = serde_json::from_str(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(self.inner.load(input))
    }
    pub fn add_actor(
        &mut self,
        content: usize,
        inverses: &str,
        attachments: &str,
    ) -> Result<usize, JsValue> {
        let inverses: Vec<[f64; 16]> =
            serde_json::from_str(inverses).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let attachments: Vec<crate::units::Attachment> =
            serde_json::from_str(attachments).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let id = self.inner.actor(content, &inverses);
        self.inner.attach(id, &attachments);
        Ok(id)
    }
    pub fn add_view(&mut self, data: &str, attachments: &str) -> Result<usize, JsValue> {
        let input = serde_json::from_str(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let attachments: Vec<crate::units::Attachment> =
            serde_json::from_str(attachments).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let id = self.inner.view(input);
        self.inner.view_attach(id, &attachments);
        Ok(id)
    }
    pub fn input_ptr(&self) -> *const f64 {
        self.inner.input.as_ptr()
    }
    pub fn commands_ptr(&self) -> *const f64 {
        self.inner.commands.as_ptr()
    }
    pub fn output_ptr(&self) -> *const f64 {
        self.inner.output.as_ptr()
    }
    pub fn nodes_ptr(&self) -> *const f64 {
        self.inner.nodes.as_ptr()
    }
    pub fn places_ptr(&self) -> *const u32 {
        self.inner.places.as_ptr()
    }
    pub fn places_len(&self) -> usize {
        self.inner.places.len()
    }
    pub fn changed_ptr(&self) -> *const u32 {
        self.inner.changed.as_ptr()
    }
    pub fn skin_ptr(&self) -> *const f32 {
        self.inner.skin.as_ptr()
    }
    pub fn header_ptr(&self) -> *const f64 {
        self.inner.header.as_ptr()
    }
    pub fn weapons_ptr(&self) -> *const f64 {
        self.inner.weapon_input.as_ptr()
    }
    pub fn actor_node_start(&self, id: usize) -> usize {
        self.inner.actor_node_start(id)
    }
    pub fn actor_skin_start(&self, id: usize) -> usize {
        self.inner.actor_skin_start(id)
    }
    pub fn view_node_start(&self, id: usize) -> usize {
        self.inner.view_node_start(id)
    }
    pub fn present_units(&mut self) {
        self.inner.pose_frame();
    }
    pub fn present_first_person(&mut self) {
        self.inner.attachment_frame();
    }
    pub fn place_actors(&mut self, count: usize) {
        self.inner.place_actors(count);
    }
    pub fn refresh_actor(&mut self, id: usize) {
        self.inner.refresh_actor(id);
    }
}

/// The page's animated props and log traps: registration at load, then one
/// frame call over fixed records (src/presentation/props.js).
#[wasm_bindgen]
pub struct PageProps {
    inner: crate::props::Props,
}
#[wasm_bindgen]
impl PageProps {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PageProps {
        Self {
            inner: crate::props::Props::new(),
        }
    }
    pub fn load_clip(&mut self, frames: usize, fps: f64, root_motion: bool, tracks: Vec<i32>, data: Vec<f32>) -> u32 {
        self.inner.load_clip(frames, fps, root_motion, tracks, data)
    }
    pub fn add_prop(&mut self, data: &str) -> Result<usize, JsValue> {
        let input = serde_json::from_str(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(self.inner.add_prop(input))
    }
    pub fn add_trap(&mut self, data: &str) -> Result<usize, JsValue> {
        let input = serde_json::from_str(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(self.inner.add_trap(input))
    }
    pub fn bone_start(&self, trap: bool, index: usize) -> usize {
        self.inner.bone_start(trap, index)
    }
    pub fn feet_at(&self) -> usize {
        crate::props::Props::feet_at()
    }
    pub fn bodies_at(&self) -> usize {
        crate::props::Props::bodies_at()
    }
    pub fn input_ptr(&self) -> *const f64 {
        self.inner.input.as_ptr()
    }
    pub fn input_len(&self) -> usize {
        self.inner.input.len()
    }
    pub fn prop_input_ptr(&self) -> *const f64 {
        self.inner.prop_input.as_ptr()
    }
    pub fn prop_output_ptr(&self) -> *const f64 {
        self.inner.prop_output.as_ptr()
    }
    pub fn trap_input_ptr(&self) -> *const f64 {
        self.inner.trap_input.as_ptr()
    }
    pub fn trap_output_ptr(&self) -> *const f64 {
        self.inner.trap_output.as_ptr()
    }
    pub fn bones_ptr(&self) -> *const f64 {
        self.inner.bones.as_ptr()
    }
    pub fn bones_len(&self) -> usize {
        self.inner.bones.len()
    }
    pub fn events_ptr(&self) -> *const f64 {
        self.inner.events.as_ptr()
    }
    pub fn header_ptr(&self) -> *const f64 {
        self.inner.header.as_ptr()
    }
    pub fn present_props(&mut self) {
        self.inner.frame();
    }
}

/// The page's hulls: rigs registered once, then two calls a frame over
/// fixed records (src/presentation/vehicles.js).
#[wasm_bindgen]
pub struct PageVehicles {
    inner: crate::vehicles::Vehicles,
}
#[wasm_bindgen]
impl PageVehicles {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PageVehicles {
        Self {
            inner: crate::vehicles::Vehicles::new(),
        }
    }
    pub fn load_clip(&mut self, frames: usize, fps: f64, root_motion: bool, tracks: Vec<i32>, data: Vec<f32>) -> u32 {
        self.inner.load_clip(frames, fps, root_motion, tracks, data)
    }
    pub fn add_rig(&mut self, data: &str) -> Result<usize, JsValue> {
        let input = serde_json::from_str(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(self.inner.add_rig(input))
    }
    pub fn slot_input(&self, rig: usize, slot: usize) -> usize {
        self.inner.slot_input(rig, slot)
    }
    pub fn slot_output(&self, rig: usize, slot: usize) -> usize {
        self.inner.slot_output(rig, slot)
    }
    pub fn entries_capacity(&self) -> usize {
        self.inner.entries_capacity()
    }
    pub fn input_ptr(&self) -> *const f64 {
        self.inner.input.as_ptr()
    }
    pub fn input_len(&self) -> usize {
        self.inner.input.len()
    }
    pub fn stream_in_ptr(&self) -> *const f64 {
        self.inner.stream_in.as_ptr()
    }
    pub fn stream_in_len(&self) -> usize {
        self.inner.stream_in.len()
    }
    pub fn stream_out_ptr(&self) -> *const f64 {
        self.inner.stream_out.as_ptr()
    }
    pub fn stream_out_len(&self) -> usize {
        self.inner.stream_out.len()
    }
    pub fn header_ptr(&self) -> *const f64 {
        self.inner.header.as_ptr()
    }
    pub fn place_hulls(&mut self) {
        self.inner.place();
    }
    pub fn pose_hulls(&mut self) {
        self.inner.pose();
    }
}
