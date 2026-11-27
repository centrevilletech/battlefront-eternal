//! Regression tests for per-frame instance, motion, shadow-caster, and packet
//! limits, GL 3.3 1024-texel texture packing, zero-allocation packing/overflow,
//! single-charge drop accounting, tied draw ordering, and log throttling.
use bfe_render::{
    core::{Camera, MultiDraw},
    frame_plan::layout::{meta as M, role as R},
    instances as I,
    math,
    motion as MO,
    native_casters::Casters,
    native_motion::{NativeMotion, VIEW_RESERVE},
    renderer::Renderer,
    shader::{self, Kind, ROW_TEXELS},
    frame::FrameTargets,
};
use glow::HasContext;
use std::cell::RefCell;
use std::ffi::c_void;

#[global_allocator]
static ALLOCATOR: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

struct MockDevice {
    max_tex: i32,
    next_id: u32,
    bound_tex: u32,
    last_uniform_i: i32,
    last_buf_size: i32,
    record_events: bool,
    frame_buffer: u32,
    winding: u32,
    cull_face: u32,
    trace_faces: bool,
    submissions: Vec<(Option<i32>, u32, u32, u32)>,
    tex_sizes: Vec<(u32, i32, i32)>,
    tex_image_calls: usize,
    tex_sub_calls: usize,
    max_seen_width: i32,
    max_seen_height: i32,
    sub_exceeded_tex: bool,
    dim_exceeded_max: bool,
    uploaded_rows: Vec<f32>,
    drawn_bases: Vec<(i32, i32)>,
    skin_sub_uploads: usize,
    tracked_skin_textures: Vec<u32>,
}

impl Default for MockDevice {
    fn default() -> Self {
        Self {
            max_tex: 1024,
            next_id: 0,
            bound_tex: 0,
            last_uniform_i: 0,
            last_buf_size: 0,
            record_events: true,
            frame_buffer: 0,
            winding: glow::CCW,
            cull_face: glow::BACK,
            trace_faces: false,
            submissions: Vec::new(),
            tex_sizes: Vec::new(),
            tex_image_calls: 0,
            tex_sub_calls: 0,
            max_seen_width: 0,
            max_seen_height: 0,
            sub_exceeded_tex: false,
            dim_exceeded_max: false,
            uploaded_rows: Vec::new(),
            drawn_bases: Vec::new(),
            skin_sub_uploads: 0,
            tracked_skin_textures: Vec::new(),
        }
    }
}

thread_local! {
    static MOCK: RefCell<MockDevice> = RefCell::new(MockDevice::default());
}

struct NoMulti;
impl MultiDraw for NoMulti {
    fn multi_draw(&self, _: u32, _: &mut [i32], _: u32, _: &mut [i32]) {}
}

extern "system" fn gl_string(_: u32) -> *const u8 {
    c"3.3 synthetic".as_ptr().cast()
}
unsafe extern "system" fn gl_get_integerv(pname: u32, out: *mut i32) {
    let val = if pname == glow::MAX_TEXTURE_SIZE {
        MOCK.with_borrow(|d| d.max_tex)
    } else {
        0
    };
    unsafe { *out = val };
}
unsafe extern "system" fn gl_gen(n: i32, out: *mut u32) {
    MOCK.with_borrow_mut(|d| {
        for i in 0..n.max(0) as usize {
            d.next_id += 1;
            unsafe { *out.add(i) = d.next_id };
        }
    });
}
extern "system" fn gl_del(_: i32, _: *const u32) {}
extern "system" fn gl_create_obj() -> u32 {
    MOCK.with_borrow_mut(|d| {
        d.next_id += 1;
        d.next_id
    })
}
extern "system" fn gl_create_shader(_: u32) -> u32 {
    MOCK.with_borrow_mut(|d| {
        d.next_id += 1;
        d.next_id
    })
}
extern "system" fn gl_shader_source(_: u32, _: i32, _: *const *const u8, _: *const i32) {}
extern "system" fn gl_noop1(_: u32) {}
extern "system" fn depth_range(_: f32, _: f32) {}
extern "system" fn clear_depth(_: f32) {}
extern "system" fn depth_mask(_: u8) {}
extern "system" fn front_face(mode: u32) { MOCK.with_borrow_mut(|d| d.winding = mode); }
extern "system" fn cull_face(mode: u32) { MOCK.with_borrow_mut(|d| d.cull_face = mode); }
extern "system" fn draw_elements(_: u32, _: i32, _: u32, _: *const c_void) {
    MOCK.with_borrow_mut(|d| if d.trace_faces {
        d.submissions.push((None, d.frame_buffer, d.winding, d.cull_face));
    });
}
extern "system" fn gl_noop2(_: u32, _: u32) {}
extern "system" fn gl_noop3(_: u32, _: u32, _: i32) {}
extern "system" fn gl_noop4_u(_: u32, _: u32, _: u32, _: u32) {}
extern "system" fn gl_noop4_b(_: u8, _: u8, _: u8, _: u8) {}
extern "system" fn gl_noop4_i(_: i32, _: i32, _: i32, _: i32) {}
extern "system" fn gl_noop4_f(_: f32, _: f32, _: f32, _: f32) {}
unsafe extern "system" fn gl_get_iv(_: u32, pname: u32, out: *mut i32) {
    let val = if pname == glow::COMPILE_STATUS || pname == glow::LINK_STATUS { 1 } else { 0 };
    unsafe { *out = val };
}
unsafe extern "system" fn gl_get_buf_iv(_: u32, _: u32, out: *mut i32) {
    let sz = MOCK.with_borrow(|d| d.last_buf_size);
    unsafe { *out = sz };
}
extern "system" fn gl_bind_attrib(_: u32, _: u32, _: *const u8) {}
extern "system" fn gl_get_uniform_loc(_: u32, _: *const u8) -> i32 {
    1
}
extern "system" fn gl_get_block_idx(_: u32, _: *const u8) -> u32 {
    0
}
extern "system" fn gl_uniform_block_binding(_: u32, _: u32, _: u32) {}
extern "system" fn gl_bind_buffer_base(_: u32, index: u32, buffer: u32) {
    if index == 0 { MOCK.with_borrow_mut(|d| d.frame_buffer = buffer); }
}
extern "system" fn gl_buffer_data(_: u32, size: isize, _: *const c_void, _: u32) {
    MOCK.with_borrow_mut(|d| d.last_buf_size = size as i32);
}
extern "system" fn gl_buffer_sub_data(_: u32, _: isize, _: isize, _: *const c_void) {}
extern "system" fn gl_attrib_ptr(_: u32, _: i32, _: u32, _: u8, _: i32, _: *const c_void) {}
extern "system" fn gl_bind_texture(_: u32, id: u32) {
    MOCK.with_borrow_mut(|d| d.bound_tex = id);
}
extern "system" fn gl_tex_image_2d(
    _: u32,
    _: i32,
    _: i32,
    width: i32,
    height: i32,
    _: i32,
    _: u32,
    _: u32,
    _: *const c_void,
) {
    MOCK.with_borrow_mut(|d| {
        d.tex_image_calls += 1;
        d.max_seen_width = d.max_seen_width.max(width);
        d.max_seen_height = d.max_seen_height.max(height);
        if width > d.max_tex || height > d.max_tex {
            d.dim_exceeded_max = true;
        }
        if d.record_events {
            if let Some(entry) = d.tex_sizes.iter_mut().find(|e| e.0 == d.bound_tex) {
                entry.1 = width;
                entry.2 = height;
            } else {
                d.tex_sizes.push((d.bound_tex, width, height));
            }
        }
    });
}
unsafe extern "system" fn gl_tex_sub_image_2d(
    _: u32,
    _: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    _: u32,
    _: u32,
    pixels: *const c_void,
) {
    MOCK.with_borrow_mut(|d| {
        d.tex_sub_calls += 1;
        d.max_seen_width = d.max_seen_width.max(x + width);
        d.max_seen_height = d.max_seen_height.max(y + height);
        if x + width > d.max_tex || y + height > d.max_tex {
            d.dim_exceeded_max = true;
        }
        if let Some(&(_, tw, th)) = d.tex_sizes.iter().find(|e| e.0 == d.bound_tex) {
            if x + width > tw || y + height > th {
                d.sub_exceeded_tex = true;
            }
        }
        if d.tracked_skin_textures.contains(&d.bound_tex) {
            d.skin_sub_uploads += 1;
        }
        if d.record_events && !pixels.is_null() && width == (I::PACK_COLS_MAX * ROW_TEXELS) as i32 {
            let floats = (width as usize) * (height as usize) * 4;
            let slice = unsafe { std::slice::from_raw_parts(pixels.cast::<f32>(), floats) };
            d.uploaded_rows.clear();
            d.uploaded_rows.extend_from_slice(slice);
        }
    });
}
extern "system" fn gl_uniform_1i(_: i32, v: i32) {
    MOCK.with_borrow_mut(|d| d.last_uniform_i = v);
}
extern "system" fn gl_uniform_1f(_: i32, _: f32) {}
extern "system" fn gl_uniform_2f(_: i32, _: f32, _: f32) {}
extern "system" fn gl_uniform_mat4(_: i32, _: i32, _: u8, _: *const f32) {}
extern "system" fn gl_draw_instanced(_: u32, _: i32, _: u32, _: *const c_void, primcount: i32) {
    MOCK.with_borrow_mut(|d| {
        if d.record_events {
            d.drawn_bases.push((d.last_uniform_i, primcount));
            if d.trace_faces {
                d.submissions.push((Some(d.last_uniform_i), d.frame_buffer, d.winding, d.cull_face));
            }
        }
    });
}
extern "system" fn gl_fb_tex(_: u32, _: u32, _: u32, _: u32, _: i32) {}
extern "system" fn gl_check_fb(_: u32) -> u32 {
    glow::FRAMEBUFFER_COMPLETE
}

fn synthetic_renderer(max_tex: i32) -> Renderer<glow::Context, NoMulti> {
    MOCK.with_borrow_mut(|d| {
        *d = MockDevice {
            max_tex,
            ..MockDevice::default()
        };
    });
    let gl = unsafe {
        glow::Context::from_loader_function(|name| match name {
            "glGetString" => gl_string as *const c_void,
            "glGetIntegerv" => gl_get_integerv as *const c_void,
            "glGenBuffers" | "glGenTextures" | "glGenVertexArrays" | "glGenFramebuffers" => gl_gen as *const c_void,
            "glDeleteBuffers" | "glDeleteTextures" | "glDeleteVertexArrays" | "glDeleteFramebuffers" => gl_del as *const c_void,
            "glCreateProgram" => gl_create_obj as *const c_void,
            "glCreateShader" => gl_create_shader as *const c_void,
            "glShaderSource" => gl_shader_source as *const c_void,
            "glCompileShader" | "glLinkProgram" | "glDeleteShader" | "glDeleteProgram"
            | "glUseProgram" | "glBindVertexArray" | "glEnableVertexAttribArray"
            | "glActiveTexture" | "glEnable" | "glDisable"
            | "glDepthFunc" | "glClear" | "glStencilMask" => gl_noop1 as *const c_void,
            "glAttachShader" | "glDetachShader" | "glBindBuffer" | "glBindFramebuffer" | "glVertexAttribDivisor"
            | "glBlendEquationSeparate" => gl_noop2 as *const c_void,
            "glDrawElements" => draw_elements as *const c_void,
            "glFrontFace" => front_face as *const c_void,
            "glCullFace" => cull_face as *const c_void,
            "glDepthMask" => depth_mask as *const c_void,
            "glBindTexture" => gl_bind_texture as *const c_void,
            "glGetShaderiv" | "glGetProgramiv" => gl_get_iv as *const c_void,
            "glGetBufferParameteriv" => gl_get_buf_iv as *const c_void,
            "glBindAttribLocation" => gl_bind_attrib as *const c_void,
            "glGetUniformLocation" => gl_get_uniform_loc as *const c_void,
            "glGetUniformBlockIndex" => gl_get_block_idx as *const c_void,
            "glUniformBlockBinding" => gl_uniform_block_binding as *const c_void,
            "glBindBufferBase" => gl_bind_buffer_base as *const c_void,
            "glBufferData" => gl_buffer_data as *const c_void,
            "glBufferSubData" => gl_buffer_sub_data as *const c_void,
            "glVertexAttribPointer" => gl_attrib_ptr as *const c_void,
            "glTexParameteri" => gl_noop3 as *const c_void,
            "glTexImage2D" => gl_tex_image_2d as *const c_void,
            "glTexSubImage2D" => gl_tex_sub_image_2d as *const c_void,
            "glUniform1i" => gl_uniform_1i as *const c_void,
            "glUniform1f" => gl_uniform_1f as *const c_void,
            "glUniform2f" => gl_uniform_2f as *const c_void,
            "glUniformMatrix4fv" => gl_uniform_mat4 as *const c_void,
            "glBlendFuncSeparate" => gl_noop4_u as *const c_void,
            "glColorMask" => gl_noop4_b as *const c_void,
            "glViewport" => gl_noop4_i as *const c_void,
            "glClearColor" => gl_noop4_f as *const c_void,
            "glDrawElementsInstanced" => gl_draw_instanced as *const c_void,
            "glFramebufferTexture2D" => gl_fb_tex as *const c_void,
            "glCheckFramebufferStatus" => gl_check_fb as *const c_void,
            "glPixelStorei" => gl_noop2 as *const c_void,
            "glDepthRangef" => depth_range as *const c_void,
            "glClearDepthf" => clear_depth as *const c_void,
            _ => std::ptr::null(),
        })
    };
    Renderer::new(gl, NoMulti)
}

struct Targets;
impl FrameTargets for Targets {
    fn bind(&mut self, _: &[f64]) -> Result<(), String> { Ok(()) }
    fn resolve(&mut self, _: i32) -> Result<(), String> { Ok(()) }
    fn copy(&mut self, _: i32, _: i32, _: u32) -> Result<(), String> { Ok(()) }
    fn mipmap(&mut self, _: i32) -> Result<(), String> { Ok(()) }
}

fn identity_camera() -> Camera {
    let id = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];
    Camera {
        view: id,
        frustum: math::Frustum::from_matrix(&id),
        light_count: 0,
    }
}

fn register_basic_draw_targets(r: &mut Renderer<glow::Context, NoMulti>) -> (u32, u32, u32) {
    let vs = "#version 300 es\nuniform mat4 viewMatrix;\nuniform mat4 modelMatrix;\nin vec3 position;\nvoid main() { gl_Position = viewMatrix * modelMatrix * vec4(position, 1.0); }\n";
    let fs = "#version 300 es\nprecision highp float;\nuniform float sunScale;\nout vec4 c;\nvoid main() { c = vec4(sunScale); }\n";
    let prog = r.core.add_program(Kind::Rigid, vs, fs, &[("position".into(), 0)], &[]).unwrap();
    let geo = r.core.add_geometry();
    r.core.geometry_index(geo, &[0, 0, 1, 0, 2, 0], glow::UNSIGNED_SHORT);
    let blend = r.instances.add_blend(&[1, glow::FUNC_ADD, glow::FUNC_ADD, glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA], 1, glow::LEQUAL, &[prog, 0]);
    (prog, geo, blend)
}

fn write_record(input: &mut [f64], index: usize, kind: f64, groups: usize, tx: f64) {
    let row = &mut input[I::IN_RECORDS + index * I::REC_STRIDE..I::IN_RECORDS + (index + 1) * I::REC_STRIDE];
    row.fill(0.0);
    row[I::REC_KIND] = kind;
    row[I::REC_PROGRAM] = 0.0;
    row[I::REC_GEOMETRY] = 0.0;
    row[I::REC_GROUPS] = groups as f64;
    for g in 0..4 {
        row[I::REC_TEXTURES + g] = -1.0;
    }
    row[I::REC_SPHERE + 3] = 0.05;
    row[I::REC_WORLD] = 1.0;
    row[I::REC_WORLD + 5] = 1.0;
    row[I::REC_WORLD + 10] = 1.0;
    row[I::REC_WORLD + 12] = tx;
    row[I::REC_WORLD + 15] = 1.0;
    row[I::REC_BLEND] = 0.0;
}

#[test]
fn renderer_respects_1024_and_128_max_texture_size_and_grows_lazily() {
    let mut r = synthetic_renderer(1024);
    assert_eq!(r.instances.instance_texture_size(), (128, 128));
    assert_eq!(r.instances.row_capacity(), I::MAX_ROWS);
    assert_eq!(r.instances.bone_texture_size(), (1024, 64));
    assert_eq!(r.instances.bone_capacity(), I::MAX_BONES);
    MOCK.with_borrow(|d| {
        assert!(!d.dim_exceeded_max, "initial texture exceeded 1024 limit");
        assert!(d.max_seen_width <= 1024 && d.max_seen_height <= 1024);
    });

    register_basic_draw_targets(&mut r);
    r.core.input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
    let camera = identity_camera();

    // Under INITIAL_ROWS (512): stays at 128x128 (256 KiB).
    for i in 0..100 {
        write_record(&mut r.core.input, i, 2.0, 1, 0.0);
    }
    r.instances.overflow.begin_frame();
    r.instances.draw(&mut r.core, &camera, 100);
    assert_eq!(r.instances.instance_texture_size(), (128, 128));
    assert_eq!(r.instances.overflow.total_drops(), 0);

    // Full 2048 rows (1024 records x 2 groups): grows lazily to 128x512, still <= 1024.
    for i in 0..I::MAX_RECORDS {
        write_record(&mut r.core.input, i, 2.0, 2, 0.0);
    }
    r.instances.overflow.begin_frame();
    r.instances.draw(&mut r.core, &camera, I::MAX_RECORDS);
    assert_eq!(r.instances.instance_texture_size(), (128, 512));
    assert_eq!(r.instances.overflow.total_drops(), 0);
    assert_eq!(r.instances.overflow.peak_rows, I::MAX_ROWS);
    MOCK.with_borrow(|d| {
        assert!(!d.dim_exceeded_max, "grown texture exceeded 1024 limit");
        assert!(!d.sub_exceeded_tex, "texSubImage2D exceeded allocated texture");
        assert!(d.max_seen_width <= 1024 && d.max_seen_height <= 1024);
    });

    // Extreme 128-texel device limit: degrades gracefully to 512 rows and counts excess.
    let mut small = synthetic_renderer(128);
    assert_eq!(small.instances.instance_texture_size(), (128, 128));
    assert_eq!(small.instances.row_capacity(), 512);
    assert_eq!(small.instances.bone_texture_size(), (128, 128));
    assert_eq!(small.instances.bone_capacity(), 4096);
    register_basic_draw_targets(&mut small);
    small.core.input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
    for i in 0..600 {
        write_record(&mut small.core.input, i, 2.0, 1, 0.0);
    }
    small.instances.overflow.begin_frame();
    small.instances.draw(&mut small.core, &camera, 600);
    assert_eq!(small.instances.overflow.dropped_rows, 88);
    assert_eq!(small.instances.overflow.peak_rows, 512);
    MOCK.with_borrow(|d| {
        assert!(!d.dim_exceeded_max, "small-limit texture exceeded 128");
        assert!(!d.sub_exceeded_tex, "small-limit sub upload exceeded texture");
    });
}

#[test]
fn instance_draw_and_overflow_path_performs_zero_heap_allocations() {
    let mut r = synthetic_renderer(1024);
    register_basic_draw_targets(&mut r);
    r.core.input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
    let camera = identity_camera();

    // Full opaque + blended overflow batch (500 x 4 opaque = 2000 rows + 524 blended).
    for i in 0..500 {
        write_record(&mut r.core.input, i, 2.0, 4, 0.0);
    }
    for i in 500..I::MAX_RECORDS {
        write_record(&mut r.core.input, i, I::KIND_BLENDED_RIGID, 1, 0.0);
    }

    // Disable vector recording in the mock GL stubs while counting heap allocations.
    MOCK.with_borrow_mut(|d| d.record_events = false);

    // Measure first overflowing opaque+blended call AND a full blended-only overflow call
    // directly after admission (no warmup required for blended vectors or sort scratch).
    let before = bfe_test_alloc::thread_allocations();

    r.instances.overflow.begin_frame();
    r.instances.draw(&mut r.core, &camera, I::MAX_RECORDS + 8);

    for i in 0..(I::MAX_BLENDED + 12) {
        write_record(&mut r.core.input, i, I::KIND_BLENDED_RIGID, 1, 0.0);
    }
    r.instances.overflow.begin_frame();
    r.instances.draw(&mut r.core, &camera, I::MAX_BLENDED + 12);

    let allocs = bfe_test_alloc::thread_allocations().wrapping_sub(before);
    assert_eq!(allocs, 0, "InstancePass::draw allocated {allocs} times on full/overflow batches");
    assert_eq!(r.instances.overflow.dropped_records, 8);
    assert!(r.instances.overflow.dropped_blended > 0);
    assert!(r.instances.overflow.dropped_rows > 0);
}

#[test]
fn mixed_row_and_blend_exhaustion_charges_each_omitted_item_once() {
    let mut r = synthetic_renderer(1024);
    register_basic_draw_targets(&mut r);
    r.core.input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
    let camera = identity_camera();

    // 509 opaque records x 4 groups = 2036 opaque rows + 25 blended records.
    // Only 12 blended rows fit in MAX_ROWS (2048); 13 blended items are omitted solely by row cap.
    for i in 0..509 {
        write_record(&mut r.core.input, i, 2.0, 4, 0.0);
    }
    for i in 509..(509 + 25) {
        write_record(&mut r.core.input, i, I::KIND_BLENDED_RIGID, 1, 0.0);
    }
    r.instances.overflow.begin_frame();
    r.instances.draw(&mut r.core, &camera, 509 + 25);
    assert_eq!(r.instances.overflow.dropped_rows, 13);
    assert_eq!(r.instances.overflow.dropped_blended, 0);
    assert_eq!(r.instances.overflow.frame_drops, 13);
    assert_eq!(r.instances.overflow.total_drops(), 13);

    // Mixed cap exhaustion: 500 opaque x 4 = 2000 rows, plus 524 blended records.
    // 12 omitted by MAX_BLENDED (512), then 464 of the remaining 512 omitted by MAX_ROWS (only 48 rows left).
    // Total omitted = 524 - 48 = 476, charged once each (12 + 464 = 476).
    let mut mixed = synthetic_renderer(1024);
    register_basic_draw_targets(&mut mixed);
    mixed.core.input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
    for i in 0..500 {
        write_record(&mut mixed.core.input, i, 2.0, 4, 0.0);
    }
    for i in 500..(500 + 524) {
        write_record(&mut mixed.core.input, i, I::KIND_BLENDED_RIGID, 1, 0.0);
    }
    mixed.instances.overflow.begin_frame();
    mixed.instances.draw(&mut mixed.core, &camera, 500 + 524);
    assert_eq!(mixed.instances.overflow.dropped_blended, 12);
    assert_eq!(mixed.instances.overflow.dropped_rows, 464);
    assert_eq!(mixed.instances.overflow.frame_drops, 476);
    assert_eq!(mixed.instances.overflow.total_drops(), 476);
}

#[test]
fn tied_opaque_and_transparent_order_is_preserved_across_old_64_limit() {
    let mut r = synthetic_renderer(1024);
    register_basic_draw_targets(&mut r);
    r.core.input[shader::FRAME_OFFSETS[18] + 1] = 1000.0;
    let camera = identity_camera();

    // 80 opaque records with identical draw keys and distinct translation markers (small values inside unit frustum).
    for i in 0..80 {
        write_record(&mut r.core.input, i, 2.0, 1, (i as f64) * 0.001);
    }
    // Insert the depth-tied groups in descending ID order, three records per ID.
    for i in 0..96 {
        let idx = 80 + i;
        write_record(&mut r.core.input, idx, I::KIND_BLENDED_RIGID, 1, 0.2 + (i as f64) * 0.001);
        let base = I::IN_RECORDS + idx * I::REC_STRIDE;
        r.core.input[base + I::REC_BLEND + 1] = 0.0; // group_order
        r.core.input[base + I::REC_BLEND + 2] = 0.0; // render_order
        r.core.input[base + I::REC_BLEND + 3] = ((95 - i) / 3) as f64;
    }
    r.instances.overflow.begin_frame();
    r.instances.draw(&mut r.core, &camera, 80 + 96);
    assert_eq!(r.instances.overflow.total_drops(), 0);

    // Check uploaded instance texture rows 0..80 preserved exact tied insertion order.
    MOCK.with_borrow(|d| {
        for i in 0..80 {
            let tx = d.uploaded_rows[i * ROW_TEXELS * 4 + 12];
            let expected = (i as f32) * 0.001;
            assert!((tx - expected).abs() < 1e-6, "opaque tie order broken at row {i}: got {tx}, want {expected}");
        }
    });

    r.frame_reserve();
    r.frame_begin();
    for at in [0, 16] { r.core.input[at..at + 16].copy_from_slice(&camera.view); }
    assert!(r.frame_publish(R::TRANSPARENT));
    MOCK.with_borrow_mut(|d| d.drawn_bases.clear());
    r.frame_aux(&mut Targets, R::TRANSPARENT).unwrap();
    let expected: Vec<_> = (0..32).rev().flat_map(|group| {
        (0..3).map(move |member| (80 + group * 3 + member, 1))
    }).collect();
    MOCK.with_borrow(|d| assert_eq!(d.drawn_bases, expected));
}

#[test]
fn overflow_log_cadence_throttles_rising_and_constant_excess_and_retains_interval_peak() {
    // 1. Steadily rising drops every frame: must NOT flood stderr on every new peak.
    let mut rising = I::InstanceOverflow::default();
    let mut emitted = Vec::new();
    for frame in 1..=601u32 {
        rising.begin_frame();
        rising.note_record_drop(frame as usize);
        if rising.finish_frame() {
            emitted.push((frame, rising.last_report_peak_drops));
        }
    }
    assert_eq!(emitted, vec![(1, 1), (301, 301), (601, 601)]);
    assert_eq!(rising.overflow_frames, 601);

    // 2. Constant excess: emits at exact LOG_INTERVAL_FRAMES intervals.
    let mut constant = I::InstanceOverflow::default();
    let mut const_emitted = Vec::new();
    for frame in 1..=601u32 {
        constant.begin_frame();
        constant.note_row_drop(5);
        if constant.finish_frame() {
            const_emitted.push((frame, constant.last_report_peak_drops));
        }
    }
    assert_eq!(const_emitted, vec![(1, 5), (301, 5), (601, 5)]);

    // 3. Mid-interval spike retained until interval report.
    let mut spike = I::InstanceOverflow::default();
    let mut spike_emitted = Vec::new();
    for frame in 1..=301u32 {
        spike.begin_frame();
        let drops = match frame {
            1 => 2,
            120 => 99,
            301 => 3,
            _ => 0,
        };
        if drops > 0 {
            spike.note_blended_drop(drops);
        }
        if spike.finish_frame() {
            spike_emitted.push((frame, spike.last_report_peak_drops));
        }
    }
    assert_eq!(spike_emitted, vec![(1, 2), (301, 99)]);
}

#[test]
fn frame_publish_ignores_stale_records_on_secondary_far_roles_and_counts_bone_truncation() {
    let mut r = synthetic_renderer(1024);
    r.frame_reserve();
    r.frame_begin();

    // Secondary FAR roles with stale nonzero M::RECORDS must stay valid and unclipped.
    for role in [R::MIRROR0_FAR, R::MIRROR1_FAR, R::PROBE0, R::PROBE23] {
        r.frames.meta[role * M::LEN + M::RECORDS] = 17.0;
        r.frames.meta[role * M::LEN + M::BONES] = 0.0;
        assert!(r.frame_publish(role), "secondary FAR role {role} failed with stale M::RECORDS");
    }
    assert_eq!(r.instances.overflow.total_drops(), 0);

    // Primary NEAR role with bone count above MAX_BONES reports truncation in overflow.dropped_bones.
    r.frames.meta[R::NEAR * M::LEN + M::RECORDS] = 4.0;
    r.frames.meta[R::NEAR * M::LEN + M::BONES] = (I::MAX_BONES + 13) as f64;
    assert!(r.frame_publish(R::NEAR));
    assert_eq!(r.frames.meta[R::NEAR * M::LEN + M::BONES], I::MAX_BONES as f64);
    assert_eq!(r.instances.overflow.dropped_bones, 13);
}

#[test]
fn native_casters_keep_stable_slots_reuse_free_slots_and_count_saturation() {
    let mut r = synthetic_renderer(1024);
    let mut skin_tex_ids = Vec::new();
    let mut raw_gl_ids = Vec::new();
    for _ in 0..4 {
        let gl_tex = unsafe { r.core.gl.create_texture().unwrap() };
        MOCK.with_borrow_mut(|d| {
            raw_gl_ids.push(d.next_id);
        });
        skin_tex_ids.push(r.core.add_texture(gl_tex));
    }
    MOCK.with_borrow_mut(|d| d.tracked_skin_textures = raw_gl_ids);
    let mut casters = Casters::new(&mut r.core, &skin_tex_ids, 2).unwrap();

    // Write distinct bone matrices for 7 potential records.
    for rec in 0..7 {
        let row = &mut r.core.input[I::IN_RECORDS + rec * I::REC_STRIDE..I::IN_RECORDS + (rec + 1) * I::REC_STRIDE];
        row.fill(0.0);
        row[I::REC_KIND] = 1.0; // skinned
        row[I::REC_BONES] = 1.0;
        row[I::REC_BONE_AT] = rec as f64;
        r.core.bones[rec * 16] = (rec + 1) as f32;
    }

    // Frame 1: records 0, 1, 2 skinned -> 3 uploads into slots 0, 1, 2.
    MOCK.with_borrow_mut(|d| d.skin_sub_uploads = 0);
    assert_eq!(casters.prepare(&mut r.core, 3).unwrap(), 0);
    assert_eq!(MOCK.with_borrow(|d| d.skin_sub_uploads), 3);

    // Frame 2: record 0 becomes NO_SHADOW while records 1 and 2 keep identical bones.
    // Stable slot retention means records 1 and 2 stay in slots 1 and 2 -> 0 uploads.
    r.core.input[I::IN_RECORDS + I::REC_FLAGS] = I::NO_SHADOW as f64;
    MOCK.with_borrow_mut(|d| d.skin_sub_uploads = 0);
    assert_eq!(casters.prepare(&mut r.core, 3).unwrap(), 0);
    assert_eq!(MOCK.with_borrow(|d| d.skin_sub_uploads), 0, "later skinned casters thrashed slots");

    // Frame 3: records 0, 1 are rigid (free slots 0, 1), records 2, 3, 4, 5, 6 are skinned (5 skinned > 4 slots).
    // Records 2, 3 take slots 2, 3; records 4, 5 reuse free slots 0, 1; record 6 overflows (dropped_skins = 1).
    r.core.input[I::IN_RECORDS + I::REC_FLAGS] = 0.0;
    r.core.input[I::IN_RECORDS + I::REC_KIND] = 2.0;
    r.core.input[I::IN_RECORDS + I::REC_STRIDE + I::REC_KIND] = 2.0;
    assert_eq!(casters.prepare(&mut r.core, 7).unwrap(), 1);
}

#[test]
fn native_motion_reserves_first_person_slots_and_counts_overflow() {
    let mut r = synthetic_renderer(1024);
    r.motion.enable_extended_records();
    assert_eq!(r.motion.input.len(), MO::IN_RECORDS + 512 * MO::REC_STRIDE);
    let geo = r.core.add_geometry();
    r.core.geometry_index(geo, &[0, 0, 1, 0, 2, 0], glow::UNSIGNED_SHORT);
    let mut native = NativeMotion::for_pass(&mut r.core, &r.motion).unwrap();

    let total_moving = MO::MAX_RECORDS + 20;
    assert!(total_moving <= I::MAX_RECORDS);

    // Frame 1: seed history for all total_moving rigid records.
    for i in 0..total_moving {
        write_record(&mut r.core.input, i, 2.0, 1, i as f64);
        let base = I::IN_RECORDS + i * I::REC_STRIDE;
        r.core.input[base + I::REC_MOTION_ID] = (i + 1) as f64;
        r.core.input[base + I::REC_MOTION_GENERATION] = 1.0;
    }
    assert_eq!(native.prepare(&r.core, &mut r.motion, total_moving, false), 0);
    native.complete(true);

    // Frame 2: move every record so all total_moving want velocity proxies.
    for i in 0..total_moving {
        let base = I::IN_RECORDS + i * I::REC_STRIDE;
        r.core.input[base + I::REC_WORLD + 12] += 1.0;
    }
    let dropped_scene = native.prepare(&r.core, &mut r.motion, total_moving, true);
    let expected_scene = MO::MAX_RECORDS - VIEW_RESERVE;
    assert_eq!(native.count, expected_scene);
    assert_eq!(dropped_scene, total_moving - expected_scene);

    // First-person view still has its full VIEW_RESERVE slots guaranteed.
    let dropped_view = native.view(&r.core, &mut r.motion, VIEW_RESERVE + 4);
    assert_eq!(native.count, MO::MAX_RECORDS);
    assert_eq!(dropped_view, 4);
}

#[test]
fn temporal_history_reserves_two_pose_buffers_for_the_admitted_sources() {
    for (extended, records) in [(false, 256), (true, 1024)] {
        let mut r = synthetic_renderer(1024);
        if extended { r.motion.enable_extended_records(); }
        let bytes = records * std::mem::size_of::<[f64; I::REC_STRIDE]>();
        let bytes = u32::try_from(bytes).expect("a pose buffer is far below 4 GiB");
        let (history, buffers) = bfe_test_alloc::measure_thread_of_size(bytes, || NativeMotion::for_pass(&mut r.core, &r.motion));
        assert!(history.is_ok());
        assert_eq!(buffers, 2, "{records} source records per pose buffer");
    }
}

#[test]
fn original_velocity_admission_fills_the_scene_then_places_the_view() {
    let mut r = synthetic_renderer(1024);
    assert_eq!(r.motion.input.len(), MO::IN_RECORDS + 256 * MO::REC_STRIDE);
    register_basic_draw_targets(&mut r);
    let mut native = NativeMotion::new(&mut r.core).unwrap();
    for i in 0..250 {
        write_record(&mut r.core.input, i, 0., 1, 0.);
        r.core.input[I::IN_RECORDS + i * I::REC_STRIDE + I::REC_MOTION_ID] = (i + 1) as f64;
    }
    native.prepare(&r.core, &mut r.motion, 250, false);
    native.complete(true);
    for i in 0..250 {
        r.core.input[I::IN_RECORDS + i * I::REC_STRIDE + I::REC_WORLD + 12] = 1.;
    }
    assert_eq!(native.prepare(&r.core, &mut r.motion, 250, true), 0);
    assert_eq!(native.count, 250);
    assert_eq!(native.view(&r.core, &mut r.motion, 16), 10);
    assert_eq!(native.count, 256);
}

#[test]
fn a_shared_skin_palette_preserves_the_last_instances_previous_pose() {
    for extended in [false, true] {
        let mut r = synthetic_renderer(1024);
        if extended { r.motion.enable_extended_records(); }
        register_basic_draw_targets(&mut r);
        let mut native = NativeMotion::for_pass(&mut r.core, &r.motion).unwrap();
        for i in 0..200 {
            write_record(&mut r.core.input, i, 1., 1, 0.);
            let at = I::IN_RECORDS + i * I::REC_STRIDE;
            r.core.input[at + I::REC_BONE_AT] = 7.;
            r.core.input[at + I::REC_BONES] = 96.;
            r.core.input[at + I::REC_MOTION_ID] = (i + 1) as f64;
        }
        native.prepare(&r.core, &mut r.motion, 200, false);
        native.complete(true);
        r.core.input[I::IN_RECORDS + 199 * I::REC_STRIDE + I::REC_WORLD + 12] = 0.375;
        assert_eq!(native.prepare(&r.core, &mut r.motion, 200, true), 0);
        assert_eq!(native.count, 1);
        assert_eq!(r.motion.input[MO::IN_RECORDS + MO::REC_PREVIOUS_WORLD + 12], 0.);
        assert_eq!(r.motion.input[MO::IN_RECORDS + MO::REC_WORLD + 12], 0.375);
    }
}

#[test]
fn far_world_and_instances_share_depth_order_and_restore_winding() {
    use bfe_render::{core::{VIEW_ACTORS, VIEW_FAR}, far::*};
    let mut r = synthetic_renderer(1024);
    let (prog, geo, _) = register_basic_draw_targets(&mut r);
    let table = r.far.add_table(&mut r.core, prog, "", &[], &[]).unwrap();
    let blend = [glow::FUNC_ADD, glow::FUNC_ADD, glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA];
    let item = r.far.add_item(&r.core, prog, table, geo,
        F_COLOR_WRITE | F_TRANSPARENT | F_BLEND | F_DOUBLE_BACK_FRONT,
        glow::LEQUAL, &blend, &[0.; 4], &[0., 0., 0., 1.], 32);
    r.core.bind_frame(VIEW_ACTORS);
    let actors = MOCK.with_borrow(|d| d.frame_buffer);
    r.core.bind_frame(VIEW_FAR);
    let world = MOCK.with_borrow(|d| d.frame_buffer);
    assert_ne!(world, actors);
    r.frame_reserve();
    let id = identity_camera().view;
    for (shown, count, enabled) in [(true, 3, true), (false, 3, true), (true, 0, true), (true, 3, false)] {
        r.frame_begin();
        for at in [0, 16] { r.core.input[at..at + 16].copy_from_slice(&id); }
        for (i, z) in [-0.3_f64, 0.3].into_iter().enumerate() {
            write_record(&mut r.core.input, i, I::KIND_BLENDED_RIGID, 1, 0.);
            let at = I::IN_RECORDS + i * I::REC_STRIDE + I::REC_WORLD;
            r.core.input[at] = -1.;
            r.core.input[at + 14] = z;
        }
        r.far.input[FAR_DRAW] = f64::from(enabled);
        r.far.input[32] = f64::from(shown);
        r.far.range(item, 0, count);
        r.frames.meta[R::FAR * M::LEN + M::RECORDS] = 2.;
        assert!(r.frame_publish(R::FAR));
        MOCK.with_borrow_mut(|d| { d.trace_faces = true; d.submissions.clear(); });
        r.frame_aux(&mut Targets, R::FAR).unwrap();
        let mut expected = vec![(Some(1), actors, glow::CW, glow::BACK)];
        if shown && count > 0 && enabled {
            expected.extend([(None, world, glow::CCW, glow::FRONT), (None, world, glow::CCW, glow::BACK)]);
        }
        expected.push((Some(0), actors, glow::CW, glow::BACK));
        MOCK.with_borrow(|d| {
            assert_eq!(d.submissions, expected, "shown={shown} count={count} enabled={enabled}");
            assert_eq!((d.winding, d.frame_buffer), (glow::CCW, world));
        });
    }
}

#[test]
fn shadow_publication_crosses_both_old_growth_thresholds_without_allocating() {
    use bfe_render::{shadow as S, static_depth::Region};
    let mut r = synthetic_renderer(1024);
    register_basic_draw_targets(&mut r);
    let texture = unsafe { r.core.gl.create_texture().unwrap() };
    let texture = r.core.add_texture(texture);
    let mut casters = Casters::new(&mut r.core, &[texture], 1).unwrap();
    let camera = identity_camera().view;
    let region = Region { inverse: camera, half: [1.; 3], radius: None };
    let mut projection = camera;
    projection[0] = 0.04;
    r.frame_reserve();
    r.shadow.admit_native_commands(4096);
    r.frame_shadow_reserve();
    MOCK.with_borrow_mut(|d| d.record_events = false);
    for (groups, regional) in [(1, false), (2, false), (4, false), (4, true), (4, false)] {
        for i in 0..1024 {
            write_record(&mut r.core.input, i, 2., groups, if i == 500 { 8. } else { 0. });
        }
        let before = bfe_test_alloc::thread_allocations();
        r.frame_begin();
        casters.set_region(&r.core.input, 1024, regional.then_some(&region));
        assert_eq!(casters.prepare(&mut r.core, 1024).unwrap(), 0);
        for role in R::SHADOW_DYNAMIC0..=R::SHADOW_DYNAMIC3 {
            r.shadow.input[S::S_TARGET] = -1.;
            casters.publish(&r.core, &mut r.shadow.input, 1024, &camera, &projection).unwrap();
            assert!(r.frame_publish(role));
        }
        r.shadow.input[S::S_DRAWS] = -1.;
        r.frame_aux(&mut Targets, R::SHADOW_DYNAMIC0).unwrap();
        assert_eq!(bfe_test_alloc::thread_allocations().wrapping_sub(before), 0, "groups={groups}, regional={regional}");
        assert_eq!(r.shadow.input[S::S_DRAWS] as usize, (1024 - usize::from(regional)) * groups);
        assert_eq!(r.core.input[I::IN_RECORDS + 500 * I::REC_STRIDE + I::REC_FLAGS], 0.);
    }
}
