//! Standalone native game application (`bfe-client` with no arguments).
//!
//! Coordinates the complete lifecycle:
//! `Startup` (bounded update handoff/check) -> `StartupLoader` (`bfe-desktop` content
//! discovery and preparation) -> `Shell` (`bfe-shell` + `bfe-online`) ->
//! `BattleLoading` -> `Battle` (`InstantAction` on `WorldThread` or `Online` on
//! `online::client::Client` with `bfe-battle-ui` overlays) -> `Shell` -> `Exit`.

pub mod battle;
pub mod cli;
pub mod script;
pub mod settings;
pub mod shell;
pub mod startup;

use crate::{
    app::{
        battle::{BattleAuthority, BattleController, BattleTransition},
        cli::{parse_args, CliDispatch, StandaloneOptions},
        script::{GateDriver, GatePhase},
        settings::{current_desktop_paths, AppSettingsStore},
        shell::{pump_headless_audio, ShellHost, ShellTransition},
        startup::{run_bounded_update_check, StartupContentState, UpdateCheckOutcome},
    },
    audio::{AudioDevice, SharedAudioRouter, Status as AudioStatus},
};
use bfe_desktop::presence::{activity::RawPresenceInput, PresenceController};
use bfe_online::ServerBrowserService;
use bfe_shell::{
    audio::ShellAudioMixer,
    launch::BattleRequest,
    screens::{MultiplayerLaunch, PopupState, ScreenId},
};
use glow::HasContext;
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay},
    prelude::*,
    surface::{Surface, SwapInterval, WindowSurface},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{
    cell::Cell,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, NamedKey, PhysicalKey},
    window::{CursorGrabMode, Fullscreen, Window, WindowId},
};

thread_local! {
    static RENDER_THREAD_ALLOCS: Cell<u64> = const { Cell::new(0) };
}

/// Increments the calling thread's allocation counter. Called by the global allocator
/// in `main.rs` without locks or heap allocations.
#[inline]
pub fn record_thread_alloc() {
    let _ = RENDER_THREAD_ALLOCS.try_with(|c| c.set(c.get().wrapping_add(1)));
}

/// Returns the current thread's cumulative heap allocation count.
#[inline]
pub fn render_thread_alloc_count() -> u64 {
    RENDER_THREAD_ALLOCS.try_with(|c| c.get()).unwrap_or(0)
}

/// Target battle to load during [`AppState::BattleLoading`].
#[derive(Clone, Debug)]
pub enum BattleLoadTarget {
    InstantAction {
        request: BattleRequest,
        playlist_index: usize,
    },
    Online {
        launch: MultiplayerLaunch,
    },
}

impl BattleLoadTarget {
    pub fn map_id(&self) -> &str {
        match self {
            Self::InstantAction {
                request,
                playlist_index,
            } => request
                .playlist
                .as_ref()
                .and_then(|p| p.entries.get(*playlist_index))
                .map(|e| e.map.as_str())
                .unwrap_or(request.map.as_str()),
            Self::Online { launch } => {
                if launch.map.is_empty() {
                    "nab2"
                } else {
                    launch.map.as_str()
                }
            }
        }
    }
}

/// Top-level standalone game state machine.
pub enum AppState {
    StartupLoader(StartupContentState),
    Shell,
    BattleLoading {
        target: BattleLoadTarget,
        frames_drawn: u32,
    },
    Battle(Box<BattleController>),
}

/// Main entry point for `bfe-client`.
pub fn run(with_audio: bool) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(args) {
        Ok(CliDispatch::Help(text)) => {
            println!("{text}");
            std::process::exit(0);
        }
        Ok(CliDispatch::Version(ver)) => {
            println!("{ver}");
            std::process::exit(0);
        }
        Ok(CliDispatch::LegacyLocal(raw)) => {
            if !crate::local::launch::dispatch() {
                let result = crate::local::options::Options::parse(
                    &raw,
                    std::env::var_os("BFE_CONTENT_DIR"),
                )
                .and_then(|options| options.ok_or_else(|| "missing local battle mode".into()))
                .and_then(crate::local::launch::run);
                if let Err(error) = result {
                    eprintln!("bfe-client: {error}");
                    std::process::exit(1);
                }
            }
        }
        Ok(CliDispatch::LegacyJoin(_)) | Ok(CliDispatch::LegacyWindow(_)) => {
            crate::session_window::run_with_audio(with_audio);
        }
        Ok(CliDispatch::Standalone(opts)) => {
            if let Err(err) = run_standalone_game(opts, with_audio) {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

/// Runs the standalone native game (`Startup` -> `StartupLoader` -> `Shell` -> `Battle`).
pub fn run_standalone_game(opts: StandaloneOptions, with_audio: bool) -> Result<(), String> {
    let paths = current_desktop_paths();
    let mut settings = AppSettingsStore::load(paths);

    // Deliverable 1: Bounded startup update handoff/check before opening the window.
    if let UpdateCheckOutcome::UpdateAvailable { version, kind } =
        run_bounded_update_check(&settings, opts.skip_update_check)
    {
        eprintln!("update available: {version} ({kind})");
    }

    let startup_content =
        StartupContentState::resolve(opts.content_dir.as_deref(), &mut settings);

    let audio_status = Arc::new(AudioStatus::default());
    let audio_router = SharedAudioRouter::new();
    let audio_device = if with_audio && opts.audio {
        match AudioDevice::open_dynamic(audio_router.clone()) {
            Ok(dev) => Some(dev),
            Err(err) => {
                eprintln!("audio device unavailable ({err}); pumping headless audio");
                None
            }
        }
    } else {
        None
    };

    let gate = opts
        .script
        .as_deref()
        .map(|name| GateDriver::new(name, opts.shots_dir.clone()));

    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = StandaloneWindowApp {
        opts,
        settings,
        audio_status,
        audio_router,
        _audio_device: audio_device,
        headless_scratch: [0.0; ::audio::FRAMES * 2],
        state: AppState::StartupLoader(startup_content),
        shell: None,
        window: None,
        surface: None,
        context: None,
        gl: None,
        focused: true,
        pointer_locked: false,
        fullscreen: false,
        last_frame: Instant::now(),
        presence: PresenceController::default(),
        last_presence_key: String::new(),
        gate,
        error: None,
    };

    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    if let Some(err) = app.error {
        return Err(err);
    }
    Ok(())
}

struct StandaloneWindowApp {
    opts: StandaloneOptions,
    settings: AppSettingsStore,
    audio_status: Arc<AudioStatus>,
    audio_router: Arc<SharedAudioRouter>,
    _audio_device: Option<AudioDevice>,
    headless_scratch: [f32; ::audio::FRAMES * 2],
    state: AppState,
    shell: Option<ShellHost>,
    window: Option<Window>,
    surface: Option<Surface<WindowSurface>>,
    context: Option<PossiblyCurrentContext>,
    gl: Option<glow::Context>,
    focused: bool,
    pointer_locked: bool,
    fullscreen: bool,
    last_frame: Instant,
    presence: PresenceController,
    last_presence_key: String,
    gate: Option<GateDriver>,
    error: Option<String>,
}

impl StandaloneWindowApp {
    fn sync_pointer_lock(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let want_lock = match &self.state {
            AppState::Battle(battle) => self.focused && battle.wants_pointer_lock(),
            _ => false,
        };
        if want_lock == self.pointer_locked {
            return;
        }
        if want_lock {
            let locked = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
            window.set_cursor_visible(!locked);
            self.pointer_locked = locked;
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
            self.pointer_locked = false;
        }
    }

    fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        if let Some(window) = &self.window {
            window.set_fullscreen(
                self.fullscreen
                    .then_some(Fullscreen::Borderless(None)),
            );
        }
    }

    fn choose_content_folder(&mut self) {
        let picked = std::env::var("BFE_CONTENT_SOURCE")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                self.settings
                    .desktop
                    .game_dir
                    .as_ref()
                    .map(PathBuf::from)
            });
        if let Some(folder) = picked {
            self.settings.desktop.game_dir = Some(folder.to_string_lossy().to_string());
            let _ = self.settings.save_desktop();
            self.state = AppState::StartupLoader(StartupContentState::resolve(
                Some(&folder),
                &mut self.settings,
            ));
        }
    }

    fn update_rich_presence(&mut self) {
        let raw = match &self.state {
            AppState::StartupLoader(_) => RawPresenceInput {
                phase: Some("preparing".to_string()),
                ..RawPresenceInput::default()
            },
            AppState::Shell => {
                let is_lobby = self
                    .shell
                    .as_ref()
                    .is_some_and(|s| s.app.current_screen() == ScreenId::Multiplayer);
                RawPresenceInput {
                    phase: Some(if is_lobby { "lobby" } else { "menu" }.to_string()),
                    ..RawPresenceInput::default()
                }
            }
            AppState::BattleLoading { target, .. } => RawPresenceInput {
                phase: Some("loading".to_string()),
                map_id: Some(target.map_id().to_string()),
                ..RawPresenceInput::default()
            },
            AppState::Battle(battle) => match &battle.authority {
                BattleAuthority::InstantAction { map_id, era, .. } => RawPresenceInput {
                    phase: Some("match".to_string()),
                    mode: Some("instant-action".to_string()),
                    map_id: Some(map_id.clone()),
                    era: Some(era.clone()),
                    ..RawPresenceInput::default()
                },
                BattleAuthority::Online {
                    map_id,
                    era,
                    launch,
                    ..
                } => RawPresenceInput {
                    phase: Some("match".to_string()),
                    mode: Some("multiplayer".to_string()),
                    map_id: Some(map_id.clone()),
                    era: Some(era.clone()),
                    room_name: Some(launch.server_id.clone()),
                    party_size: Some(1),
                    party_max: Some(32),
                    ..RawPresenceInput::default()
                },
            },
        };

        let key = format!("{raw:?}");
        if key != self.last_presence_key {
            let _ = self.presence.set_activity(&raw);
            self.last_presence_key = key;
        }
    }

    fn restore_shell_audio(&self, content_dir: &Path) {
        if let Some(shell) = &self.shell {
            let mut mixer = ShellAudioMixer::new(&shell.app.data, Some(content_dir));
            mixer.set_volumes(
                shell.app.settings.master_volume,
                shell.app.settings.music_volume,
                shell.app.settings.interface_volume,
            );
            self.audio_router.set_live(None);
            self.audio_router.set_shell(Some(mixer));
        }
    }

    fn blit_framebuffer_to_window(&self, src_fb: glow::Framebuffer, src_size: [i32; 2]) {
        let (Some(gl), Some(surface), Some(context)) = (&self.gl, &self.surface, &self.context)
        else {
            return;
        };
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(src_fb));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
            gl.disable(glow::SCISSOR_TEST);
            gl.blit_framebuffer(
                0,
                0,
                src_size[0],
                src_size[1],
                0,
                0,
                self.opts.size[0],
                self.opts.size[1],
                glow::COLOR_BUFFER_BIT,
                glow::LINEAR,
            );
        }
        let _ = surface.swap_buffers(context);
    }

    fn check_gl_errors(&mut self) {
        if let Some(gl) = &self.gl {
            let err = unsafe { gl.get_error() };
            if err != glow::NO_ERROR {
                if let Some(gate) = &mut self.gate {
                    gate.gl_errors += 1;
                }
            }
        }
    }
}

impl ApplicationHandler for StandaloneWindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let init_result = (|| -> Result<(), String> {
            self.fullscreen = self.opts.fullscreen;
            let mut attrs = Window::default_attributes()
                .with_title("Star Wars Battlefront Eternal")
                .with_inner_size(PhysicalSize::new(
                    self.opts.size[0] as u32,
                    self.opts.size[1] as u32,
                ))
                .with_visible(true);
            if self.fullscreen {
                attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
            }

            let template = ConfigTemplateBuilder::new()
                .with_alpha_size(8)
                .with_depth_size(24)
                .with_stencil_size(8);
            let (window, config) = DisplayBuilder::new()
                .with_window_attributes(Some(attrs))
                .build(event_loop, template, |configs| {
                    configs
                        .reduce(|a, b| {
                            if b.num_samples() == 0 && a.num_samples() != 0 {
                                b
                            } else {
                                a
                            }
                        })
                        .expect("GL config")
                })
                .map_err(|e| e.to_string())?;
            let window = window.ok_or("window was not created")?;
            let raw_handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
            let display = config.display();

            let context_api = if self.opts.desktop_gl {
                ContextApi::OpenGl(Some(Version::new(3, 3)))
            } else {
                ContextApi::Gles(Some(Version::new(3, 0)))
            };
            let ctx_attrs = ContextAttributesBuilder::new()
                .with_context_api(context_api)
                .build(Some(raw_handle));
            let fallback_attrs = ContextAttributesBuilder::new()
                .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
                .build(Some(raw_handle));

            let not_current = unsafe {
                display
                    .create_context(&config, &ctx_attrs)
                    .or_else(|_| display.create_context(&config, &fallback_attrs))
            }
            .map_err(|e| e.to_string())?;

            let surface_attrs = window
                .build_surface_attributes(Default::default())
                .map_err(|e| e.to_string())?;
            let surface = unsafe { display.create_window_surface(&config, &surface_attrs) }
                .map_err(|e| e.to_string())?;
            let context = not_current
                .make_current(&surface)
                .map_err(|e| e.to_string())?;
            let _ = surface.set_swap_interval(&context, SwapInterval::DontWait);

            let make_gl = || unsafe {
                glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name))
            };
            let gl = make_gl();
            let shell = ShellHost::new(
                self.opts.size,
                &self.settings,
                self.opts.server_target.clone(),
                make_gl,
            )?;

            self.window = Some(window);
            self.surface = Some(surface);
            self.context = Some(context);
            self.gl = Some(gl);
            self.shell = Some(shell);
            self.last_frame = Instant::now();
            Ok(())
        })();

        if let Err(err) = init_result {
            self.error = Some(err);
            event_loop.exit();
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            if self.pointer_locked && self.focused {
                if let AppState::Battle(battle) = &mut self.state {
                    battle.input.motion(dx, dy);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Focused(focused) => {
                let focused = if self.gate.is_some() { true } else { focused };
                self.focused = focused;
                if !focused {
                    if let AppState::Battle(battle) = &mut self.state {
                        battle.input.clear();
                        if !battle.spawn_ctrl.is_open()
                            && !battle.scoreboard_ctrl.model.is_open()
                            && !battle.pause_menu.is_open
                        {
                            battle.pause_menu.open("focus", battle.height);
                        }
                    }
                }
                self.sync_pointer_lock();
            }
            WindowEvent::Resized(phys) => {
                if phys.width > 0 && phys.height > 0 {
                    let new_size = [phys.width as i32, phys.height as i32];
                    self.opts.size = new_size;
                    if let (Some(surface), Some(context)) = (&self.surface, &self.context) {
                        if let (Some(w), Some(h)) = (
                            NonZeroU32::new(phys.width),
                            NonZeroU32::new(phys.height),
                        ) {
                            surface.resize(context, w, h);
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                match &mut self.state {
                    AppState::Shell => {
                        if let Some(shell) = &mut self.shell {
                            shell.handle_cursor_move(position.x, position.y);
                        }
                    }
                    AppState::Battle(battle) => {
                        battle.handle_cursor_move(position.x, position.y);
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                match &mut self.state {
                    AppState::StartupLoader(StartupContentState::WaitingForFolder { .. }) => {
                        if down && button == MouseButton::Left {
                            self.choose_content_folder();
                        }
                    }
                    AppState::Shell => {
                        if down && button == MouseButton::Left {
                            if let Some(shell) = &mut self.shell {
                                let [w, h] = shell.size;
                                let transition = shell.handle_mouse_click(
                                    w as f64 * 0.5,
                                    h as f64 * 0.5,
                                    &mut self.settings,
                                    &self.audio_router,
                                );
                                self.apply_shell_transition(transition, event_loop);
                            }
                        }
                    }
                    AppState::Battle(battle) => {
                        let code = match button {
                            MouseButton::Left => Some(0),
                            MouseButton::Middle => Some(1),
                            MouseButton::Right => Some(2),
                            _ => None,
                        };
                        if let Some(b) = code {
                            battle.handle_mouse_button(b, down, &self.settings);
                        }
                        self.sync_pointer_lock();
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as f64 * 100.0,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                if let AppState::Battle(battle) = &mut self.state {
                    battle.handle_wheel(dy);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                if down && !event.repeat {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        match code {
                            KeyCode::F11 => {
                                self.toggle_fullscreen();
                                return;
                            }
                            KeyCode::F9 => {
                                self.choose_content_folder();
                                return;
                            }
                            _ => {}
                        }
                    }
                }

                match &mut self.state {
                    AppState::StartupLoader(StartupContentState::WaitingForFolder { .. }) => {
                        if down && !event.repeat {
                            self.choose_content_folder();
                        }
                    }
                    AppState::Shell => {
                        if down {
                            if let Some(key_str) = map_shell_key(&event.logical_key) {
                                if let Some(shell) = &mut self.shell {
                                    let transition = shell.handle_key(
                                        &key_str,
                                        &mut self.settings,
                                        &self.audio_router,
                                    );
                                    self.apply_shell_transition(transition, event_loop);
                                }
                            }
                        }
                    }
                    AppState::Battle(battle) => {
                        if let PhysicalKey::Code(code) = event.physical_key {
                            if let Some(dom_code) = keycode_to_dom_code(code) {
                                battle.handle_key(
                                    dom_code,
                                    down,
                                    event.repeat,
                                    &mut self.settings,
                                );
                            }
                        }
                        self.sync_pointer_lock();
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                self.step_frame(event_loop);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl StandaloneWindowApp {
    fn apply_shell_transition(
        &mut self,
        transition: Option<ShellTransition>,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(transition) = transition else {
            return;
        };
        match transition {
            ShellTransition::StartInstantAction(request) => {
                if let Some(shell) = &mut self.shell {
                    let map_id = request
                        .playlist
                        .as_ref()
                        .and_then(|p| p.entries.first())
                        .map(|e| e.map.as_str())
                        .unwrap_or(request.map.as_str());
                    shell.prepare_map_loading(map_id, &self.audio_router);
                }
                self.state = AppState::BattleLoading {
                    target: BattleLoadTarget::InstantAction {
                        request,
                        playlist_index: 0,
                    },
                    frames_drawn: 0,
                };
            }
            ShellTransition::StartOnline(launch) => {
                if let Some(shell) = &mut self.shell {
                    let map_id = if launch.map.is_empty() {
                        "nab2"
                    } else {
                        launch.map.as_str()
                    };
                    shell.prepare_map_loading(map_id, &self.audio_router);
                }
                self.state = AppState::BattleLoading {
                    target: BattleLoadTarget::Online { launch },
                    frames_drawn: 0,
                };
            }
            ShellTransition::Exit => {
                if let Some(gate) = &self.gate {
                    let _ = gate.write_summary_report();
                }
                event_loop.exit();
            }
        }
    }

    fn step_frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().clamp(0.001, 0.1);
        self.last_frame = now;

        if self._audio_device.is_none() {
            pump_headless_audio(
                &self.audio_router,
                &self.audio_status,
                &mut self.headless_scratch,
            );
        }

        self.update_rich_presence();
        self.sync_pointer_lock();

        let frame_res: Result<(), String> = (|| {
            match &mut self.state {
                AppState::StartupLoader(loader_state) => {
                    loader_state.poll(&mut self.settings);
                    let blit_info = if let Some(shell) = &mut self.shell {
                        shell.draw_startup_loader(dt, loader_state);
                        Some((shell.canvas.framebuffer, shell.size))
                    } else {
                        None
                    };
                    let ready_dir = match loader_state {
                        StartupContentState::Ready(dir) => Some(dir.clone()),
                        _ => None,
                    };
                    if let Some((fb, sz)) = blit_info {
                        self.blit_framebuffer_to_window(fb, sz);
                    }
                    if let Some(content_dir) = ready_dir {
                        if let Some(shell) = &mut self.shell {
                            shell.load_prepared_content(
                                &content_dir,
                                &self.settings,
                                &self.audio_router,
                            )?;
                            shell.draw_startup_loader(
                                0.016,
                                &StartupContentState::Ready(content_dir),
                            );
                            if let Some(gate) = &mut self.gate {
                                if gate.phase == GatePhase::StartupLoader {
                                    gate.save_shell_shot(shell, "01-startup-loader.png")?;
                                    gate.transition_to(GatePhase::GateScreen);
                                }
                            }
                        }
                        self.state = AppState::Shell;
                    }
                }
                AppState::Shell => {
                    let mut transition = if let Some(shell) = &mut self.shell {
                        let t = shell.step_and_draw(dt, &mut self.settings, &self.audio_router);
                        let fb = shell.canvas.framebuffer;
                        let sz = shell.size;
                        self.blit_framebuffer_to_window(fb, sz);
                        t
                    } else {
                        None
                    };
                    if transition.is_none() {
                        if let (Some(gate), Some(shell)) = (&mut self.gate, &mut self.shell) {
                            transition = gate.step_shell(shell, &mut self.settings)?;
                        }
                    }
                    self.apply_shell_transition(transition, event_loop);
                }
                AppState::BattleLoading {
                    target,
                    frames_drawn,
                } => {
                    *frames_drawn += 1;
                    let current_frames = *frames_drawn;
                    let target_clone = target.clone();
                    if let Some(shell) = &mut self.shell {
                        shell.draw_battle_loading(dt);
                        if current_frames == 6 {
                            if let Some(gate) = &mut self.gate {
                                if !gate
                                    .screenshots_written
                                    .iter()
                                    .any(|s| s == "08-ia-loading.png")
                                {
                                    gate.save_shell_shot(shell, "08-ia-loading.png")?;
                                }
                            }
                        }
                        let fb = shell.canvas.framebuffer;
                        let sz = shell.size;
                        self.blit_framebuffer_to_window(fb, sz);
                    }
                    if current_frames >= 10 {
                        let content_dir = self
                            .shell
                            .as_ref()
                            .and_then(|s| s.content_dir.clone())
                            .unwrap_or_else(|| self.settings.paths.content_root.clone());
                        let Some(context) = &self.context else {
                            return Err("missing GL context".to_string());
                        };
                        let display = context.display();
                        let make_gl = || unsafe {
                            glow::Context::from_loader_function_cstr(|name| {
                                display.get_proc_address(name)
                            })
                        };
                        match target_clone {
                            BattleLoadTarget::InstantAction {
                                request,
                                playlist_index,
                            } => {
                                let battle = BattleController::start_instant_action(
                                    &content_dir,
                                    request,
                                    playlist_index,
                                    self.opts.size,
                                    &self.settings,
                                    &self.audio_router,
                                    make_gl,
                                )?;
                                self.state = AppState::Battle(Box::new(battle));
                            }
                            BattleLoadTarget::Online { launch } => {
                                let selected_addr = self.shell.as_ref().and_then(|s| {
                                    s.browser.selected().and_then(|entry| {
                                        entry.address.as_ref().map(|a| {
                                            format!("{a}:{}", entry.port.unwrap_or(27015))
                                        })
                                    })
                                });
                                match BattleController::start_online(
                                    &content_dir,
                                    launch,
                                    self.opts.server_target.clone().or(selected_addr),
                                    self.opts.size,
                                    &self.settings,
                                    &self.audio_router,
                                    make_gl,
                                ) {
                                    Ok(battle) => {
                                        self.state = AppState::Battle(Box::new(battle));
                                    }
                                    Err(err) => {
                                        if self.gate.is_some() {
                                            return Err(format!("Online join failed during gate: {err}"));
                                        }
                                        self.restore_shell_audio(&content_dir);
                                        if let Some(shell) = &mut self.shell {
                                            shell.app.push_screen(ScreenId::Multiplayer);
                                            shell.app.popup = Some(PopupState::Notice {
                                                message: format!("DISCONNECTED: {err}"),
                                            });
                                        }
                                        self.state = AppState::Shell;
                                    }
                                }
                            }
                        }
                    }
                }
                AppState::Battle(battle) => {
                    if let Some(gate) = &mut self.gate {
                        gate.step_battle(battle, &mut self.settings)?;
                    }
                    let transition = battle.step_and_draw(
                        dt,
                        self.focused,
                        &mut self.settings,
                        &self.audio_router,
                    )?;
                    let fb = battle.scene.canvas.framebuffer;
                    let sz = battle.size;
                    self.blit_framebuffer_to_window(fb, sz);

                    if let Some(t) = transition {
                        let content_dir = self
                            .shell
                            .as_ref()
                            .and_then(|s| s.content_dir.clone())
                            .unwrap_or_else(|| self.settings.paths.content_root.clone());
                        match t {
                            BattleTransition::NextPlaylistBattle {
                                request,
                                next_index,
                            } => {
                                let playlist_len = request
                                    .playlist
                                    .as_ref()
                                    .map(|p| p.entries.len())
                                    .unwrap_or(0);
                                if next_index < playlist_len {
                                    if let Some(shell) = &mut self.shell {
                                        if let Some(next_entry) = request
                                            .playlist
                                            .as_ref()
                                            .and_then(|p| p.entries.get(next_index))
                                        {
                                            shell.prepare_map_loading(
                                                &next_entry.map,
                                                &self.audio_router,
                                            );
                                        }
                                    }
                                    self.state = AppState::BattleLoading {
                                        target: BattleLoadTarget::InstantAction {
                                            request,
                                            playlist_index: next_index,
                                        },
                                        frames_drawn: 0,
                                    };
                                } else {
                                    self.restore_shell_audio(&content_dir);
                                    if let Some(shell) = &mut self.shell {
                                        shell.app.push_screen(ScreenId::InstantAction);
                                    }
                                    self.state = AppState::Shell;
                                }
                            }
                            BattleTransition::RestartMission {
                                request,
                                playlist_index,
                            } => {
                                if let Some(shell) = &mut self.shell {
                                    let map_id = request
                                        .playlist
                                        .as_ref()
                                        .and_then(|p| p.entries.get(playlist_index))
                                        .map(|e| e.map.as_str())
                                        .unwrap_or(request.map.as_str());
                                    shell.prepare_map_loading(map_id, &self.audio_router);
                                }
                                self.state = AppState::BattleLoading {
                                    target: BattleLoadTarget::InstantAction {
                                        request,
                                        playlist_index,
                                    },
                                    frames_drawn: 0,
                                };
                            }
                            BattleTransition::QuitToMainMenu => {
                                self.restore_shell_audio(&content_dir);
                                if let Some(shell) = &mut self.shell {
                                    shell.app.stack = vec![ScreenId::Main];
                                    shell.app.rebuild_cached_rows();
                                }
                                self.state = AppState::Shell;
                            }
                            BattleTransition::LeaveOnline { reason } => {
                                self.restore_shell_audio(&content_dir);
                                if let Some(shell) = &mut self.shell {
                                    shell.app.push_screen(ScreenId::Multiplayer);
                                    if let Some(msg) = reason {
                                        shell.app.popup = Some(PopupState::Notice {
                                            message: format!("DISCONNECTED: {msg}"),
                                        });
                                    }
                                }
                                self.state = AppState::Shell;
                            }
                            BattleTransition::ExitToWindows => {
                                event_loop.exit();
                            }
                        }
                    }
                }
            }
            Ok(())
        })();

        self.check_gl_errors();

        if let Err(err) = frame_res {
            self.error = Some(err);
            event_loop.exit();
        }
    }
}

fn map_shell_key(key: &Key) -> Option<String> {
    match key {
        Key::Named(NamedKey::ArrowUp) => Some("ArrowUp".to_string()),
        Key::Named(NamedKey::ArrowDown) => Some("ArrowDown".to_string()),
        Key::Named(NamedKey::ArrowLeft) => Some("ArrowLeft".to_string()),
        Key::Named(NamedKey::ArrowRight) => Some("ArrowRight".to_string()),
        Key::Named(NamedKey::Enter) => Some("Enter".to_string()),
        Key::Named(NamedKey::Escape) => Some("Escape".to_string()),
        Key::Named(NamedKey::Backspace) => Some("Backspace".to_string()),
        Key::Named(NamedKey::Delete) => Some("Delete".to_string()),
        Key::Named(NamedKey::Tab) => Some("Tab".to_string()),
        Key::Named(NamedKey::Space) => Some(" ".to_string()),
        Key::Character(s) => {
            let ch = s.chars().next()?;
            if ch.is_control() {
                None
            } else {
                Some(ch.to_string())
            }
        }
        _ => None,
    }
}

fn keycode_to_dom_code(code: KeyCode) -> Option<&'static str> {
    match code {
        KeyCode::KeyW => Some("KeyW"),
        KeyCode::KeyA => Some("KeyA"),
        KeyCode::KeyS => Some("KeyS"),
        KeyCode::KeyD => Some("KeyD"),
        KeyCode::KeyE => Some("KeyE"),
        KeyCode::KeyR => Some("KeyR"),
        KeyCode::KeyC => Some("KeyC"),
        KeyCode::KeyX => Some("KeyX"),
        KeyCode::KeyZ => Some("KeyZ"),
        KeyCode::KeyV => Some("KeyV"),
        KeyCode::KeyF => Some("KeyF"),
        KeyCode::KeyQ => Some("KeyQ"),
        KeyCode::KeyM => Some("KeyM"),
        KeyCode::KeyT => Some("KeyT"),
        KeyCode::KeyY => Some("KeyY"),
        KeyCode::Space => Some("Space"),
        KeyCode::ShiftLeft => Some("ShiftLeft"),
        KeyCode::ShiftRight => Some("ShiftRight"),
        KeyCode::ControlLeft => Some("ControlLeft"),
        KeyCode::ControlRight => Some("ControlRight"),
        KeyCode::Tab => Some("Tab"),
        KeyCode::Enter => Some("Enter"),
        KeyCode::NumpadEnter => Some("NumpadEnter"),
        KeyCode::Escape => Some("Escape"),
        KeyCode::Backspace => Some("Backspace"),
        KeyCode::ArrowUp => Some("ArrowUp"),
        KeyCode::ArrowDown => Some("ArrowDown"),
        KeyCode::ArrowLeft => Some("ArrowLeft"),
        KeyCode::ArrowRight => Some("ArrowRight"),
        KeyCode::PageUp => Some("PageUp"),
        KeyCode::PageDown => Some("PageDown"),
        KeyCode::Home => Some("Home"),
        KeyCode::End => Some("End"),
        KeyCode::Digit1 => Some("Digit1"),
        KeyCode::Digit2 => Some("Digit2"),
        KeyCode::Digit3 => Some("Digit3"),
        KeyCode::Digit4 => Some("Digit4"),
        KeyCode::F1 => Some("F1"),
        KeyCode::F2 => Some("F2"),
        KeyCode::F3 => Some("F3"),
        KeyCode::F4 => Some("F4"),
        _ => None,
    }
}
