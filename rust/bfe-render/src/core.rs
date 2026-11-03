// What every pass of the renderer shares: the GL context (three's, in the
// page) and its multi-draw, the programs made from three's GLSL, the
// geometries and textures the page registered, the far scene's image, the
// inputs the page writes each frame (the frame block's values and the
// passes' records as f64, the drawn soldiers' bones as f32), a frame block
// for each view, the GL state the passes leave behind them, and the stats of
// the phase being drawn.
//
// A phase (renderer.rs) starts from nothing known about the GL state, since
// three drew in between, and each pass changes only what differs from what
// the last draw left (GlState), counting every call it makes.
use crate::math::{self, Frustum, Mat4};
use crate::shader::{self, Kind, Stage, FRAME_FLOATS, FRAME_LIGHT_COUNT, MAX_LIGHTS};
use glow::HasContext;

// Fixed texture units.
pub const UNIT_MAP: u32 = 0;
pub const UNIT_FAR: u32 = 1;
pub const UNIT_BUMP: u32 = 2;
pub const UNIT_BONES: u32 = 14;
pub const UNIT_INSTANCES: u32 = 15;
// The tree canopies' state texture, and the terrain's layers.
pub const UNIT_TREE_STATES: u32 = 3;
pub const UNIT_LAYER1: u32 = 4;
pub const UNIT_DETAIL: u32 = 5;
pub const UNIT_LAYERS: u32 = 6; // layerMap0..7: 6..13
const UNITS: usize = 16;
const SAMPLERS: &[(&str, u32)] = &[("map", UNIT_MAP), ("farMap", UNIT_FAR), ("bumpMap", UNIT_BUMP), ("boneTexture", UNIT_BONES), ("bfeInstances", UNIT_INSTANCES),
    ("treeStates", UNIT_TREE_STATES), ("layer1Map", UNIT_LAYER1), ("detailMap", UNIT_DETAIL),
    ("layerMap0", UNIT_LAYERS), ("layerMap1", UNIT_LAYERS + 1), ("layerMap2", UNIT_LAYERS + 2), ("layerMap3", UNIT_LAYERS + 3),
    ("layerMap4", UNIT_LAYERS + 4), ("layerMap5", UNIT_LAYERS + 5), ("layerMap6", UNIT_LAYERS + 6), ("layerMap7", UNIT_LAYERS + 7)];

// The views a frame draws, each with a frame block of its own: the near
// scene, the far scene (its own camera range), the first-person view scene,
// one spare (a mirror's near scene), and the far scene seen from a water's
// mirror (water.js reflect, drawn before the frame's own far scene).
pub const VIEW_NEAR: usize = 0;
pub const VIEW_FAR: usize = 1;
pub const VIEW_FIRST_PERSON: usize = 2;
pub const VIEW_SPARE: usize = 3;
pub const VIEW_FAR_MIRROR: usize = 4;
pub const VIEWS: usize = 5;

// The passes, in the order the renderer makes them and its stats count
// their draws.
pub const PASS_WORLD: usize = 0;
pub const PASS_INSTANCES: usize = 1;
pub const PASS_FAR: usize = 2;
pub const PASS_TERRAIN: usize = 3;
pub const PASS_TRANSPARENT: usize = 4;
pub const PASS_EFFECTS: usize = 5;
pub const PASS_FOLIAGE: usize = 6;
pub const PASS_VIEW: usize = 7;
pub const PASS_POST: usize = 8;
pub const PASS_HUD: usize = 9;
pub const PASS_WATER: usize = 10;
pub const PASSES: usize = 11;

// The multi-draw call, which glow does not wrap (WEBGL_multi_draw on the web).
pub trait MultiDraw {
    fn multi_draw(&self, mode: u32, counts: &mut [i32], element_type: u32, offsets: &mut [i32]);
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Value {
    F(usize, [f32; 4]),
    I(i32),
}

pub(crate) struct Program<C: HasContext> {
    pub(crate) program: C::Program,
    // Uniforms set from a draw's material (light-region values...), each with
    // the value the program holds now.
    pub(crate) uniforms: Vec<(String, Option<C::UniformLocation>, Option<Value>)>,
    pub(crate) zero_lights: Option<C::UniformLocation>,
    pub(crate) zero_light_count: Option<C::UniformLocation>,
    pub(crate) instance_base: Option<C::UniformLocation>,
    pub(crate) lights: Vec<f32>,
    pub(crate) light_count: i32,
}

pub(crate) struct Geometry<C: HasContext> {
    pub(crate) vao: C::VertexArray,
    pub(crate) buffers: Vec<C::Buffer>,
    pub(crate) index_type: u32,
    pub(crate) index_bytes: i32,
    pub(crate) index_count: i32,
    pub(crate) groups: Vec<(i32, i32)>,
}

// What a phase counts. The first five are what the page has always read
// (the near phase's world and instanced draws, its instances, program
// switches and GL calls); each pass's draws follow, in pass order.
#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub world_draws: u32,
    pub instance_draws: u32,
    pub instances: u32,
    pub program_switches: u32,
    pub gl_calls: u32,
    pub pass_draws: [u32; PASSES],
}

impl Stats {
    // As a phase returns them to the page.
    pub fn values(&self) -> Vec<u32> {
        let mut v = vec![self.world_draws, self.instance_draws, self.instances, self.program_switches, self.gl_calls];
        v.extend_from_slice(&self.pass_draws);
        v
    }
}

// What the passes of a view read from its frame block: the camera's view
// matrix and frustum (three's, in f64), and how many of the frame's point
// lights there are.
pub struct Camera {
    pub view: Mat4,
    pub frustum: Frustum,
    pub light_count: usize,
}

// The GL state as the passes left it, None where it is unknown: a call is
// made only when a draw wants something else. reset() at a phase's start,
// as three may have changed anything since. Each call is counted in the
// phase's stats.
pub struct GlState<C: HasContext> {
    program: Option<C::Program>,
    cull: Option<u32>,
    alpha_to_coverage: Option<bool>,
    depth_write: Option<bool>,
    front: Option<u32>,
    unit: Option<u32>,
    bound: [Option<Option<C::Texture>>; UNITS],
    // For the passes that blend or test depth otherwise; the near phase's
    // opaque passes only set them once, at its start.
    blend: Option<bool>,
    blend_equation: Option<(u32, u32)>,
    blend_func: Option<(u32, u32, u32, u32)>,
    depth_test: Option<bool>,
    depth_func: Option<u32>,
    color_mask: Option<[bool; 4]>,
}

impl<C: HasContext> Default for GlState<C> {
    fn default() -> Self {
        GlState {
            program: None, cull: None, alpha_to_coverage: None, depth_write: None, front: None, unit: None, bound: [None; UNITS],
            blend: None, blend_equation: None, blend_func: None, depth_test: None, depth_func: None, color_mask: None,
        }
    }
}

impl<C: HasContext> GlState<C> {
    pub fn reset(&mut self) {
        *self = GlState::default();
    }

    pub fn use_program(&mut self, gl: &C, stats: &mut Stats, program: C::Program) {
        if self.program != Some(program) {
            unsafe { gl.use_program(Some(program)) };
            self.program = Some(program);
            stats.program_switches += 1;
            stats.gl_calls += 1;
        }
    }

    pub fn bind_texture(&mut self, gl: &C, stats: &mut Stats, unit: u32, texture: Option<C::Texture>) {
        if self.bound[unit as usize] != Some(texture) {
            if self.unit != Some(unit) {
                unsafe { gl.active_texture(glow::TEXTURE0 + unit) };
                self.unit = Some(unit);
                stats.gl_calls += 1;
            }
            unsafe { gl.bind_texture(glow::TEXTURE_2D, texture) };
            self.bound[unit as usize] = Some(texture);
            stats.gl_calls += 1;
        }
    }

    // A material's side as three culls it: 0 front (back faces culled), 1
    // back (front faces culled), 2 both (no culling).
    pub fn cull(&mut self, gl: &C, stats: &mut Stats, side: u32) {
        if self.cull != Some(side) {
            unsafe {
                match side {
                    2 => gl.disable(glow::CULL_FACE),
                    s => {
                        if self.cull.is_none() || self.cull == Some(2) {
                            gl.enable(glow::CULL_FACE);
                            stats.gl_calls += 1;
                        }
                        gl.cull_face(if s == 1 { glow::FRONT } else { glow::BACK });
                    }
                }
            }
            self.cull = Some(side);
            stats.gl_calls += 1;
        }
    }

    pub fn alpha_to_coverage(&mut self, gl: &C, stats: &mut Stats, on: bool) {
        if self.alpha_to_coverage != Some(on) {
            unsafe {
                if on { gl.enable(glow::SAMPLE_ALPHA_TO_COVERAGE) } else { gl.disable(glow::SAMPLE_ALPHA_TO_COVERAGE) }
            }
            self.alpha_to_coverage = Some(on);
            stats.gl_calls += 1;
        }
    }

    pub fn depth_write(&mut self, gl: &C, stats: &mut Stats, on: bool) {
        if self.depth_write != Some(on) {
            unsafe { gl.depth_mask(on) };
            self.depth_write = Some(on);
            stats.gl_calls += 1;
        }
    }

    // A draw's culling, alpha to coverage and depth write, in that order.
    pub fn set_state(&mut self, gl: &C, stats: &mut Stats, cull: u32, alpha_to_coverage: bool, depth_write: bool) {
        self.cull(gl, stats, cull);
        self.alpha_to_coverage(gl, stats, alpha_to_coverage);
        self.depth_write(gl, stats, depth_write);
    }

    pub fn front_face(&mut self, gl: &C, stats: &mut Stats, front: u32) {
        if self.front != Some(front) {
            unsafe { gl.front_face(front) };
            self.front = Some(front);
            stats.gl_calls += 1;
        }
    }

    pub fn blend(&mut self, gl: &C, stats: &mut Stats, on: bool) {
        if self.blend != Some(on) {
            unsafe {
                if on { gl.enable(glow::BLEND) } else { gl.disable(glow::BLEND) }
            }
            self.blend = Some(on);
            stats.gl_calls += 1;
        }
    }

    pub fn blend_equation(&mut self, gl: &C, stats: &mut Stats, rgb: u32, alpha: u32) {
        if self.blend_equation != Some((rgb, alpha)) {
            unsafe { gl.blend_equation_separate(rgb, alpha) };
            self.blend_equation = Some((rgb, alpha));
            stats.gl_calls += 1;
        }
    }

    pub fn blend_func(&mut self, gl: &C, stats: &mut Stats, src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32) {
        let func = (src_rgb, dst_rgb, src_alpha, dst_alpha);
        if self.blend_func != Some(func) {
            unsafe { gl.blend_func_separate(src_rgb, dst_rgb, src_alpha, dst_alpha) };
            self.blend_func = Some(func);
            stats.gl_calls += 1;
        }
    }

    pub fn depth_test(&mut self, gl: &C, stats: &mut Stats, on: bool) {
        if self.depth_test != Some(on) {
            unsafe {
                if on { gl.enable(glow::DEPTH_TEST) } else { gl.disable(glow::DEPTH_TEST) }
            }
            self.depth_test = Some(on);
            stats.gl_calls += 1;
        }
    }

    pub fn depth_func(&mut self, gl: &C, stats: &mut Stats, func: u32) {
        if self.depth_func != Some(func) {
            unsafe { gl.depth_func(func) };
            self.depth_func = Some(func);
            stats.gl_calls += 1;
        }
    }

    pub fn color_mask(&mut self, gl: &C, stats: &mut Stats, mask: [bool; 4]) {
        if self.color_mask != Some(mask) {
            unsafe { gl.color_mask(mask[0], mask[1], mask[2], mask[3]) };
            self.color_mask = Some(mask);
            stats.gl_calls += 1;
        }
    }
}

pub struct Core<C: HasContext, M: MultiDraw> {
    pub gl: C,
    pub(crate) multi: M,
    pub(crate) programs: Vec<Program<C>>,
    pub(crate) geometries: Vec<Geometry<C>>,
    pub(crate) textures: Vec<C::Texture>,
    pub(crate) far_map: Option<C::Texture>,
    frame_ubos: [C::Buffer; VIEWS],
    // The frame block as last uploaded (f32, as three uploads it).
    pub(crate) frame: Vec<f32>,
    pub input: Vec<f64>,
    pub bones: Vec<f32>,
    pub state: GlState<C>,
    pub stats: Stats,
    pub log: Vec<String>,
}

pub(crate) fn upload_texture<C: HasContext>(gl: &C, width: i32, height: i32) -> C::Texture {
    unsafe {
        let t = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA32F as i32, width, height, 0, glow::RGBA, glow::FLOAT, glow::PixelUnpackData::Slice(None));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        t
    }
}

pub(crate) fn bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

impl<C: HasContext, M: MultiDraw> Core<C, M> {
    // input_len and bones_len: the inputs' sizes, the frame block's values
    // and every pass's records (instances.rs), and the bones.
    pub fn new(gl: C, multi: M, input_len: usize, bones_len: usize) -> Core<C, M> {
        let frame_ubos = std::array::from_fn(|_| unsafe {
            let ubo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
            gl.buffer_data_size(glow::UNIFORM_BUFFER, (FRAME_FLOATS * 4) as i32, glow::DYNAMIC_DRAW);
            gl.bind_buffer(glow::UNIFORM_BUFFER, None);
            ubo
        });
        Core {
            gl, multi, programs: vec![], geometries: vec![], textures: vec![], far_map: None, frame_ubos, frame: vec![0.0; FRAME_FLOATS],
            input: vec![0.0; input_len], bones: vec![0.0; bones_len], state: GlState::default(), stats: Stats::default(), log: vec![],
        }
    }

    pub fn add_texture(&mut self, t: C::Texture) -> u32 {
        self.textures.push(t);
        (self.textures.len() - 1) as u32
    }

    pub fn set_far_map(&mut self, t: u32) {
        self.far_map = Some(self.textures[t as usize]);
    }

    // A program from three's final GLSL for one of its materials. attributes:
    // the locations three's own program gave each input, so a layout made
    // for three's matches.
    pub fn add_program(&mut self, kind: Kind, vs: &str, fs: &str, attributes: &[(String, u32)], material_uniforms: &[String]) -> Result<u32, String> {
        let gl = &self.gl;
        let vs = shader::rewrite(vs, Stage::Vertex, kind)?;
        let fs = shader::rewrite(fs, Stage::Fragment, kind)?;
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = vec![];
            for (stage, src) in [(glow::VERTEX_SHADER, &vs), (glow::FRAGMENT_SHADER, &fs)] {
                let s = gl.create_shader(stage)?;
                gl.shader_source(s, src);
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    let log = gl.get_shader_info_log(s);
                    gl.delete_shader(s);
                    gl.delete_program(program);
                    return Err(format!("{} shader: {}", if stage == glow::VERTEX_SHADER { "vertex" } else { "fragment" }, log));
                }
                gl.attach_shader(program, s);
                shaders.push(s);
            }
            for (name, location) in attributes {
                gl.bind_attrib_location(program, *location, name);
            }
            gl.link_program(program);
            for s in shaders {
                gl.detach_shader(program, s);
                gl.delete_shader(s);
            }
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(format!("link: {log}"));
            }
            if let Some(block) = gl.get_uniform_block_index(program, "BfeFrame") {
                gl.uniform_block_binding(program, block, 0);
            }
            gl.use_program(Some(program));
            for (name, unit) in SAMPLERS {
                if let Some(l) = gl.get_uniform_location(program, name) {
                    gl.uniform_1_i32(Some(&l), *unit as i32);
                }
            }
            let uniforms = material_uniforms.iter().map(|n| (n.clone(), gl.get_uniform_location(program, n), None)).collect();
            let p = Program {
                program, uniforms,
                zero_lights: gl.get_uniform_location(program, "zeroLights"),
                zero_light_count: gl.get_uniform_location(program, "zeroLightCount"),
                instance_base: gl.get_uniform_location(program, "bfeInstanceBase"),
                lights: vec![], light_count: -1,
            };
            gl.use_program(None);
            self.programs.push(p);
        }
        Ok((self.programs.len() - 1) as u32)
    }

    pub fn add_geometry(&mut self) -> u32 {
        unsafe {
            let vao = self.gl.create_vertex_array().unwrap();
            self.geometries.push(Geometry { vao, buffers: vec![], index_type: glow::UNSIGNED_INT, index_bytes: 4, index_count: 0, groups: vec![] });
        }
        (self.geometries.len() - 1) as u32
    }

    // A vertex buffer of the geometry (one attribute's, or several
    // interleaved); its slot for geometry_pointer.
    pub fn geometry_buffer(&mut self, g: u32, data: &[u8]) -> u32 {
        let gl = &self.gl;
        let geo = &mut self.geometries[g as usize];
        unsafe {
            let b = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, data, glow::STATIC_DRAW);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            geo.buffers.push(b);
        }
        (geo.buffers.len() - 1) as u32
    }

    #[allow(clippy::too_many_arguments)]
    pub fn geometry_pointer(&mut self, g: u32, buffer: u32, location: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: i32) {
        let gl = &self.gl;
        let geo = &self.geometries[g as usize];
        unsafe {
            gl.bind_vertex_array(Some(geo.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(geo.buffers[buffer as usize]));
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_f32(location, size, ty, normalized, stride, offset);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    pub fn geometry_index(&mut self, g: u32, data: &[u8], ty: u32) {
        let gl = &self.gl;
        let geo = &mut self.geometries[g as usize];
        unsafe {
            gl.bind_vertex_array(Some(geo.vao));
            let b = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(b));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, data, glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            geo.buffers.push(b);
        }
        geo.index_type = ty;
        geo.index_bytes = if ty == glow::UNSIGNED_SHORT { 2 } else { 4 };
        geo.index_count = data.len() as i32 / geo.index_bytes;
    }

    pub fn geometry_group(&mut self, g: u32, start: i32, count: i32) {
        self.geometries[g as usize].groups.push((start, count));
    }

    // A phase's start: its stats from zero, nothing known of the GL state.
    pub fn begin_phase(&mut self) {
        self.stats = Stats::default();
        self.state.reset();
    }

    // A view's frame block: the page's values (input[0..FRAME_FLOATS]) as
    // three uploads them (f32), into the view's buffer, bound where every
    // program's BfeFrame reads (binding 0).
    pub fn upload_frame(&mut self, view: usize) {
        for i in 0..FRAME_FLOATS {
            self.frame[i] = self.input[i] as f32;
        }
        let ubo = self.frame_ubos[view];
        unsafe {
            self.gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
            self.gl.buffer_sub_data_u8_slice(glow::UNIFORM_BUFFER, 0, bytes(&self.frame));
            self.gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(ubo));
        }
        self.stats.gl_calls += 3;
    }

    // The camera and light count of the frame block's values in `input`.
    pub fn camera(&self) -> Camera {
        let input = &self.input;
        let mut projection = [0.0; 16];
        let mut view = [0.0; 16];
        projection.copy_from_slice(&input[0..16]);
        view.copy_from_slice(&input[16..32]);
        let frustum = Frustum::from_matrix(&math::mul(&projection, &view));
        Camera { view, frustum, light_count: (input[FRAME_LIGHT_COUNT] as usize).min(MAX_LIGHTS) }
    }

    // The state the opaque passes draw in, whatever three left: depth tested
    // (less or equal; each draw says whether it writes), no blending, polygon
    // offset or stencil, every channel written, counter-clockwise front faces.
    // renderer.resetState() follows the phase.
    pub fn opaque(&mut self) {
        let (gl, state, stats) = (&self.gl, &mut self.state, &mut self.stats);
        state.depth_test(gl, stats, true);
        state.depth_func(gl, stats, glow::LEQUAL);
        state.blend(gl, stats, false);
        unsafe {
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.disable(glow::STENCIL_TEST);
        }
        stats.gl_calls += 2;
        state.color_mask(gl, stats, [true; 4]);
        state.front_face(gl, stats, glow::CCW);
    }

    // The far scene's image on its unit, for the programs that read it (farMap).
    pub fn bind_far_map(&mut self) {
        self.state.bind_texture(&self.gl, &mut self.stats, UNIT_FAR, self.far_map);
    }

    // Two views' frame block buffers swapped: a phase that draws one view's
    // passes for another camera (renderer.rs mirror_frame) uploads into a
    // buffer of its own, not one the frame's draws may still be reading.
    pub fn swap_views(&mut self, a: usize, b: usize) {
        self.frame_ubos.swap(a, b);
    }

    // A view's frame block as last uploaded, bound again where the programs
    // read it (a phase drawn later in the same frame).
    pub fn bind_frame(&mut self, view: usize) {
        unsafe { self.gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(self.frame_ubos[view])) };
        self.stats.gl_calls += 1;
    }

    // A phase's end: no vertex array bound and counter-clockwise front faces,
    // as three's resetState expects; the phase's stats.
    pub fn end_phase(&mut self) -> Stats {
        unsafe {
            self.gl.bind_vertex_array(None);
            self.gl.front_face(glow::CCW);
        }
        self.state.front = Some(glow::CCW);
        self.stats.gl_calls += 2;
        self.stats
    }
}
