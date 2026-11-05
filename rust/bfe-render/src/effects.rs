// The battle's effects, drawn in the near scene's transparent list; their
// simulation stays in the page. Two kinds of item, each drawn with the
// program three compiled for it and three's GL state for its material:
//
// - Particle batches: particles.js's (bolts and their glow, sparks, smoke,
//   the effects' particles worked out on the CPU) and effects-gpu.js's
//   analytic ones (particles placed in the vertex shader from their spawn
//   values and their effect's emitter). Each is one instanced draw of
//   quads. The page writes each shown batch's instances into `data` (a CPU
//   batch's all, an analytic batch's from the first that changed),
//   attribute by attribute as the batch keeps them (or interleaved, for one
//   that keeps them so: the Enhanced sniper's); here they are interleaved
//   and uploaded with one call into a vertex buffer of the batch's own,
//   which its vertex array reads. An analytic batch's owner
//   (effects-gpu.js GpuEffects: its tracks, and the frame's emitters, a row
//   an effect) is a texture of three's for the tracks and one of the
//   renderer's for the emitters, a row of which the page writes for each
//   effect playing.
// - Dynamic meshes (the Classic arc caster's lightning, lightning.js):
//   vertices and indices the page rebuilds every frame, uploaded as far as
//   they are used (three uploads its whole buffers), drawn in each of
//   three's passes for it (a double-sided transparent material: back faces,
//   then front), its material's values set as three sets them.
// - Models (the muzzle flashes, the effects' model particles: game.js
//   glowModel; the droidekas' shields, droideka-shield.js): a registered
//   geometry drawn with a world matrix the page writes each frame (the
//   model-view matrix, and the normal matrix where the program reads one,
//   worked out here as three works them out), culled by its bounding sphere
//   as three culls it, in each of three's passes, with its material's
//   values. One model serves every object of the same geometry, programs,
//   texture and state.
//
// The page lists the frame's draws (src/rust-render/effects.js), each with
// the key three would have sorted it by in its transparent list; each is an
// item of the transparent pass's list (transparent.rs Source), drawn where
// three would have drawn it among everything else that blends.
use crate::core::{bytes, Core, GlState, MultiDraw, Stats, Value, PASS_EFFECTS, UNIT_MAP};
use crate::math::{self, Mat4};
use crate::transparent::{three_z, Item, Source, View};
use glow::HasContext;
use std::collections::HashMap;

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
// A draw: its kind, then
//   a batch (KIND_BATCH): batch, instances, first written, where in `data`, capacity;
//   a mesh (KIND_MESH): mesh, vertices, indices, where in `data` (its
//   attributes, then its indices' bits, then its material values);
//   a model (KIND_MODEL): model, where its world matrix is in `matrices`,
//   where its material values are in `data`;
// then three's sort key for it (DRAW_KEY: group order, render order, the
// centre of its bounding sphere through its world matrix, its id).
pub const DRAW_KEY: usize = 6;
pub const DRAW_STRIDE: usize = 12;
pub const KIND_BATCH: usize = 0;
pub const KIND_MESH: usize = 1;
pub const KIND_MODEL: usize = 2;
pub const MAX_DRAWS: usize = 512;
pub const INPUT_LEN: usize = DRAW_AT + MAX_DRAWS * DRAW_STRIDE;
// Floats of data a frame, to start with (the page asks for more).
pub const DATA_LEN: usize = 1 << 18;
// The models' world matrices a frame (f64, 16 each).
pub const MATRICES_LEN: usize = MAX_DRAWS * 16;
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
    // The instance attributes' sizes (floats), as the page writes them
    // (none: a batch whose program finds its instances by their number).
    sizes: Vec<usize>,
    stride: usize,
    owner: Option<usize>,
    // Where the program keeps NAMES (its material values), if it reads them.
    slots: [Option<usize>; 5],
    // Its own material values (batch_values), written after its instances,
    // and where its program keeps them.
    values: Vec<MaterialValue>,
    value_slots: Vec<Option<C::UniformLocation>>,
    // Textures on units of their own (batch_sampler), and 3D ones
    // (batch_sampler_3d: the jetpack flame's noise).
    samplers: Vec<(u32, Option<C::Texture>)>,
    samplers_3d: Vec<(u32, Option<C::Texture>)>,
    // Indices an instance draws: the corner quad's 6, or its own base's.
    index_count: i32,
    // Its instances come interleaved already (batch_interleaved).
    interleaved: bool,
    // The side three draws it with (batch_side; 2 both, a particle's).
    side: u32,
    // Its depth test, function and write when not a particle's (tested
    // less-or-equal, not written): batch_depth.
    depth: Option<(bool, u32, bool)>,
}

struct Owner<C: HasContext> {
    tracks: C::Texture,
    track_rows: f32,
    emitters: C::Texture,
    rows: usize,
}

// One of three's passes of a mesh: its program for the side drawn (0 front,
// 1 back, 2 both), a vertex array at that program's locations, and where
// the program keeps each of the mesh's material values.
struct MeshPass<C: HasContext> {
    program: usize,
    side: u32,
    vao: C::VertexArray,
    slots: Vec<Option<C::UniformLocation>>,
    // modelMatrix, which three's own materials read under shadows (its
    // world position): the identity, a mesh's vertices being in the world.
    model: Option<C::UniformLocation>,
}

struct Mesh<C: HasContext> {
    passes: Vec<MeshPass<C>>,
    texture: Option<C::Texture>,
    blend: Blend,
    // Depth test, its function, depth write.
    depth: (bool, u32, bool),
    vbo: C::Buffer,
    ebo: C::Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
    sizes: Vec<usize>,
    stride: usize,
    // The material values, in the order the page writes them.
    values: Vec<MaterialValue>,
}

// One of three's passes of a model: its program for the side drawn, the
// geometry (core.rs) at that program's locations, where the program keeps
// its model-view matrix and each of the model's material values.
struct ModelPass<C: HasContext> {
    program: usize,
    side: u32,
    geometry: usize,
    model_view: Option<C::UniformLocation>,
    normal: Option<C::UniformLocation>,
    slots: Vec<Option<C::UniformLocation>>,
    // modelMatrix, which three's own materials read under shadows (its
    // world position): the model's world matrix, set per draw.
    model: Option<C::UniformLocation>,
}

struct Model<C: HasContext> {
    passes: Vec<ModelPass<C>>,
    texture: Option<C::Texture>,
    blend: Blend,
    depth: (bool, u32, bool),
    values: Vec<MaterialValue>,
    // Its geometry's bounding sphere (object space): centre, radius (below
    // 0: never culled, as three draws an object it does not cull).
    sphere: ([f64; 3], f64),
    // Textures on units of their own besides its map (model_sampler).
    samplers: Vec<(u32, Option<C::Texture>)>,
}

pub struct EffectsPass<C: HasContext> {
    quad: C::Buffer,
    quad_index: C::Buffer,
    batches: Vec<Batch<C>>,
    owners: Vec<Owner<C>>,
    meshes: Vec<Mesh<C>>,
    models: Vec<Model<C>>,
    // What the meshes' and models' programs hold of their material values
    // (by program and name).
    held: HashMap<(usize, String), Vec<f32>>,
    pub input: Vec<f64>,
    pub data: Vec<f32>,
    pub matrices: Vec<f64>,
    staging: Vec<f32>,
}

// add_batch's corner for a batch with a base of its own (batch_base).
pub const NO_CORNER: u32 = u32::MAX;

// The quad every batch draws (particles.js QUAD, QUAD_INDEX).
const QUAD: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0];
const QUAD_INDEX: [u16; 6] = [0, 1, 2, 0, 2, 3];

const UNPACK_FLIP_Y_WEBGL: u32 = 0x9240;
const UNPACK_PREMULTIPLY_ALPHA_WEBGL: u32 = 0x9241;

// A draw's culling and winding as three's setMaterial makes them: side 0
// front (back faces culled), 1 back (three culls back faces with clockwise
// front faces), 2 both (no culling); an object whose world matrix mirrors
// winds the other way (three's frontFaceCW).
fn sides<C: HasContext>(gl: &C, state: &mut GlState<C>, stats: &mut Stats, side: u32, mirrored: bool) {
    state.cull(gl, stats, if side == 2 { 2 } else { 0 });
    state.front_face(gl, stats, if (side == 1) != mirrored { glow::CW } else { glow::CCW });
}

// A material value: its name, its size in floats and its form (0 a float
// or vector of that size, or a matrix of 9 or 16; 1 an array of floats; 2
// an int; 4 an array of vec4s), as the page names them ("name:size" or
// "name:size:form", comma-separated).
pub type MaterialValue = (String, usize, u32);

// The identity, as a mesh's modelMatrix (its vertices in the world).
const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn material_values(values: &str) -> Vec<MaterialValue> {
    values.split(',').filter(|s| !s.is_empty()).filter_map(|s| {
        let mut parts = s.split(':');
        let name = parts.next()?.to_string();
        let size = parts.next()?.parse().ok()?;
        let form = parts.next().and_then(|f| f.parse().ok()).unwrap_or(0);
        Some((name, size, form))
    }).collect()
}

// A material value into a program, when it holds another (`held`, by
// program and name).
#[allow(clippy::too_many_arguments)]
fn set_value<C: HasContext>(gl: &C, stats: &mut Stats, held: &mut HashMap<(usize, String), Vec<f32>>, program: usize, value: &MaterialValue, location: Option<&C::UniformLocation>, values: &[f32]) {
    let Some(location) = location else { return };
    let held = held.entry((program, value.0.clone())).or_default();
    if held.as_slice() == values {
        return;
    }
    unsafe {
        match (value.2, values.len()) {
            (1, _) => gl.uniform_1_f32_slice(Some(location), values),
            (2, _) => gl.uniform_1_i32(Some(location), values[0] as i32),
            (4, _) => gl.uniform_4_f32_slice(Some(location), values),
            (_, 1) => gl.uniform_1_f32(Some(location), values[0]),
            (_, 2) => gl.uniform_2_f32(Some(location), values[0], values[1]),
            (_, 3) => gl.uniform_3_f32(Some(location), values[0], values[1], values[2]),
            (_, 4) => gl.uniform_4_f32(Some(location), values[0], values[1], values[2], values[3]),
            (_, 9) => gl.uniform_matrix_3_f32_slice(Some(location), false, values),
            (_, 16) => gl.uniform_matrix_4_f32_slice(Some(location), false, values),
            _ => return,
        }
    }
    held.clear();
    held.extend_from_slice(values);
    stats.gl_calls += 1;
}

fn blending<C: HasContext>(gl: &C, state: &mut GlState<C>, stats: &mut Stats, blend: &Blend) {
    state.blend(gl, stats, blend[0] != 0);
    if blend[0] != 0 {
        state.blend_equation(gl, stats, blend[1], blend[2]);
        state.blend_func(gl, stats, blend[3], blend[4], blend[5], blend[6]);
    }
}

// Interleaves `n` items of planar attributes (sizes, one plane after
// another from `data[at..]`) into `out`, `stride` floats an item.
fn interleave(out: &mut Vec<f32>, data: &[f32], at: usize, n: usize, sizes: &[usize], stride: usize) {
    out.clear();
    out.resize(n * stride, 0.0);
    let (mut src, mut offset) = (at, 0);
    for &size in sizes {
        let plane = &data[src..src + n * size];
        for i in 0..n {
            out[i * stride + offset..i * stride + offset + size].copy_from_slice(&plane[i * size..(i + 1) * size]);
        }
        src += n * size;
        offset += size;
    }
}

impl<C: HasContext> EffectsPass<C> {
    pub fn replace_texture(&mut self, old: C::Texture, new: C::Texture) {
        for owner in &mut self.owners { if owner.tracks == old { owner.tracks = new; } }
        for batch in &mut self.batches {
            if batch.texture == Some(old) { batch.texture = Some(new); }
            for (_, texture) in batch.samplers.iter_mut().chain(batch.samplers_3d.iter_mut()) { if *texture == Some(old) { *texture = Some(new); } }
        }
        for mesh in &mut self.meshes { if mesh.texture == Some(old) { mesh.texture = Some(new); } }
        for model in &mut self.models {
            if model.texture == Some(old) { model.texture = Some(new); }
            for (_, texture) in &mut model.samplers { if *texture == Some(old) { *texture = Some(new); } }
        }
    }

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
        EffectsPass {
            quad, quad_index, batches: vec![], owners: vec![], meshes: vec![], models: vec![], held: HashMap::new(),
            input: vec![0.0; INPUT_LEN], data: vec![0.0; DATA_LEN], matrices: vec![0.0; MATRICES_LEN], staging: vec![],
        }
    }

    // Room for `floats` of data a frame.
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
    #[allow(clippy::too_many_arguments)]
    pub fn add_batch<M: MultiDraw>(&mut self, core: &Core<C, M>, program: u32, texture: i32, blend: &[u32], corner: u32, attributes: &[u32], owner: i32) -> u32 {
        let gl = &core.gl;
        let sizes: Vec<usize> = attributes.chunks(2).map(|a| a[1] as usize).collect();
        let stride: usize = sizes.iter().sum();
        let (vao, vbo) = unsafe {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            // The corner quad (none: a base of its own follows, batch_base).
            if corner != NO_CORNER {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.quad));
                gl.enable_vertex_attrib_array(corner);
                gl.vertex_attrib_pointer_f32(corner, 2, glow::FLOAT, false, 0, 0);
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.quad_index));
            }
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
            values: vec![], value_slots: vec![], samplers: vec![], samplers_3d: vec![], index_count: 6, interleaved: false, side: 2, depth: None,
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

    // A batch's base, for one drawn without the corner quad: its vertex
    // attributes (location, size pairs) and their data (attribute after
    // attribute, `vertices` each), and its indices (16 bits).
    pub fn batch_base<M: MultiDraw>(&mut self, core: &Core<C, M>, batch: u32, attributes: &[u32], vertices: u32, data: &[f32], index: &[u16]) {
        let Some(b) = self.batches.get_mut(batch as usize) else { return };
        let gl = &core.gl;
        let sizes: Vec<usize> = attributes.chunks(2).map(|a| a[1] as usize).collect();
        let stride: usize = sizes.iter().sum();
        let mut interleaved = vec![];
        interleave(&mut interleaved, data, 0, vertices as usize, &sizes, stride);
        unsafe {
            gl.bind_vertex_array(Some(b.vao));
            let vbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&interleaved), glow::STATIC_DRAW);
            let mut offset = 0;
            for a in attributes.chunks(2) {
                gl.enable_vertex_attrib_array(a[0]);
                gl.vertex_attrib_pointer_f32(a[0], a[1] as i32, glow::FLOAT, false, (stride * 4) as i32, (offset * 4) as i32);
                offset += a[1] as usize;
            }
            let ebo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(index), glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        b.index_count = index.len() as i32;
    }

    // A batch whose instances the page writes interleaved, as it keeps them
    // (one buffer of all its attributes, in add_batch's order).
    pub fn batch_interleaved(&mut self, batch: u32) {
        if let Some(b) = self.batches.get_mut(batch as usize) {
            b.interleaved = true;
        }
    }

    // The side a batch is drawn with, for one three draws single-sided (0
    // front, 1 back; a particle's is 2, both).
    pub fn batch_side(&mut self, batch: u32, side: u32) {
        if let Some(b) = self.batches.get_mut(batch as usize) {
            b.side = side;
        }
    }

    // A batch's depth state when it isn't a particle's: flags (1 test, 2
    // write) and function (the steam's volumes: neither tested nor written).
    pub fn batch_depth(&mut self, batch: u32, flags: u32, func: u32) {
        if let Some(b) = self.batches.get_mut(batch as usize) {
            b.depth = Some((flags & 1 != 0, func, flags & 2 != 0));
        }
    }

    // A batch drawn into a texture of its own rather than the scene (the
    // steam's volumes, into their half-size target, cleared first): its
    // `count` instances and its values as the page wrote them from `at` of
    // the data, `capacity` the batch's room, `depth` the scene's depth
    // (registered texture, -1 none), the frame's soft values as the page
    // wrote them. Leaves the renderer's framebuffer over that texture bound
    // and the viewport its size: the page has three bind its own again.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_into<M: MultiDraw>(&mut self, core: &mut Core<C, M>, batch: usize, target: u32, width: i32, height: i32, count: usize, at: usize, capacity: usize) {
        let Ok(fb) = core.framebuffer(Some(target), None) else { return };
        core.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        unsafe {
            core.gl.viewport(0, 0, width, height);
            core.gl.clear_color(0.0, 0.0, 0.0, 0.0);
            core.state.color_mask(&core.gl, &mut core.stats, [true; 4]);
            core.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        core.stats.gl_calls += 3;
        self.common(core);
        self.draw_batch(core, [batch, count, 0, at, capacity]);
    }

    // A batch's own material values ("name:size" or "name:size:form",
    // comma-separated: form 2 an int), which the page writes after its
    // instances each frame.
    pub fn batch_values<M: MultiDraw>(&mut self, core: &Core<C, M>, batch: u32, values: &str) {
        let Some(b) = self.batches.get_mut(batch as usize) else { return };
        let p = core.programs[b.program].program;
        b.values = material_values(values);
        b.value_slots = b.values.iter().map(|v| unsafe { core.gl.get_uniform_location(p, &v.0) }).collect();
    }

    // A texture a batch's program samples by `name`, on `unit` (its sampler
    // set there; every batch of the program has it there).
    pub fn batch_sampler<M: MultiDraw>(&mut self, core: &Core<C, M>, batch: u32, name: &str, unit: u32, texture: i32) {
        let Some(b) = self.batches.get_mut(batch as usize) else { return };
        let gl = &core.gl;
        let p = core.programs[b.program].program;
        unsafe {
            if let Some(l) = gl.get_uniform_location(p, name) {
                gl.use_program(Some(p));
                gl.uniform_1_i32(Some(&l), unit as i32);
                gl.use_program(None);
            }
        }
        let t = if texture < 0 { None } else { core.textures.get(texture as usize).copied() };
        match b.samplers.iter_mut().find(|(u, _)| *u == unit) {
            Some(entry) => entry.1 = t,
            None => b.samplers.push((unit, t)),
        }
    }

    // A 3D texture a batch's program samples by `name`, on `unit`: as
    // batch_sampler, bound as a 3D texture.
    pub fn batch_sampler_3d<M: MultiDraw>(&mut self, core: &Core<C, M>, batch: u32, name: &str, unit: u32, texture: i32) {
        let Some(b) = self.batches.get_mut(batch as usize) else { return };
        let gl = &core.gl;
        let p = core.programs[b.program].program;
        unsafe {
            if let Some(l) = gl.get_uniform_location(p, name) {
                gl.use_program(Some(p));
                gl.uniform_1_i32(Some(&l), unit as i32);
                gl.use_program(None);
            }
        }
        let t = if texture < 0 { None } else { core.textures.get(texture as usize).copied() };
        match b.samplers_3d.iter_mut().find(|(u, _)| *u == unit) {
            Some(entry) => entry.1 = t,
            None => b.samplers_3d.push((unit, t)),
        }
    }

    // A batch's texture again (three made it anew).
    pub fn batch_texture<M: MultiDraw>(&mut self, core: &Core<C, M>, batch: u32, texture: i32) {
        if let Some(b) = self.batches.get_mut(batch as usize) {
            b.texture = if texture < 0 { None } else { core.textures.get(texture as usize).copied() };
        }
    }

    // A dynamic mesh: its texture (-1 none), blending, depth (flags: 1 test,
    // 2 write; and its function), its vertex attributes' sizes and its
    // material values (name:size, comma-separated), both in the order the
    // page writes them. Its passes follow (mesh_pass).
    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh<M: MultiDraw>(&mut self, core: &Core<C, M>, texture: i32, blend: &[u32], depth_flags: u32, depth_func: u32, sizes: &[u32], values: &str) -> u32 {
        let gl = &core.gl;
        let (vbo, ebo) = unsafe { (gl.create_buffer().unwrap(), gl.create_buffer().unwrap()) };
        let mut b = [0u32; 7];
        b.copy_from_slice(&blend[..7]);
        let sizes: Vec<usize> = sizes.iter().map(|&s| s as usize).collect();
        let values = material_values(values);
        self.meshes.push(Mesh {
            passes: vec![], texture: if texture < 0 { None } else { Some(core.textures[texture as usize]) }, blend: b,
            depth: (depth_flags & 1 != 0, depth_func, depth_flags & 2 != 0), vbo, ebo, vertex_capacity: 0, index_capacity: 0,
            stride: sizes.iter().sum(), sizes, values,
        });
        (self.meshes.len() - 1) as u32
    }

    // A mesh's texture again (its material changed, or three made it anew).
    pub fn mesh_texture<M: MultiDraw>(&mut self, core: &Core<C, M>, mesh: u32, texture: i32) {
        if let Some(m) = self.meshes.get_mut(mesh as usize) {
            m.texture = if texture < 0 { None } else { core.textures.get(texture as usize).copied() };
        }
    }

    // One of three's passes of a mesh, in drawing order: the program for its
    // side, and the locations that program reads the mesh's attributes at
    // (in add_mesh's order).
    pub fn mesh_pass<M: MultiDraw>(&mut self, core: &Core<C, M>, mesh: u32, program: u32, side: u32, locations: &[u32]) -> u32 {
        let gl = &core.gl;
        let m = &mut self.meshes[mesh as usize];
        let p = core.programs[program as usize].program;
        let vao = unsafe {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(m.vbo));
            let mut offset = 0;
            for (k, &size) in m.sizes.iter().enumerate() {
                if let Some(&l) = locations.get(k) {
                    gl.enable_vertex_attrib_array(l);
                    gl.vertex_attrib_pointer_f32(l, size as i32, glow::FLOAT, false, (m.stride * 4) as i32, (offset * 4) as i32);
                }
                offset += size;
            }
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(m.ebo));
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            vao
        };
        let slots = m.values.iter().map(|v| unsafe { gl.get_uniform_location(p, &v.0) }).collect();
        let model = unsafe { gl.get_uniform_location(p, "modelMatrix") };
        m.passes.push(MeshPass { program: program as usize, side, vao, slots, model });
        (m.passes.len() - 1) as u32
    }

    // A model: its texture (-1 none), blending, depth (flags: 1 test, 2
    // write; and its function), its material values (name:size,
    // comma-separated, in the order the page writes them) and its geometry's
    // bounding sphere (centre, radius). Its passes follow (model_pass).
    #[allow(clippy::too_many_arguments)]
    pub fn add_model<M: MultiDraw>(&mut self, core: &Core<C, M>, texture: i32, blend: &[u32], depth_flags: u32, depth_func: u32, values: &str, sphere: &[f64]) -> u32 {
        let mut b = [0u32; 7];
        b.copy_from_slice(&blend[..7]);
        let values = material_values(values);
        self.models.push(Model {
            passes: vec![], texture: if texture < 0 { None } else { Some(core.textures[texture as usize]) }, blend: b,
            depth: (depth_flags & 1 != 0, depth_func, depth_flags & 2 != 0), values, sphere: ([sphere[0], sphere[1], sphere[2]], sphere[3]),
            samplers: vec![],
        });
        (self.models.len() - 1) as u32
    }

    // A model's texture again (three made it anew).
    pub fn model_texture<M: MultiDraw>(&mut self, core: &Core<C, M>, model: u32, texture: i32) {
        if let Some(m) = self.models.get_mut(model as usize) {
            m.texture = if texture < 0 { None } else { core.textures.get(texture as usize).copied() };
        }
    }

    // One of three's passes of a model, in drawing order: the program for its
    // side, and the geometry registered for that program (core.rs).
    pub fn model_pass<M: MultiDraw>(&mut self, core: &Core<C, M>, model: u32, program: u32, side: u32, geometry: u32) -> u32 {
        let gl = &core.gl;
        let m = &mut self.models[model as usize];
        let p = core.programs[program as usize].program;
        let (model_view, normal, model, slots) = unsafe {
            (gl.get_uniform_location(p, "modelViewMatrix"), gl.get_uniform_location(p, "normalMatrix"), gl.get_uniform_location(p, "modelMatrix"),
                m.values.iter().map(|v| gl.get_uniform_location(p, &v.0)).collect())
        };
        // The scene's depth (common) on its unit, for a program that reads it.
        unsafe {
            if let Some(l) = gl.get_uniform_location(p, "sceneDepth") {
                gl.use_program(Some(p));
                gl.uniform_1_i32(Some(&l), UNIT_SCENE_DEPTH as i32);
                gl.use_program(None);
            }
        }
        m.passes.push(ModelPass { program: program as usize, side, geometry: geometry as usize, model_view, normal, slots, model });
        (m.passes.len() - 1) as u32
    }

    // A texture a model's programs sample by `name` besides its map, on
    // `unit` (their sampler set there), after its passes; again when three
    // made it anew.
    pub fn model_sampler<M: MultiDraw>(&mut self, core: &Core<C, M>, model: u32, name: &str, unit: u32, texture: i32) {
        let Some(m) = self.models.get_mut(model as usize) else { return };
        let gl = &core.gl;
        for pass in &m.passes {
            let p = core.programs[pass.program].program;
            unsafe {
                if let Some(l) = gl.get_uniform_location(p, name) {
                    gl.use_program(Some(p));
                    gl.uniform_1_i32(Some(&l), unit as i32);
                    gl.use_program(None);
                }
            }
        }
        let t = if texture < 0 { None } else { core.textures.get(texture as usize).copied() };
        match m.samplers.iter_mut().find(|(u, _)| *u == unit) {
            Some(entry) => entry.1 = t,
            None => m.samplers.push((unit, t)),
        }
    }

    // What every effect's draw leaves alike, set at the start of a run (three
    // drew in between): every channel written, no stencil, polygon offset or
    // alpha to coverage; the scene's depth on its unit when the page took it
    // this frame (soft particles, or an effect that reads it whatever the
    // setting), else nothing there (never read then).
    fn common<M: MultiDraw>(&self, core: &mut Core<C, M>) {
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        state.color_mask(gl, stats, [true; 4]);
        state.alpha_to_coverage(gl, stats, false);
        unsafe {
            gl.disable(glow::STENCIL_TEST);
            gl.disable(glow::POLYGON_OFFSET_FILL);
        }
        stats.gl_calls += 2;
        let depth = self.input[IN_SOFT + 5];
        let depth = if depth >= 0.0 { core.textures.get(depth as usize).copied() } else { None };
        state.bind_texture(gl, stats, UNIT_SCENE_DEPTH, depth);
    }

    // The frame's draw `d` as the page wrote it.
    fn entry(&self, d: usize) -> (usize, [usize; 5]) {
        let at = DRAW_AT + d * DRAW_STRIDE;
        let mut e = [0usize; 5];
        for (k, v) in e.iter_mut().enumerate() {
            *v = self.input[at + 1 + k] as usize;
        }
        (self.input[at] as usize, e)
    }

    // A batch: its instances written this frame, interleaved, into its
    // buffer (made again when the batch's room grew), then one draw.
    fn draw_batch<M: MultiDraw>(&mut self, core: &mut Core<C, M>, e: [usize; 5]) {
        let [bi, count, from, at, capacity] = e;
        let Some(b) = self.batches.get_mut(bi) else { return };
        if count == 0 {
            return;
        }
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        unsafe {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(b.vbo));
            stats.gl_calls += 1;
            if capacity != b.capacity {
                gl.buffer_data_size(glow::ARRAY_BUFFER, (capacity.max(count) * b.stride * 4) as i32, glow::DYNAMIC_DRAW);
                b.capacity = capacity.max(count);
                stats.gl_calls += 1;
            }
            if from < count && b.stride > 0 {
                let n = count - from;
                if b.interleaved {
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, (from * b.stride * 4) as i32, bytes(&self.data[at..at + n * b.stride]));
                } else {
                    interleave(&mut self.staging, &self.data, at, n, &b.sizes, b.stride);
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, (from * b.stride * 4) as i32, bytes(&self.staging));
                }
                stats.gl_calls += 1;
            }
            let program = &mut core.programs[b.program];
            state.use_program(gl, stats, program.program);
            // Its own values, after its instances.
            let mut v = at + (count - from.min(count)) * b.stride;
            for (k, value) in b.values.iter().enumerate() {
                set_value(gl, stats, &mut self.held, b.program, value, b.value_slots[k].as_ref(), &self.data[v..v + value.1]);
                v += value.1;
            }
            for &(unit, t) in &b.samplers {
                state.bind_texture(gl, stats, unit, t);
            }
            for &(unit, t) in &b.samplers_3d {
                state.bind_texture_3d(gl, stats, unit, t);
            }
            // The frame's soft particle values, and the owner's rows.
            let input = &self.input;
            let owner = b.owner.map(|o| &self.owners[o]);
            let f = |k: usize| input[IN_SOFT + k] as f32;
            let values: [Option<Value>; 5] = [
                Some(Value::F(1, [f(0), 0.0, 0.0, 0.0])),
                Some(Value::F(2, [f(1), f(2), 0.0, 0.0])),
                Some(Value::F(2, [f(3), f(4), 0.0, 0.0])),
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
            // A particle material: depth tested less-or-equal and not
            // written, double-sided (or the side three draws it with), or
            // the depth state it was given.
            sides(gl, state, stats, b.side, false);
            let (test, func, write) = b.depth.unwrap_or((true, glow::LEQUAL, false));
            state.depth_test(gl, stats, test);
            state.depth_func(gl, stats, func);
            state.depth_write(gl, stats, write);
            blending(gl, state, stats, &b.blend);
            gl.bind_vertex_array(Some(b.vao));
            gl.draw_elements_instanced(glow::TRIANGLES, b.index_count, glow::UNSIGNED_SHORT, 0, count as i32);
            stats.gl_calls += 2;
            stats.pass_draws[PASS_EFFECTS] += 1;
        }
    }

    // A mesh: its vertices and indices uploaded as far as they are used,
    // then each of its passes drawn.
    fn draw_mesh<M: MultiDraw>(&mut self, core: &mut Core<C, M>, e: [usize; 5]) {
        let [mi, vertices, indices, at, _] = e;
        let Some(m) = self.meshes.get_mut(mi) else { return };
        if indices == 0 || vertices == 0 || m.passes.is_empty() {
            return;
        }
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let index_at = at + vertices * m.stride;
        let values_at = index_at + indices;
        unsafe {
            interleave(&mut self.staging, &self.data, at, vertices, &m.sizes, m.stride);
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(m.vbo));
            if vertices > m.vertex_capacity {
                m.vertex_capacity = vertices.next_power_of_two();
                gl.buffer_data_size(glow::ARRAY_BUFFER, (m.vertex_capacity * m.stride * 4) as i32, glow::DYNAMIC_DRAW);
                stats.gl_calls += 1;
            }
            gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes(&self.staging));
            // The indices through the first pass's vertex array, whose
            // element buffer they are.
            gl.bind_vertex_array(Some(m.passes[0].vao));
            if indices > m.index_capacity {
                m.index_capacity = indices.next_power_of_two();
                gl.buffer_data_size(glow::ELEMENT_ARRAY_BUFFER, (m.index_capacity * 4) as i32, glow::DYNAMIC_DRAW);
                stats.gl_calls += 1;
            }
            gl.buffer_sub_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, 0, bytes(&self.data[index_at..index_at + indices]));
            stats.gl_calls += 4;
            for pass in &m.passes {
                let program = &core.programs[pass.program];
                state.use_program(gl, stats, program.program);
                if let Some(l) = pass.model.as_ref() {
                    set_value(gl, stats, &mut self.held, pass.program, &("modelMatrix".to_string(), 16, 0), Some(l), &IDENTITY);
                }
                // The material's values, each set when the program holds another.
                let mut v = values_at;
                for (k, value) in m.values.iter().enumerate() {
                    set_value(gl, stats, &mut self.held, pass.program, value, pass.slots[k].as_ref(), &self.data[v..v + value.1]);
                    v += value.1;
                }
                if m.texture.is_some() {
                    state.bind_texture(gl, stats, UNIT_MAP, m.texture);
                }
                sides(gl, state, stats, pass.side, false);
                state.depth_test(gl, stats, m.depth.0);
                state.depth_func(gl, stats, m.depth.1);
                state.depth_write(gl, stats, m.depth.2);
                blending(gl, state, stats, &m.blend);
                gl.bind_vertex_array(Some(pass.vao));
                gl.draw_elements(glow::TRIANGLES, indices as i32, glow::UNSIGNED_INT, 0);
                stats.gl_calls += 2;
                stats.pass_draws[PASS_EFFECTS] += 1;
            }
        }
    }

    // A model with its world matrix: the model-view matrix as three makes
    // it (the camera's inverse world times the object's, in doubles), then
    // each of its passes.
    fn draw_model<M: MultiDraw>(&mut self, core: &mut Core<C, M>, view: &View, e: [usize; 5]) {
        let [mi, matrix_at, values_at, _, _] = e;
        let Some(m) = self.models.get(mi) else { return };
        let mut world: Mat4 = [0.0; 16];
        world.copy_from_slice(&self.matrices[matrix_at..matrix_at + 16]);
        let model_view = math::mul(&view.camera.view, &world);
        let mut mv = [0.0f32; 16];
        for (k, v) in mv.iter_mut().enumerate() {
            *v = model_view[k] as f32;
        }
        let mirrored = math::mirrors(&world);
        // three's normalMatrix, for the programs that read one.
        let mut normal = [0.0f32; 9];
        if m.passes.iter().any(|p| p.normal.is_some()) {
            for (k, v) in math::normal_matrix(&model_view).iter().enumerate() {
                normal[k] = *v as f32;
            }
        }
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        for pass in &m.passes {
            let program = &core.programs[pass.program];
            state.use_program(gl, stats, program.program);
            if let Some(l) = pass.model_view.as_ref() {
                unsafe { gl.uniform_matrix_4_f32_slice(Some(l), false, &mv) };
                stats.gl_calls += 1;
            }
            if let Some(l) = pass.normal.as_ref() {
                unsafe { gl.uniform_matrix_3_f32_slice(Some(l), false, &normal) };
                stats.gl_calls += 1;
            }
            if let Some(l) = pass.model.as_ref() {
                let w: [f32; 16] = std::array::from_fn(|k| world[k] as f32);
                unsafe { gl.uniform_matrix_4_f32_slice(Some(l), false, &w) };
                stats.gl_calls += 1;
            }
            let mut v = values_at;
            for (k, value) in m.values.iter().enumerate() {
                set_value(gl, stats, &mut self.held, pass.program, value, pass.slots[k].as_ref(), &self.data[v..v + value.1]);
                v += value.1;
            }
            if m.texture.is_some() {
                state.bind_texture(gl, stats, UNIT_MAP, m.texture);
            }
            for &(unit, t) in &m.samplers {
                state.bind_texture(gl, stats, unit, t);
            }
            sides(gl, state, stats, pass.side, mirrored);
            state.depth_test(gl, stats, m.depth.0);
            state.depth_func(gl, stats, m.depth.1);
            state.depth_write(gl, stats, m.depth.2);
            blending(gl, state, stats, &m.blend);
            let geo = &core.geometries[pass.geometry];
            unsafe {
                gl.bind_vertex_array(Some(geo.vao));
                gl.draw_elements(glow::TRIANGLES, geo.index_count, geo.index_type, 0);
            }
            stats.gl_calls += 2;
            stats.pass_draws[PASS_EFFECTS] += 1;
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

// The effects as a source of the transparent list's items: each of the
// frame's draws one item, with the key the page wrote for it, z worked out
// as three works it out; the owners' emitters uploaded as the list is made.
impl<C: HasContext, M: MultiDraw> Source<C, M> for EffectsPass<C> {
    fn emit(&mut self, core: &mut Core<C, M>, view: &View, source: u32, items: &mut Vec<Item>) {
        let draws = (self.input[IN_DRAWS] as usize).min(MAX_DRAWS);
        if draws == 0 {
            return;
        }
        self.upload_owners(core);
        for d in 0..draws {
            // A model three culls by its bounding sphere, through its world
            // matrix (not one it does not cull: a radius below 0).
            if let (KIND_MODEL, e) = self.entry(d) {
                let Some(m) = self.models.get(e[0]) else { continue };
                if m.sphere.1 >= 0.0 {
                    let mut world: Mat4 = [0.0; 16];
                    world.copy_from_slice(&self.matrices[e[1]..e[1] + 16]);
                    let (center, radius) = math::sphere_to_world(&world, &m.sphere.0, m.sphere.1);
                    if !view.camera.frustum.sphere(&center, radius) {
                        continue;
                    }
                }
            }
            let k = &self.input[DRAW_AT + d * DRAW_STRIDE + DRAW_KEY..DRAW_AT + (d + 1) * DRAW_STRIDE];
            // One batch for all: consecutive effects in the list are one
            // run (draw_run draws its items in turn), set up once.
            items.push(Item {
                group_order: k[0], render_order: k[1], z: three_z(&view.view_projection, None, &[k[2], k[3], k[4]]), id: k[5],
                source, batch: 0, record: d as u32,
            });
        }
    }

    fn draw_run(&mut self, core: &mut Core<C, M>, view: &View, run: &[Item]) {
        self.common(core);
        for item in run {
            match self.entry(item.record as usize) {
                (KIND_BATCH, e) => self.draw_batch(core, e),
                (KIND_MESH, e) => self.draw_mesh(core, e),
                (KIND_MODEL, e) => self.draw_model(core, view, e),
                _ => {}
            }
        }
    }
}
