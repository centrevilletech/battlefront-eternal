// What the near scene blends, drawn in three's back-to-front order: the
// transparent list. Every pass whose draws blend (this one's own: the map's
// blended buckets; the effects; the water...) is a source of items for it.
//
// The list. Three draws its transparent list after every opaque draw,
// sorted by WebGLRenderList's reversePainterSortStable: group order (the
// renderOrder of the nearest Group above the object), then render order
// (the object's renderOrder), both ascending, then z descending (the clip
// space z of its bounding sphere's centre: farthest first), then the
// object's id, and otherwise in the order three met them. An Item is one
// entry of it: that key, and what draws it (its source, the source's batch
// and record). Each frame, in the transparent phase:
//
//   1. Each source emits its items (Source::emit) for the near camera
//      (View: the camera, frustum and view-projection the frame block holds).
//      three_z() gives z as three computes it, operation for operation.
//   2. The pass sorts them all together as three sorts (a stable sort by
//      the same key), then merges in what three still draws itself: the
//      page writes the keys of three's own remaining transparent items, in
//      three's order, into `keys` (KEY_STRIDE f64 each). Items and three's
//      draws then interleave exactly as one list sorted by three would.
//   3. Segment k is the items between three's item k-1 and its item k:
//      segment 0 is drawn at once (the start of three's transparent list),
//      segment k > 0 right after three draws its item k-1 (the page calls
//      transparent_segment(k) from there); `segments` holds where each
//      starts (three's count + 2 values). With nothing left in three, the
//      whole list is segment 0.
//   4. A segment is drawn in runs: consecutive items of one source with the
//      same batch go to that source's draw_run in one call, in list order,
//      so a source may draw a run as one draw (instanced, or one buffer)
//      when its items allow it. Each segment starts with nothing known of
//      the GL state (three drew in between); the GlState cache spares
//      repeated calls within it.
//
// A source is a pass implementing Source, given a SOURCE_* id (append new
// ones) and listed in renderer.rs's transparent phase. Its emit() pushes
// items with its own `record` (what to draw) and `batch` (what may draw
// together); its draw_run() draws them. Both default to nothing.
//
// This pass's own items, each registered once, then each frame taken out
// of three's list by the page in the near scene's own render (three still
// culls and keys them, and draws them in any other render: a mirror's) and
// written as a record in `objects`, with three's key for it:
// - the map's blended buckets (one mesh each, the identity for a model
//   matrix, a zero material that blends);
// - objects drawn with three's basic and sprite materials (the command
//   posts' holograms, their flares...), whose records carry their matrix
//   and material values;
// each drawn as three draws it: double-sided ones twice, back faces then
// front, each pass with three's program for that side.
use crate::core::{Camera, Core, MultiDraw, Value, PASS_TRANSPARENT, UNIT_MAP};
use crate::lights::light_mask;
use crate::math::{self, Mat4};
use crate::shader::{FRAME_LIGHTS, MAX_LIGHTS};
use crate::world::{material_values, point_lights};
use glow::HasContext;
use std::cmp::Ordering;
use std::collections::HashMap;

// The sources, in the order renderer.rs lists them.
pub const SOURCE_TRANSPARENT: u32 = 0;
pub const SOURCE_EFFECTS: u32 = 1;
pub const SOURCE_WATER: u32 = 2;

// One entry of the transparent list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Item {
    // three's sort key.
    pub group_order: f64,
    pub render_order: f64,
    pub z: f64,
    pub id: f64,
    // Who draws it, and how: set by the source.
    pub source: u32,
    pub batch: u32,
    pub record: u32,
}

// reversePainterSortStable on two keys (group order, render order, z, id).
// A comparison three's comparator makes NaN of counts as equal, as
// Array.prototype.sort takes it.
pub fn compare(a: &[f64; 4], b: &[f64; 4]) -> Ordering {
    let order = |x: f64| if x < 0.0 { Ordering::Less } else if x > 0.0 { Ordering::Greater } else { Ordering::Equal };
    if a[0] != b[0] {
        order(a[0] - b[0])
    } else if a[1] != b[1] {
        order(a[1] - b[1])
    } else if a[2] != b[2] {
        order(b[2] - a[2])
    } else {
        order(a[3] - b[3])
    }
}

impl Item {
    pub fn key(&self) -> [f64; 4] {
        [self.group_order, self.render_order, self.z, self.id]
    }
}

// What a source draws for: the near camera (as core.camera() reads it) and
// three's _projScreenMatrix (projection times the camera's inverse world).
pub struct View {
    pub camera: Camera,
    pub view_projection: Mat4,
}

// z as three's projectObject computes it: the bounding sphere's centre as
// a Vector4 (w 1), through the object's world matrix (none: the identity,
// which changes nothing), then the view-projection; its z (no divide).
pub fn three_z(view_projection: &Mat4, world: Option<&Mat4>, center: &[f64; 3]) -> f64 {
    let (mut x, mut y, mut z, mut w) = (center[0], center[1], center[2], 1.0);
    if let Some(e) = world {
        let (a, b, c, d) = (x, y, z, w);
        x = e[0] * a + e[4] * b + e[8] * c + e[12] * d;
        y = e[1] * a + e[5] * b + e[9] * c + e[13] * d;
        z = e[2] * a + e[6] * b + e[10] * c + e[14] * d;
        w = e[3] * a + e[7] * b + e[11] * c + e[15] * d;
    }
    let e = view_projection;
    e[2] * x + e[6] * y + e[10] * z + e[14] * w
}

// A pass whose draws blend. emit(): this frame's items, pushed onto `items`
// (source set to `source`). draw_run(): a run of them, in list order.
pub trait Source<C: HasContext, M: MultiDraw> {
    fn emit(&mut self, _core: &mut Core<C, M>, _view: &View, _source: u32, _items: &mut Vec<Item>) {}
    fn draw_run(&mut self, _core: &mut Core<C, M>, _view: &View, _run: &[Item]) {}
}

// three's remaining items (keys) the page can write a frame.
pub const MAX_THREE: usize = 4096;
pub const KEY_STRIDE: usize = 4;

// The frame's list: every source's items, sorted, and where each segment
// starts. Taken out of the pass while the sources draw.
pub struct List {
    pub items: Vec<Item>,
    scratch:Vec<Item>,
    pub segments: Vec<u32>,
    // The view the frame's items were emitted for (its later segments draw for it too).
    pub view: Option<View>,
    // The frame block that view reads (core.rs VIEW_*: the near scene's, or
    // a mirror's), bound again for the later segments.
    pub frame: usize,
}

// Keep stable ordering without sort_by's temporary heap allocation. The
// bound covers registered map buckets plus the bounded effect/rig/line rows.
const LIST_RESERVE:usize=16384;
impl Default for List {
    fn default()->Self {Self{items:Vec::new(),scratch:Vec::new(),segments:Vec::new(),view:None,frame:0}}
}
impl List {
    pub fn reserved()->Self {Self{items:Vec::with_capacity(LIST_RESERVE),scratch:Vec::with_capacity(LIST_RESERVE),segments:Vec::with_capacity(MAX_THREE+2),view:None,frame:0}}
    // Sorts the items as three would, then finds the segments around
    // three's own items (`keys`, three's sorted order).
    pub fn order(&mut self, keys: &[f64], three: usize) {
        let n=self.items.len();self.scratch.resize(n,Item::default());
        let mut width=1;
        while width<n {
            let mut first=0;
            while first<n {
                let middle=(first+width).min(n);let end=(middle+width).min(n);
                let (mut left,mut right,mut out)=(first,middle,first);
                while out<end {
                    if right==end || left<middle && compare(&self.items[left].key(),&self.items[right].key())!=Ordering::Greater {
                        self.scratch[out]=self.items[left];left+=1;
                    }else{self.scratch[out]=self.items[right];right+=1;}
                    out+=1;
                }
                first=end;
            }
            std::mem::swap(&mut self.items,&mut self.scratch);width=width.saturating_mul(2);
        }
        self.segments.clear();
        self.segments.push(0);
        let mut at = 0usize;
        for k in 0..three {
            let key = [keys[KEY_STRIDE * k], keys[KEY_STRIDE * k + 1], keys[KEY_STRIDE * k + 2], keys[KEY_STRIDE * k + 3]];
            while at < self.items.len() && compare(&self.items[at].key(), &key) == Ordering::Less {
                at += 1;
            }
            self.segments.push(at as u32);
        }
        self.segments.push(self.items.len() as u32);
    }

    // Segment k's items.
    pub fn segment(&self, k: usize) -> &[Item] {
        if k + 1 >= self.segments.len() {
            return &[];
        }
        &self.items[self.segments[k] as usize..self.segments[k + 1] as usize]
    }
}

// Draws a segment in runs, each run by its source.
pub fn draw_segment<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>, view: &View, items: &[Item], sources: &mut [&mut dyn Source<C, M>]) {
    let mut i = 0;
    while i < items.len() {
        let (source, batch) = (items[i].source, items[i].batch);
        let mut j = i + 1;
        while j < items.len() && items[j].source == source && items[j].batch == batch {
            j += 1;
        }
        if let Some(s) = sources.get_mut(source as usize) {
            s.draw_run(core, view, &items[i..j]);
        }
        i = j;
    }
}

// Three's blending, as setBlending sets it: (enabled, equation rgb, equation
// alpha, src rgb, dst rgb, src alpha, dst alpha).
pub type Blend = [u32; 7];

// The GL state of a draw, as three's setMaterial makes it: blending, depth
// test and its function, depth write, colour write.
#[derive(Clone, Copy, PartialEq)]
pub struct DrawState {
    pub blend: Blend,
    pub depth_test: bool,
    pub depth_func: u32,
    pub depth_write: bool,
    pub color_write: bool,
    pub alpha_to_coverage: bool,
}

impl DrawState {
    // The page's flags: 1 depth test, 2 depth write, 4 colour write.
    pub fn from(blend: &[u32], depth_func: u32, flags: u32) -> DrawState {
        let mut b = [0u32; 7];
        b.copy_from_slice(&blend[..7]);
        DrawState { blend: b, depth_test: flags & 1 != 0, depth_func, depth_write: flags & 2 != 0, color_write: flags & 4 != 0, alpha_to_coverage: flags & 8 != 0 }
    }
}

// A draw's GL state; side: 0 front (back faces culled), 1 back (three
// culls back faces with clockwise front faces), 2 both (no culling).
pub fn apply<C: HasContext, M: MultiDraw>(core: &mut Core<C, M>, state: &DrawState, side: u32) {
    let (gl, st, stats) = (&core.gl, &mut core.state, &mut core.stats);
    st.cull(gl, stats, if side == 2 { 2 } else { 0 });
    st.front_face(gl, stats, if side == 1 { glow::CW } else { glow::CCW });
    st.alpha_to_coverage(gl, stats, state.alpha_to_coverage);
    let b = &state.blend;
    st.blend(gl, stats, b[0] != 0);
    if b[0] != 0 {
        st.blend_equation(gl, stats, b[1], b[2]);
        st.blend_func(gl, stats, b[3], b[4], b[5], b[6]);
    }
    st.depth_test(gl, stats, state.depth_test);
    st.depth_func(gl, stats, state.depth_func);
    st.depth_write(gl, stats, state.depth_write);
    st.color_mask(gl, stats, [state.color_write; 4]);
}

// A pass of a blended bucket: three's program for its side, and the
// material values that program reads (by its slots).
struct Pass {
    program: usize,
    side: u32,
    uniforms: Vec<(usize, Value)>,
}

// One of the map's blended buckets.
struct Bucket<C: HasContext> {
    geometry: usize,
    passes: Vec<Pass>,
    textures: Vec<(u32, C::Texture)>,
    state: DrawState,
    point_lit: bool,
    // Its box (world space): the point lights that reach it.
    bbox: ([f64; 3], [f64; 3]),
}

// Objects: the material three draws them with (OBJECT_*), and their
// records in `objects` (f64), written by the page each frame after the
// count in objects[0].
pub const OBJECT_BASIC: u32 = 0; // MeshBasicMaterial
pub const OBJECT_SPRITE: u32 = 1; // SpriteMaterial
pub const MAX_OBJECTS: usize = 512;
pub const OBJ_STRIDE: usize = 40;
pub const OBJ_INDEX: usize = 0; // the registered object
pub const OBJ_KEY: usize = 1; // three's key: group order, render order, z, id
pub const OBJ_WORLD: usize = 5; // matrixWorld
pub const OBJ_DIFFUSE: usize = 21; // diffuse rgb, opacity
pub const OBJ_ROTATION: usize = 25; // a sprite's rotation
pub const OBJ_CENTER: usize = 26; // a sprite's centre
pub const OBJ_MAP: usize = 28; // mapTransform (3x3, column-major)
pub const OBJ_BUCKET: usize = 37; // 1: a blended bucket's record (index: the bucket), its key alone

// Where a program reads an object's values, and what it holds now.
struct ObjectUniforms<C: HasContext> {
    model_view: Option<C::UniformLocation>,
    model: Option<C::UniformLocation>,
    diffuse: Option<C::UniformLocation>,
    opacity: Option<C::UniformLocation>,
    map_transform: Option<C::UniformLocation>,
    rotation: Option<C::UniformLocation>,
    center: Option<C::UniformLocation>,
    values: [f32; 16],
}

// One registered object: what draws it (three's program per pass), its
// index range, texture and state.
struct Object<C: HasContext> {
    kind: u32,
    geometry: usize,
    start: i32,
    count: i32,
    passes: Vec<(usize, u32)>,
    texture: Option<C::Texture>,
    state: DrawState,
}

pub struct TransparentPass<C: HasContext> {
    pub list: List,
    // Written by the page each frame: three's remaining items' keys.
    pub keys: Vec<f64>,
    // Written by the page each frame: the objects' records (count first).
    pub objects: Vec<f64>,
    buckets: Vec<Bucket<C>>,
    things: Vec<Object<C>>,
    uniforms: HashMap<usize, ObjectUniforms<C>>,
    light_list: Vec<f32>,
}

impl<C: HasContext> TransparentPass<C> {
    pub fn replace_texture(&mut self, old: C::Texture, new: C::Texture) {
        for bucket in &mut self.buckets { for (_, texture) in &mut bucket.textures { if *texture == old { *texture = new; } } }
        for object in &mut self.things { if object.texture == Some(old) { object.texture = Some(new); } }
    }

    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> TransparentPass<C> {
        TransparentPass {
            list: List::reserved(), keys: vec![0.0; MAX_THREE * KEY_STRIDE], objects: vec![0.0; 1 + MAX_OBJECTS * OBJ_STRIDE],
            buckets: vec![], things: vec![], uniforms: HashMap::new(), light_list: Vec::with_capacity(8 * MAX_LIGHTS),
        }
    }

    // A blended bucket. bounds: its box, min and max (world space).
    pub fn add_bucket(&mut self, geometry: u32, blend: &[u32], depth_func: u32, flags: u32, point_lit: bool, bounds: &[f64]) -> u32 {
        self.buckets.push(Bucket {
            geometry: geometry as usize, passes: vec![], textures: vec![], state: DrawState::from(blend, depth_func, flags), point_lit,
            bbox: ([bounds[0], bounds[1], bounds[2]], [bounds[3], bounds[4], bounds[5]]),
        });
        (self.buckets.len() - 1) as u32
    }

    // A pass of the bucket, in drawing order: three's program for `side`.
    pub fn bucket_pass(&mut self, b: u32, program: u32, side: u32) -> u32 {
        let bucket = &mut self.buckets[b as usize];
        bucket.passes.push(Pass { program: program as usize, side, uniforms: vec![] });
        (bucket.passes.len() - 1) as u32
    }

    pub fn bucket_texture<M: MultiDraw>(&mut self, core: &Core<C, M>, b: u32, unit: u32, texture: u32) {
        let t = core.textures[texture as usize];
        self.buckets[b as usize].textures.push((unit, t));
    }

    // A material value a pass's program reads (by the name add_program was given).
    pub fn bucket_uniform<M: MultiDraw>(&mut self, core: &Core<C, M>, b: u32, pass: u32, name: &str, values: &[f32], int: bool) -> bool {
        let p = &mut self.buckets[b as usize].passes[pass as usize];
        let program = &core.programs[p.program];
        let Some(slot) = program.uniforms.iter().position(|(n, _, _)| n == name) else { return false };
        let mut v = [0.0f32; 4];
        v[..values.len().min(4)].copy_from_slice(&values[..values.len().min(4)]);
        p.uniforms.push((slot, if int { Value::I(values[0] as i32) } else { Value::F(values.len(), v) }));
        true
    }

    // An object: kind (OBJECT_*), its geometry's index range, its state as
    // a bucket's.
    #[allow(clippy::too_many_arguments)]
    pub fn add_object(&mut self, kind: u32, geometry: u32, start: i32, count: i32, blend: &[u32], depth_func: u32, flags: u32) -> u32 {
        self.things.push(Object { kind, geometry: geometry as usize, start, count, passes: vec![], texture: None, state: DrawState::from(blend, depth_func, flags) });
        (self.things.len() - 1) as u32
    }

    // A pass of the object, in drawing order: three's program for `side`.
    pub fn object_pass<M: MultiDraw>(&mut self, core: &Core<C, M>, o: u32, program: u32, side: u32) {
        let p = program as usize;
        self.things[o as usize].passes.push((p, side));
        core.programs[p].material_owners.set(core.programs[p].material_owners.get() | 2);
        self.uniforms.entry(p).or_insert_with(|| {
            let (gl, program) = (&core.gl, core.programs[p].program);
            let at = |name: &str| unsafe { gl.get_uniform_location(program, name) };
            ObjectUniforms {
                model_view: at("modelViewMatrix"), model: at("modelMatrix"), diffuse: at("diffuse"), opacity: at("opacity"),
                map_transform: at("mapTransform"), rotation: at("rotation"), center: at("center"), values: [f32::NAN; 16],
            }
        });
    }

    pub fn object_texture<M: MultiDraw>(&mut self, core: &Core<C, M>, o: u32, texture: u32) {
        self.things[o as usize].texture = Some(core.textures[texture as usize]);
    }

    // An object's draw: its program's values from the record, then the draw.
    fn draw_object<M: MultiDraw>(&mut self, core: &mut Core<C, M>, view: &View, rec: usize) {
        let r = &self.objects[1 + rec * OBJ_STRIDE..1 + (rec + 1) * OBJ_STRIDE];
        let Some(o) = self.things.get(r[OBJ_INDEX] as usize) else { return };
        let mut world = [0.0; 16];
        world.copy_from_slice(&r[OBJ_WORLD..OBJ_WORLD + 16]);
        // object.modelViewMatrix as three makes it, uploaded as f32.
        let model_view = math::mul(&view.camera.view, &world);
        let (mut mv32, mut model32, mut map32) = ([0.0f32; 16], [0.0f32; 16], [0.0f32; 9]);
        for k in 0..16 {
            mv32[k] = model_view[k] as f32;
            model32[k] = world[k] as f32;
        }
        for k in 0..9 {
            map32[k] = r[OBJ_MAP + k] as f32;
        }
        // The values a program may hold, by slot: diffuse 0-2, opacity 3,
        // rotation 4, centre 5-6, map transform 7-15.
        let mut values = [0.0f32; 16];
        values[..7].copy_from_slice(&[r[OBJ_DIFFUSE] as f32, r[OBJ_DIFFUSE + 1] as f32, r[OBJ_DIFFUSE + 2] as f32, r[OBJ_DIFFUSE + 3] as f32,
            r[OBJ_ROTATION] as f32, r[OBJ_CENTER] as f32, r[OBJ_CENTER + 1] as f32]);
        values[7..].copy_from_slice(&map32);
        for &(p, side) in &o.passes {
            apply(core, &o.state, side);
            let gl = &core.gl;
            let (state, stats) = (&mut core.state, &mut core.stats);
            state.use_program(gl, stats, core.programs[p].program);
            let u = self.uniforms.get_mut(&p).unwrap();
            // EffectsPass may have written this same program since our last
            // object. Publish this material again rather than trust our copy.
            if core.programs[p].material_owners.get() == 3 { u.values.fill(f32::NAN); }
            unsafe {
                gl.uniform_matrix_4_f32_slice(u.model_view.as_ref(), false, &mv32);
                stats.gl_calls += 1;
                if o.kind == OBJECT_SPRITE && u.model.is_some() {
                    gl.uniform_matrix_4_f32_slice(u.model.as_ref(), false, &model32);
                    stats.gl_calls += 1;
                }
                // The material's values, each only when it differs from what the program holds.
                let mut set = |slot: usize, n: usize, f: &mut dyn FnMut()| {
                    if u.values[slot..slot + n] != values[slot..slot + n] {
                        f();
                        u.values[slot..slot + n].copy_from_slice(&values[slot..slot + n]);
                        stats.gl_calls += 1;
                    }
                };
                if u.diffuse.is_some() { set(0, 3, &mut || gl.uniform_3_f32(u.diffuse.as_ref(), values[0], values[1], values[2])); }
                if u.opacity.is_some() { set(3, 1, &mut || gl.uniform_1_f32(u.opacity.as_ref(), values[3])); }
                if u.rotation.is_some() { set(4, 1, &mut || gl.uniform_1_f32(u.rotation.as_ref(), values[4])); }
                if u.center.is_some() { set(5, 2, &mut || gl.uniform_2_f32(u.center.as_ref(), values[5], values[6])); }
                if u.map_transform.is_some() { set(7, 9, &mut || gl.uniform_matrix_3_f32_slice(u.map_transform.as_ref(), false, &map32)); }
            }
            if let Some(t) = o.texture {
                state.bind_texture(gl, stats, UNIT_MAP, Some(t));
            }
            let geo = &core.geometries[o.geometry];
            unsafe {
                gl.bind_vertex_array(Some(geo.vao));
                gl.draw_elements(glow::TRIANGLES, o.count, geo.index_type, o.start * geo.index_bytes);
            }
            stats.gl_calls += 2;
            stats.pass_draws[PASS_TRANSPARENT] += 1;
        }
    }
}

// Items: a bucket's record is its index; an object's is its record's
// index with this bit set.
const OBJECT_ITEM: u32 = 1 << 31;

// The view the frame block's values in `input` describe.
pub fn view<C: HasContext, M: MultiDraw>(core: &Core<C, M>) -> View {
    let mut projection = [0.0; 16];
    let mut inverse = [0.0; 16];
    projection.copy_from_slice(&core.input[0..16]);
    inverse.copy_from_slice(&core.input[16..32]);
    View { camera: core.camera(), view_projection: math::mul(&projection, &inverse) }
}

impl<C: HasContext, M: MultiDraw> Source<C, M> for TransparentPass<C> {
    fn emit(&mut self, _core: &mut Core<C, M>, _view: &View, source: u32, items: &mut Vec<Item>) {
        // What the page took out of three's list, with three's keys: a
        // bucket's record names the bucket, an object's is its record.
        let n = (self.objects[0] as usize).min(MAX_OBJECTS);
        for k in 0..n {
            let r = &self.objects[1 + k * OBJ_STRIDE..];
            let record = if r[OBJ_BUCKET] != 0.0 {
                let b = r[OBJ_INDEX] as usize;
                if b >= self.buckets.len() || self.buckets[b].passes.is_empty() {
                    continue;
                }
                b as u32
            } else {
                OBJECT_ITEM | k as u32
            };
            items.push(Item { group_order: r[OBJ_KEY], render_order: r[OBJ_KEY + 1], z: r[OBJ_KEY + 2], id: r[OBJ_KEY + 3], source, batch: record, record });
        }
    }

    fn draw_run(&mut self, core: &mut Core<C, M>, view: &View, run: &[Item]) {
        let light_count = view.camera.light_count;
        for item in run {
            if item.record & OBJECT_ITEM != 0 {
                self.draw_object(core, view, (item.record & !OBJECT_ITEM) as usize);
                continue;
            }
            let b = &self.buckets[item.record as usize];
            let mask = if b.point_lit && light_count > 0 {
                light_mask(&core.input[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS], light_count, std::slice::from_ref(&b.bbox), None)
            } else {
                0
            };
            for p in &b.passes {
                apply(core, &b.state, p.side);
                let gl = &core.gl;
                let (state, stats) = (&mut core.state, &mut core.stats);
                let program = &mut core.programs[p.program];
                state.use_program(gl, stats, program.program);
                material_values(gl, stats, program, &p.uniforms);
                if b.point_lit {
                    point_lights(gl, stats, program, &core.frame[FRAME_LIGHTS..FRAME_LIGHTS + 8 * MAX_LIGHTS], light_count, mask, &mut self.light_list);
                }
                for &(unit, t) in &b.textures {
                    state.bind_texture(gl, stats, unit, Some(t));
                }
                let geo = &core.geometries[b.geometry];
                unsafe {
                    gl.bind_vertex_array(Some(geo.vao));
                    gl.draw_elements(glow::TRIANGLES, geo.index_count, geo.index_type, 0);
                }
                stats.gl_calls += 2;
                stats.pass_draws[PASS_TRANSPARENT] += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(g: f64, r: f64, z: f64, id: f64) -> Item {
        Item { group_order: g, render_order: r, z, id, source: 0, batch: 0, record: id as u32 }
    }

    #[test]
    fn sorts_as_three_does() {
        let mut list = List { items: vec![item(0.0, 0.0, 0.5, 3.0), item(0.0, 0.0, 0.9, 7.0), item(0.0, -18.0, 0.1, 9.0), item(1.0, -50.0, 0.99, 1.0), item(0.0, 0.0, 0.5, 2.0)], ..Default::default() };
        list.order(&[], 0);
        let ids: Vec<f64> = list.items.iter().map(|i| i.id).collect();
        // group order, render order, farthest (largest z) first, then id.
        assert_eq!(ids, vec![9.0, 7.0, 2.0, 3.0, 1.0]);
        assert_eq!(list.segments, vec![0, 5]);
    }

    #[test]
    fn segments_around_copy_commands() {
        let mut list = List { items: vec![item(0.0, 0.0, 0.9, 1.0), item(0.0, 0.0, 0.5, 2.0), item(0.0, 0.0, 0.1, 3.0), item(0.0, 10.0, 0.9, 4.0)], ..Default::default() };
        // Two copy commands: one between z 0.9 and 0.5, one after render order 10.
        let keys = [0.0, 0.0, 0.7, 50.0, 0.0, 20.0, 0.5, 51.0];
        list.order(&keys, 2);
        assert_eq!(list.segments, vec![0, 1, 4, 4]);
        assert_eq!(list.segment(0).len(), 1);
        assert_eq!(list.segment(1).iter().map(|i| i.id).collect::<Vec<_>>(), vec![2.0, 3.0, 4.0]);
        assert!(list.segment(2).is_empty());
        assert!(list.segment(3).is_empty());
    }

    #[test]
    fn material_alpha_to_coverage_is_preserved() {
        let blend = [1, glow::FUNC_ADD, glow::FUNC_ADD, glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA];
        let state = DrawState::from(&blend, glow::LEQUAL, 1 | 4 | 8);
        assert!(state.alpha_to_coverage);
        assert!(state.depth_test && state.color_write);
        assert!(!state.depth_write);
        assert!(!DrawState::from(&blend, glow::LEQUAL, 1 | 4).alpha_to_coverage);
    }

    #[test]
    fn z_as_three() {
        let vp = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.002, -1.0, 0.0, 0.0, -0.2002, 0.0];
        assert_eq!(three_z(&vp, None, &[1.0, 2.0, -10.0]), -1.002 * -10.0 + -0.2002);
        let mut m = [0.0; 16];
        m[0] = 1.0; m[5] = 1.0; m[10] = 1.0; m[15] = 1.0; m[14] = -5.0;
        assert_eq!(three_z(&vp, Some(&m), &[0.0, 0.0, 0.0]), -1.002 * -5.0 + -0.2002);
    }
}
