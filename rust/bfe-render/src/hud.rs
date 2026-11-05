// The battle HUD (hud.js): the interface's quads (src/interface/batch.js),
// drawn over the finished frame on the canvas through the interface's own
// perspective camera, as three draws the batch's mesh: each run of quads
// with one texture (and mask), in the batch's order, alpha blended, neither
// depth tested nor written, both sides at once (a ShaderMaterial is drawn in
// one pass, even double-sided and blended). Three's program for the batch's
// material is used as three compiled it (Kind::Plain).
//
// The batch's vertex arrays are mirrored here (`position`, `uv`, `mask_uv`,
// `tint`, as batch.js lays them out); the page copies the vertices that
// changed since the last frame into them and says which (IN_UPLOAD), and
// they go up as one range an array. Then the frame's runs (IN_FIRST_GROUP,
// GROUP f64 each: first index, index count, texture, mask texture, masked).
use crate::core::{bytes, Core, MultiDraw, Value, PASS_HUD};
use crate::post::{set_uniform, UNIFORM};
use glow::HasContext;
use std::collections::HashMap;

pub const QUADS: usize = 2048;
pub const MAX_GROUPS: usize = 1024;
// The page's values (f64): how many runs, the program, the camera's two
// matrices as uniforms (post.rs UNIFORM each), the slots of map, mask and
// masked, the vertices changed [start, end), the canvas's size, then the
// runs.
pub const IN_GROUPS: usize = 0;
pub const IN_PROGRAM: usize = 1;
pub const IN_CAMERA: usize = 2;
pub const IN_SLOTS: usize = IN_CAMERA + 2 * UNIFORM;
pub const IN_UPLOAD: usize = IN_SLOTS + 3;
pub const IN_VIEWPORT: usize = IN_UPLOAD + 2;
pub const IN_FIRST_GROUP: usize = IN_VIEWPORT + 2;
pub const GROUP: usize = 5;
pub const INPUT_LEN: usize = IN_FIRST_GROUP + MAX_GROUPS * GROUP;
// Each array's floats a vertex, and where the program reads it.
const SIZES: [usize; 4] = [3, 2, 2, 4];
pub const LOCATIONS: [u32; 4] = [0, 1, 2, 3];

pub struct HudPass<C: HasContext> {
    vao: Option<C::VertexArray>,
    buffers: Vec<C::Buffer>,
    pub input: Vec<f64>,
    pub position: Vec<f32>,
    pub uv: Vec<f32>,
    pub mask_uv: Vec<f32>,
    pub tint: Vec<f32>,
    matrices: HashMap<(usize, usize), [f32; 16]>,
}

impl<C: HasContext> HudPass<C> {
    // A restored context has fresh buffers but the CPU mirrors are current.
    pub fn reupload<M: MultiDraw>(&self, core: &mut Core<C, M>) {
        for (k, data) in [&self.position, &self.uv, &self.mask_uv, &self.tint].iter().enumerate() {
            if let Some(buffer) = self.buffers.get(k) { unsafe {
                core.gl.bind_buffer(glow::ARRAY_BUFFER, Some(*buffer));
                core.gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes(data));
            }}
        }
        unsafe { core.gl.bind_buffer(glow::ARRAY_BUFFER, None); }
    }

    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> HudPass<C> {
        let v = QUADS * 4;
        HudPass {
            vao: None, buffers: vec![], input: vec![0.0; INPUT_LEN],
            position: vec![0.0; v * 3], uv: vec![0.0; v * 2], mask_uv: vec![0.0; v * 2], tint: vec![0.0; v * 4], matrices: HashMap::new(),
        }
    }

    // The batch's index (batch.js: two triangles a quad), once.
    pub fn init<M: MultiDraw>(&mut self, core: &mut Core<C, M>, index: &[u32]) {
        if self.vao.is_some() {
            return;
        }
        let gl = &core.gl;
        unsafe {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            for (k, size) in SIZES.iter().enumerate() {
                let b = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                gl.buffer_data_size(glow::ARRAY_BUFFER, (QUADS * 4 * size * 4) as i32, glow::DYNAMIC_DRAW);
                gl.enable_vertex_attrib_array(LOCATIONS[k]);
                gl.vertex_attrib_pointer_f32(LOCATIONS[k], *size as i32, glow::FLOAT, false, 0, 0);
                self.buffers.push(b);
            }
            let ib = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ib));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(index), glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            self.buffers.push(ib);
            self.vao = Some(vao);
        }
    }

    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        let Some(vao) = self.vao else { return };
        let input = &self.input;
        let groups = (input[IN_GROUPS] as usize).min(MAX_GROUPS);
        let (start, end) = ((input[IN_UPLOAD] as usize).min(QUADS * 4), (input[IN_UPLOAD + 1] as usize).min(QUADS * 4));
        let gl = &core.gl;
        if end > start {
            for (k, data) in [&self.position, &self.uv, &self.mask_uv, &self.tint].iter().enumerate() {
                let size = SIZES[k];
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.buffers[k]));
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, (start * size * 4) as i32, bytes(&data[start * size..end * size]));
                }
                core.stats.gl_calls += 2;
            }
            unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, None) };
            core.stats.gl_calls += 1;
        }
        if groups == 0 {
            return;
        }
        let program = input[IN_PROGRAM] as usize;
        if program >= core.programs.len() {
            return;
        }
        {
            let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
            unsafe {
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                gl.viewport(0, 0, input[IN_VIEWPORT] as i32, input[IN_VIEWPORT + 1] as i32);
            }
            stats.gl_calls += 2;
            state.depth_test(gl, stats, false);
            state.set_state(gl, stats, 2, false, false);
            state.blend(gl, stats, true);
            state.blend_equation(gl, stats, glow::FUNC_ADD, glow::FUNC_ADD);
            state.blend_func(gl, stats, glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            state.color_mask(gl, stats, [true; 4]);
            state.front_face(gl, stats, glow::CCW);
            unsafe {
                gl.disable(glow::POLYGON_OFFSET_FILL);
                gl.disable(glow::STENCIL_TEST);
                gl.disable(glow::SCISSOR_TEST);
            }
            stats.gl_calls += 3;
            state.use_program(gl, stats, core.programs[program].program);
        }
        let mut unit = 0;
        for k in 0..2 {
            let at = IN_CAMERA + k * UNIFORM;
            set_uniform(core, &mut self.matrices, program, &input[at..at + UNIFORM], &mut unit);
        }
        let slots = [input[IN_SLOTS] as i64, input[IN_SLOTS + 1] as i64, input[IN_SLOTS + 2] as i64];
        let set = |core: &mut Core<C, M>, slot: i64, value: Value| {
            if slot < 0 {
                return;
            }
            let gl = &core.gl;
            let u = &mut core.programs[program].uniforms[slot as usize];
            if u.2 != Some(value) {
                unsafe {
                    match value {
                        Value::I(i) => gl.uniform_1_i32(u.1.as_ref(), i),
                        Value::F(_, v) => gl.uniform_1_f32(u.1.as_ref(), v[0]),
                    }
                }
                u.2 = Some(value);
                core.stats.gl_calls += 1;
            }
        };
        // map on unit 0, mask on unit 1.
        set(core, slots[0], Value::I(0));
        set(core, slots[1], Value::I(1));
        unsafe { core.gl.bind_vertex_array(Some(vao)) };
        core.stats.gl_calls += 1;
        for g in 0..groups {
            let r = &input[IN_FIRST_GROUP + g * GROUP..IN_FIRST_GROUP + (g + 1) * GROUP];
            let (first, count, map, mask, masked) = (r[0] as i32, r[1] as i32, r[2], r[3], r[4] != 0.0);
            // A texture still loading: three draws it with its empty one,
            // which leaves the canvas as it was.
            if count <= 0 || map < 0.0 || mask < 0.0 {
                continue;
            }
            let (map, mask) = (core.textures[map as usize], core.textures[mask as usize]);
            core.state.bind_texture(&core.gl, &mut core.stats, 0, Some(map));
            core.state.bind_texture(&core.gl, &mut core.stats, 1, Some(mask));
            set(core, slots[2], Value::I(masked as i32));
            unsafe { core.gl.draw_elements(glow::TRIANGLES, count, glow::UNSIGNED_INT, first * 4) };
            core.stats.gl_calls += 1;
            core.stats.pass_draws[PASS_HUD] += 1;
        }
    }
}
