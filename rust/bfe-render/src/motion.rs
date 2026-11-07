// The temporal upscaler's motion of the bodies that move on their own
// (src/motion-vectors.js), drawn in place of three's render of their
// proxies and of the first-person view's locked mask, into the upscaler's
// velocity target (input resolution, half floats): the target cleared to
// no motion (unless three drew into it first: the sky's ships, which draw
// their own), each proxy drawn with three's program for it (Kind::Motion)
// from its row, in the order three sorts them, then the view scene's parts
// with the locked mask's program, marked fixed on screen. Neither is depth
// tested; a proxy's fragment shader keeps only what lies on the surface
// the scene's depth shows there (sceneDepth).
//
// The page writes the frame's values (the cameras as they were drawn, the
// view-projections now and last frame, the depth range, the target) and a
// record a draw into `input` (f64), the drawn rigs' bones as they are now
// into `bones` and as they were last frame into `previous` (f32, three's
// layout: 16 floats a bone). Each record's row (shader.rs: its matrices
// now and last frame, its bones' bases) goes in the pass's instance
// texture; consecutive records of one program, geometry, index range and
// side are one instanced draw.
//
// Before the records, the sky's ships draw their own motion (sky-flyers.js:
// MotionVectors.addExtra), as three draws them first into the target: an
// instanced mesh a model part, with three's program for its material as
// three compiled it (Kind::Plain, every uniform set per draw), its ships'
// matrices now and last frame and whether each shows as instance
// attributes, which the page writes each frame (`extras`, `extra_values`).
// They write only where the near scene drew nothing, which no proxy covers.
use crate::core::{bytes, upload_texture, Core, MultiDraw, PASS_MOTION, UNIT_BONES, UNIT_INSTANCES};
use crate::post::{set_uniform, UNIFORM};
use crate::math::{self, Mat4};
use crate::shader::{self, ROW_TEXELS};
use glow::HasContext;
use std::collections::HashMap;

// The frame's values (f64 slots of `input`): the near camera's projection
// and view matrix (as the scene was drawn: jittered), the first-person
// camera's, the unjittered view-projections now and last frame
// (currentViewProjection, previousViewProjection), the depth range
// (depthRange), the target (a registered texture) and its size, the scene's
// depth (sceneDepth: a registered texture), whether to clear the target,
// and how many bone matrices the page wrote into `bones` and `previous`.
pub const IN_PROJECTION: usize = 0;
pub const IN_VIEW: usize = 16;
pub const IN_VIEW_PROJECTION: usize = 32;
pub const IN_VIEW_VIEW: usize = 48;
pub const IN_CURRENT: usize = 64;
pub const IN_PREVIOUS: usize = 80;
pub const IN_RANGE: usize = 96;
pub const IN_TARGET: usize = 98;
pub const IN_SIZE: usize = 99;
pub const IN_DEPTH: usize = 101;
pub const IN_CLEAR: usize = 102;
pub const IN_BONES: usize = 103;
pub const IN_PREVIOUS_BONES: usize = 104;
pub const IN_RECORDS: usize = 106;
// A record: its kind, three's program for it, its geometry, the index range
// drawn (first, count), the side drawn (0 front: back faces culled, 1 back,
// 2 both), where its bones are in `bones` and last frame's in `previous`
// (first matrix), then its matrices: world now and last frame, bind, bind
// inverse now and last frame.
pub const REC_KIND: usize = 0;
pub const REC_PROGRAM: usize = 1;
pub const REC_GEOMETRY: usize = 2;
pub const REC_FIRST: usize = 3;
pub const REC_COUNT: usize = 4;
pub const REC_SIDE: usize = 5;
pub const REC_BONE_AT: usize = 6;
pub const REC_PREVIOUS_AT: usize = 7;
pub const REC_WORLD: usize = 8;
pub const REC_PREVIOUS_WORLD: usize = 24;
pub const REC_BIND: usize = 40;
pub const REC_BIND_INVERSE: usize = 56;
pub const REC_PREVIOUS_BIND_INVERSE: usize = 72;
pub const REC_STRIDE: usize = 88;
// A proxy, drawn with the near camera; a part of the first-person view,
// drawn with the view's camera.
pub const KIND_PROXY: f64 = 1.0;
pub const KIND_LOCKED: f64 = 2.0;
pub const MAX_RECORDS: usize = 256;
// The bone textures' rows (as wide as the instances pass's).
pub const BONE_TEXELS: usize = 1024;
pub const BONE_ROWS: usize = 32;
pub const MAX_BONES: usize = BONE_TEXELS * BONE_ROWS / 4;
pub const INPUT_LEN: usize = IN_RECORDS + MAX_RECORDS * REC_STRIDE;
// Where the programs read last frame's bones and the scene's depth: units
// none of their other samplers use (bfeInstances and boneTexture are on
// the core's fixed ones).
pub const UNIT_PREVIOUS_BONES: u32 = 12;
pub const UNIT_SCENE_DEPTH: u32 = 13;
// The extras the page writes each frame (f64, `extras`): how many, then an
// extra each: its registration, how many instances draw, the side drawn,
// where its instances' values start in `extra_values` (f32: each instance
// attribute's values for those instances, one attribute after another), how
// many uniforms, then the uniforms (post.rs UNIFORM each).
pub const EX_ID: usize = 0;
pub const EX_INSTANCES: usize = 1;
pub const EX_SIDE: usize = 2;
pub const EX_AT: usize = 3;
pub const EX_UNIFORMS: usize = 4;
pub const EX_FIRST: usize = 5;
pub const MAX_EXTRA_UNIFORMS: usize = 8;
pub const EXTRA: usize = EX_FIRST + MAX_EXTRA_UNIFORMS * UNIFORM;
pub const MAX_EXTRAS: usize = 16;
pub const EXTRAS_LEN: usize = 1 + MAX_EXTRAS * EXTRA;
pub const EXTRA_VALUES: usize = 32768;

// A program's uniforms the pass sets: the projection, the view-projections
// now and last frame, the depth range.
struct Uniforms<C: HasContext> {
    projection: Option<C::UniformLocation>,
    current: Option<C::UniformLocation>,
    previous: Option<C::UniformLocation>,
    range: Option<C::UniformLocation>,
}

// An extra: its program and geometry, its per-instance attributes (a
// buffer each, floats an instance) and how many instances they hold.
struct Extra<C: HasContext> {
    program: usize,
    geometry: usize,
    attributes: Vec<(C::Buffer, usize)>,
    max: usize,
}

pub struct MotionPass<C: HasContext> {
    rows_texture: C::Texture,
    bones_texture: C::Texture,
    previous_texture: C::Texture,
    rows: Vec<f32>,
    pub input: Vec<f64>,
    pub bones: Vec<f32>,
    pub previous: Vec<f32>,
    uniforms: HashMap<usize, Uniforms<C>>,
    extras: Vec<Extra<C>>,
    pub extras_input: Vec<f64>,
    pub extra_values: Vec<f32>,
    // Matrices each extra's program holds, by (program, slot).
    matrices: HashMap<(usize, usize), [f32; 16]>,
}

fn matrix(v: &[f64]) -> Mat4 {
    let mut m = [0.0; 16];
    m.copy_from_slice(&v[..16]);
    m
}

fn as_f32(v: &[f64]) -> [f32; 16] {
    let mut m = [0.0f32; 16];
    for k in 0..16 {
        m[k] = v[k] as f32;
    }
    m
}

impl<C: HasContext> MotionPass<C> {
    pub fn new<M: MultiDraw>(core: &mut Core<C, M>) -> MotionPass<C> {
        let gl = &core.gl;
        let rows_texture = upload_texture(gl, ROW_TEXELS as i32, MAX_RECORDS as i32);
        let bones_texture = upload_texture(gl, BONE_TEXELS as i32, BONE_ROWS as i32);
        let previous_texture = upload_texture(gl, BONE_TEXELS as i32, BONE_ROWS as i32);
        unsafe { gl.bind_texture(glow::TEXTURE_2D, None) };
        MotionPass {
            rows_texture, bones_texture, previous_texture, rows: vec![0.0; MAX_RECORDS * ROW_TEXELS * 4], input: vec![0.0; INPUT_LEN],
            bones: vec![0.0; MAX_BONES * 16], previous: vec![0.0; MAX_BONES * 16], uniforms: HashMap::new(),
            extras: vec![], extras_input: vec![0.0; EXTRAS_LEN], extra_values: vec![0.0; EXTRA_VALUES], matrices: HashMap::new(),
        }
    }

    // An extra (a mesh that draws its own motion): its program, its geometry
    // (the attributes it shares with every instance, and its index), and its
    // per-instance attributes for up to `max` instances, each [location,
    // columns, rows] (a matrix takes `columns` locations of `rows` floats).
    pub fn add_extra<M: MultiDraw>(&mut self, core: &mut Core<C, M>, program: u32, geometry: u32, max: u32, layout: &[u32]) -> u32 {
        let gl = &core.gl;
        let max = max as usize;
        let mut attributes = vec![];
        unsafe {
            gl.bind_vertex_array(Some(core.geometries[geometry as usize].vao));
            for a in layout.chunks_exact(3) {
                let (location, columns, rows) = (a[0], a[1], a[2] as i32);
                let floats = columns as usize * rows as usize;
                let b = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                gl.buffer_data_size(glow::ARRAY_BUFFER, (max.max(1) * floats * 4) as i32, glow::DYNAMIC_DRAW);
                for c in 0..columns {
                    gl.enable_vertex_attrib_array(location + c);
                    gl.vertex_attrib_pointer_f32(location + c, rows, glow::FLOAT, false, floats as i32 * 4, c as i32 * rows * 4);
                    gl.vertex_attrib_divisor(location + c, 1);
                }
                attributes.push((b, floats));
            }
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        self.extras.push(Extra { program: program as usize, geometry: geometry as usize, attributes, max });
        (self.extras.len() - 1) as u32
    }

    // The extras the page wrote, in its order (three's), into the target
    // bound now: each one's instances sent, its uniforms set, one instanced
    // draw.
    fn draw_extras<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        let n = (self.extras_input[0].max(0.0) as usize).min(MAX_EXTRAS);
        for e in 0..n {
            let ex = &self.extras_input[1 + e * EXTRA..1 + (e + 1) * EXTRA];
            let Some(extra) = self.extras.get(ex[EX_ID] as usize) else { continue };
            let instances = (ex[EX_INSTANCES].max(0.0) as usize).min(extra.max);
            if instances == 0 || extra.program >= core.programs.len() || extra.geometry >= core.geometries.len() {
                continue;
            }
            let mut at = ex[EX_AT].max(0.0) as usize;
            if at + instances * extra.attributes.iter().map(|a| a.1).sum::<usize>() > EXTRA_VALUES {
                continue;
            }
            let gl = &core.gl;
            for &(buffer, floats) in &extra.attributes {
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                    crate::core::spare_view_slot();
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes(&self.extra_values[at..at + instances * floats]));
                }
                at += instances * floats;
                core.stats.gl_calls += 2;
            }
            unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, None) };
            core.stats.gl_calls += 1;
            let program = extra.program;
            core.state.use_program(&core.gl, &mut core.stats, core.programs[program].program);
            let mut unit = 0u32;
            for k in 0..(ex[EX_UNIFORMS] as usize).min(MAX_EXTRA_UNIFORMS) {
                let u = EX_FIRST + k * UNIFORM;
                set_uniform(core, &mut self.matrices, program, &ex[u..u + UNIFORM], &mut unit);
            }
            core.state.cull(&core.gl, &mut core.stats, ex[EX_SIDE] as u32);
            let geo = &core.geometries[extra.geometry];
            unsafe {
                core.gl.bind_vertex_array(Some(geo.vao));
                core.gl.draw_elements_instanced(glow::TRIANGLES, geo.index_count, geo.index_type, 0, instances as i32);
            }
            core.stats.gl_calls += 2;
            core.stats.instance_draws += 1;
            core.stats.pass_draws[PASS_MOTION] += 1;
        }
        self.extras_input[0] = 0.0;
    }

    // A program's uniforms, found the first time it draws (its samplers put
    // on the pass's units then).
    fn uniforms<M: MultiDraw>(&mut self, core: &mut Core<C, M>, program: usize) -> &Uniforms<C> {
        let gl = &core.gl;
        let p = core.programs[program].program;
        self.uniforms.entry(program).or_insert_with(|| unsafe {
            for (name, unit) in [("previousBones", UNIT_PREVIOUS_BONES), ("sceneDepth", UNIT_SCENE_DEPTH)] {
                if let Some(l) = gl.get_uniform_location(p, name) {
                    gl.uniform_1_i32(Some(&l), unit as i32);
                }
            }
            Uniforms {
                projection: gl.get_uniform_location(p, "projectionMatrix"),
                current: gl.get_uniform_location(p, "currentViewProjection"),
                previous: gl.get_uniform_location(p, "previousViewProjection"),
                range: gl.get_uniform_location(p, "depthRange"),
            }
        })
    }

    // records: how many the page wrote this frame.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, records: usize) {
        let records = records.min(MAX_RECORDS);
        let target = self.input[IN_TARGET];
        if target < 0.0 {
            return;
        }
        // The renderer's own framebuffer over the target's texture.
        let Ok(fb) = core.framebuffer(Some(target as u32), None) else { return };
        core.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        let (width, height) = (self.input[IN_SIZE] as i32, self.input[IN_SIZE + 1] as i32);
        unsafe { core.gl.viewport(0, 0, width, height) };
        core.stats.gl_calls += 1;
        // Neither depth tested nor blended, every channel written.
        crate::post::full_screen(core);
        if self.input[IN_CLEAR] != 0.0 {
            unsafe {
                core.gl.clear_color(0.0, 0.0, 0.0, 0.0);
                core.gl.clear(glow::COLOR_BUFFER_BIT);
            }
            core.stats.gl_calls += 2;
        }
        // The sky's ships' own motion, first, as three draws them.
        self.draw_extras(core);
        if records == 0 {
            return;
        }
        let input = &self.input;
        let (view, view_view) = (matrix(&input[IN_VIEW..]), matrix(&input[IN_VIEW_VIEW..]));
        for r in 0..records {
            let rec = &input[IN_RECORDS + r * REC_STRIDE..IN_RECORDS + (r + 1) * REC_STRIDE];
            let world = matrix(&rec[REC_WORLD..]);
            let model_view = math::mul(if rec[REC_KIND] == KIND_LOCKED { &view_view } else { &view }, &world);
            let normal = math::normal_matrix(&model_view);
            let row = &mut self.rows[r * ROW_TEXELS * 4..(r + 1) * ROW_TEXELS * 4];
            row.fill(0.0);
            for k in 0..16 {
                row[shader::ROW_MODEL * 4 + k] = world[k] as f32;
                row[shader::ROW_MODEL_VIEW * 4 + k] = model_view[k] as f32;
                row[shader::ROW_BIND_INVERSE * 4 + k] = rec[REC_BIND_INVERSE + k] as f32;
                row[shader::ROW_BIND * 4 + k] = rec[REC_BIND + k] as f32;
                row[shader::ROW_PREVIOUS_MODEL * 4 + k] = rec[REC_PREVIOUS_WORLD + k] as f32;
                row[shader::ROW_PREVIOUS_BIND_INVERSE * 4 + k] = rec[REC_PREVIOUS_BIND_INVERSE + k] as f32;
            }
            for c in 0..3 {
                for k in 0..3 {
                    row[(shader::ROW_NORMAL + c) * 4 + k] = normal[c * 3 + k] as f32;
                }
            }
            row[shader::ROW_COUNTS * 4 + 1] = rec[REC_BONE_AT] as f32;
            row[shader::ROW_COUNTS * 4 + 2] = rec[REC_PREVIOUS_AT] as f32;
        }
        let bones = (input[IN_BONES] as usize).min(MAX_BONES);
        let previous = (input[IN_PREVIOUS_BONES] as usize).min(MAX_BONES);
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        unsafe {
            state.bind_texture(gl, stats, UNIT_INSTANCES, Some(self.rows_texture));
            crate::core::spare_view_slot();
            gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, ROW_TEXELS as i32, records as i32, glow::RGBA, glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytes(&self.rows[..records * ROW_TEXELS * 4]))));
            stats.gl_calls += 1;
            for (texture, unit, data, n) in [(self.bones_texture, UNIT_BONES, &self.bones, bones), (self.previous_texture, UNIT_PREVIOUS_BONES, &self.previous, previous)] {
                state.bind_texture(gl, stats, unit, Some(texture));
                if n > 0 {
                    let rows = (n * 4 + BONE_TEXELS - 1) / BONE_TEXELS;
                    crate::core::spare_view_slot();
                    gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, BONE_TEXELS as i32, rows as i32, glow::RGBA, glow::FLOAT,
                        glow::PixelUnpackData::Slice(Some(bytes(&data[..rows * BONE_TEXELS * 4]))));
                    stats.gl_calls += 1;
                }
            }
        }
        let depth = input[IN_DEPTH];
        let depth = if depth < 0.0 { None } else { core.textures.get(depth as usize).copied() };
        core.state.bind_texture(&core.gl, &mut core.stats, UNIT_SCENE_DEPTH, depth);
        let (projection, view_projection) = (as_f32(&self.input[IN_PROJECTION..]), as_f32(&self.input[IN_VIEW_PROJECTION..]));
        let (current, last) = (as_f32(&self.input[IN_CURRENT..]), as_f32(&self.input[IN_PREVIOUS..]));
        let range = (self.input[IN_RANGE] as f32, self.input[IN_RANGE + 1] as f32);
        // Runs of consecutive records drawn alike, one instanced draw each, in
        // the page's order.
        let key = |rec: &[f64]| [rec[REC_KIND], rec[REC_PROGRAM], rec[REC_GEOMETRY], rec[REC_FIRST], rec[REC_COUNT], rec[REC_SIDE]];
        let mut set: Vec<usize> = Vec::with_capacity(4);
        let mut i = 0;
        while i < records {
            let rec = &self.input[IN_RECORDS + i * REC_STRIDE..IN_RECORDS + (i + 1) * REC_STRIDE];
            let k = key(rec);
            let mut j = i + 1;
            while j < records && key(&self.input[IN_RECORDS + j * REC_STRIDE..IN_RECORDS + (j + 1) * REC_STRIDE]) == k {
                j += 1;
            }
            let (program, geometry) = (k[1] as usize, k[2] as usize);
            if program < core.programs.len() && geometry < core.geometries.len() && k[4] > 0.0 {
                core.state.use_program(&core.gl, &mut core.stats, core.programs[program].program);
                if !set.contains(&program) {
                    let u = self.uniforms(core, program);
                    let p = if k[0] == KIND_LOCKED { &view_projection } else { &projection };
                    let gl = &core.gl;
                    unsafe {
                        gl.uniform_matrix_4_f32_slice(u.projection.as_ref(), false, p);
                        gl.uniform_matrix_4_f32_slice(u.current.as_ref(), false, &current);
                        gl.uniform_matrix_4_f32_slice(u.previous.as_ref(), false, &last);
                        gl.uniform_2_f32(u.range.as_ref(), range.0, range.1);
                    }
                    core.stats.gl_calls += 4;
                    set.push(program);
                }
                core.state.cull(&core.gl, &mut core.stats, k[5] as u32);
                let (gl, programs, geometries) = (&core.gl, &core.programs, &core.geometries);
                let geo = &geometries[geometry];
                unsafe {
                    gl.uniform_1_i32(programs[program].instance_base.as_ref(), i as i32);
                    gl.bind_vertex_array(Some(geo.vao));
                    gl.draw_elements_instanced(glow::TRIANGLES, k[4] as i32, geo.index_type, k[3] as i32 * geo.index_bytes, (j - i) as i32);
                }
                core.stats.gl_calls += 3;
                core.stats.instance_draws += 1;
                core.stats.pass_draws[PASS_MOTION] += 1;
            }
            i = j;
        }
        core.stats.instances = records as u32;
    }
}
