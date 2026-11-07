// The near scene's terrain water (water.js): the flat surface over the water
// cells, in Classic settings drawn with the look's own textures (Low: the
// textured pass), at High and Ultra in two passes (the tint over the opaque
// scene's copy, then the mirror's reflection); the rings spreading where
// things meet the water; and at High and Ultra the shore's lapping and
// splashes (water-shore.js). All blend, so they are items of the
// transparent list (transparent.rs Source): each with three's sort key (its
// group's and its own render order, z of its geometry's bounding sphere,
// its id) and drawn in the list's order, with three's program for its
// material as three compiled it (Kind::Plain: the water keeps its own
// `time`).
//
// A surface draws a geometry the page registered once (core.rs); the rings
// are a mesh of their own whose vertices change, mirrored here (`rings_*`,
// as water.js lays them out): the page copies in the vertices that changed.
// The shore's two meshes draw one quad instanced, each instance's values
// from buffers of an instance set (`instances`, laid out by the page), which
// the page fills again when three would upload them.
// Each frame the page writes each surface's values (SURFACE f64 each):
// whether three would draw it, its sort key, the sphere's centre, its draw
// range, its uniforms (post.rs UNIFORM each) and its one uniform array (the
// waders, read by the High and Ultra passes).
use crate::core::{bytes, Core, MultiDraw, PASS_WATER};
use crate::math::Mat4;
use crate::post::{set_uniform, UNIFORM};
use crate::transparent::{apply, three_z, DrawState, Item, Source, View};
use glow::HasContext;
use std::collections::HashMap;

// A surface's values (f64): shown, program, group order, render order, id,
// the sphere's centre (x, y, z), its world matrix (16), the first index and
// how many, how many uniforms, then the uniforms; then its uniform array:
// the program's slot for it (-1: none), floats an element, elements, and
// their values.
pub const S_SHOWN: usize = 0;
pub const S_PROGRAM: usize = 1;
pub const S_KEY: usize = 2; // group order, render order, id
pub const S_CENTER: usize = 5;
pub const S_WORLD: usize = 8;
pub const S_RANGE: usize = 24; // first index, count
pub const S_UNIFORMS: usize = 26;
pub const S_FIRST: usize = 27;
pub const MAX_UNIFORMS: usize = 32;
pub const S_ARRAY: usize = S_FIRST + MAX_UNIFORMS * UNIFORM;
pub const ARRAY_FLOATS: usize = 32;
pub const SURFACE: usize = S_ARRAY + 3 + ARRAY_FLOATS;
pub const MAX_SURFACES: usize = 8;
// The rings' vertex arrays (water.js ringMesh: RIPPLE_MAX x RIPPLE_SEGMENTS
// quads): position 3, uv 2, alpha 1 a vertex, and where the program reads
// each; the vertices changed [start, end) follow the surfaces' values.
pub const RING_VERTICES: usize = 100 * 40 * 4;
const RING_SIZES: [usize; 3] = [3, 2, 1];
pub const RING_LOCATIONS: [u32; 3] = [0, 1, 2];
pub const IN_RING_UPLOAD: usize = MAX_SURFACES * SURFACE;
// The instance sets' state after it: per set, whether the page wrote its
// values anew (1: upload them) and how many instances draw.
pub const IN_SETS: usize = IN_RING_UPLOAD + 2;
pub const MAX_SETS: usize = 2;
pub const INPUT_LEN: usize = IN_SETS + 2 * MAX_SETS;
// The instance sets' values (f32), each attribute's for up to `max`
// instances one after another, the sets one after another.
pub const INSTANCE_FLOATS: usize = 4096;

enum Shape<C: HasContext> {
    // A geometry the core holds.
    Geometry(usize),
    // The rings' own.
    Rings(C::VertexArray),
}

struct Surface<C: HasContext> {
    shape: Shape<C>,
    index_type: u32,
    index_bytes: i32,
    state: DrawState,
    side: u32,
    // Its instances' set, for one drawn instanced.
    set: Option<usize>,
}

// Per-instance attributes the page fills (water-shore.js's shore and
// phase): a buffer each, (buffer, floats an instance, where its values
// start in `instances`), for up to `max` instances.
struct Set<C: HasContext> {
    attributes: Vec<(C::Buffer, usize, usize)>,
    max: usize,
}

pub struct WaterPass<C: HasContext> {
    surfaces: Vec<Surface<C>>,
    pub input: Vec<f64>,
    pub rings_position: Vec<f32>,
    pub rings_uv: Vec<f32>,
    pub rings_alpha: Vec<f32>,
    ring_buffers: Vec<C::Buffer>,
    pub instances: Vec<f32>,
    sets: Vec<Set<C>>,
    matrices: HashMap<(usize, usize), [f32; 16]>,
    // The array each program holds, by (program, slot).
    arrays: HashMap<(usize, usize), Vec<f32>>,
    array: Vec<f32>,
}

impl<C: HasContext> WaterPass<C> {
    // A restored context has fresh buffers but the CPU mirrors are current.
    pub fn reupload<M: MultiDraw>(&self, core: &mut Core<C, M>) {
        for (k, data) in [&self.rings_position, &self.rings_uv, &self.rings_alpha].iter().enumerate() {
            if let Some(buffer) = self.ring_buffers.get(k) { unsafe {
                core.gl.bind_buffer(glow::ARRAY_BUFFER, Some(*buffer));
                crate::core::spare_view_slot();
                core.gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes(data));
            }}
        }
        unsafe { core.gl.bind_buffer(glow::ARRAY_BUFFER, None); }
    }

    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> WaterPass<C> {
        WaterPass {
            surfaces: vec![], input: vec![0.0; INPUT_LEN], rings_position: vec![], rings_uv: vec![], rings_alpha: vec![], ring_buffers: vec![],
            instances: vec![0.0; INSTANCE_FLOATS], sets: vec![], matrices: HashMap::new(), arrays: HashMap::new(), array: vec![],
        }
    }

    // A surface drawing a geometry the page registered (core.rs); blend,
    // depth function and flags as transparent.rs DrawState takes them; side
    // as transparent.rs apply().
    pub fn add_surface<M: MultiDraw>(&mut self, core: &Core<C, M>, geometry: u32, blend: &[u32], depth_func: u32, flags: u32, side: u32) -> u32 {
        let g = &core.geometries[geometry as usize];
        self.surfaces.push(Surface {
            shape: Shape::Geometry(geometry as usize), index_type: g.index_type, index_bytes: g.index_bytes, state: DrawState::from(blend, depth_func, flags), side, set: None,
        });
        (self.surfaces.len() - 1) as u32
    }

    // The rings: their own arrays (mirrored here) and index, once.
    pub fn add_rings<M: MultiDraw>(&mut self, core: &Core<C, M>, index: &[u32], blend: &[u32], depth_func: u32, flags: u32, side: u32) -> u32 {
        let gl = &core.gl;
        self.rings_position = vec![0.0; RING_VERTICES * 3];
        self.rings_uv = vec![0.0; RING_VERTICES * 2];
        self.rings_alpha = vec![0.0; RING_VERTICES];
        let vao = unsafe {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            for (k, size) in RING_SIZES.iter().enumerate() {
                let b = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                gl.buffer_data_size(glow::ARRAY_BUFFER, (RING_VERTICES * size * 4) as i32, glow::DYNAMIC_DRAW);
                gl.enable_vertex_attrib_array(RING_LOCATIONS[k]);
                gl.vertex_attrib_pointer_f32(RING_LOCATIONS[k], *size as i32, glow::FLOAT, false, 0, 0);
                self.ring_buffers.push(b);
            }
            let ib = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ib));
            crate::core::spare_view_slot();
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(index), glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            self.ring_buffers.push(ib);
            vao
        };
        self.surfaces.push(Surface { shape: Shape::Rings(vao), index_type: glow::UNSIGNED_INT, index_bytes: 4, state: DrawState::from(blend, depth_func, flags), side, set: None });
        (self.surfaces.len() - 1) as u32
    }

    // An instance set: a buffer for each attribute (sizes: floats an
    // instance), for up to `max` instances, its values at `at` in
    // `instances` (each attribute's after the last's). None when there is no
    // room.
    pub fn add_set<M: MultiDraw>(&mut self, core: &Core<C, M>, max: u32, sizes: &[u32]) -> Option<u32> {
        let max = max as usize;
        let mut at = 0;
        for set in &self.sets {
            for &(_, size, from) in &set.attributes {
                at = at.max(from + size * set.max);
            }
        }
        if self.sets.len() >= MAX_SETS || at + max * sizes.iter().map(|&s| s as usize).sum::<usize>() > INSTANCE_FLOATS {
            return None;
        }
        let gl = &core.gl;
        let mut attributes = vec![];
        for &size in sizes {
            let size = size as usize;
            let buffer = unsafe {
                let b = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                gl.buffer_data_size(glow::ARRAY_BUFFER, (max * size * 4) as i32, glow::DYNAMIC_DRAW);
                gl.bind_buffer(glow::ARRAY_BUFFER, None);
                b
            };
            attributes.push((buffer, size, at));
            at += max * size;
        }
        self.sets.push(Set { attributes, max });
        Some((self.sets.len() - 1) as u32)
    }

    // A surface drawn instanced: attribute k of `set` read at `location` of
    // its vertex array, one value an instance.
    pub fn instanced<M: MultiDraw>(&mut self, core: &Core<C, M>, surface: u32, set: u32, k: u32, location: u32) {
        let s = &mut self.surfaces[surface as usize];
        let Shape::Geometry(g) = s.shape else { return };
        let (buffer, size, _) = self.sets[set as usize].attributes[k as usize];
        let gl = &core.gl;
        unsafe {
            gl.bind_vertex_array(Some(core.geometries[g].vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_f32(location, size as i32, glow::FLOAT, false, 0, 0);
            gl.vertex_attrib_divisor(location, 1);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        s.set = Some(set as usize);
    }

    fn values(&self, s: usize) -> &[f64] {
        &self.input[s * SURFACE..(s + 1) * SURFACE]
    }
}

impl<C: HasContext, M: MultiDraw> Source<C, M> for WaterPass<C> {
    fn emit(&mut self, core: &mut Core<C, M>, view: &View, source: u32, items: &mut Vec<Item>) {
        // The rings' vertices that changed go up first (before any draw).
        let (start, end) = ((self.input[IN_RING_UPLOAD] as usize).min(RING_VERTICES), (self.input[IN_RING_UPLOAD + 1] as usize).min(RING_VERTICES));
        if end > start && self.ring_buffers.len() == 4 {
            let gl = &core.gl;
            for (k, data) in [&self.rings_position, &self.rings_uv, &self.rings_alpha].iter().enumerate() {
                let size = RING_SIZES[k];
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.ring_buffers[k]));
                    crate::core::spare_view_slot();
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, (start * size * 4) as i32, bytes(&data[start * size..end * size]));
                }
                core.stats.gl_calls += 2;
            }
            unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, None) };
            core.stats.gl_calls += 1;
            self.input[IN_RING_UPLOAD] = 0.0;
            self.input[IN_RING_UPLOAD + 1] = 0.0;
        }
        // The instance sets the page wrote anew, as three uploads them.
        for (k, set) in self.sets.iter().enumerate() {
            let at = IN_SETS + 2 * k;
            if self.input[at] == 0.0 {
                continue;
            }
            self.input[at] = 0.0;
            let count = (self.input[at + 1].max(0.0) as usize).min(set.max);
            if count == 0 {
                continue;
            }
            let gl = &core.gl;
            for &(buffer, size, from) in &set.attributes {
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                    crate::core::spare_view_slot();
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes(&self.instances[from..from + count * size]));
                }
                core.stats.gl_calls += 2;
            }
            unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, None) };
            core.stats.gl_calls += 1;
        }
        for s in 0..self.surfaces.len().min(MAX_SURFACES) {
            let v = self.values(s);
            if v[S_SHOWN] == 0.0 || v[S_RANGE + 1] <= 0.0 {
                continue;
            }
            if let Some(set) = self.surfaces[s].set {
                if self.input[IN_SETS + 2 * set + 1] <= 0.0 {
                    continue;
                }
            }
            let mut world: Mat4 = [0.0; 16];
            world.copy_from_slice(&v[S_WORLD..S_WORLD + 16]);
            items.push(Item {
                group_order: v[S_KEY], render_order: v[S_KEY + 1], id: v[S_KEY + 2],
                z: three_z(&view.view_projection, Some(&world), &[v[S_CENTER], v[S_CENTER + 1], v[S_CENTER + 2]]),
                source, batch: s as u32, record: s as u32,
            });
        }
    }

    fn draw_run(&mut self, core: &mut Core<C, M>, _view: &View, run: &[Item]) {
        for item in run {
            let s = item.record as usize;
            let surface = &self.surfaces[s];
            let v = &self.input[s * SURFACE..(s + 1) * SURFACE];
            let program = v[S_PROGRAM] as usize;
            if program >= core.programs.len() {
                continue;
            }
            apply(core, &surface.state, surface.side);
            core.state.use_program(&core.gl, &mut core.stats, core.programs[program].program);
            let mut unit = 0;
            let n = (v[S_UNIFORMS] as usize).min(MAX_UNIFORMS);
            for k in 0..n {
                let at = S_FIRST + k * UNIFORM;
                set_uniform(core, &mut self.matrices, program, &v[at..at + UNIFORM], &mut unit);
            }
            // Its uniform array, when it differs from what the program holds.
            let slot = v[S_ARRAY];
            if slot >= 0.0 && (slot as usize) < core.programs[program].uniforms.len() {
                let (width, count) = (v[S_ARRAY + 1] as usize, v[S_ARRAY + 2] as usize);
                let floats = (width * count).min(ARRAY_FLOATS);
                self.array.clear();
                self.array.extend(v[S_ARRAY + 3..S_ARRAY + 3 + floats].iter().map(|&x| x as f32));
                let key = (program, slot as usize);
                if self.arrays.get(&key) != Some(&self.array) {
                    let location = core.programs[program].uniforms[slot as usize].1.clone();
                    unsafe {
                        match width {
                            1 => core.gl.uniform_1_f32_slice(location.as_ref(), &self.array),
                            2 => core.gl.uniform_2_f32_slice(location.as_ref(), &self.array),
                            3 => core.gl.uniform_3_f32_slice(location.as_ref(), &self.array),
                            _ => core.gl.uniform_4_f32_slice(location.as_ref(), &self.array),
                        }
                    }
                    self.arrays.insert(key, self.array.clone());
                    core.stats.gl_calls += 1;
                }
            }
            let vao = match surface.shape {
                Shape::Geometry(g) => core.geometries[g].vao,
                Shape::Rings(vao) => vao,
            };
            let (count, first) = (v[S_RANGE + 1] as i32, v[S_RANGE] as i32 * surface.index_bytes);
            unsafe {
                core.gl.bind_vertex_array(Some(vao));
                match surface.set {
                    Some(set) => {
                        let instances = (self.input[IN_SETS + 2 * set + 1].max(0.0) as usize).min(self.sets[set].max);
                        core.gl.draw_elements_instanced(glow::TRIANGLES, count, surface.index_type, first, instances as i32);
                    }
                    None => core.gl.draw_elements(glow::TRIANGLES, count, surface.index_type, first),
                }
            }
            core.stats.gl_calls += 2;
            core.stats.pass_draws[PASS_WATER] += 1;
        }
    }
}
