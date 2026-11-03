// The far scene, drawn into the far target before the near scene in place
// of three's render of it (src/rust-render/far.js): the sky dome and the
// models on its horizon, the map's low-detail buckets, the far terrain and
// the far water, with a frame block of the far camera's own (VIEW_FAR). A
// water's mirror (water.js reflect) draws it too, from its own camera into
// its own target, before the frame's far scene: its frame block is
// VIEW_FAR_MIRROR's, and the mirror's second pass draws over its sky
// without clearing (FAR_KEEP).
//
// Each thing three would draw there is an item, registered once from its
// mesh and material: a program made from three's GLSL (with the frame block
// where the material reads only the shared values, Kind::Plain where it
// keeps values of its own under their names), its geometry and state, and
// where each uniform the program reads besides the frame block comes from:
// a fixed value, a slot of the input the page writes each frame, a texture,
// the camera, or the item's own matrices. Each frame the page marks the
// items that draw and writes what moves (the sky's matrices, the water's
// values); the pass clears the target as three does, culls each item as
// three would (its bounding sphere; the map's buckets by spatial cell, as
// render-culling.js does; the terrain by its cells and the fade start, as
// terrain-patches.js does), sorts them as three sorts its render lists
// (opaque: group order, render order, material, depth, id; blended: group
// order, render order, back to front, id) and draws them, setting a
// uniform or a piece of state only when it differs from what is set.
//
// The Enhanced sky's flyers and the Enhanced ocean read a sky probe, a cube
// map (sky-probe.js): a cube sampler's source binds it by its target. The
// probe's captures are the far pass too, each face a phase of its own (the
// probe phase) with a frame block of its own, and the cube's mipmaps made
// after the last face as three makes them. Shadows, lamp lighting and the
// temporal upscaler need nothing more of the far pass (the upscaler's jitter
// is in the far camera's projection, in the frame block already).
use crate::core::{Core, MultiDraw, PASS_FAR, PASS_WATER, UNIT_BUMP, UNIT_FAR, UNIT_MAP};
use crate::math::{self, Mat4};
use glow::HasContext;
use std::cmp::Ordering;
use std::collections::HashMap;

// The far input (f64): the clear colour (premultiplied, as three sets it),
// whether the pass draws this frame at all (0: three does), whether it
// draws over the target as it is (1: three's autoClear is off, as for a
// water mirror's far scene drawn over its sky), then the items' slots, laid
// out by the page. An item's first slot says whether it draws this frame; a
// placed item's matrix follows it (16), or a terrain's level (0 full, 1
// reduced); then whatever its uniforms read.
pub const FAR_CLEAR: usize = 0;
pub const FAR_DRAW: usize = 4;
pub const FAR_KEEP: usize = 5;
// A sky probe's face (far_probe_frame): 1 + the cube texture whose mipmaps
// the pass makes once it has drawn (three makes them after the last face:
// CubeCamera.update), 0 for none.
pub const FAR_MIPMAPS: usize = 6;
// 1 when the page wrote the input for the near scene's items (far.js
// drawNear): only the near phase draws them (draw_near), once.
pub const FAR_NEAR: usize = 7;
pub const FAR_SLOTS: usize = 8;
pub const FAR_INPUT_LEN: usize = 16384;
pub const FAR_VERTICES_LEN: usize = 16384;

// Where a uniform's value comes from (far_source's kind).
pub const SRC_FLOATS: u32 = 0; // fixed values
pub const SRC_INPUT: u32 = 1; // the input from slot `at`
pub const SRC_TEXTURE: u32 = 2; // texture `at`
pub const SRC_TEXTURE_INPUT: u32 = 3; // the texture the input names at `at` (-1: none)
pub const SRC_PROJECTION: u32 = 4;
pub const SRC_VIEW: u32 = 5;
pub const SRC_EYE: u32 = 6;
pub const SRC_MODEL: u32 = 7;
pub const SRC_MODEL_VIEW: u32 = 8;
pub const SRC_NORMAL: u32 = 9;
pub const SRC_CUBE_INPUT: u32 = 10; // the cube map the input names at `at` (-1: none, left unbound)

// An item's flags (far_item): its material's side (0 front, 1 back, 2 both),
// then these.
pub const F_DEPTH_TEST: u32 = 4;
pub const F_DEPTH_WRITE: u32 = 8;
pub const F_COLOR_WRITE: u32 = 16;
pub const F_ALPHA_TO_COVERAGE: u32 = 32;
pub const F_TRANSPARENT: u32 = 64; // in three's transparent list
pub const F_PLACED: u32 = 128; // its matrix comes from the input
pub const F_CULLED: u32 = 256; // frustum culled by its bounding sphere
pub const F_BLEND: u32 = 512;

// GL uniform types.
const FLOAT: u32 = 0x1406;
const FLOAT_VEC2: u32 = 0x8B50;
const FLOAT_VEC3: u32 = 0x8B51;
const FLOAT_VEC4: u32 = 0x8B52;
const INT: u32 = 0x1404;
const BOOL: u32 = 0x8B56;
const FLOAT_MAT3: u32 = 0x8B5B;
const FLOAT_MAT4: u32 = 0x8B5C;
const SAMPLER_2D: u32 = 0x8B5E;
const SAMPLER_CUBE: u32 = 0x8B60;

// Units for the samplers besides the core's (map, farMap, bumpMap).
const FIRST_UNIT: u32 = 3;
const LAST_UNIT: u32 = 13;

const IDENTITY: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

// Floats of one element of a uniform type (None: a type the pass can't set).
fn floats_of(ty: u32) -> Option<usize> {
    match ty {
        FLOAT | INT | BOOL | SAMPLER_2D | SAMPLER_CUBE => Some(1),
        FLOAT_VEC2 => Some(2),
        FLOAT_VEC3 => Some(3),
        FLOAT_VEC4 => Some(4),
        FLOAT_MAT3 => Some(9),
        FLOAT_MAT4 => Some(16),
        _ => None,
    }
}

// A uniform the pass sets: its location and type, a sampler's unit, and the
// value the program holds (NaN until set).
struct Uniform<C: HasContext> {
    location: Option<C::UniformLocation>,
    ty: u32,
    unit: u32,
    value: Vec<f32>,
}

#[derive(Clone)]
enum Source {
    Floats(Vec<f32>),
    Input(usize),
    Texture(u32),
    TextureInput(usize),
    Projection,
    View,
    Eye,
    Model,
    ModelView,
    Normal,
    CubeInput(usize),
}

// A bucket's spatial cells (render-culling.js): runs of consecutive
// triangles in one cell, each cell's box, and the ranges drawn.
struct Cells {
    runs: Vec<[u32; 3]>,
    parts: Vec<([f64; 3], [f64; 3])>,
    visible: Vec<u8>,
    offsets: Vec<i32>,
    counts: Vec<i32>,
}

// A terrain level (terrain-patches.js): its index, cells in row order
// (first index, end, box), each a range of it.
struct Level<C: HasContext> {
    buffer: C::Buffer,
    cells: Vec<(u32, u32, [f64; 3], [f64; 3])>,
}

struct Terrain<C: HasContext> {
    levels: Vec<Option<Level<C>>>,
    bound: Option<usize>,
    offsets: Vec<i32>,
    counts: Vec<i32>,
}

enum Draw<C: HasContext> {
    Range(i32, i32),
    Cells(Cells),
    Terrain(Terrain<C>),
}

// An instanced item's per-instance attributes (three's InstancedMesh): each a
// buffer the input fills every frame (floats an instance, first slot), and
// the slot holding how many instances draw.
struct Instanced<C: HasContext> {
    count: usize,
    attributes: Vec<(C::Buffer, usize, usize)>,
    data: Vec<f32>,
}

struct Item<C: HasContext> {
    program: usize,
    table: usize,
    geometry: usize,
    sources: Vec<(usize, Source)>,
    flags: u32,
    depth_func: u32,
    blend: [u32; 6],
    // group order, render order, material id, object id (three's sort keys)
    order: [f64; 4],
    sphere: ([f64; 3], f64),
    slot: usize,
    // Where its index starts in its geometry's (one shared by several
    // buckets), in indices.
    base: i32,
    draw: Draw<C>,
    instanced: Option<Instanced<C>>,
    // Its draw range from the input each frame (first index, count) instead
    // of a fixed one: a geometry whose range three moves (the storm's bolt).
    range_at: Option<usize>,
    // Vertex buffers of its geometry the page refreshes (the storm's bolt,
    // rewritten at each strike): (buffer slot, where its values start in
    // `vertices`, floats), uploaded when the input's slot `refresh_at` is 1.
    refresh: Vec<(usize, usize, usize)>,
    refresh_at: usize,
}

pub struct FarPass<C: HasContext> {
    pub input: Vec<f64>,
    // Vertex values the page writes for items whose buffers it refreshes.
    pub vertices: Vec<f32>,
    tables: Vec<Vec<Uniform<C>>>,
    items: Vec<Item<C>>,
    // This frame's items and their depths (three's z), in drawing order.
    list: Vec<(usize, f64)>,
    value: Vec<f32>,
    vao: Option<C::VertexArray>,
    // Programs whose samplers are on units already (unit by name).
    units: HashMap<(usize, String), u32>,
    // A sky probe's frame blocks, one a cube face (made the first time a
    // probe is drawn): a capture's six faces read their own.
    probe_ubos: Vec<C::Buffer>,
    // The pass whose draws this call's items count as: the far pass's, or
    // the water's for the Enhanced ocean's near surface (draw_near).
    counted: usize,
}

impl<C: HasContext> FarPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> FarPass<C> {
        FarPass { input: vec![0.0; FAR_INPUT_LEN], vertices: vec![0.0; FAR_VERTICES_LEN], tables: vec![], items: vec![], list: vec![], value: vec![], vao: None, units: HashMap::new(), probe_ubos: vec![], counted: PASS_FAR }
    }

    // The uniforms of a program the pass sets (all it reads besides the frame
    // block): names, GL types and array lengths. Samplers other than the
    // core's get units of their own. None when it reads a type the pass
    // can't set, or needs more units than there are.
    pub fn add_table<M: MultiDraw>(&mut self, core: &mut Core<C, M>, program: u32, names: &str, types: &[u32], counts: &[u32]) -> Option<u32> {
        let gl = &core.gl;
        let handle = core.programs[program as usize].program;
        let mut uniforms = vec![];
        let mut next = FIRST_UNIT;
        let mut assign = vec![];
        for (i, name) in names.split(',').filter(|s| !s.is_empty()).enumerate() {
            let ty = types[i];
            let floats = floats_of(ty)? * counts[i].max(1) as usize;
            let location = unsafe { gl.get_uniform_location(handle, name) };
            let mut unit = 0;
            // A cube sampler (the Enhanced sky probe's) gets a unit of its own,
            // its cube map bound there (SRC_CUBE_INPUT).
            if ty == SAMPLER_2D || ty == SAMPLER_CUBE {
                unit = match name {
                    "map" => UNIT_MAP,
                    "farMap" => UNIT_FAR,
                    "bumpMap" => UNIT_BUMP,
                    _ => {
                        let key = (program as usize, name.to_string());
                        match self.units.get(&key) {
                            Some(&u) => u,
                            None => {
                                if next > LAST_UNIT {
                                    return None;
                                }
                                next += 1;
                                assign.push((location.clone(), next - 1));
                                self.units.insert(key, next - 1);
                                next - 1
                            }
                        }
                    }
                };
            }
            uniforms.push(Uniform { location, ty, unit, value: vec![f32::NAN; floats] });
        }
        if !assign.is_empty() {
            unsafe {
                gl.use_program(Some(handle));
                for (location, unit) in &assign {
                    gl.uniform_1_i32(location.as_ref(), *unit as i32);
                }
                gl.use_program(None);
            }
        }
        self.tables.push(uniforms);
        Some((self.tables.len() - 1) as u32)
    }

    // An item: program, its table, geometry, flags (F_*), depth function,
    // blending [equation rgb, alpha, source rgb, destination rgb, source
    // alpha, destination alpha], three's sort keys [group order, render
    // order, material id, object id], its bounding sphere in object space
    // [centre, radius], and its first input slot. It draws the whole index
    // until given a range, cells or a terrain.
    #[allow(clippy::too_many_arguments)]
    pub fn add_item<M: MultiDraw>(&mut self, core: &Core<C, M>, program: u32, table: u32, geometry: u32, flags: u32, depth_func: u32, blend: &[u32], order: &[f64], sphere: &[f64], slot: u32) -> u32 {
        let mut b = [0u32; 6];
        b.copy_from_slice(&blend[..6]);
        let count = core.geometries[geometry as usize].index_count;
        self.items.push(Item {
            program: program as usize, table: table as usize, geometry: geometry as usize, sources: vec![], flags, depth_func, blend: b,
            order: [order[0], order[1], order[2], order[3]], sphere: ([sphere[0], sphere[1], sphere[2]], sphere[3]),
            slot: slot as usize, base: 0, draw: Draw::Range(0, count), instanced: None, range_at: None, refresh: vec![], refresh_at: 0,
        });
        (self.items.len() - 1) as u32
    }

    // Where uniform `uniform` (of the item's table) comes from: kind SRC_*,
    // the fixed values or texture, or the input slot `at`.
    pub fn source(&mut self, item: u32, uniform: u32, kind: u32, values: &[f32], at: u32) {
        let source = match kind {
            SRC_FLOATS => Source::Floats(values.to_vec()),
            SRC_INPUT => Source::Input(at as usize),
            SRC_TEXTURE => Source::Texture(at),
            SRC_TEXTURE_INPUT => Source::TextureInput(at as usize),
            SRC_PROJECTION => Source::Projection,
            SRC_VIEW => Source::View,
            SRC_EYE => Source::Eye,
            SRC_MODEL => Source::Model,
            SRC_MODEL_VIEW => Source::ModelView,
            SRC_CUBE_INPUT => Source::CubeInput(at as usize),
            _ => Source::Normal,
        };
        self.items[item as usize].sources.push((uniform as usize, source));
    }

    // The part of the index the item draws (three's draw range).
    pub fn range(&mut self, item: u32, start: i32, count: i32) {
        self.items[item as usize].draw = Draw::Range(start, count);
    }

    // Where the item's own index starts in its geometry's (a geometry several
    // buckets share): its ranges and cells are its own index's, offset by it.
    pub fn base(&mut self, item: u32, base: i32) {
        self.items[item as usize].base = base;
    }

    // Its draw range from the input's slots `at` (first index, count), each
    // frame.
    pub fn range_input(&mut self, item: u32, at: u32) {
        self.items[item as usize].range_at = Some(at as usize);
    }

    // A vertex buffer of its geometry (`buffer`: its slot there) refreshed
    // from `vertices` (`floats` values from `at`) whenever the input's slot
    // `flag` is 1 (the page wrote them anew).
    pub fn refresh(&mut self, item: u32, buffer: u32, at: u32, floats: u32, flag: u32) {
        let it = &mut self.items[item as usize];
        it.refresh.push((buffer as usize, at as usize, floats as usize));
        it.refresh_at = flag as usize;
    }

    // A per-instance attribute of an instanced item, at `location` (a matrix
    // takes `columns` locations of `rows` floats), for up to `max` instances:
    // its values from the input's slot `at` each frame, the instance count
    // from slot `count`.
    #[allow(clippy::too_many_arguments)]
    pub fn instanced<M: MultiDraw>(&mut self, core: &mut Core<C, M>, item: u32, location: u32, columns: u32, rows: i32, max: u32, count: u32, at: u32) {
        let gl = &core.gl;
        let it = &mut self.items[item as usize];
        let floats = columns as usize * rows as usize;
        let buffer = unsafe {
            gl.bind_vertex_array(Some(core.geometries[it.geometry].vao));
            let b = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
            gl.buffer_data_size(glow::ARRAY_BUFFER, (max as usize * floats * 4) as i32, glow::DYNAMIC_DRAW);
            for c in 0..columns {
                gl.enable_vertex_attrib_array(location + c);
                gl.vertex_attrib_pointer_f32(location + c, rows, glow::FLOAT, false, floats as i32 * 4, c as i32 * rows * 4);
                gl.vertex_attrib_divisor(location + c, 1);
            }
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            b
        };
        let inst = it.instanced.get_or_insert_with(|| Instanced { count: count as usize, attributes: vec![], data: vec![] });
        inst.count = count as usize;
        inst.attributes.push((buffer, floats, at as usize));
    }

    // The cells render-culling.js culls a large bucket by: its triangles in
    // cells of cell_size (by their centres), runs of consecutive triangles in
    // one cell, each cell's box over its vertices.
    pub fn cells(&mut self, item: u32, positions: &[f32], index: &[u32], cell_size: f64) {
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
        self.items[item as usize].draw = Draw::Cells(Cells { visible: vec![0; parts.len()], parts, runs, offsets: vec![0; n], counts: vec![0; n] });
    }

    // A level of a terrain item (0 full, 1 reduced): its index (32-bit) and
    // cells [first index, end, min x, y, z, max x, y, z] in row order.
    pub fn terrain<M: MultiDraw>(&mut self, core: &mut Core<C, M>, item: u32, level: u32, index: &[u32], cells: &[f64]) {
        let gl = &core.gl;
        let it = &mut self.items[item as usize];
        let vao = core.geometries[it.geometry].vao;
        // An index buffer is one from its first binding on (WebGL): made in
        // the geometry's vertex array, which then draws this level.
        let buffer = unsafe {
            gl.bind_vertex_array(Some(vao));
            let b = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(b));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, crate::core::bytes(index), glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            b
        };
        let cells = cells.as_chunks::<8>().0.iter().map(|c| (c[0] as u32, c[1] as u32, [c[2], c[3], c[4]], [c[5], c[6], c[7]])).collect::<Vec<_>>();
        if !matches!(it.draw, Draw::Terrain(_)) {
            it.draw = Draw::Terrain(Terrain { levels: vec![], bound: None, offsets: vec![], counts: vec![] });
        }
        if let Draw::Terrain(t) = &mut it.draw {
            while t.levels.len() <= level as usize {
                t.levels.push(None);
            }
            let n = cells.len();
            if t.offsets.len() < n {
                t.offsets.resize(n, 0);
                t.counts.resize(n, 0);
            }
            if let Some(old) = t.levels[level as usize].replace(Level { buffer, cells }) {
                unsafe { gl.delete_buffer(old.buffer) };
            }
            t.bound = Some(level as usize);
        }
    }

    // The far phase (renderer.rs far_frame, or far_mirror_frame for a water
    // mirror's camera): that camera's frame block (the view's buffer), the
    // target cleared as three clears it (unless three's autoClear is off),
    // then this frame's items in three's order.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, frame: usize) {
        if self.input[FAR_DRAW] == 0.0 {
            return;
        }
        core.upload_frame(frame);
        self.counted = PASS_FAR;
        self.draw_items(core);
    }

    // The near phase's part (renderer.rs frame, a mirror's too): the items
    // the page marked from the near scene (the Enhanced ocean's surface,
    // drawn there in three's place: far.js drawNear), with the phase's frame
    // block, over what the phase drew (the page sets FAR_KEEP). Counted as
    // the water pass's draws: the ocean is the water's surface.
    pub fn draw_near<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        if self.input[FAR_NEAR] == 0.0 || self.input[FAR_DRAW] == 0.0 {
            return;
        }
        self.input[FAR_NEAR] = 0.0;
        self.counted = PASS_WATER;
        self.draw_items(core);
        self.counted = PASS_FAR;
    }

    // A face of a sky probe's capture (sky-probe.js, renderer.rs
    // far_probe_frame): the far pass for the face's camera into the face
    // three bound, with a frame block of the probe's own for the face; after
    // the last face, the cube's mipmaps when three would make them.
    pub fn draw_probe<M: MultiDraw>(&mut self, core: &mut Core<C, M>, face: usize) {
        if self.input[FAR_DRAW] == 0.0 {
            return;
        }
        let gl = &core.gl;
        while self.probe_ubos.len() < 6 {
            let ubo = unsafe {
                let ubo = gl.create_buffer().unwrap();
                gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
                gl.buffer_data_size(glow::UNIFORM_BUFFER, (crate::shader::FRAME_FLOATS * 4) as i32, glow::DYNAMIC_DRAW);
                gl.bind_buffer(glow::UNIFORM_BUFFER, None);
                ubo
            };
            self.probe_ubos.push(ubo);
        }
        // The frame block's values (core.upload_frame), into the face's buffer.
        let ubo = self.probe_ubos[face.min(5)];
        for i in 0..crate::shader::FRAME_FLOATS {
            core.frame[i] = core.input[i] as f32;
        }
        unsafe {
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
            gl.buffer_sub_data_u8_slice(glow::UNIFORM_BUFFER, 0, crate::core::bytes(&core.frame));
            gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(ubo));
        }
        core.stats.gl_calls += 3;
        self.counted = PASS_FAR;
        self.draw_items(core);
        let cube = self.input[FAR_MIPMAPS];
        if cube > 0.0 {
            if let Some(&texture) = core.textures.get(cube as usize - 1) {
                core.state.bind_texture_target(&core.gl, &mut core.stats, crate::core::UNIT_MAP, glow::TEXTURE_CUBE_MAP, Some(texture));
                unsafe { core.gl.generate_mipmap(glow::TEXTURE_CUBE_MAP) };
                core.stats.gl_calls += 1;
            }
        }
    }

    // This frame's items for the camera whose frame block is bound: the
    // target cleared as three clears it (unless three's autoClear is off),
    // then the items in three's order.
    fn draw_items<M: MultiDraw>(&mut self, core: &mut Core<C, M>) {
        let camera = core.camera();
        let mut projection = [0.0; 16];
        projection.copy_from_slice(&core.input[0..16]);
        let view = camera.view;
        let screen = math::mul(&projection, &view);
        let eye = [core.input[32], core.input[33], core.input[34]];
        // terrain-patches.js leaves out cells wholly nearer than the fade
        // start (less a metre), where the far shaders discard every fragment.
        let start = (core.input[crate::shader::FRAME_OFFSETS[18]] - 1.0).max(0.0);
        let limit = start * start;
        let input = &self.input;
        if input[FAR_KEEP] == 0.0 {
            unsafe {
                let c = &input[FAR_CLEAR..FAR_CLEAR + 4];
                core.gl.clear_color(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32);
            }
            core.stats.gl_calls += 1;
            let (gl, state, stats) = (&core.gl, &mut core.state, &mut core.stats);
            state.color_mask(gl, stats, [true; 4]);
            state.depth_write(gl, stats, true);
            unsafe { gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT) };
            stats.gl_calls += 1;
        }
        core.opaque();
        // The vertices the page wrote anew for items shown this frame (in view
        // or not: the page counts them as sent), uploaded.
        for item in &self.items {
            if item.refresh.is_empty() || input[item.slot] == 0.0 || input[item.refresh_at] == 0.0 {
                continue;
            }
            let geo = &core.geometries[item.geometry];
            for &(buffer, at, floats) in &item.refresh {
                if let Some(&b) = geo.buffers.get(buffer) {
                    unsafe {
                        core.gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                        core.gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, crate::core::bytes(&self.vertices[at..at + floats]));
                    }
                    core.stats.gl_calls += 2;
                }
            }
            unsafe { core.gl.bind_buffer(glow::ARRAY_BUFFER, None) };
            core.stats.gl_calls += 1;
        }
        // This frame's items: shown, in view, with their depths.
        self.list.clear();
        for (i, item) in self.items.iter().enumerate() {
            if input[item.slot] == 0.0 {
                continue;
            }
            let world = matrix_of(item, input);
            let (center, radius) = math::sphere_to_world(&world, &item.sphere.0, item.sphere.1);
            if item.flags & F_CULLED != 0 && !camera.frustum.sphere(&center, radius) {
                continue;
            }
            // three's z: the sphere's centre through the view and projection.
            let z = math::sphere_to_world(&screen, &center, 0.0).0[2];
            self.list.push((i, z));
        }
        let items = &self.items;
        self.list.sort_by(|a, b| order(&items[a.0], a.1, &items[b.0], b.1));
        self.vao = None;
        let list = std::mem::take(&mut self.list);
        for &(i, _) in &list {
            self.draw_item(core, i, &projection, &view, &eye, &camera.frustum, limit);
        }
        self.list = list;
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_item<M: MultiDraw>(&mut self, core: &mut Core<C, M>, i: usize, projection: &Mat4, view: &Mat4, eye: &[f64; 3], frustum: &math::Frustum, limit: f64) {
        let counted = self.counted;
        let gl = &core.gl;
        let (state, stats) = (&mut core.state, &mut core.stats);
        let item = &mut self.items[i];
        let input = &self.input;
        let geo = &core.geometries[item.geometry];
        // What it draws: its ranges this frame (none: nothing in view).
        let mut ranges = 1usize;
        let (first, count);
        match &mut item.draw {
            Draw::Range(s, c) => {
                let (s, c) = match item.range_at {
                    Some(at) => (input[at] as i32, input[at + 1] as i32),
                    None => (*s, *c),
                };
                first = s + item.base;
                count = c;
            }
            Draw::Cells(cells) => {
                for (k, (min, max)) in cells.parts.iter().enumerate() {
                    cells.visible[k] = frustum.aabb(min, max) as u8;
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
                        cells.offsets[ranges] = (start as i32 + item.base) * geo.index_bytes;
                        cells.counts[ranges] = (end as i64 - start) as i32;
                        ranges += 1;
                    }
                    start = r[1] as i64;
                    end = r[2];
                }
                if start >= 0 {
                    cells.offsets[ranges] = (start as i32 + item.base) * geo.index_bytes;
                    cells.counts[ranges] = (end as i64 - start) as i32;
                    ranges += 1;
                }
                if ranges == 0 {
                    return;
                }
                first = cells.offsets[0] / geo.index_bytes;
                count = end as i32 + item.base - first;
            }
            Draw::Terrain(t) => {
                let level = (input[item.slot + 1] as usize).min(t.levels.len().saturating_sub(1));
                let Some(l) = t.levels.get(level).and_then(|l| l.as_ref()) else { return };
                ranges = terrain_runs(&l.cells, frustum, eye, limit, &mut t.offsets, &mut t.counts);
                if ranges == 0 {
                    return;
                }
                first = t.offsets[0] / 4;
                count = t.counts[0];
                if t.bound != Some(level) {
                    // The level's index into the geometry's vertex array.
                    unsafe {
                        gl.bind_vertex_array(Some(geo.vao));
                        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(l.buffer));
                    }
                    stats.gl_calls += 2;
                    self.vao = Some(geo.vao);
                    t.bound = Some(level);
                }
            }
        }
        if count <= 0 && ranges <= 1 {
            return;
        }
        // An instanced item's instances this frame, uploaded.
        let mut instances = 0;
        if let Some(inst) = &mut item.instanced {
            instances = input[inst.count].max(0.0) as usize;
            if instances == 0 {
                return;
            }
            for &(buffer, floats, at) in &inst.attributes {
                inst.data.clear();
                inst.data.extend(input[at..at + instances * floats].iter().map(|&v| v as f32));
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, crate::core::bytes(&inst.data));
                }
                stats.gl_calls += 2;
            }
            unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, None) };
            stats.gl_calls += 1;
        }
        let program = &core.programs[item.program];
        state.use_program(gl, stats, program.program);
        // Its uniforms, each set when it differs from what the program holds.
        let world = matrix_of(item, input);
        let mut model_view: Option<Mat4> = None;
        let table = &mut self.tables[item.table];
        for (u, source) in &item.sources {
            let uniform = &mut table[*u];
            let value = &mut self.value;
            value.clear();
            let push16 = |value: &mut Vec<f32>, m: &[f64]| value.extend(m.iter().map(|&v| v as f32));
            match source {
                Source::Floats(v) => value.extend_from_slice(v),
                Source::Input(at) => {
                    let n = uniform.value.len();
                    value.extend(input[*at..*at + n].iter().map(|&v| v as f32));
                }
                Source::Texture(t) => {
                    state.bind_texture(gl, stats, uniform.unit, Some(core.textures[*t as usize]));
                    continue;
                }
                Source::TextureInput(at) => {
                    let t = input[*at];
                    state.bind_texture(gl, stats, uniform.unit, if t >= 0.0 { core.textures.get(t as usize).copied() } else { None });
                    continue;
                }
                Source::CubeInput(at) => {
                    // A cube sampler given none (the Classic flyers') is never
                    // read: nothing is bound for it.
                    let t = input[*at];
                    if t >= 0.0 {
                        state.bind_texture_target(gl, stats, uniform.unit, glow::TEXTURE_CUBE_MAP, core.textures.get(t as usize).copied());
                    }
                    continue;
                }
                Source::Projection => push16(value, projection),
                Source::View => push16(value, view),
                Source::Eye => push16(value, eye),
                Source::Model => push16(value, &world),
                Source::ModelView | Source::Normal => {
                    let mv = *model_view.get_or_insert_with(|| math::mul(view, &world));
                    if matches!(source, Source::ModelView) {
                        push16(value, &mv);
                    } else {
                        push16(value, &math::normal_matrix(&mv));
                    }
                }
            }
            if value.len() != uniform.value.len() || *value == uniform.value {
                continue;
            }
            let l = uniform.location.as_ref();
            unsafe {
                match uniform.ty {
                    FLOAT => gl.uniform_1_f32_slice(l, value),
                    FLOAT_VEC2 => gl.uniform_2_f32_slice(l, value),
                    FLOAT_VEC3 => gl.uniform_3_f32_slice(l, value),
                    FLOAT_VEC4 => gl.uniform_4_f32_slice(l, value),
                    FLOAT_MAT3 => gl.uniform_matrix_3_f32_slice(l, false, value),
                    FLOAT_MAT4 => gl.uniform_matrix_4_f32_slice(l, false, value),
                    _ => {
                        let ints: Vec<i32> = value.iter().map(|&v| v as i32).collect();
                        gl.uniform_1_i32_slice(l, &ints);
                    }
                }
            }
            uniform.value.copy_from_slice(value);
            stats.gl_calls += 1;
        }
        // three's setMaterial: culling, blending, depth, colour, coverage.
        let f = item.flags;
        state.cull(gl, stats, f & 3);
        if f & F_BLEND != 0 {
            let b = item.blend;
            state.blend(gl, stats, true);
            state.blend_equation(gl, stats, b[0], b[1]);
            state.blend_func(gl, stats, b[2], b[3], b[4], b[5]);
        } else {
            state.blend(gl, stats, false);
        }
        state.depth_func(gl, stats, item.depth_func);
        state.depth_test(gl, stats, f & F_DEPTH_TEST != 0);
        state.depth_write(gl, stats, f & F_DEPTH_WRITE != 0);
        state.color_mask(gl, stats, [f & F_COLOR_WRITE != 0; 4]);
        state.alpha_to_coverage(gl, stats, f & F_ALPHA_TO_COVERAGE != 0);
        if self.vao != Some(geo.vao) {
            unsafe { gl.bind_vertex_array(Some(geo.vao)) };
            self.vao = Some(geo.vao);
            stats.gl_calls += 1;
        }
        unsafe {
            if ranges > 1 {
                match &mut item.draw {
                    Draw::Cells(c) => core.multi.multi_draw(glow::TRIANGLES, &mut c.counts[..ranges], geo.index_type, &mut c.offsets[..ranges]),
                    Draw::Terrain(t) => core.multi.multi_draw(glow::TRIANGLES, &mut t.counts[..ranges], glow::UNSIGNED_INT, &mut t.offsets[..ranges]),
                    Draw::Range(..) => {}
                }
            } else if matches!(item.draw, Draw::Terrain(_)) {
                gl.draw_elements(glow::TRIANGLES, count, glow::UNSIGNED_INT, first * 4);
            } else if instances > 0 {
                gl.draw_elements_instanced(glow::TRIANGLES, count, geo.index_type, first * geo.index_bytes, instances as i32);
            } else {
                gl.draw_elements(glow::TRIANGLES, count, geo.index_type, first * geo.index_bytes);
            }
        }
        stats.gl_calls += 1;
        stats.pass_draws[counted] += 1;
    }

    // How many items are registered (the page's checks).
    pub fn items(&self) -> u32 {
        self.items.len() as u32
    }
}

// terrain-patches.js update(): the cells in view and wholly past the fade
// start (`limit`: its square), consecutive ones (in the cells' order, empty
// cells having none) joined in one range. Writes each range's byte offset
// and index count; returns how many.
fn terrain_runs(cells: &[(u32, u32, [f64; 3], [f64; 3])], frustum: &math::Frustum, eye: &[f64; 3], limit: f64, offsets: &mut [i32], counts: &mut [i32]) -> usize {
    let mut ranges = 0;
    let (mut from, mut to) = (-1i64, 0u32);
    for &(s, e, min, max) in cells {
        let mut shown = frustum.aabb(&min, &max);
        if shown && limit > 0.0 {
            let dx = (eye[0] - min[0]).abs().max((eye[0] - max[0]).abs());
            let dy = (eye[1] - min[1]).abs().max((eye[1] - max[1]).abs());
            let dz = (eye[2] - min[2]).abs().max((eye[2] - max[2]).abs());
            shown = dx * dx + dy * dy + dz * dz >= limit;
        }
        if !shown {
            if from >= 0 {
                offsets[ranges] = from as i32 * 4;
                counts[ranges] = (to as i64 - from) as i32;
                ranges += 1;
                from = -1;
            }
            continue;
        }
        if from < 0 {
            from = s as i64;
        }
        to = e;
    }
    if from >= 0 {
        offsets[ranges] = from as i32 * 4;
        counts[ranges] = (to as i64 - from) as i32;
        ranges += 1;
    }
    ranges
}

// An item's matrix: the input's, for a placed one; else the identity (the
// map's own meshes, in the world already).
fn matrix_of<C: HasContext>(item: &Item<C>, input: &[f64]) -> Mat4 {
    if item.flags & F_PLACED == 0 {
        return IDENTITY;
    }
    let mut m = [0.0; 16];
    m.copy_from_slice(&input[item.slot + 1..item.slot + 17]);
    m
}

// three's painterSortStable (opaque) and reversePainterSortStable (blended):
// the opaque list first.
fn order<C: HasContext>(a: &Item<C>, za: f64, b: &Item<C>, zb: f64) -> Ordering {
    let ta = a.flags & F_TRANSPARENT != 0;
    let tb = b.flags & F_TRANSPARENT != 0;
    if ta != tb {
        return ta.cmp(&tb);
    }
    let by = |x: f64, y: f64| x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    if a.order[0] != b.order[0] {
        return by(a.order[0], b.order[0]);
    }
    if a.order[1] != b.order[1] {
        return by(a.order[1], b.order[1]);
    }
    if !ta {
        if a.order[2] != b.order[2] {
            return by(a.order[2], b.order[2]);
        }
        if za != zb {
            return by(za, zb);
        }
    } else if za != zb {
        return by(zb, za);
    }
    by(a.order[3], b.order[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(transparent: bool, order: [f64; 4]) -> Item<glow::Context> {
        Item {
            program: 0, table: 0, geometry: 0, sources: vec![], flags: if transparent { F_TRANSPARENT } else { 0 }, depth_func: 0, blend: [0; 6],
            order, sphere: ([0.0; 3], 1.0), slot: 0, base: 0, draw: Draw::Range(0, 3), instanced: None, range_at: None, refresh: vec![], refresh_at: 0,
        }
    }

    #[test]
    fn three_sorts_its_lists_so() {
        // [group order, render order, material id, object id], depth.
        let items = [
            (item(true, [0.0, 0.0, 9.0, 1.0]), 0.2),
            (item(false, [0.0, 0.0, 7.0, 2.0]), 0.9),
            (item(false, [0.0, -1e9, 8.0, 3.0]), 0.5),
            (item(false, [0.0, 0.0, 7.0, 4.0]), 0.1),
            (item(true, [0.0, 0.0, 3.0, 5.0]), 0.7),
            (item(true, [0.0, -20.0, 4.0, 6.0]), 0.1),
            (item(false, [0.0, 0.0, 7.0, 7.0]), 0.1),
        ];
        let mut list: Vec<usize> = (0..items.len()).collect();
        list.sort_by(|&a, &b| order(&items[a].0, items[a].1, &items[b].0, items[b].1));
        let ids: Vec<f64> = list.iter().map(|&i| items[i].0.order[3]).collect();
        // Opaque: render order, material, near to far, id; then blended:
        // render order, far to near.
        assert_eq!(ids, [3.0, 4.0, 7.0, 2.0, 6.0, 5.0, 1.0]);
    }

    #[test]
    fn terrain_runs_as_terrain_patches_draws_them() {
        // Clip space as the frustum; four cells along x, the second wholly
        // within 0.5 of the eye (hidden by the fade start), the fourth out
        // of view.
        let mut identity = [0.0; 16];
        for i in 0..4 {
            identity[i * 5] = 1.0;
        }
        let frustum = math::Frustum::from_matrix(&identity);
        let cell = |s: u32, e: u32, x: f64| (s, e, [x, -0.1, -0.1], [x + 0.1, 0.1, 0.1]);
        let cells = [cell(0, 30, -0.9), cell(30, 60, -0.05), cell(60, 66, 0.5), cell(66, 90, 2.0)];
        let (mut offsets, mut counts) = (vec![0; 4], vec![0; 4]);
        let n = terrain_runs(&cells, &frustum, &[0.0; 3], 0.5 * 0.5, &mut offsets, &mut counts);
        assert_eq!((n, &offsets[..n], &counts[..n]), (2, &[0, 60 * 4][..], &[30, 6][..]));
        // No fade start: the three in view, one range.
        let n = terrain_runs(&cells, &frustum, &[0.0; 3], 0.0, &mut offsets, &mut counts);
        assert_eq!((n, &offsets[..n], &counts[..n]), (1, &[0][..], &[66][..]));
    }
}
