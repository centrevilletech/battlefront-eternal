// The frame's last passes over the near scene's image, as graphics.js plans
// them (postSteps): the soft focus's quarter-size copy, bloom's extract and
// two blurs, the composite (the soft-focus blur, the glow and the speed
// blur), FXAA or FXAA (FAST), and the upscale to the window (bilinear,
// spatial, or the temporal upscaler's motion resolve, resolve into its
// history and sharpen). Each is one full-screen triangle with three's
// program for the pass's material (made as three compiled it: Kind::Plain),
// into a target: one of three's (its texture under a framebuffer of this
// pass's own; the temporal upscaler's are half floats) or the canvas. A
// pass drawn twice in a frame (bloom's blur) is two steps, each with its
// own values.
//
// The page writes the frame's steps into `input` (STEP f64 each): the
// program, the target (-1: the canvas; -2: the framebuffer bound now, its
// viewport as three set it: the near scene's start, near.js), its size,
// then each uniform the
// program reads as a slot of its uniforms (core.rs Program.uniforms) with a
// type and value (UNIFORM f64 each). Values are only sent when they differ
// from what the program holds.
//
// The same steps draw the full-screen passes outside the post chain
// (draw_screen, counted as the screen pass's: the far scene's image last of
// the near scene's opaque objects, the underwater passes, the storm's cloud
// panorama), each with its material's own state where it is not a
// full-screen pass's (`states`, STATE f64 a step): its depth test and
// function, depth write, blending, and the geometry it draws (three's own,
// where a varying must be interpolated over three's triangles). The page
// writes a step's state before the phase that draws it; drawn, it is zeroed.
use crate::core::{Core, MultiDraw, Value, PASS_POST, PASS_SCREEN};
use glow::HasContext;
use std::collections::HashMap;

pub const STEP_PROGRAM: usize = 0;
pub const STEP_TARGET: usize = 1;
pub const STEP_WIDTH: usize = 2;
pub const STEP_HEIGHT: usize = 3;
pub const STEP_UNIFORMS: usize = 4; // how many follow
pub const STEP_FIRST: usize = 5;
// A uniform: its slot, its type, then up to 16 values.
pub const UNIFORM: usize = 18;
pub const MAX_UNIFORMS: usize = 12;
pub const STEP: usize = STEP_FIRST + MAX_UNIFORMS * UNIFORM;
pub const MAX_STEPS: usize = 8;
// A step's target: the canvas, or the framebuffer bound now.
pub const TARGET_CANVAS: f64 = -1.0;
pub const TARGET_BOUND: f64 = -2.0;
// Uniform types: a float vector of 1-4, an int (a bool too), a mat3, a mat4,
// a sampler (value: the texture; unit: the k-th sampler of the draw).
pub const TYPE_INT: u32 = 5;
pub const TYPE_MAT3: u32 = 9;
pub const TYPE_MAT4: u32 = 16;
pub const TYPE_SAMPLER: u32 = 32;
// A sampler3D (the storm's noise volume: three's Data3DTexture), bound by its
// target on the draw's next unit.
pub const TYPE_SAMPLER_3D: u32 = 33;
// A step's state (draw_screen): flags, the depth function, the blending
// equations (rgb, alpha) and factors (source rgb, destination rgb, source
// alpha, destination alpha), the geometry and its index count.
pub const STATE: usize = 10;
pub const STATE_FLAGS: usize = 0;
pub const STATE_DEPTH_FUNC: usize = 1;
pub const STATE_EQUATIONS: usize = 2;
pub const STATE_FACTORS: usize = 4;
pub const STATE_GEOMETRY: usize = 8;
pub const STATE_DEPTH_TEST: u32 = 1;
pub const STATE_DEPTH_WRITE: u32 = 2;
pub const STATE_BLEND: u32 = 4;
pub const STATE_OWN_GEOMETRY: u32 = 8;

// render-passes.js: one oversized triangle, positions then uvs.
const TRIANGLE: [f32; 15] = [-1.0, -1.0, 0.0, 3.0, -1.0, 0.0, -1.0, 3.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0];
// Where every post program reads them (add_program's attributes).
pub const POSITION: u32 = 0;
pub const UV: u32 = 1;

pub struct PostPass<C: HasContext> {
    vao: C::VertexArray,
    _buffer: C::Buffer,
    // Framebuffers over three's targets' textures.
    targets: Vec<C::Framebuffer>,
    pub input: Vec<f64>,
    // Matrices each program holds, by (program, slot).
    matrices: HashMap<(usize, usize), [f32; 16]>,
    // The steps' own states (draw_screen), STATE f64 a step.
    pub states: Vec<f64>,
}

// A uniform from the page's slots (UNIFORM f64: slot, type, values) into a
// program, unless it holds it already; a sampler is bound to `unit`.
pub(crate) fn set_uniform<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>, matrices: &mut HashMap<(usize, usize), [f32; 16]>, program: usize, u: &[f64], unit: &mut u32) {
    let slot = u[0] as usize;
    let ty = u[1] as u32;
    let v = &u[2..18];
    if slot >= core.programs[program].uniforms.len() {
        return;
    }
    match ty {
        TYPE_SAMPLER => {
            // A texture of none (three's empty one): nothing bound, which a
            // pass only reads behind a switch that is off.
            let k = *unit;
            *unit += 1;
            let texture = if v[0] < 0.0 { None } else { core.textures.get(v[0] as usize).copied() };
            core.state.bind_texture(&core.gl, &mut core.stats, k, texture);
            set_value(core, program, slot, Value::I(k as i32));
        }
        TYPE_SAMPLER_3D => {
            let k = *unit;
            *unit += 1;
            let texture = if v[0] < 0.0 { None } else { core.textures.get(v[0] as usize).copied() };
            core.state.bind_texture_3d(&core.gl, &mut core.stats, k, texture);
            set_value(core, program, slot, Value::I(k as i32));
        }
        TYPE_INT => set_value(core, program, slot, Value::I(v[0] as i32)),
        TYPE_MAT3 | TYPE_MAT4 => {
            let n = ty as usize;
            let mut m = [0.0f32; 16];
            for k in 0..n {
                m[k] = v[k] as f32;
            }
            if matrices.get(&(program, slot)) == Some(&m) {
                return;
            }
            let location = core.programs[program].uniforms[slot].1.clone();
            unsafe {
                if ty == TYPE_MAT3 {
                    core.gl.uniform_matrix_3_f32_slice(location.as_ref(), false, &m[..9]);
                } else {
                    core.gl.uniform_matrix_4_f32_slice(location.as_ref(), false, &m);
                }
            }
            matrices.insert((program, slot), m);
            core.stats.gl_calls += 1;
        }
        n @ 1..=4 => {
            let mut f = [0.0f32; 4];
            for k in 0..n as usize {
                f[k] = v[k] as f32;
            }
            set_value(core, program, slot, Value::F(n as usize, f));
        }
        _ => {}
    }
}

fn set_value<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>, program: usize, slot: usize, value: Value) {
    let gl = &core.gl;
    let u = &mut core.programs[program].uniforms[slot];
    if u.2 == Some(value) {
        return;
    }
    unsafe {
        match value {
            Value::F(1, v) => gl.uniform_1_f32(u.1.as_ref(), v[0]),
            Value::F(2, v) => gl.uniform_2_f32(u.1.as_ref(), v[0], v[1]),
            Value::F(3, v) => gl.uniform_3_f32(u.1.as_ref(), v[0], v[1], v[2]),
            Value::F(_, v) => gl.uniform_4_f32(u.1.as_ref(), v[0], v[1], v[2], v[3]),
            Value::I(i) => gl.uniform_1_i32(u.1.as_ref(), i),
        }
    }
    u.2 = Some(value);
    core.stats.gl_calls += 1;
}

// The state a full-screen pass draws in, as three sets it for one: no depth
// test or write, no blending, back faces culled, every channel written.
pub(crate) fn full_screen<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>) {
    let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
    state.depth_test(gl, stats, false);
    state.blend(gl, stats, false);
    state.set_state(gl, stats, 0, false, false);
    state.color_mask(gl, stats, [true; 4]);
    state.front_face(gl, stats, glow::CCW);
    unsafe {
        gl.disable(glow::POLYGON_OFFSET_FILL);
        gl.disable(glow::STENCIL_TEST);
        gl.disable(glow::SCISSOR_TEST);
    }
    stats.gl_calls += 3;
}

// The framebuffer bound now cleared as three's WebGLBackground clears it
// before a scene: every channel, depth and stencil, with three's clear
// colour (rgba, premultiplied as the page wrote it), every buffer writable,
// no scissor (a target's own scissor test, which three's are without).
pub(crate) fn clear<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>, rgba: &[f64]) {
    let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
    state.color_mask(gl, stats, [true; 4]);
    state.depth_write(gl, stats, true);
    unsafe {
        gl.disable(glow::SCISSOR_TEST);
        gl.stencil_mask(0xffff_ffff);
        gl.clear_color(rgba[0] as f32, rgba[1] as f32, rgba[2] as f32, rgba[3] as f32);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
    }
    stats.gl_calls += 4;
}

impl<C: HasContext> PostPass<C> {
    pub fn new<M: MultiDraw>(core: &mut Core<C, M>) -> PostPass<C> {
        let gl = &core.gl;
        let (vao, buffer) = unsafe {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            let buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            crate::core::spare_view_slot();
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, crate::core::bytes(&TRIANGLE), glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(POSITION);
            gl.vertex_attrib_pointer_f32(POSITION, 3, glow::FLOAT, false, 0, 0);
            gl.enable_vertex_attrib_array(UV);
            gl.vertex_attrib_pointer_f32(UV, 2, glow::FLOAT, false, 0, 9 * 4);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            (vao, buffer)
        };
        PostPass { vao, _buffer: buffer, targets: vec![], input: vec![0.0; MAX_STEPS * STEP], matrices: HashMap::new(), states: vec![0.0; MAX_STEPS * STATE] }
    }

    // A target: a framebuffer over one of three's targets' colour textures.
    pub fn add_target<M: MultiDraw>(&mut self, core: &mut Core<C, M>, texture: u32) -> u32 {
        let gl = &core.gl;
        unsafe {
            let fb = gl.create_framebuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(core.textures[texture as usize]), 0);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.targets.push(fb);
        }
        (self.targets.len() - 1) as u32
    }

    // The steps the page wrote, in order.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, steps: usize) {
        self.draw_steps(core, steps, PASS_POST, false);
    }

    // The steps the page wrote, each with its own state (`states`), counted
    // as the screen pass's: the full-screen passes outside the post chain.
    pub fn draw_screen<M: MultiDraw>(&mut self, core: &mut Core<C, M>, steps: usize) {
        self.draw_steps(core, steps, PASS_SCREEN, true);
    }

    fn draw_steps<M: MultiDraw>(&mut self, core: &mut Core<C, M>, steps: usize, counted: usize, stated: bool) {
        if steps == 0 {
            return;
        }
        full_screen(core);
        let (input, matrices) = (&self.input, &mut self.matrices);
        for s in 0..steps.min(MAX_STEPS) {
            let step = &input[s * STEP..(s + 1) * STEP];
            let program = step[STEP_PROGRAM] as usize;
            // Its state, read once and zeroed for the next phase.
            let mut own = [0.0f64; STATE];
            if stated {
                own.copy_from_slice(&self.states[s * STATE..(s + 1) * STATE]);
                self.states[s * STATE..(s + 1) * STATE].fill(0.0);
            }
            if program >= core.programs.len() {
                continue;
            }
            let target = step[STEP_TARGET];
            if target != TARGET_BOUND {
                let gl = &core.gl;
                unsafe {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, if target < 0.0 { None } else { self.targets.get(target as usize).copied() });
                    gl.viewport(0, 0, step[STEP_WIDTH] as i32, step[STEP_HEIGHT] as i32);
                }
                core.stats.gl_calls += 2;
            }
            core.state.use_program(&core.gl, &mut core.stats, core.programs[program].program);
            let mut unit = 0u32;
            let n = (step[STEP_UNIFORMS] as usize).min(MAX_UNIFORMS);
            for k in 0..n {
                let at = STEP_FIRST + k * UNIFORM;
                set_uniform(core, matrices, program, &step[at..at + UNIFORM], &mut unit);
            }
            if stated {
                set_state(core, &own);
            }
            let flags = own[STATE_FLAGS] as u32;
            let geometry = own[STATE_GEOMETRY] as usize;
            unsafe {
                if flags & STATE_OWN_GEOMETRY != 0 && geometry < core.geometries.len() {
                    let geo = &core.geometries[geometry];
                    core.gl.bind_vertex_array(Some(geo.vao));
                    if geo.primitive != glow::TRIANGLES {
                        core.gl.line_width(geo.line_width);
                        core.stats.gl_calls += 1;
                    }
                    if geo.indexed {
                        core.gl.draw_elements(geo.primitive, own[STATE_GEOMETRY + 1] as i32, geo.index_type, geo.first * geo.index_bytes);
                    } else {
                        core.gl.draw_arrays(geo.primitive, geo.first, own[STATE_GEOMETRY + 1] as i32);
                    }
                } else {
                    core.gl.bind_vertex_array(Some(self.vao));
                    core.gl.draw_arrays(glow::TRIANGLES, 0, 3);
                }
            }
            core.stats.gl_calls += 2;
            core.stats.pass_draws[counted] += 1;
        }
    }
}

// A step's own state (draw_screen) as three's setMaterial sets its
// material's: its depth test and function, its depth write and its
// blending; a step without one draws as a full-screen pass.
fn set_state<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>, own: &[f64]) {
    let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
    let flags = own[STATE_FLAGS] as u32;
    state.depth_test(gl, stats, flags & STATE_DEPTH_TEST != 0);
    if flags & STATE_DEPTH_TEST != 0 {
        state.depth_func(gl, stats, own[STATE_DEPTH_FUNC] as u32);
    }
    state.depth_write(gl, stats, flags & STATE_DEPTH_WRITE != 0);
    state.blend(gl, stats, flags & STATE_BLEND != 0);
    if flags & STATE_BLEND != 0 {
        let (e, f) = (&own[STATE_EQUATIONS..STATE_EQUATIONS + 2], &own[STATE_FACTORS..STATE_FACTORS + 4]);
        state.blend_equation(gl, stats, e[0] as u32, e[1] as u32);
        state.blend_func(gl, stats, f[0] as u32, f[1] as u32, f[2] as u32, f[3] as u32);
    }
}
