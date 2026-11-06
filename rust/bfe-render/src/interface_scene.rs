// InterfaceSurface.drawFrame, ClassicView's room/models and SpawnStage's
// room/projector/holomap share this phase. The page publishes values and retained
// resource ids; Rust sorts the draw records, selects raster state and draws.
// No Three program, reflection or draw is needed, including after context loss.
use crate::core::{Core, MultiDraw, PASS_HUD};
use crate::post::set_uniform;
use glow::HasContext;
use std::collections::HashMap;

#[path = "interface_layout.rs"]
pub mod layout;
use layout::*;

// Flags are orthogonal to the side field. Transparent records sort after
// opaque records; Basic projector pieces retain their two face passes.
pub const DEPTH_TEST: u32 = 1;
pub const DEPTH_WRITE: u32 = 2;
pub const BLEND: u32 = 4;
pub const TRANSPARENT: u32 = 8;
pub const TWO_PASS: u32 = 16;

pub struct InterfacePass {
    pub input: Vec<f64>,
    pub streams: Vec<Stream>,
    order: Vec<usize>,
    matrices: HashMap<(usize, usize), [f32; 16]>,
}

// Dynamic quads and motes publish into persistent WASM byte mirrors. The
// journal recreates allocation/layout; these current bytes are moved across
// context loss and reuploaded after replay, without retaining an old GL object.
pub struct Stream { pub geometry: u32, pub buffer: u32, pub data: Vec<u8> }

impl Default for InterfacePass {
    fn default() -> Self {
        Self { input: vec![0.0; MAX_DRAWS * STRIDE], streams: Vec::new(), order: Vec::with_capacity(MAX_DRAWS), matrices: HashMap::new() }
    }
}

impl InterfacePass {
    // InterfacePass.upload's cold resource registration. Stream ids never
    // change with camera or material grouping, just like retained geometry ids.
    pub fn add_stream(&mut self, geometry: u32, buffer: u32, data: &[u8]) -> u32 {
        // A cold attribute-array replacement resizes this same GPU buffer.
        // Reuse its mirror slot too: replaying an obsolete, larger mirror into
        // the resized allocation would fail before the current bytes upload.
        for (id, stream) in self.streams.iter_mut().enumerate() {
            if stream.geometry == geometry && stream.buffer == buffer {
                stream.data.clear();
                stream.data.extend_from_slice(data);
                return id as u32;
            }
        }
        let id = self.streams.len() as u32;
        self.streams.push(Stream { geometry, buffer, data: data.to_vec() });
        id
    }

    pub fn reupload<C: HasContext, M: MultiDraw>(&self, core: &mut Core<C, M>) {
        for stream in &self.streams {
            core.geometry_buffer_sub_data(stream.geometry, stream.buffer, 0, &stream.data);
        }
    }

    // WebGLRenderLists' opaque/transparent comparators, with input position as
    // the stable tie-break for material groups belonging to the same object.
    // sort_unstable requires no temporary allocation once the phase is warm.
    fn sort(&mut self, draws: usize) {
        self.order.clear();
        self.order.extend(0..draws.min(MAX_DRAWS));
        let input = &self.input;
        self.order.sort_unstable_by(|&a, &b| {
            let (x, y) = (&input[a * STRIDE..], &input[b * STRIDE..]);
            let tx = x[FLAGS] as u32 & TRANSPARENT != 0;
            let ty = y[FLAGS] as u32 & TRANSPARENT != 0;
            tx.cmp(&ty)
                .then_with(|| x[GROUP_ORDER].total_cmp(&y[GROUP_ORDER]))
                .then_with(|| x[RENDER_ORDER].total_cmp(&y[RENDER_ORDER]))
                .then_with(|| if tx { y[DEPTH].total_cmp(&x[DEPTH]) } else {
                    x[MATERIAL].total_cmp(&y[MATERIAL]).then_with(|| x[DEPTH].total_cmp(&y[DEPTH]))
                })
                .then_with(|| x[OBJECT].total_cmp(&y[OBJECT]))
                .then(a.cmp(&b))
        });
    }

    // renderInterface's replacement preserves the currently bound framebuffer
    // and viewport. This includes the holomap's offscreen target and the canvas
    // alpha used over shell movies. Clearing stays at the caller's layer boundary.
    pub fn draw<C: HasContext, M: MultiDraw>(&mut self, core: &mut Core<C, M>, draws: usize) {
        self.sort(draws);
        for &row in &self.order {
            let r = &self.input[row * STRIDE..(row + 1) * STRIDE];
            let (program, geometry) = (r[PROGRAM] as usize, r[GEOMETRY] as usize);
            if program >= core.programs.len() || geometry >= core.geometries.len() || r[COUNT] <= 0.0 { continue; }
            let flags = r[FLAGS] as u32;
            let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
            state.use_program(gl, stats, core.programs[program].program);
            state.depth_test(gl, stats, flags & DEPTH_TEST != 0);
            state.depth_write(gl, stats, flags & DEPTH_WRITE != 0);
            state.depth_func(gl, stats, r[DEPTH_FUNC] as u32);
            state.blend(gl, stats, flags & BLEND != 0);
            if flags & BLEND != 0 {
                state.blend_equation(gl, stats, r[EQUATION_RGB] as u32, r[EQUATION_ALPHA] as u32);
                state.blend_func(gl, stats, r[SRC_RGB] as u32, r[DST_RGB] as u32, r[SRC_ALPHA] as u32, r[DST_ALPHA] as u32);
            }
            state.color_mask(gl, stats, [true; 4]);
            state.front_face(gl, stats, if r[FLIP] != 0.0 { glow::CW } else { glow::CCW });
            unsafe {
                gl.disable(glow::POLYGON_OFFSET_FILL);
                gl.disable(glow::STENCIL_TEST);
                gl.disable(glow::SCISSOR_TEST);
                gl.disable(glow::SAMPLE_ALPHA_TO_COVERAGE);
            }
            stats.gl_calls += 4;
            let mut unit = 0;
            for k in 0..(r[UNIFORMS] as usize).min(MAX_UNIFORMS) {
                let at = VALUES + k * UNIFORM;
                set_uniform(core, &mut self.matrices, program, &r[at..at + UNIFORM], &mut unit);
            }
            let passes = if flags & TWO_PASS != 0 { 2 } else { 1 };
            for pass in 0..passes {
                let side = if passes == 2 { 1 - pass } else { r[SIDE] as u32 };
                core.state.set_state(&core.gl, &mut core.stats, side, false, flags & DEPTH_WRITE != 0);
                let geo = &core.geometries[geometry];
                unsafe {
                    core.gl.bind_vertex_array(Some(geo.vao));
                    if geo.indexed {
                        core.gl.draw_elements(r[PRIMITIVE] as u32, r[COUNT] as i32, geo.index_type, r[FIRST] as i32 * geo.index_bytes);
                    } else {
                        core.gl.draw_arrays(r[PRIMITIVE] as u32, r[FIRST] as i32, r[COUNT] as i32);
                    }
                }
                core.stats.gl_calls += 2;
                core.stats.pass_draws[PASS_HUD] += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layers_material_groups_and_transparent_depth_keep_interface_order() {
        let mut pass = InterfacePass::default();
        for (i, (flags, layer, depth, material)) in [
            (TRANSPARENT, 2., 5., 1.), (0, 0., 9., 2.),
            (TRANSPARENT, 2., 8., 2.), (0, 0., 2., 1.),
            (TRANSPARENT, 1., 1., 3.), (TRANSPARENT, 2., 8., 2.),
        ].iter().enumerate() {
            let r = &mut pass.input[i * STRIDE..];
            r[FLAGS] = *flags as f64; r[RENDER_ORDER] = *layer;
            r[DEPTH] = *depth; r[MATERIAL] = *material;
        }
        pass.sort(6);
        assert_eq!(pass.order, [3, 1, 4, 2, 5, 0]);
        pass.sort(0);
        assert!(pass.order.is_empty());
    }

    #[test]
    fn resized_attributes_keep_only_the_current_recovery_mirror() {
        let mut pass = InterfacePass::default();
        let id = pass.add_stream(3, 0, &[1, 2, 3, 4]);
        assert_eq!(pass.add_stream(3, 0, &[8, 9]), id);
        assert_eq!(pass.streams.len(), 1);
        assert_eq!(pass.streams[id as usize].data, [8, 9]);
    }
}
