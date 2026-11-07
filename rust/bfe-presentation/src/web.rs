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
    pub fn add_turret(&mut self, data: &str) -> Result<usize, JsValue> {
        let input = serde_json::from_str(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(self.inner.add_turret(input))
    }
    pub fn turret_input_ptr(&self) -> *const f64 { self.inner.turret_input.as_ptr() }
    pub fn nodes_ptr(&self) -> *const f64 { self.inner.nodes.as_ptr() }
    pub fn nodes_len(&self) -> usize { self.inner.nodes.len() }
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

/// Thin browser binding for the item phase: fixed blocks and four calls. The
/// page answers the rounds' ray and water requests between `present_items`
/// and `resume_items`, as many rounds as it allows a frame.
#[wasm_bindgen]
pub struct PageItems {
    inner: crate::items::Items,
}
#[wasm_bindgen]
impl PageItems {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PageItems {
        Self {
            inner: crate::items::Items::new(),
        }
    }
    pub fn frame_ptr(&self) -> *const f64 {
        self.inner.frame.as_ptr()
    }
    pub fn frame_len(&self) -> usize {
        self.inner.frame.len()
    }
    pub fn records_ptr(&self) -> *const f64 {
        self.inner.records.as_ptr()
    }
    pub fn records_len(&self) -> usize {
        self.inner.records.len()
    }
    pub fn definitions_ptr(&self) -> *const f64 {
        self.inner.definitions.as_ptr()
    }
    pub fn definitions_len(&self) -> usize {
        self.inner.definitions.len()
    }
    pub fn requests_ptr(&self) -> *const f64 {
        self.inner.requests.as_ptr()
    }
    pub fn requests_len(&self) -> usize {
        self.inner.requests.len()
    }
    pub fn answers_ptr(&self) -> *const f64 {
        self.inner.answers.as_ptr()
    }
    pub fn answers_len(&self) -> usize {
        self.inner.answers.len()
    }
    pub fn views_ptr(&self) -> *const f64 {
        self.inner.views.as_ptr()
    }
    pub fn views_len(&self) -> usize {
        self.inner.views.len()
    }
    pub fn releases_ptr(&self) -> *const f64 {
        self.inner.releases.as_ptr()
    }
    pub fn releases_len(&self) -> usize {
        self.inner.releases.len()
    }
    pub fn ripples_ptr(&self) -> *const f64 {
        self.inner.ripples.as_ptr()
    }
    pub fn ripples_len(&self) -> usize {
        self.inner.ripples.len()
    }
    pub fn header_ptr(&self) -> *const f64 {
        self.inner.header.as_ptr()
    }
    pub fn header_len(&self) -> usize {
        self.inner.header.len()
    }
    pub fn define_item(&mut self, index: usize) {
        self.inner.define(index);
    }
    pub fn apply_item_records(&mut self, count: usize) {
        self.inner.apply_records(count);
    }
    pub fn reset_items(&mut self, epoch: f64) {
        self.inner.reset_epoch(epoch);
    }
    pub fn present_items(&mut self) {
        self.inner.present();
    }
    pub fn resume_items(&mut self, abandon: bool) {
        self.inner.resume(abandon);
    }
}

/// Thin browser binding for the ordnance phase: fixed buffers the page
/// fills and reads, one `present` per frame.
#[wasm_bindgen]
pub struct PageOrdnance {
    inner: crate::ordnance::Ordnance,
}
#[wasm_bindgen]
impl PageOrdnance {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PageOrdnance {
        Self {
            inner: crate::ordnance::Ordnance::new(),
        }
    }
    pub fn input_ptr(&self) -> *const f64 {
        self.inner.input.as_ptr()
    }
    pub fn input_len(&self) -> usize {
        self.inner.input.len()
    }
    pub fn net_ptr(&self) -> *const f64 {
        self.inner.net_input.as_ptr()
    }
    pub fn net_len(&self) -> usize {
        self.inner.net_input.len()
    }
    pub fn style_ptr(&self) -> *const f64 {
        self.inner.style_input.as_ptr()
    }
    pub fn header_ptr(&self) -> *const f64 {
        self.inner.header.as_ptr()
    }
    pub fn calls_ptr(&self) -> *const f64 {
        self.inner.calls().as_ptr()
    }
    pub fn calls_len(&self) -> usize {
        self.inner.calls().len()
    }
    pub fn counts_ptr(&self) -> *const u32 {
        self.inner.counts().as_ptr()
    }
    /// The six particle attributes (position, colour, shape, axis, axis2,
    /// frame) of batch handle regions, by attribute index.
    pub fn quad_ptr(&self, attribute: usize) -> *const f32 {
        self.inner.quad_arrays()[attribute.min(5)].as_ptr()
    }
    pub fn quad_len(&self, attribute: usize) -> usize {
        self.inner.quad_arrays()[attribute.min(5)].len()
    }
    /// The four blaster attributes (head, tail, colour, size).
    pub fn blaster_ptr(&self, attribute: usize) -> *const f32 {
        self.inner.blaster_arrays()[attribute.min(3)].as_ptr()
    }
    pub fn blaster_len(&self, attribute: usize) -> usize {
        self.inner.blaster_arrays()[attribute.min(3)].len()
    }
    pub fn set_style(&mut self, index: usize) {
        self.inner.set_style(index);
    }
    pub fn flush_net(&mut self, count: usize) {
        self.inner.net_flush(count);
    }
    pub fn present_ordnance(&mut self) {
        self.inner.present();
    }
}

// LeavingWarning has one fixed row and one frame call. The host borrows
// its outputs so reading alpha cannot advance the spring a second time.
#[wasm_bindgen]
pub struct PageWarning {
    inner: crate::leaving_warning::Warning,
}
#[wasm_bindgen]
impl PageWarning {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PageWarning {
        Self { inner: crate::leaving_warning::Warning::default() }
    }
    pub fn row_ptr(&self) -> *const f64 { self.inner.row.as_ptr() }
    pub fn frame(&mut self) { self.inner.frame(); }
}

// Commands are written into one bounded ring. A flash token names its
// serial, never its packed storage index, so reuse cannot revive a token.
#[wasm_bindgen]
pub struct PageLights { inner: crate::lights::Lights }
#[wasm_bindgen]
impl PageLights {
    #[wasm_bindgen(constructor)]
    pub fn new(count: usize) -> PageLights { Self { inner: crate::lights::Lights::new(count) } }
    pub fn input_ptr(&self) -> *const f64 { self.inner.input.as_ptr() }
    pub fn commands_ptr(&self) -> *const f64 { self.inner.commands.as_ptr() }
    pub fn output_ptr(&self) -> *const f64 { self.inner.output.as_ptr() }
    pub fn handles_ptr(&self) -> *const f64 { self.inner.handles.as_ptr() }
    pub fn header_ptr(&self) -> *const f64 { self.inner.header.as_ptr() }
    pub fn frame(&mut self) { self.inner.frame(); }
}

// The shell has no battle simulation instance. Its small page-thread
// controller shares the required presentation module; audio-control owns
// the policy and the AudioWorklet continues to own actual PCM playback.
#[wasm_bindgen]
pub struct PageShellMusic { inner: audio_control::shell_music::Music }
#[wasm_bindgen]
impl PageShellMusic {
    #[wasm_bindgen(constructor)]
    pub fn new(groups: &str) -> Result<PageShellMusic, JsValue> {
        let groups = serde_json::from_str(groups).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self { inner: audio_control::shell_music::Music::new(groups) })
    }
    pub fn row_ptr(&self) -> *const f64 { self.inner.row.as_ptr() }
    pub fn frame(&mut self) { self.inner.frame(); }
}

// Host geometry arrays stay ordinary retained resources. Only changed spans
// are copied from this owner's fixed banks after the coarse interface call.
#[wasm_bindgen]
pub struct PageInterfaceBatch { inner: crate::interface_batch::Batch }
#[wasm_bindgen]
impl PageInterfaceBatch {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PageInterfaceBatch { Self { inner: crate::interface_batch::Batch::default() } }
    pub fn frame_ptr(&self) -> *const f64 { self.inner.frame.as_ptr() }
    pub fn commands_ptr(&self) -> *const f64 { self.inner.commands.as_ptr() }
    pub fn position_ptr(&self) -> *const f32 { self.inner.position.as_ptr() }
    pub fn uv_ptr(&self) -> *const f32 { self.inner.uv.as_ptr() }
    pub fn mask_ptr(&self) -> *const f32 { self.inner.mask.as_ptr() }
    pub fn tint_ptr(&self) -> *const f32 { self.inner.tint.as_ptr() }
    pub fn dirty_ptr(&self) -> *const u8 { self.inner.dirty.as_ptr() }
    pub fn step(&mut self) { self.inner.step(); }
}
