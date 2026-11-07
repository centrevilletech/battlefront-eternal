// The Enhanced tree canopies (tree-canopies.js): the leaves the near jungle
// backdrop's models are rebuilt with near the eye, one camera-facing card an
// instance, drawn in the near phase (and a mirror's) before the opaque world,
// with three's program for their material (the frame block's camera, time
// and fog: the World kind) and its state: both sides, alpha to coverage,
// depth tested and written, no blending.
//
// The page selects the groups whose leaves draw (TreeCanopies.update, for
// each camera that renders the near scene) and writes their cards into
// slots of three instanced attributes (centre, size, shade). The renderer
// keeps its own copy of them, a buffer an attribute: each frame the page
// copies the instances whose slots changed since it last copied them into
// `leaves` (each attribute's values at its instances' places) and names
// their range in the input, which the pass uploads before it draws.
use crate::core::{bytes, Core, MultiDraw, PASS_CANOPIES, UNIT_MAP};
use glow::HasContext;

// The input (f64): whether three would draw the mesh this frame, its
// program, its texture (-1: none), how many instances draw, and the
// instances the page copied this frame [first, end).
pub const C_SHOWN: usize = 0;
pub const C_PROGRAM: usize = 1;
pub const C_TEXTURE: usize = 2;
pub const C_INSTANCES: usize = 3;
pub const C_COPIED: usize = 4;
pub const INPUT_LEN: usize = 6;

struct Mesh<C: HasContext> {
    geometry: usize,
    // A buffer an attribute: (buffer, floats an instance, where its values
    // start in `leaves`).
    attributes: Vec<(C::Buffer, usize, usize)>,
    max: usize,
    count: i32,
}

pub struct CanopiesPass<C: HasContext> {
    pub input: Vec<f64>,
    pub leaves: Vec<f32>,
    mesh: Option<Mesh<C>>,
}

impl<C: HasContext> CanopiesPass<C> {
    // A restored context has fresh buffers but the CPU mirrors are current.
    pub fn reupload<M: MultiDraw>(&self, core: &mut Core<C, M>) {
        if let Some(mesh) = &self.mesh { for &(buffer, size, at) in &mesh.attributes { unsafe {
            core.gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            crate::core::spare_view_slot();
            core.gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes(&self.leaves[at..at + mesh.max * size]));
        }}}
        unsafe { core.gl.bind_buffer(glow::ARRAY_BUFFER, None); }
    }

    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> CanopiesPass<C> {
        CanopiesPass { input: vec![0.0; INPUT_LEN], leaves: vec![], mesh: None }
    }

    // The canopies' mesh: a geometry the page registered (core.rs: its card's
    // corners and index), and its per-instance attributes, read at
    // `locations`, `sizes` floats an instance, for up to `max` instances.
    pub fn set_mesh<M: MultiDraw>(&mut self, core: &Core<C, M>, geometry: u32, locations: &[u32], sizes: &[u32], max: u32) {
        let gl = &core.gl;
        let g = &core.geometries[geometry as usize];
        let max = max as usize;
        let mut attributes = vec![];
        let mut at = 0;
        unsafe {
            gl.bind_vertex_array(Some(g.vao));
            for (&location, &size) in locations.iter().zip(sizes) {
                let size = size as usize;
                let b = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                gl.buffer_data_size(glow::ARRAY_BUFFER, (max * size * 4) as i32, glow::DYNAMIC_DRAW);
                gl.enable_vertex_attrib_array(location);
                gl.vertex_attrib_pointer_f32(location, size as i32, glow::FLOAT, false, 0, 0);
                gl.vertex_attrib_divisor(location, 1);
                attributes.push((b, size, at));
                at += max * size;
            }
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        self.leaves = vec![0.0; at];
        self.mesh = Some(Mesh { geometry: geometry as usize, attributes, max, count: g.index_count });
    }

    // The near phase's part (renderer.rs frame, a mirror's too): the
    // instances the page copied, uploaded, then the cards drawn.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        let Some(mesh) = &self.mesh else { return };
        let input = &self.input;
        let (first, end) = ((input[C_COPIED].max(0.0) as usize).min(mesh.max), (input[C_COPIED + 1].max(0.0) as usize).min(mesh.max));
        if end > first {
            let gl = &core.gl;
            for &(buffer, size, at) in &mesh.attributes {
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                    crate::core::spare_view_slot();
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, (first * size * 4) as i32, bytes(&self.leaves[at + first * size..at + end * size]));
                }
                core.stats.gl_calls += 2;
            }
            unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, None) };
            core.stats.gl_calls += 1;
            self.input[C_COPIED] = 0.0;
            self.input[C_COPIED + 1] = 0.0;
        }
        let input = &self.input;
        let instances = (input[C_INSTANCES].max(0.0) as usize).min(mesh.max);
        let program = input[C_PROGRAM] as usize;
        if input[C_SHOWN] == 0.0 || instances == 0 || program >= core.programs.len() {
            return;
        }
        let texture = input[C_TEXTURE];
        let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
        state.use_program(gl, stats, core.programs[program].program);
        state.bind_texture(gl, stats, UNIT_MAP, if texture >= 0.0 { core.textures.get(texture as usize).copied() } else { None });
        // three's setMaterial for it: both sides, alpha to coverage, depth
        // tested and written (the phase's less or equal), no blending.
        state.set_state(gl, stats, 2, true, true);
        state.front_face(gl, stats, glow::CCW);
        state.blend(gl, stats, false);
        state.depth_test(gl, stats, true);
        state.depth_func(gl, stats, glow::LEQUAL);
        state.color_mask(gl, stats, [true; 4]);
        let g = &core.geometries[mesh.geometry];
        unsafe {
            gl.bind_vertex_array(Some(g.vao));
            gl.draw_elements_instanced(glow::TRIANGLES, mesh.count, g.index_type, 0, instances as i32);
        }
        stats.gl_calls += 2;
        stats.pass_draws[PASS_CANOPIES] += 1;
    }
}
