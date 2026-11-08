mod diagnostics;
mod line_resources;
// The renderer in the page: a WebGL2 context shared with three.js, three's
// textures used where they are, inputs the page writes into WASM memory.
// Here the core's bindings and each phase's; each pass's own are in a file
// of their own here, a `#[wasm_bindgen] impl BfeRenderer` block each.
mod recovery;
mod scene;
pub use scene::BfeScene;
mod canopies;
mod cameras;
mod actor_resources;
mod actor_flyers;
mod effects;
mod effect_resources;
mod far;
mod foliage;
mod hud;
mod images;
mod interface;
mod instances;
mod motion;
mod post;
mod frame;
mod shadow;
mod terrain;
mod transparent;
mod view;
mod water;
mod world;
mod static_world;
mod static_depth;

use crate::core::MultiDraw;
use crate::renderer::Renderer;
use crate::shader::Kind;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

pub struct WebMultiDraw(Option<web_sys::WebglMultiDraw>, web_sys::WebGl2RenderingContext,
    Option<(Option<web_sys::WebGlFramebuffer>, Option<web_sys::WebGlFramebuffer>)>);

impl MultiDraw for WebMultiDraw {
    fn context_lost(&self) -> bool { self.1.is_context_lost() }
    fn multi_draw(&self, mode: u32, counts: &mut [i32], element_type: u32, offsets: &mut [i32]) {
        match &self.0 {
            Some(ext) => ext.multi_draw_elements_webgl_with_i32_array_and_i32_array(mode, counts, 0, element_type, offsets, 0, counts.len() as i32),
            None => {
                for (c, o) in counts.iter().zip(offsets.iter()) {
                    self.1.draw_elements_with_i32(mode, *c, element_type, *o);
                }
            }
        }
    }
    fn save_framebuffers(&mut self) -> bool {
        let get = |p| self.1.get_parameter(p).ok().and_then(|v| v.dyn_into::<web_sys::WebGlFramebuffer>().ok());
        self.2 = Some((get(glow::DRAW_FRAMEBUFFER_BINDING), get(glow::READ_FRAMEBUFFER_BINDING)));
        true
    }
    fn read_saved_draw_framebuffer(&self) {
        if let Some((draw, _)) = &self.2 { self.1.bind_framebuffer(glow::READ_FRAMEBUFFER, draw.as_ref()); }
    }
    fn restore_framebuffers(&self) {
        if let Some((draw, read)) = &self.2 {
            self.1.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw.as_ref());
            self.1.bind_framebuffer(glow::READ_FRAMEBUFFER, read.as_ref());
        }
    }
}

#[wasm_bindgen]
pub struct BfeRenderer {
    r: Renderer<glow::Context, WebMultiDraw>,
    recovery: recovery::Recovery,
    hud_model: crate::hud_model::HudModel,
    rebuilding: bool,
    static_worlds: Vec<static_world::StaticSlot>,
    static_retry: Option<crate::prepare::PendingWorld>,
    static_sources: Vec<static_world::StaticSource>,
    line_resources:crate::line_resources::Lines,
    actor_resources:crate::actor_resources::Bank,
    water_sources:Vec<crate::water_resources::WaterSource>,
    water_geometry:std::collections::HashMap<(u32,u32),u32>,
}

#[wasm_bindgen]
impl BfeRenderer {
    #[wasm_bindgen(constructor)]
    pub fn new(context: web_sys::WebGl2RenderingContext) -> BfeRenderer {
        let ext = context.get_extension("WEBGL_multi_draw").ok().flatten().map(|e| e.unchecked_into::<web_sys::WebglMultiDraw>());
        let multi = WebMultiDraw(ext, context.clone(), None);
        BfeRenderer { r: Renderer::new(glow::Context::from_webgl2_context(context), multi), recovery: recovery::Recovery::default(), hud_model: crate::hud_model::HudModel::new(), rebuilding: false, static_worlds: vec![], static_retry: None, static_sources: vec![],line_resources:crate::line_resources::Lines::default(),actor_resources:crate::actor_resources::Bank::default(),water_sources:vec![],water_geometry:std::collections::HashMap::new() }
    }

    // Where the page writes the frame's values and records (f64), and the
    // drawn soldiers' bone matrices (f32), as byte offsets into memory.
    // Pacing is browser I/O; the native owner chooses every preload variant.
    pub fn prepare_begin_frame(&mut self,family:u32,cascades:u32,lamps:bool,shadows:bool) {self.r.prepare.begin(family,cascades,lamps,shadows);}
    pub fn prepare_next_frame(&mut self)->Result<String,JsValue> {
        while let Some(key)=self.r.prepare.next_key(){
            if self.r.core.has_owned_program(key){continue;}
            self.add_owned_program(key)?;return Ok(key.to_string());
        }
        Ok(String::new())
    }
    pub fn prepare_keys_frame(&self,family:u32,cascades:u32,lamps:bool,shadows:bool)->String {crate::prepare::Prepare::keys(family,cascades,lamps,shadows)}
    pub fn scene_stamp_set_frame(&mut self,stamp:u32) {self.r.core.scene_stamp=stamp;}
    // Diagnostic allocation happens only on explicit inspection, never drawing.
    pub fn renderer_diagnostics_frame(&self)->String {
        let calls:u64=self.r.frames.stats.chunks(crate::frame_plan::layout::stats::LEN)
            .map(|row|row[crate::frame_plan::layout::stats::PASS_DRAWS..].iter().map(|v|*v as u64).sum::<u64>()).sum();
        serde_json::json!({"frame":self.r.core.scene_stamp,"calls":calls,"programs":self.r.core.program_count(),"geometries":self.r.core.geometry_total(),"samples":self.r.core.textures.len()}).to_string()
    }
    pub fn scene_stamp_frame(&mut self, advance:bool)->u32 {
        if advance { self.r.core.scene_stamp=self.r.core.scene_stamp.wrapping_add(1); }
        self.r.core.scene_stamp
    }
    pub fn input_ptr(&mut self) -> u32 { self.r.core.input.as_mut_ptr() as u32 }
    pub fn input_len(&self) -> u32 { self.r.core.input.len() as u32 }
    pub fn bones_ptr(&mut self) -> u32 { self.r.core.bones.as_mut_ptr() as u32 }
    pub fn bones_len(&self) -> u32 { self.r.core.bones.len() as u32 }

    // Where the page writes the shadow receivers' block (f64, shader.rs
    // BfeShadow's float layout), and the cascades' depth maps it reads
    // (texture ids in cascade order; none: shadows off).
    pub fn shadow_input_ptr(&mut self) -> u32 { self.r.core.shadow_input.as_mut_ptr() as u32 }
    pub fn shadow_input_len(&self) -> u32 { self.r.core.shadow_input.len() as u32 }
    // The caller rebuilds borrowed textures first, preserving their logical ids.
    // Only those ids are retained here; an old WebGlTexture cannot survive a loss.
    pub fn set_shadow_maps(&mut self, ids: &[u32]) {
        self.remember_set_shadow_maps(ids); // recovery
 self.r.core.set_shadow_maps(ids) }

    pub fn add_texture(&mut self, resources: &crate::resources::GpuResources, id: u32) -> Result<u32, JsValue> {
        let texture = resources.descriptor_texture(id)?;
        self.remember_texture();
        let key = unsafe { self.r.core.gl.register_external_texture(texture) };
        Ok(self.r.core.add_texture(key))
    }

    // The journal keeps the logical id; restoration receives its current handle.
    pub fn replace_sample_texture(&mut self, id: u32, resources: &crate::resources::GpuResources, source: u32) -> Result<(), JsValue> {
        let texture = resources.descriptor_texture(source)?;
        let key = unsafe { self.r.core.gl.register_external_texture(texture) };
        let old = self.r.replace_sample_texture(id as usize, key).map_err(|e| JsValue::from_str(&e))?;
        self.r.core.gl.unregister_external_texture(old);
        Ok(())
    }

    pub fn set_far_map(&mut self, texture: u32) {
        self.remember_set_far_map(texture); // recovery
 self.r.core.set_far_map(texture) }

    // kind 0 world, 1 skinned, 2 rigid, 3 plain, 4 effect, 9 motion; attributes
    // "name:location,..."; the material uniforms the program reads,
    // comma-separated.
    pub fn add_program(&mut self, kind: u32, vs: &str, fs: &str, attributes: &str, uniforms: &str) -> Result<u32, JsValue> {
        self.remember_add_program(kind, vs, fs, attributes, uniforms); // recovery
 let kind = match kind { 1 => Kind::Skinned, 2 => Kind::Rigid, 3 => Kind::Plain, 4 => Kind::Effect, 9 => Kind::Motion, _ => Kind::World };
        let attributes: Vec<(String, u32)> = attributes.split(',').filter(|s| !s.is_empty())
            .filter_map(|s| { let (n, l) = s.split_once(':')?; Some((n.to_string(), l.parse().ok()?)) }).collect();
        let uniforms: Vec<String> = uniforms.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
        self.r.core.add_program(kind, vs, fs, &attributes, &uniforms).map_err(|e| JsValue::from_str(&e))
    }

    // add_program with the lamps read from the lamp table (lamp lighting on,
    // the program's pass giving each draw its lamps: set_lamps, the records'
    // lamp indices, world_lamps).
    pub fn add_program_lamps(&mut self, kind: u32, vs: &str, fs: &str, attributes: &str, uniforms: &str) -> Result<u32, JsValue> {
        self.remember_add_program_lamps(kind, vs, fs, attributes, uniforms); // recovery
 let kind = match kind { 1 => Kind::Skinned, 2 => Kind::Rigid, 3 => Kind::Plain, 4 => Kind::Effect, _ => Kind::World };
        let attributes: Vec<(String, u32)> = attributes.split(',').filter(|s| !s.is_empty())
            .filter_map(|s| { let (n, l) = s.split_once(':')?; Some((n.to_string(), l.parse().ok()?)) }).collect();
        let uniforms: Vec<String> = uniforms.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
        self.r.core.add_program_lamps(kind, vs, fs, &attributes, &uniforms, true).map_err(|e| JsValue::from_str(&e))
    }

    // The lamp table: eight floats a lamp source (position, radius, colour, pad).
    pub fn set_lamps(&mut self, data: &[f32]) {
        self.remember_set_lamps(data); // recovery
 self.r.core.set_lamps(data) }

    // The key is the source authority for owned shader families. Replaying it
    // uses the same program cache; it never asks a source-free Three material to
    // compile or selects a browser draw when linking fails.
    pub fn add_owned_program(&mut self, key: &str) -> Result<u32, JsValue> {
        self.remember_add_owned_program(key); // recovery
 let id = self.r.core.add_owned_program(key).map_err(|e| JsValue::from_str(&e))?;
        if key.starts_with("post-hud/") {
            let spec = crate::programs::spec(key).map_err(|e| JsValue::from_str(&e))?;
            self.r.post.prepare_program(id as usize, spec.uniforms);
        }
        Ok(id)
    }

    // Draw a captured shader-conformance fixture with its already restored GL
    // inputs. Normal frame submission uses the phase APIs below. The corpus
    // replay runs inside the shipped application, so this stays in the module;
    // it binds through the program cache like every other draw so the cache
    // never believes a different program is current.
    #[allow(clippy::too_many_arguments)]
    pub fn replay_program_draw(&mut self, program: u32, mode: u32, count: i32, index_type: u32, offset: i32, instances: i32, indexed: bool) -> Result<(), JsValue> { use glow::HasContext;
        let core = &mut self.r.core;
        let handle = core.programs.get(program as usize).ok_or_else(|| JsValue::from_str("invalid replay program"))?.program;
        if count < 0 || offset < 0 || instances < 0 { return Err(JsValue::from_str("invalid replay draw range")); }
        core.state.use_program(&core.gl, &mut core.stats, handle);
        unsafe {
            if indexed { core.gl.draw_elements_instanced(mode, count, index_type, offset, instances); }
            else { core.gl.draw_arrays_instanced(mode, offset, count, instances); }
        }
        Ok(())
    }

    // Allocate before recording any dependent buffer/pointer command. JS keeps
    // this numeric id across a context loss, so replay cannot compact the table.
    pub fn add_geometry(&mut self) -> u32 {
        self.remember_add_geometry(); // recovery
 self.r.core.add_geometry() }
    pub fn geometry_buffer(&mut self, g: u32, data: &[u8]) -> u32 {
        self.remember_geometry_buffer(g, data); // recovery
 self.r.core.geometry_buffer(g, data) }
    #[allow(clippy::too_many_arguments)]
    pub fn geometry_pointer(&mut self, g: u32, buffer: u32, location: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: i32) {
        self.remember_geometry_pointer(g, buffer, location, size, ty, normalized, stride, offset); // recovery
 self.r.core.geometry_pointer(g, buffer, location, size, ty, normalized, stride, offset)
    }
    pub fn geometry_index(&mut self, g: u32, data: &[u8], ty: u32) {
        self.remember_geometry_index(g, data, ty); // recovery
 self.r.core.geometry_index(g, data, ty) }
    pub fn geometry_group(&mut self, g: u32, start: i32, count: i32) {
        self.remember_geometry_group(g, start, count); // recovery
 self.r.core.geometry_group(g, start, count) }

    // LinesPass preserves Line/LineSegments/LineLoop topology without converting
    // vertices to triangles. First/indexed/width are retained geometry state;
    // vertex count stays a frame value because the cable's used length changes.
    pub fn geometry_primitive(&mut self, g: u32, mode: u32, first: i32, indexed: bool, width: f32) {
        self.remember_geometry_primitive(g, mode, first, indexed, width); // recovery
 self.r.core.geometry_primitive(g, mode, first, indexed, width);
    }

}

mod interface_resources;
mod hud_resources;
