// Soldiers' bodies and the guns they hold (and, behind ?rustRigs, vehicles'
// and props' skinned rigs), instanced: a draw a model, texture, program and
// state, each instance's per-object values in a row of the instance texture
// and its bones in one texture holding every drawn rig's. The page writes a
// record an object into `input`, after the frame block's values, and the
// bones into `bones` (src/rust-render/instances.js).
use crate::core::{bytes, upload_texture, Camera, Core, MultiDraw, PASS_INSTANCES, UNIT_BONES, UNIT_INSTANCES, UNIT_MAP};
use crate::lights::{box_to_world, light_mask, sphere_light_mask};
use crate::math;
use crate::shader::{self, FRAME_FLOATS, FRAME_LIGHTS, MAX_LIGHTS, ROW_TEXELS};
use glow::HasContext;

// A soldier's record in `input`, after the frame block's values (f64 slots).
pub const IN_RECORDS: usize = FRAME_FLOATS;
pub const REC_KIND: usize = 0; // 1: skinned body, 2: rigid mesh
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
pub const REC_STRIDE: usize = REC_EXTRA + shader::ROW_EXTRA_TEXELS * 4;
pub const MAX_RECORDS: usize = 256;
pub const MAX_ROWS: usize = 512;
pub const BONE_TEXELS: usize = 1024; // a bone texture row
pub const BONE_ROWS: usize = 64;
pub const MAX_BONES: usize = BONE_TEXELS * BONE_ROWS / 4;
// The inputs this pass needs (core.rs input and bones).
pub const INPUT_LEN: usize = IN_RECORDS + MAX_RECORDS * REC_STRIDE;
pub const BONES_LEN: usize = MAX_BONES * 16;

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
        }
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
        if self.draws.is_empty() {
            return;
        }
        self.draws.sort_by_key(|d| d.0);
        for (i, d) in self.draws.iter().enumerate() {
            self.rows[i * ROW_TEXELS * 4..(i + 1) * ROW_TEXELS * 4].copy_from_slice(&self.templates[d.1]);
        }
        unsafe {
            state.bind_texture(gl, stats, UNIT_INSTANCES, Some(self.instance_texture));
            gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, ROW_TEXELS as i32, self.draws.len() as i32, glow::RGBA, glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytes(&self.rows[..self.draws.len() * ROW_TEXELS * 4]))));
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
