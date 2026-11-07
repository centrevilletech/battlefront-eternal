// The first-person view scene: the arms and the weapon in view, drawn last
// over the near scene after a depth clear, with its own camera (near 0.3 m,
// far 10 m) and frame block (VIEW_FIRST_PERSON: the view camera, and the
// point lights packed for the view scene, which holds a copy of each of the
// near scene's: Graphics.syncViewLights).
//
// Each part is a rigid mesh hung from a bone of the arms' rig (or the gun in
// its hand), drawn as the soldiers' guns are (instances.rs): a record a part
// the page wrote, instanced by model, texture and program, its matrices,
// look and lights in a row of the instance texture. Its draws count as this
// pass's.
//
// Then what the view blends (the muzzle flash on the gun), as three's
// transparent list draws it: the page writes each draw in three's order
// (DRAW f64 each): three's program for its material as three compiled it
// (Kind::Plain), its geometry, blending and state (transparent.rs DrawState),
// the sides it is drawn with in turn, its index range and every uniform its
// program reads (post.rs UNIFORM each). A part that blends whose material
// is a zero material (its lights and lamps per instance) is a record of its
// own kind (instances.rs KIND_BLENDED_RIGID): its draw is that record's
// (program below 0, D_GEOMETRY the k-th of the records that blend), in the
// same order.
use crate::core::{Camera, Core, MultiDraw, PASS_INSTANCES, PASS_VIEW};
use crate::instances::InstancePass;
use crate::effects::EffectsPass;
use crate::post::{set_uniform, UNIFORM};
use crate::transparent::{apply, DrawState};
use glow::HasContext;
use std::collections::HashMap;
use std::marker::PhantomData;

// A blended draw's values (f64): program, geometry, blend (7), depth
// function, flags (transparent.rs DrawState), how many sides and each (two
// slots: back faces then front for a double-sided material), the first index
// and how many, how many uniforms, then the uniforms.
pub const D_PROGRAM: usize = 0;
pub const D_GEOMETRY: usize = 1;
pub const D_BLEND: usize = 2;
pub const D_DEPTH_FUNC: usize = 9;
pub const D_FLAGS: usize = 10;
pub const D_SIDES: usize = 11; // how many, then two
pub const D_RANGE: usize = 14;
pub const D_UNIFORMS: usize = 16;
pub const D_FIRST: usize = 17;
// A draw whose program is VIEW_BATCH is an effects batch drawn in view order,
// not a program draw: it reuses the geometry, range and uniform-count slots
// for the batch id, its first instance and count, and its capacity.
pub const VIEW_BATCH: f64 = -2.0;
pub const D_BATCH: usize = D_GEOMETRY;
pub const D_BATCH_AT: usize = D_RANGE;
pub const D_BATCH_COUNT: usize = D_RANGE + 1;
pub const D_BATCH_CAPACITY: usize = D_UNIFORMS;
pub const MAX_UNIFORMS: usize = 32;
pub const DRAW: usize = D_FIRST + MAX_UNIFORMS * UNIFORM;
pub const MAX_DRAWS: usize = 16;
// How many draws the page wrote, then the draws.
pub const IN_DRAWS: usize = 0;
pub const IN_FIRST_DRAW: usize = 1;
pub const INPUT_LEN: usize = IN_FIRST_DRAW + MAX_DRAWS * DRAW;

pub struct ViewPass<C: HasContext> {
    gl: PhantomData<C>,
    pub input: Vec<f64>,
    matrices: HashMap<(usize, usize), [f32; 16]>,
}

impl<C: HasContext> ViewPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> ViewPass<C> {
        ViewPass { gl: PhantomData, input: vec![0.0; INPUT_LEN], matrices: HashMap::new() }
    }

    // records: how many the page wrote for the view's parts.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, camera: &Camera, instances: &mut InstancePass<C>, effects: &mut EffectsPass<C>, records: usize) {
        if records > 0 {
            let before = core.stats.pass_draws[PASS_INSTANCES];
            instances.draw(core, camera, records);
            let drawn = core.stats.pass_draws[PASS_INSTANCES] - before;
            core.stats.pass_draws[PASS_INSTANCES] = before;
            core.stats.pass_draws[PASS_VIEW] += drawn;
        }
        let draws = (self.input[IN_DRAWS] as usize).min(MAX_DRAWS);
        for d in 0..draws {
            let v = &self.input[IN_FIRST_DRAW + d * DRAW..IN_FIRST_DRAW + (d + 1) * DRAW];
            if v[D_PROGRAM]==crate::actor_layout::kind::VIEW_MODEL as f64 {effects.draw_view_model(core,camera,v[D_GEOMETRY] as usize,v[D_RANGE] as usize,v[D_RANGE+1] as usize);continue;}
            if v[D_PROGRAM] == VIEW_BATCH {
                effects.draw_view_batch(core, v[D_BATCH] as usize, v[D_BATCH_COUNT] as usize, v[D_BATCH_AT] as usize, v[D_BATCH_CAPACITY] as usize);
                continue;
            }
            if v[D_PROGRAM] < 0.0 {
                let before = core.stats.pass_draws[PASS_INSTANCES];
                instances.draw_blended(core, v[D_GEOMETRY] as usize);
                let drawn = core.stats.pass_draws[PASS_INSTANCES] - before;
                core.stats.pass_draws[PASS_INSTANCES] = before;
                core.stats.pass_draws[PASS_VIEW] += drawn;
                continue;
            }
            let (program, geometry) = (v[D_PROGRAM] as usize, v[D_GEOMETRY] as usize);
            if program >= core.programs.len() || geometry >= core.geometries.len() || v[D_RANGE + 1] <= 0.0 {
                continue;
            }
            let mut blend = [0u32; 7];
            for k in 0..7 {
                blend[k] = v[D_BLEND + k] as u32;
            }
            let state = DrawState::from(&blend, v[D_DEPTH_FUNC] as u32, v[D_FLAGS] as u32);
            core.state.use_program(&core.gl, &mut core.stats, core.programs[program].program);
            let mut unit = 0;
            for k in 0..(v[D_UNIFORMS] as usize).min(MAX_UNIFORMS) {
                let at = D_FIRST + k * UNIFORM;
                set_uniform(core, &mut self.matrices, program, &v[at..at + UNIFORM], &mut unit);
            }
            let (vao, index_type, index_bytes) = {
                let g = &core.geometries[geometry];
                (g.vao, g.index_type, g.index_bytes)
            };
            for s in 0..(v[D_SIDES] as usize).min(2) {
                apply(core, &state, v[D_SIDES + 1 + s] as u32);
                unsafe {
                    core.gl.bind_vertex_array(Some(vao));
                    core.gl.draw_elements(glow::TRIANGLES, v[D_RANGE + 1] as i32, index_type, v[D_RANGE] as i32 * index_bytes);
                }
                core.stats.gl_calls += 2;
                core.stats.pass_draws[PASS_VIEW] += 1;
            }
        }
        self.input[IN_DRAWS] = 0.0;
    }
}
