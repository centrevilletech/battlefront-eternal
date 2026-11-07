// The sun's shadow cascades, drawn by the renderer in place of three's shadow
// pass: graphics.js draws each cascade's static cache the frame its window
// lands and, every frame (the last cascade every other one), its live map:
// the cache's depth copied in, then the moving casters over it. For each such
// pass the page walks the scene as three's shadow pass walks it
// (src/rust-render/shadow.js) and writes a draw a caster: three's program for
// its depth material, used as three compiled it (Kind::Plain: every uniform
// the page's), its geometry and the range three would draw, the side its
// material casts with, whether its matrix mirrors, its uniforms' values.
//
// The pass draws into a framebuffer of its own over the map's depth texture
// (three's shadow target; receivers read only its depth), cleared to the far
// plane or given the cache's depth, in three's shadow state: no blending,
// depth tested and written, no colour, the draw's culling.
use crate::core::{Core, MultiDraw, PASS_SHADOW};
use crate::post::{set_uniform, UNIFORM};
use glow::HasContext;
use std::collections::HashMap;
use std::marker::PhantomData;

// The pass's values (f64): the depth texture drawn into, the one copied from
// first (below 0: none, the map is cleared), the map's size (square), how many
// draws, then the draws.
pub const S_TARGET: usize = 0;
pub const S_FROM: usize = 1;
pub const S_SIZE: usize = 2;
pub const S_DRAWS: usize = 3;
pub const S_FIRST: usize = 4;
// A draw: program, geometry, first index, how many (indices, or vertices
// for a geometry without an index), side (0 front faces, 1 back, 2 both),
// mirrored (1: its world matrix mirrors: front faces wind clockwise), how
// many uniforms, then the uniforms (post.rs set_uniform's layout).
pub const D_PROGRAM: usize = 0;
pub const D_GEOMETRY: usize = 1;
pub const D_START: usize = 2;
pub const D_COUNT: usize = 3;
pub const D_SIDE: usize = 4;
pub const D_MIRRORED: usize = 5;
pub const D_UNIFORMS: usize = 6;
pub const D_FIRST: usize = 7;
pub const MAX_UNIFORMS: usize = 8;
pub const DRAW: usize = D_FIRST + MAX_UNIFORMS * UNIFORM;
pub const MAX_DRAWS: usize = 256;
pub const INPUT_LEN: usize = S_FIRST + MAX_DRAWS * DRAW;

pub struct ShadowPass<C: HasContext> {
    gl: PhantomData<C>,
    pub input: Vec<f64>,
    pub static_view: Vec<f64>,
    pub statics: crate::static_depth::Bank,
    // Matrices each program holds, by (program, slot) (post.rs set_uniform).
    matrices: HashMap<(usize, usize), [f32; 16]>,
}

impl<C: HasContext> ShadowPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> ShadowPass<C> {
        ShadowPass { gl: PhantomData, input: vec![0.0; INPUT_LEN], static_view: vec![0.;crate::static_layout::SHADOW_LENGTH], statics: crate::static_depth::Bank::default(), matrices: HashMap::new() }
    }

    // Content-sized native descriptor commands share the existing draw stream;
    // reserve their slots cold so a view never resizes either side's arrays.
    pub fn reserve_static(&mut self) {
        let count=MAX_DRAWS+(self.statics.sources.len()+2).min(crate::static_depth::MAX_RECORDS);
        self.input.resize(S_FIRST+count*DRAW,0.);
        self.matrices.reserve(64);
    }

    // One cascade pass, as the page wrote it.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>) -> Result<(), String> {
        let v = &self.input;
        let target = v[S_TARGET];
        if target < 0.0 {
            return Ok(());
        }
        let size = v[S_SIZE] as i32;
        let fb = core.framebuffer(None, Some(target as u32))?;
        if v[S_FROM] >= 0.0 {
            // graphics.js copyDepth: the cache's depth in place of a clear.
            let from = core.framebuffer(None, Some(v[S_FROM] as u32))?;
            core.blit_depth(from, fb, size, size);
            core.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        }
        core.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        // Three's shadow state: no blending or scissor, depth tested (less or
        // equal) and written, no colour (the depth materials write none).
        {
            let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
            unsafe {
                gl.viewport(0, 0, size, size);
                gl.disable(glow::SCISSOR_TEST);
                gl.disable(glow::POLYGON_OFFSET_FILL);
                gl.disable(glow::STENCIL_TEST);
            }
            stats.gl_calls += 4;
            state.blend(gl, stats, false);
            state.depth_test(gl, stats, true);
            state.depth_func(gl, stats, glow::LEQUAL);
            state.depth_write(gl, stats, true);
            state.color_mask(gl, stats, [false; 4]);
            state.alpha_to_coverage(gl, stats, false);
            if v[S_FROM] < 0.0 {
                // three clears the map before it draws (renderer.clear()).
                unsafe {
                    gl.clear_depth_f32(1.0);
                    gl.clear(glow::DEPTH_BUFFER_BIT);
                }
                stats.gl_calls += 2;
            }
        }
        let mut projection=[0.;16];projection.copy_from_slice(&self.static_view[..16]);
        let mut view=[0.;16];view.copy_from_slice(&self.static_view[16..32]);
        let static_frustum=crate::math::Frustum::from_matrix(&crate::math::mul(&projection,&view));
        let draws = (v[S_DRAWS] as usize).min((v.len()-S_FIRST)/DRAW);
        for d in 0..draws {
            let at = S_FIRST + d * DRAW;
            if self.input[at+D_PROGRAM]<0. {
                self.statics.draw(core,&mut self.matrices,self.input[at+D_GEOMETRY] as usize,&self.static_view,&static_frustum);
                continue;
            }
            let (program, geometry) = (self.input[at + D_PROGRAM] as usize, self.input[at + D_GEOMETRY] as usize);
            let count = self.input[at + D_COUNT] as i32;
            if program >= core.programs.len() || geometry >= core.geometries.len() || count <= 0 {
                continue;
            }
            core.state.use_program(&core.gl, &mut core.stats, core.programs[program].program);
            let mut unit = 0;
            for k in 0..(self.input[at + D_UNIFORMS] as usize).min(MAX_UNIFORMS) {
                let u = at + D_FIRST + k * UNIFORM;
                let values: [f64; UNIFORM] = self.input[u..u + UNIFORM].try_into().unwrap();
                set_uniform(core, &mut self.matrices, program, &values, &mut unit);
            }
            let side = self.input[at + D_SIDE] as u32;
            let mirrored = self.input[at + D_MIRRORED] != 0.0;
            let start = self.input[at + D_START] as i32;
            let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
            state.cull(gl, stats, side);
            state.front_face(gl, stats, if mirrored { glow::CW } else { glow::CCW });
            let g = &core.geometries[geometry];
            unsafe {
                gl.bind_vertex_array(Some(g.vao));
                if g.index_count > 0 {
                    gl.draw_elements(glow::TRIANGLES, count, g.index_type, start * g.index_bytes);
                } else {
                    gl.draw_arrays(glow::TRIANGLES, start, count);
                }
            }
            stats.gl_calls += 2;
            stats.pass_draws[PASS_SHADOW] += 1;
        }
        // Colour writes back on for whoever draws next (three resets its own).
        core.state.color_mask(&core.gl, &mut core.stats, [true; 4]);
        self.input[S_DRAWS] = 0.0;
        Ok(())
    }
}
