//! Headless OpenGL pixel verification for the refraction opacity curve and
//! near-eye billboard fade shaders, routed through the safe `Bridge` and
//! `GpuResources` particle draw pipeline.
use crate::{
    render_bridge::Bridge,
    resources::{Ranges, Resources},
};
use bfe_render::{
    cameras::layout::shader_frame as F,
    effects as render,
    transparent::{Item, Source, View},
};
use glow::HasContext;
use glutin::{
    api::egl::{
        context::PossiblyCurrentContext,
        device::Device,
        display::Display,
        surface::Surface,
    },
    config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
    context::{ContextApi, ContextAttributesBuilder, Version},
    prelude::*,
    surface::{PbufferSurface, SurfaceAttributesBuilder},
};
use serde_json::json;
use std::{num::NonZeroU32, sync::Mutex};

static GL_LOCK: Mutex<()> = Mutex::new(());

const SIZE: i32 = 64;
const NO_BLEND: [u32; 7] = [
    0,
    glow::FUNC_ADD,
    glow::FUNC_ADD,
    glow::ONE,
    glow::ZERO,
    glow::ONE,
    glow::ZERO,
];

type Context = (glow::Context, glow::Context, glow::Context, Option<Surface<PbufferSurface>>, PossiblyCurrentContext);

struct HeadlessRender {
    bridge: Bridge,
    gpu: Resources,
    target_tex: u32,
    _surface: Option<Surface<PbufferSurface>>,
    _context: PossiblyCurrentContext,
}

impl HeadlessRender {
    /// A headless context, or `None` (with one line saying why) on a machine that cannot make
    /// one, so a test run there is a skip. Everything after the context exists is a real check:
    /// a failure to set up the draw target or the shaders panics.
    fn open_or_skip(test_name: &str) -> Option<Self> {
        match Self::open_context() {
            Ok((gl_core, gl_ranges, gl_gpu, surface, context)) => Some(
                Self::init(gl_core, gl_ranges, gl_gpu, surface, context)
                    .unwrap_or_else(|e| panic!("{test_name}: draw target setup failed: {e}")),
            ),
            Err(err) => {
                // Written to the process's own stderr: the test harness hides what a passing
                // test prints with `eprintln!`, and a skipped run must show in the log.
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr(),
                    "skipping {test_name}: no headless EGL context can be created here ({err})"
                );
                None
            }
        }
    }

    fn open_context() -> Result<Context, String> {
        let devices = Device::query_devices().map_err(|e| format!("egl devices: {e}"))?;
        let mut last_err = String::from("no usable EGL device found");
        for device in devices {
            // Upstream glutin EGL context creation and glow proc-address loading
            // require FFI blocks with no safe headless opener in the workspace.
            let opened: Result<Context, String> = unsafe {
                let display = match Display::with_device(&device, None) {
                    Ok(d) => d,
                    Err(e) => {
                        last_err = format!("egl display: {e}");
                        continue;
                    }
                };
                let mut found = Err(last_err.clone());
                'configs: for surface_type in [ConfigSurfaceTypes::empty(), ConfigSurfaceTypes::PBUFFER] {
                    let template = ConfigTemplateBuilder::new()
                        .with_surface_type(surface_type)
                        .build();
                    let Some(config) = display.find_configs(template).ok().and_then(|mut it| it.next()) else {
                        continue;
                    };
                    for api in [
                        ContextApi::OpenGl(Some(Version::new(3, 3))),
                        ContextApi::Gles(Some(Version::new(3, 0))),
                    ] {
                        let attrs = ContextAttributesBuilder::new()
                            .with_context_api(api)
                            .build(None);
                        let Ok(not_current) = display.create_context(&config, &attrs) else {
                            continue;
                        };
                        let current = if surface_type.is_empty() {
                            not_current
                                .make_current_surfaceless()
                                .map(|ctx| (ctx, None))
                                .map_err(|e| e.to_string())
                        } else {
                            let pbuf_attrs = SurfaceAttributesBuilder::<PbufferSurface>::new()
                                .build(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap());
                            display
                                .create_pbuffer_surface(&config, &pbuf_attrs)
                                .map_err(|e| e.to_string())
                                .and_then(|surf| {
                                    not_current
                                        .make_current(&surf)
                                        .map(|ctx| (ctx, Some(surf)))
                                        .map_err(|e| e.to_string())
                                })
                        };
                        match current {
                            Ok((context, surface)) => {
                                let make_gl = || {
                                    glow::Context::from_loader_function_cstr(|name| {
                                        display.get_proc_address(name)
                                    })
                                };
                                found = Ok((make_gl(), make_gl(), make_gl(), surface, context));
                                break 'configs;
                            }
                            Err(e) => found = Err(e),
                        }
                    }
                }
                found
            };
            match opened {
                Ok(opened) => return Ok(opened),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn init(
        gl_core: glow::Context,
        gl_ranges: glow::Context,
        gl_gpu: glow::Context,
        surface: Option<Surface<PbufferSurface>>,
        context: PossiblyCurrentContext,
    ) -> Result<Self, String> {
        let mut bridge = Bridge::new(gl_core, Ranges::new(gl_ranges));
        let mut gpu = Resources::new(gl_gpu);

        // 90 deg FOV, aspect 1.0, near 0.1, far 100.0; identity view, eye at origin.
        let near = 0.1_f64;
        let far = 100.0_f64;
        let proj = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, -(far + near) / (far - near), -1.0,
            0.0, 0.0, -(2.0 * far * near) / (far - near), 0.0,
        ];
        let view = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        bridge.r.core.input[F::PROJECTION..F::PROJECTION + 16].copy_from_slice(&proj);
        bridge.r.core.input[F::VIEW..F::VIEW + 16].copy_from_slice(&view);
        bridge.r.core.input[F::EYE..F::EYE + 3].copy_from_slice(&[0.0, 0.0, 0.0]);
        let fade_at = bfe_render::shader::FRAME_OFFSETS[18];
        bridge.r.core.input[fade_at..fade_at + 6].copy_from_slice(&[
            500.0,
            1000.0,
            SIZE as f64,
            SIZE as f64,
            500.0,
            1000.0,
        ]);
        bridge.r.core.upload_frame(0);

        let src = gpu.source(
            &json!({
                "width": SIZE,
                "height": SIZE,
                "depth": 1,
                "target": glow::TEXTURE_2D,
                "format": 1023,
                "dataType": 1009,
                "srgb": false,
                "renderTarget": true
            })
            .to_string(),
        )?;
        let smp = gpu.sample(
            src,
            r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}"#,
        )?;
        gpu.upload(smp)?;
        let target_tex = bridge.r.core.add_texture(gpu.texture(smp)?);

        Ok(Self {
            bridge,
            gpu,
            target_tex,
            _surface: surface,
            _context: context,
        })
    }

    fn add_rgba8_texture(
        &mut self,
        width: i32,
        height: i32,
        pixels: &[u8],
        mipmaps: bool,
    ) -> Result<u32, String> {
        let src = self.gpu.source(
            &json!({
                "width": width,
                "height": height,
                "depth": 1,
                "target": glow::TEXTURE_2D,
                "format": 1023,
                "dataType": 1009,
                "srgb": false
            })
            .to_string(),
        )?;
        self.gpu.source_pixels(src, pixels)?;
        let min_filter = if mipmaps { 1008 } else { 1006 };
        let smp = self.gpu.sample(
            src,
            &json!({
                "wrapS": 1001,
                "wrapT": 1001,
                "wrapR": 1001,
                "min": min_filter,
                "mag": 1006,
                "anisotropy": 1,
                "flip": false,
                "premultiply": false,
                "alignment": 4,
                "mipmaps": mipmaps,
                "compare": null
            })
            .to_string(),
        )?;
        self.gpu.upload(smp)?;
        Ok(self.bridge.r.core.add_texture(self.gpu.texture(smp)?))
    }

    fn add_rgba32f_texture(&mut self, width: i32, height: i32, data: &[f32]) -> Result<u32, String> {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let src = self.gpu.source(
            &json!({
                "width": width,
                "height": height,
                "depth": 1,
                "target": glow::TEXTURE_2D,
                "format": 1023,
                "dataType": 1015,
                "srgb": false
            })
            .to_string(),
        )?;
        self.gpu.source_pixels(src, &bytes)?;
        let smp = self.gpu.sample(
            src,
            r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#,
        )?;
        self.gpu.upload(smp)?;
        Ok(self.bridge.r.core.add_texture(self.gpu.texture(smp)?))
    }

    fn draw_and_read(&mut self, batch: u32, at: usize, row: &[f32]) -> Vec<u8> {
        self.bridge.r.effects.data[at..at + row.len()].copy_from_slice(row);
        self.bridge.r.effects.draw_into(
            &mut self.bridge.r.core,
            batch as usize,
            self.target_tex,
            SIZE,
            SIZE,
            1,
            at,
            1,
        );
        read_rgba(&self.bridge.r.core.gl)
    }
}

fn read_rgba(gl: &glow::Context) -> Vec<u8> {
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    // Upstream glow::HasContext::read_pixels is an FFI call; Scene::pixels requires a full battle scene.
    unsafe {
        gl.read_pixels(
            0,
            0,
            SIZE,
            SIZE,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );
    }
    pixels
}

fn pixel_at(pixels: &[u8], x: i32, y: i32) -> [u8; 4] {
    let idx = ((y * SIZE + x) * 4) as usize;
    [pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3]]
}

fn attr_loc(spec: &bfe_render::programs::Spec, name: &str) -> u32 {
    spec.attributes
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("missing attribute {name}"))
        .1
}

fn cpu_row(pos: [f32; 3], color: [f32; 4], size: f32, shape: f32) -> [f32; 21] {
    [
        pos[0], pos[1], pos[2],
        color[0], color[1], color[2], color[3],
        size, 0.0, size, shape,
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0, 1.0,
    ]
}

fn assert_near_u8(actual: u8, expected: u8, tol: u8, label: &str) {
    assert!(
        actual.abs_diff(expected) <= tol,
        "{label}: expected {expected} +/- {tol}, got {actual}"
    );
}

#[test]
fn refraction_shader_renders_opacity_curve_blur_shift_and_near_fade() {
    let _guard = GL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut ctx) = HeadlessRender::open_or_skip("refraction_shader_renders_opacity_curve_blur_shift_and_near_fade") else {
        return;
    };
    let spec = bfe_render::programs::spec("effects/particle-cpu").unwrap();
    let (base_vertex, _) = bfe_render::programs::sources(spec);
    let vertex = super::refraction::refraction_vertex(&base_vertex).unwrap();
    let attribs: Vec<_> = spec
        .attributes
        .iter()
        .map(|(name, slot)| ((*name).to_owned(), *slot))
        .collect();
    let uniforms = [
        "modelViewMatrix",
        "projectionMatrix",
        "map",
        "sceneColor",
        "blurSettings",
    ]
    .map(str::to_owned);
    let program = ctx
        .bridge
        .r
        .core
        .add_program(
            bfe_render::shader::Kind::Effect,
            &vertex,
            include_str!("refraction.frag"),
            &attribs,
            &uniforms,
        )
        .unwrap();

    let white_map = ctx.add_rgba8_texture(1, 1, &[255, 255, 255, 255], false).unwrap();
    let mut grad = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _y in 0..SIZE {
        for x in 0..SIZE {
            let r = ((x * 255) / (SIZE - 1)) as u8;
            let g = (255 - r) as u8;
            grad.extend_from_slice(&[r, g, 200, 255]);
        }
    }
    let scene_tex = ctx.add_rgba8_texture(SIZE, SIZE, &grad, true).unwrap();

    let fields = [
        attr_loc(spec, "iPosition"), 3,
        attr_loc(spec, "iColor"), 4,
        attr_loc(spec, "iShape"), 4,
        attr_loc(spec, "iAxis"), 3,
        attr_loc(spec, "iAxis2"), 3,
        attr_loc(spec, "iFrame"), 4,
    ];
    let corner = attr_loc(spec, "corner");
    let make_batch = |ctx: &mut HeadlessRender, blur: [f32; 2]| -> u32 {
        let handle = ctx.bridge.fx_add_batch(program, white_map as i32, &NO_BLEND, corner, &fields, -1);
        ctx.bridge.fx_batch_interleaved(handle);
        ctx.bridge.fx_batch_side(handle, 2);
        ctx.bridge.fx_batch_depth(handle, 0, glow::ALWAYS);
        ctx.bridge.fx_batch_sampler(handle, "sceneColor", 12, scene_tex as i32);
        ctx.bridge
            .r
            .effects
            .batch_constants(&ctx.bridge.r.core, handle, &[("blurSettings", &blur)]);
        ctx.bridge.r.effects.prepare_batch(&ctx.bridge.r.core, handle, 1);
        handle
    };
    let batch_plain = make_batch(&mut ctx, [0.0, 0.0]);
    let batch_blur = make_batch(&mut ctx, [2.0, 0.0]);
    ctx.bridge.fx_reserve(64);

    // Keep z = -10.0, size = 4.0 so nearDepth (9.0) >= nearSpan (8.0) and near-eye fade is 1.0.
    // The quad spans NDC [-0.4, 0.4] (pixels 20..44).
    // 1. Curved opacity + 2x blur boost: raw 0.25 -> 0.6875 (175), raw 0.10 -> 0.29 (74), raw 0.60 -> 1.0 (255).
    let px_q = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -10.0], [1.0, 1.0, 1.0, 0.25], 4.0, 0.0));
    assert_near_u8(pixel_at(&px_q, 32, 32)[3], 175, 2, "refraction alpha at raw=0.25");

    let px_low = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -10.0], [1.0, 1.0, 1.0, 0.10], 4.0, 0.0));
    assert_near_u8(pixel_at(&px_low, 32, 32)[3], 74, 2, "refraction alpha at raw=0.10");

    let px_hi = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -10.0], [1.0, 1.0, 1.0, 0.60], 4.0, 0.0));
    assert_eq!(pixel_at(&px_hi, 32, 32)[3], 255, "refraction alpha clamps to 1.0 at raw=0.60");

    // RGB tint multiplies sceneColor.rgb.
    let px_tint = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -10.0], [0.5, 1.0, 0.25, 0.50], 4.0, 0.0));
    let untinted = pixel_at(&px_hi, 32, 32);
    let tinted = pixel_at(&px_tint, 32, 32);
    assert_near_u8(tinted[0], untinted[0] / 2, 3, "refraction red tint");
    assert_near_u8(tinted[1], untinted[1], 3, "refraction green tint");
    assert_near_u8(tinted[2], untinted[2] / 4, 3, "refraction blue tint");

    // 2. Radial BlurValue UV displacement pushes edge samples outward along the horizontal gradient.
    // At z=-10, size=4, 90 deg FOV, blurValue=2.0 adds refractShift = offset * (2.0 * 2.0) = offset * 4.0,
    // doubling the corner's NDC offset and shifting x=22 from r~89 to r~50 and x=42 from r~170 to r~212.
    let no_blur_left = pixel_at(&px_hi, 22, 32)[0];
    let no_blur_right = pixel_at(&px_hi, 42, 32)[0];
    let px_blur = ctx.draw_and_read(batch_blur, 0, &cpu_row([0.0, 0.0, -10.0], [1.0, 1.0, 1.0, 0.60], 4.0, 0.0));
    let blur_left = pixel_at(&px_blur, 22, 32)[0];
    let blur_right = pixel_at(&px_blur, 42, 32)[0];
    assert_near_u8(no_blur_left, 89, 2, "unblurred refraction left sample");
    assert_near_u8(no_blur_right, 170, 2, "unblurred refraction right sample");
    assert_near_u8(blur_left, 50, 3, "radial blur left shift at blurValue=2.0");
    assert_near_u8(blur_right, 212, 3, "radial blur right shift at blurValue=2.0");

    // 3. Near-eye cull (<= 1 m) and linear fade across 2 * size inside refraction_vertex.
    let px_cull = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -0.8], [1.0, 1.0, 1.0, 0.50], 2.0, 0.0));
    assert_eq!(pixel_at(&px_cull, 32, 32), [0, 0, 0, 0], "refraction sprite at 0.8 m must cull");

    let px_plane = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -1.0], [1.0, 1.0, 1.0, 0.50], 2.0, 0.0));
    assert_eq!(pixel_at(&px_plane, 32, 32), [0, 0, 0, 0], "refraction sprite at 1.0 m must cull");

    let px_half = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -3.0], [1.0, 1.0, 1.0, 0.50], 2.0, 0.0));
    assert_near_u8(pixel_at(&px_half, 32, 32)[3], 128, 2, "refraction near-fade at 3.0 m (size=2.0)");

    let px_full = ctx.draw_and_read(batch_plain, 0, &cpu_row([0.0, 0.0, -6.0], [1.0, 1.0, 1.0, 0.50], 2.0, 0.0));
    assert_eq!(pixel_at(&px_full, 32, 32)[3], 255, "refraction sprite past fade span is full opacity");
}

#[test]
fn swbf2_near_fade_renders_cull_and_linear_fade_on_cpu_and_analytic_shaders() {
    let _guard = GL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut ctx) = HeadlessRender::open_or_skip("swbf2_near_fade_renders_cull_and_linear_fade_on_cpu_and_analytic_shaders") else {
        return;
    };
    let white_map = ctx.add_rgba8_texture(1, 1, &[255, 255, 255, 255], false).unwrap();
    let tracks_tex = ctx.add_rgba32f_texture(64, 1, &vec![0.0f32; 64 * 4]).unwrap();
    let owner = ctx.bridge.fx_add_owner(tracks_tex, 1.0);
    ctx.bridge.r.effects.prepare_owner(&mut ctx.bridge.r.core, owner, 1);
    ctx.bridge.fx_reserve(64);
    ctx.bridge.r.effects.input[render::IN_SOFT] = 0.0;

    // Upload the single emitter row at data[0..12] through EffectsPass's own owner uploader.
    let emitter_row: [f32; 12] = [
        0.0, 0.0, 0.0, 1.0, // origin + tint
        0.0, 0.0, 0.0, 1.0, // identity quaternion
        0.1, 1.0, 0.0, 0.0, // clock time + effectScale = 1.0
    ];
    ctx.bridge.r.effects.data[0..12].copy_from_slice(&emitter_row);
    ctx.bridge.r.effects.input[render::IN_OWNERS] = 1.0;
    ctx.bridge.r.effects.input[render::OWNER_AT..render::OWNER_AT + 4]
        .copy_from_slice(&[owner as f64, 1.0, 1.0, 0.0]);
    ctx.bridge.r.effects.input[render::IN_DRAWS] = 1.0;
    let mut items: Vec<Item> = Vec::new();
    let view = View {
        camera: ctx.bridge.r.core.camera(),
        view_projection: [0.0; 16],
    };
    Source::emit(&mut ctx.bridge.r.effects, &mut ctx.bridge.r.core, &view, 0, &mut items);

    // Part 1: effects/particle-cpu via swbf2_particle_program
    {
        let spec = bfe_render::programs::spec("effects/particle-cpu").unwrap();
        let program = super::swbf2_particle_program(&mut ctx.bridge, spec).unwrap();
        let fields = [
            attr_loc(spec, "iPosition"), 3,
            attr_loc(spec, "iColor"), 4,
            attr_loc(spec, "iShape"), 4,
            attr_loc(spec, "iAxis"), 3,
            attr_loc(spec, "iAxis2"), 3,
            attr_loc(spec, "iFrame"), 4,
        ];
        let handle = ctx.bridge.fx_add_batch(program, white_map as i32, &NO_BLEND, attr_loc(spec, "corner"), &fields, -1);
        ctx.bridge.fx_batch_interleaved(handle);
        ctx.bridge.fx_batch_side(handle, 2);
        ctx.bridge.fx_batch_depth(handle, 0, glow::ALWAYS);
        ctx.bridge.r.effects.prepare_batch(&ctx.bridge.r.core, handle, 1);

        let px_08 = ctx.draw_and_read(handle, 16, &cpu_row([0.0, 0.0, -0.8], [1.0, 1.0, 1.0, 1.0], 2.0, 0.0));
        assert_eq!(pixel_at(&px_08, 32, 32), [0, 0, 0, 0], "cpu billboard at 0.8 m must cull");

        let px_10 = ctx.draw_and_read(handle, 16, &cpu_row([0.0, 0.0, -1.0], [1.0, 1.0, 1.0, 1.0], 2.0, 0.0));
        assert_eq!(pixel_at(&px_10, 32, 32), [0, 0, 0, 0], "cpu billboard at 1.0 m must cull");

        let px_30 = ctx.draw_and_read(handle, 16, &cpu_row([0.0, 0.0, -3.0], [1.0, 1.0, 1.0, 1.0], 2.0, 0.0));
        assert_near_u8(pixel_at(&px_30, 32, 32)[3], 128, 2, "cpu billboard near-fade at 3.0 m (size=2.0)");

        let px_50 = ctx.draw_and_read(handle, 16, &cpu_row([0.0, 0.0, -5.0], [1.0, 1.0, 1.0, 1.0], 2.0, 0.0));
        assert_eq!(pixel_at(&px_50, 32, 32)[3], 255, "cpu billboard at 5.0 m reaches full opacity");

        // Flat world-aligned quad (shape = 2.0) bypasses billboard near-eye cull.
        let px_flat = ctx.draw_and_read(handle, 16, &cpu_row([0.0, 0.0, -0.8], [1.0, 1.0, 1.0, 1.0], 2.0, 2.0));
        assert_eq!(pixel_at(&px_flat, 32, 32)[3], 255, "flat quad at 0.8 m must not be billboard-culled");
    }

    // Part 2: effects/particle-analytic via swbf2_particle_program
    {
        let spec = bfe_render::programs::spec("effects/particle-analytic").unwrap();
        let program = super::swbf2_particle_program(&mut ctx.bridge, spec).unwrap();
        let fields = [
            attr_loc(spec, "spawnPosition"), 3,
            attr_loc(spec, "spawnVelocity"), 3,
            attr_loc(spec, "spawnColor"), 4,
            attr_loc(spec, "spawnShape"), 4,
            attr_loc(spec, "spawnMeta"), 4,
        ];
        let handle = ctx.bridge.fx_add_batch(
            program,
            white_map as i32,
            &NO_BLEND,
            attr_loc(spec, "corner"),
            &fields,
            owner as i32,
        );
        ctx.bridge.fx_batch_interleaved(handle);
        ctx.bridge.fx_batch_side(handle, 2);
        ctx.bridge.fx_batch_depth(handle, 0, glow::ALWAYS);
        ctx.bridge.r.effects.prepare_batch(&ctx.bridge.r.core, handle, 1);

        let analytic_row = |z: f32, size: f32| -> [f32; 18] {
            [
                0.0, 0.0, z,         // spawnPosition
                0.0, 0.0, 0.0,       // spawnVelocity
                1.0, 1.0, 1.0, 1.0,  // spawnColor
                size, 0.0, 0.0, 0.0, // spawnShape
                0.0, 1.0, 0.0, 0.0,  // spawnMeta (birth, life, emitterSlot, trackRow)
            ]
        };

        let px_08 = ctx.draw_and_read(handle, 16, &analytic_row(-0.8, 2.0));
        assert_eq!(pixel_at(&px_08, 32, 32), [0, 0, 0, 0], "analytic billboard at 0.8 m must cull");

        let px_10 = ctx.draw_and_read(handle, 16, &analytic_row(-1.0, 2.0));
        assert_eq!(pixel_at(&px_10, 32, 32), [0, 0, 0, 0], "analytic billboard at 1.0 m must cull");

        let px_30 = ctx.draw_and_read(handle, 16, &analytic_row(-3.0, 2.0));
        assert_near_u8(pixel_at(&px_30, 32, 32)[3], 128, 2, "analytic billboard near-fade at 3.0 m (size=2.0)");

        let px_50 = ctx.draw_and_read(handle, 16, &analytic_row(-5.0, 2.0));
        assert_eq!(pixel_at(&px_50, 32, 32)[3], 255, "analytic billboard at 5.0 m reaches full opacity");
    }
}
