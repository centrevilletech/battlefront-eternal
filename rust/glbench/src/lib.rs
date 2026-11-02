// How much a WebGL call costs when Rust makes it through glow (web-sys and
// wasm-bindgen's glue) against the same call made from JavaScript. The page
// runs both over the same context and objects; only who calls differs.
use glow::HasContext;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // An empty JavaScript function: the cost of crossing out of WASM alone.
    #[wasm_bindgen(js_namespace = glbench, js_name = noop)]
    fn js_noop(i: u32);
}

#[wasm_bindgen]
pub struct Bench {
    gl: glow::Context,
    program: glow::Program,
    vaos: Vec<glow::VertexArray>,
    textures: Vec<glow::Texture>,
    model: glow::UniformLocation,
    tint: glow::UniformLocation,
    ubo: glow::Buffer,
    matrix: [f32; 16],
    block: Vec<f32>,
}

const VS: &str = "#version 300 es
layout(std140) uniform Frame { mat4 viewProjection; vec4 fog; };
uniform mat4 model;
in vec3 position;
void main() { gl_Position = viewProjection * model * vec4(position, 1.0); }";
const FS: &str = "#version 300 es
precision mediump float;
uniform vec4 tint;
uniform sampler2D map;
out vec4 color;
void main() { color = texture(map, vec2(0.5)) * tint; }";

#[wasm_bindgen]
impl Bench {
    #[wasm_bindgen(constructor)]
    pub fn new(context: web_sys::WebGl2RenderingContext, objects: u32) -> Bench {
        let gl = glow::Context::from_webgl2_context(context);
        unsafe {
            let program = gl.create_program().unwrap();
            for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
                let s = gl.create_shader(kind).unwrap();
                gl.shader_source(s, src);
                gl.compile_shader(s);
                gl.attach_shader(program, s);
            }
            gl.bind_attrib_location(program, 0, "position");
            gl.link_program(program);
            gl.use_program(Some(program));
            let block = gl.get_uniform_block_index(program, "Frame").unwrap();
            gl.uniform_block_binding(program, block, 0);
            let model = gl.get_uniform_location(program, "model").unwrap();
            let tint = gl.get_uniform_location(program, "tint").unwrap();
            let vertices: [f32; 9] = [0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.001, 0.0];
            let bytes: &[u8] = core::slice::from_raw_parts(vertices.as_ptr() as *const u8, 36);
            let indices: [u16; 3] = [0, 1, 2];
            let ibytes: &[u8] = core::slice::from_raw_parts(indices.as_ptr() as *const u8, 6);
            let mut vaos = Vec::new();
            let mut textures = Vec::new();
            for _ in 0..objects {
                let vao = gl.create_vertex_array().unwrap();
                gl.bind_vertex_array(Some(vao));
                let vb = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vb));
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
                let ib = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ib));
                gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, ibytes, glow::STATIC_DRAW);
                vaos.push(vao);
                let t = gl.create_texture().unwrap();
                gl.bind_texture(glow::TEXTURE_2D, Some(t));
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, 1, 1, 0, glow::RGBA, glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&[255, 255, 255, 255])));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
                textures.push(t);
            }
            gl.bind_vertex_array(None);
            let ubo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
            gl.buffer_data_size(glow::UNIFORM_BUFFER, 80, glow::DYNAMIC_DRAW);
            gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(ubo));
            let mut matrix = [0.0f32; 16];
            matrix[0] = 1.0; matrix[5] = 1.0; matrix[10] = 1.0; matrix[15] = 1.0;
            Bench { gl, program, vaos, textures, model, tint, ubo, matrix, block: matrix.iter().copied().chain([0.0; 4]).collect() }
        }
    }

    // The program and the frame's uniform block, as the JavaScript loops
    // expect them bound before they run.
    pub fn bind(&self) {
        unsafe {
            self.gl.use_program(Some(self.program));
            self.gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(self.ubo));
        }
    }

    // A draw's usual state changes, then the draw.
    pub fn draws(&mut self, n: u32) {
        let gl = &self.gl;
        let k = self.vaos.len();
        unsafe {
            for i in 0..n as usize {
                self.matrix[12] = i as f32 * 1e-6;
                gl.bind_vertex_array(Some(self.vaos[i % k]));
                gl.uniform_matrix_4_f32_slice(Some(&self.model), false, &self.matrix);
                gl.uniform_4_f32(Some(&self.tint), 1.0, 1.0, 1.0, (i & 1) as f32);
                gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[i % k]));
                gl.draw_elements(glow::TRIANGLES, 3, glow::UNSIGNED_SHORT, 0);
            }
            gl.bind_vertex_array(None);
        }
    }

    pub fn uniform4f(&self, n: u32) {
        unsafe { for i in 0..n { self.gl.uniform_4_f32(Some(&self.tint), 1.0, 1.0, 1.0, (i & 1) as f32); } }
    }

    pub fn matrix(&mut self, n: u32) {
        unsafe {
            for i in 0..n {
                self.matrix[12] = i as f32 * 1e-6;
                self.gl.uniform_matrix_4_f32_slice(Some(&self.model), false, &self.matrix);
            }
        }
    }

    pub fn bind_vao(&self, n: u32) {
        let k = self.vaos.len();
        unsafe { for i in 0..n as usize { self.gl.bind_vertex_array(Some(self.vaos[i % k])); } self.gl.bind_vertex_array(None); }
    }

    pub fn bind_texture(&self, n: u32) {
        let k = self.textures.len();
        unsafe { for i in 0..n as usize { self.gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[i % k])); } }
    }

    pub fn draw_only(&self, n: u32) {
        unsafe {
            self.gl.bind_vertex_array(Some(self.vaos[0]));
            for _ in 0..n { self.gl.draw_elements(glow::TRIANGLES, 3, glow::UNSIGNED_SHORT, 0); }
            self.gl.bind_vertex_array(None);
        }
    }

    pub fn ubo_upload(&mut self, n: u32) {
        unsafe {
            self.gl.bind_buffer(glow::UNIFORM_BUFFER, Some(self.ubo));
            for i in 0..n {
                self.block[16] = i as f32;
                let bytes = core::slice::from_raw_parts(self.block.as_ptr() as *const u8, self.block.len() * 4);
                self.gl.buffer_sub_data_u8_slice(glow::UNIFORM_BUFFER, 0, bytes);
            }
        }
    }

    pub fn noop(&self, n: u32) { for i in 0..n { js_noop(i); } }

    // Rust's own loop with nothing crossing: the floor under every loop above.
    pub fn empty(&mut self, n: u32) -> f32 {
        let mut s = 0.0f32;
        for i in 0..n { self.matrix[12] = i as f32 * 1e-6; s += self.matrix[12]; }
        s
    }
}
