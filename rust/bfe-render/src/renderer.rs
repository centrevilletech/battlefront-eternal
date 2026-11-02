// The battle renderer: the near scene's static world and its soldiers (their
// bodies and held guns), drawn with few GL calls:
//
// - one uniform-block upload a frame for everything the frame shares;
// - the map's buckets culled by spatial cell as render-culling.js does, the
//   visible runs drawn by one multi-draw a bucket, in program order;
// - every visible soldier's body part (and gun) drawn instanced, a draw a
//   model, texture and program, its per-object values in a texture row and
//   its bones in one texture holding every drawn soldier's.
//
// The page writes the frame's inputs (camera, lights, each soldier) into
// `input` and `bones` before `frame` (web.rs, src/rust-render.js).
use crate::math::{self, Frustum, Mat4};
use crate::shader::{self, Kind, Stage, FRAME_FLOATS, FRAME_LIGHTS, FRAME_LIGHT_COUNT, MAX_LIGHTS, ROW_TEXELS};
use glow::HasContext;
use std::collections::HashMap;

// A soldier's record in `input`, after the frame block's values (f64 slots).
pub const IN_RECORDS: usize = FRAME_FLOATS;
pub const REC_KIND: usize = 0; // 1: skinned body, 2: rigid mesh
pub const REC_PROGRAM: usize = 1;
pub const REC_GEOMETRY: usize = 2;
pub const REC_GROUPS: usize = 3; // groups drawn; 0 draws the whole index
pub const REC_TEXTURES: usize = 4; // a texture a group (4 slots)
pub const REC_BONES: usize = 8;
pub const REC_BONE_AT: usize = 9; // first matrix in `bones`
pub const REC_SPHERE: usize = 10; // object space: centre, radius
pub const REC_BOX: usize = 14; // object space: min, max (the light mask of a rigid mesh)
pub const REC_WORLD: usize = 20; // matrixWorld
pub const REC_BIND_INVERSE: usize = 36;
pub const REC_LOOK: usize = 52; // sunScale, ambientPull, brightness
pub const REC_FLAGS: usize = 56; // cull side (0 front, 1 back, 2 both) | 4 alpha to coverage | 8 depth write
pub const REC_BIND: usize = 60; // bindMatrix (skinned)
pub const REC_STRIDE: usize = 76;
pub const MAX_RECORDS: usize = 256;
pub const MAX_ROWS: usize = 512;
pub const BONE_TEXELS: usize = 1024; // a bone texture row
pub const BONE_ROWS: usize = 64;
pub const MAX_BONES: usize = BONE_TEXELS * BONE_ROWS / 4;

// Fixed texture units.
pub const UNIT_MAP: u32 = 0;
pub const UNIT_FAR: u32 = 1;
pub const UNIT_BUMP: u32 = 2;
pub const UNIT_BONES: u32 = 14;
pub const UNIT_INSTANCES: u32 = 15;
const SAMPLERS: &[(&str, u32)] = &[("map", UNIT_MAP), ("farMap", UNIT_FAR), ("bumpMap", UNIT_BUMP), ("boneTexture", UNIT_BONES), ("bfeInstances", UNIT_INSTANCES)];

// The multi-draw call, which glow does not wrap (WEBGL_multi_draw on the web).
pub trait MultiDraw {
    fn multi_draw(&self, mode: u32, counts: &mut [i32], element_type: u32, offsets: &mut [i32]);
}

#[derive(Clone, Copy, PartialEq)]
enum Value {
    F(usize, [f32; 4]),
    I(i32),
}

struct Program<C: HasContext> {
    program: C::Program,
    // Uniforms set from a draw's material (light-region values...), each with
    // the value the program holds now.
    uniforms: Vec<(String, Option<C::UniformLocation>, Option<Value>)>,
    zero_lights: Option<C::UniformLocation>,
    zero_light_count: Option<C::UniformLocation>,
    instance_base: Option<C::UniformLocation>,
    lights: Vec<f32>,
    light_count: i32,
}

struct Geometry<C: HasContext> {
    vao: C::VertexArray,
    buffers: Vec<C::Buffer>,
    index_type: u32,
    index_bytes: i32,
    index_count: i32,
    groups: Vec<(i32, i32)>,
}

struct Cells {
    // Consecutive triangles in one cell: [part, first index, end].
    runs: Vec<[u32; 3]>,
    parts: Vec<([f64; 3], [f64; 3])>,
    visible: Vec<u8>,
    offsets: Vec<i32>,
    counts: Vec<i32>,
}

struct World<C: HasContext> {
    geometry: usize,
    program: usize,
    textures: Vec<(u32, C::Texture)>,
    uniforms: Vec<(usize, Value)>,
    cull: u32,
    alpha_to_coverage: bool,
    depth_write: bool,
    point_lit: bool,
    order: u32,
    sphere: ([f64; 3], f64),
    bbox: ([f64; 3], [f64; 3]),
    cells: Option<Cells>,
}

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub world_draws: u32,
    pub instance_draws: u32,
    pub instances: u32,
    pub program_switches: u32,
    pub gl_calls: u32,
}

pub struct Renderer<C: HasContext, M: MultiDraw> {
    pub gl: C,
    multi: M,
    programs: Vec<Program<C>>,
    geometries: Vec<Geometry<C>>,
    textures: Vec<C::Texture>,
    world: Vec<World<C>>,
    world_order: Vec<usize>,
    by_program: bool,
    far_map: Option<C::Texture>,
    frame_ubo: C::Buffer,
    frame: Vec<f32>,
    instance_texture: C::Texture,
    bone_texture: C::Texture,
    rows: Vec<f32>,
    bone_upload: Vec<f32>,
    pub input: Vec<f64>,
    pub bones: Vec<f32>,
    pub stats: Stats,
    // Draw instances this frame: (key, record, group).
    draws: Vec<([u32; 6], usize, usize)>,
    templates: Vec<[f32; ROW_TEXELS * 4]>,
    pub log: Vec<String>,
}

fn upload_texture<C: HasContext>(gl: &C, width: i32, height: i32) -> C::Texture {
    unsafe {
        let t = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA32F as i32, width, height, 0, glow::RGBA, glow::FLOAT, glow::PixelUnpackData::Slice(None));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        t
    }
}

fn bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

// point-lights.js lightMask: bit i for each packed light whose sphere
// reaches one of the boxes (those `visible` marks, when given).
fn light_mask(lights: &[f64], count: usize, boxes: &[([f64; 3], [f64; 3])], visible: Option<&[u8]>) -> u32 {
    let mut mask = 0u32;
    for i in 0..count {
        let s = &lights[8 * i..8 * i + 4];
        let range = s[3];
        if range <= 0.0 {
            mask |= 1 << i;
            continue;
        }
        let reach = range * range;
        for (b, (min, max)) in boxes.iter().enumerate() {
            if let Some(v) = visible {
                if v[b] == 0 {
                    continue;
                }
            }
            let dx = (min[0] - s[0]).max(0.0).max(s[0] - max[0]);
            let dy = (min[1] - s[1]).max(0.0).max(s[1] - max[1]);
            let dz = (min[2] - s[2]).max(0.0).max(s[2] - max[2]);
            if dx * dx + dy * dy + dz * dz <= reach {
                mask |= 1 << i;
                break;
            }
        }
    }
    mask
}

// sphereLightMask.
fn sphere_light_mask(lights: &[f64], count: usize, c: &[f64; 3], radius: f64) -> u32 {
    let mut mask = 0u32;
    for i in 0..count {
        let s = &lights[8 * i..8 * i + 4];
        let (dx, dy, dz) = (s[0] - c[0], s[1] - c[1], s[2] - c[2]);
        if s[3] <= 0.0 || (dx * dx + dy * dy + dz * dz).sqrt() <= s[3] + radius {
            mask |= 1 << i;
        }
    }
    mask
}

// Box3.applyMatrix4.
fn box_to_world(m: &Mat4, min: &[f64; 3], max: &[f64; 3]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for k in 0..8 {
        let p = [if k & 4 != 0 { max[0] } else { min[0] }, if k & 2 != 0 { max[1] } else { min[1] }, if k & 1 != 0 { max[2] } else { min[2] }];
        let (q, _) = math::sphere_to_world(m, &p, 0.0);
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
    }
    (lo, hi)
}

impl<C: HasContext, M: MultiDraw> Renderer<C, M> {
    pub fn new(gl: C, multi: M) -> Renderer<C, M> {
        unsafe {
            let frame_ubo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(frame_ubo));
            gl.buffer_data_size(glow::UNIFORM_BUFFER, (FRAME_FLOATS * 4) as i32, glow::DYNAMIC_DRAW);
            gl.bind_buffer(glow::UNIFORM_BUFFER, None);
            let instance_texture = upload_texture(&gl, ROW_TEXELS as i32, MAX_ROWS as i32);
            let bone_texture = upload_texture(&gl, BONE_TEXELS as i32, BONE_ROWS as i32);
            gl.bind_texture(glow::TEXTURE_2D, None);
            Renderer {
                gl, multi, programs: vec![], geometries: vec![], textures: vec![], world: vec![], world_order: vec![], by_program: true,
                far_map: None, frame_ubo, frame: vec![0.0; FRAME_FLOATS], instance_texture, bone_texture,
                rows: vec![0.0; MAX_ROWS * ROW_TEXELS * 4], bone_upload: vec![0.0; MAX_BONES * 16],
                input: vec![0.0; IN_RECORDS + MAX_RECORDS * REC_STRIDE], bones: vec![0.0; MAX_BONES * 16],
                stats: Stats::default(), draws: Vec::with_capacity(MAX_ROWS), templates: Vec::with_capacity(MAX_RECORDS), log: vec![],
            }
        }
    }

    pub fn add_texture(&mut self, t: C::Texture) -> u32 {
        self.textures.push(t);
        (self.textures.len() - 1) as u32
    }

    pub fn set_far_map(&mut self, t: u32) {
        self.far_map = Some(self.textures[t as usize]);
    }

    // A program from three's final GLSL for one of its materials. attributes:
    // the locations three's own program gave each input, so a layout made
    // for three's matches.
    pub fn add_program(&mut self, kind: Kind, vs: &str, fs: &str, attributes: &[(String, u32)], material_uniforms: &[String]) -> Result<u32, String> {
        let gl = &self.gl;
        let vs = shader::rewrite(vs, Stage::Vertex, kind)?;
        let fs = shader::rewrite(fs, Stage::Fragment, kind)?;
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = vec![];
            for (stage, src) in [(glow::VERTEX_SHADER, &vs), (glow::FRAGMENT_SHADER, &fs)] {
                let s = gl.create_shader(stage)?;
                gl.shader_source(s, src);
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    let log = gl.get_shader_info_log(s);
                    gl.delete_shader(s);
                    gl.delete_program(program);
                    return Err(format!("{} shader: {}", if stage == glow::VERTEX_SHADER { "vertex" } else { "fragment" }, log));
                }
                gl.attach_shader(program, s);
                shaders.push(s);
            }
            for (name, location) in attributes {
                gl.bind_attrib_location(program, *location, name);
            }
            gl.link_program(program);
            for s in shaders {
                gl.detach_shader(program, s);
                gl.delete_shader(s);
            }
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(format!("link: {log}"));
            }
            if let Some(block) = gl.get_uniform_block_index(program, "BfeFrame") {
                gl.uniform_block_binding(program, block, 0);
            }
            gl.use_program(Some(program));
            for (name, unit) in SAMPLERS {
                if let Some(l) = gl.get_uniform_location(program, name) {
                    gl.uniform_1_i32(Some(&l), *unit as i32);
                }
            }
            let uniforms = material_uniforms.iter().map(|n| (n.clone(), gl.get_uniform_location(program, n), None)).collect();
            let p = Program {
                program, uniforms,
                zero_lights: gl.get_uniform_location(program, "zeroLights"),
                zero_light_count: gl.get_uniform_location(program, "zeroLightCount"),
                instance_base: gl.get_uniform_location(program, "bfeInstanceBase"),
                lights: vec![], light_count: -1,
            };
            gl.use_program(None);
            self.programs.push(p);
        }
        Ok((self.programs.len() - 1) as u32)
    }

    pub fn add_geometry(&mut self) -> u32 {
        unsafe {
            let vao = self.gl.create_vertex_array().unwrap();
            self.geometries.push(Geometry { vao, buffers: vec![], index_type: glow::UNSIGNED_INT, index_bytes: 4, index_count: 0, groups: vec![] });
        }
        (self.geometries.len() - 1) as u32
    }

    // A vertex buffer of the geometry (one attribute's, or several
    // interleaved); its slot for geometry_pointer.
    pub fn geometry_buffer(&mut self, g: u32, data: &[u8]) -> u32 {
        let gl = &self.gl;
        let geo = &mut self.geometries[g as usize];
        unsafe {
            let b = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, data, glow::STATIC_DRAW);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            geo.buffers.push(b);
        }
        (geo.buffers.len() - 1) as u32
    }

    pub fn geometry_pointer(&mut self, g: u32, buffer: u32, location: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: i32) {
        let gl = &self.gl;
        let geo = &self.geometries[g as usize];
        unsafe {
            gl.bind_vertex_array(Some(geo.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(geo.buffers[buffer as usize]));
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_f32(location, size, ty, normalized, stride, offset);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    pub fn geometry_index(&mut self, g: u32, data: &[u8], ty: u32) {
        let gl = &self.gl;
        let geo = &mut self.geometries[g as usize];
        unsafe {
            gl.bind_vertex_array(Some(geo.vao));
            let b = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(b));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, data, glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            geo.buffers.push(b);
        }
        geo.index_type = ty;
        geo.index_bytes = if ty == glow::UNSIGNED_SHORT { 2 } else { 4 };
        geo.index_count = data.len() as i32 / geo.index_bytes;
    }

    pub fn geometry_group(&mut self, g: u32, start: i32, count: i32) {
        self.geometries[g as usize].groups.push((start, count));
    }

    // bounds: sphere centre and radius, then box min and max (world space).
    pub fn add_world(&mut self, geometry: u32, program: u32, order: u32, flags: u32, bounds: &[f64]) -> u32 {
        self.world.push(World {
            geometry: geometry as usize, program: program as usize, textures: vec![], uniforms: vec![],
            cull: flags & 3, alpha_to_coverage: flags & 4 != 0, depth_write: flags & 8 != 0, point_lit: flags & 16 != 0, order,
            sphere: ([bounds[0], bounds[1], bounds[2]], bounds[3]),
            bbox: ([bounds[4], bounds[5], bounds[6]], [bounds[7], bounds[8], bounds[9]]),
            cells: None,
        });
        (self.world.len() - 1) as u32
    }

    pub fn world_texture(&mut self, w: u32, unit: u32, texture: u32) {
        let t = self.textures[texture as usize];
        self.world[w as usize].textures.push((unit, t));
    }

    // A material value its program reads (by the name add_program was given).
    pub fn world_uniform(&mut self, w: u32, name: &str, values: &[f32], int: bool) -> bool {
        let world = &mut self.world[w as usize];
        let program = &self.programs[world.program];
        let Some(slot) = program.uniforms.iter().position(|(n, _, _)| n == name) else { return false };
        let mut v = [0.0f32; 4];
        v[..values.len().min(4)].copy_from_slice(&values[..values.len().min(4)]);
        world.uniforms.push((slot, if int { Value::I(values[0] as i32) } else { Value::F(values.len(), v) }));
        true
    }

    // The cells render-culling.js culls a large bucket by: its triangles in
    // 48 m cells by their centres (shadow-casters.js), runs of consecutive
    // triangles in one cell, each cell's box over its vertices.
    pub fn world_cells(&mut self, w: u32, positions: &[f32], index: &[u32], cell_size: f64) {
        let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
        let mut parts: Vec<([f64; 3], [f64; 3])> = vec![];
        let triangles = index.len() / 3;
        let mut cell_of = vec![0u32; triangles];
        let p = |v: u32, a: usize| positions[v as usize * 3 + a] as f64;
        for t in 0..triangles {
            let (a, b, c) = (index[3 * t], index[3 * t + 1], index[3 * t + 2]);
            let x = (p(a, 0) + p(b, 0) + p(c, 0)) / 3.0;
            let z = (p(a, 2) + p(b, 2) + p(c, 2)) / 3.0;
            let key = ((x / cell_size).floor() as i64, (z / cell_size).floor() as i64);
            let id = *ids.entry(key).or_insert_with(|| {
                parts.push(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]));
                parts.len() - 1
            });
            cell_of[t] = id as u32;
            for v in [a, b, c] {
                let (min, max) = &mut parts[id];
                for k in 0..3 {
                    min[k] = min[k].min(p(v, k));
                    max[k] = max[k].max(p(v, k));
                }
            }
        }
        let mut runs = vec![];
        let mut first = 0;
        while first < triangles {
            let cell = cell_of[first];
            let mut end = first + 1;
            while end < triangles && cell_of[end] == cell {
                end += 1;
            }
            runs.push([cell, (first * 3) as u32, (end * 3) as u32]);
            first = end;
        }
        let n = runs.len();
        self.world[w as usize].cells = Some(Cells { visible: vec![0; parts.len()], parts, runs, offsets: vec![0; n], counts: vec![0; n] });
    }

    // three's opaque order (material creation) or by program first.
    pub fn sort(&mut self, by_program: bool) {
        self.by_program = by_program;
        let mut order: Vec<usize> = (0..self.world.len()).collect();
        if by_program {
            order.sort_by_key(|&i| (self.world[i].program, self.world[i].order));
        } else {
            order.sort_by_key(|&i| self.world[i].order);
        }
        self.world_order = order;
    }

    pub fn frame(&mut self, records: usize) -> Result<Stats, String> {
        let mut stats = Stats::default();
        let gl = &self.gl;
        let input = &self.input;
        // The frame block, from the page's values as three uploads them (f32).
        for i in 0..FRAME_FLOATS {
            self.frame[i] = input[i] as f32;
        }
        let mut projection = [0.0; 16];
        let mut view = [0.0; 16];
        projection.copy_from_slice(&input[0..16]);
        view.copy_from_slice(&input[16..32]);
        let frustum = Frustum::from_matrix(&math::mul(&projection, &view));
        let light_count = (input[FRAME_LIGHT_COUNT] as usize).min(MAX_LIGHTS);
        let lights = &input[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
        let lights32 = &self.frame[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
        unsafe {
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(self.frame_ubo));
            gl.buffer_sub_data_u8_slice(glow::UNIFORM_BUFFER, 0, bytes(&self.frame));
            gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(self.frame_ubo));
            // State three may have left otherwise; renderer.resetState() follows.
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.disable(glow::BLEND);
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.disable(glow::STENCIL_TEST);
            gl.color_mask(true, true, true, true);
            gl.front_face(glow::CCW);
            gl.active_texture(glow::TEXTURE0 + UNIT_FAR);
            gl.bind_texture(glow::TEXTURE_2D, self.far_map);
            stats.gl_calls += 12;
        }
        let mut current_program = usize::MAX;
        let mut current_cull = u32::MAX;
        let mut current_a2c = None;
        let mut current_depth_write = None;
        let mut bound: [Option<C::Texture>; 16] = [None; 16];
        bound[UNIT_FAR as usize] = self.far_map;
        let mut active_unit = UNIT_FAR;
        let mut light_list: Vec<f32> = Vec::with_capacity(8 * MAX_LIGHTS);
        macro_rules! set_state {
            ($cull:expr, $a2c:expr, $dw:expr) => {
                if current_cull != $cull {
                    match $cull {
                        2 => gl.disable(glow::CULL_FACE),
                        c => {
                            if current_cull == 2 || current_cull == u32::MAX {
                                gl.enable(glow::CULL_FACE);
                                stats.gl_calls += 1;
                            }
                            gl.cull_face(if c == 1 { glow::FRONT } else { glow::BACK });
                        }
                    }
                    current_cull = $cull;
                    stats.gl_calls += 1;
                }
                if current_a2c != Some($a2c) {
                    if $a2c { gl.enable(glow::SAMPLE_ALPHA_TO_COVERAGE) } else { gl.disable(glow::SAMPLE_ALPHA_TO_COVERAGE) }
                    current_a2c = Some($a2c);
                    stats.gl_calls += 1;
                }
                if current_depth_write != Some($dw) {
                    gl.depth_mask($dw);
                    current_depth_write = Some($dw);
                    stats.gl_calls += 1;
                }
            };
        }
        macro_rules! bind_texture {
            ($unit:expr, $t:expr) => {
                if bound[$unit as usize] != Some($t) {
                    if active_unit != $unit {
                        gl.active_texture(glow::TEXTURE0 + $unit);
                        active_unit = $unit;
                        stats.gl_calls += 1;
                    }
                    gl.bind_texture(glow::TEXTURE_2D, Some($t));
                    bound[$unit as usize] = Some($t);
                    stats.gl_calls += 1;
                }
            };
        }
        macro_rules! use_program {
            ($p:expr) => {
                if current_program != $p {
                    gl.use_program(Some(self.programs[$p].program));
                    current_program = $p;
                    stats.program_switches += 1;
                    stats.gl_calls += 1;
                }
            };
        }

        // 1. The static world.
        for &wi in &self.world_order {
            let w = &mut self.world[wi];
            // three tests a mesh's bounding sphere before it draws any of it.
            if !frustum.sphere(&w.sphere.0, w.sphere.1) {
                continue;
            }
            let geo = &self.geometries[w.geometry];
            let (mut ranges, mut first, mut count) = (1usize, 0i32, geo.index_count);
            let mut mask = 0u32;
            if let Some(cells) = &mut w.cells {
                for (i, (min, max)) in cells.parts.iter().enumerate() {
                    cells.visible[i] = frustum.aabb(min, max) as u8;
                }
                ranges = 0;
                let (mut start, mut end) = (-1i64, 0u32);
                for r in &cells.runs {
                    if cells.visible[r[0] as usize] == 0 {
                        continue;
                    }
                    if start >= 0 && r[1] == end {
                        end = r[2];
                        continue;
                    }
                    if start >= 0 {
                        cells.offsets[ranges] = start as i32 * geo.index_bytes;
                        cells.counts[ranges] = (end as i64 - start) as i32;
                        ranges += 1;
                    }
                    start = r[1] as i64;
                    end = r[2];
                }
                if start >= 0 {
                    cells.offsets[ranges] = start as i32 * geo.index_bytes;
                    cells.counts[ranges] = (end as i64 - start) as i32;
                    ranges += 1;
                }
                if ranges == 0 {
                    continue;
                }
                first = cells.offsets[0] / geo.index_bytes;
                count = end as i32 - first;
                if w.point_lit && light_count > 0 {
                    mask = light_mask(lights, light_count, &cells.parts, Some(&cells.visible));
                }
            } else if w.point_lit && light_count > 0 {
                mask = light_mask(lights, light_count, std::slice::from_ref(&w.bbox), None);
            }
            unsafe {
                use_program!(w.program);
                let program = &mut self.programs[w.program];
                for &(slot, value) in &w.uniforms {
                    let u = &mut program.uniforms[slot];
                    if u.2 != Some(value) {
                        match value {
                            Value::F(1, v) => gl.uniform_1_f32(u.1.as_ref(), v[0]),
                            Value::F(2, v) => gl.uniform_2_f32(u.1.as_ref(), v[0], v[1]),
                            Value::F(3, v) => gl.uniform_3_f32(u.1.as_ref(), v[0], v[1], v[2]),
                            Value::F(_, v) => gl.uniform_4_f32(u.1.as_ref(), v[0], v[1], v[2], v[3]),
                            Value::I(i) => gl.uniform_1_i32(u.1.as_ref(), i),
                        }
                        u.2 = Some(value);
                        stats.gl_calls += 1;
                    }
                }
                if w.point_lit && program.zero_light_count.is_some() {
                    // useLights: the masked lights in pack order.
                    light_list.clear();
                    for i in 0..light_count {
                        if mask & (1 << i) != 0 {
                            light_list.extend_from_slice(&lights32[8 * i..8 * i + 8]);
                        }
                    }
                    let n = (light_list.len() / 8) as i32;
                    if n > 0 && program.lights != light_list {
                        gl.uniform_4_f32_slice(program.zero_lights.as_ref(), &light_list);
                        program.lights.clear();
                        program.lights.extend_from_slice(&light_list);
                        stats.gl_calls += 1;
                    }
                    if program.light_count != n {
                        gl.uniform_1_i32(program.zero_light_count.as_ref(), n);
                        program.light_count = n;
                        stats.gl_calls += 1;
                    }
                }
                for &(unit, t) in &w.textures {
                    bind_texture!(unit, t);
                }
                set_state!(w.cull, w.alpha_to_coverage, w.depth_write);
                gl.bind_vertex_array(Some(geo.vao));
                stats.gl_calls += 1;
                if ranges > 1 {
                    let cells = w.cells.as_mut().unwrap();
                    self.multi.multi_draw(glow::TRIANGLES, &mut cells.counts[..ranges], geo.index_type, &mut cells.offsets[..ranges]);
                } else {
                    gl.draw_elements(glow::TRIANGLES, count, geo.index_type, first * geo.index_bytes);
                }
                stats.gl_calls += 1;
                stats.world_draws += 1;
            }
        }

        // 2. Soldiers' bodies and guns, instanced.
        self.draws.clear();
        self.templates.clear();
        let mut bone_cursor = 0usize;
        for r in 0..records.min(MAX_RECORDS) {
            let rec = &input[IN_RECORDS + r * REC_STRIDE..IN_RECORDS + (r + 1) * REC_STRIDE];
            let mut world = [0.0; 16];
            world.copy_from_slice(&rec[REC_WORLD..REC_WORLD + 16]);
            let (center, radius) = math::sphere_to_world(&world, &[rec[REC_SPHERE], rec[REC_SPHERE + 1], rec[REC_SPHERE + 2]], rec[REC_SPHERE + 3]);
            if !frustum.sphere(&center, radius) {
                continue;
            }
            let skinned = rec[REC_KIND] == 1.0;
            let bone_count = rec[REC_BONES] as usize;
            if skinned && bone_cursor + bone_count > MAX_BONES {
                continue;
            }
            let model_view = math::mul(&view, &world);
            let normal = math::normal_matrix(&model_view);
            let mask = if light_count == 0 {
                0
            } else if skinned {
                sphere_light_mask(lights, light_count, &center, radius)
            } else {
                let b = box_to_world(&world, &[rec[REC_BOX], rec[REC_BOX + 1], rec[REC_BOX + 2]], &[rec[REC_BOX + 3], rec[REC_BOX + 4], rec[REC_BOX + 5]]);
                light_mask(lights, light_count, std::slice::from_ref(&b), None)
            };
            let mut row = [0.0f32; ROW_TEXELS * 4];
            for k in 0..16 {
                row[shader::ROW_MODEL * 4 + k] = world[k] as f32;
                row[shader::ROW_MODEL_VIEW * 4 + k] = model_view[k] as f32;
                row[shader::ROW_BIND_INVERSE * 4 + k] = rec[REC_BIND_INVERSE + k] as f32;
                row[shader::ROW_BIND * 4 + k] = rec[REC_BIND + k] as f32;
            }
            for c in 0..3 {
                for k in 0..3 {
                    row[(shader::ROW_NORMAL + c) * 4 + k] = normal[c * 3 + k] as f32;
                }
            }
            for k in 0..4 {
                row[shader::ROW_LOOK * 4 + k] = rec[REC_LOOK + k] as f32;
            }
            let mut n = 0usize;
            for i in 0..light_count {
                if mask & (1 << i) != 0 {
                    row[shader::ROW_LIGHTS * 4 + n] = i as f32;
                    n += 1;
                }
            }
            row[shader::ROW_COUNTS * 4] = n as f32;
            row[shader::ROW_COUNTS * 4 + 1] = bone_cursor as f32;
            if skinned {
                let at = rec[REC_BONE_AT] as usize * 16;
                self.bone_upload[bone_cursor * 16..(bone_cursor + bone_count) * 16].copy_from_slice(&self.bones[at..at + bone_count * 16]);
                bone_cursor += bone_count;
            }
            let template = self.templates.len();
            self.templates.push(row);
            let mirrored = math::mirrors(&world) as u32;
            let groups = (rec[REC_GROUPS] as usize).max(1);
            for g in 0..groups.min(4) {
                let key = [rec[REC_PROGRAM] as u32, rec[REC_GEOMETRY] as u32, g as u32, rec[REC_TEXTURES + g] as u32, (rec[REC_FLAGS] as u32 & 15) | mirrored << 4, rec[REC_GROUPS] as u32];
                if self.draws.len() < MAX_ROWS {
                    self.draws.push((key, template, g));
                }
            }
        }
        if !self.draws.is_empty() {
            self.draws.sort_by_key(|d| d.0);
            for (i, d) in self.draws.iter().enumerate() {
                self.rows[i * ROW_TEXELS * 4..(i + 1) * ROW_TEXELS * 4].copy_from_slice(&self.templates[d.1]);
            }
            unsafe {
                bind_texture!(UNIT_INSTANCES, self.instance_texture);
                gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, ROW_TEXELS as i32, self.draws.len() as i32, glow::RGBA, glow::FLOAT,
                    glow::PixelUnpackData::Slice(Some(bytes(&self.rows[..self.draws.len() * ROW_TEXELS * 4]))));
                stats.gl_calls += 1;
                if bone_cursor > 0 {
                    bind_texture!(UNIT_BONES, self.bone_texture);
                    let rows = (bone_cursor * 4 + BONE_TEXELS - 1) / BONE_TEXELS;
                    gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, BONE_TEXELS as i32, rows as i32, glow::RGBA, glow::FLOAT,
                        glow::PixelUnpackData::Slice(Some(bytes(&self.bone_upload[..rows * BONE_TEXELS * 4]))));
                    stats.gl_calls += 1;
                }
                let mut front = glow::CCW;
                let mut i = 0;
                while i < self.draws.len() {
                    let key = self.draws[i].0;
                    let mut j = i + 1;
                    while j < self.draws.len() && self.draws[j].0 == key {
                        j += 1;
                    }
                    let (p, g) = (key[0] as usize, key[1] as usize);
                    use_program!(p);
                    let program = &self.programs[p];
                    gl.uniform_1_i32(program.instance_base.as_ref(), i as i32);
                    let geo = &self.geometries[g];
                    bind_texture!(UNIT_MAP, self.textures[key[3] as usize]);
                    set_state!(key[4] & 3, key[4] & 4 != 0, key[4] & 8 != 0);
                    let want = if key[4] & 16 != 0 { glow::CW } else { glow::CCW };
                    if want != front {
                        gl.front_face(want);
                        front = want;
                        stats.gl_calls += 1;
                    }
                    gl.bind_vertex_array(Some(geo.vao));
                    let (start, count) = if key[5] == 0 || geo.groups.is_empty() { (0, geo.index_count) } else { geo.groups[key[2] as usize] };
                    gl.draw_elements_instanced(glow::TRIANGLES, count, geo.index_type, start * geo.index_bytes, (j - i) as i32);
                    stats.gl_calls += 3;
                    stats.instance_draws += 1;
                    i = j;
                }
                stats.instances = self.draws.len() as u32;
            }
        }
        unsafe {
            gl.bind_vertex_array(None);
            gl.front_face(glow::CCW);
            stats.gl_calls += 2;
        }
        self.stats = stats;
        Ok(stats)
    }
}
