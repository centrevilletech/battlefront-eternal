// Soldiers' bodies and the guns they hold (and, behind ?rustRigs, vehicles'
// and props' skinned rigs), instanced: a draw a model, texture, program and
// state, each instance's per-object values in a row of the instance texture
// and its bones in one texture holding every drawn rig's. The client writes a
// record an object into `input`, after the frame block's values, and the
// bones into `bones`.
//
// A rig whose material blends (bes1's jedi fighter, its glass) is a record
// of a third kind: its row and bones are made with the others', but it is
// drawn as an item of the near scene's transparent list (transparent.rs
// Source), with its material's blending, depth and passes (add_blend), at
// the place three's sort would draw it. A rigid part that blends (a flying
// round's glow) is the same, of a fourth kind.
use crate::core::{bytes, upload_texture, Camera, Core, MultiDraw, PASS_INSTANCES, UNIT_BONES, UNIT_BUMP, UNIT_INSTANCES, UNIT_MAP};
use crate::transparent::{three_z, Item, Source, View};
use crate::lights::{box_to_world, light_mask, sphere_light_mask};
use crate::math;
use crate::shader::{self, FRAME_FLOATS, FRAME_LIGHTS, MAX_LIGHTS, ROW_TEXELS};
use glow::HasContext;

// A soldier's record in `input`, after the frame block's values (f64 slots).
pub const IN_RECORDS: usize = FRAME_FLOATS;
pub const REC_KIND: usize = 0; // 1: skinned body, 2: rigid mesh, 3: skinned rig that blends, 4: rigid mesh that blends
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
// A native caster can be excluded without changing its colour draw state.
pub const NO_SHADOW: u32 = 16;
// Native temporal identity uses the three reserved scalar slots after flags.
pub const REC_MOTION_ID:usize=57;
pub const REC_MOTION_GENERATION:usize=58;
pub const REC_MOTION_PART:usize=59;
pub const REC_BIND: usize = 60; // bindMatrix (skinned)
pub const REC_EXTRA: usize = 76; // a material's own values the instance gives (shader.rs EXTRAS: its row from ROW_EXTRA)
// A blending rig's or part's: its blend (add_blend), then three's key for it but z:
// group order, render order, id (z from its sphere's centre in the world).
pub const REC_BLEND: usize = REC_EXTRA + shader::ROW_EXTRA_TEXELS * 4;
pub const REC_STRIDE: usize = REC_BLEND + 4;
// receiveShadow, for a program that receives shadows (shader.rs SHADOW_EXTRAS: the record's EXTRA + 14). Bodies,
// held weapons and vehicle rigs set it, as their meshes do in the client build (combat.js,
// vehicle-rig.js); 2004 props and first-person view models leave it off.
pub const REC_RECEIVE_SHADOW: usize = REC_EXTRA + 14;
// Optional bump texture id encoded as (1 + texture_id) so zero means unbound; specular [exp, r, g, b] in texel 1.
// How far a 2005 soldier near the camera has faded (0 drawn whole), for the extended actor programs
// (shader.rs EXTRAS: bfeFade, the record's EXTRA + 15); 0 leaves every program's output as it was.
pub const REC_FADE: usize = REC_EXTRA + 15;
pub const REC_BUMP: usize = REC_EXTRA + 2;
pub const REC_SPECULAR: usize = REC_EXTRA + 4;
pub const KIND_BLENDED: f64 = 3.0;
pub const KIND_BLENDED_RIGID: f64 = 4.0;
// Per-frame instance limits shared by both 2004 and 2005 worlds.
// Logical rows pack up to PACK_COLS_MAX per physical texture scanline so
// MAX_ROWS fits within the desktop GL 3.3 minimum GL_MAX_TEXTURE_SIZE (1024),
// and the row texture starts at INITIAL_ROWS (256 KiB) and grows lazily only
// when a frame draws more than INITIAL_ROWS visible GPU rows.
pub const MAX_RECORDS: usize = 1024;
pub const MAX_ROWS: usize = 2048;
pub const INITIAL_ROWS: usize = 512;
pub const PACK_COLS_MAX: usize = 4;
pub const MAX_BLENDED: usize = 512;
pub const LOG_INTERVAL_FRAMES: u32 = 300;
// Skinned shadow caster texture pool; assigned dynamically only to skinned
// shadow casters so raising MAX_RECORDS does not create extra skin targets.
pub const MAX_CASTER_SKINS: usize = 256;
pub const BONE_TEXELS: usize = 1024; // a bone texture row
pub const BONE_ROWS: usize = 64;
pub const MAX_BONES: usize = BONE_TEXELS * BONE_ROWS / 4;
// The inputs this pass needs (core.rs input and bones).
pub const INPUT_LEN: usize = IN_RECORDS + MAX_RECORDS * REC_STRIDE;
pub const BONES_LEN: usize = MAX_BONES * 16;

/// Computes the packed RGBA32F row-texture layout `(pack_cols, init_height, max_tex_height, max_rows)`
/// so both texture width (`pack_cols * ROW_TEXELS`) and height stay within `max_tex`.
pub fn row_layout(max_tex: usize, wanted_rows: usize, initial_rows: usize) -> (usize, usize, usize, usize) {
    let max_tex = if max_tex == 0 { 1024 } else { max_tex.max(ROW_TEXELS) };
    let max_cols = (max_tex / ROW_TEXELS).min(PACK_COLS_MAX);
    let pack_cols = if max_cols >= 4 { 4 } else if max_cols >= 2 { 2 } else { 1 };
    let max_tex_height = wanted_rows.div_ceil(pack_cols).min(max_tex).max(1);
    let max_rows = (pack_cols * max_tex_height).min(wanted_rows);
    let init_height = initial_rows.min(max_rows).div_ceil(pack_cols).min(max_tex_height).max(1);
    (pack_cols, init_height, max_tex_height, max_rows)
}

/// Computes the RGBA32F bone-texture layout `(texels, rows, max_bones)` so both
/// texture dimensions stay within `max_tex` while keeping `texels` a multiple of 4.
pub fn bone_layout(max_tex: usize, wanted_bones: usize) -> (usize, usize, usize) {
    let max_tex = if max_tex == 0 { 1024 } else { max_tex.max(4) };
    let texels = (max_tex & !3).clamp(4, BONE_TEXELS);
    let rows = (wanted_bones * 4).div_ceil(texels).min(max_tex).max(1);
    let max_bones = (texels * rows / 4).min(wanted_bones);
    (texels, rows, max_bones)
}

/// Overflow counters and high-water marks for the per-frame instance bank.
/// Any object dropped because a bank limit was reached is counted here so the
/// developer stats overlay and stderr log can report it instead of dropping silently.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstanceOverflow {
    pub dropped_records: u32,
    pub dropped_rows: u32,
    pub dropped_blended: u32,
    pub dropped_bones: u32,
    pub dropped_caster_skins: u32,
    pub dropped_motion: u32,
    pub frame_drops: u32,
    pub overflow_frames: u32,
    pub peak_records: usize,
    pub peak_rows: usize,
    pub peak_blended: usize,
    pub peak_bones: usize,
    pub last_report_peak_drops: u32,
    interval_peak_drops: u32,
    frames_since_log: u32,
    logged_once: bool,
}

/// A count of omitted items as the `u32` the counters keep; more than `u32::MAX` saturates.
fn drops(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

impl InstanceOverflow {
    pub fn begin_frame(&mut self) {
        self.frame_drops = 0;
    }

    pub fn total_drops(&self) -> u32 {
        self.dropped_records
            .saturating_add(self.dropped_rows)
            .saturating_add(self.dropped_blended)
            .saturating_add(self.dropped_bones)
            .saturating_add(self.dropped_caster_skins)
            .saturating_add(self.dropped_motion)
    }

    pub fn note_record_drop(&mut self, count: usize) {
        let count = drops(count);
        self.dropped_records = self.dropped_records.saturating_add(count);
        self.frame_drops = self.frame_drops.saturating_add(count);
    }

    pub fn note_row_drop(&mut self, count: usize) {
        let count = drops(count);
        self.dropped_rows = self.dropped_rows.saturating_add(count);
        self.frame_drops = self.frame_drops.saturating_add(count);
    }

    pub fn note_blended_drop(&mut self, count: usize) {
        let count = drops(count);
        self.dropped_blended = self.dropped_blended.saturating_add(count);
        self.frame_drops = self.frame_drops.saturating_add(count);
    }

    pub fn note_bone_drop(&mut self, count: usize) {
        let count = drops(count);
        self.dropped_bones = self.dropped_bones.saturating_add(count);
        self.frame_drops = self.frame_drops.saturating_add(count);
    }

    pub fn note_caster_skin_drop(&mut self, count: usize) {
        let count = drops(count);
        self.dropped_caster_skins = self.dropped_caster_skins.saturating_add(count);
        self.frame_drops = self.frame_drops.saturating_add(count);
    }

    pub fn note_motion_drop(&mut self, count: usize) {
        let count = drops(count);
        self.dropped_motion = self.dropped_motion.saturating_add(count);
        self.frame_drops = self.frame_drops.saturating_add(count);
    }

    /// Finalizes the current frame's overflow accounting. Emits immediately on
    /// an initial overflow, then enforces a minimum `LOG_INTERVAL_FRAMES`
    /// cadence while tracking the worst single-frame drop count across the
    /// suppressed interval in `last_report_peak_drops`.
    pub fn finish_frame(&mut self) -> bool {
        if self.logged_once {
            self.frames_since_log = self.frames_since_log.saturating_add(1);
        }
        if self.frame_drops == 0 {
            return false;
        }
        self.overflow_frames = self.overflow_frames.saturating_add(1);
        self.interval_peak_drops = self.interval_peak_drops.max(self.frame_drops);
        if !self.logged_once || self.frames_since_log >= LOG_INTERVAL_FRAMES {
            self.logged_once = true;
            self.frames_since_log = 0;
            self.last_report_peak_drops = self.interval_peak_drops;
            self.interval_peak_drops = 0;
            true
        } else {
            false
        }
    }
}

// A blending material's draw state: three's blending (enabled, equations,
// factors), depth test, function and write, and its passes in drawing
// order (program, side: 0 front, 1 back, 2 both).
pub struct BlendState {
    blend: [u32; 7],
    depth: (bool, u32, bool),
    passes: Vec<(usize, u32)>,
}

// A blending rig or part drawn this frame: its blend, its row, three's key for it
// (group order, render order, its sphere's centre in the world, id), its
// geometry, its texture and whether its world matrix mirrors.
struct Blended {
    blend: usize,
    row: usize,
    key: [f64; 6],
    geometry: usize,
    texture: u32,
    bump: u32,
    mirrored: bool,
}

pub struct InstancePass<C: HasContext> {
    pub actor_descriptors: Vec<crate::actor_resources::InstanceDescriptor>,
    pub overflow: InstanceOverflow,
    instance_texture: C::Texture,
    // What a material without a map samples, as three's empty texture
    // gives it: nothing (all channels 0).
    empty: C::Texture,
    bone_texture: C::Texture,
    pack_cols: usize,
    tex_height: usize,
    max_tex_height: usize,
    max_rows: usize,
    bone_texels: usize,
    bone_rows: usize,
    max_bones: usize,
    rows: Vec<f32>,
    bone_upload: Vec<f32>,
    // Draw instances this frame: (key, record, group).
    draws: Vec<([u32; 7], usize, usize)>,
    templates: Vec<[f32; ROW_TEXELS * 4]>,
    blends: Vec<BlendState>,
    // This frame's blending rigs and parts, their rows after the others' (the
    // transparent list draws them), and their rows' templates.
    blended: Vec<Blended>,
    blended_templates: Vec<usize>,
    far_blended_order: Vec<(usize, f64)>,
}

impl Blended {
    fn painter_key(&self, z: f64) -> [f64; 4] {
        [self.key[0], self.key[1], z, self.key[5]]
    }
}

impl<C: HasContext> InstancePass<C> {
    pub fn row_capacity(&self) -> usize { self.max_rows }
    pub fn bone_capacity(&self) -> usize { self.max_bones }
    pub fn instance_texture_size(&self) -> (usize, usize) { (self.pack_cols * ROW_TEXELS, self.tex_height) }
    pub fn bone_texture_size(&self) -> (usize, usize) { (self.bone_texels, self.bone_rows) }
    pub fn texture_bytes(&self) -> u64 {
        ((self.pack_cols * ROW_TEXELS * self.tex_height + self.bone_texels * self.bone_rows + 1) * 16) as u64
    }
    pub fn new<M: MultiDraw>(core: &mut Core<C, M>) -> InstancePass<C> {
        let gl = &core.gl;
        let raw_max = unsafe { gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) };
        let max_tex = if raw_max > 0 { raw_max as usize } else { 1024 };
        let (pack_cols, tex_height, max_tex_height, max_rows) = row_layout(max_tex, MAX_ROWS, INITIAL_ROWS);
        let (bone_texels, bone_rows, max_bones) = bone_layout(max_tex, MAX_BONES);
        let instance_texture = core.hold_texture(upload_texture(gl, (pack_cols * ROW_TEXELS) as i32, tex_height as i32));
        let bone_texture = core.hold_texture(upload_texture(gl, bone_texels as i32, bone_rows as i32));
        let empty = core.hold_texture(upload_texture(gl, 1, 1));
        unsafe { gl.bind_texture(glow::TEXTURE_2D, None) };
        InstancePass {
            actor_descriptors: vec![],
            overflow: InstanceOverflow::default(),
            instance_texture, empty, bone_texture,
            pack_cols, tex_height, max_tex_height, max_rows,
            bone_texels, bone_rows, max_bones,
            rows: vec![0.0; max_tex_height * pack_cols * ROW_TEXELS * 4],
            bone_upload: vec![0.0; bone_rows * bone_texels * 4],
            draws: Vec::with_capacity(max_rows),
            templates: Vec::with_capacity(MAX_RECORDS),
            blends: vec![],
            blended: Vec::with_capacity(MAX_BLENDED),
            blended_templates: Vec::with_capacity(MAX_BLENDED),
            far_blended_order: Vec::with_capacity(MAX_BLENDED),
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

    // ActorMaterial settings preserve blend identity and side order while
    // replacing its linked catalog program on a cold resource command.
    pub fn blend_program(&mut self,id:u32,program:u32){if let Some(blend)=self.blends.get_mut(id as usize){for pass in &mut blend.passes{pass.0=program as usize;}}}
    // records: how many the client wrote this frame.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, camera: &Camera, records: usize) {
        // Actor resource handles expand in native storage before either opaque
        // batching or transparent emission. Live records contain only poses.
        for r in 0..records.min(MAX_RECORDS){let row=&mut core.input[IN_RECORDS+r*REC_STRIDE..IN_RECORDS+(r+1)*REC_STRIDE];if row[REC_GEOMETRY]==crate::actor_layout::kind::RETAINED as f64{if let Some(d)=self.actor_descriptors.get(row[REC_PROGRAM] as usize){d.publish(row);}else{row[REC_PROGRAM]=u32::MAX as f64;row[REC_GEOMETRY]=u32::MAX as f64;}}}
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let (programs, geometries, textures) = (&core.programs, &core.geometries, &core.textures);
        let (used, bone_cursor) = pack_instance_batch(
            &core.input,
            &core.bones,
            camera,
            records,
            geometries.len(),
            programs.len(),
            self.blends.len(),
            self.max_rows,
            self.max_bones,
            &mut self.draws,
            &mut self.templates,
            &mut self.blended,
            &mut self.blended_templates,
            &mut self.rows,
            &mut self.bone_upload,
            &mut self.overflow,
        );
        if used == 0 {
            return;
        }
        unsafe {
            let tex_width = self.pack_cols * ROW_TEXELS;
            let upload_height = used.div_ceil(self.pack_cols);
            state.bind_texture(gl, stats, UNIT_INSTANCES, Some(self.instance_texture));
            if upload_height > self.tex_height {
                let new_height = upload_height.next_power_of_two().min(self.max_tex_height);
                gl.tex_image_2d(
                    glow::TEXTURE_2D, 0, glow::RGBA32F as i32,
                    tex_width as i32, new_height as i32, 0,
                    glow::RGBA, glow::FLOAT, glow::PixelUnpackData::Slice(None),
                );
                self.tex_height = new_height;
                stats.gl_calls += 1;
            }
            crate::core::spare_view_slot();
            gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, tex_width as i32, upload_height as i32, glow::RGBA, glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytes(&self.rows[..upload_height * tex_width * 4]))));
            stats.gl_calls += 1;
            if bone_cursor > 0 {
                state.bind_texture(gl, stats, UNIT_BONES, Some(self.bone_texture));
                let rows = (bone_cursor * 4).div_ceil(self.bone_texels);
                crate::core::spare_view_slot();
                gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, self.bone_texels as i32, rows as i32, glow::RGBA, glow::FLOAT,
                    glow::PixelUnpackData::Slice(Some(bytes(&self.bone_upload[..rows * self.bone_texels * 4]))));
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
                let program = &programs[core.instance_programs.get(p).copied().unwrap_or(p)];
                state.use_program(gl, stats, program.program);
                gl.uniform_1_i32(program.instance_base.as_ref(), i as i32);
                let geo = &geometries[g];
                // A geometry whose upload came up short draws nothing (core.rs buffer_upload).
                if geo.short { i = j; continue; }
                state.bind_texture(gl, stats, UNIT_MAP, Some(textures.get(key[3] as usize).copied().unwrap_or(self.empty)));
                if key[6] != u32::MAX {
                    state.bind_texture(gl, stats, UNIT_BUMP, Some(textures.get(key[6] as usize).copied().unwrap_or(self.empty)));
                }
                state.set_state(gl, stats, key[4] & 3, key[4] & 4 != 0, key[4] & 8 != 0);
                // Mirrored objects' faces wind the other way (three's frontFaceCW).
                state.front_face(gl, stats, if key[4] & 16 != 0 { glow::CW } else { glow::CCW });
                gl.bind_vertex_array(Some(geo.vao));
                let (start, count) = if key[5] == 0 || geo.groups.is_empty() { (0, geo.index_count) } else { geo.groups[key[2] as usize] };
                gl.draw_elements_instanced(glow::TRIANGLES, count, geo.index_type, start * geo.index_bytes, (j - i) as i32);
                core.profile.draw(count.max(0) as u64,(j-i) as u64,1);
                stats.gl_calls += 3;
                stats.instance_draws += 1;
                stats.pass_draws[PASS_INSTANCES] += 1;
                i = j;
            }
            stats.instances = self.draws.len() as u32;
        }
    }
}

// The blending rigs and parts as a source of the transparent list's items: each one
// item with the key the client wrote for it (z as three works it out, from
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
        for item in run {
            self.draw_blended(core, item.record as usize);
        }
    }
}

impl<C: HasContext> InstancePass<C> {
    // The k-th blending rig or part of the records drawn last (its row and
    // bones uploaded then): each of its material's passes with its blending,
    // depth and culling as three sets them. The transparent list draws them
    // in its order; the first-person view in its own (view.rs).
    pub fn draw_blended<M: MultiDraw>(&mut self, core: &mut Core<C, M>, k: usize) {
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let (programs, geometries, textures) = (&core.programs, &core.geometries, &core.textures);
        state.bind_texture(gl, stats, UNIT_INSTANCES, Some(self.instance_texture));
        state.bind_texture(gl, stats, UNIT_BONES, Some(self.bone_texture));
        state.color_mask(gl, stats, [true; 4]);
        state.alpha_to_coverage(gl, stats, false);
        {
            let Some(b) = self.blended.get(k) else { return };
            let (Some(blend), Some(geo)) = (self.blends.get(b.blend), geometries.get(b.geometry)) else { return };
            if geo.short { return; }
            state.bind_texture(gl, stats, UNIT_MAP, Some(textures.get(b.texture as usize).copied().unwrap_or(self.empty)));
            if b.bump != u32::MAX {
                state.bind_texture(gl, stats, UNIT_BUMP, Some(textures.get(b.bump as usize).copied().unwrap_or(self.empty)));
            }
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
                let Some(program) = programs.get(core.instance_programs.get(p).copied().unwrap_or(p)) else { continue };
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
                core.profile.draw(geo.index_count.max(0) as u64,1,1);
                stats.pass_draws[PASS_INSTANCES] += 1;
            }
        }
    }

    pub(crate) fn order_far_blends(&mut self, screen: &math::Mat4) {
        self.far_blended_order.clear();
        for (i, part) in self.blended.iter().enumerate() {
            let center = [part.key[2], part.key[3], part.key[4]];
            let z = math::sphere_to_world(screen, &center, 0.).0[2];
            self.far_blended_order.push((i, z));
        }
        let parts = &self.blended;
        self.far_blended_order.sort_unstable_by(|&(a, za), &(b, zb)| {
            crate::transparent::compare(&parts[a].painter_key(za), &parts[b].painter_key(zb))
                .then_with(|| a.cmp(&b))
        });
    }

    pub(crate) fn far_blend(&self, at: usize) -> Option<(usize, [f64; 4])> {
        let &(i, z) = self.far_blended_order.get(at)?;
        Some((i, self.blended[i].painter_key(z)))
    }

}

#[allow(clippy::too_many_arguments)]
fn pack_instance_batch(
    input: &[f64],
    bones: &[f32],
    camera: &Camera,
    records: usize,
    geometry_count: usize,
    program_count: usize,
    blend_count: usize,
    max_rows: usize,
    max_bones: usize,
    draws: &mut Vec<([u32; 7], usize, usize)>,
    templates: &mut Vec<[f32; ROW_TEXELS * 4]>,
    blended_out: &mut Vec<Blended>,
    blended_templates: &mut Vec<usize>,
    rows: &mut [f32],
    bone_upload: &mut [f32],
    overflow: &mut InstanceOverflow,
) -> (usize, usize) {
    overflow.peak_records = overflow.peak_records.max(records);
    if records > MAX_RECORDS {
        overflow.note_record_drop(records - MAX_RECORDS);
    }
    let view = &camera.view;
    let frustum = &camera.frustum;
    // Past the fade end the actor shaders paint the far scene's image instead of the surface (world.rs).
    let fade_end = crate::world::fade_end(input);
    let light_count = camera.light_count;
    let lights = &input[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
    draws.clear();
    templates.clear();
    blended_out.clear();
    blended_templates.clear();
    let mut bone_cursor = 0usize;
    for r in 0..records.min(MAX_RECORDS) {
        let rec = &input[IN_RECORDS + r * REC_STRIDE..IN_RECORDS + (r + 1) * REC_STRIDE];
        let mut world = [0.0; 16];
        world.copy_from_slice(&rec[REC_WORLD..REC_WORLD + 16]);
        let (center, radius) = math::sphere_to_world(&world, &[rec[REC_SPHERE], rec[REC_SPHERE + 1], rec[REC_SPHERE + 2]], rec[REC_SPHERE + 3]);
        if !frustum.sphere(&center, radius) || crate::world::sphere_beyond(view, &center, radius, fade_end) {
            continue;
        }
        let blended = rec[REC_KIND] == KIND_BLENDED || rec[REC_KIND] == KIND_BLENDED_RIGID;
        let skinned = rec[REC_KIND] == 1.0 || rec[REC_KIND] == KIND_BLENDED;
        let bone_count = rec[REC_BONES] as usize;
        if skinned && (bone_cursor + bone_count > max_bones || (rec[REC_BONE_AT] as usize + bone_count) * 16 > bones.len()) {
            overflow.note_bone_drop(1);
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
            // The bind inverse and bind matrices go into each bone row here (bindInverse * bone * bind);
            // the vertex stage takes both as the identity (shader.rs INSTANCE_VERTEX_LOAD). In f32 inputs,
            // as the shader multiplied them.
            let inverse: [f64; 16] = std::array::from_fn(|k| rec[REC_BIND_INVERSE + k] as f32 as f64);
            let bind: [f64; 16] = std::array::from_fn(|k| rec[REC_BIND + k] as f32 as f64);
            for b in 0..bone_count {
                let from = &bones[at + b * 16..at + (b + 1) * 16];
                let bone: [f64; 16] = std::array::from_fn(|k| from[k] as f64);
                let folded = math::mul(&math::mul(&inverse, &bone), &bind);
                for k in 0..16 {
                    bone_upload[(bone_cursor + b) * 16 + k] = folded[k] as f32;
                }
            }
            bone_cursor += bone_count;
        }
        let template = templates.len();
        templates.push(row);
        if (rec[REC_GEOMETRY] as usize) >= geometry_count || (!blended && (rec[REC_PROGRAM] as usize) >= program_count) {
            continue;
        }
        let bump = if rec[REC_BUMP] > 0.0 { rec[REC_BUMP] as u32 - 1 } else { u32::MAX };
        if blended {
            let b = rec[REC_BLEND] as usize;
            if b < blend_count {
                if blended_out.len() < MAX_BLENDED {
                    let texture = if rec[REC_TEXTURES] < 0.0 { u32::MAX } else { rec[REC_TEXTURES] as u32 };
                    let key = [rec[REC_BLEND + 1], rec[REC_BLEND + 2], center[0], center[1], center[2], rec[REC_BLEND + 3]];
                    blended_out.push(Blended { blend: b, row: 0, key, geometry: rec[REC_GEOMETRY] as usize, texture, bump, mirrored: math::mirrors(&world) });
                    blended_templates.push(template);
                } else {
                    overflow.note_blended_drop(1);
                }
            }
            continue;
        }
        let mirrored = math::mirrors(&world) as u32;
        let groups = (rec[REC_GROUPS] as usize).max(1);
        for g in 0..groups.min(4) {
            // No texture (none below 0): nothing bound on the map's unit.
            let texture = if rec[REC_TEXTURES + g] < 0.0 { u32::MAX } else { rec[REC_TEXTURES + g] as u32 };
            let key = [rec[REC_PROGRAM] as u32, rec[REC_GEOMETRY] as u32, g as u32, texture, (rec[REC_FLAGS] as u32 & 15) | mirrored << 4, rec[REC_GROUPS] as u32, bump];
            if draws.len() < max_rows {
                draws.push((key, template, g));
            } else {
                overflow.note_row_drop(1);
            }
        }
    }
    if draws.is_empty() && blended_out.is_empty() {
        return (0, bone_cursor);
    }
    // (d.1, d.2) is strictly increasing in insertion order, so an allocation-free
    // unstable sort on (key, template, group) preserves exact tie order.
    draws.sort_unstable_by_key(|d| (d.0, d.1, d.2));
    for (i, d) in draws.iter().enumerate() {
        rows[i * ROW_TEXELS * 4..(i + 1) * ROW_TEXELS * 4].copy_from_slice(&templates[d.1]);
    }
    // The blending rigs' and parts' rows after the others'.
    let mut used = draws.len();
    let total_blended = blended_out.len();
    for (b, &template) in blended_out.iter_mut().zip(blended_templates.iter()) {
        if used >= max_rows {
            break;
        }
        b.row = used;
        rows[used * ROW_TEXELS * 4..(used + 1) * ROW_TEXELS * 4].copy_from_slice(&templates[template]);
        used += 1;
    }
    let kept_blended = used - draws.len();
    if kept_blended < total_blended {
        // Charge each omitted blended item once against the limiting bank (rows).
        overflow.note_row_drop(total_blended - kept_blended);
    }
    blended_out.truncate(kept_blended);
    overflow.peak_rows = overflow.peak_rows.max(used);
    overflow.peak_blended = overflow.peak_blended.max(blended_out.len());
    overflow.peak_bones = overflow.peak_bones.max(bone_cursor);
    (used, bone_cursor)
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    fn identity_camera() -> Camera {
        let id = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        Camera {
            view: id,
            frustum: math::Frustum::from_matrix(&id),
            light_count: 0,
        }
    }

    fn write_record(input: &mut [f64], index: usize, kind: f64, groups: usize) {
        let row = &mut input[IN_RECORDS + index * REC_STRIDE..IN_RECORDS + (index + 1) * REC_STRIDE];
        row.fill(0.0);
        row[REC_KIND] = kind;
        row[REC_PROGRAM] = 0.0;
        row[REC_GEOMETRY] = 0.0;
        row[REC_GROUPS] = groups as f64;
        for g in 0..4 {
            row[REC_TEXTURES + g] = -1.0;
        }
        row[REC_SPHERE + 3] = 0.1;
        row[REC_WORLD] = 1.0;
        row[REC_WORLD + 5] = 1.0;
        row[REC_WORLD + 10] = 1.0;
        row[REC_WORLD + 15] = 1.0;
        row[REC_BLEND] = 0.0;
    }

    #[test]
    fn scene_with_more_than_256_records_and_64_blended_draws_them_all() {
        let opaque_count = 320usize;
        let blended_count = 96usize;
        let total = opaque_count + blended_count;
        assert!(total <= MAX_RECORDS);
        assert!(blended_count <= MAX_BLENDED);
        assert!(total <= MAX_ROWS);

        let mut input = vec![0.0; INPUT_LEN];
        input[shader::FRAME_OFFSETS[18]] = 0.0;
        input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
        for i in 0..opaque_count {
            write_record(&mut input, i, 2.0, 1);
        }
        for i in 0..blended_count {
            write_record(&mut input, opaque_count + i, KIND_BLENDED_RIGID, 1);
        }

        let camera = identity_camera();
        let bones = vec![0.0f32; BONES_LEN];
        let mut draws = Vec::with_capacity(MAX_ROWS);
        let mut templates = Vec::with_capacity(MAX_RECORDS);
        let mut blended = Vec::with_capacity(MAX_BLENDED);
        let mut blended_templates = Vec::with_capacity(MAX_BLENDED);
        let mut rows = vec![0.0f32; MAX_ROWS * ROW_TEXELS * 4];
        let mut bone_upload = vec![0.0f32; MAX_BONES * 16];
        let mut overflow = InstanceOverflow::default();
        overflow.begin_frame();

        let (used, _) = pack_instance_batch(
            &input, &bones, &camera, total, 1, 1, 1, MAX_ROWS, MAX_BONES,
            &mut draws, &mut templates, &mut blended, &mut blended_templates,
            &mut rows, &mut bone_upload, &mut overflow,
        );

        assert_eq!(draws.len(), opaque_count);
        assert_eq!(blended.len(), blended_count);
        assert_eq!(used, total);
        assert_eq!(overflow.total_drops(), 0);
        assert!(!overflow.finish_frame());
        assert_eq!(overflow.peak_records, total);
        assert_eq!(overflow.peak_rows, total);
        assert_eq!(overflow.peak_blended, blended_count);
    }

    #[test]
    fn overflow_beyond_limits_is_counted_never_silent() {
        let camera = identity_camera();
        let bones = vec![0.0f32; BONES_LEN];
        let mut draws = Vec::with_capacity(MAX_ROWS);
        let mut templates = Vec::with_capacity(MAX_RECORDS);
        let mut blended = Vec::with_capacity(MAX_BLENDED);
        let mut blended_templates = Vec::with_capacity(MAX_BLENDED);
        let mut rows = vec![0.0f32; MAX_ROWS * ROW_TEXELS * 4];
        let mut bone_upload = vec![0.0f32; MAX_BONES * 16];
        let mut overflow = InstanceOverflow::default();

        // Exceed MAX_RECORDS and MAX_ROWS (via 4 groups per record).
        let mut input = vec![0.0; INPUT_LEN];
        input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
        for i in 0..MAX_RECORDS {
            write_record(&mut input, i, 2.0, 4);
        }
        overflow.begin_frame();
        let (used, _) = pack_instance_batch(
            &input, &bones, &camera, MAX_RECORDS + 5, 1, 1, 1, MAX_ROWS, MAX_BONES,
            &mut draws, &mut templates, &mut blended, &mut blended_templates,
            &mut rows, &mut bone_upload, &mut overflow,
        );
        assert_eq!(used, MAX_ROWS);
        assert_eq!(overflow.dropped_records, 5);
        assert_eq!(overflow.dropped_rows, (MAX_RECORDS * 4 - MAX_ROWS) as u32);
        assert!(overflow.finish_frame());
        assert_eq!(overflow.overflow_frames, 1);

        // Exceed MAX_BLENDED on the immediately following frame: counted, but
        // throttled from stderr until LOG_INTERVAL_FRAMES elapse.
        for i in 0..(MAX_BLENDED + 7) {
            write_record(&mut input, i, KIND_BLENDED_RIGID, 1);
        }
        overflow.begin_frame();
        let (used_b, _) = pack_instance_batch(
            &input, &bones, &camera, MAX_BLENDED + 7, 1, 1, 1, MAX_ROWS, MAX_BONES,
            &mut draws, &mut templates, &mut blended, &mut blended_templates,
            &mut rows, &mut bone_upload, &mut overflow,
        );
        assert_eq!(blended.len(), MAX_BLENDED);
        assert_eq!(used_b, MAX_BLENDED);
        assert_eq!(overflow.dropped_blended, 7);
        assert!(!overflow.finish_frame());
        assert_eq!(overflow.overflow_frames, 2);
    }

    #[test]
    fn the_drop_counts_saturate_at_their_bound() {
        let mut overflow = InstanceOverflow {
            dropped_records: u32::MAX - 1,
            dropped_rows: u32::MAX,
            overflow_frames: u32::MAX,
            ..InstanceOverflow::default()
        };
        overflow.begin_frame();
        overflow.note_record_drop(5);
        overflow.note_row_drop(usize::MAX);
        overflow.note_bone_drop(3);
        assert_eq!(overflow.dropped_records, u32::MAX, "a sum past the bound stays at it");
        assert_eq!(overflow.dropped_rows, u32::MAX);
        assert_eq!(overflow.frame_drops, u32::MAX, "the frame total saturates too");
        assert_eq!(overflow.total_drops(), u32::MAX, "and so does the total of all kinds");
        assert!(overflow.finish_frame());
        assert_eq!(overflow.overflow_frames, u32::MAX);
    }
}
