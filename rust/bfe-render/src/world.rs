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
    lamps: Option<(usize, usize)>,
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
    light_list: Vec<f32>,
    visible: Vec<(usize, usize, i32, i32, u32)>,
    depth_program: Option<usize>,
    depth_prepass: bool,
    pub input: Vec<f32>,
    pub input_active: bool,
}

impl<C: HasContext> WorldPass<C> {
    pub fn uses_lamps<M: MultiDraw>(&self, _core: &Core<C,M>, id: u32) -> bool {
        self.world.get(id as usize).is_some_and(|w| w.lamps.is_some())
    }

    // WorldPass.draw used to send the same identity-origin lamp selection for
    // every static bucket. Resolve uniform slots once during cold admission;
    // the frame now publishes one five-float row, consumed by every bucket.
    pub fn init_lamps<M: MultiDraw>(&mut self, core: &Core<C,M>, id: u32) {
        let Some(w)=self.world.get_mut(id as usize) else {return;};
        let program=&core.programs[w.program];
        let index=program.uniforms.iter().position(|(n,_,_)| n=="bfeLampIndex");
        let count=program.uniforms.iter().position(|(n,_,_)| n=="bfeLampCount");
        if let (Some(index),Some(count))=(index,count) {
            let at=w.uniforms.len();
            w.uniforms.push((index,Value::F(4,[-1.;4])));
            w.uniforms.push((count,Value::F(1,[0.;4])));
            w.lamps=Some((at,at+1));
        }
    }

    pub fn program_id(&self,id:u32)->Option<u32> {self.world.get(id as usize).map(|w|w.program as u32)}
    // A typed settings command changes material/program state in place. Cells,
    // visibility and sort identity survive; slots are resolved again cold.
    pub fn reprogram(&mut self,id:u32,program:u32) {
        if let Some(w)=self.world.get_mut(id as usize) {w.program=program as usize;w.uniforms.clear();w.textures.clear();w.lamps=None;}
    }

    pub fn replace_texture(&mut self, old: C::Texture, new: C::Texture) {
        for world in &mut self.world { for (_, texture) in &mut world.textures { if *texture == old { *texture = new; } } }
    }

    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> WorldPass<C> {
        WorldPass { world: vec![], order: vec![], by_program: true, light_list: Vec::with_capacity(8 * MAX_LIGHTS), visible: vec![], depth_program: None, depth_prepass: false, input: vec![-1.,-1.,-1.,-1.,0.], input_active: false }
    }

    pub fn depth_prepass<M: MultiDraw>(&mut self, core: &mut Core<C, M>, enabled: bool) -> Result<(), String> {
        if enabled && self.depth_program.is_none() {
            let vertex = "#version 300 es\nprecision highp float;\nuniform mat4 projectionMatrix;\nuniform mat4 viewMatrix;\nlayout(location=0) in vec3 position;\nvoid main(){vec4 mvPosition=viewMatrix*vec4(position,1.0);gl_Position=projectionMatrix*mvPosition;}";
            let fragment = "#version 300 es\nprecision highp float;\nvoid main(){}";
            self.depth_program = Some(core.add_program(crate::shader::Kind::Plain, vertex, fragment, &[("position".into(), 0)], &["projectionMatrix".into(), "viewMatrix".into()])? as usize);
        }
        self.depth_prepass = enabled;
        Ok(())
    }

    // flags: cull side | 4 alpha to coverage | 8 depth write | 16 point lit
    // | 32 the terrain (its draws count for the terrain pass). bounds: sphere
    // centre and radius, then box min and max (world space).
    pub fn add_world(&mut self, geometry: u32, program: u32, order: u32, flags: u32, bounds: &[f64]) -> u32 {
        self.world.push(World {
            lamps: None, geometry: geometry as usize, program: program as usize, textures: Vec::with_capacity(16), uniforms: Vec::with_capacity(16),
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
        let _ = core;
        let Some(world)=self.world.get_mut(w as usize) else {return;};
        if let Some((index,count))=world.lamps {
            let mut row=[-1.,-1.,-1.,-1.,0.];let n=values.len().min(5);row[..n].copy_from_slice(&values[..n]);
            world.uniforms[index].1=Value::F(4,[row[0],row[1],row[2],row[3]]);
            world.uniforms[count].1=Value::F(1,[row[4].clamp(0.,4.),0.,0.,0.]);
        }
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
        self.visible.reserve(self.world.len().saturating_sub(self.visible.len()));
    }

    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, camera: &Camera) {
        if self.input_active {
            let row=&self.input;
            for world in &mut self.world { if let Some((index,count))=world.lamps {
                world.uniforms[index].1=Value::F(4,[row[0],row[1],row[2],row[3]]);
                world.uniforms[count].1=Value::F(1,[row[4].clamp(0.,4.),0.,0.,0.]);
            }}
        }
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let (programs, geometries) = (&mut core.programs, &core.geometries);
        let frustum = &camera.frustum;
        // Past the view distance's fade end the near shaders paint the far scene's image instead of the
        // surface, so geometry wholly deeper than that (view depth, as the shaders measure it) changes nothing.
        let fade_end = fade_end(&core.input);
        let view = camera.view;
        let light_count = camera.light_count;
        let lights = &core.input[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
        let lights32 = &core.frame[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS];
        let light_list = &mut self.light_list;
        self.visible.clear();
        for &wi in &self.order {
            let w = &mut self.world[wi];
            // three tests a mesh's bounding sphere before it draws any of it.
            if !w.shown || !frustum.sphere(&w.sphere.0, w.sphere.1) || sphere_beyond(&view, &w.sphere.0, w.sphere.1, fade_end) {
                continue;
            }
            let geo = &geometries[w.geometry];
            let (mut ranges, mut first, mut count) = (1usize, 0i32, geo.index_count);
            let mut mask = 0u32;
            if let Some(cells) = &mut w.cells {
                for (i, (min, max)) in cells.parts.iter().enumerate() {
                    cells.visible[i] = (frustum.aabb(min, max) && !box_beyond(&view, min, max, fade_end)) as u8;
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
            self.visible.push((wi, ranges, first, count, mask));
        }
        // Solid geometry establishes coverage before the expensive lighting
        // shaders. Cutouts retain their material's alpha-to-coverage pass.
        if let Some(id) = self.depth_program.filter(|_| self.depth_prepass) {
            let timer = core.profile.begin(gl);
            state.use_program(gl, stats, programs[id].program);
            unsafe {
                gl.uniform_matrix_4_f32_slice(programs[id].uniforms[0].1.as_ref(), false, &core.frame[..16]);
                gl.uniform_matrix_4_f32_slice(programs[id].uniforms[1].1.as_ref(), false, &core.frame[16..32]);
            }
            stats.gl_calls += 2;
            state.color_mask(gl, stats, [false; 4]);
            state.depth_func(gl, stats, glow::LEQUAL);
            for &(wi, ranges, first, count, _) in &self.visible {
                let w = &mut self.world[wi];
                if !w.depth_write || w.alpha_to_coverage { continue; }
                let geo = &geometries[w.geometry];
                state.set_state(gl, stats, w.cull, false, true);
                unsafe {
                    gl.bind_vertex_array(Some(geo.vao));
                    if ranges > 1 {
                        let cells = w.cells.as_mut().unwrap();
                        core.multi.multi_draw(glow::TRIANGLES, &mut cells.counts[..ranges], geo.index_type, &mut cells.offsets[..ranges]);
                    } else {
                        gl.draw_elements(glow::TRIANGLES, count, geo.index_type, first * geo.index_bytes);
                    }
                }
                let indices = if ranges > 1 { w.cells.as_ref().unwrap().counts[..ranges].iter().map(|n| *n as u64).sum() } else { count.max(0) as u64 };
                core.profile.draw(indices, 1, if ranges > 1 {core.multi.multi_draw_calls(ranges)} else {1});
                stats.gl_calls += 2;
            }
            state.color_mask(gl, stats, [true; 4]);
            core.profile.end(gl, "opaque depth", timer);
        }
        for &(wi, ranges, first, count, mask) in &self.visible {
            let w = &mut self.world[wi];
            let geo = &geometries[w.geometry];
            let timer = core.profile.begin(gl);
            unsafe {
                state.use_program(gl, stats, programs[w.program].program);
                let program = &mut programs[w.program];
                material_values(gl, stats, program, &w.uniforms);
                if w.point_lit {
                    point_lights(gl, stats, program, lights32, light_count, mask, light_list);
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
                let indices = if ranges > 1 { w.cells.as_ref().unwrap().counts[..ranges].iter().map(|v| *v as u64).sum() } else { count.max(0) as u64 };
                core.profile.draw(indices, 1, if ranges > 1 { core.multi.multi_draw_calls(ranges) } else { 1 });
                core.profile.end(gl, if w.pass == PASS_TERRAIN {"terrain"} else {"opaque"}, timer);
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

// Whether every point of a box lies deeper than `limit` along the view axis (view depth = -view-space z).
fn box_beyond(view: &[f64; 16], min: &[f64; 3], max: &[f64; 3], limit: f64) -> bool {
    let forward = [-view[2], -view[6], -view[10]];
    let mut nearest = -view[14];
    for k in 0..3 {
        nearest += forward[k] * if forward[k] > 0. { min[k] } else { max[k] };
    }
    nearest > limit
}
// The view distance's fade end this frame (infinite when the frame has none).
pub(crate) fn fade_end(input: &[f64]) -> f64 {
    let fade = crate::shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == "fade").map(|i| crate::shader::FRAME_OFFSETS[i] + 1);
    let fade_end = fade.map_or(f64::INFINITY, |at| input[at]);
    if fade_end.is_finite() && fade_end > 0. { fade_end } else { f64::INFINITY }
}
pub(crate) fn sphere_beyond(view: &[f64; 16], center: &[f64; 3], radius: f64, limit: f64) -> bool {
    let depth = -(view[2] * center[0] + view[6] * center[1] + view[10] * center[2] + view[14]);
    depth - radius > limit
}

#[cfg(test)]
mod fade_tests {
    // Looking down -z from the origin (identity view): a box from 300 to 320 m ahead is past a 220 m fade end,
    // one reaching back to 200 m is not, and one beside the eye at 230 m off-axis but 10 m deep is not.
    #[test]
    fn only_geometry_wholly_past_the_fade_end_is_dropped() {
        let view = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
        assert!(super::box_beyond(&view, &[-5., -5., -320.], &[5., 5., -300.], 220.));
        assert!(!super::box_beyond(&view, &[-5., -5., -320.], &[5., 5., -200.], 220.));
        assert!(!super::box_beyond(&view, &[225., -5., -15.], &[235., 5., -5.], 220.));
        assert!(super::sphere_beyond(&view, &[0., 0., -300.], 50., 220.) && !super::sphere_beyond(&view, &[0., 0., -300.], 90., 220.));
        assert!(!super::box_beyond(&view, &[-5., -5., -320.], &[5., 5., -300.], f64::INFINITY));
    }
}
