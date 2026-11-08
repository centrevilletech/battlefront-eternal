#![deny(warnings)]
//! Native front-end shell demonstration binary (`shell-demo`).
//!
//! Usage:
//!   `shell-demo CONTENT_DIR [--seconds N] [--screenshot FILE] [--screen SCREEN] [--no-audio]`
//!
//! Opens a native window (or falls back to an offscreen EGL device context when
//! no X11 display is available) starting on the click gate (`--screen gate`, the
//! default) or directly on a requested screen (`main`, `sp`, `ia`, `ia-era`,
//! `options`, `options-audio`, `profile`, `mp`, `mp-lan`, `mp-online`,
//! `loading`, `loading-zoom`). Keyboard and mouse navigate every screen,
//! Instant Action edits a playlist, `LAUNCH` reports the `BattleRequest` it
//! would launch on stdout, and shell music + interface sounds render through the
//! native `audio::frontend::Frontend` mixer (and `cpal` audio device when
//! available).

use bfe_content_native::textures::Library;
use bfe_online::browser::{
    BattleSummary, RotationItem, ServerBrowser, ServerBrowserService, ServerEntry, Source,
};
use bfe_render::{
    core::{Core, MultiDraw},
    hud::{
        HudPass, GROUP, IN_CAMERA, IN_FIRST_GROUP, IN_GROUPS, IN_PROGRAM, IN_SLOTS, IN_UPLOAD,
        IN_VIEWPORT, MAX_GROUPS, QUADS,
    },
    interface::{Batch, Font, Glyph},
    post::UNIFORM,
};
use bfe_shell::{
    audio::{ShellAudioMixer, TARGET_RATE},
    data::{load_shell_data_from_dir, ShellData},
    launch::{format_rotation, PlaylistEntry},
    loading::{planet_name, LoadingTimeline, LookId},
    render::{draw_loading, draw_shell, ShellFonts, ShellTextures, StageTexture},
    screens::{IaFocus, MultiplayerKind, OptionsPageId, ScreenId, ShellAction, ShellApp},
    version::{AboutInfo, DEFAULT_SIMULATION},
};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SizedSample,
};
use glow::HasContext;
use glutin::{
    config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
    context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay},
    prelude::*,
    surface::{Surface, SwapInterval, WindowSurface},
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
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use three::{CoordinateSystem, Matrix4};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, WindowEvent},
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
    screen: String,
    audio: bool,
    size: [i32; 2],
}

impl DemoOptions {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut content_dir: Option<PathBuf> =
            std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from);
        let mut seconds = None;
        let mut screenshot = None;
        let mut screen = "gate".to_string();
        let mut audio = true;

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
                "--screen" => {
                    screen = args.next().ok_or("missing screen name for --screen")?;
                }
                "--no-audio" => {
                    audio = false;
                }
                _ if arg.starts_with('-') => {
                    return Err(format!("unknown option: {arg}"));
                }
                _ => {
                    content_dir = Some(PathBuf::from(arg));
                }
            }
        }

        let content_dir = content_dir.ok_or(
            "usage: shell-demo CONTENT_DIR [--seconds N] [--screenshot FILE] [--screen SCREEN] [--no-audio]",
        )?;
        Ok(Self {
            content_dir,
            seconds,
            screenshot,
            screen,
            audio,
            size: [1024, 768],
        })
    }
}

fn build_demo_browser() -> ServerBrowser {
    let mut browser = ServerBrowser::default();
    let token = browser.begin_refresh(false).unwrap_or(1);
    browser.finish_refresh_ok(
        token,
        vec![
            ServerEntry {
                id: "theed-1".to_string(),
                name: "THEED PALACE GARRISON".to_string(),
                address: Some("203.0.113.10".to_string()),
                port: Some(18451),
                location: "Virginia, US".to_string(),
                country: "US".to_string(),
                simulation: Some(DEFAULT_SIMULATION.to_string()),
                players: 14,
                max_players: 32,
                bots: Some(true),
                latency_ms: Some(34),
                battle: Some(BattleSummary {
                    map: "nab2".to_string(),
                    side: "rep".to_string(),
                    era: "cw".to_string(),
                    mode: "conquest".to_string(),
                    ..BattleSummary::default()
                }),
                rotation: vec![RotationItem::Slug("swbf1/nab2:rep:conquest".to_string())],
                ..ServerEntry::default()
            },
            ServerEntry {
                id: "hoth-2".to_string(),
                name: "ECHO BASE OUTPOST".to_string(),
                address: Some("198.51.100.24".to_string()),
                port: Some(18451),
                location: "Frankfurt, DE".to_string(),
                country: "DE".to_string(),
                simulation: Some(DEFAULT_SIMULATION.to_string()),
                players: 22,
                max_players: 32,
                bots: Some(true),
                latency_ms: Some(78),
                battle: Some(BattleSummary {
                    map: "hot1".to_string(),
                    side: "all".to_string(),
                    era: "gcw".to_string(),
                    mode: "conquest".to_string(),
                    ..BattleSummary::default()
                }),
                rotation: vec![RotationItem::Slug("swbf1/hot1:all:conquest".to_string())],
                ..ServerEntry::default()
            },
        ],
        1000,
    );
    browser
}

struct ShellAudioStream {
    _stream: cpal::Stream,
}

impl ShellAudioStream {
    fn try_open(audio: Arc<Mutex<ShellAudioMixer>>) -> Option<Self> {
        let device = cpal::default_host().default_output_device()?;
        let rate = TARGET_RATE;
        let supported = device
            .supported_output_configs()
            .ok()?
            .find(|c| {
                c.channels() == 2
                    && c.min_sample_rate().0 <= rate
                    && c.max_sample_rate().0 >= rate
                    && matches!(
                        c.sample_format(),
                        cpal::SampleFormat::F32
                            | cpal::SampleFormat::I16
                            | cpal::SampleFormat::U16
                    )
            })?
            .with_sample_rate(cpal::SampleRate(rate));
        let format = supported.sample_format();
        let config = supported.config();
        let stream = match format {
            cpal::SampleFormat::F32 => Self::build::<f32>(&device, &config, audio)?,
            cpal::SampleFormat::I16 => Self::build::<i16>(&device, &config, audio)?,
            cpal::SampleFormat::U16 => Self::build::<u16>(&device, &config, audio)?,
            _ => return None,
        };
        stream.play().ok()?;
        Some(Self { _stream: stream })
    }

    fn build<T: SizedSample + FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        audio: Arc<Mutex<ShellAudioMixer>>,
    ) -> Option<cpal::Stream> {
        let mut block = [0.0f32; ::audio::FRAMES * 2];
        let mut pos = ::audio::FRAMES * 2;
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    for sample in data {
                        if pos >= block.len() {
                            if let Ok(mut guard) = audio.lock() {
                                guard.render_interleaved(&mut block);
                            } else {
                                block.fill(0.0);
                            }
                            pos = 0;
                        }
                        *sample = T::from_sample(block[pos]);
                        pos += 1;
                    }
                },
                move |_| {},
                None,
            )
            .ok()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DemoMode {
    Shell,
    LoadingTrack,
    LoadingZoom,
}

struct ShellScene {
    canvas: OffscreenCanvas,
    core: Core<glow::Context, GlMultiDraw>,
    hud: HudPass<glow::Context>,
    batch: Batch,
    fonts: ShellFonts,
    textures: ShellTextures,
    app: ShellApp,
    browser: ServerBrowser,
    audio: Arc<Mutex<ShellAudioMixer>>,
    _audio_stream: Option<ShellAudioStream>,
    audio_scratch: [f32; ::audio::FRAMES * 2],
    mode: DemoMode,
    loading_timeline: LoadingTimeline,
    loading_planet: String,
    loading_text: String,
    loading_time: f64,
    size: [i32; 2],
    frames: u64,
    gl_errors: u64,
    steady_allocs: u64,
    steady_frames: u64,
}

impl ShellScene {
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
        // the 1x1 solid white texture (`ShellTextures.solid = 1`).
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

        // Upload bitmap fonts (`gamefont_large`, `gamefont_medium`, `gamefont_small`, `gamefont_tiny`).
        let fonts = Self::load_fonts(&data, &mut upload_path);

        // Upload UI pieces, poster stills, map previews, and loading screen chrome.
        let mut textures = ShellTextures {
            solid: 1,
            by_name: HashMap::new(),
            loading_stages: Vec::new(),
        };

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
            "load_rotating_disk",
            "load_led_0",
            "load_led_1",
            "load_led_2",
            "load_edge_horz",
            "load_edge_vert",
            "load_edge_corner",
            "map_unk",
        ];

        for &name in NAMED_TEXTURES {
            if let Some(url) = data.textures.get(name) {
                if let Some((handle, _, _)) = upload_path(&url) {
                    textures.by_name.insert(name.to_string(), handle);
                }
            }
        }
        for map in &data.maps {
            let preview_name = format!("map_{}", map.id);
            if let Some(url) = data.textures.get(&preview_name) {
                if let Some((handle, _, _)) = upload_path(&url) {
                    textures.by_name.insert(preview_name, handle);
                }
            }
        }
        if let Some(disk_url) = &data.load.disk {
            if let Some((handle, _, _)) = upload_path(disk_url) {
                textures
                    .by_name
                    .insert("load_rotating_disk".to_string(), handle);
            }
        }
        if let Some(horz_url) = &data.load.edges.horz {
            if let Some((handle, _, _)) = upload_path(horz_url) {
                textures
                    .by_name
                    .insert("load_edge_horz".to_string(), handle);
            }
        }
        if let Some(vert_url) = &data.load.edges.vert {
            if let Some((handle, _, _)) = upload_path(vert_url) {
                textures
                    .by_name
                    .insert("load_edge_vert".to_string(), handle);
            }
        }
        if let Some(led_url) = data.load.leds.first() {
            if let Some((handle, _, _)) = upload_path(led_url) {
                textures.by_name.insert("load_led_0".to_string(), handle);
            }
        }

        // Load stage images for `nab2` (or first map) for the loading screen view.
        let load_map = data
            .maps
            .iter()
            .find(|m| m.id == "nab2")
            .or_else(|| data.maps.first());
        let mut stage_targets = Vec::new();
        if let Some(map) = load_map {
            if let Some(load_meta) = &map.load {
                let total = load_meta.stages.len();
                for (i, stage) in load_meta.stages.iter().enumerate() {
                    if let Some((handle, w, h)) = upload_path(&stage.url) {
                        let wf = w.max(1) as f64;
                        let hf = h.max(1) as f64;
                        let target = if i + 1 < total { stage.target } else { None };
                        textures.loading_stages.push(StageTexture {
                            texture: handle,
                            width: wf,
                            height: hf,
                            target,
                        });
                        stage_targets.push(target.map(|t| {
                            [
                                t[0] / wf,
                                t[1] / hf,
                                (t[0] + t[2]) / wf,
                                (t[1] + t[3]) / hf,
                            ]
                        }));
                    }
                }
            }
        }
        if stage_targets.is_empty() {
            stage_targets = vec![
                Some([0.2, 0.3, 0.25, 0.35]),
                Some([0.3, 0.4, 0.35, 0.45]),
                Some([0.25, 0.35, 0.3, 0.4]),
                None,
            ];
        }
        let loading_timeline = LoadingTimeline::new(
            data.load.track,
            data.load.zoom,
            data.load.hold,
            stage_targets,
        );
        let loading_planet = load_map
            .map(|m| planet_name(&m.id, &data.strings, m.load.as_ref()))
            .unwrap_or_else(|| "NABOO".to_string());
        let loading_text = data
            .strings
            .get(&data.load.text)
            .cloned()
            .unwrap_or_else(|| "LOADING".to_string());

        let about = AboutInfo::from_content_dir(&options.content_dir, Some("0.1.0-preview.4"));
        let mut app = ShellApp::new(data.clone(), Some(&about));
        let mut browser = build_demo_browser();

        // Populate a realistic default rotation in Instant Action so the right
        // list and LAUNCH button are active immediately when inspected.
        app.ia_entries = vec![
            PlaylistEntry::new("nab2", Some("cw".to_string())),
            PlaylistEntry::new("hot1", Some("gcw".to_string())),
            PlaylistEntry::new("kas2", Some("cw".to_string())),
        ];

        let mut shell_audio = ShellAudioMixer::new(&data, Some(&options.content_dir));

        let (mode, loading_time) = Self::apply_initial_screen(
            &options.screen,
            &mut app,
            &mut browser,
            &mut shell_audio,
            &loading_timeline,
        );
        let initial_sounds = app.drain_sounds();
        let initial_media = app.drain_media_events();
        for snd in initial_sounds {
            shell_audio.play(snd);
        }
        shell_audio.apply_media_events(&initial_media, &data);

        let audio = Arc::new(Mutex::new(shell_audio));
        let audio_stream = if options.audio {
            ShellAudioStream::try_open(audio.clone())
        } else {
            None
        };

        Ok(Self {
            canvas,
            core,
            hud,
            batch: Batch::new(),
            fonts,
            textures,
            app,
            browser,
            audio,
            _audio_stream: audio_stream,
            audio_scratch: [0.0; ::audio::FRAMES * 2],
            mode,
            loading_timeline,
            loading_planet,
            loading_text,
            loading_time,
            size: options.size,
            frames: 0,
            gl_errors: 0,
            steady_allocs: 0,
            steady_frames: 0,
        })
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

    fn apply_initial_screen(
        screen: &str,
        app: &mut ShellApp,
        browser: &mut ServerBrowser,
        audio: &mut ShellAudioMixer,
        timeline: &LoadingTimeline,
    ) -> (DemoMode, f64) {
        match screen {
            "gate" => (DemoMode::Shell, 0.0),
            "main" => {
                app.unlock_gate();
                (DemoMode::Shell, 0.0)
            }
            "sp" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Singleplayer);
                (DemoMode::Shell, 0.0)
            }
            "ia" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Singleplayer);
                app.push_screen(ScreenId::InstantAction);
                app.ia_focus = IaFocus::Available;
                (DemoMode::Shell, 0.0)
            }
            "ia-era" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Singleplayer);
                app.push_screen(ScreenId::InstantAction);
                app.ia_add();
                (DemoMode::Shell, 0.0)
            }
            "options" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Options);
                (DemoMode::Shell, 0.0)
            }
            "options-audio" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Options);
                app.push_screen(ScreenId::OptionsPage(OptionsPageId::Audio));
                (DemoMode::Shell, 0.0)
            }
            "profile" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Profile);
                (DemoMode::Shell, 0.0)
            }
            "mp" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Multiplayer);
                (DemoMode::Shell, 0.0)
            }
            "mp-lan" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Multiplayer);
                browser.set_source(Source::Lan);
                let tok = browser.begin_refresh(false).unwrap_or(1);
                browser.finish_refresh_ok(
                    tok,
                    vec![ServerEntry {
                        id: "lan-1".to_string(),
                        name: "LOCAL LAN SQUADRON".to_string(),
                        address: Some("192.0.2.42".to_string()),
                        port: Some(18451),
                        players: 4,
                        max_players: 16,
                        latency_ms: Some(2),
                        battle: Some(BattleSummary {
                            map: "hot1".to_string(),
                            side: "all".to_string(),
                            era: "gcw".to_string(),
                            mode: "conquest".to_string(),
                            ..BattleSummary::default()
                        }),
                        rotation: vec![RotationItem::Slug("swbf1/hot1:all:conquest".to_string())],
                        ..ServerEntry::default()
                    }],
                    1000,
                );
                app.sync_from_browser(browser);
                app.push_screen(ScreenId::MultiplayerList(MultiplayerKind::Lan));
                (DemoMode::Shell, 0.0)
            }
            "mp-online" => {
                app.unlock_gate();
                app.push_screen(ScreenId::Multiplayer);
                app.sync_from_browser(browser);
                app.push_screen(ScreenId::MultiplayerList(MultiplayerKind::Online));
                (DemoMode::Shell, 0.0)
            }
            "loading" | "loading-track" => {
                audio.schedule_loading_plan(&timeline.sounds(), 0.0);
                (DemoMode::LoadingTrack, 2.2)
            }
            "loading-zoom" => {
                audio.schedule_loading_plan(&timeline.sounds(), 0.0);
                (DemoMode::LoadingZoom, 3.85)
            }
            _ => (DemoMode::Shell, 0.0),
        }
    }

    fn load_fonts(
        data: &ShellData,
        upload_path: &mut impl FnMut(&str) -> Option<(u32, u32, u32)>,
    ) -> ShellFonts {
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

        ShellFonts {
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

    fn step_and_draw(&mut self, dt: f64) {
        let warm = self.frames >= 4;
        let alloc_before = ALLOC_COUNT.load(Ordering::Relaxed);

        let w = self.size[0] as f64;
        let h = self.size[1] as f64;
        match self.mode {
            DemoMode::Shell => {
                // Advance pulse clocks without triggering heap allocations on the
                // hot frame path.
                if dt.is_finite() && dt > 0.0 {
                    self.app.gate_elapsed += dt;
                    self.app.pulse_elapsed += dt;
                }
                draw_shell(&mut self.batch, &self.fonts, &self.textures, &self.app, w, h);
            }
            DemoMode::LoadingTrack | DemoMode::LoadingZoom => {
                let state = self.loading_timeline.at(self.loading_time);
                draw_loading(
                    &mut self.batch,
                    &self.fonts,
                    &self.textures,
                    &state,
                    self.loading_time,
                    &self.loading_planet,
                    &self.loading_text,
                    LookId::Bf1,
                    w,
                    h,
                );
            }
        }

        // Copy changed vertices from `self.batch` into `self.hud` and draw.
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

        // When no cpal stream is active, advance the mixer on the frame thread
        // so `audio_frames` and music/loading cues still run zero-allocation.
        if self._audio_stream.is_none() {
            if let Ok(mut guard) = self.audio.lock() {
                for _ in 0..6 {
                    guard.render_interleaved(&mut self.audio_scratch);
                }
            }
        }

        let alloc_after = ALLOC_COUNT.load(Ordering::Relaxed);
        if warm {
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

    fn flush_app_events(&mut self) -> bool {
        let sounds = self.app.drain_sounds();
        let media_events = self.app.drain_media_events();
        if !sounds.is_empty() || !media_events.is_empty() {
            if let Ok(mut guard) = self.audio.lock() {
                for snd in sounds {
                    guard.play(snd);
                }
                guard.apply_media_events(&media_events, &self.app.data);
            }
        }
        if let Some(action) = self.app.take_action() {
            match action {
                ShellAction::LaunchBattle(req) => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "event": "launch_battle",
                            "family": req.family.as_str(),
                            "map": req.map,
                            "era": req.era,
                            "mode": req.mode,
                            "rotation": req.playlist.as_ref().map(|p| format_rotation(&p.entries)),
                        })
                    );
                }
                ShellAction::LaunchMultiplayer(mp) => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "event": "launch_multiplayer",
                            "launch": mp,
                        })
                    );
                }
                ShellAction::ExitToWindows => {
                    return true;
                }
            }
        }
        false
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
        // Flip rows vertically (OpenGL framebuffer row 0 is bottom).
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
        let audio_frames = self
            .audio
            .lock()
            .map(|g| (g.current_time() * (TARGET_RATE as f64)).round() as u64)
            .unwrap_or(0);
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
            "audioFrames": audio_frames,
            "versionLine": self.app.version_text,
        })
    }
}

struct WindowRunning {
    scene: ShellScene,
    surface: Surface<WindowSurface>,
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
            .with_title("Battlefront Eternal - Shell Demo")
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
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let load = || unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                display.get_proc_address(name) as *const _
            })
        };
        let scene = ShellScene::new(options, load)?;
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
        self.scene.step_and_draw(dt);
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
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if r.scene.app.current_screen() == ScreenId::Gate {
                    r.scene.app.unlock_gate();
                    if r.scene.flush_app_events() {
                        self.stop(el, None);
                    }
                } else {
                    r.scene.app.accept(&mut r.scene.browser);
                    if r.scene.flush_app_events() {
                        self.stop(el, None);
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let key_str: Option<String> = match event.physical_key {
                    PhysicalKey::Code(KeyCode::ArrowUp) => Some("ArrowUp".to_string()),
                    PhysicalKey::Code(KeyCode::ArrowDown) => Some("ArrowDown".to_string()),
                    PhysicalKey::Code(KeyCode::ArrowLeft) => Some("ArrowLeft".to_string()),
                    PhysicalKey::Code(KeyCode::ArrowRight) => Some("ArrowRight".to_string()),
                    PhysicalKey::Code(KeyCode::Tab) => Some("Tab".to_string()),
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                        Some("Enter".to_string())
                    }
                    PhysicalKey::Code(KeyCode::Escape) => Some("Escape".to_string()),
                    PhysicalKey::Code(KeyCode::Backspace) => Some("Backspace".to_string()),
                    PhysicalKey::Code(KeyCode::Delete) => Some("Delete".to_string()),
                    _ => match &event.logical_key {
                        Key::Character(s) => Some(s.to_string()),
                        Key::Named(NamedKey::Space) => Some(" ".to_string()),
                        _ => None,
                    },
                };
                if let Some(ks) = key_str {
                    r.scene
                        .app
                        .handle_key(&ks, false, false, false, &mut r.scene.browser);
                    if r.scene.flush_app_events() {
                        self.stop(el, None);
                    }
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

    let mut scene = ShellScene::new(options, load)?;
    let start = Instant::now();
    let duration = options.seconds.unwrap_or(0.1);
    let dt = 1.0 / 60.0;

    loop {
        scene.step_and_draw(dt);
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
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("BFE_HEADLESS_EGL").is_none() {
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
