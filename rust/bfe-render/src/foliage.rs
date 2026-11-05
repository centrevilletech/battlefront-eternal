// The near scene's foliage (foliage.js, foliage-gpu.js): each class's cards
// (a grasspatch's or a leafpatch's), one instanced draw a class over the
// patches in view, drawn in the near phase with three's program for the
// class (its card shader: cards turned to the camera, swaying, fading with
// distance, in the map's fog).
//
// The patches are chosen here, as FoliageGpuBatch.update chooses them, when
// the page's Foliage.update runs (select): a patch passes when its sphere
// is in the camera's frustum, it is within its cull distance across the
// ground, and its depth along the view is within its fade. Only the cells of
// the patch index around the camera are tested (foliage-gpu.js patchIndex:
// the patches with a cull distance in cells as wide as the longest one, the
// rest always tested), the selection kept in patch order. When it changes,
// the selected patches' rows (position, tint, the patch's axes, its own
// distance) go up in one buffer for the next draw. What the card shader reads
// besides is the camera as it was at the selection (the eye, its right, up
// and back), the animation time and the near scene's end, so a frame drawn
// without an update (paused) draws as three would: the last selection.
use crate::core::{bytes, Core, MultiDraw, Value, PASS_FOLIAGE, UNIT_MAP};
use crate::math::{self, Frustum};
use glow::HasContext;
use std::collections::HashMap;

// The page's values for a selection (f64 slots of `input`): the camera's
// projection, its inverse world matrix and its world matrix, its position,
// the near scene's end, the foliage's animation time.
pub const IN_PROJECTION: usize = 0;
pub const IN_VIEW: usize = 16;
pub const IN_WORLD: usize = 32;
pub const IN_EYE: usize = 48;
pub const IN_END: usize = 51;
pub const IN_TIME: usize = 52;
pub const INPUT_LEN: usize = 53;

// A patch's row in the instance buffer (f32): its position, tint, the three
// axes of its matrix, and its own distance (0: the near scene's end).
pub const ROW: usize = 16;
// A patch's bounds as the page gives them (f64): position, sphere centre and
// radius (radius < 0: none), cull distance (infinite: always tested), max
// distance.
pub const BOUNDS: usize = 9;
// A card vertex as the page gives it (f32): position 3, color 4, uv 2,
// corner 2, shape 4, rate 1.
pub const VERTEX: usize = 16;
const VERTEX_PARTS: [i32; 6] = [3, 4, 2, 2, 4, 1];
const ROW_PARTS: [i32; 6] = [3, 3, 3, 3, 3, 1];

// foliage-gpu.js patchIndex: the smallest cell.
const MIN_CELL: f64 = 8.0;

struct Patch {
    position: [f64; 3],
    sphere: Option<([f64; 3], f64)>,
    cull: f64,
    max: f64,
}

// The authored distance and view-depth rules are distinct for grass and leaves.
fn visible(p: &Patch, frustum: &Frustum, eye: [f64; 3], back: [f64; 3], near_end: f64, grass: bool) -> bool {
    let max = if p.max != 0.0 && !p.max.is_nan() { p.max } else { near_end };
    if let Some((c, r)) = &p.sphere { if !frustum.sphere(c, *r) { return false; } }
    if (p.position[0] - eye[0]).hypot(p.position[2] - eye[2]) > p.cull { return false; }
    let d = [p.position[0] - eye[0], p.position[1] - eye[1], p.position[2] - eye[2]];
    let depth = -(d[0] * back[0] + d[1] * back[1] + d[2] * back[2]);
    !(if grass { depth > max || depth < -5.0 } else { depth > max + 20.0 || depth < -25.0 })
}


struct Class<C: HasContext> {
    program: usize,
    texture: C::Texture,
    grass: bool,
    wiggle: f32,
    vao: C::VertexArray,
    _buffers: Vec<C::Buffer>,
    rows_buffer: C::Buffer,
    index_count: i32,
    // Uniforms the pass sets by location (not values the core caches):
    // modelMatrix is read by three's card program under shadows (its world
    // position), the identity (the cards are in the world).
    model_view: Option<C::UniformLocation>,
    map_transform: Option<C::UniformLocation>,
    model: Option<C::UniformLocation>,
    map_matrix: [f32; 9],
    // Material values (slots of the program's uniforms), constant.
    constants: Vec<(usize, Value)>,
    patches: Vec<Patch>,
    rows: Vec<f32>,
    cell: f64,
    cells: HashMap<(i64, i64), Vec<u32>>,
    always: Vec<u32>,
    selected: Vec<u32>,
    next: Vec<u32>,
    upload: Vec<f32>,
    dirty: bool,
    // The camera, time and end as the last selection saw them.
    eye: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    back: [f32; 3],
    time: f32,
    end: f32,
}

// Slots of a program's material uniforms (core.rs Program.uniforms) the card
// shaders read, by name; None where the program does not read it.
struct Slots {
    eye: Option<usize>,
    right: Option<usize>,
    up: Option<usize>,
    back: Option<usize>,
    time: Option<usize>,
    end: Option<usize>,
    wiggle: Option<usize>,
    fog_range: Option<usize>,
    fog_color: Option<usize>,
}

pub struct FoliagePass<C: HasContext> {
    classes: Vec<Class<C>>,
    // Off: three draws the foliage (the page's switch for this pass alone).
    pub input: Vec<f64>,
    // The view matrix each program last had (modelViewMatrix: the cards are
    // in world space, so it is the camera's), by program.
    model_views: HashMap<usize, [f32; 16]>,
    map_matrices: HashMap<usize, [f32; 9]>,
    // The programs whose modelMatrix holds the identity.
    models: std::collections::HashSet<usize>,
}

fn slot<C: HasContext, M: MultiDraw>(core: &Core<C, M>, program: usize, name: &str) -> Option<usize> {
    core.programs[program].uniforms.iter().position(|(n, _, _)| n == name)
}

// A material value into its program, when it differs from what the program holds.
fn set<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>, program: usize, slot: Option<usize>, value: Value) {
    let Some(slot) = slot else { return };
    let gl = &core.gl;
    let u = &mut core.programs[program].uniforms[slot];
    if u.2 == Some(value) {
        return;
    }
    unsafe {
        match value {
            Value::F(1, v) => gl.uniform_1_f32(u.1.as_ref(), v[0]),
            Value::F(2, v) => gl.uniform_2_f32(u.1.as_ref(), v[0], v[1]),
            Value::F(3, v) => gl.uniform_3_f32(u.1.as_ref(), v[0], v[1], v[2]),
            Value::F(_, v) => gl.uniform_4_f32(u.1.as_ref(), v[0], v[1], v[2], v[3]),
            Value::I(i) => gl.uniform_1_i32(u.1.as_ref(), i),
        }
    }
    u.2 = Some(value);
    core.stats.gl_calls += 1;
}

fn vec3(v: [f32; 3]) -> Value {
    Value::F(3, [v[0], v[1], v[2], 0.0])
}

fn float(v: f32) -> Value {
    Value::F(1, [v, 0.0, 0.0, 0.0])
}

impl<C: HasContext> FoliagePass<C> {
    pub fn replace_texture(&mut self, old: C::Texture, new: C::Texture) {
        for class in &mut self.classes { if class.texture == old { class.texture = new; } }
    }

    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> FoliagePass<C> {
        FoliagePass { classes: vec![], input: vec![0.0; INPUT_LEN], model_views: HashMap::new(), map_matrices: HashMap::new(), models: std::collections::HashSet::new() }
    }

    // A class: three's program for its cards (core.add_program's), its
    // texture, whether it is grass, its wiggle; its cards' vertices (VERTEX
    // f32 each) and index; where the program reads each vertex part and
    // each row part (attribute locations, -1 where it reads none); its
    // patches' rows (ROW f32 each) and bounds (BOUNDS f64 each); the map's
    // uv transform (mat3, column-major) and the material's colour, opacity
    // and alpha test.
    #[allow(clippy::too_many_arguments)]
    pub fn add_class<M: MultiDraw>(&mut self, core: &mut Core<C, M>, program: u32, texture: u32, grass: bool, wiggle: f32, vertices: &[f32], index: &[u32],
        locations: &[i32], rows: &[f32], bounds: &[f64], map_matrix: &[f32], diffuse: &[f32], opacity: f32, alpha_test: f32) -> u32 {
        let program = program as usize;
        let gl = &core.gl;
        let p = core.programs[program].program;
        let (vao, buffers, rows_buffer) = unsafe {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            let vertex_buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertex_buffer));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(vertices), glow::STATIC_DRAW);
            let mut offset = 0;
            for (k, size) in VERTEX_PARTS.iter().enumerate() {
                if let Some(&l) = locations.get(k) {
                    if l >= 0 {
                        gl.enable_vertex_attrib_array(l as u32);
                        gl.vertex_attrib_pointer_f32(l as u32, *size, glow::FLOAT, false, (VERTEX * 4) as i32, offset * 4);
                    }
                }
                offset += size;
            }
            let rows_buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(rows_buffer));
            gl.buffer_data_size(glow::ARRAY_BUFFER, (bounds.len() / BOUNDS * ROW * 4).max(ROW * 4) as i32, glow::DYNAMIC_DRAW);
            let mut offset = 0;
            for (k, size) in ROW_PARTS.iter().enumerate() {
                if let Some(&l) = locations.get(VERTEX_PARTS.len() + k) {
                    if l >= 0 {
                        gl.enable_vertex_attrib_array(l as u32);
                        gl.vertex_attrib_pointer_f32(l as u32, *size, glow::FLOAT, false, (ROW * 4) as i32, offset * 4);
                        gl.vertex_attrib_divisor(l as u32, 1);
                    }
                }
                offset += size;
            }
            let index_buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(index_buffer));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(index), glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            (vao, vec![vertex_buffer, index_buffer], rows_buffer)
        };
        let (model_view, map_transform, model) = unsafe { (gl.get_uniform_location(p, "modelViewMatrix"), gl.get_uniform_location(p, "mapTransform"), gl.get_uniform_location(p, "modelMatrix")) };
        let mut map = [0.0f32; 9];
        map[..map_matrix.len().min(9)].copy_from_slice(&map_matrix[..map_matrix.len().min(9)]);
        let mut constants = vec![];
        let mut constant = |name: &str, value: Value| {
            if let Some(s) = slot(core, program, name) {
                constants.push((s, value));
            }
        };
        constant("diffuse", Value::F(3, [diffuse[0], diffuse[1], diffuse[2], 0.0]));
        constant("opacity", float(opacity));
        constant("alphaTest", float(alpha_test));
        // patchIndex: cells as wide as the longest cull distance.
        let mut patches = Vec::with_capacity(bounds.len() / BOUNDS);
        let mut reach = 0.0f64;
        for b in bounds.as_chunks::<BOUNDS>().0 {
            let sphere = if b[6] < 0.0 { None } else { Some(([b[3], b[4], b[5]], b[6])) };
            if b[7].is_finite() && b[7] > reach {
                reach = b[7];
            }
            patches.push(Patch { position: [b[0], b[1], b[2]], sphere, cull: b[7], max: b[8] });
        }
        let cell = MIN_CELL.max(reach);
        let mut cells: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        let mut always = vec![];
        for (i, p) in patches.iter().enumerate() {
            if !p.cull.is_finite() {
                always.push(i as u32);
                continue;
            }
            cells.entry(((p.position[0] / cell).floor() as i64, (p.position[2] / cell).floor() as i64)).or_default().push(i as u32);
        }
        let n = patches.len();
        self.classes.push(Class {
            program, texture: core.textures[texture as usize], grass, wiggle, vao, _buffers: buffers, rows_buffer, index_count: index.len() as i32,
            model_view, map_transform, model, map_matrix: map, constants, patches, rows: rows.to_vec(), cell, cells, always,
            selected: Vec::with_capacity(n), next: Vec::with_capacity(n), upload: vec![], dirty: false,
            eye: [0.0; 3], right: [0.0; 3], up: [0.0; 3], back: [0.0; 3], time: 0.0, end: 0.0,
        });
        (self.classes.len() - 1) as u32
    }

    // FoliageGpuBatch.update for every class, from the page's values in
    // `input`; returns how many patches were selected.
    pub fn select(&mut self) -> u32 {
        let input = &self.input;
        let mut projection = [0.0; 16];
        let mut view = [0.0; 16];
        projection.copy_from_slice(&input[IN_PROJECTION..IN_PROJECTION + 16]);
        view.copy_from_slice(&input[IN_VIEW..IN_VIEW + 16]);
        let frustum = Frustum::from_matrix(&math::mul(&projection, &view));
        let w: &[f64] = &input[IN_WORLD..IN_WORLD + 16];
        let eye = [input[IN_EYE], input[IN_EYE + 1], input[IN_EYE + 2]];
        let back = [w[8], w[9], w[10]];
        let near_end = input[IN_END];
        let f = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];
        let mut total = 0;
        for class in &mut self.classes {
            class.eye = f(eye);
            class.right = f([w[0], w[1], w[2]]);
            class.up = f([w[4], w[5], w[6]]);
            class.back = f(back);
            class.time = input[IN_TIME] as f32;
            class.end = near_end as f32;
            let mut next = std::mem::take(&mut class.next);
            next.clear();
            let test = |i: u32, out: &mut Vec<u32>| {
                if visible(&class.patches[i as usize], &frustum, eye, back, near_end, class.grass) { out.push(i); }
            };
            for &i in &class.always {
                test(i, &mut next);
            }
            let (cx, cz) = ((eye[0] / class.cell).floor() as i64, (eye[2] / class.cell).floor() as i64);
            for x in cx - 1..=cx + 1 {
                for z in cz - 1..=cz + 1 {
                    if let Some(list) = class.cells.get(&(x, z)) {
                        for &i in list {
                            test(i, &mut next);
                        }
                    }
                }
            }
            if !class.cells.is_empty() {
                next.sort_unstable();
            }
            if next != class.selected {
                class.upload.clear();
                for &i in &next {
                    class.upload.extend_from_slice(&class.rows[i as usize * ROW..(i as usize + 1) * ROW]);
                }
                class.dirty = true;
                std::mem::swap(&mut class.selected, &mut next);
            }
            class.next = next;
            total += class.selected.len() as u32;
        }
        total
    }

    // Each class's cards over its selected patches, as three draws them:
    // depth tested and written, both sides, alpha to coverage, no blending.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        let mut view_matrix = [0.0f32; 16];
        view_matrix.copy_from_slice(&core.frame[16..32]);
        let fog_range = Value::F(2, [core.frame[68], core.frame[69], 0.0, 0.0]);
        let fog_color = Value::F(3, [core.frame[60], core.frame[61], core.frame[62], 0.0]);
        for class in &mut self.classes {
            if class.selected.is_empty() {
                continue;
            }
            let program = class.program;
            core.state.use_program(&core.gl, &mut core.stats, core.programs[program].program);
            let slots = Slots {
                eye: slot(core, program, "foliageEye"), right: slot(core, program, "foliageRight"), up: slot(core, program, "foliageUp"),
                back: slot(core, program, "foliageBack"), time: slot(core, program, "foliageTime"), end: slot(core, program, "foliageEnd"),
                wiggle: slot(core, program, "foliageWiggle"), fog_range: slot(core, program, "foliageFogRange"), fog_color: slot(core, program, "foliageFogColor"),
            };
            for &(s, v) in &class.constants {
                set(core, program, Some(s), v);
            }
            set(core, program, slots.eye, vec3(class.eye));
            set(core, program, slots.right, vec3(class.right));
            set(core, program, slots.up, vec3(class.up));
            set(core, program, slots.back, vec3(class.back));
            set(core, program, slots.time, float(class.time));
            set(core, program, slots.end, float(class.end));
            set(core, program, slots.wiggle, float(class.wiggle));
            set(core, program, slots.fog_range, fog_range);
            set(core, program, slots.fog_color, fog_color);
            let gl = &core.gl;
            unsafe {
                if self.model_views.get(&program) != Some(&view_matrix) {
                    gl.uniform_matrix_4_f32_slice(class.model_view.as_ref(), false, &view_matrix);
                    self.model_views.insert(program, view_matrix);
                    core.stats.gl_calls += 1;
                }
                if self.map_matrices.get(&program) != Some(&class.map_matrix) {
                    gl.uniform_matrix_3_f32_slice(class.map_transform.as_ref(), false, &class.map_matrix);
                    self.map_matrices.insert(program, class.map_matrix);
                    core.stats.gl_calls += 1;
                }
                if class.model.is_some() && self.models.insert(program) {
                    gl.uniform_matrix_4_f32_slice(class.model.as_ref(), false, &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
                    core.stats.gl_calls += 1;
                }
            }
            core.state.bind_texture(gl, &mut core.stats, UNIT_MAP, Some(class.texture));
            core.state.set_state(gl, &mut core.stats, 2, true, true);
            unsafe {
                gl.bind_vertex_array(Some(class.vao));
                if class.dirty {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(class.rows_buffer));
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes(&class.upload));
                    gl.bind_buffer(glow::ARRAY_BUFFER, None);
                    class.dirty = false;
                    core.stats.gl_calls += 3;
                }
                gl.draw_elements_instanced(glow::TRIANGLES, class.index_count, glow::UNSIGNED_INT, 0, class.selected.len() as i32);
            }
            core.stats.gl_calls += 2;
            core.stats.pass_draws[PASS_FOLIAGE] += 1;
        }
    }

    // Programs are made again after a context loss or a settings change
    // (three's), so a class's cached values for one are forgotten with it.
    pub fn forget(&mut self) {
        self.model_views.clear();
        self.map_matrices.clear();
        self.models.clear();
    }

    pub fn classes(&self) -> usize {
        self.classes.len()
    }

    pub fn selected(&self, class: u32) -> u32 {
        self.classes.get(class as usize).map_or(0, |c| c.selected.len() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grass_and_leaf_distance_boundaries_keep_authored_rules() {
        let frustum = Frustum::from_matrix(&[1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0]);
        let mut patch = Patch { position: [0.0,0.0,-50.0], sphere: None, cull: f64::INFINITY, max: 20.0 };
        let test = |p: &Patch, grass| visible(p, &frustum, [0.0;3], [0.0,0.0,1.0], 100.0, grass);
        assert!(!test(&patch, true));
        patch.max = 0.0;
        assert!(test(&patch, true));
        patch.max = 30.0;
        assert!(test(&patch, false));
        patch.position[2] = -50.0001;
        assert!(!test(&patch, false));
        patch.position[2] = 5.0;
        assert!(test(&patch, true));
        patch.position[2] = 5.0001;
        assert!(!test(&patch, true));
        assert!(test(&patch, false));
        patch.cull = 5.0;
        assert!(!test(&patch, false));
        patch.cull = f64::INFINITY;
        patch.sphere = Some(([10.0,0.0,0.0], 1.0));
        assert!(!test(&patch, false));
    }
}
