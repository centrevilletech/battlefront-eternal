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
    /// Body display keeps native references to the already-retained sources.
    pub fn bind_sources(&mut self,vehicles:&PageVehicles,props:&PageProps){
        self.inner.rider_sources.bind(&vehicles.inner.sources,&props.inner.sources);
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
    pub fn bind_network(&mut self,network:&PageNetwork,actor:usize,id:f64,generation:f64,kind:f64){if let Some(owner)=&network.inner{self.inner.network.bind(actor,id,generation,kind,owner);}}
    pub fn network_states_ptr(&self)->*const f64{self.inner.network.states_ptr()}
    pub fn network_states_len(&self)->usize{self.inner.network.states_len()}
    pub fn unbind_network(&mut self,actor:usize){self.inner.network.unbind(actor);}
    pub fn collision_input_ptr(&self)->*const f64{self.inner.collision.input.as_ptr()}
    pub fn collision_input_len(&self)->usize{self.inner.collision.input.len()}
    pub fn collision_output_ptr(&self)->*const f64{self.inner.collision.output.as_ptr()}
    pub fn collision_output_len(&self)->usize{self.inner.collision.output.len()}
    pub fn collision_shapes_ptr(&self)->*const f64{self.inner.collision.shapes.as_ptr()}
    pub fn collision_shapes_len(&self)->usize{self.inner.collision.shapes.len()}
    pub fn collision_actors_ptr(&self)->*const u32{self.inner.collision.actors.as_ptr()}
    pub fn collision_actors_len(&self)->usize{self.inner.collision.actors.len()}
    pub fn collide_units(&mut self,count:usize){self.inner.collision_frame(count);}
    pub fn prepare_aim(&mut self,count:usize){self.inner.aim_frame(count);}
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
    pub fn add_unit_rider(&mut self, units:&PageUnits, actor:usize)->i32 {
        units.inner.rider_source(actor).map_or(-1,|(source,rig)|self.inner.add_unit_rider(source,rig) as i32)
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
    pub fn bind_network(&mut self,network:&PageNetwork,rig:usize,id:f64,generation:f64,kind:f64){if let Some(owner)=&network.inner{self.inner.network.bind(rig,id,generation,kind,owner);}}
    pub fn network_states_ptr(&self)->*const f64{self.inner.network.states_ptr()}
    pub fn network_states_len(&self)->usize{self.inner.network.states_len()}
    pub fn unbind_network(&mut self,rig:usize){self.inner.network.unbind(rig);}
    /// The frame's CastRay block (VehicleHeader.RAYS records in use).
    pub fn rays_ptr(&self) -> *const f64 {
        self.inner.rays.as_ptr()
    }
    pub fn rays_len(&self) -> usize {
        self.inner.rays.len()
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

/// Packet bytes and outbound input share the display module's memory. The
/// expensive receive/ring reservation is lazy: an encode-only host needs none.
#[wasm_bindgen]
pub struct PageNetwork {
    mounts: Option<crate::network::mounts::Phase>,
    preview: Option<crate::network::preview::Phase>,
    inner: Option<crate::network::Shared>,
    bytes: Vec<u8>,
    words: Vec<f64>,
    numbers: Vec<f64>,
    encoded: Vec<u8>,
    inspection:Vec<f64>,
    outbound: Vec<Option<crate::network::outbound::Outbound>>,
    outbound_results: Vec<f64>,
    outbound_fires:Vec<f64>,
    outbound_actions:Vec<f64>,
    network_error: Option<&'static str>,
    bounds: [f64;6],
}
#[wasm_bindgen]
impl PageNetwork {
    #[wasm_bindgen(constructor)]
    pub fn new()->PageNetwork {Self{mounts:None,preview:None,inner:None,bytes:vec![0;crate::network_layout::net_limits::PAYLOAD],words:vec![0.;crate::network_layout::net_limits::EXCHANGE],numbers:vec![0.;crate::network_layout::net_outbound::STRIDE],encoded:vec![0;1100],inspection:Vec::new(),outbound:Vec::new(),outbound_results:vec![0.;crate::network_layout::net_limits::PREVIEW_COMMANDS*crate::network_layout::net_outbound::STRIDE],outbound_fires:vec![0.;crate::network_layout::net_limits::PREVIEW_LOOKS*crate::network_layout::net_outbound_fire::STRIDE],outbound_actions:vec![0.;crate::network_layout::net_limits::PREVIEW_LOOKS],network_error:None,bounds:[0.,1.,0.,1.,0.,1.]}}
    pub fn close(&mut self){if let Some(p)=&mut self.preview{p.reset();}if let Some(m)=&mut self.mounts{m.reset();}for row in &mut self.outbound{*row=None;}if let Some(s)=&self.inner{let mut s=s.borrow_mut();s.reset(0.);s.leases.reset();}}
    pub fn timeline_input_ptr(&self)->*const f64{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().timeline.input.as_ptr())}
    pub fn timeline_output_ptr(&self)->*const f64{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().timeline.output.as_ptr())}
    pub fn timeline_push(&mut self){if let Some(s)=&self.inner{s.borrow_mut().timeline.push();}}
    pub fn timeline_options(&mut self,maximum:usize,hz:f64,extrapolate:f64){if let Some(s)=&self.inner{let mut s=s.borrow_mut();s.maximum=maximum.clamp(1,bfe_pose::timeline_layout::timeline_limits::SNAPSHOTS);s.hz=hz;s.extrapolate=extrapolate;}}
    pub fn timeline_setup(&mut self){if self.inner.is_none(){self.inner=Some(std::rc::Rc::new(std::cell::RefCell::new(crate::network::Session::new())));}}
    pub fn receive_setup(&mut self){self.timeline_setup();self.preview_setup();if self.mounts.is_none(){self.mounts=Some(crate::network::mounts::Phase::new());}if let Some(s)=&self.inner{s.borrow_mut().reserve_receive();}}
    pub fn lookup(&mut self,kind:u32,id:f64)->i32{self.inner.as_ref().and_then(|s|s.borrow_mut().lookup(kind,id,&self.bounds)).map_or(-1,|id|id as i32)}
    pub fn capture(&self,tick:f64)->String{self.inner.as_ref().map_or_else(||"null".into(),|s|s.borrow().capture(tick).to_string())}
    pub fn set_bounds(&mut self,x:f64,dx:f64,z:f64,dz:f64,y:f64,dy:f64){self.bounds=[x,dx,z,dz,y,dy];}
    pub fn bytes_ptr(&self)->*const u8{self.bytes.as_ptr()}
    pub fn words_ptr(&self)->*const f64{self.words.as_ptr()}
    pub fn numbers_ptr(&self)->*const f64{self.numbers.as_ptr()}
    pub fn encoded_ptr(&self)->*const u8{self.encoded.as_ptr()}
    pub fn header_ptr(&self)->*const f64{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().header.as_ptr())}
    pub fn records_ptr(&self)->*const f64{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().leases.records.as_ptr())}
    pub fn seats_ptr(&self)->*const u32{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().leases.seats.as_ptr())}
    pub fn payloads_ptr(&self)->*const u8{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().leases.payloads.as_ptr())}
    pub fn publications_ptr(&self)->*const u32{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().leases.publications.as_ptr())}
    pub fn lease_changes_ptr(&self)->*const i32{self.inner.as_ref().map_or(std::ptr::null(),|s|s.borrow().leases.changes.as_ptr())}
    pub fn capture_record(&self,id:usize)->String{self.inner.as_ref().map_or_else(||"null".into(),|s|s.borrow().capture_record(id).to_string())}
    pub fn capture_publication(&self)->String{self.inner.as_ref().map_or_else(||"null".into(),|s|s.borrow().capture_publication().to_string())}
    // One native entry consumes retention requests, decodes the packet and
    // feeds mount histories directly. Primitive status keeps glue allocation
    // outside the ordinary receive path; error text is requested only on fault.
    pub fn receive(&mut self,length:usize)->bool{
        let result=(||->crate::network::Result<()>{
            let bytes=self.bytes.get(..length).ok_or(crate::network::EXCHANGE)?;
            let s=self.inner.as_ref().ok_or("page receive is not initialized")?;let mut session=s.borrow_mut();session.receive(bytes,&self.bounds)?;
            if let Some(mounts)=&mut self.mounts{use crate::network_layout::{net_header as h,net_record as r,net_kind as k,net_limits as l};
                if session.header[h::VALID]!=0.{if session.header[h::EPOCH_CHANGED]!=0.{mounts.reset();}
                    let count=(session.header[h::RECORDS]as usize).min(l::RECORDS);
                    for i in 0..count{let at=session.leases.publications[i]as usize*r::STRIDE;let row=&session.leases.records[at..at+r::STRIDE];if row[r::KIND]==k::MOUNT as f64{mounts.ingest(row);}}
                }
            }Ok(())
        })();if let Err(error)=result{self.network_error=Some(error);false}else{true}
    }
    pub fn reset(&mut self,epoch:f64){if let Some(s)=&self.inner{s.borrow_mut().reset(epoch);}if let Some(m)=&mut self.mounts{m.reset();}}
    pub fn remove(&mut self,id:f64){if let Some(s)=&self.inner{s.borrow_mut().timeline.remove(id);}if let Some(m)=&mut self.mounts{m.remove(id);}}
    pub fn present(&mut self,tick:f64,player:f64,vehicle:f64){if let Some(s)=&self.inner{s.borrow_mut().present(tick,player,vehicle);}if let Some(m)=&mut self.mounts{m.present(tick);}if let Some(p)=&mut self.preview{p.present();}}
    // A display without another world interval refreshes own view facts only.
    // Resampling the ring at the same tick would erase its retained dt and
    // change walker foot easing in the hull phase after a zero-tick frame.
    pub fn present_view(&mut self){if let Some(p)=&mut self.preview{p.present();}}
    // Standalone previews and diagnostics need no receive baselines. Live
    // receivers reserve this same phase during ordinary receive setup.
    pub fn preview_setup(&mut self){if self.preview.is_none(){self.preview=Some(crate::network::preview::Phase::new());}}
    pub fn preview_frame_ptr(&self)->*const f64{self.preview.as_ref().map_or(std::ptr::null(),|p|p.frame.as_ptr())}
    pub fn preview_commands_ptr(&self)->*const f64{self.preview.as_ref().map_or(std::ptr::null(),|p|p.commands.as_ptr())}
    pub fn preview_looks_ptr(&self)->*const f64{self.preview.as_ref().map_or(std::ptr::null(),|p|p.looks.as_ptr())}
    pub fn preview_output_ptr(&self)->*const f64{self.preview.as_ref().map_or(std::ptr::null(),|p|p.output.as_ptr())}
    pub fn smooth_input_ptr(&self)->*const f64{self.preview.as_ref().map_or(std::ptr::null(),|p|p.smooth_input.as_ptr())}
    pub fn smooth_commands_ptr(&self)->*const f64{self.preview.as_ref().map_or(std::ptr::null(),|p|p.smooth_commands.as_ptr())}
    pub fn smooth_output_ptr(&self)->*const f64{self.preview.as_ref().map_or(std::ptr::null(),|p|p.smooth_output.as_ptr())}
    pub fn mount_frame_ptr(&self)->*const f64{self.mounts.as_ref().map_or(std::ptr::null(),|m|m.frame.as_ptr())}
    pub fn mount_input_ptr(&self)->*const f64{self.mounts.as_ref().map_or(std::ptr::null(),|m|m.input.as_ptr())}
    pub fn mount_output_ptr(&self)->*const f64{self.mounts.as_ref().map_or(std::ptr::null(),|m|m.output.as_ptr())}
    /// Cold fixture inspection uses the same numeric readers. Its result
    /// stream is absent from live receive and display publication.
    pub fn inspection_ptr(&self)->*const f64{self.inspection.as_ptr()}
    pub fn inspect(&mut self,op:u32,length:usize)->Result<usize,JsValue>{if self.inspection.is_empty(){self.inspection=vec![0.;crate::network_layout::net_limits::EXCHANGE];}let words=self.words.get(..length).ok_or_else(||JsValue::from_str("invalid page networking exchange"))?;let mut input=crate::network::Words::new(words);let mut output=crate::network::Output{values:&mut self.inspection,at:0};crate::network::read::run(op,&mut input,&mut output).map_err(JsValue::from_str)?;Ok(output.at)}
    pub fn outbound_new(&mut self)->usize{if self.outbound.len()>=crate::network_layout::net_limits::OWNERS && !self.outbound.iter().any(Option::is_none){return 0;}if let Some(i)=self.outbound.iter().position(Option::is_none){self.outbound[i]=Some(Default::default());i+1}else{self.outbound.push(Some(Default::default()));self.outbound.len()}}
    pub fn outbound_drop(&mut self,id:usize){if let Some(s)=self.outbound.get_mut(id.wrapping_sub(1)){*s=None;}}
    // NetworkInputs batches every command in one exchange. Aim and look
    // results retain command order; callbacks borrow these rows afterwards.
    pub fn outbound_fires_ptr(&self)->*const f64{self.outbound_fires.as_ptr()}
    pub fn outbound_actions_ptr(&self)->*const f64{self.outbound_actions.as_ptr()}
    pub fn outbound_results_ptr(&self)->*const f64{self.outbound_results.as_ptr()}
    pub fn outbound_batch(&mut self,id:usize,length:usize,count:usize)->bool {
        use crate::network_layout::{net_outbound as o,net_outbound_batch as b,net_preview_frame as f};
        let result=(||->crate::network::Result<()>{
            if count>crate::network_layout::net_limits::PREVIEW_COMMANDS{return Err(crate::network::EXCHANGE);}
            // Every command span is bounded before it can index the shared
            // result rows. Setup reserves the largest supported frame batch.
            let mut input=crate::network::Words::new(self.words.get(..length).ok_or(crate::network::EXCHANGE)?);
            // A malformed later command cannot commit earlier look, trigger
            // or preview state. Preflight the whole batch using borrowed spans.
            let mut checked=crate::network::Words::new(input.values);
            for _ in 0..count {
                let prefix=checked.span(b::STRIDE)?;let size=prefix[b::WORD_COUNT];
                if !size.is_finite()||size<0.||size.fract()!=0.||size>crate::network::ROOM as f64{return Err(crate::network::EXCHANGE);}
                let words=checked.span(size as usize)?;
                crate::network::outbound::Outbound::validate_command(&mut crate::network::Words::new(words))?;
            }
            checked.finish()?;
            let state=self.outbound.get_mut(id.wrapping_sub(1)).and_then(Option::as_mut).ok_or(crate::network::EXCHANGE)?;
            let mut fires=0;let mut actions=0;
            for row in 0..count {
                let prefix=input.span(b::STRIDE)?;let size=prefix[b::WORD_COUNT];
                if !size.is_finite() || size<0. || size.fract()!=0. || size>crate::network::ROOM as f64{return Err(crate::network::EXCHANGE);}
                let words=input.span(size as usize)?;
                let values=&mut self.outbound_results[row*o::STRIDE..][..o::STRIDE];
                state.run(crate::network_layout::net_op::OUTBOUND_COMMAND,&mut crate::network::Words::new(words),&mut crate::network::Output{values:&mut *values,at:0})?;
                // The native trigger owner publishes callbacks in source
                // order. Full callback spans clip additional presentation
                // work without growing storage during a stalled frame.
                use crate::network_layout::{net_outbound_fire as fire,net_limits as limits};
                values[o::FIRE_START]=fires as f64;values[o::ACTION_START]=actions as f64;
                for event in state.fires(){if fires==limits::PREVIEW_LOOKS{break;}
                    let target=&mut self.outbound_fires[fires*fire::STRIDE..];target[fire::BUTTON]=event.button as f64;target[fire::PRESSED]=event.pressed as u8 as f64;target[fire::HELD]=event.held as u8 as f64;fires+=1;}
                for &action in state.actions(){if actions==limits::PREVIEW_LOOKS{break;}self.outbound_actions[actions]=action as f64;actions+=1;}
                values[o::FIRE_COUNT]=fires as f64-values[o::FIRE_START];values[o::ACTION_COUNT]=actions as f64-values[o::ACTION_START];
                if let Some(p)=&mut self.preview {p.frame[f::COMMAND_SCALE]=prefix[b::MOUSE_SCALE];p.frame[f::COMMAND_INVERT]=prefix[b::INVERT];p.command_aim(words,values);}
            }
            input.finish()
        })();
        if let Err(error)=result{self.network_error=Some(error);false}else{true}
    }
    // All external ray answers enter together; no individual fire callback
    // asks WASM to reconstruct or converge the same body's camera basis.
    pub fn outbound_aim(&mut self,count:usize){
        use crate::network_layout::{net_limits as l,net_outbound as o};
        for row in self.outbound_results[..count.min(l::PREVIEW_COMMANDS)*o::STRIDE].chunks_exact_mut(o::STRIDE){crate::network::preview::Phase::resolve_command(row);}
    }
    // Hot exports return primitive statuses. Error strings are materialized
    // only for malformed exchanges, avoiding bindgen result tuples per send.
    pub fn take_network_error(&mut self)->String{self.network_error.take().unwrap_or(crate::network::EXCHANGE).into()}
    // Fixture capture is explicitly cold. Live callbacks consume numeric
    // views and never request an owned JSON copy of outbound accounting.
    pub fn outbound_capture(&self,id:usize)->String{self.outbound.get(id.wrapping_sub(1)).and_then(Option::as_ref).map_or_else(||"null".into(),|s|s.capture())}
    pub fn outbound_run(&mut self,id:usize,op:u32,length:usize)->bool {
        let result=(||->crate::network::Result<()>{
            let words=self.words.get(..length).ok_or(crate::network::EXCHANGE)?;
            let mut input=crate::network::Words::new(words);let mut output=crate::network::Output{values:&mut self.numbers,at:0};
            self.outbound.get_mut(id.wrapping_sub(1)).and_then(Option::as_mut).ok_or(crate::network::EXCHANGE)?.run(op,&mut input,&mut output)
        })();
        if let Err(error)=result{self.network_error=Some(error);false}else{true}
    }
    pub fn encode(&mut self,op:u32,length:usize)->i32 {
        let result=self.words.get(..length).ok_or(crate::network::EXCHANGE).and_then(|words|crate::network::input::encode(op,&mut crate::network::Words::new(words),&mut self.encoded));
        match result{Ok(n)=>n as i32,Err(error)=>{self.network_error=Some(error);-1}}
    }
    // EntityInputSender's successful receipt owns cadence. Transport facts
    // select a publication, while Rust retains the delivered held revision.
    pub fn encode_outbound(&mut self,id:usize,length:usize,tick:f64,edges:usize,interval:usize)->i32 {
        let Some(state)=self.outbound.get(id.wrapping_sub(1)).and_then(Option::as_ref)else{self.network_error=Some(crate::network::EXCHANGE);return -1;};
        if !state.allow(tick,edges,interval){return 0;}
        let result=self.words.get(..length).ok_or(crate::network::EXCHANGE).and_then(|words|crate::network::input::encode(crate::network_layout::net_op::INPUT,&mut crate::network::Words::new(words),&mut self.encoded));
        match result{Ok(n)=>n as i32,Err("input packet exceeds bounds")=>-2,Err(error)=>{self.network_error=Some(error);-1}}
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

// Footfalls shares audio's detector but retains its visual lifetime. Resource
// registration and cold samples are separate from the one displayed phase.
#[wasm_bindgen]
pub struct PageFeet {inner:crate::feet::Feet}
#[wasm_bindgen]
impl PageFeet {
    #[wasm_bindgen(constructor)]
    pub fn new()->Self{Self{inner:Default::default()}}
    pub fn add(&mut self,content:&str)->Result<usize,JsValue>{Ok(self.inner.add(serde_json::from_str(content).map_err(|e|JsValue::from_str(&e.to_string()))?))}
    pub fn heights(content:&str)->Result<String,JsValue>{let content=serde_json::from_str(content).map_err(|e|JsValue::from_str(&e.to_string()))?;Ok(serde_json::to_string(&crate::feet::heights(&content)).unwrap())}
    pub fn input_ptr(&self)->*const f64{self.inner.input.as_ptr()}
    pub fn input_len(&self)->usize{self.inner.input.len()}
    pub fn output_ptr(&self)->*const f64{self.inner.output.as_ptr()}
    pub fn output_len(&self)->usize{self.inner.output.len()}
    pub fn frame(&mut self){self.inner.frame();}
}

#[wasm_bindgen]
extern "C" {#[wasm_bindgen(js_namespace=Math,js_name=random)] fn page_random()->f64;}
// One retained owner spans event collection, batched impact matching and
// final drawing. No shot or particle crosses a per-object native call.
#[wasm_bindgen]
pub struct PageSniper {inner:crate::sniper::Sniper}
#[wasm_bindgen]
impl PageSniper {
    #[wasm_bindgen(constructor)] pub fn new()->Self{Self{inner:Default::default()}}
    pub fn input_len(&self)->usize{self.inner.input.len()}
    pub fn input_ptr(&self)->*const f64{self.inner.input.as_ptr()}
    pub fn events_len(&self)->usize{self.inner.events.len()}
    pub fn events_ptr(&self)->*const f64{self.inner.events.as_ptr()}
    pub fn streaks_len(&self)->usize{self.inner.streaks.len()}
    pub fn streaks_ptr(&self)->*const f32{self.inner.streaks.as_ptr()}
    pub fn flares_len(&self)->usize{self.inner.flares.len()}
    pub fn flares_ptr(&self)->*const f32{self.inner.flares.as_ptr()}
    pub fn matches_len(&self)->usize{self.inner.matches.len()}
    pub fn matches_ptr(&self)->*const f64{self.inner.matches.as_ptr()}
    pub fn counts_len(&self)->usize{self.inner.counts.len()}
    pub fn counts_ptr(&self)->*const u32{self.inner.counts.as_ptr()}
    pub fn collect(&mut self){self.inner.collect(&mut ||page_random());}
    pub fn match_impacts(&mut self,count:usize){self.inner.match_impacts(count);}
    pub fn frame(&mut self){self.inner.frame(&mut ||page_random());}
}
