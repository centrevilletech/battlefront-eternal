// The battle's effects, drawn in the near scene's transparent list; their
// simulation stays in the page.
//
// Particle batches: particles.js's (bolts and their glow, sparks, smoke,
// the effects' particles worked out on the CPU) and effects-gpu.js's
// analytic ones (particles placed in the vertex shader from their spawn
// values and their effect's emitter). Each is one instanced draw of quads,
// with the program three compiled for it and three's GL state for its
// material. The page writes each shown batch's instances into `data` (a CPU
// batch's all, an analytic batch's from the first that changed), attribute
// by attribute as the batch keeps them; here they are interleaved and
// uploaded with one call into a vertex buffer of the batch's own, which its
// vertex array reads. An analytic batch's owner (effects-gpu.js GpuEffects:
// its tracks, and the frame's emitters, a row an effect) is a texture of
// three's for the tracks and one of the renderer's for the emitters, a row
// of which the page writes for each effect that is playing.
//
// The page lists the frame's draws in three's order (src/rust-render/
// effects.js), each batch's GL state as three's setMaterial sets it.
use crate::core::{bytes, Core, MultiDraw, Value, PASS_EFFECTS, UNIT_MAP};
use glow::HasContext;

// The texture units of the analytic batches' owners (tracks, emitters),
// and of the scene's depth (soft particles).
pub const UNIT_TRACKS: u32 = 4;
pub const UNIT_EMITTERS: u32 = 5;
pub const UNIT_SCENE_DEPTH: u32 = 6;

// The page's inputs a frame (f64, `input`): how many draws and owners, the
// soft particles' values, then the owners' entries, then the draws'.
pub const IN_DRAWS: usize = 0;
pub const IN_OWNERS: usize = 1;
pub const IN_SOFT: usize = 2; // softParticles, depthResolution (2), cameraRange (2), the depth's texture
pub const OWNER_AT: usize = 8;
pub const OWNER_STRIDE: usize = 4; // owner, rows, rows written, where in `data`
pub const MAX_OWNERS: usize = 8;
pub const DRAW_AT: usize = OWNER_AT + MAX_OWNERS * OWNER_STRIDE;
pub const DRAW_STRIDE: usize = 5; // batch, instances, first written, where in `data`, capacity
pub const MAX_DRAWS: usize = 512;
pub const INPUT_LEN: usize = DRAW_AT + MAX_DRAWS * DRAW_STRIDE;
// Floats of instance data a frame, to start with (the page asks for more).
pub const DATA_LEN: usize = 1 << 18;
// Floats of an emitter row (effects-gpu.js: three RGBA texels).
const EMITTER_FLOATS: usize = 12;

// The material values the batches' programs read from the frame (by name,
// as the page gives them to add_program): softParticles, depthResolution,
// cameraRange, then an owner's trackRows and emitterRows.
const NAMES: [&str; 5] = ["softParticles", "depthResolution", "cameraRange", "trackRows", "emitterRows"];

// Three's blending as setBlending makes it: enabled, equation rgb and
// alpha, source and destination factors rgb and alpha.
pub type Blend = [u32; 7];

struct Batch<C: HasContext> {
    program: usize,
    texture: Option<C::Texture>,
    blend: Blend,
    vao: C::VertexArray,
    vbo: C::Buffer,
    // Instances the vertex buffer has room for.
    capacity: usize,
    // The instance attributes' sizes (floats), as the page writes them.
    sizes: Vec<usize>,
    stride: usize,
    owner: Option<usize>,
    // Where the program keeps NAMES (its material values), if it reads them.
    slots: [Option<usize>; 5],
}

struct Owner<C: HasContext> {
    tracks: C::Texture,
    track_rows: f32,
    emitters: C::Texture,
    rows: usize,
}

pub struct EffectsPass<C: HasContext> {
    quad: C::Buffer,
    quad_index: C::Buffer,
    batches: Vec<Batch<C>>,
    owners: Vec<Owner<C>>,
    pub input: Vec<f64>,
    pub data: Vec<f32>,
    staging: Vec<f32>,
}

// The quad every batch draws (particles.js QUAD, QUAD_INDEX).
const QUAD: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0];
const QUAD_INDEX: [u16; 6] = [0, 1, 2, 0, 2, 3];

impl<C: HasContext> EffectsPass<C> {
    pub fn new<M: MultiDraw>(core: &mut Core<C, M>) -> EffectsPass<C> {
        let gl = &core.gl;
        let (quad, quad_index) = unsafe {
            let quad = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&QUAD), glow::STATIC_DRAW);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            let index = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(index));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(&QUAD_INDEX), glow::STATIC_DRAW);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
            (quad, index)
        };
        EffectsPass { quad, quad_index, batches: vec![], owners: vec![], input: vec![0.0; INPUT_LEN], data: vec![0.0; DATA_LEN], staging: vec![] }
    }

    // Room for `floats` of instance data a frame.
    pub fn reserve(&mut self, floats: usize) {
        if floats > self.data.len() {
            self.data.resize(floats, 0.0);
        }
    }

    // An analytic batches' owner: three's texture of its tracks (rows of
    // them), and the renderer's of its emitters.
    pub fn add_owner<M: MultiDraw>(&mut self, core: &Core<C, M>, tracks: u32, track_rows: f32) -> u32 {
        let emitters = unsafe { core.gl.create_texture().unwrap() };
        self.owners.push(Owner { tracks: core.textures[tracks as usize], track_rows, emitters, rows: 0 });
        (self.owners.len() - 1) as u32
    }

    // A particle batch: its program, texture (none: -1) and blending; the
    // locations of its corner and of its instance attributes, with their
    // sizes (location, size pairs, in the order the page writes them); its
    // owner (an analytic batch's; none: -1).
    pub fn add_batch<M: MultiDraw>(&mut self, core: &Core<C, M>, program: u32, texture: i32, blend: &[u32], corner: u32, attributes: &[u32], owner: i32) -> u32 {
        let gl = &core.gl;
        let sizes: Vec<usize> = attributes.chunks(2).map(|a| a[1] as usize).collect();
        let stride: usize = sizes.iter().sum();
        let (vao, vbo) = unsafe {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.quad));
            gl.enable_vertex_attrib_array(corner);
            gl.vertex_attrib_pointer_f32(corner, 2, glow::FLOAT, false, 0, 0);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.quad_index));
            let vbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let mut offset = 0;
            for a in attributes.chunks(2) {
                gl.enable_vertex_attrib_array(a[0]);
                gl.vertex_attrib_pointer_f32(a[0], a[1] as i32, glow::FLOAT, false, (stride * 4) as i32, (offset * 4) as i32);
                gl.vertex_attrib_divisor(a[0], 1);
                offset += a[1] as usize;
            }
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            (vao, vbo)
        };
        let p = &core.programs[program as usize];
        let slots = NAMES.map(|name| p.uniforms.iter().position(|(n, _, _)| n == name));
        let mut b = [0u32; 7];
        b.copy_from_slice(&blend[..7]);
        self.batches.push(Batch {
            program: program as usize, texture: if texture < 0 { None } else { Some(core.textures[texture as usize]) }, blend: b,
            vao, vbo, capacity: 0, sizes, stride, owner: if owner < 0 { None } else { Some(owner as usize) }, slots,
        });
        // Its samplers besides the map on their units.
        unsafe {
            gl.use_program(Some(p.program));
            for (name, unit) in [("tracks", UNIT_TRACKS), ("emitters", UNIT_EMITTERS), ("sceneDepth", UNIT_SCENE_DEPTH)] {
                if let Some(l) = gl.get_uniform_location(p.program, name) {
                    gl.uniform_1_i32(Some(&l), unit as i32);
                }
            }
            gl.use_program(None);
        }
        (self.batches.len() - 1) as u32
    }

    // A batch's texture again (three made it anew).
    pub fn batch_texture<M: MultiDraw>(&mut self, core: &Core<C, M>, batch: u32, texture: i32) {
        if let Some(b) = self.batches.get_mut(batch as usize) {
            b.texture = if texture < 0 { None } else { core.textures.get(texture as usize).copied() };
        }
    }

    // The frame's draws (as the page wrote them), each batch's instances
    // uploaded first, in the state of the transparent list: depth tested,
    // not written; the near view's frame block bound.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        let draws = (self.input[IN_DRAWS] as usize).min(MAX_DRAWS);
        if draws == 0 {
            return;
        }
        self.upload_owners(core);
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        // What setMaterial leaves for every batch alike: no culling,
        // counter-clockwise front faces, depth tested less-or-equal and not
        // written, every channel written, no stencil, polygon offset or
        // alpha to coverage.
        state.cull(gl, stats, 2);
        state.front_face(gl, stats, glow::CCW);
        state.depth_test(gl, stats, true);
        state.depth_func(gl, stats, glow::LEQUAL);
        state.depth_write(gl, stats, false);
        state.color_mask(gl, stats, [true; 4]);
        state.alpha_to_coverage(gl, stats, false);
        unsafe {
            gl.disable(glow::STENCIL_TEST);
            gl.disable(glow::POLYGON_OFFSET_FILL);
        }
        stats.gl_calls += 2;
        let soft = [self.input[IN_SOFT] as f32, self.input[IN_SOFT + 1] as f32, self.input[IN_SOFT + 2] as f32, self.input[IN_SOFT + 3] as f32, self.input[IN_SOFT + 4] as f32];
        // The scene's depth for soft particles, else nothing on its unit
        // (never read then).
        let depth = self.input[IN_SOFT + 5];
        let depth = if soft[0] > 0.5 && depth >= 0.0 { core.textures.get(depth as usize).copied() } else { None };
        state.bind_texture(gl, stats, UNIT_SCENE_DEPTH, depth);
        for d in 0..draws {
            let e = &self.input[DRAW_AT + d * DRAW_STRIDE..DRAW_AT + (d + 1) * DRAW_STRIDE];
            let (bi, count, from, at, capacity) = (e[0] as usize, e[1] as usize, e[2] as usize, e[3] as usize, e[4] as usize);
            let Some(b) = self.batches.get_mut(bi) else { continue };
            if count == 0 {
                continue;
            }
            unsafe {
                // The instances written this frame, interleaved, into the
                // batch's buffer (made again when the batch's room grew).
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b.vbo));
                stats.gl_calls += 1;
                if capacity != b.capacity {
                    gl.buffer_data_size(glow::ARRAY_BUFFER, (capacity.max(count) * b.stride * 4) as i32, glow::DYNAMIC_DRAW);
                    b.capacity = capacity.max(count);
                    stats.gl_calls += 1;
                }
                if from < count {
                    let n = count - from;
                    self.staging.clear();
                    self.staging.resize(n * b.stride, 0.0);
                    let mut src = at;
                    let mut offset = 0;
                    for &size in &b.sizes {
                        let plane = &self.data[src..src + n * size];
                        for i in 0..n {
                            self.staging[i * b.stride + offset..i * b.stride + offset + size].copy_from_slice(&plane[i * size..(i + 1) * size]);
                        }
                        src += n * size;
                        offset += size;
                    }
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, (from * b.stride * 4) as i32, bytes(&self.staging));
                    stats.gl_calls += 1;
                }
                let program = &mut core.programs[b.program];
                state.use_program(gl, stats, program.program);
                // The frame's soft particle values, and the owner's rows.
                let owner = b.owner.map(|o| &self.owners[o]);
                let values: [Option<Value>; 5] = [
                    Some(Value::F(1, [soft[0], 0.0, 0.0, 0.0])),
                    Some(Value::F(2, [soft[1], soft[2], 0.0, 0.0])),
                    Some(Value::F(2, [soft[3], soft[4], 0.0, 0.0])),
                    owner.map(|o| Value::F(1, [o.track_rows, 0.0, 0.0, 0.0])),
                    owner.map(|o| Value::F(1, [o.rows as f32, 0.0, 0.0, 0.0])),
                ];
                for (k, slot) in b.slots.iter().enumerate() {
                    let (Some(slot), Some(value)) = (slot, values[k]) else { continue };
                    let u = &mut program.uniforms[*slot];
                    if u.2 != Some(value) {
                        if let Value::F(n, v) = value {
                            if n == 1 {
                                gl.uniform_1_f32(u.1.as_ref(), v[0]);
                            } else {
                                gl.uniform_2_f32(u.1.as_ref(), v[0], v[1]);
                            }
                        }
                        u.2 = Some(value);
                        stats.gl_calls += 1;
                    }
                }
                if b.texture.is_some() {
                    state.bind_texture(gl, stats, UNIT_MAP, b.texture);
                }
                if let Some(o) = owner {
                    state.bind_texture(gl, stats, UNIT_TRACKS, Some(o.tracks));
                    state.bind_texture(gl, stats, UNIT_EMITTERS, Some(o.emitters));
                }
                let blend = &b.blend;
                state.blend(gl, stats, blend[0] != 0);
                if blend[0] != 0 {
                    state.blend_equation(gl, stats, blend[1], blend[2]);
                    state.blend_func(gl, stats, blend[3], blend[4], blend[5], blend[6]);
                }
                gl.bind_vertex_array(Some(b.vao));
                gl.draw_elements_instanced(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0, count as i32);
                stats.gl_calls += 2;
                stats.pass_draws[PASS_EFFECTS] += 1;
            }
        }
    }

    // The emitters of each owner the frame's analytic batches read: its
    // rows (made again when the page's texture has a new size), those
    // written uploaded.
    fn upload_owners<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        let owners = (self.input[IN_OWNERS] as usize).min(MAX_OWNERS);
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        for k in 0..owners {
            let e = &self.input[OWNER_AT + k * OWNER_STRIDE..OWNER_AT + (k + 1) * OWNER_STRIDE];
            let (o, rows, written, at) = (e[0] as usize, e[1] as usize, e[2] as usize, e[3] as usize);
            let Some(owner) = self.owners.get_mut(o) else { continue };
            unsafe {
                state.bind_texture(gl, stats, UNIT_EMITTERS, Some(owner.emitters));
                if rows != owner.rows {
                    gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA32F as i32, 3, rows as i32, 0, glow::RGBA, glow::FLOAT, glow::PixelUnpackData::Slice(None));
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                    owner.rows = rows;
                    stats.gl_calls += 5;
                }
                let written = written.min(rows);
                if written > 0 {
                    // Rows as they are: whatever three's last upload left
                    // these at would flip or premultiply them.
                    gl.pixel_store_bool(UNPACK_FLIP_Y_WEBGL, false);
                    gl.pixel_store_bool(UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                    gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, 3, written as i32, glow::RGBA, glow::FLOAT,
                        glow::PixelUnpackData::Slice(Some(bytes(&self.data[at..at + written * EMITTER_FLOATS]))));
                    stats.gl_calls += 4;
                }
            }
        }
    }
}

const UNPACK_FLIP_Y_WEBGL: u32 = 0x9240;
const UNPACK_PREMULTIPLY_ALPHA_WEBGL: u32 = 0x9241;
