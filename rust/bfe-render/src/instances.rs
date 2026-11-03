// Soldiers' bodies and the guns they hold (and, behind ?rustRigs, vehicles'
// and props' skinned rigs), instanced: a draw a model, texture, program and
// state, each instance's per-object values in a row of the instance texture
// and its bones in one texture holding every drawn rig's. The page writes a
// record an object into `input`, after the frame block's values, and the
// bones into `bones` (src/rust-render/instances.js).
//
// A rig whose material blends (bes1's jedi fighter, its glass) is a record
// of a third kind: its row and bones are made with the others', but it is
// drawn as an item of the near scene's transparent list (transparent.rs
// Source), with its material's blending, depth and passes (add_blend), at
// the place three's sort would draw it.
use crate::core::{bytes, upload_texture, Camera, Core, MultiDraw, PASS_INSTANCES, UNIT_BONES, UNIT_INSTANCES, UNIT_MAP};
use crate::transparent::{three_z, Item, Source, View};
use crate::lights::{box_to_world, light_mask, sphere_light_mask};
use crate::math;
use crate::shader::{self, FRAME_FLOATS, FRAME_LIGHTS, MAX_LIGHTS, ROW_TEXELS};
use glow::HasContext;

// A soldier's record in `input`, after the frame block's values (f64 slots).
pub const IN_RECORDS: usize = FRAME_FLOATS;
pub const REC_KIND: usize = 0; // 1: skinned body, 2: rigid mesh, 3: skinned rig that blends
pub const REC_PROGRAM: usize = 1;
pub const REC_GEOMETRY: usize = 2;
pub const REC_GROUPS: usize = 3; // groups drawn; 0 draws the whole index
pub const REC_TEXTURES: usize = 4; // a texture a group (4 slots; below 0 none)
pub const REC_BONES: usize = 8;
pub const REC_BONE_AT: usize = 9; // first matrix in `bones`
pub const REC_SPHERE: usize = 10; // object space: centre, radius
pub const REC_BOX: usize = 14; // object space: min, max (the light mask of a rigid mesh)
pub const REC_WORLD: usize = 20; // matrixWorld
pub const REC_BIND_INVERSE: usize = 36;
pub const REC_LOOK: usize = 52; // sunScale, ambientPull, brightness
pub const REC_FLAGS: usize = 56; // cull side (0 front, 1 back, 2 both) | 4 alpha to coverage | 8 depth write
pub const REC_BIND: usize = 60; // bindMatrix (skinned)
pub const REC_EXTRA: usize = 76; // a material's own values the instance gives (shader.rs EXTRAS: its row from ROW_EXTRA)
// A blending rig's: its blend (add_blend), then three's key for it but z:
// group order, render order, id (z from its sphere's centre in the world).
pub const REC_BLEND: usize = REC_EXTRA + shader::ROW_EXTRA_TEXELS * 4;
pub const REC_STRIDE: usize = REC_BLEND + 4;
pub const KIND_BLENDED: f64 = 3.0;
pub const MAX_RECORDS: usize = 256;
pub const MAX_ROWS: usize = 512;
pub const BONE_TEXELS: usize = 1024; // a bone texture row
pub const BONE_ROWS: usize = 64;
pub const MAX_BONES: usize = BONE_TEXELS * BONE_ROWS / 4;
// The inputs this pass needs (core.rs input and bones).
pub const INPUT_LEN: usize = IN_RECORDS + MAX_RECORDS * REC_STRIDE;
pub const BONES_LEN: usize = MAX_BONES * 16;

// A blending material's draw state: three's blending (enabled, equations,
// factors), depth test, function and write, and its passes in drawing
// order (program, side: 0 front, 1 back, 2 both).
pub struct BlendState {
    blend: [u32; 7],
    depth: (bool, u32, bool),
    passes: Vec<(usize, u32)>,
}

// A blending rig drawn this frame: its blend, its row, three's key for it
// (group order, render order, its sphere's centre in the world, id), its
// geometry, its texture and whether its world matrix mirrors.
struct Blended {
    blend: usize,
    row: usize,
    key: [f64; 6],
    geometry: usize,
    texture: u32,
    mirrored: bool,
}

pub struct InstancePass<C: HasContext> {
    instance_texture: C::Texture,
    // What a material without a map samples, as three's empty texture
    // gives it: nothing (all channels 0).
    empty: C::Texture,
    bone_texture: C::Texture,
    rows: Vec<f32>,
    bone_upload: Vec<f32>,
    // Draw instances this frame: (key, record, group).
    draws: Vec<([u32; 6], usize, usize)>,
    templates: Vec<[f32; ROW_TEXELS * 4]>,
    blends: Vec<BlendState>,
    // This frame's blending rigs, their rows after the others' (the
    // transparent list draws them), and their rows' templates.
    blended: Vec<Blended>,
    blended_templates: Vec<usize>,
}

impl<C: HasContext> InstancePass<C> {
    pub fn new<M: MultiDraw>(core: &mut Core<C, M>) -> InstancePass<C> {
        let gl = &core.gl;
        let instance_texture = upload_texture(gl, ROW_TEXELS as i32, MAX_ROWS as i32);
        let bone_texture = upload_texture(gl, BONE_TEXELS as i32, BONE_ROWS as i32);
        let empty = upload_texture(gl, 1, 1);
        unsafe { gl.bind_texture(glow::TEXTURE_2D, None) };
        InstancePass {
            instance_texture, empty, bone_texture, rows: vec![0.0; MAX_ROWS * ROW_TEXELS * 4], bone_upload: vec![0.0; MAX_BONES * 16],
            draws: Vec::with_capacity(MAX_ROWS), templates: Vec::with_capacity(MAX_RECORDS),
            blends: vec![], blended: vec![], blended_templates: vec![],
        }
    }

    // A blending material's draw state: blending (7 values), depth flags
    // (1 test, 2 write) and function, and its passes in drawing order
    // (program, side pairs).
    pub fn add_blend(&mut self, blend: &[u32], depth_flags: u32, depth_func: u32, passes: &[u32]) -> u32 {
        let mut b = [0u32; 7];
        b.copy_from_slice(&blend[..7]);
        let passes = passes.chunks(2).map(|p| (p[0] as usize, p[1])).collect();
        self.blends.push(BlendState { blend: b, depth: (depth_flags & 1 != 0, depth_func, depth_flags & 2 != 0), passes });
        (self.blends.len() - 1) as u32
    }

    // records: how many the page wrote this frame.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, camera: &Camera, records: usize) {
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let (programs, geometries, textures) = (&core.programs, &core.geometries, &core.textures);
        let (input, bones) = (&core.input, &core.bones);
        let view = &camera.view;
        let frustum = &camera.frustum;
        let light_count = camera.light_count;
        let lights = &input[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
        self.draws.clear();
        self.templates.clear();
        self.blended.clear();
        self.blended_templates.clear();
        let mut bone_cursor = 0usize;
        for r in 0..records.min(MAX_RECORDS) {
            let rec = &input[IN_RECORDS + r * REC_STRIDE..IN_RECORDS + (r + 1) * REC_STRIDE];
            let mut world = [0.0; 16];
            world.copy_from_slice(&rec[REC_WORLD..REC_WORLD + 16]);
            let (center, radius) = math::sphere_to_world(&world, &[rec[REC_SPHERE], rec[REC_SPHERE + 1], rec[REC_SPHERE + 2]], rec[REC_SPHERE + 3]);
            if !frustum.sphere(&center, radius) {
                continue;
            }
            let blended = rec[REC_KIND] == KIND_BLENDED;
            let skinned = rec[REC_KIND] == 1.0 || blended;
            let bone_count = rec[REC_BONES] as usize;
            if skinned && bone_cursor + bone_count > MAX_BONES {
                continue;
            }
            let model_view = math::mul(view, &world);
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
            for k in 0..shader::ROW_EXTRA_TEXELS * 4 {
                row[shader::ROW_EXTRA * 4 + k] = rec[REC_EXTRA + k] as f32;
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
                self.bone_upload[bone_cursor * 16..(bone_cursor + bone_count) * 16].copy_from_slice(&bones[at..at + bone_count * 16]);
                bone_cursor += bone_count;
            }
            let template = self.templates.len();
            self.templates.push(row);
            if blended {
                let b = rec[REC_BLEND] as usize;
                if b < self.blends.len() && self.blended.len() < 64 {
                    let texture = if rec[REC_TEXTURES] < 0.0 { u32::MAX } else { rec[REC_TEXTURES] as u32 };
                    let key = [rec[REC_BLEND + 1], rec[REC_BLEND + 2], center[0], center[1], center[2], rec[REC_BLEND + 3]];
                    self.blended.push(Blended { blend: b, row: 0, key, geometry: rec[REC_GEOMETRY] as usize, texture, mirrored: math::mirrors(&world) });
                    self.blended_templates.push(template);
                }
                continue;
            }
            let mirrored = math::mirrors(&world) as u32;
            let groups = (rec[REC_GROUPS] as usize).max(1);
            for g in 0..groups.min(4) {
                // No texture (none below 0): nothing bound on the map's unit.
                let texture = if rec[REC_TEXTURES + g] < 0.0 { u32::MAX } else { rec[REC_TEXTURES + g] as u32 };
                let key = [rec[REC_PROGRAM] as u32, rec[REC_GEOMETRY] as u32, g as u32, texture, (rec[REC_FLAGS] as u32 & 15) | mirrored << 4, rec[REC_GROUPS] as u32];
                if self.draws.len() < MAX_ROWS {
                    self.draws.push((key, template, g));
                }
            }
        }
        if self.draws.is_empty() && self.blended.is_empty() {
            return;
        }
        self.draws.sort_by_key(|d| d.0);
        for (i, d) in self.draws.iter().enumerate() {
            self.rows[i * ROW_TEXELS * 4..(i + 1) * ROW_TEXELS * 4].copy_from_slice(&self.templates[d.1]);
        }
        // The blending rigs' rows after the others'.
        let mut used = self.draws.len();
        for (b, &template) in self.blended.iter_mut().zip(&self.blended_templates) {
            if used >= MAX_ROWS {
                break;
            }
            b.row = used;
            self.rows[used * ROW_TEXELS * 4..(used + 1) * ROW_TEXELS * 4].copy_from_slice(&self.templates[template]);
            used += 1;
        }
        self.blended.truncate(used - self.draws.len());
        unsafe {
            state.bind_texture(gl, stats, UNIT_INSTANCES, Some(self.instance_texture));
            gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, ROW_TEXELS as i32, used as i32, glow::RGBA, glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytes(&self.rows[..used * ROW_TEXELS * 4]))));
            stats.gl_calls += 1;
            if bone_cursor > 0 {
                state.bind_texture(gl, stats, UNIT_BONES, Some(self.bone_texture));
                let rows = (bone_cursor * 4 + BONE_TEXELS - 1) / BONE_TEXELS;
                gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, BONE_TEXELS as i32, rows as i32, glow::RGBA, glow::FLOAT,
                    glow::PixelUnpackData::Slice(Some(bytes(&self.bone_upload[..rows * BONE_TEXELS * 4]))));
                stats.gl_calls += 1;
            }
            let mut i = 0;
            while i < self.draws.len() {
                let key = self.draws[i].0;
                let mut j = i + 1;
                while j < self.draws.len() && self.draws[j].0 == key {
                    j += 1;
                }
                let (p, g) = (key[0] as usize, key[1] as usize);
                let program = &programs[p];
                state.use_program(gl, stats, program.program);
                gl.uniform_1_i32(program.instance_base.as_ref(), i as i32);
                let geo = &geometries[g];
                state.bind_texture(gl, stats, UNIT_MAP, Some(textures.get(key[3] as usize).copied().unwrap_or(self.empty)));
                state.set_state(gl, stats, key[4] & 3, key[4] & 4 != 0, key[4] & 8 != 0);
                // Mirrored objects' faces wind the other way (three's frontFaceCW).
                state.front_face(gl, stats, if key[4] & 16 != 0 { glow::CW } else { glow::CCW });
                gl.bind_vertex_array(Some(geo.vao));
                let (start, count) = if key[5] == 0 || geo.groups.is_empty() { (0, geo.index_count) } else { geo.groups[key[2] as usize] };
                gl.draw_elements_instanced(glow::TRIANGLES, count, geo.index_type, start * geo.index_bytes, (j - i) as i32);
                stats.gl_calls += 3;
                stats.instance_draws += 1;
                stats.pass_draws[PASS_INSTANCES] += 1;
                i = j;
            }
            stats.instances = self.draws.len() as u32;
        }
    }
}

// The blending rigs as a source of the transparent list's items: each one
// item with the key the page wrote for it (z as three works it out, from
// its sphere's centre in the world), drawn from the row and bones the near
// phase uploaded, in each of its material's passes with its blending,
// depth and culling as three sets them.
impl<C: HasContext, M: MultiDraw> Source<C, M> for InstancePass<C> {
    fn emit(&mut self, _core: &mut Core<C, M>, view: &View, source: u32, items: &mut Vec<Item>) {
        for (k, b) in self.blended.iter().enumerate() {
            let key = &b.key;
            items.push(Item {
                group_order: key[0], render_order: key[1], z: three_z(&view.view_projection, None, &[key[2], key[3], key[4]]), id: key[5],
                source, batch: 0, record: k as u32,
            });
        }
    }

    fn draw_run(&mut self, core: &mut Core<C, M>, _view: &View, run: &[Item]) {
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let (programs, geometries, textures) = (&core.programs, &core.geometries, &core.textures);
        state.bind_texture(gl, stats, UNIT_INSTANCES, Some(self.instance_texture));
        state.bind_texture(gl, stats, UNIT_BONES, Some(self.bone_texture));
        state.color_mask(gl, stats, [true; 4]);
        state.alpha_to_coverage(gl, stats, false);
        for item in run {
            let Some(b) = self.blended.get(item.record as usize) else { continue };
            let (Some(blend), Some(geo)) = (self.blends.get(b.blend), geometries.get(b.geometry)) else { continue };
            state.bind_texture(gl, stats, UNIT_MAP, Some(textures.get(b.texture as usize).copied().unwrap_or(self.empty)));
            let f = &blend.blend;
            state.blend(gl, stats, f[0] != 0);
            if f[0] != 0 {
                state.blend_equation(gl, stats, f[1], f[2]);
                state.blend_func(gl, stats, f[3], f[4], f[5], f[6]);
            }
            state.depth_test(gl, stats, blend.depth.0);
            state.depth_func(gl, stats, blend.depth.1);
            state.depth_write(gl, stats, blend.depth.2);
            for &(p, side) in &blend.passes {
                let Some(program) = programs.get(p) else { continue };
                state.use_program(gl, stats, program.program);
                // As three culls a side: back faces, the front wound the
                // other way for a back pass (and for a mirrored object).
                state.cull(gl, stats, if side == 2 { 2 } else { 0 });
                state.front_face(gl, stats, if (side == 1) != b.mirrored { glow::CW } else { glow::CCW });
                unsafe {
                    gl.uniform_1_i32(program.instance_base.as_ref(), b.row as i32);
                    gl.bind_vertex_array(Some(geo.vao));
                    gl.draw_elements_instanced(glow::TRIANGLES, geo.index_count, geo.index_type, 0, 1);
                }
                stats.gl_calls += 3;
                stats.instance_draws += 1;
                stats.pass_draws[PASS_INSTANCES] += 1;
            }
        }
    }
}
