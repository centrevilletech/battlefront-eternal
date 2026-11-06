// The renderer in the page: a WebGL2 context shared with three.js, three's
// textures used where they are, inputs the page writes into WASM memory.
// Here the core's bindings and each phase's; each pass's own are in a file
// of their own here, a `#[wasm_bindgen] impl BfeRenderer` block each.
mod recovery;
mod canopies;
mod effects;
mod far;
mod foliage;
mod hud;
mod interface;
mod instances;
mod motion;
mod post;
mod shadow;
mod terrain;
mod transparent;
mod view;
mod water;
mod world;

use crate::core::MultiDraw;
use crate::renderer::Renderer;
use crate::shader::Kind;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

pub struct WebMultiDraw(Option<web_sys::WebglMultiDraw>, web_sys::WebGl2RenderingContext);

impl MultiDraw for WebMultiDraw {
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
}

#[wasm_bindgen]
pub struct BfeRenderer {
    r: Renderer<glow::Context, WebMultiDraw>,
    recovery: recovery::Recovery,
    rebuilding: bool,
}

#[wasm_bindgen]
impl BfeRenderer {
    #[wasm_bindgen(constructor)]
    pub fn new(context: web_sys::WebGl2RenderingContext) -> BfeRenderer {
        let ext = context.get_extension("WEBGL_multi_draw").ok().flatten().map(|e| e.unchecked_into::<web_sys::WebglMultiDraw>());
        let multi = WebMultiDraw(ext, context.clone());
        BfeRenderer { r: Renderer::new(glow::Context::from_webgl2_context(context), multi), recovery: recovery::Recovery::default(), rebuilding: false }
    }

    // Where the page writes the frame's values and records (f64), and the
    // drawn soldiers' bone matrices (f32), as byte offsets into memory.
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

    pub fn add_texture(&mut self, texture: web_sys::WebGlTexture) -> u32 {
        self.remember_texture();
        let key = unsafe { self.r.core.gl.register_external_texture(texture) };
        self.r.core.add_texture(key)
    }

    // The journal keeps the logical id; restoration receives its current handle.
    pub fn replace_sample_texture(&mut self, id: u32, texture: web_sys::WebGlTexture) -> Result<(), JsValue> {
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
 self.r.core.add_owned_program(key).map_err(|e| JsValue::from_str(&e))
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

    // The phases (renderer.rs). Each returns its stats: [world draws,
    // instanced draws, instances, program switches, GL calls], then each
    // pass's draws in pass order (world, instances, far, terrain,
    // transparent, effects, foliage, view, post, hud, water, shadow, motion,
    // canopies, screen).
    pub fn far_frame(&mut self) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.far_frame().map_err(|e| JsValue::from_str(&e))?.values())
    }
    pub fn frame(&mut self, records: u32) -> Result<Vec<u32>, JsValue> { Ok(self.r.frame(records as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // three: how many of three's own transparent items the page wrote the
    // keys of (web/transparent.rs); image: the post steps it wrote for the
    // far scene's image, drawn first.
    pub fn transparent_frame(&mut self, three: u32, image: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.transparent_frame(three as usize, image as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // An effects batch drawn into a texture of its own (registered) of
    // width x height, cleared first: its count instances and values as the
    // page wrote them from `at` of the effects' data, capacity its room
    // (web/effects.rs). three binds its own framebuffer again after.
    #[allow(clippy::too_many_arguments)]
    pub fn fx_into_frame(&mut self, batch: u32, target: u32, width: u32, height: u32, count: u32, at: u32, capacity: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.fx_into_frame(batch as usize, target, width as i32, height as i32, count as usize, at as usize, capacity as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // records: how many the page wrote for the view's parts.
    pub fn view_frame(&mut self, records: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.view_frame(records as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // steps: how many the page wrote (web/post.rs).
    pub fn post_frame(&mut self, steps: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.post_frame(steps as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // records: how many the page wrote for the mirror's camera.
    pub fn mirror_frame(&mut self, records: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.mirror_frame(records as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // What a mirror's render blends: three and image, as for transparent_frame.
    pub fn mirror_transparent_frame(&mut self, three: u32, image: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.mirror_transparent_frame(three as usize, image as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // records: how many the page wrote (web/motion.rs).
    pub fn motion_frame(&mut self, records: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.motion_frame(records as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    pub fn hud_frame(&mut self) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.hud_frame().map_err(|e| JsValue::from_str(&e))?.values())
    }
    // clear: three's clear colour (rgba, premultiplied); steps: the post
    // steps the page wrote for the far scene's image (near.js).
    pub fn near_begin(&mut self, clear: &[f64], steps: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.near_begin(clear, steps as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
    // steps: the post steps the page wrote for a full-screen pass outside the
    // post chain, each with its own state (web/post.rs post_states_ptr).
    pub fn screen_frame(&mut self, steps: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.screen_frame(steps as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }
}
