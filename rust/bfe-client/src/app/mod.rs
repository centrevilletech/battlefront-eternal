//! Standalone native game application (`bfe-client` with no arguments).
//!
//! Coordinates the complete lifecycle:
//! `Startup` (bounded update handoff/check + hosted server `attach()`) ->
//! `StartupLoader` (`bfe-desktop` content discovery and preparation) ->
//! `Shell` (`bfe-shell` + `bfe-interface` options + `bfe-online` + `HostController`) ->
//! `HostStarting` (progress until `/info` answers) ->
//! `BattleLoading` -> `Battle` (`InstantAction` on `WorldThread` via `Context::world_admit`
//! or `Online` on `online::client::Client` with `bfe-battle-ui` overlays) -> `Shell` -> `Exit`.
//!
//! ## Threading and Allocation Invariants
//! - The main (`winit`) thread owns the OpenGL context, the front-end [`ShellHost`],
//!   and the 3D battle [`BattleController`].
//! - During an Instant Action battle, the authoritative simulation runs on a dedicated
//!   60 Hz thread (`"bfe-world-60hz"`) admitted via `Context::world_admit`, exchanging
//!   inputs and frame publications over lock-free bounded channels without ever blocking
//!   the render thread.
//! - A thread-local allocation counter (`RENDER_THREAD_ALLOCS`) is incremented by the
//!   global allocator in `main.rs` so `--script gate` can verify zero steady-state
//!   heap allocations on the render thread after initial warmup.
//!
//! ## Subsystem Ownership Summary
//! | Subsystem | Crate / Module | Responsibility |
//! |---|---|---|
//! | Window & GL Context | `winit` + `glutin` + `glow` | OS windowing, EGL/GLX surface, GLES 3.0 / GL 3.3 context |
//! | Desktop Lifecycle | `bfe_desktop` | Paths, atomic settings, self-update check, `PrepareSession`, `HostController`, Discord Rich Presence |
//! | Front-End Shell | `bfe_shell` | Gate, Main Menu, Singleplayer, Instant Action playlist, Profile, Multiplayer browser, Host dialog |
//! | Options & Settings | `bfe_interface` | 59-key `Settings` schema, `OptionsController`, `ClassicTheme::paint_stack` (`GAME`, `VIDEO`, `AUDIO`, `CONTROLS`) |
//! | Server Browser | `bfe_online` | Master directory queries, LAN UDP broadcast discovery, direct `/info` probes |
//! | Battle UI Overlays | `bfe_battle_ui` | `SpawnScreenController`, `ScoreboardController`, `PauseMenu`, `ChatController` |
//! | Local Simulation | `WorldThread` (`Context::world_admit`) | Dedicated 60 Hz mission simulation thread (`"bfe-world-60hz"`) |
//! | Online Client | `crate::online::client::Client` | Authenticated UDP/WebRTC session, snapshot receiver, client-side prediction replay |
//! | 3D Scene & HUD | `crate::scene::Scene` + `bfe_render` | Terrain, sky, water, skeletal/skinned models, particles, and 3D-anchored HUD batch |
//! | Audio Routing | `SharedAudioRouter` | Lock-free switch between front-end `ShellAudioMixer` and in-battle 3D `Stream` |
//!
//! ## Hosted Server Lifecycle (`bfe_desktop::host::HostController`)
//! - **Startup Re-Attach**: `run_standalone_game` constructs `HostController` pointing at
//!   `<user_data>/server` and calls `attach()` so any detached `bfeserver` left running
//!   from a previous session (`CloseChoice::KeepRunning`) is adopted automatically.
//! - **Create & Launch**: Selecting `CREATE` on `ScreenId::HostDialog` maps `HostConfig`
//!   into `HostOptions` (`host_options_from_shell`) and transitions to `AppState::HostStarting`,
//!   which spawns `bfeserver` and polls `/admin` and `/info` on `"bfe-host-start"` while
//!   rendering the loading screen. Once `/info` answers, the local player joins through
//!   the standard `BattleLoadTarget::Online` path (`BattleController::prepare_online`).
//! - **Close Prompt**: Closing the window while a hosted server is active opens the
//!   3-button `ClosePrompt` modal (`KeepRunning`, `StopAndQuit`, `Cancel`).

//! ## State Machine Transitions
//! ```text
//! [Startup: run_bounded_update_check + HostController::attach]
//!        |
//!        v
//! [AppState::StartupLoader] --(content ready)--> [AppState::Shell]
//!                                                   |          ^
//!                    +------------------------------+          |
//!                    | (LAUNCH / JOIN / CREATE)                | (Quit / Leave / Playlist end)
//!                    v                                         |
//!        [AppState::HostStarting]                              |
//!                    | (/info answers)                         |
//!                    v                                         |
//!        [AppState::BattleLoading] ----------------------------> [AppState::Battle]
//! ```
//!
//! ## Key Handling and Coordinate Spaces
//! - **Front-end Shell (`AppState::Shell`)**: Logical keys (`Key::Named` / `Key::Character`)
//!   are mapped via [`map_shell_key`]. When `ScreenId::Options` or `ScreenId::OptionsPage(_)`
//!   is active, pointer and keyboard events route through `bfe_interface::OptionsController`
//!   and `bfe_interface::ClassicTheme::paint_stack`.
//! - **In-Battle (`AppState::Battle`)**: Physical key codes (`PhysicalKey::Code`) are mapped
//!   to DOM `KeyboardEvent.code` strings via [`keycode_to_dom_code`] so `bfe-battle-ui`
//!   (`PauseMenu`, `ChatController`, `SpawnScreenController`, `ScoreboardController`)
//!   and 3D movement (`Input`) share identical key identifiers with the web reference.
pub mod battle;
pub mod cli;
mod loading;
mod platform_smoke;
mod slint_ui;
pub mod script;
pub mod settings;
pub mod shell;
mod page_font;
mod page_backdrop;
mod page_blur;
mod page_integrations;
mod page_proof;
pub(crate) mod page_battle;
mod page_pause;
mod page_browser;
mod page_surfaces;
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
use bfe_desktop::{
    host::{
        resolve_server_executable, ClosePrompt, HostController, HostControllerConfig, HostJoinInfo,
        HostOptions,
    },
    presence::{activity::RawPresenceInput, PresenceController},
};
use bfe_shell::{
    audio::ShellAudioMixer,
    launch::BattleRequest,
    screens::{MultiplayerLaunch, PopupState, ScreenId},
};
mod text_input;

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
    collections::BTreeMap,
    num::NonZeroU32,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc, Arc, Mutex},
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::{CursorGrabMode, Fullscreen, Window, WindowId},
};

#[cfg(target_os = "linux")]
mod x11_pointer;

thread_local! {
    /// Per-thread cumulative heap allocation counter updated by the global `CountingAlloc`
    /// in `main.rs` without locks or re-entrant allocations.
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
    /// Local Instant Action battle (`BattleRequest` and current index in its playlist rotation).
    InstantAction {
        /// Mission and playlist configuration selected on `ScreenId::InstantAction`.
        request: BattleRequest,
        /// Zero-based index into `request.playlist` for the battle being loaded.
        playlist_index: usize,
    },
    /// Online or locally hosted multiplayer battle joined over UDP (`online::client::Client`).
    Online {
        /// Target server address, port, map, era, and player name.
        launch: MultiplayerLaunch,
    },
}

impl BattleLoadTarget {
    /// Returns the map identifier (`"nab2"`, `"tat1"`, `"bes1"`, etc.) for the battle being loaded.
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
    /// Startup content discovery and background preparation loader screen.
    StartupLoader(StartupContentState),
    /// Front-end shell (`Gate`, `Main`, `InstantAction`, `bfe-interface` `Options`, `Multiplayer`, `HostDialog`).
    Shell,
    /// Progress screen displayed while `HostController::start` spawns `bfeserver` and polls `/info`.
    HostStarting {
        /// Validated host configuration from `SET GAME OPTIONS`.
        options: HostOptions,
        /// Online launch descriptor that will be updated with `HostJoinInfo` once `/info` responds.
        launch: MultiplayerLaunch,
        /// One-shot receiver delivering `Result<HostJoinInfo, String>` from the `"bfe-host-start"` thread.
        rx: mpsc::Receiver<Result<HostJoinInfo, String>>,
        /// Number of loading frames rendered while waiting for `/info`.
        frames_drawn: u32,
    },
    /// Animated planet zoom loading screen rendered before admitting/connecting a battle.
    BattleLoading {
        /// Instant Action or Online battle target to initialize once loading frames complete.
        target: BattleLoadTarget,
        /// Number of loading frames rendered so far.
        frames_drawn: u32,
        work: Box<loading::Work>,
    },
    /// Active 3D battle with `bfe-battle-ui` overlays (`Spawn`, `HUD`, `Scoreboard`, `Pause`).
    Battle(Box<BattleController>),
}

/// Main entry point for `bfe-client`.
///
/// Parses command-line arguments first so `--help` / `--version` exit 0 immediately
/// before touching any display server or audio device, and routes either to legacy
/// diagnostic sub-modes (`--local`, `--join`, `--map`) or to [`run_standalone_game`].
pub fn run(with_audio: bool) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(args) {
        Ok(CliDispatch::Help(text)) => {
            // Print usage text to stdout and exit 0 cleanly.
            println!("{text}");
            std::process::exit(0);
        }
        Ok(CliDispatch::Version(ver)) => {
            // Print package version to stdout and exit 0 cleanly.
            println!("{ver}");
            std::process::exit(0);
        }
        Ok(CliDispatch::LegacyLocal(raw)) => {
            // Dispatch legacy `--local` single-battle CLI mode.
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
            // Dispatch legacy `--join`, `--map`, or recorded `SESSION_DIR` window.
            crate::session_window::run_with_audio(with_audio);
        }
        Ok(CliDispatch::Standalone(opts)) => {
            // Launch the default standalone native game lifecycle.
            if let Err(err) = run_standalone_game(opts, with_audio) {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Err(err) => {
            // Report invalid CLI arguments to stderr and exit 1.
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

/// Runs the standalone native game (`Startup` -> `StartupLoader` -> `Shell` -> `Battle`).
///
/// ## Initialization Sequence
/// 1. **Paths & Settings**: Resolves [`DesktopPaths`] (`current_desktop_paths`) and loads
///    `DesktopSettings`, `bfe_interface::Settings` (all 59 keys), and `PersistedPlayerSettings`
///    via [`AppSettingsStore::load`].
/// 2. **Bounded Update Check**: Executes [`run_bounded_update_check`] with a hard `<= 1.5s`
///    timeout so offline environments proceed immediately.
/// 3. **Startup Content Resolution**: Invokes [`StartupContentState::resolve`] to locate
///    prepared content (`common/common.json`) or start a background `PrepareSession`.
/// 4. **`HostController::attach`**: Initializes [`bfe_desktop::host::HostController`] pointing
///    at `<user_data>/server` and calls `attach()` so any detached `bfeserver` left running
///    from a prior session (`CloseChoice::KeepRunning`) is adopted automatically.
/// 5. **Dynamic Audio Router**: Opens a hardware `cpal` stream via [`AudioDevice::open_dynamic`]
///    or falls back to deterministic headless quantum pumping (`pump_headless_audio`).
/// 6. **Event Loop Execution**: Enters `winit`'s `ControlFlow::Poll` loop via `event_loop.run_app`.
pub fn run_standalone_game(opts: StandaloneOptions, with_audio: bool) -> Result<(), String> {
    // 1. Resolve application data paths and load `DesktopSettings`, `bfe_interface::Settings`,
    //    and `PersistedPlayerSettings`.
    let paths = current_desktop_paths();
    let mut settings = AppSettingsStore::load(paths);

    // 2. Bounded startup update handoff/check before opening the window (<= 1.5s timeout).
    if let UpdateCheckOutcome::UpdateAvailable { version, kind } =
        run_bounded_update_check(&settings, opts.skip_update_check)
    {
        eprintln!("update available: {version} ({kind})");
    }

    // 3. Resolve prepared content or start a background `PrepareSession`.
    let startup_content = StartupContentState::resolve(opts.content_dir.as_deref(), &mut settings);

    // 4. Wire `bfe_desktop::host::HostController` and re-attach to any surviving hosted
    //    server (`attach()`) at startup.
    let env_map: BTreeMap<String, String> = std::env::vars().collect();
    let current_exe = std::env::current_exe().ok();
    let mut host_cfg = HostControllerConfig::new(
        settings.paths.user_data.join("server"),
        settings.paths.content_root.clone(),
    );
    host_cfg.executable = resolve_server_executable(
        &env_map,
        current_exe.as_deref(),
        Some(&settings.paths.project_root),
    );
    host_cfg.online_consent = settings.player.privacy_notice_accepted;
    let host_controller =
        HostController::new(host_cfg).map_err(|e| format!("host controller init: {e}"))?;
    if let Err(err) = host_controller.attach() {
        eprintln!("host controller attach warning: {err}");
    }
    let host_controller = Arc::new(Mutex::new(host_controller));

    // 5. Initialize the shared audio router (`ShellAudioMixer` <-> live battle `Stream`),
    //    falling back to headless quantum pumping if no audio output device is available.
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

    // 6. Construct the automated `--script gate` driver if requested on the command line.
    let gate = opts
        .script
        .as_deref()
        .filter(|name| !matches!(*name, "platform-smoke" | "slint-proof"))
        .map(|name| GateDriver::new(name, opts.shots_dir.clone()));

    // 7. Create the `winit` event loop in continuous `Poll` mode for 60 Hz rendering.
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let platform_smoke = (opts.script.as_deref() == Some("platform-smoke"))
        .then(|| platform_smoke::Smoke::new(opts.shots_dir.clone()));
    let mut app = StandaloneWindowApp {
        platform_smoke,
        opts,
        settings,
        host_controller,
        audio_status,
        audio_router,
        _audio_device: audio_device,
        headless_scratch: vec![0.0; ::audio::FRAMES * 2]
            .into_boxed_slice()
            .try_into()
            .unwrap(),
        state: AppState::StartupLoader(startup_content),
        shell: None,
        #[cfg(target_os = "linux")]
        x11_grab: None,
        slint_ui: None,
        window: None,
        surface: None,
        context: None,
        gl: None,
        focused: true,
        pointer_locked: false,
        pointer_icon: winit::window::CursorIcon::Default,
        cursor_physical: [0.0; 2],
        text_input: text_input::TextInput::default(),
        modifiers: ModifiersState::empty(),
        ime_preedit: false,
        clipboard: arboard::Clipboard::new().ok(),
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

/// Top-level `winit` [`ApplicationHandler`] owning the OS window, EGL/GLX surface,
/// OpenGL context, [`ShellHost`], [`HostController`], [`PresenceController`], and [`AppState`].
struct StandaloneWindowApp {
    // Physical window coordinates are mapped once into the active interface plane.
    cursor_physical: [f64; 2],
    text_input: text_input::TextInput,
    modifiers: ModifiersState,
    ime_preedit: bool,
    clipboard: Option<arboard::Clipboard>,
    /// Parsed command-line options (`--width`, `--height`, `--script`, `--server`, etc.).
    opts: StandaloneOptions,
    /// Persistent desktop, `bfe-interface`, and player settings store.
    settings: AppSettingsStore,
    /// Thread-safe handle to `bfe_desktop::host::HostController` for local server hosting.
    host_controller: Arc<Mutex<HostController>>,
    /// Atomic audio callback health status.
    audio_status: Arc<AudioStatus>,
    /// Shared audio router switching cleanly between `ShellAudioMixer` and battle `Stream`.
    audio_router: Arc<SharedAudioRouter>,
    /// Active hardware `cpal` stream handle when audio output is enabled and available.
    _audio_device: Option<AudioDevice>,
    /// Pre-allocated stereo scratch buffer used to pump audio quantums when running headless.
    headless_scratch: Box<[f32; ::audio::FRAMES * 2]>,
    /// Active top-level state (`StartupLoader`, `Shell`, `HostStarting`, `BattleLoading`, `Battle`).
    state: AppState,
    /// Front-end shell and loading screen renderer (`bfe-shell` + `bfe-interface`).
    shell: Option<Box<ShellHost>>,
    /// Release the X11 device grab before dropping its borrowed window connection.
    #[cfg(target_os = "linux")]
    x11_grab: Option<x11_pointer::Grab>,
    /// UI releases its graphics resources before the shared context and surface.
    slint_ui: Option<slint_ui::SlintHost>,
    /// Native OS window handle.
    window: Option<Arc<Window>>,
    /// `glutin` window surface for buffer swaps.
    surface: Option<Rc<Surface<WindowSurface>>>,
    /// Current OpenGL / OpenGL ES context.
    context: Option<Rc<PossiblyCurrentContext>>,
    /// `glow` function pointer table bound to the current GL context.
    gl: Option<glow::Context>,
    /// Whether the OS window currently has input focus.
    focused: bool,
    /// Whether the cursor is currently locked and hidden for 3D combat mouselook.
    pointer_locked: bool,
    pointer_icon: winit::window::CursorIcon,
    /// Whether borderless fullscreen mode is currently active (`F11`).
    fullscreen: bool,
    /// Timestamp of the previous rendered frame for computing `dt`.
    last_frame: Instant,
    /// Discord Rich Presence controller (`bfe-desktop`).
    presence: PresenceController,
    /// Cached key of the last Rich Presence payload to avoid redundant IPC updates.
    last_presence_key: String,
    /// Optional automated `--script gate` driver.
    gate: Option<GateDriver>,
    platform_smoke: Option<platform_smoke::Smoke>,
    /// Fatal error message captured during frame stepping or GL initialization.
    error: Option<String>,
}

impl StandaloneWindowApp {
    fn interface_cursor(&self) -> [f64; 2] {
        let size = match &self.state {
            AppState::Battle(battle) => battle.size,
            _ => self
                .shell
                .as_ref()
                .map_or(self.opts.size, |shell| shell.layout_size),
        };
        [
            self.cursor_physical[0] * size[0] as f64 / self.opts.size[0].max(1) as f64,
            self.cursor_physical[1] * size[1] as f64 / self.opts.size[1].max(1) as f64,
        ]
    }

    fn text_field(&mut self) -> Option<(&'static str, &mut String, usize)> {
        match &mut self.state {
            AppState::Shell => {
                let shell = self.shell.as_mut()?;
                if shell.browser_page() && shell.page.browser.search {
                    return Some(("browser-search", &mut shell.page.browser.filters.name, 255));
                }
                let app = &mut shell.app;
                if app.popup.is_some() {
                    return None;
                }
                if app.profile.editing {
                    return Some(("profile", &mut app.profile.draft, 32));
                }
                if app.current_screen() == ScreenId::DirectConnect {
                    return if app.direct_field == 1 {
                        Some(("direct-port", &mut app.direct_port_draft, 5))
                    } else {
                        Some(("direct-address", &mut app.direct_connect_draft, 255))
                    };
                }
                if app.current_screen() == ScreenId::HostDialog {
                    return match app.host_form.focus {
                        0 => Some(("host-name", &mut app.host_config.server_name, 80)),
                        4 => Some(("host-players", &mut app.host_form.players, 2)),
                        _ => None,
                    };
                }
                None
            }
            AppState::Battle(battle) if battle.chat.is_open() => Some((
                "chat",
                &mut battle.chat.draft,
                bfe_battle_ui::chat::CHAT_LIMIT,
            )),
            _ => None,
        }
    }

    fn insert_text(&mut self, value: &str) {
        let mut editor = std::mem::take(&mut self.text_input);
        if let Some((field, text, limit)) = self.text_field() {
            editor.focus(Some(field), text);
            editor.insert(text, value, limit);
        } else {
            editor.focus(None, "");
        }
        self.text_input = editor;
        self.refresh_text_field();
    }

    fn refresh_text_field(&mut self) {
        if let Some(shell) = &mut self.shell {
            if shell.browser_page() && shell.page.browser.search {
                shell.refresh_page_search();
            }
            if shell.app.profile.editing {
                // The shared editor owns selection; Page only paints its state.
                let (cursor,anchor) = self.text_input.selection();
                let profile = &mut shell.app.profile;
                let cursor = cursor.min(profile.draft.len());
                profile.caret = profile.draft[..cursor].chars().count();
                profile.selected_all = cursor != anchor && cursor.max(anchor) == profile.draft.len() && cursor.min(anchor) == 0;
            }
            if shell.app.current_screen() == ScreenId::HostDialog {
                shell.app.rebuild_cached_rows();
            }
        }
    }

    fn edit_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        let Some(key) = map_shell_key(&event.logical_key) else {
            return false;
        };
        let command = self.modifiers.control_key() || self.modifiers.super_key();
        let shift = self.modifiers.shift_key();
        let paste = if command && key.eq_ignore_ascii_case("v") {
            self.clipboard
                .as_mut()
                .and_then(|clipboard| clipboard.get_text().ok())
        } else {
            None
        };
        let mut editor = std::mem::take(&mut self.text_input);
        let preedit = self.ime_preedit;
        let handled = if let Some((field, text, limit)) = self.text_field() {
            editor.focus(Some(field), text);
            if editor.key(text, &key, shift, command) {
                true
            } else if command && key.eq_ignore_ascii_case("v") {
                if let Some(value) = paste {
                    editor.insert(text, &value, limit);
                }
                true
            } else if !command && !preedit && !matches!(key.as_str(), "Enter" | "Escape" | "Tab") {
                if let Some(value) = &event.text {
                    editor.insert(text, value, limit);
                }
                // Up/down remain host row navigation. Printable keys never reach WASD.
                !matches!(key.as_str(), "ArrowUp" | "ArrowDown")
            } else {
                false
            }
        } else {
            editor.focus(None, "");
            false
        };
        self.text_input = editor;
        if handled {
            self.refresh_text_field();
        }
        handled
    }

    /// Synchronizes OS cursor grab (`Locked` / `Confined` vs `None`) and cursor visibility
    /// with the active state: locked during 3D combat when no battle UI overlay is open,
    /// and unlocked whenever the shell, spawn screen, scoreboard, or pause menu is active.
    ///
    /// ## Pointer Grab Rules
    /// - In `AppState::Battle`, pointer lock is requested if and only if `self.focused` is
    ///   `true` and `battle.wants_pointer_lock()` is `true` (all 2D overlays closed).
    /// - Attempts `CursorGrabMode::Locked` first (relative raw motion via `DeviceEvent::MouseMotion`)
    ///   and falls back to `CursorGrabMode::Confined` on window managers that disallow lock.
    /// - In all other states (`StartupLoader`, `Shell`, `HostStarting`, `BattleLoading`),
    ///   releases cursor grab and restores OS cursor visibility.
    fn sync_pointer_lock(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let icon = self.slint_ui.as_ref().and_then(|ui| ui.cursor()).unwrap_or_else(|| {
            match &self.state {
                AppState::Battle(battle) => battle.cursor_icon(),
                _ => winit::window::CursorIcon::Default,
            }
        });
        if icon != self.pointer_icon {
            window.set_cursor(icon);
            self.pointer_icon = icon;
        }
        let want_lock = match &self.state {
            AppState::Battle(battle) => self.focused && battle.wants_pointer_lock(),
            _ => false,
        };
        #[cfg(target_os = "linux")]
        if let Some(grab) = &self.x11_grab {
            if let Err(error) = grab.keep_inside(window, self.cursor_physical, self.opts.size) {
                eprintln!("cursor recenter failed: {error}");
            }
        }
        if want_lock == self.pointer_locked {
            return;
        }
        if want_lock {
            let locked = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
            #[cfg(target_os = "linux")]
            let locked = if locked {
                match x11_pointer::correct_grab(window) {
                    Ok(grab) => {
                        self.x11_grab = grab;
                        true
                    }
                    Err(error) => {
                        let _ = window.set_cursor_grab(CursorGrabMode::None);
                        eprintln!("cursor confinement failed: {error}");
                        false
                    }
                }
            } else {
                false
            };
            window.set_cursor_visible(!locked);
            self.pointer_locked = locked;
        } else {
            #[cfg(target_os = "linux")]
            self.x11_grab.take();
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
            self.pointer_locked = false;
        }
    }

    /// Toggles borderless fullscreen mode on the active window (`F11`).
    ///
    /// Flips `self.fullscreen` and applies `Fullscreen::Borderless(None)` (or `None` for
    /// windowed mode) on the active `winit::window::Window`.
    fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        if let Some(window) = &self.window {
            window.set_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
        }
    }

    /// Re-runs startup content resolution when the user selects a content or SWBF1 install
    /// folder (`F9` on the waiting-for-folder loader screen).
    ///
    /// Checks `BFE_CONTENT_SOURCE` or `settings.desktop.game_dir`, persists the chosen
    /// directory to `desktop-settings.json`, and re-enters `AppState::StartupLoader`.
    fn choose_content_folder(&mut self) {
        let picked = std::env::var("BFE_CONTENT_SOURCE")
            .ok()
            .map(PathBuf::from)
            .or_else(|| self.settings.desktop.game_dir.as_ref().map(PathBuf::from));
        if let Some(folder) = picked {
            self.settings.desktop.game_dir = Some(folder.to_string_lossy().to_string());
            let _ = self.settings.save_desktop();
            self.state = AppState::StartupLoader(StartupContentState::resolve(
                Some(&folder),
                &mut self.settings,
            ));
        }
    }

    /// Updates Discord Rich Presence (`PresenceController::set_activity`) whenever the
    /// high-level application phase, map, era, or multiplayer room changes.
    ///
    /// ## Activity Mapping
    /// - `StartupLoader` -> `phase = "preparing"`
    /// - `Shell` (`Multiplayer` screen) -> `phase = "lobby"`
    /// - `Shell` (all other screens) -> `phase = "menu"`
    /// - `HostStarting` -> `phase = "loading"`, `room_name = options.name`
    /// - `BattleLoading` -> `phase = "loading"`, `map_id = target.map_id()`
    /// - `Battle` (`InstantAction`) -> `phase = "match"`, `mode = "instant-action"`
    /// - `Battle` (`Online`) -> `phase = "match"`, `mode = "multiplayer"`, `party_size = 1`
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
            // Poll the background `"bfe-host-start"` worker thread (`rx.try_recv()`)
            // while rendering the animated planet loading screen (`draw_host_starting`).
            // Once `HostController::start` confirms `/admin` (`status == "running"`)
            // and `GET /info` on the game port, transition into `AppState::BattleLoading`
            // with `BattleLoadTarget::Online` so the local host player joins through
            // the normal online multiplayer client path (`BattleController::prepare_online`).
            AppState::HostStarting { options, .. } => RawPresenceInput {
                phase: Some("loading".to_string()),
                room_name: Some(options.name.clone()),
                ..RawPresenceInput::default()
            },
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

    /// Disconnects the live battle audio stream and re-installs a [`ShellAudioMixer`]
    /// when returning from a battle to the front-end shell.
    ///
    /// Detaches the battle `Stream` (`audio_router.set_live(None)`) and constructs a
    /// fresh [`ShellAudioMixer`] configured with the user's persisted `master_volume`,
    /// `music_volume`, and `interface_volume`.
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

    /// Handles window/application close requests, consulting `HostController::close_prompt()`
    /// first when a hosted server is active.
    ///
    /// ## Hosted Server Close-Prompt Flow
    /// 1. Queries `HostController::close_prompt()`, which returns `Some(ClosePrompt)` when
    ///    the local `bfeserver` child process is still running (`status == "running"`).
    /// 2. If active (and not running headless `--script gate`), transitions to `AppState::Shell`
    ///    and opens the 3-button `PopupState::HostClosePrompt` modal:
    ///    - `[0]` `"Keep server running and quit"` -> `CloseChoice::KeepRunning` (`detach()`)
    ///    - `[1]` `"Stop server and quit"` -> `CloseChoice::StopAndQuit` (`stop()`)
    ///    - `[2]` `"Cancel"` -> `CloseChoice::Cancel` (remain in app)
    /// 3. If no hosted server is running, writes any `--script gate` summary report and
    ///    calls `event_loop.exit()`.
    fn request_app_close(&mut self, event_loop: &ActiveEventLoop) {
        // When not running an automated gate script, check if `HostController` has a running
        // hosted server that requires the 3-choice close prompt (`KeepRunning`, `StopAndQuit`, `Cancel`).
        if self.gate.is_none() {
            let maybe_prompt = self
                .host_controller
                .lock()
                .ok()
                .and_then(|hc| hc.close_prompt());
            if let Some(prompt) = maybe_prompt {
                let content_dir = self
                    .shell
                    .as_ref()
                    .and_then(|s| s.content_dir.clone())
                    .unwrap_or_else(|| self.settings.paths.content_root.clone());
                if !matches!(self.state, AppState::Shell) {
                    self.restore_shell_audio(&content_dir);
                    self.state = AppState::Shell;
                }
                if let Some(shell) = &mut self.shell {
                    shell.show_host_close_prompt(&prompt);
                }
                return;
            }
        }
        if let Some(gate) = &self.gate {
            let _ = gate.write_summary_report();
        }
        event_loop.exit();
    }

    /// Blits the offscreen FBO (`src_fb`) of the active shell or battle pass into the
    /// default window framebuffer (`0`) and swaps buffers.
    ///
    /// Uses `glBlitFramebuffer` with `GL_LINEAR` filtering from `READ_FRAMEBUFFER = src_fb`
    /// to `DRAW_FRAMEBUFFER = 0` (the default window surface) and immediately swaps the
    /// window backbuffer via `glutin::surface::Surface::swap_buffers`.
    fn blit_framebuffer_to_window(&self, src_fb: glow::Framebuffer, src_size: [i32; 2]) -> Result<(), String> {
        let (Some(ui), Some(surface), Some(context)) = (&self.slint_ui, &self.surface, &self.context)
        else {
            return Err("window presentation resources missing".into());
        };
        // The Slint notifier copies this frame after its transparent clear and
        // before its UI draw. Only this host swaps the shared window surface.
        ui.render(src_fb, src_size)?;
        surface.swap_buffers(context).map_err(|e| format!("window presentation failed: {e}"))
    }

    /// Polls `glGetError()` after each frame and increments `gate.gl_errors` if non-zero.
    ///
    /// Invoked at the end of every rendered frame in [`Self::step_frame`]. Any non-zero
    /// `glGetError()` code increments `gate.gl_errors` so `--script gate` can assert
    /// `glErrors == 0` across all 24 shell and battle phases.
    fn check_gl_errors(&mut self) {
        if let Some(gl) = &self.gl {
            let err = unsafe { gl.get_error() };
            if err != glow::NO_ERROR {
                if let Some(gate) = &mut self.gate {
                    gate.gl_errors += 1;
                }
                if self.platform_smoke.is_some() {
                    self.error = Some(format!("platform smoke GL error {err}"));
                }
            }
        }
    }
}

impl ApplicationHandler for StandaloneWindowApp {
    /// Initializes the OS window, GL surface/context, and [`ShellHost`] on first resume.
    ///
    /// ## GL Context Creation Strategy
    /// 1. Builds the native OS window at `self.opts.size` (`1280x720` default) and
    ///    selects a single-sample `RGBA8 + D24S8` `glutin` surface configuration.
    /// 2. Requests an OpenGL ES 3.0 context by default (matching `bfe-render`'s `#version 300 es`
    ///    shaders), falling back transparently to Desktop OpenGL 3.3 Core if GLES is unavailable.
    /// 3. Sets `SwapInterval::DontWait` so the 60 Hz simulation and gate script never
    ///    stall on compositor vsync throttling.
    /// 4. Constructs [`ShellHost`] with the built-in 5x7 ASCII fallback font so frame 1
    ///    renders the startup loader immediately.
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

            // Request an 8-bit RGBA, 24-bit depth, 8-bit stencil single-sample config.
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

            // Prefer OpenGL ES 3.0 by default (matching WebGL2 shaders), falling back to OpenGL 3.3.
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
            let physical = window.inner_size();
            self.opts.size = [physical.width as i32, physical.height as i32];
            let shell = Box::new(ShellHost::new(
                self.opts.size,
                &self.settings,
                self.opts.server_target.clone(),
                make_gl,
            )?);

            window.set_ime_allowed(true);
            let window = Arc::new(window);
            let context = Rc::new(context);
            let surface = Rc::new(surface);
            self.slint_ui = Some(slint_ui::SlintHost::new(
                window.clone(),
                context.clone(),
                surface.clone(),
                self.opts.script.as_deref() == Some("slint-proof"),
            )?);
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

    /// Routes raw hardware mouse motion deltas into `battle.input` while pointer-locked.
    ///
    /// Forwards raw hardware mouse motion deltas `(dx, dy)` to `battle.input.motion(dx, dy)`
    /// when `self.pointer_locked` and `self.focused` are both `true`.
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
                    if self.opts.script.as_deref() == Some("slint-proof") {
                        eprintln!("[input] locked raw motion {dx},{dy}");
                    }
                }
            }
        }
    }

    /// Routes window lifecycle, focus, resize, pointer, and keyboard events to the active state.
    ///
    /// ## Event Routing Table
    /// - `WindowEvent::CloseRequested`: Invokes [`Self::request_app_close`].
    /// - `WindowEvent::Focused`: Updates `self.focused`, auto-opens `PauseMenu` when
    ///   focus is lost during active 3D combat, and updates pointer lock.
    /// - `WindowEvent::Resized`: Resizes the `glutin` window surface (`NonZeroU32`).
    /// - `WindowEvent::CursorMoved`: Routes pointer hover to `ShellHost` (including
    ///   `bfe-interface` hit regions) or `BattleController` overlays.
    /// - `WindowEvent::MouseInput`: Routes clicks to `ShellHost`, `BattleController`,
    ///   or the folder-picker fallback.
    /// - `WindowEvent::MouseWheel`: Scrolls `SpawnScreen`, `Scoreboard`, or weapon cycle.
    /// - `WindowEvent::KeyboardInput`: Handles `F11` (fullscreen) and `F9` (content folder),
    ///   then forwards logical keys to `ShellHost` or physical DOM codes to `BattleController`.
    /// - `WindowEvent::RedrawRequested`: Steps and draws one frame via [`Self::step_frame`].
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.opts.script.as_deref() == Some("slint-proof") {
            match &event {
                WindowEvent::KeyboardInput { event, .. } => eprintln!(
                    "[input] physical={:?} logical={:?} state={:?}",
                    event.physical_key, event.logical_key, event.state
                ),
                WindowEvent::MouseInput { button, state, .. } => {
                    eprintln!("[input] button={button:?} state={state:?}");
                }
                WindowEvent::MouseWheel { delta, .. } => eprintln!("[input] wheel={delta:?}"),
                _ => {}
            }
        }
        let consumed = self.slint_ui.as_mut()
            .is_some_and(|ui| ui.event(&event, self.pointer_locked));
        if consumed {
            self.sync_pointer_lock();
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                // Consult `HostController::close_prompt()` before exiting the event loop.
                self.request_app_close(event_loop);
            }
            WindowEvent::Focused(focused) => {
                // Keep focus active during headless Xorg `--script gate` runs; otherwise
                // clear held combat inputs and open the pause menu on focus loss.
                let focused = if self.gate.is_some() || self.platform_smoke.is_some() {
                    true
                } else {
                    focused
                };
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
                // Resize the EGL/GLX window surface when physical dimensions change.
                if phys.width > 0 && phys.height > 0 {
                    let new_size = [phys.width as i32, phys.height as i32];
                    self.opts.size = new_size;
                    if let (Some(surface), Some(context)) = (&self.surface, &self.context) {
                        if let (Some(w), Some(h)) =
                            (NonZeroU32::new(phys.width), NonZeroU32::new(phys.height))
                        {
                            surface.resize(context, w, h);
                        }
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // Winit already expresses inner_size and CursorMoved in physical pixels.
                // Query the current extent after a monitor scale change; never divide
                // the pointer alone by scale_factor (that would apply scaling twice).
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    self.opts.size = [size.width.max(1) as i32, size.height.max(1) as i32];
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Ime(Ime::Preedit(text, _)) => self.ime_preedit = !text.is_empty(),
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.ime_preedit = false;
                self.insert_text(&text);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_physical = [position.x, position.y];
                let [x, y] = self.interface_cursor();
                // Forward cursor coordinates to `ShellHost` (including `bfe-interface` hit regions)
                // or to the active `BattleController` overlays.
                match &mut self.state {
                    AppState::Shell => {
                        if let Some(shell) = &mut self.shell {
                            shell.handle_cursor_move(x, y);
                        }
                    }
                    AppState::Battle(battle) => {
                        battle.handle_cursor_move(x, y);
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                let [x, y] = self.interface_cursor();
                if down && matches!(&self.state, AppState::Shell)
                    && std::env::var_os("BFE_MENU_INPUT_TRACE").is_some()
                {
                    eprintln!("[menu pointer] screen={:?} window={:?} framebuffer={:?} physical={:?} mapped={:?} scale={}",
                        self.shell.as_ref().map(|shell| shell.app.current_screen()),
                        self.opts.size, self.shell.as_ref().map(|shell| shell.size),
                        self.cursor_physical, [x,y], self.window.as_ref().map_or(1., |window| window.scale_factor()));
                }
                match &mut self.state {
                    AppState::StartupLoader(StartupContentState::WaitingForFolder { .. }) => {
                        if down && button == MouseButton::Left {
                            self.choose_content_folder();
                        }
                    }
                    AppState::Shell => {
                        if down {
                            let code = match button {
                                MouseButton::Left => Some(0),
                                MouseButton::Middle => Some(1),
                                MouseButton::Right => Some(2),
                                MouseButton::Back => Some(3),
                                MouseButton::Forward => Some(4),
                                _ => None,
                            };
                            if let (Some(b), Some(shell)) = (code, &mut self.shell) {
                                let transition = shell.handle_mouse_button(
                                    b,
                                    x,
                                    y,
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
                            MouseButton::Back => Some(3),
                            MouseButton::Forward => Some(4),
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
                let [x,y] = self.interface_cursor();
                // Convert line or pixel scroll deltas into scoreboard / spawn scroll offsets.
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as f64 * 100.0,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                match &mut self.state {
                    AppState::Battle(battle)=>battle.handle_wheel(dy),
                    AppState::Shell=>{if let Some(shell)=&mut self.shell{shell.handle_wheel(x,y,dy,&mut self.settings);}},
                    _=>{},
                }
            }
            WindowEvent::KeyboardInput { event, is_synthetic, .. } => {
                let down = event.state == ElementState::Pressed;
                let key_capture_active = matches!(self.state, AppState::Shell)
                    && self
                        .shell
                        .as_ref()
                        .is_some_and(|s| s.is_key_capture_active());
                // Handle global function hotkeys (`F11` fullscreen toggle, `F9` folder picker).
                // When `KeyCaptureModal` is open, `F9` is a bindable key and must reach the modal.
                if down && !event.repeat {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        match code {
                            KeyCode::F11 => {
                                self.toggle_fullscreen();
                                return;
                            }
                            KeyCode::F9 if !key_capture_active && !matches!(&self.state, AppState::Battle(_)) => {
                                self.choose_content_folder();
                                return;
                            }
                            _ => {}
                        }
                    }
                }

                // Editing consumes navigation/deletion before menu or combat bindings.
                // KeyEvent.text handles layout/compose output; IME commits arrive separately.
                if down && self.edit_key(&event) {
                    return;
                }

                match &mut self.state {
                    AppState::StartupLoader(StartupContentState::WaitingForFolder { .. }) => {
                        if down && !event.repeat {
                            self.choose_content_folder();
                        }
                    }
                    AppState::Shell => {
                        if down && !is_synthetic {
                            let shift = self.modifiers.shift_key();
                            let options_active = self
                                .shell
                                .as_ref()
                                .is_some_and(|s| s.is_interface_options_active());
                            if key_capture_active && event.repeat {
                                return;
                            }
                            let key_str = if options_active {
                                match event.physical_key {
                                    PhysicalKey::Code(code) => keycode_to_dom_code(code)
                                        .map(str::to_string)
                                        .or_else(|| map_shell_key(&event.logical_key)),
                                    _ => map_shell_key(&event.logical_key),
                                }
                            } else {
                                map_shell_key(&event.logical_key)
                            };
                            if let (Some(key_str), Some(shell)) = (key_str, &mut self.shell) {
                                let transition = shell.handle_key_with_shift(
                                    &key_str,
                                    shift,
                                    &mut self.settings,
                                    &self.audio_router,
                                );
                                self.apply_shell_transition(transition, event_loop);
                            }
                        }
                    }
                    AppState::Battle(battle) => {
                        battle.set_modifiers(
                            self.modifiers.shift_key() || self.modifiers.control_key(),
                            self.modifiers.control_key()
                                || self.modifiers.alt_key()
                                || self.modifiers.super_key(),
                        );
                        if let PhysicalKey::Code(code) = event.physical_key {
                            if let Some(dom_code) = keycode_to_dom_code(code) {
                                battle.handle_key(dom_code, down, event.repeat, &mut self.settings);
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

    /// Requests a continuous redraw on every event loop iteration (`ControlFlow::Poll`).
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl StandaloneWindowApp {
    /// Applies a [`ShellTransition`] emitted by [`ShellHost`] (`StartInstantAction`,
    /// `StartOnline`, `StartHost`, `HostCloseDecision`, or `Exit`).
    ///
    /// ## Transition Handling
    /// - `StartInstantAction(req)`: Prepares map loading screen (`ShellHost::prepare_map_loading`)
    ///   and enters `AppState::BattleLoading` for `playlist_index = 0`.
    /// - `StartOnline(launch)`: Prepares map loading screen and enters `AppState::BattleLoading`
    ///   for `BattleLoadTarget::Online`.
    /// - `StartHost(options, launch)`: Synchronizes `content_root` and `online_consent` on
    ///   `HostController`, spawns the `"bfe-host-start"` background thread to execute
    ///   `HostController::start(&options)`, and enters `AppState::HostStarting` until `/info` answers.
    /// - `HostCloseDecision(idx)`: Converts button index `0..=2` via `ClosePrompt::choice_for_index`
    ///   and calls `HostController::apply_close_choice`, exiting when `Ok(true)`.
    /// - `Exit`: Invokes [`Self::request_app_close`].
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
                // Configure the planet loading screen and transition to `AppState::BattleLoading`.
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
                    work: Box::default(),
                };
            }
            ShellTransition::StartOnline(launch) => {
                // Configure the planet loading screen for the selected online server and transition.
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
                    work: Box::default(),
                };
            }
            ShellTransition::StartHost(options, launch) => {
                // Synchronize `content_root` and `online_consent` on `HostController`, then spawn
                // `"bfe-host-start"` to run `HostController::start(&options)` until `/info` answers.
                let content_dir = self
                    .shell
                    .as_ref()
                    .and_then(|s| s.content_dir.clone())
                    .unwrap_or_else(|| self.settings.paths.content_root.clone());
                let consent = self
                    .shell
                    .as_ref()
                    .map(|s| !s.app.mp_needs_notice)
                    .unwrap_or(self.settings.player.privacy_notice_accepted);
                if let Ok(mut hc) = self.host_controller.lock() {
                    hc.set_content_root(content_dir);
                    hc.set_online_consent(consent);
                }
                let hc_arc = self.host_controller.clone();
                let opts_for_thread = options.clone();
                let (tx, rx) = mpsc::channel();
                let _ = std::thread::Builder::new()
                    .name("bfe-host-start".to_string())
                    .spawn(move || {
                        let res = hc_arc
                            .lock()
                            .map_err(|e| format!("host controller lock poisoned: {e}"))
                            .and_then(|hc| hc.start(&opts_for_thread).map_err(|e| e.to_string()));
                        let _ = tx.send(res);
                    });
                self.state = AppState::HostStarting {
                    options,
                    launch,
                    rx,
                    frames_drawn: 0,
                };
            }
            ShellTransition::HostCloseDecision(index) => {
                // Map the 3-button close dialog index (`0` = KeepRunning, `1` = StopAndQuit, `2` = Cancel)
                // via `ClosePrompt::choice_for_index` and apply it on `HostController`.
                let choice = ClosePrompt::new().choice_for_index(index);
                let decision = self
                    .host_controller
                    .lock()
                    .map_err(|e| format!("host controller lock poisoned: {e}"))
                    .and_then(|hc| hc.apply_close_choice(choice).map_err(|e| e.to_string()));
                match decision {
                    Ok(true) => {
                        if let Some(gate) = &self.gate {
                            let _ = gate.write_summary_report();
                        }
                        event_loop.exit();
                    }
                    Ok(false) => {}
                    Err(err) => {
                        if let Some(shell) = &mut self.shell {
                            shell.app.popup = Some(PopupState::Notice {
                                message: format!("HOST STOP ERROR: {err}"),
                            });
                        }
                    }
                }
            }
            ShellTransition::Exit => {
                self.request_app_close(event_loop);
            }
        }
    }

    /// Advances the active [`AppState`] by one frame, renders into the offscreen FBO,
    /// blits to the window surface, and checks for OpenGL errors.
    ///
    /// ## Per-Frame Pipeline
    /// 1. Computes clamped frame delta `dt` (`0.001..=0.1` seconds).
    /// 2. Pumps 6 stereo 128-frame audio quantums (`pump_headless_audio`) when running
    ///    without a hardware `cpal` output stream.
    /// 3. Updates Discord Rich Presence (`update_rich_presence`) and OS cursor grab (`sync_pointer_lock`).
    /// 4. Dispatches the active [`AppState`] variant (`StartupLoader`, `Shell`, `HostStarting`,
    ///    `BattleLoading`, or `Battle`), blitting the resulting FBO to the window.
    /// 5. Polls `glGetError()` via [`Self::check_gl_errors`] to enforce the zero-GL-error gate.
    fn step_frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().clamp(0.001, 0.1);
        self.last_frame = now;

        // If no hardware `cpal` audio device is open, pump headless audio quantums so
        // `ShellAudioMixer` and battle `Stream` queues drain identically to hardware playback.
        if self._audio_device.is_none() {
            pump_headless_audio(
                &self.audio_router,
                &self.audio_status,
                &mut self.headless_scratch,
            );
        }

        self.update_rich_presence();
        if let Some(ui) = &self.slint_ui {
            ui.update(matches!(&self.state, AppState::Battle(battle) if battle.wants_pointer_lock()));
            if ui.wants_quit() {
                self.request_app_close(event_loop);
            }
        }
        self.sync_pointer_lock();
        // FemtoVG shares the context and changes its state between game frames.
        if let Some(shell) = &mut self.shell {
            shell.core.state.reset();
        }
        if let AppState::Battle(battle) = &mut self.state {
            battle.scene.bridge.r.core.state.reset();
        }

        let frame_res: Result<(), String> = (|| {
            if let (Some(shell), Some(context), Some(window)) =
                (&mut self.shell, &self.context, &self.window)
            {
                let display = context.display();
                shell.resize(self.opts.size, window.scale_factor(), || unsafe {
                    glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name))
                })?;
            }
            if let (AppState::Battle(battle), Some(window)) = (&mut self.state, &self.window) {
                battle.page_size = battle.size.map(|v| {
                    (v as f64 / window.scale_factor()).round().max(1.) as i32
                });
            }
            match &mut self.state {
                AppState::StartupLoader(loader_state) => {
                    // Poll background `PrepareSession` progress and render the loader screen.
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
                        self.blit_framebuffer_to_window(fb, sz)?;
                    }
                    // Once content is ready, load `common.json` / `shell.json` into `ShellHost`
                    // and transition into `AppState::Shell`.
                    if let Some(content_dir) = ready_dir {
                        if let Ok(mut hc) = self.host_controller.lock() {
                            hc.set_content_root(content_dir.clone());
                            hc.set_online_consent(self.settings.player.privacy_notice_accepted);
                        }
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
                            if let Some(smoke) = &mut self.platform_smoke {
                                smoke.capture("startup", shell.size, &shell.pixels()?)?;
                                smoke.startup = true;
                            }
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
                    // Render the active shell or `bfe-interface` options screen and step `--script gate`.
                    let mut transition = if let Some(shell) = &mut self.shell {
                        let t = shell.step_and_draw(dt, &mut self.settings, &self.audio_router);
                        let fb = shell.canvas.framebuffer;
                        let sz = shell.size;
                        self.blit_framebuffer_to_window(fb, sz)?;
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
                    if let (Some(smoke), Some(shell), Some(context)) =
                        (&mut self.platform_smoke, &mut self.shell, &self.context)
                    {
                        if !smoke.startup {
                            return Err("platform smoke missed startup rendering".into());
                        }
                        let display = context.display();
                        if let Some(scene) = smoke.advance(
                            shell,
                            &mut self.settings,
                            &self.audio_router,
                            || unsafe {
                                glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name))
                            },
                        )? {
                            let ui = self.slint_ui.as_mut().ok_or("Slint host missing")?;
                            let input = ui.prove_input()?;
                            ui.render(scene.canvas.framebuffer, scene.size)?;
                            smoke.record_ui(scene.size, &ui.pixels(), input)?;
                            self.blit_framebuffer_to_window(scene.canvas.framebuffer, scene.size)?;
                        }
                    }
                }
                AppState::HostStarting {
                    options,
                    launch,
                    rx,
                    frames_drawn,
                } => {
                    // Render progress while `HostController::start` waits for `/admin` and `/info`.
                    *frames_drawn += 1;
                    let server_name = options.name.clone();
                    let launch_clone = launch.clone();
                    let poll_res = rx.try_recv();
                    if let Some(shell) = &mut self.shell {
                        shell.draw_host_starting(dt, &server_name);
                        let fb = shell.canvas.framebuffer;
                        let sz = shell.size;
                        self.blit_framebuffer_to_window(fb, sz)?;
                    }
                    match poll_res {
                        Ok(Ok(join_info)) => {
                            // `/info` answered! Join the local player through the normal online path.
                            let mut join_launch = launch_clone;
                            join_launch.address = Some(join_info.address.clone());
                            join_launch.port = Some(join_info.port);
                            join_launch.server_id =
                                format!("{}:{}", join_info.address, join_info.port);
                            if let Some(shell) = &mut self.shell {
                                let map_id = if join_launch.map.is_empty() {
                                    "nab2"
                                } else {
                                    join_launch.map.as_str()
                                };
                                shell.prepare_map_loading(map_id, &self.audio_router);
                            }
                            self.state = AppState::BattleLoading {
                                target: BattleLoadTarget::Online {
                                    launch: join_launch,
                                },
                                frames_drawn: 0,
                                work: Box::default(),
                            };
                        }
                        Ok(Err(err)) => {
                            // Surface any startup/readiness error on `ScreenId::HostDialog`.
                            if let Some(shell) = &mut self.shell {
                                shell.app.push_screen(ScreenId::HostDialog);
                                shell.app.popup = Some(PopupState::Notice {
                                    message: format!("HOST ERROR: {err}"),
                                });
                            }
                            self.state = AppState::Shell;
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            if let Some(shell) = &mut self.shell {
                                shell.app.push_screen(ScreenId::HostDialog);
                                shell.app.popup = Some(PopupState::Notice {
                                    message: "HOST ERROR: host start worker disconnected"
                                        .to_string(),
                                });
                            }
                            self.state = AppState::Shell;
                        }
                    }
                }
                AppState::BattleLoading {
                    target,
                    frames_drawn,
                    work,
                } => {
                    *frames_drawn += 1;
                    let error_screen = if matches!(target, BattleLoadTarget::Online { .. }) {
                        ScreenId::Multiplayer
                    } else {
                        ScreenId::InstantAction
                    };
                    let root = self
                        .shell
                        .as_ref()
                        .and_then(|s| s.content_dir.clone())
                        .unwrap_or_else(|| self.settings.paths.content_root.clone());
                    let display = self.context.as_ref().ok_or("missing GL context")?.display();
                    let result = work.advance(
                        &root,
                        target,
                        self.opts.size,
                        &self.settings,
                        Arc::clone(&self.audio_router),
                        &self.opts.server_target,
                        move || unsafe {
                            glow::Context::from_loader_function_cstr(|name| {
                                display.get_proc_address(name)
                            })
                        },
                    );
                    let mut handover = None;
                    let frame = if let Some(shell) = &mut self.shell {
                        shell.draw_battle_loading(dt);
                        if *frames_drawn == 6 {
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
                        handover =
                            work.handover(shell.battle_loading_complete(), &self.audio_router);
                        if handover.is_some() {
                            eprintln!(
                                "battle loading: frames={} peakFrameMs={:.3} elapsed={:.3}s",
                                shell.loading_frames, shell.loading_peak_ms, shell.loading_time
                            );
                        }
                        Some((shell.canvas.framebuffer, shell.size))
                    } else {
                        None
                    };
                    if let Some((fb, size)) = frame {
                        self.blit_framebuffer_to_window(fb, size)?;
                    }
                    if let Err(error) = result {
                        if self.gate.is_some() {
                            return Err(format!("battle admission failed: {error}"));
                        }
                        self.restore_shell_audio(&root);
                        if let Some(shell) = &mut self.shell {
                            shell.app.push_screen(error_screen);
                            shell.app.popup = Some(PopupState::Notice {
                                message: format!("ADMISSION FAILED: {error}"),
                            });
                        }
                        self.state = AppState::Shell;
                    } else if let Some(battle) = handover {
                        eprintln!(
                            "battle loading: timeline and admission complete; entering battle"
                        );
                        self.state = AppState::Battle(battle);
                        // The pointer may not move during admission; initialize the new
                        // overlay's hover and cursor icon from its actual window position.
                        let [x, y] = self.interface_cursor();
                        if let AppState::Battle(battle) = &mut self.state {
                            battle.handle_cursor_move(x, y);
                        }
                    }
                }
                AppState::Battle(battle) => {
                    // Step `--script gate` combat inputs, render the 3D battle + HUD overlays,
                    // and handle post-battle playlist progression or pause menu exits.
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
                    self.blit_framebuffer_to_window(fb, sz)?;

                    if let Some(t) = transition {
                        let content_dir = self
                            .shell
                            .as_ref()
                            .and_then(|s| s.content_dir.clone())
                            .unwrap_or_else(|| self.settings.paths.content_root.clone());
                        match t {
                            // When the player activates `DONE` on the post-match scoreboard (`done_triggered`),
                            // advance to `next_index` in `request.playlist` if another mission remains,
                            // or return to `ScreenId::InstantAction` when the playlist finishes.
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
                                        work: Box::default(),
                                    };
                                } else {
                                    self.restore_shell_audio(&content_dir);
                                    if let Some(shell) = &mut self.shell {
                                        shell.app.push_screen(ScreenId::InstantAction);
                                    }
                                    self.state = AppState::Shell;
                                }
                            }
                            // When the player confirms `RESTART MISSION` in the Escape pause menu,
                            // re-enter `AppState::BattleLoading` for the same `playlist_index` so
                            // `BattleController::prepare_instant_action` re-admits the mission from tick 0.
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
                                    work: Box::default(),
                                };
                            }
                            // Restore front-end shell audio, reset the screen stack to `ScreenId::Main`,
                            // and transition back to `AppState::Shell`.
                            BattleTransition::QuitToMainMenu => {
                                self.restore_shell_audio(&content_dir);
                                if let Some(shell) = &mut self.shell {
                                    shell.app.settings.menu_theme=self.settings.player.shell.menu_theme.clone();
                                    shell.app.stack = vec![ScreenId::Main];
                                    shell.app.rebuild_cached_rows();
                                }
                                self.state = AppState::Shell;
                            }
                            BattleTransition::NextOnlineBattle { launch } => {
                                if let Some(shell) = &mut self.shell {
                                    shell.prepare_map_loading(&launch.map, &self.audio_router);
                                }
                                self.state = AppState::BattleLoading {
                                    target: BattleLoadTarget::Online { launch },
                                    frames_drawn: 0,
                                    work: Box::default(),
                                };
                            }
                            // Restore front-end shell audio, return to `ScreenId::Multiplayer`, and
                            // display a `DISCONNECTED` notice popup if the session ended with a network fault.
                            BattleTransition::LeaveOnline { reason } => {
                                if self.gate.is_some() {
                                    if let Some(message) = reason.as_ref() {
                                        return Err(format!(
                                            "Online battle failed during gate: {message}"
                                        ));
                                    }
                                }
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
                                self.request_app_close(event_loop);
                            }
                        }
                    }
                }
            }
            Ok(())
        })();

        self.check_gl_errors();
        if self.error.is_some() || self.platform_smoke.as_ref().is_some_and(|s| s.done) {
            event_loop.exit();
        }

        if let Err(err) = frame_res {
            self.error = Some(err);
            event_loop.exit();
        }
    }
}

/// Maps a `winit` logical [`Key`] to the string key identifier expected by `bfe-shell`
/// and `bfe-interface` (`"ArrowUp"`, `"Enter"`, `"Escape"`, or printable character).
///
/// ## Key Mapping Rules
/// - Navigation keys (`ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `Enter`,
///   `Escape`, `Backspace`, `Delete`, `Tab`) map to their canonical key names.
/// - `NamedKey::Space` maps to `" "`.
/// - Printable characters (`Key::Character`) map to their first non-control Unicode scalar
///   for profile name entry and hotkey rebinding.
fn map_shell_key(key: &Key) -> Option<String> {
    match key {
        Key::Named(NamedKey::ArrowUp) => Some("ArrowUp".to_string()),
        Key::Named(NamedKey::ArrowDown) => Some("ArrowDown".to_string()),
        Key::Named(NamedKey::ArrowLeft) => Some("ArrowLeft".to_string()),
        Key::Named(NamedKey::ArrowRight) => Some("ArrowRight".to_string()),
        Key::Named(NamedKey::Enter) => Some("Enter".to_string()),
        Key::Named(NamedKey::Escape) => Some("Escape".to_string()),
        Key::Named(NamedKey::Backspace) => Some("Backspace".to_string()),
        Key::Named(NamedKey::Home) => Some("Home".to_string()),
        Key::Named(NamedKey::End) => Some("End".to_string()),
        Key::Named(NamedKey::PageUp) => Some("PageUp".to_string()),
        Key::Named(NamedKey::PageDown) => Some("PageDown".to_string()),
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

/// Maps a physical [`KeyCode`] to its DOM `KeyboardEvent.code` string (`"KeyW"`, `"Space"`,
/// `"ArrowUp"`, etc.) for `BattleController::handle_key` and `bfe-battle-ui`.
///
/// ## Supported Physical Key Codes
/// - Movement & posture: `KeyW`, `KeyA`, `KeyS`, `KeyD`, `Space`, `KeyC`, `KeyX`, `KeyZ`
/// - Weapons & interaction: `KeyE`, `KeyR`, `KeyF`, `KeyQ`, `KeyV`, `Digit1..Digit4`
/// - Overlays & chat: `KeyM` (Spawn), `Tab` (Scoreboard), `Escape` (Pause), `KeyT`/`KeyY` (Chat)
/// - Navigation & camera: `ArrowUp..ArrowRight`, `PageUp`, `PageDown`, `Home`, `End`, `F1..F4`
/// - Modifiers: `ShiftLeft`, `ShiftRight`, `ControlLeft`, `ControlRight`
fn keycode_to_dom_code(code: KeyCode) -> Option<&'static str> {
    crate::input::keycode_to_dom_code(code)
}
