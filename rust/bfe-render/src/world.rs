// The map's static world in the near scene: its opaque buckets, each one
// mesh three would draw with the identity for a model matrix, culled by its
// bounding sphere as three culls it, and the largest by spatial cell as
// render-culling.js does, the visible runs of each drawn by one multi-draw.
// Drawn in three's opaque order (material creation) unless sorted by
// program (?rustSort), each bucket's material values and point lights set
// only when they differ from what its program holds.
use crate::core::{Camera, Core, MultiDraw, Program, Stats, Value, PASS_TERRAIN, PASS_WORLD};
use crate::lights::light_mask;
use crate::shader::{FRAME_LIGHTS, MAX_LIGHTS};
use glow::HasContext;
use std::collections::HashMap;

struct Cells {
    // Consecutive triangles in one cell: [part, first index, end].
    runs: Vec<[u32; 3]>,
    parts: Vec<([f64; 3], [f64; 3])>,
    visible: Vec<u8>,
    offsets: Vec<i32>,
    counts: Vec<i32>,
}

struct World<C: HasContext> {
    geometry: usize,
    program: usize,
    textures: Vec<(u32, C::Texture)>,
    uniforms: Vec<(usize, Value)>,
    cull: u32,
    alpha_to_coverage: bool,
    depth_write: bool,
    point_lit: bool,
    order: u32,
    sphere: ([f64; 3], f64),
    bbox: ([f64; 3], [f64; 3]),
    cells: Option<Cells>,
    shown: bool,
    // The pass its draws count for (the terrain's: PASS_TERRAIN).
    pass: usize,
}

pub struct WorldPass<C: HasContext> {
    world: Vec<World<C>>,
    order: Vec<usize>,
    by_program: bool,
}

impl<C: HasContext> WorldPass<C> {
    pub fn replace_texture(&mut self, old: C::Texture, new: C::Texture) {
        for world in &mut self.world { for (_, texture) in &mut world.textures { if *texture == old { *texture = new; } } }
    }

    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> WorldPass<C> {
        WorldPass { world: vec![], order: vec![], by_program: true }
    }

    // flags: cull side | 4 alpha to coverage | 8 depth write | 16 point lit
    // | 32 the terrain (its draws count for the terrain pass). bounds: sphere
    // centre and radius, then box min and max (world space).
    pub fn add_world(&mut self, geometry: u32, program: u32, order: u32, flags: u32, bounds: &[f64]) -> u32 {
        self.world.push(World {
            geometry: geometry as usize, program: program as usize, textures: vec![], uniforms: vec![],
            cull: flags & 3, alpha_to_coverage: flags & 4 != 0, depth_write: flags & 8 != 0, point_lit: flags & 16 != 0, order,
            sphere: ([bounds[0], bounds[1], bounds[2]], bounds[3]),
            bbox: ([bounds[4], bounds[5], bounds[6]], [bounds[7], bounds[8], bounds[9]]),
            cells: None,
            shown: true,
            pass: if flags & 32 != 0 { PASS_TERRAIN } else { PASS_WORLD },
        });
        (self.world.len() - 1) as u32
    }

    pub fn world_texture<M: MultiDraw>(&mut self, core: &Core<C, M>, w: u32, unit: u32, texture: u32) {
        let t = core.textures[texture as usize];
        self.world[w as usize].textures.push((unit, t));
    }

    // A material value its program reads (by the name add_program was given).
    pub fn world_uniform<M: MultiDraw>(&mut self, core: &Core<C, M>, w: u32, name: &str, values: &[f32], int: bool) -> bool {
        let world = &mut self.world[w as usize];
        let program = &core.programs[world.program];
        let Some(slot) = program.uniforms.iter().position(|(n, _, _)| n == name) else { return false };
        let mut v = [0.0f32; 4];
        v[..values.len().min(4)].copy_from_slice(&values[..values.len().min(4)]);
        world.uniforms.push((slot, if int { Value::I(values[0] as i32) } else { Value::F(values.len(), v) }));
        true
    }

    // A world draw's lamps this frame (a DYNAMIC bucket's, while lamps are
    // on): values [4 lamp table indices (-1: none), count], its program's
    // WORLD_LAMP_UNIFORMS, set in place of last frame's.
    pub fn world_lamps<M: MultiDraw>(&mut self, core: &Core<C, M>, w: u32, values: &[f32]) {
        let world = &mut self.world[w as usize];
        let program = &core.programs[world.program];
        let mut put = |name: &str, value: Value| {
            if let Some(slot) = program.uniforms.iter().position(|(n, _, _)| n == name) {
                match world.uniforms.iter_mut().find(|(s, _)| *s == slot) {
                    Some(entry) => entry.1 = value,
                    None => world.uniforms.push((slot, value)),
                }
            }
        };
        put("bfeLampIndex", Value::F(4, [values[0], values[1], values[2], values[3]]));
        put("bfeLampCount", Value::F(1, [values[4], 0.0, 0.0, 0.0]));
    }

    // The cells render-culling.js culls a large bucket by: its triangles in
    // 48 m cells by their centres (shadow-casters.js), runs of consecutive
    // triangles in one cell, each cell's box over its vertices.
    pub fn world_cells(&mut self, w: u32, positions: &[f32], index: &[u32], cell_size: f64) {
        let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
        let mut parts: Vec<([f64; 3], [f64; 3])> = vec![];
        let triangles = index.len() / 3;
        let mut cell_of = vec![0u32; triangles];
        let p = |v: u32, a: usize| positions[v as usize * 3 + a] as f64;
        for t in 0..triangles {
            let (a, b, c) = (index[3 * t], index[3 * t + 1], index[3 * t + 2]);
            let x = (p(a, 0) + p(b, 0) + p(c, 0)) / 3.0;
            let z = (p(a, 2) + p(b, 2) + p(c, 2)) / 3.0;
            let key = ((x / cell_size).floor() as i64, (z / cell_size).floor() as i64);
            let id = *ids.entry(key).or_insert_with(|| {
                parts.push(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]));
                parts.len() - 1
            });
            cell_of[t] = id as u32;
            for v in [a, b, c] {
                let (min, max) = &mut parts[id];
                for k in 0..3 {
                    min[k] = min[k].min(p(v, k));
                    max[k] = max[k].max(p(v, k));
                }
            }
        }
        let mut runs = vec![];
        let mut first = 0;
        while first < triangles {
            let cell = cell_of[first];
            let mut end = first + 1;
            while end < triangles && cell_of[end] == cell {
                end += 1;
            }
            runs.push([cell, (first * 3) as u32, (end * 3) as u32]);
            first = end;
        }
        let n = runs.len();
        self.world[w as usize].cells = Some(Cells { visible: vec![0; parts.len()], parts, runs, offsets: vec![0; n], counts: vec![0; n] });
    }

    // Cells given as they are (terrain-patches.js: each cell one range of
    // the index, its triangles' box): ranges [first index, end] and boxes
    // [min, max] per cell, in index order.
    pub fn world_runs(&mut self, w: u32, ranges: &[u32], boxes: &[f64]) {
        let n = ranges.len() / 2;
        let runs = (0..n).map(|i| [i as u32, ranges[2 * i], ranges[2 * i + 1]]).collect();
        let parts = (0..n).map(|i| ([boxes[6 * i], boxes[6 * i + 1], boxes[6 * i + 2]], [boxes[6 * i + 3], boxes[6 * i + 4], boxes[6 * i + 5]])).collect();
        self.world[w as usize].cells = Some(Cells { visible: vec![0; n], parts, runs, offsets: vec![0; n], counts: vec![0; n] });
    }

    // Whether a bucket is drawn (false: three draws it again, or nothing does).
    pub fn world_shown(&mut self, w: u32, shown: bool) {
        self.world[w as usize].shown = shown;
    }

    // three's opaque order (material creation) or by program first.
    pub fn sort(&mut self, by_program: bool) {
        self.by_program = by_program;
        let mut order: Vec<usize> = (0..self.world.len()).collect();
        if by_program {
            order.sort_by_key(|&i| (self.world[i].program, self.world[i].order));
        } else {
            order.sort_by_key(|&i| self.world[i].order);
        }
        self.order = order;
    }

    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, camera: &Camera) {
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let (programs, geometries) = (&mut core.programs, &core.geometries);
        let frustum = &camera.frustum;
        let light_count = camera.light_count;
        let lights = &core.input[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
        let lights32 = &core.frame[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
        let mut light_list: Vec<f32> = Vec::with_capacity(8 * MAX_LIGHTS);
        for &wi in &self.order {
            let w = &mut self.world[wi];
            // three tests a mesh's bounding sphere before it draws any of it.
            if !w.shown || !frustum.sphere(&w.sphere.0, w.sphere.1) {
                continue;
            }
            let geo = &geometries[w.geometry];
            let (mut ranges, mut first, mut count) = (1usize, 0i32, geo.index_count);
            let mut mask = 0u32;
            if let Some(cells) = &mut w.cells {
                for (i, (min, max)) in cells.parts.iter().enumerate() {
                    cells.visible[i] = frustum.aabb(min, max) as u8;
                }
                ranges = 0;
                let (mut start, mut end) = (-1i64, 0u32);
                for r in &cells.runs {
                    if cells.visible[r[0] as usize] == 0 {
                        continue;
                    }
                    if start >= 0 && r[1] == end {
                        end = r[2];
                        continue;
                    }
                    if start >= 0 {
                        cells.offsets[ranges] = start as i32 * geo.index_bytes;
                        cells.counts[ranges] = (end as i64 - start) as i32;
                        ranges += 1;
                    }
                    start = r[1] as i64;
                    end = r[2];
                }
                if start >= 0 {
                    cells.offsets[ranges] = start as i32 * geo.index_bytes;
                    cells.counts[ranges] = (end as i64 - start) as i32;
                    ranges += 1;
                }
                if ranges == 0 {
                    continue;
                }
                first = cells.offsets[0] / geo.index_bytes;
                count = end as i32 - first;
                if w.point_lit && light_count > 0 {
                    mask = light_mask(lights, light_count, &cells.parts, Some(&cells.visible));
                }
            } else if w.point_lit && light_count > 0 {
                mask = light_mask(lights, light_count, std::slice::from_ref(&w.bbox), None);
            }
            unsafe {
                state.use_program(gl, stats, programs[w.program].program);
                let program = &mut programs[w.program];
                material_values(gl, stats, program, &w.uniforms);
                if w.point_lit {
                    point_lights(gl, stats, program, lights32, light_count, mask, &mut light_list);
                }
                for &(unit, t) in &w.textures {
                    state.bind_texture(gl, stats, unit, Some(t));
                }
                state.set_state(gl, stats, w.cull, w.alpha_to_coverage, w.depth_write);
                gl.bind_vertex_array(Some(geo.vao));
                stats.gl_calls += 1;
                if ranges > 1 {
                    let cells = w.cells.as_mut().unwrap();
                    core.multi.multi_draw(glow::TRIANGLES, &mut cells.counts[..ranges], geo.index_type, &mut cells.offsets[..ranges]);
                } else {
                    gl.draw_elements(glow::TRIANGLES, count, geo.index_type, first * geo.index_bytes);
                }
                stats.gl_calls += 1;
                stats.world_draws += 1;
                stats.pass_draws[w.pass] += 1;
            }
        }
    }
}

// A draw's material values (the uniforms add_program was told of, by slot),
// each set only when it differs from what the program holds.
pub(crate) fn material_values<C: HasContext>(gl: &C, stats: &mut Stats, program: &mut Program<C>, values: &[(usize, Value)]) {
    for &(slot, value) in values {
        let u = &mut program.uniforms[slot];
        if u.2 != Some(value) {
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
            stats.gl_calls += 1;
        }
    }
}

// A draw's point lights, as useLights gives them for its mask: the masked
// lights of the frame's (lights32, the frame block's f32 values) in pack
// order, set only when they differ from what the program holds. `list` is
// scratch space.
pub(crate) fn point_lights<C: HasContext>(gl: &C, stats: &mut Stats, program: &mut Program<C>, lights32: &[f32], light_count: usize, mask: u32, list: &mut Vec<f32>) {
    if program.zero_light_count.is_none() {
        return;
    }
    list.clear();
    for i in 0..light_count {
        if mask & (1 << i) != 0 {
            list.extend_from_slice(&lights32[8 * i..8 * i + 8]);
        }
    }
    let n = (list.len() / 8) as i32;
    unsafe {
        if n > 0 && program.lights != *list {
            gl.uniform_4_f32_slice(program.zero_lights.as_ref(), list);
            program.lights.clear();
            program.lights.extend_from_slice(list);
            stats.gl_calls += 1;
        }
        if program.light_count != n {
            gl.uniform_1_i32(program.zero_light_count.as_ref(), n);
            program.light_count = n;
            stats.gl_calls += 1;
        }
    }
}
