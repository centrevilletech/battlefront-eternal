// The renderer in the page: a WebGL2 context shared with three.js, three's
// textures used where they are, inputs the page writes into WASM memory.
use crate::renderer::{MultiDraw, Renderer, IN_RECORDS, MAX_BONES, REC_STRIDE};
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
}

#[wasm_bindgen]
impl BfeRenderer {
    #[wasm_bindgen(constructor)]
    pub fn new(context: web_sys::WebGl2RenderingContext) -> BfeRenderer {
        let ext = context.get_extension("WEBGL_multi_draw").ok().flatten().map(|e| e.unchecked_into::<web_sys::WebglMultiDraw>());
        let multi = WebMultiDraw(ext, context.clone());
        BfeRenderer { r: Renderer::new(glow::Context::from_webgl2_context(context), multi) }
    }

    // Where the page writes the frame's values and records (f64), and the
    // drawn soldiers' bone matrices (f32), as byte offsets into memory.
    pub fn input_ptr(&mut self) -> u32 { self.r.input.as_mut_ptr() as u32 }
    pub fn input_len(&self) -> u32 { self.r.input.len() as u32 }
    pub fn records_at(&self) -> u32 { IN_RECORDS as u32 }
    pub fn record_stride(&self) -> u32 { REC_STRIDE as u32 }
    pub fn bones_ptr(&mut self) -> u32 { self.r.bones.as_mut_ptr() as u32 }
    pub fn bones_len(&self) -> u32 { (MAX_BONES * 16) as u32 }

    pub fn add_texture(&mut self, texture: web_sys::WebGlTexture) -> u32 {
        let key = unsafe { self.r.gl.register_external_texture(texture) };
        self.r.add_texture(key)
    }

    pub fn set_far_map(&mut self, texture: u32) { self.r.set_far_map(texture) }

    // kind 0 world, 1 skinned, 2 rigid; attributes "name:location,..."; the
    // material uniforms the program reads, comma-separated.
    pub fn add_program(&mut self, kind: u32, vs: &str, fs: &str, attributes: &str, uniforms: &str) -> Result<u32, JsValue> {
        let kind = match kind { 1 => Kind::Skinned, 2 => Kind::Rigid, _ => Kind::World };
        let attributes: Vec<(String, u32)> = attributes.split(',').filter(|s| !s.is_empty())
            .filter_map(|s| { let (n, l) = s.split_once(':')?; Some((n.to_string(), l.parse().ok()?)) }).collect();
        let uniforms: Vec<String> = uniforms.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
        self.r.add_program(kind, vs, fs, &attributes, &uniforms).map_err(|e| JsValue::from_str(&e))
    }

    pub fn add_geometry(&mut self) -> u32 { self.r.add_geometry() }
    pub fn geometry_buffer(&mut self, g: u32, data: &[u8]) -> u32 { self.r.geometry_buffer(g, data) }
    pub fn geometry_pointer(&mut self, g: u32, buffer: u32, location: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: i32) {
        self.r.geometry_pointer(g, buffer, location, size, ty, normalized, stride, offset)
    }
    pub fn geometry_index(&mut self, g: u32, data: &[u8], ty: u32) { self.r.geometry_index(g, data, ty) }
    pub fn geometry_group(&mut self, g: u32, start: i32, count: i32) { self.r.geometry_group(g, start, count) }

    pub fn add_world(&mut self, geometry: u32, program: u32, order: u32, flags: u32, bounds: &[f64]) -> u32 {
        self.r.add_world(geometry, program, order, flags, bounds)
    }
    pub fn world_texture(&mut self, w: u32, unit: u32, texture: u32) { self.r.world_texture(w, unit, texture) }
    pub fn world_uniform(&mut self, w: u32, name: &str, values: &[f32], int: bool) -> bool { self.r.world_uniform(w, name, values, int) }
    pub fn world_cells(&mut self, w: u32, positions: &[f32], index: &[u32], cell_size: f64) { self.r.world_cells(w, positions, index, cell_size) }
    pub fn sort(&mut self, by_program: bool) { self.r.sort(by_program) }

    // Draws the frame; returns [world draws, instanced draws, instances,
    // program switches, GL calls].
    pub fn frame(&mut self, records: u32) -> Result<Vec<u32>, JsValue> {
        let s = self.r.frame(records as usize).map_err(|e| JsValue::from_str(&e))?;
        Ok(vec![s.world_draws, s.instance_draws, s.instances, s.program_switches, s.gl_calls])
    }
}
