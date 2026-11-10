#![deny(warnings, clippy::all)]
//! Native menu and options demonstration binary (`menu-demo`) for `bfe-interface`.
//!
//! Usage:
//!   `menu-demo CONTENT_DIR [--seconds N] [--screenshot FILE] [--screenshot-dir DIR] [--screen SCREEN]`
//!
//! Supported `--screen` values:
//! - `options` (top-level `OPTIONS` menu using `SHELL_OPTIONS_LAYOUT` matching `bfe-shell`)
//! - `options-top` (top-level `OPTIONS` menu using `TOP_LAYOUT` from `src/menus/option-pages.js`)
//! - `video` / `options-video` (`VIDEO OPTIONS` page with all 31 video settings)
//! - `game` / `options-game` (`GAME OPTIONS` page with all 8 game settings + reset row)
//! - `audio` / `options-audio` (`AUDIO OPTIONS` page with all 11 audio settings)
//! - `controls` / `options-controls` (`CONTROL OPTIONS` page with mouse/flight sliders + 32/19/16 binding rows)
//! - `popup` (`confirmPopup` modal over `VIDEO OPTIONS`)

use bfe_content_native::textures::Library;
use bfe_interface::{
    confirm_popup,
    fonts::{Font, Fonts, Glyph},
    option_pages::{OptionsContext, OptionsController, SHELL_OPTIONS_LAYOUT, TOP_LAYOUT},
    settings::Settings,
    surface::{Batch, Surface},
    theme::{ClassicTheme, HitRegion},
};
use bfe_render::{
    core::{Core, MultiDraw},
    hud::{
        HudPass, GROUP, IN_CAMERA, IN_FIRST_GROUP, IN_GROUPS, IN_PROGRAM, IN_SLOTS, IN_UPLOAD,
        IN_VIEWPORT, MAX_GROUPS, QUADS,
    },
    post::UNIFORM,
};
use bfe_shell::data::{load_shell_data_from_dir, ShellData};
use glow::HasContext;
use glutin::{
    config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
    context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay},
    prelude::*,
    surface::{Surface as GlutinSurface, SwapInterval, WindowSurface},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    collections::HashMap,
    fs::File,
    io::BufWriter,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use three::{CoordinateSystem, Matrix4};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

struct CountingAllocator;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL_ALLOC: CountingAllocator = CountingAllocator;

struct OffscreenCanvas {
    gl: glow::Context,
    framebuffer: glow::Framebuffer,
    color: glow::Renderbuffer,
    depth: glow::Renderbuffer,
    size: [i32; 2],
}

impl OffscreenCanvas {
    fn new(gl: glow::Context, size: [i32; 2]) -> Result<Self, String> {
        unsafe {
            let framebuffer = gl.create_framebuffer()?;
            let color = gl.create_renderbuffer()?;
            let depth = gl.create_renderbuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, size[0], size[1]);
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::RENDERBUFFER,
                Some(color),
            );
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH24_STENCIL8, size[0], size[1]);
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_STENCIL_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth),
            );
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_renderbuffer(color);
                gl.delete_renderbuffer(depth);
                gl.delete_framebuffer(framebuffer);
                return Err("native canvas framebuffer is incomplete".into());
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            Ok(Self {
                gl,
                framebuffer,
                color,
                depth,
                size,
            })
        }
    }

    fn show(&self) {
        unsafe {
            let read = self
                .gl
                .get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
            let draw = self
                .gl
                .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING);
            let clipped = self.gl.is_enabled(glow::SCISSOR_TEST);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.framebuffer));
            self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
            self.gl.blit_framebuffer(
                0,
                0,
                self.size[0],
                self.size[1],
                0,
                0,
                self.size[0],
                self.size[1],
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
            self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
            if clipped {
                self.gl.enable(glow::SCISSOR_TEST);
            }
        }
    }
}

impl Drop for OffscreenCanvas {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_framebuffer(self.framebuffer);
            self.gl.delete_renderbuffer(self.color);
            self.gl.delete_renderbuffer(self.depth);
        }
    }
}

struct GlMultiDraw {
    gl: glow::Context,
    saved: Cell<(Option<glow::Framebuffer>, Option<glow::Framebuffer>)>,
}

impl GlMultiDraw {
    fn new(gl: glow::Context) -> Self {
        Self {
            gl,
            saved: Cell::new((None, None)),
        }
    }
}

impl MultiDraw for GlMultiDraw {
    fn multi_draw(&self, mode: u32, counts: &mut [i32], ty: u32, offsets: &mut [i32]) {
        for (&count, &offset) in counts.iter().zip(offsets.iter()) {
            unsafe {
                self.gl.draw_elements(mode, count, ty, offset);
            }
        }
    }

    fn save_framebuffers(&mut self) -> bool {
        unsafe {
            self.saved.set((
                self.gl
                    .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                self.gl
                    .get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
            ));
        }
        true
    }

    fn restore_framebuffers(&self) {
        let (draw, read) = self.saved.get();
        unsafe {
            self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
            self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
        }
    }
}

#[derive(Clone, Debug)]
struct DemoOptions {
    content_dir: PathBuf,
    seconds: Option<f64>,
    screenshot: Option<PathBuf>,
    screenshot_dir: Option<PathBuf>,
    screen: String,
    size: [i32; 2],
}

impl DemoOptions {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut content_dir: Option<PathBuf> =
            std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from);
        let mut seconds = None;
        let mut screenshot = None;
        let mut screenshot_dir = None;
        let mut screen = "options".to_string();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--seconds" => {
                    let v: f64 = args
                        .next()
                        .ok_or("missing value for --seconds")?
                        .parse()
                        .map_err(|_| "invalid --seconds value")?;
                    if !v.is_finite() || v <= 0.0 {
                        return Err("--seconds must be positive".into());
                    }
                    seconds = Some(v);
                }
                "--screenshot" => {
                    let p = args.next().ok_or("missing path for --screenshot")?;
                    screenshot = Some(PathBuf::from(p));
                }
                "--screenshot-dir" => {
                    let p = args.next().ok_or("missing path for --screenshot-dir")?;
                    screenshot_dir = Some(PathBuf::from(p));
                }
                "--screen" => {
                    screen = args.next().ok_or("missing screen name for --screen")?;
                }
                "--no-audio" => {}
                _ if arg.starts_with('-') => {
                    return Err(format!("unknown option: {arg}"));
                }
                _ => {
                    content_dir = Some(PathBuf::from(arg));
                }
            }
        }

        let content_dir = content_dir.ok_or(
            "usage: menu-demo CONTENT_DIR [--seconds N] [--screenshot FILE] [--screenshot-dir DIR] [--screen SCREEN]",
        )?;
        Ok(Self {
            content_dir,
            seconds,
            screenshot,
            screenshot_dir,
            screen,
            size: [1024, 768],
        })
    }
}

struct MenuScene {
    canvas: OffscreenCanvas,
    core: Core<glow::Context, GlMultiDraw>,
    hud: HudPass<glow::Context>,
    batch: Batch,
    fonts: Fonts,
    surface: Surface,
    controller: OptionsController,
    hits: Vec<HitRegion>,
    size: [i32; 2],
    frames: u64,
    gl_errors: u64,
    steady_allocs: u64,
    steady_frames: u64,
}

impl MenuScene {
    fn new(
        options: &DemoOptions,
        mut load_gl: impl FnMut() -> glow::Context,
    ) -> Result<Self, String> {
        let canvas = OffscreenCanvas::new(load_gl(), options.size)?;
        let mut core = Core::new(load_gl(), GlMultiDraw::new(load_gl()), 0, 0);
        core.canvas_framebuffer = Some(canvas.framebuffer);
        let mut hud = HudPass::new(&mut core);
        Self::init_hud_pass(&mut core, &mut hud, options.size)?;

        // Slot 0 in `core.textures` is a dummy texture so `Batch` handle `0`
        // ("no texture") never collides with a valid texture index; slot 1 is
        // the 1x1 solid white texture (`Surface.solid = 1`).
        let dummy_tex = Self::upload_rgba(&core.gl, 1, 1, &[255, 255, 255, 255])?;
        core.add_texture(dummy_tex);
        let solid_tex = Self::upload_rgba(&core.gl, 1, 1, &[255, 255, 255, 255])?;
        core.add_texture(solid_tex);

        let data = load_shell_data_from_dir(&options.content_dir);
        let mut lib = Library::new(&options.content_dir, false, 4096, 0)?;

        let mut uploaded_paths: HashMap<String, (u32, u32, u32)> = HashMap::new();
        let mut upload_path = |rel_url: &str| -> Option<(u32, u32, u32)> {
            let clean = rel_url.trim_start_matches('/');
            if let Some(&cached) = uploaded_paths.get(clean) {
                return Some(cached);
            }
            let img_idx = lib.load(clean).ok()?;
            let img = &lib.images[img_idx];
            let tex =
                Self::upload_rgba(&core.gl, img.width as i32, img.height as i32, &img.data).ok()?;
            let handle = core.add_texture(tex);
            let entry = (handle, img.width, img.height);
            uploaded_paths.insert(clean.to_string(), entry);
            Some(entry)
        };

        let fonts = Self::load_fonts(&data, &mut upload_path);
        let mut surface = Surface::from_viewport(options.size[0] as f64, options.size[1] as f64);

        const NAMED_TEXTURES: &[&str] = &[
            "atat_screen",
            "atte_screen",
            "coreship_screen",
            "hoth_screen",
            "generic2_screen",
            "generic3_screen",
            "gc_screen",
            "his_brief_bg",
            "iface_bgmeta_space",
            "main",
            "buttonleft",
            "buttonmid",
            "buttonright",
            "headerbuttonleft",
            "headerbuttonmid",
            "headerbuttonright",
            "border_3_pieces",
            "border_3_pieces_Selected",
            "listbox_cursor",
            "listbox_hilight",
            "slider_bg",
            "slider_fg",
            "slider_sound",
            "rep_icon",
            "cis_icon",
            "imp_icon",
            "all_icon",
        ];

        for &name in NAMED_TEXTURES {
            if let Some(url) = data.textures.get(name) {
                if let Some((handle, _, _)) = upload_path(&url) {
                    surface.set_texture(name, handle);
                }
            }
        }

        let controller = Self::build_controller(&options.screen, &data, options.size);

        Ok(Self {
            canvas,
            core,
            hud,
            batch: Batch::new(),
            fonts,
            surface,
            controller,
            hits: Vec::with_capacity(64),
            size: options.size,
            frames: 0,
            gl_errors: 0,
            steady_allocs: 0,
            steady_frames: 0,
        })
    }

    fn build_controller(screen: &str, data: &ShellData, size: [i32; 2]) -> OptionsController {
        let top_layout = if screen == "options-top" {
            TOP_LAYOUT
        } else {
            SHELL_OPTIONS_LAYOUT
        };
        let ctx = OptionsContext {
            strings: data.strings.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            in_battle: false,
            viewpoint: None,
            has_audio_panel: false,
            has_online: true,
            has_integrations: false,
            has_credits: true,
            top_layout,
            surface_height: 768.0 + ((size[1] as f64) / (size[0].max(1) as f64) - 0.75) * 768.0,
            viewport: Some([size[0] as u32, size[1] as u32]),
            gpu_renderer: String::new(),
        };
        let mut ctrl = OptionsController::new(Settings::new(), ctx);
        match screen {
            "options" | "options-top" => {}
            "video" | "options-video" => {
                ctrl.open_page("video");
            }
            "game" | "options-game" => {
                ctrl.open_page("game");
            }
            "audio" | "options-audio" => {
                ctrl.open_page("audio");
            }
            "controls" | "options-controls" => {
                ctrl.open_page("controls");
            }
            "popup" => {
                ctrl.open_page("video");
                let popup = confirm_popup("RESET VIDEO OPTIONS TO DEFAULTS?", &ctrl.context.strings);
                ctrl.stack.push(popup);
            }
            _ => {}
        }
        ctrl
    }

    fn init_hud_pass(
        core: &mut Core<glow::Context, GlMultiDraw>,
        hud: &mut HudPass<glow::Context>,
        size: [i32; 2],
    ) -> Result<(), String> {
        let key = "post-hud/hud";
        let program = core.add_owned_program(key).map_err(|e| e.to_string())?;
        let mut index = Vec::with_capacity(QUADS * 6);
        for i in 0..QUADS as u32 {
            let a = i * 4;
            index.extend([a, a + 1, a + 2, a + 2, a + 1, a + 3]);
        }
        hud.init(core, &index);
        let spec = bfe_render::programs::spec(key).map_err(|e| e.to_string())?;
        hud.prepare_program(program as usize, spec.uniforms);
        hud.resources.program = Some(program);
        hud.resources.slots = [-1; 3];
        hud.resources.cameras.clear();
        for (i, (name, _)) in spec.uniforms.iter().enumerate() {
            match *name {
                "map" => hud.resources.slots[0] = i as i32,
                "mask" => hud.resources.slots[1] = i as i32,
                "masked" => hud.resources.slots[2] = i as i32,
                "projectionMatrix" => hud.resources.cameras.push((i, 0)),
                "modelViewMatrix" => hud.resources.cameras.push((i, 1)),
                _ => return Err("unknown HUD uniform".into()),
            }
        }

        let mut projection = Matrix4::default();
        let tan = (std::f64::consts::PI / 6.0).tan();
        projection.make_perspective(
            -tan,
            tan,
            0.75 * tan,
            -0.75 * tan,
            1.0,
            10000.0,
            CoordinateSystem::WebGL,
        );
        let view = Matrix4::default();
        hud.input[IN_PROGRAM] = program as f64;
        for i in 0..2 {
            let at = IN_CAMERA + i * UNIFORM;
            hud.input[at] = -1.0;
            if let Some(&(slot, kind)) = hud.resources.cameras.get(i) {
                hud.input[at] = slot as f64;
                hud.input[at + 1] = 16.0;
                hud.input[at + 2..at + 18].copy_from_slice(if kind == 0 {
                    &projection.elements
                } else {
                    &view.elements
                });
            }
        }
        for i in 0..3 {
            hud.input[IN_SLOTS + i] = hud.resources.slots[i] as f64;
        }
        hud.input[IN_VIEWPORT] = size[0] as f64;
        hud.input[IN_VIEWPORT + 1] = size[1] as f64;
        Ok(())
    }

    fn load_fonts(
        data: &ShellData,
        upload_path: &mut impl FnMut(&str) -> Option<(u32, u32, u32)>,
    ) -> Fonts {
        let mut page_handles: HashMap<String, u32> = HashMap::new();
        for (page_name, page_url) in &data.pages {
            if let Some((handle, _, _)) = upload_path(page_url) {
                page_handles.insert(page_name.clone(), handle);
            }
        }

        let build_font = |name: &str, default_h: f64| -> Font {
            let Some(sf) = data.fonts.get(name) else {
                return Font {
                    height: default_h,
                    glyphs: HashMap::new(),
                };
            };
            let mut glyphs = HashMap::with_capacity(sf.glyphs.len());
            for row in &sf.glyphs {
                if row.len() >= 11 {
                    let code = row[0] as u16;
                    let page_idx = row[1] as usize;
                    let page_tex = sf
                        .pages
                        .get(page_idx)
                        .and_then(|p| page_handles.get(p))
                        .copied()
                        .unwrap_or(1);
                    glyphs.insert(
                        code,
                        Glyph {
                            page: page_tex,
                            advance: row[2],
                            bounds: [row[3], row[5], row[4], row[6]],
                            uv: [row[7], row[8], row[9], row[10]],
                        },
                    );
                }
            }
            Font {
                height: sf.line_height.max(1.0),
                glyphs,
            }
        };

        Fonts {
            large: build_font("gamefont_large", 28.0),
            medium: build_font("gamefont_medium", 20.0),
            small: build_font("gamefont_small", 16.0),
            tiny: build_font("gamefont_tiny", 14.0),
        }
    }

    fn upload_rgba(
        gl: &glow::Context,
        width: i32,
        height: i32,
        rgba: &[u8],
    ) -> Result<glow::Texture, String> {
        unsafe {
            let tex = gl.create_texture().map_err(|e| e.to_string())?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width,
                height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(rgba)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
            Ok(tex)
        }
    }

    fn step_and_draw(&mut self, dt: f64, collect_hits: bool) {
        let warm = self.frames >= 4;
        let alloc_before = ALLOC_COUNT.load(Ordering::Relaxed);

        self.controller.stack.update(dt);
        let backdrop = self.surface.options_backdrop();
        let hits_opt = if collect_hits {
            Some(&mut self.hits)
        } else {
            None
        };
        ClassicTheme::paint_stack(
            &mut self.batch,
            &self.fonts,
            &self.surface,
            &self.controller.stack,
            backdrop,
            hits_opt,
        );

        {
            let b = &self.batch;
            let hud = &mut self.hud;
            let (start, end) = if self.frames == 0 {
                (0, b.count * 4)
            } else {
                (b.start.min(QUADS * 4), b.end.min(QUADS * 4))
            };
            if end > start {
                hud.position[start * 3..end * 3]
                    .copy_from_slice(&b.position[start * 3..end * 3]);
                hud.uv[start * 2..end * 2].copy_from_slice(&b.uv[start * 2..end * 2]);
                hud.mask_uv[start * 2..end * 2]
                    .copy_from_slice(&b.mask_uv[start * 2..end * 2]);
                hud.tint[start * 4..end * 4].copy_from_slice(&b.tint[start * 4..end * 4]);
            }
            hud.input[IN_UPLOAD] = start as f64;
            hud.input[IN_UPLOAD + 1] = end as f64;
            let groups = b.draws.len().min(MAX_GROUPS);
            hud.input[IN_GROUPS] = groups as f64;
            for (i, draw) in b.draws.iter().take(groups).enumerate() {
                let o = IN_FIRST_GROUP + i * GROUP;
                hud.input[o] = draw.first as f64;
                hud.input[o + 1] = draw.count as f64;
                hud.input[o + 2] = draw.texture as f64;
                hud.input[o + 3] = (if draw.mask == 0 {
                    draw.texture
                } else {
                    draw.mask
                }) as f64;
                hud.input[o + 4] = u8::from(draw.mask != 0) as f64;
            }
        }

        unsafe {
            let gl = &self.core.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.disable(glow::SCISSOR_TEST);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
        self.hud.draw(&mut self.core);

        let alloc_after = ALLOC_COUNT.load(Ordering::Relaxed);
        if warm && !collect_hits {
            self.steady_allocs += alloc_after.saturating_sub(alloc_before);
            self.steady_frames += 1;
        }

        unsafe {
            let err = self.core.gl.get_error();
            if err != glow::NO_ERROR {
                self.gl_errors += 1;
            }
        }
        self.frames += 1;
    }

    fn save_screenshot(&self, path: &Path) -> Result<(), String> {
        let w = self.size[0] as usize;
        let h = self.size[1] as usize;
        let mut pixels = vec![0u8; w * h * 4];
        unsafe {
            let gl = &self.core.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                self.size[0],
                self.size[1],
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        let mut flipped = vec![0u8; w * h * 4];
        let row_bytes = w * 4;
        for y in 0..h {
            let src = (h - 1 - y) * row_bytes;
            let dst = y * row_bytes;
            flipped[dst..dst + row_bytes].copy_from_slice(&pixels[src..src + row_bytes]);
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let file = File::create(path).map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&flipped)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn summary_json(&self) -> serde_json::Value {
        let allocs_per_frame = if self.steady_frames > 0 {
            (self.steady_allocs as f64) / (self.steady_frames as f64)
        } else {
            0.0
        };
        serde_json::json!({
            "frames": self.frames,
            "steadyFrames": self.steady_frames,
            "steadyAllocs": self.steady_allocs,
            "allocsPerFrame": allocs_per_frame,
            "glErrors": self.gl_errors,
            "quads": self.batch.count,
            "draws": self.batch.draws.len(),
            "screen": self.controller.stack.top().map(|s| s.id.as_str()).unwrap_or(""),
        })
    }
}

struct WindowRunning {
    scene: MenuScene,
    surface: GlutinSurface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
    start: Instant,
    last: Instant,
    next: Instant,
    refresh: Duration,
    cursor_pos: [f64; 2],
    options: DemoOptions,
    screenshot_saved: bool,
}

impl WindowRunning {
    fn new(el: &ActiveEventLoop, options: &DemoOptions) -> Result<Self, String> {
        let attrs = Window::default_attributes()
            .with_title("Battlefront Eternal - Menu Demo")
            .with_visible(false)
            .with_resizable(true)
            .with_inner_size(PhysicalSize::new(
                options.size[0] as u32,
                options.size[1] as u32,
            ));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(
                el,
                ConfigTemplateBuilder::new()
                    .with_alpha_size(8)
                    .with_depth_size(24)
                    .with_stencil_size(8),
                |c| {
                    c.min_by_key(|c| c.num_samples())
                        .expect("no GL configuration")
                },
            )
            .map_err(|e| e.to_string())?;
        let window = window.ok_or("window creation failed")?;
        let display = config.display();
        let raw = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
            .build(Some(raw));
        let context = unsafe { display.create_context(&config, &context_attrs) }
            .map_err(|e| e.to_string())?;
        let surface = unsafe {
            display.create_window_surface(
                &config,
                &window
                    .build_surface_attributes(Default::default())
                    .map_err(|e| e.to_string())?,
            )
        }
        .map_err(|e| e.to_string())?;
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        let _ =
            surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let load = || unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                display.get_proc_address(name) as *const _
            })
        };
        let scene = MenuScene::new(options, load)?;
        window.set_visible(true);
        let now = Instant::now();
        Ok(Self {
            scene,
            surface,
            context,
            window,
            start: now,
            last: now,
            next: now,
            refresh: Duration::from_secs_f64(1.0 / 60.0),
            cursor_pos: [0.0, 0.0],
            options: options.clone(),
            screenshot_saved: false,
        })
    }

    fn redraw(&mut self) -> Result<bool, String> {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().clamp(0.0, 0.1);
        self.last = now;
        self.scene.step_and_draw(dt, false);
        self.scene.canvas.show();
        self.window.pre_present_notify();
        self.surface
            .swap_buffers(&self.context)
            .map_err(|e| e.to_string())?;
        self.next = (self.next + self.refresh).max(Instant::now());

        if !self.screenshot_saved && self.scene.frames >= 3 {
            if let Some(path) = &self.options.screenshot {
                self.scene.save_screenshot(path)?;
                self.screenshot_saved = true;
                if self.options.seconds.is_none() {
                    println!("{}", self.scene.summary_json());
                    return Ok(true);
                }
            }
        }

        if let Some(secs) = self.options.seconds {
            if now.duration_since(self.start).as_secs_f64() >= secs {
                if !self.screenshot_saved {
                    if let Some(path) = &self.options.screenshot {
                        self.scene.save_screenshot(path)?;
                        self.screenshot_saved = true;
                    }
                }
                println!("{}", self.scene.summary_json());
                return Ok(true);
            }
        }

        Ok(false)
    }
}

struct DemoApp {
    options: DemoOptions,
    running: Option<WindowRunning>,
    error: Option<String>,
}

impl DemoApp {
    fn stop(&mut self, el: &ActiveEventLoop, error: Option<String>) {
        self.error = error;
        self.running.take();
        el.exit();
    }
}

impl ApplicationHandler for DemoApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match WindowRunning::new(el, &self.options) {
            Ok(r) => {
                r.window.request_redraw();
                self.running = Some(r);
            }
            Err(e) => self.stop(el, Some(e)),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                self.stop(el, None);
            }
            WindowEvent::RedrawRequested => match r.redraw() {
                Ok(true) => self.stop(el, None),
                Ok(false) => {}
                Err(e) => self.stop(el, Some(e)),
            },
            WindowEvent::CursorMoved { position, .. } => {
                r.cursor_pos = [position.x, position.y];
                r.scene.step_and_draw(0.0, true);
                let vw = r.scene.size[0] as f64;
                let vh = r.scene.size[1] as f64;
                let ev = r.scene.controller.input.on_pointer_move(
                    &mut r.scene.controller.stack,
                    &r.scene.hits,
                    position.x,
                    position.y,
                    vw,
                    vh,
                );
                r.scene.controller.apply_event(ev);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Pressed {
                    r.scene.step_and_draw(0.0, true);
                    let vw = r.scene.size[0] as f64;
                    let vh = r.scene.size[1] as f64;
                    let ev = r.scene.controller.input.on_pointer_down(
                        &mut r.scene.controller.stack,
                        &r.scene.hits,
                        r.cursor_pos[0],
                        r.cursor_pos[1],
                        vw,
                        vh,
                    );
                    r.scene.controller.apply_event(ev);
                } else {
                    r.scene.controller.input.on_pointer_up();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        if y > 0.0 {
                            -1
                        } else {
                            1
                        }
                    }
                    MouseScrollDelta::PixelDelta(p) => {
                        if p.y > 0.0 {
                            -1
                        } else {
                            1
                        }
                    }
                };
                let ev = r
                    .scene
                    .controller
                    .input
                    .on_wheel(&mut r.scene.controller.stack, dy);
                r.scene.controller.apply_event(ev);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let code: Option<&str> = match event.physical_key {
                    PhysicalKey::Code(KeyCode::ArrowUp) => Some("ArrowUp"),
                    PhysicalKey::Code(KeyCode::ArrowDown) => Some("ArrowDown"),
                    PhysicalKey::Code(KeyCode::ArrowLeft) => Some("ArrowLeft"),
                    PhysicalKey::Code(KeyCode::ArrowRight) => Some("ArrowRight"),
                    PhysicalKey::Code(KeyCode::PageUp) => Some("PageUp"),
                    PhysicalKey::Code(KeyCode::PageDown) => Some("PageDown"),
                    PhysicalKey::Code(KeyCode::Tab) => Some("Tab"),
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => Some("Enter"),
                    PhysicalKey::Code(KeyCode::Space) => Some("Space"),
                    PhysicalKey::Code(KeyCode::Escape) => Some("Escape"),
                    PhysicalKey::Code(KeyCode::Backspace) => Some("Backspace"),
                    PhysicalKey::Code(KeyCode::Delete) => Some("Delete"),
                    _ => match &event.logical_key {
                        Key::Named(NamedKey::Space) => Some("Space"),
                        _ => None,
                    },
                };
                if let Some(c) = code {
                    r.scene.controller.on_key_down(c, false);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(r) = &self.running {
            if Instant::now() >= r.next {
                r.window.request_redraw();
            } else {
                el.set_control_flow(ControlFlow::WaitUntil(r.next));
            }
        }
    }
}

fn run_headless_egl(options: &DemoOptions) -> Result<(), String> {
    let device = glutin::api::egl::device::Device::query_devices()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no EGL device")?;
    let display = unsafe { glutin::api::egl::display::Display::with_device(&device, None) }
        .map_err(|e| e.to_string())?;
    let template = ConfigTemplateBuilder::new()
        .with_surface_type(ConfigSurfaceTypes::empty())
        .build();
    let config = unsafe { display.find_configs(template) }
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no EGL config")?;
    let attrs = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
        .build(None);
    let context = unsafe { display.create_context(&config, &attrs) }.map_err(|e| e.to_string())?;
    let _context = context
        .make_current_surfaceless()
        .map_err(|e| e.to_string())?;
    let load = || unsafe {
        glow::Context::from_loader_function(|name| {
            display
                .get_proc_address(&std::ffi::CString::new(name).unwrap())
                .cast()
        })
    };

    let mut scene = MenuScene::new(options, load)?;

    if let Some(dir) = &options.screenshot_dir {
        let data = load_shell_data_from_dir(&options.content_dir);
        for (scr, filename) in [
            ("options", "options.png"),
            ("options-top", "options-top.png"),
            ("video", "video-options.png"),
            ("game", "game-options.png"),
            ("audio", "audio-options.png"),
            ("controls", "control-options.png"),
        ] {
            scene.controller = MenuScene::build_controller(scr, &data, options.size);
            scene.frames = 0;
            for _ in 0..5 {
                scene.step_and_draw(1.0 / 60.0, false);
            }
            scene.save_screenshot(&dir.join(filename))?;
        }
    }

    let start = Instant::now();
    let duration = options.seconds.unwrap_or(0.1);
    let dt = 1.0 / 60.0;

    loop {
        scene.step_and_draw(dt, false);
        if scene.frames >= 5 && start.elapsed().as_secs_f64() >= duration {
            break;
        }
        if options.seconds.is_some() {
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    if let Some(path) = &options.screenshot {
        scene.save_screenshot(path)?;
    }
    println!("{}", scene.summary_json());
    Ok(())
}

fn run() -> Result<(), String> {
    let options = DemoOptions::parse()?;
    if std::env::var_os("DISPLAY").is_some()
        && std::env::var_os("BFE_HEADLESS_EGL").is_none()
        && options.screenshot_dir.is_none()
    {
        let mut app = DemoApp {
            options: options.clone(),
            running: None,
            error: None,
        };
        if let Ok(el) = EventLoop::new() {
            el.run_app(&mut app).map_err(|e| e.to_string())?;
            if let Some(err) = app.error {
                return run_headless_egl(&options).or(Err(err));
            }
            return Ok(());
        }
    }
    run_headless_egl(&options)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
