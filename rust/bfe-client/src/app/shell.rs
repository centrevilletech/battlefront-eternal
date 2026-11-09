//! Native front-end host over the existing shell and options screen stacks.
//!
//! ShellApp owns front-end actions; OptionsController owns the option widgets,
//! slider interaction, key capture and persistence. The page renderer paints
//! their state and supplies viewport hit regions without duplicating actions.
//!
//! Licensed Saira and look surfaces are admitted once with the GL resources.
//! Page receives current media texture outputs from the host each frame.
//! Loading screens retain their separate timeline.
//!
//! The host also owns server-browser polling, audio routing and the shell's
//! framebuffer. Network discovery and content preparation stay off the UI thread.

use crate::{
    app::{
        settings::AppSettingsStore,
        startup::{build_fallback_ascii_font, StartupContentState},
    },
    audio::{SharedAudioRouter, Status as AudioStatus},
    canvas::Canvas,
    resources::Ranges,
};
use bfe_content_native::textures::Library;
use bfe_desktop::host::{
    AiDifficulty, ClosePrompt, HostMode, HostOptions, RelayPolicy, TeamAssignment,
};
use bfe_interface::{
    HitRegion, OptionsContext, OptionsController, Surface as InterfaceSurfaceState,
};
use bfe_online::{
    default_identity_path, discover_lan, ping_host_port, resolve_host_port, resolve_master_url,
    BattleSummary, BrowserKey, BrowserState, FilterUpdate, Filters, GameInfo, LanDiscoveryOptions,
    MapCatalogEntry, MasterClient, PlayerIdentity, RefreshKind, RotationItem, ServerBrowser,
    ServerBrowserService, ServerEntry, SortColumn, SortSpec, Source, DIRECT_INFO_TIMEOUT, LAN_PORT,
    PING_TIMEOUT,
};
use bfe_render::{
    core::Core,
    hud::{
        HudPass, GROUP, IN_CAMERA, IN_FIRST_GROUP, IN_GROUPS, IN_PROGRAM, IN_SLOTS, IN_UPLOAD,
        IN_VIEWPORT, MAX_GROUPS, QUADS,
    },
    interface::{Batch, Font, Glyph},
    post::UNIFORM,
};
use bfe_shell::{
    audio::ShellAudioMixer,
    data::{load_shell_data_from_dir, ShellData},
    launch::{BattleRequest, PlaylistEntry},
    loading::{planet_name, LoadingTimeline, LookId},
    render::{draw_loading_with_typeface, draw_shell, ShellFonts, ShellTextures, StageTexture},
    screens::{
        HostConfig, IaFocus, MultiplayerKind, MultiplayerLaunch, OptionsPageId, PopupState,
        ScreenId, ShellAction, ShellApp,
    },
    version::{AboutInfo, DEFAULT_SIMULATION},
};
use glow::HasContext;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use three::{CoordinateSystem, Matrix4};

/// Named UI chrome, backdrop, faction icon, and loading screen textures uploaded
/// from `common/common.json` and `shell/shell.json` into both [`ShellTextures`]
/// and [`bfe_interface::Surface`].
///
/// ## Texture Categories
/// - **Era & Mode Backdrops**: `atat_screen`, `atte_screen`, `coreship_screen`,
///   `hoth_screen`, `generic2_screen`, `generic3_screen`, `gc_screen`, `his_brief_bg`,
///   `iface_bgmeta_space`, `main`.
/// - **3-Piece Capsules & Borders**: `buttonleft`, `buttonmid`, `buttonright`,
///   `headerbuttonleft`, `headerbuttonmid`, `headerbuttonright`, `border_3_pieces`,
///   `border_3_pieces_Selected`.
/// - **Listboxes & Sliders**: `listbox_cursor`, `listbox_hilight`, `slider_bg`,
///   `slider_fg`, `slider_sound`.
/// - **Faction Emblems**: `rep_icon`, `cis_icon`, `imp_icon`, `all_icon`.
/// - **Loading Screen Widgets**: `load_rotating_disk`, `load_led_0..2`,
///   `load_edge_horz`, `load_edge_vert`, `load_edge_corner`, `map_unk`.
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

/// Converts [`ShellTextures`] and viewport dimensions into [`bfe_interface::Surface`]
/// so `ClassicTheme::paint_stack` uses the exact same GL texture handles as `draw_shell`.
fn shell_textures_to_interface_surface(
    textures: &ShellTextures,
    size: [i32; 2],
) -> InterfaceSurfaceState {
    let mut surf = InterfaceSurfaceState::from_viewport(size[0] as f64, size[1] as f64);
    surf.solid = textures.solid;
    for (k, &v) in &textures.by_name {
        surf.set_texture(k.clone(), v);
    }
    surf
}

/// Converts [`HostConfig`] (`SET GAME OPTIONS` on `ScreenId::HostDialog`) and the
/// current Instant Action playlist into [`bfe_desktop::host::HostOptions`].
///
/// ## Field Mapping (`HostConfig` -> `HostOptions`)
/// - `cfg.lan_only`: `true` -> [`HostMode::Lan`], `false` -> [`HostMode::Public`].
/// - `cfg.relay_policy`: `"always"` -> [`RelayPolicy::Always`],
///   `"direct"` / `"never"` -> [`RelayPolicy::Never`], `_` -> [`RelayPolicy::Auto`].
/// - `cfg.ai_difficulty`: `"easy"` -> [`AiDifficulty::Easy`],
///   `"hard"` -> [`AiDifficulty::Hard`], `_` -> [`AiDifficulty::Medium`].
/// - `cfg.auto_assign_teams`: `true` -> [`TeamAssignment::AutoAssign`],
///   `false` -> [`TeamAssignment::PlayerSelect`].
/// - `playlist`: Starts with `swbf1/<map>:<side>:conquest` for the primary map/era
///   and appends deduplicated `swbf1/<map>:<side>:conquest` slugs from `ia_playlist`.
/// - `victory_timer_sec`: Normalizes values in `10..=60` seconds (or converts minutes > 60).
pub fn host_options_from_shell(cfg: &HostConfig, ia_playlist: &[PlaylistEntry]) -> HostOptions {
    // Map LAN-only toggle to `HostMode::Lan` vs `HostMode::Public`.
    let mode = if cfg.lan_only {
        HostMode::Lan
    } else {
        HostMode::Public
    };
    // Map TURN relay policy string (`"auto"`, `"always"`, `"direct"`/`"never"`) to `RelayPolicy`.
    let relay = match cfg.relay_policy.as_str() {
        "always" => RelayPolicy::Always,
        "direct" | "never" => RelayPolicy::Never,
        _ => RelayPolicy::Auto,
    };
    // Map AI difficulty string (`"easy"`, `"medium"`, `"hard"`) to `AiDifficulty`.
    let ai_difficulty = match cfg.ai_difficulty.as_str() {
        "easy" => AiDifficulty::Easy,
        "hard" => AiDifficulty::Hard,
        _ => AiDifficulty::Medium,
    };
    // Map team assignment boolean to `TeamAssignment::AutoAssign` vs `PlayerSelect`.
    let team_assignment = if cfg.auto_assign_teams {
        TeamAssignment::AutoAssign
    } else {
        TeamAssignment::PlayerSelect
    };
    // Normalize primary faction identifier (`"rep"`, `"cis"`, `"all"`, or `"imp"`).
    let side = if matches!(cfg.side.as_str(), "rep" | "cis" | "all" | "imp") {
        cfg.side.clone()
    } else if cfg.era == "gcw" {
        "all".to_string()
    } else {
        "rep".to_string()
    };
    let map = if cfg.map.is_empty() {
        "nab2".to_string()
    } else {
        cfg.map.clone()
    };
    // Build the mission rotation array starting with the primary selected map/side and
    // appending any additional entries from the Instant Action playlist.
    let mut playlist = vec![format!("swbf1/{map}:{side}:conquest")];
    for entry in ia_playlist {
        let entry_era = entry.era.as_deref().unwrap_or("cw");
        let entry_side = if entry_era == "gcw" { "all" } else { "rep" };
        let slug = format!("swbf1/{}:{entry_side}:conquest", entry.map);
        if !playlist.contains(&slug) {
            playlist.push(slug);
        }
    }
    // Normalize victory timer seconds to `10..=60` when set.
    let victory_timer_sec = if (10..=60).contains(&cfg.victory_timer_sec) {
        Some(cfg.victory_timer_sec)
    } else if cfg.victory_timer_sec > 60 {
        Some((cfg.victory_timer_sec / 60).clamp(10, 60))
    } else {
        None
    };

    HostOptions {
        name: if cfg.server_name.trim().is_empty() {
            "Battlefront Eternal Host".to_string()
        } else {
            cfg.server_name.clone()
        },
        mode,
        map,
        side,
        max_players: cfg.max_players.clamp(2, 64),
        relay,
        playlist,
        password: None,
        ai_units_per_team: Some(cfg.bots.min(32)),
        ai_difficulty: Some(ai_difficulty),
        reinforcements_pct: Some(cfg.reinforcements_pct.clamp(50, 500)),
        victory_timer_sec,
        friendly_fire: Some(cfg.friendly_fire),
        heroes_enabled: Some(cfg.heroes_enabled),
        team_assignment: Some(team_assignment),
        auto_spawn: Some(cfg.auto_spawn),
    }
}

/// Transition requested by the front-end shell.
#[derive(Clone, Debug)]
pub enum ShellTransition {
    /// Enter an Instant Action battle (`BattleRequest` with optional playlist).
    StartInstantAction(BattleRequest),
    /// Start a local hosted `bfeserver` via `HostController::start(&HostOptions)`
    /// and join it through the normal online path once `/info` answers.
    StartHost(HostOptions, MultiplayerLaunch),
    /// Join a remote or LAN server (`MultiplayerLaunch`).
    StartOnline(MultiplayerLaunch),
    /// User decision on the 3-button hosted server close dialog (`0` = KeepRunning,
    /// `1` = StopAndQuit, `2` = Cancel).
    HostCloseDecision(usize),
    /// Close the application (`EXIT TO WINDOWS` confirmed).
    Exit,
}

/// Asynchronous server browser refresh payload (`(refresh_token, Result<Vec<ServerEntry>, String>)`).
type BrowserRefreshMessage = (u64, Result<Vec<ServerEntry>, String>);

/// Non-blocking wrapper around [`bfe_online::ServerBrowser`] that executes
/// master server HTTP queries, LAN UDP broadcast discovery, and TCP `/info` probes
/// on background worker threads so the 60 Hz render loop never stalls.
pub struct LiveServerBrowser {
    /// Underlying stateful server list model (`bfe-online`).
    pub inner: ServerBrowser,
    /// Receiver for the currently in-flight background discovery/list task.
    rx: Option<mpsc::Receiver<BrowserRefreshMessage>>,
    /// Optional explicit `HOST:PORT` target injected from `--server` or `BFE_SERVER_TARGET`.
    server_target: Option<String>,
}

impl LiveServerBrowser {
    /// Creates a new [`LiveServerBrowser`] with optional direct target override.
    pub fn new(server_target: Option<String>, privacy_accepted: bool) -> Self {
        let mut inner = ServerBrowser::default();
        if !privacy_accepted {
            if let Some(token) = inner.begin_refresh(false) {
                inner.finish_refresh_notice(token);
            }
        }
        Self {
            inner,
            rx: None,
            server_target,
        }
    }

    /// Returns the explicit `--server` target (`HOST:PORT`), if configured.
    pub fn server_target(&self) -> Option<&str> {
        self.server_target.as_deref()
    }

    /// Starts an asynchronous refresh of either the LAN broadcast list or the Internet
    /// master directory without blocking the render thread.
    ///
    /// ## Non-Blocking Discovery Flow
    /// 1. Checks that the privacy notice has been accepted before querying `Source::Internet`.
    /// 2. Acquires a monotonic refresh token from `ServerBrowser::begin_refresh(false)`.
    /// 3. Spawns a short-lived background thread that runs either [`Self::fetch_lan`] or
    ///    [`Self::fetch_online`] and sends `(token, res)` over `self.rx`.
    /// 4. [`Self::poll`] drains `self.rx` via `try_recv` on the render thread.
    pub fn trigger_refresh(&mut self) {
        let source = self.inner.source();
        if source == Source::Internet && self.inner.state() == &BrowserState::Notice {
            return;
        }
        let Some(token) = self.inner.begin_refresh(false) else {
            return;
        };
        let target_override = self
            .server_target
            .clone()
            .or_else(|| std::env::var("BFE_SERVER_TARGET").ok());
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        thread::spawn(move || {
            let res = match source {
                Source::Lan => Self::fetch_lan(target_override.as_deref()),
                Source::Internet => Self::fetch_online(target_override.as_deref()),
            };
            let _ = tx.send((token, res));
        });
    }

    /// Probes an explicit `HOST:PORT` server via `GET /info` (`ping_host_port`), falling
    /// back to a UDP latency probe (`measure_latency`) if `/info` is unreachable.
    ///
    /// ## Direct Target Probe Strategy
    /// 1. Attempts an HTTP `GET /info` query via `ping_host_port(target, DIRECT_INFO_TIMEOUT)`
    ///    to retrieve the server's live name, map, era, player count, and rotation.
    /// 2. If the TCP `/info` listener is unreachable, resolves `target` via `resolve_host_port`
    ///    and sends a UDP ping probe (`bfe_online::measure_latency`) so local UDP test servers
    ///    still appear in the server browser with measured RTT.
    fn probe_direct_target(target: &str) -> Option<ServerEntry> {
        if let Ok((entry, _)) = ping_host_port(target, DIRECT_INFO_TIMEOUT) {
            return Some(entry);
        }
        let (host, addr) = resolve_host_port(target).ok()?;
        let ping = bfe_online::measure_latency(addr.ip(), addr.port(), PING_TIMEOUT)?;
        Some(ServerEntry {
            id: format!("{host}:{}", addr.port()),
            name: format!("BFESERVER ({host}:{})", addr.port()),
            address: Some(host),
            port: Some(addr.port()),
            location: "Direct".to_string(),
            country: "US".to_string(),
            simulation: Some(DEFAULT_SIMULATION.to_string()),
            players: 1,
            max_players: 32,
            bots: Some(true),
            latency_ms: Some(ping),
            battle: Some(BattleSummary {
                map: "nab2".to_string(),
                side: "rep".to_string(),
                era: "cw".to_string(),
                mode: "conquest".to_string(),
                ..BattleSummary::default()
            }),
            rotation: vec![RotationItem::Slug("swbf1/nab2:rep:conquest".to_string())],
            ..ServerEntry::default()
        })
    }

    /// Discovers LAN servers via UDP broadcast (`discover_lan`) and merges any direct target.
    ///
    /// Broadcasts a LAN discovery probe on `LAN_PORT` (`250 ms` timeout, `measure_ping: true`)
    /// and prepends any direct `target_override` server entry if not already in the list.
    fn fetch_lan(target_override: Option<&str>) -> Result<Vec<ServerEntry>, String> {
        let mut servers = discover_lan(LanDiscoveryOptions {
            port: LAN_PORT,
            timeout: Duration::from_millis(250),
            targets: None,
            measure_ping: true,
        })
        .unwrap_or_default();

        if let Some(target) = target_override {
            if let Some(probed) = Self::probe_direct_target(target) {
                if !servers.iter().any(|s| {
                    s.address == probed.address && s.port == probed.port || s.id == probed.id
                }) {
                    servers.insert(0, probed);
                }
            }
        }
        Ok(servers)
    }

    /// Queries the online master server directory (`MasterClient::list_servers`) and merges
    /// any direct `--server` target.
    ///
    /// Loads or creates the player's Ed25519 identity (`default_identity_path`), queries
    /// `MasterClient::list_servers()` against `resolve_master_url()`, and merges any
    /// direct `target_override` server entry at index `0`.
    fn fetch_online(target_override: Option<&str>) -> Result<Vec<ServerEntry>, String> {
        let mut servers = Vec::new();
        if let Some(id_path) = default_identity_path() {
            if let Ok(identity) = PlayerIdentity::load_or_create(&id_path) {
                let url = resolve_master_url();
                let client = MasterClient::new(url, identity);
                if let Ok(list) = client.list_servers() {
                    servers = list;
                }
            }
        }
        if let Some(target) = target_override {
            if let Some(probed) = Self::probe_direct_target(target) {
                if !servers.iter().any(|s| {
                    s.address == probed.address && s.port == probed.port || s.id == probed.id
                }) {
                    servers.insert(0, probed);
                }
            }
        }
        Ok(servers)
    }

    /// Polls background discovery/list completion without blocking.
    ///
    /// Checks `self.rx` via `try_recv()`. When the background worker finishes, commits
    /// the server list via `finish_refresh_ok` (or `finish_refresh_err`) and returns `true`
    /// so `ShellHost::step_and_draw` re-synchronizes `ShellApp`.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };
        match rx.try_recv() {
            Ok((token, Ok(entries))) => {
                self.rx = None;
                self.inner.finish_refresh_ok(token, entries, 1000)
            }
            Ok((token, Err(err))) => {
                self.rx = None;
                self.inner.finish_refresh_err(token, false, &err, 1000)
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                false
            }
        }
    }
}

impl ServerBrowserService for LiveServerBrowser {
    /// Returns the active server list source (`Source::Internet` or `Source::Lan`).
    fn source(&self) -> Source {
        self.inner.source()
    }

    /// Switches between `Source::Internet` and `Source::Lan`, triggering an async refresh.
    fn set_source(&mut self, source: Source) -> bool {
        let changed = self.inner.set_source(source);
        if changed {
            self.trigger_refresh();
        }
        changed
    }

    /// Returns the current server browser filter criteria.
    fn filters(&self) -> &Filters {
        self.inner.filters()
    }

    /// Applies a single filter update and recomputes the visible server rows.
    fn set_filter(&mut self, update: FilterUpdate) -> bool {
        self.inner.set_filter(update)
    }

    /// Resets all server browser filters to their defaults.
    fn clear_filters(&mut self) {
        self.inner.clear_filters();
    }

    /// Returns the active sort column and direction.
    fn sort_spec(&self) -> SortSpec {
        self.inner.sort_spec()
    }

    /// Toggles or changes the active sort column.
    fn sort_by(&mut self, column: SortColumn) {
        self.inner.sort_by(column);
    }

    /// Returns the filtered and sorted server entries.
    fn rows(&self) -> Vec<&ServerEntry> {
        self.inner.rows()
    }

    /// Returns the currently highlighted server entry, if any.
    fn selected(&self) -> Option<&ServerEntry> {
        self.inner.selected()
    }

    /// Selects a server entry by its identifier.
    fn select(&mut self, server_id: Option<&str>) {
        self.inner.select(server_id);
    }

    /// Handles keyboard navigation (`Up`, `Down`, `PageUp`, `PageDown`, `Home`, `End`) in the server table.
    fn key(&mut self, key: BrowserKey, shown_rows: usize) -> bool {
        self.inner.key(key, shown_rows)
    }

    /// Formats detailed map/era/mode metadata for the selected server entry.
    fn describe(&self, server: &ServerEntry, maps: &[MapCatalogEntry]) -> GameInfo {
        self.inner.describe(server, maps)
    }

    /// Returns the current browser lifecycle state (`Notice`, `Loading`, `Ready`, `Error`).
    fn state(&self) -> &BrowserState {
        self.inner.state()
    }

    /// Checks whether a periodic background refresh is due at `now_ms`.
    fn needs_refresh(&self, now_ms: u64) -> Option<RefreshKind> {
        self.inner.needs_refresh(now_ms)
    }
}

/// Front-end renderer and controller for the startup loader, click gate, shell menus,
/// and `bfe-interface` options pages.
pub struct ShellHost {
    /// Offscreen RGBA8 + Depth/Stencil framebuffer target.
    pub canvas: Canvas,
    /// `bfe-render` GL core owning compiled shader programs and textures.
    pub core: Core<glow::Context, Ranges>,
    /// 2D/3D-anchored HUD/interface quad pass (`post-hud/hud`).
    pub hud: HudPass<glow::Context>,
    /// CPU-side quad batch populated each frame by `draw_shell`, `draw_loading`, or `ClassicTheme::paint_stack`.
    pub batch: Batch,
    /// Bitmap fonts (`large`, `medium`, `small`, `tiny`) for `bfe-shell`.
    pub fonts: ShellFonts,
    /// Uploaded texture handles and map-loading zoom stages for `bfe-shell`.
    pub textures: ShellTextures,
    /// Viewport and texture handle table for `bfe_interface::ClassicTheme`.
    pub interface_surface: InterfaceSurfaceState,
    /// Stateful `bfe-interface` controller driving `OPTIONS` and its 4 sub-pages (`GAME`, `VIDEO`, `AUDIO`, `CONTROLS`).
    pub options_ctrl: OptionsController,
    /// Interactive pointer hit regions emitted by `ClassicTheme::paint_stack` during `OPTIONS` rendering.
    pub options_hits: Vec<HitRegion>,
    /// Front-end shell state machine (`Gate`, `Main`, `Singleplayer`, `InstantAction`, `Profile`, `Multiplayer`, `HostDialog`).
    pub app: ShellApp,
    pub page: bfe_shell::page::Page,
    blur: super::page_blur::PageBlur,
    /// CSS pixels per framebuffer pixel, matching Electron on Retina displays.
    last_page_click: Option<(bfe_shell::page::Hit, Instant)>,
    /// Non-blocking LAN and Internet server browser service.
    pub browser: LiveServerBrowser,
    /// Animated planet zoom and stage crossfade timeline for loading screens.
    pub loading_timeline: LoadingTimeline,
    /// Uppercase planet title displayed on the loading screen (e.g. `"NABOO"`).
    pub loading_planet: String,
    /// Localized status text displayed on the loading screen (e.g. `"LOADING"`).
    pub loading_text: String,
    /// Elapsed seconds within the current loading timeline animation.
    pub loading_time: f64,
    /// Path to the prepared content directory once loaded.
    pub content_dir: Option<PathBuf>,
    /// Current viewport dimensions `[width, height]` in physical pixels.
    pub size: [i32; 2],
    /// Count of frames rendered by `ShellHost`.
    pub frames: u64,
}

impl ShellHost {
    /// Initializes the front-end GL pass with a built-in fallback font so the
    /// startup loader can render immediately before `common.json` is loaded.
    ///
    /// ## Initial GL Texture Table
    /// - Texture handle `0`: 1x1 white dummy texture.
    /// - Texture handle `1`: 1x1 solid white quad texture (`textures.solid = 1`).
    /// - Texture handle `2`: 128x64 RGBA8 5x7 ASCII fallback font atlas (`build_fallback_ascii_font(2)`).
    ///
    /// Also constructs `self.options_ctrl` (`bfe_interface::OptionsController`) from
    /// `settings.interface` so `OPTIONS` pages are immediately available.
    pub fn new(
        size: [i32; 2],
        settings: &AppSettingsStore,
        server_target: Option<String>,
        mut make_gl: impl FnMut() -> glow::Context,
    ) -> Result<Self, String> {
        let canvas = Canvas::new(make_gl(), size)?;
        let mut core = Core::new(make_gl(), Ranges::new(make_gl()), 0, 0);
        core.canvas_framebuffer = Some(canvas.framebuffer);
        let mut hud = HudPass::new(&mut core);
        Self::init_hud_pass(&mut core, &mut hud, size)?;

        // Slot 0 = dummy texture, Slot 1 = 1x1 solid white texture (`textures.solid = 1`).
        let dummy_tex = Self::upload_rgba(&core.gl, 1, 1, &[255, 255, 255, 255])?;
        core.add_texture(dummy_tex);
        let solid_tex = Self::upload_rgba(&core.gl, 1, 1, &[255, 255, 255, 255])?;
        core.add_texture(solid_tex);

        // Slot 2 = built-in 5x7 ASCII fallback font texture.
        let (font_rgba, fw, fh, fonts) = build_fallback_ascii_font(2);
        let font_tex = Self::upload_rgba(&core.gl, fw as i32, fh as i32, &font_rgba)?;
        core.add_texture(font_tex);

        let textures = ShellTextures {
            solid: 1,
            by_name: HashMap::new(),
            loading_stages: Vec::new(),
        };

        // Initialize `bfe-interface` fonts, surface, and `OptionsController` from `settings.interface`.
        let interface_surface = shell_textures_to_interface_surface(&textures, size);
        let mut opt_ctx = OptionsContext::default();
        opt_ctx.has_integrations = true;
        opt_ctx.surface_height = interface_surface.height;
        opt_ctx.viewport = Some([size[0].max(1) as u32, size[1].max(1) as u32]);
        let options_ctrl = OptionsController::new(settings.interface.clone(), opt_ctx);

        let data = ShellData::default();
        let mut app = ShellApp::new(data, None);
        settings.apply_to_shell_app(&mut app);

        let loading_timeline = LoadingTimeline::new(
            3.5,
            1.2,
            0.5,
            vec![
                Some([0.2, 0.3, 0.25, 0.35]),
                Some([0.3, 0.4, 0.35, 0.45]),
                Some([0.25, 0.35, 0.3, 0.4]),
                None,
            ],
        );

        let browser =
            LiveServerBrowser::new(server_target, settings.player.privacy_notice_accepted);

        let blur = super::page_blur::PageBlur::new(&core.gl, size)?;
        core.state.reset();
        Ok(Self {
            blur,
            canvas,
            core,
            hud,
            batch: Batch::new(),
            page: bfe_shell::page::Page::new(Font::default(), Font::default()),
            last_page_click: None,
            fonts,
            textures,
            interface_surface,
            options_ctrl,
            options_hits: Vec::new(),
            app,
            browser,
            loading_timeline,
            loading_planet: "BATTLEFRONT ETERNAL".to_string(),
            loading_text: "PREPARING CONTENT".to_string(),
            loading_time: 0.0,
            content_dir: None,
            size,
            frames: 0,
        })
    }

    /// Loads `common.json` and `shell.json` from `content_dir`, uploading bitmap
    /// fonts, backdrops, map previews, and stage textures, and installing a
    /// [`ShellAudioMixer`] into `audio_router`.
    ///
    /// ## Asset Upload Steps
    /// 1. Parses `common/common.json` and `shell/shell.json` via `load_shell_data_from_dir`.
    /// 2. Uploads bitmap font atlas pages (`gamefont_large`, `gamefont_medium`, `gamefont_small`,
    ///    `gamefont_tiny`) and constructs both [`ShellFonts`] and [`bfe_interface::Fonts`].
    /// 3. Uploads all UI chrome, backdrop, faction emblem, map preview (`map_<id>`),
    ///    and loading zoom stage textures into `self.core` and `self.interface_surface`.
    /// 4. Rebuilds `self.options_ctrl` (`OptionsController`) with the loaded localization
    ///    string table (`data.strings`) and current viewport dimensions.
    /// 5. Initializes [`ShellAudioMixer`] and installs it into `audio_router`.
    pub fn load_prepared_content(
        &mut self,
        content_dir: &Path,
        settings: &AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Result<(), String> {
        let data = load_shell_data_from_dir(content_dir);
        let mut lib = Library::new(content_dir, false, 4096, 0)?;

        // Deduplicate texture uploads by relative asset URL.
        let mut uploaded_paths: HashMap<String, (u32, u32, u32)> = HashMap::new();
        let core = &mut self.core;
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

        // Upload bitmap font atlas pages and build glyph metrics.
        let loaded_fonts = Self::load_fonts(&data, &mut upload_path);
        if !loaded_fonts.medium.glyphs.is_empty() {
            self.fonts = loaded_fonts;
        }

        let mut textures = ShellTextures {
            solid: 1,
            by_name: HashMap::new(),
            loading_stages: Vec::new(),
        };

        // Upload all named UI chrome and backdrop textures (`NAMED_TEXTURES`).
        for &name in NAMED_TEXTURES {
            if let Some(url) = data.textures.get(name) {
                if let Some((handle, _, _)) = upload_path(&url) {
                    textures.by_name.insert(name.to_string(), handle);
                }
            }
        }
        // Upload per-map preview thumbnails (`map_<id>`).
        for map in &data.maps {
            let preview_name = format!("map_{}", map.id);
            if let Some(url) = data.textures.get(preview_name.as_str()) {
                if let Some((handle, _, _)) = upload_path(&url) {
                    textures.by_name.insert(preview_name, handle);
                }
            }
        }
        // Upload loading screen disk, edge, and LED textures.
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

        // Upload multi-stage planet zoom textures for the loading screen.
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
                            [t[0] / wf, t[1] / hf, (t[0] + t[2]) / wf, (t[1] + t[3]) / hf]
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
        self.loading_timeline = LoadingTimeline::new(
            data.load.track,
            data.load.zoom,
            data.load.hold,
            stage_targets,
        );
        self.loading_planet = load_map
            .map(|m| planet_name(&m.id, &data.strings, m.load.as_ref()))
            .unwrap_or_else(|| "NABOO".to_string());
        self.loading_text = data
            .strings
            .get(&data.load.text)
            .cloned()
            .unwrap_or_else(|| "LOADING".to_string());
        self.textures = textures;
        let mut page_fonts = Vec::with_capacity(6);
        for bytes in [
            include_bytes!("../../assets/fonts/saira-700.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-900.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-800.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-800-normal.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-400.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-600.ttf").as_slice(),
        ] {
            let (rgba, mut font) = super::page_font::atlas(bytes)?;
            let texture = Self::upload_page_font(&self.core.gl, &rgba)?;
            let handle = self.core.add_texture(texture);
            for glyph in font.glyphs.values_mut() {
                glyph.page = handle;
            }
            page_fonts.push(font);
        }
        let medium = page_fonts.pop().unwrap();
        let regular = page_fonts.pop().unwrap();
        let bold = page_fonts.pop().unwrap();
        let title = page_fonts.pop().unwrap();
        let logo = page_fonts.pop().unwrap();
        self.page = bfe_shell::page::Page::new(page_fonts.pop().unwrap(), logo);
        self.page.title = title;
        self.page.bold = bold;
        self.page.regular = regular;
        self.page.medium = medium;
        let button_w = ((self.size[0] as f64 * 0.30).min(400.) - 16.)
            .round()
            .max(28.) as usize;
        let button_h = (self.size[1] as f64 * 0.056).clamp(38., 56.).round() as usize;
        self.page.button_source = [button_w as f64, button_h as f64];
        for look in 0..3 {
            let w = (self.size[0] as f64).round() as usize;
            let h = (self.size[1] as f64).round() as usize;
            let rgba = super::page_surfaces::pattern(w, h, look);
            let texture = Self::upload_rgba(&self.core.gl, w as i32, h as i32, &rgba)?;
            self.page.patterns[look] = self.core.add_texture(texture);
        }
        for look in 0..3 {
            for selected in [false, true] {
                let rgba = super::page_surfaces::button(button_w, button_h, look, selected);
                let texture =
                    Self::upload_rgba(&self.core.gl, button_w as i32, button_h as i32, &rgba)?;
                self.page.buttons[look * 2 + usize::from(selected)] =
                    self.core.add_texture(texture);
            }
        }

        // Rebuild `bfe-interface` fonts, surface, and `OptionsController` with loaded localization strings.
        self.interface_surface = shell_textures_to_interface_surface(&self.textures, self.size);
        let mut opt_ctx = OptionsContext::default();
        opt_ctx.strings = data
            .strings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        opt_ctx.surface_height = self.interface_surface.height;
        opt_ctx.viewport = Some([self.size[0].max(1) as u32, self.size[1].max(1) as u32]);
        self.options_ctrl = OptionsController::new(settings.interface.clone(), opt_ctx);

        let about = AboutInfo::from_content_dir(content_dir, Some(env!("CARGO_PKG_VERSION")));
        let mut app = ShellApp::new(data.clone(), Some(&about));
        settings.apply_to_shell_app(&mut app);

        // Start front-end shell audio mixer and queue initial transition cues.
        let mut shell_audio = ShellAudioMixer::new(&data, Some(content_dir));
        shell_audio.set_volumes(
            app.settings.master_volume,
            app.settings.music_volume,
            app.settings.interface_volume,
        );
        for snd in app.drain_sounds() {
            shell_audio.play(snd);
        }
        let media = app.drain_media_events();
        shell_audio.apply_media_events(&media, &data);
        audio_router.set_shell(Some(shell_audio));

        self.app = app;
        self.content_dir = Some(content_dir.to_path_buf());
        self.frames = 0;
        Ok(())
    }

    /// Returns `true` when the active screen is `OPTIONS` (or one of its sub-pages)
    /// and should be rendered and driven by `bfe-interface`'s [`OptionsController`].
    ///
    /// Returns `false` when a modal popup (`self.app.popup.is_some()`) or profile
    /// text-entry box (`self.app.profile.editing`) is active so modal popups retain focus.
    pub fn is_interface_options_active(&self) -> bool {
        self.app.popup.is_none()
            && !self.app.profile.editing
            && matches!(
                self.app.current_screen(),
                ScreenId::Options | ScreenId::OptionsPage(_)
            )
    }

    /// Synchronizes [`Self::options_ctrl`] with `self.app.current_screen()` and `settings`.
    ///
    /// Ensures `self.options_ctrl.stack` is populated with the root `"options"` screen
    /// when `ScreenId::Options` is active, or pushes the matching `bfe-interface` sub-page
    /// (`"game"`, `"video"`, `"audio"`, `"controls"`) when `ScreenId::OptionsPage(_)` is active.
    pub(super) fn sync_options_controller(&mut self, settings: &AppSettingsStore) {
        if self.options_ctrl.stack.is_empty() {
            let mut ctx = self.options_ctrl.context.clone();
            ctx.surface_height = self.interface_surface.height;
            ctx.viewport = Some([self.size[0].max(1) as u32, self.size[1].max(1) as u32]);
            self.options_ctrl = OptionsController::new(settings.interface.clone(), ctx);
        }
        if let ScreenId::OptionsPage(page) = self.app.current_screen() {
            let target_id = match page {
                OptionsPageId::Game => "options-game",
                OptionsPageId::Video => "options-video",
                OptionsPageId::Audio => "options-audio",
                OptionsPageId::Controls => "options-controls",
            };
            let current_top = self
                .options_ctrl
                .stack
                .top()
                .map(|s| s.id.as_str())
                .unwrap_or("");
            if current_top == "options" {
                let page_key = match page {
                    OptionsPageId::Game => "game",
                    OptionsPageId::Video => "video",
                    OptionsPageId::Audio => "audio",
                    OptionsPageId::Controls => "controls",
                };
                self.options_ctrl.open_page(page_key);
                self.options_ctrl.stack.sounds.drain();
            } else if current_top != target_id && !current_top.starts_with("popup") {
                self.options_ctrl.refresh_active_screen();
            }
        }
    }

    /// Drains sound cues and named actions from [`Self::options_ctrl`] and persists
    /// any modified settings into `settings` and `self.app`.
    ///
    /// ## Post-Input Synchronization
    /// 1. Drains `SoundCue` events from `self.options_ctrl.stack.sounds` into `self.app.sound_queue`.
    /// 2. Persists any modified keys from `self.options_ctrl.settings` into `settings`
    ///    (`player-settings.json`) and `self.app.settings`.
    /// 3. Handles named navigation actions (such as `"online"` -> `ScreenId::MultiplayerList`).
    /// 4. Pops `ScreenId::Options` / `ScreenId::OptionsPage(_)` from `self.app.stack` when
    ///    the user backs out of the root `bfe-interface` options screen.
    fn flush_options_controller(&mut self, settings: &mut AppSettingsStore) {
        for cue in self.options_ctrl.stack.sounds.drain() {
            self.app.sound_queue.push(cue.cue_name());
        }
        let _ =
            settings.capture_from_interface_settings(&self.options_ctrl.settings, &mut self.app);

        if let Some(action) = self.options_ctrl.last_named_action.take() {
            if action == "integrations" {
                self.open_integrations(settings);
            } else if action == "discord-cycle" {
                self.step_discord_detail(settings, false);
            } else if action == "online" {
                self.app
                    .refresh_multiplayer_list(MultiplayerKind::Online, &mut self.browser);
                self.app
                    .push_screen(ScreenId::MultiplayerList(MultiplayerKind::Online));
            }
        }

        if self.options_ctrl.stack.is_empty() {
            while matches!(
                self.app.stack.last(),
                Some(ScreenId::Options | ScreenId::OptionsPage(_))
            ) && self.app.stack.len() > 1
            {
                self.app.stack.pop();
            }
            self.app.rebuild_cached_rows();
        }
    }

    /// Shows the 3-button hosted server close confirmation dialog from [`ClosePrompt`].
    ///
    /// Copies `prompt.title` (`"Hosted server is running"`), `prompt.message`
    /// (`"Other players can keep playing after you close the app."`), the 3 button
    /// labels (`"Keep server running and quit"`, `"Stop server and quit"`, `"Cancel"`),
    /// and `prompt.default_id` (`0`) into `PopupState::HostClosePrompt`.
    pub fn show_host_close_prompt(&mut self, prompt: &ClosePrompt) {
        self.app.popup = Some(PopupState::HostClosePrompt {
            title: prompt.title.to_string(),
            message: prompt.message.to_string(),
            buttons: [
                prompt.buttons[0].to_string(),
                prompt.buttons[1].to_string(),
                prompt.buttons[2].to_string(),
            ],
            selected: prompt.default_id,
        });
    }

    /// Configures the loading screen for a specific map (`map_id`) before transitioning
    /// into a battle.
    ///
    /// Resolves the localized planet title (`planet_name`) and loading prompt string
    /// (`data.load.text`), resets `self.loading_time = 0.0`, and schedules the loading
    /// screen sound effects (`loading_timeline.sounds()`) on `ShellAudioMixer`.
    pub fn prepare_map_loading(&mut self, map_id: &str, audio_router: &SharedAudioRouter) {
        if let Some(map) = self.app.data.maps.iter().find(|m| m.id == map_id) {
            self.loading_planet = planet_name(&map.id, &self.app.data.strings, map.load.as_ref());
        } else {
            self.loading_planet = map_id.to_ascii_uppercase();
        }
        self.loading_text = self
            .app
            .data
            .strings
            .get(&self.app.data.load.text)
            .cloned()
            .unwrap_or_else(|| "LOADING".to_string());
        self.loading_time = 0.0;
        let sounds = self.loading_timeline.sounds();
        if let Ok(mut guard) = audio_router.shell.lock() {
            if let Some(mixer) = guard.as_mut() {
                mixer.schedule_loading_plan(&sounds, 0.0);
            }
        }
    }

    /// Renders the startup / preparation loader screen (`draw_loading`).
    ///
    /// ## Loader Variants
    /// - `StartupContentState::Ready`: Renders the planet title and `"LOADING"` prompt
    ///   at `self.loading_time`.
    /// - `StartupContentState::Preparing`: Renders `"INSTALLING ASSETS"` and the current
    ///   extraction phase label + integer percentage (`0%..100%`).
    /// - `StartupContentState::WaitingForFolder`: Renders `"CONTENT REQUIRED"` and the
    ///   `F9` folder-selection instructions.
    pub fn draw_startup_loader(&mut self, dt: f64, state: &StartupContentState) {
        self.loading_time += dt.clamp(0.0, 0.1);
        let (planet, text, timeline_time) = match state {
            StartupContentState::Ready(_) => (
                self.loading_planet.clone(),
                self.loading_text.clone(),
                self.loading_time,
            ),
            StartupContentState::Preparing { latest, .. } => {
                let pct = (latest.percent * 100.0).round() as u32;
                let phase = if latest.phase_label.is_empty() {
                    "PREPARING GAME CONTENT".to_string()
                } else {
                    latest.phase_label.to_ascii_uppercase()
                };
                let status = format!("{phase} ({pct}%)");
                let total_dur = self.loading_timeline.duration().max(1.0);
                let t = (latest.percent * total_dur).clamp(0.0, total_dur);
                ("INSTALLING ASSETS".to_string(), status, t)
            }
            StartupContentState::WaitingForFolder { message } => (
                "CONTENT REQUIRED".to_string(),
                message.clone(),
                self.loading_time % 3.5,
            ),
        };

        let at = self.loading_timeline.at(timeline_time);
        let w = self.size[0] as f64;
        let h = self.size[1] as f64;
        draw_loading_with_typeface(
            &mut self.batch,
            &self.fonts,
            &self.textures,
            &at,
            timeline_time,
            &planet,
            &text,
            LookId::from_theme(Some(&self.app.settings.menu_theme)),
            w,
            h,
            Some((&self.page.body, &self.page.bold)),
        );
        self.upload_and_draw_batch();
    }

    /// Renders a map-loading frame (`draw_loading`) while transitioning into a battle.
    pub fn draw_battle_loading(&mut self, dt: f64) {
        self.loading_time += dt.clamp(0.0, 0.1);
        let at = self.loading_timeline.at(self.loading_time);
        let w = self.size[0] as f64;
        let h = self.size[1] as f64;
        draw_loading_with_typeface(
            &mut self.batch,
            &self.fonts,
            &self.textures,
            &at,
            self.loading_time,
            &self.loading_planet,
            &self.loading_text,
            LookId::from_theme(Some(&self.app.settings.menu_theme)),
            w,
            h,
            Some((&self.page.body, &self.page.bold)),
        );
        self.upload_and_draw_batch();
    }

    /// Renders the progress screen while `HostController::start` spawns `bfeserver`
    /// and polls `/admin` and `/info` until ready.
    pub fn draw_host_starting(&mut self, dt: f64, server_name: &str) {
        self.loading_time += dt.clamp(0.0, 0.1);
        let at = self.loading_timeline.at(self.loading_time);
        let w = self.size[0] as f64;
        let h = self.size[1] as f64;
        let status = format!("STARTING HOSTED SERVER: {server_name}");
        draw_loading_with_typeface(
            &mut self.batch,
            &self.fonts,
            &self.textures,
            &at,
            self.loading_time,
            &self.loading_planet,
            &status,
            LookId::from_theme(Some(&self.app.settings.menu_theme)),
            w,
            h,
            Some((&self.page.body, &self.page.bold)),
        );
        self.upload_and_draw_batch();
    }

    /// Default prepared-media source while the movie host is being integrated.
    /// Decoded frame textures use the same Page inputs, including provider UVs.
    fn page_media_textures(&self) -> bfe_shell::page::MediaTextures {
        use bfe_shell::page::{FrameTexture, MediaTextures};
        let right = self.textures.backdrop_for(ScreenId::Main);
        let left = self.textures.get("coreship_screen");
        let preview = self
            .app
            .data
            .maps
            .get(self.app.ia_available_selected)
            .and_then(|map| {
                self.textures
                    .by_name
                    .iter()
                    .find(|(name, _)| name.strip_prefix("map_") == Some(map.id.as_str()))
                    .map(|(_, texture)| FrameTexture::new(*texture))
            });
        MediaTextures {
            backdrops: [
                FrameTexture::new(if left == 0 { right } else { left }),
                FrameTexture::new(right),
            ],
            preview,
        }
    }

    /// Updates and renders the active shell screen (`Gate`, `Main`, `InstantAction`,
    /// `bfe-interface` `Options`, `Profile`, `Multiplayer`, `HostDialog`, etc.).
    ///
    /// ## Rendering Dispatch
    /// - Polls background server browser results (`self.browser.poll()`) and advances
    ///   `gate_elapsed` / `pulse_elapsed` animations.
    /// - When [`Self::is_interface_options_active`] is `true`, updates `self.options_ctrl.stack`
    ///   and paints the active `bfe-interface` screen stack via [`ClassicTheme::paint_stack`],
    ///   recording interactive pointer hit regions into `self.options_hits`.
    /// - Otherwise renders the active `bfe-shell` screen (`Gate`, `Main`, `Singleplayer`,
    ///   `InstantAction`, `Profile`, `Multiplayer`, `HostDialog`, popups) via [`draw_shell`].
    pub fn step_and_draw(
        &mut self,
        dt: f64,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        if self.browser.poll() || (self.browser_page() && self.page.browser.source.is_none()) {
            self.sync_page_browser();
        }
        if dt.is_finite() && dt > 0.0 {
            self.app.gate_elapsed += dt;
            self.app.pulse_elapsed += dt;
        }
        // Movie integration bind point: replace the provider on this line.
        // Page consumes current textures/crops and never assumes a still image.
        self.page.media = self.page_media_textures();
        if !self.page.input_matches(&self.app) && std::env::var_os("BFE_MENU_INPUT_TRACE").is_some()
        {
            eprintln!(
                "[menu paint] {:?} framebuffer={:?}",
                self.app.current_screen(),
                self.size
            );
        }
        if self.page_active() {
            self.page.draw(
                &mut self.batch,
                &self.textures,
                &self.app,
                self.size[0] as f64,
                self.size[1] as f64,
            );
        } else if self.is_interface_options_active() {
            // Paint the existing options stack in the selected page look.
            self.sync_options_controller(settings);
            if dt.is_finite() && dt > 0.0 {
                self.options_ctrl.stack.update(dt);
            }
            self.page.draw(
                &mut self.batch,
                &self.textures,
                &self.app,
                self.size[0] as f64,
                self.size[1] as f64,
            );
            self.page.paint_widgets(
                &mut self.batch,
                &self.textures,
                &mut self.options_ctrl.stack,
                self.size[0] as f64,
                self.size[1] as f64,
                &self.app.settings.menu_theme,
                &mut self.options_hits,
            );
        } else {
            let w = self.size[0] as f64;
            let h = self.size[1] as f64;
            draw_shell(
                &mut self.batch,
                &self.fonts,
                &self.textures,
                &self.app,
                w,
                h,
            );
        }

        if let Some(modal) = self.page.modal_start {
            self.draw_range(0, modal, true);
            self.blur.apply(&self.core.gl, self.canvas.framebuffer, 4.);
            self.core.state.reset();
            self.draw_range(modal, self.batch.count, false);
        } else {
            self.upload_and_draw_batch();
        }
        self.flush_events(settings, audio_router)
    }

    /// Flushes queued shell sound cues, media transitions, settings changes,
    /// and screen transition actions.
    ///
    /// ## Action and Audio Dispatch
    /// - Drains `ShellApp::drain_sounds()` and `drain_media_events()` into the active
    ///   [`ShellAudioMixer`] on `audio_router`.
    /// - Persists any modified `ShellApp` settings (`profile.name`, `ia_entries`, etc.).
    /// - Converts `ShellAction::LaunchMultiplayer` with `server_id == "local-host"` into
    ///   `ShellTransition::StartHost(HostOptions, MultiplayerLaunch)` via [`host_options_from_shell`].
    /// - Converts `ShellAction::HostCloseDecision(idx)` into `ShellTransition::HostCloseDecision(idx)`.
    pub fn flush_events(
        &mut self,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        let sounds = self.app.drain_sounds();
        let media = self.app.drain_media_events();
        let _ = settings.capture_from_shell_app(&self.app);

        if !sounds.is_empty() || !media.is_empty() {
            if let Ok(mut guard) = audio_router.shell.lock() {
                if let Some(mixer) = guard.as_mut() {
                    mixer.set_volumes(
                        self.app.settings.master_volume,
                        self.app.settings.music_volume,
                        self.app.settings.interface_volume,
                    );
                    for snd in sounds {
                        mixer.play(snd);
                    }
                    mixer.apply_media_events(&media, &self.app.data);
                }
            }
        }

        if let Some(action) = self.app.take_action() {
            match action {
                ShellAction::LaunchBattle(req) => {
                    return Some(ShellTransition::StartInstantAction(req));
                }
                ShellAction::LaunchMultiplayer(mp) => {
                    if mp.server_id == "local-host" || mp.server_id == "host:local" {
                        let host_opts =
                            host_options_from_shell(&self.app.host_config, &self.app.ia_entries);
                        return Some(ShellTransition::StartHost(host_opts, mp));
                    }
                    return Some(ShellTransition::StartOnline(mp));
                }
                ShellAction::HostCloseDecision(idx) => {
                    return Some(ShellTransition::HostCloseDecision(idx));
                }
                ShellAction::ExitToWindows => {
                    return Some(ShellTransition::Exit);
                }
            }
        }
        None
    }

    /// Handles a keyboard input event in the front-end shell.
    ///
    /// ## Keyboard Routing
    /// - When [`Self::is_interface_options_active`] is `true`, synchronizes and forwards
    ///   `key` to `self.options_ctrl.on_key_down(key, false)`, then flushes sound cues,
    ///   settings changes, and screen stack pops via [`Self::flush_options_controller`].
    /// - Otherwise forwards `key` to `ShellApp::handle_key`, resetting `self.options_ctrl`
    ///   when entering `ScreenId::Options` and triggering an initial server browser refresh
    ///   as soon as the online privacy notice is accepted.
    pub fn handle_key(
        &mut self,
        key: &str,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        if std::env::var_os("BFE_MENU_INPUT_TRACE").is_some() {
            eprintln!("[menu input] {:?} key={key:?}", self.app.current_screen());
        }
        if let Some(selection) = self.page.dropdown_key(key) {
            if let Some(index) = selection {
                self.app.settings.menu_theme = ["bf1", "bf2", "eternal"][index].into();
                self.options_ctrl
                    .settings
                    .set("menuTheme", self.app.settings.menu_theme.clone());
            }
            return self.flush_events(settings, audio_router);
        }
        if self.page.game_dropdown {
            if matches!(key, "Escape" | "Tab" | "Enter" | " ") {
                self.page.game_dropdown = false;
            }
            return self.flush_events(settings, audio_router);
        }
        if key == "Tab"
            && matches!(
                self.app.current_screen(),
                ScreenId::Main | ScreenId::Singleplayer | ScreenId::Profile
            )
            && !self.app.profile.editing
            && self.app.popup.is_none()
        {
            self.page.dropdown = true;
            return self.flush_events(settings, audio_router);
        }
        if self.browser_page() && self.page_browser_key(key) {
            return self.flush_events(settings, audio_router);
        }
        if self.is_interface_options_active() {
            self.sync_options_controller(settings);
            if matches!(key, "ArrowLeft" | "ArrowRight")
                && self
                    .options_ctrl
                    .stack
                    .top()
                    .is_some_and(|s| s.id == "options-integrations")
            {
                self.step_discord_detail(settings, key == "ArrowLeft");
                return self.flush_events(settings, audio_router);
            }
            self.options_ctrl.on_key_down(key, false);
            self.flush_options_controller(settings);
            return self.flush_events(settings, audio_router);
        }
        let was_notice = self.app.mp_needs_notice;
        let prev_screen = self.app.current_screen();
        let refresh_requested = matches!(key, "Enter" | " ")
            && matches!(prev_screen, ScreenId::MultiplayerList(_))
            && self
                .app
                .active_rows()
                .get(self.app.selected_row_index(prev_screen))
                .is_some_and(|row| row.id == "refresh");
        self.app
            .handle_key(key, false, false, false, &mut self.browser);
        if !matches!(prev_screen, ScreenId::Options | ScreenId::OptionsPage(_))
            && matches!(
                self.app.current_screen(),
                ScreenId::Options | ScreenId::OptionsPage(_)
            )
        {
            self.options_ctrl.stack.clear();
            self.sync_options_controller(settings);
        }
        let entered_list = prev_screen != self.app.current_screen()
            && matches!(self.app.current_screen(), ScreenId::MultiplayerList(_));
        if (was_notice && !self.app.mp_needs_notice)
            || refresh_requested
            || (entered_list && self.browser.rx.is_none())
        {
            self.browser.trigger_refresh();
            self.sync_page_browser();
        }
        self.flush_events(settings, audio_router)
    }

    /// Updates hover focus when the mouse cursor moves to `(x_px, y_px)`.
    ///
    /// ## Pointer Hover Routing
    /// - When [`Self::is_interface_options_active`] is `true`, maps `(x_px, y_px)` against
    ///   `self.options_hits` via `self.options_ctrl.input.on_pointer_move` and queues any
    ///   resulting hover sound cues.
    /// - Otherwise maps `(x_px, y_px)` into the `1024x768` logical shell coordinate space
    ///   and updates footer focus (`IaFocus`) or menu row selection.
    fn page_active(&self) -> bool {
        matches!(
            self.app.stack.last().copied().unwrap_or(ScreenId::Gate),
            ScreenId::Gate
                | ScreenId::Main
                | ScreenId::Singleplayer
                | ScreenId::Profile
                | ScreenId::InstantActionEra
                | ScreenId::InstantAction
                | ScreenId::Multiplayer
                | ScreenId::MultiplayerList(_)
                | ScreenId::HostDialog
                | ScreenId::DirectConnect
        )
    }

    pub fn handle_cursor_move(&mut self, x_px: f64, y_px: f64) {
        if self.page_active() {
            match self.page.hit(x_px, y_px) {
                Some(bfe_shell::page::Hit::Look(i)) => self.page.dropdown_cursor = i,
                Some(bfe_shell::page::Hit::BrowserChoice(_, i)) => {
                    self.page.browser.choice_cursor = i
                }
                _ => {}
            }
            if let Some(bfe_shell::page::Hit::Row(row)) = self.page.hit(x_px, y_px) {
                self.app
                    .set_selected_row_index(self.app.current_screen(), row);
            }
            return;
        }
        if self.is_interface_options_active() {
            let vw = self.size[0].max(1) as f64;
            let vh = self.size[1].max(1) as f64;
            let ev = self.options_ctrl.input.on_pointer_move(
                &mut self.options_ctrl.stack,
                &self.options_hits,
                x_px,
                y_px,
                vw,
                vh,
            );
            self.options_ctrl.apply_event(ev);
            for cue in self.options_ctrl.stack.sounds.drain() {
                self.app.sound_queue.push(cue.cue_name());
            }
            return;
        }
        if self.app.popup.is_some() || self.app.profile.editing {
            return;
        }
        let w = self.size[0].max(1) as f64;
        let h = self.size[1].max(1) as f64;
        let ix = (x_px / w) * 1024.0;
        let iy = (y_px / h) * 768.0;

        let screen = self.app.current_screen();
        if screen == ScreenId::Gate {
            return;
        }
        if screen == ScreenId::InstantAction {
            if let Some((left, row)) = bfe_shell::render::hit_ia_list(&self.app, w, h, x_px, y_px) {
                if left {
                    self.app.ia_focus = IaFocus::Available;
                    self.app.ia_available_selected = row;
                } else {
                    self.app.ia_focus = IaFocus::Selected;
                    self.app.ia_selected_cursor = row;
                }
            }
            if iy >= 690.0 {
                if ix < 260.0 {
                    self.app.ia_focus = IaFocus::Back;
                } else if ix > 760.0 {
                    self.app.ia_focus = IaFocus::Launch;
                } else {
                    self.app.ia_focus = IaFocus::Order;
                }
            }
            return;
        }

        if let Some(index) =
            bfe_shell::render::hit_player_menu(&self.app, &self.fonts, w, h, x_px, y_px)
        {
            self.app.set_selected_row_index(screen, index);
        }
    }

    /// Handles a primary mouse button click at `(x_px, y_px)`.
    ///
    /// ## Pointer Click Routing
    /// - When [`Self::is_interface_options_active`] is `true`, dispatches
    ///   `self.options_ctrl.input.on_pointer_down` + `on_pointer_up` against `self.options_hits`
    ///   and flushes modified settings via [`Self::flush_options_controller`].
    /// - Otherwise checks the bottom-left `BACK` footer zone (`iy >= 690 && ix < 220`),
    ///   unlocks the title gate on `ScreenId::Gate`, or invokes `ShellApp::accept`.
    pub fn handle_mouse_click(
        &mut self,
        x_px: f64,
        y_px: f64,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        if !self.page.input_matches(&self.app) {
            return self.flush_events(settings, audio_router);
        }
        match self.page.hit(x_px, y_px) {
            Some(bfe_shell::page::Hit::OptionBack(widget, item)) => {
                if let Some(top) = self.options_ctrl.stack.top_mut() {
                    top.focused = Some(widget);
                    if let Some(bfe_interface::widgets::Widget::Options(options)) =
                        top.widgets.get_mut(widget)
                    {
                        options.selected = item;
                    }
                }
                self.options_ctrl.on_key_down("ArrowLeft", false);
                self.flush_options_controller(settings);
                return self.flush_events(settings, audio_router);
            }
            Some(bfe_shell::page::Hit::Theme) => {
                self.page.dropdown = !self.page.dropdown;
                self.page.dropdown_cursor = match self.app.settings.menu_theme.as_str() {
                    "bf1" => 0,
                    "bf2" => 1,
                    _ => 2,
                };
                return self.flush_events(settings, audio_router);
            }
            Some(bfe_shell::page::Hit::Look(index)) => {
                self.app.settings.menu_theme = ["bf1", "bf2", "eternal"][index].into();
                self.options_ctrl
                    .settings
                    .set("menuTheme", self.app.settings.menu_theme.clone());
                self.page.dropdown = false;
                return self.flush_events(settings, audio_router);
            }
            _ => {}
        }
        self.page.dropdown = false;
        let hit = self.page.hit(x_px, y_px);
        let double = self.last_page_click.is_some_and(|(previous, time)| {
            Some(previous) == hit && time.elapsed().as_secs_f64() < 0.35
        });
        self.last_page_click = hit.map(|hit| (hit, Instant::now()));
        if self.browser_page() {
            if let Some(hit) = self.page.hit(x_px, y_px) {
                if self.page_browser_hit(hit) {
                    if double && matches!(hit, bfe_shell::page::Hit::BrowserServer(_)) {
                        self.page_browser_hit(bfe_shell::page::Hit::BrowserFooter(3));
                    }
                    return self.flush_events(settings, audio_router);
                }
            }
        }
        if self.page_active() {
            let was_notice = self.app.mp_needs_notice;
            use bfe_shell::page::Hit;
            match self.page.hit(x_px, y_px) {
                Some(Hit::Theme) => self.page.dropdown = !self.page.dropdown,
                Some(Hit::Look(i)) => {
                    self.app.settings.menu_theme = ["bf1", "bf2", "eternal"][i].into();
                    self.page.dropdown = false;
                }
                Some(Hit::DirectField(index)) => self.app.direct_field = index,
                Some(Hit::DirectButton(true)) => self.app.accept(&mut self.browser),
                Some(Hit::DirectButton(false)) => self.app.back(),
                Some(Hit::Game) => self.page.game_dropdown = !self.page.game_dropdown,
                Some(Hit::GameChoice(index)) => {
                    if let Some((family, _, _)) = self.app.ia_game_picker_options().get(index) {
                        self.app.ia_select_family(*family);
                    }
                    self.page.game_dropdown = false;
                }
                Some(Hit::Back) => self.app.back(),
                Some(Hit::Popup(index)) => {
                    match self.app.popup.as_mut() {
                        Some(
                            bfe_shell::screens::PopupState::ConfirmExit { selected_yes, .. }
                            | bfe_shell::screens::PopupState::OnlineNotice {
                                selected_accept: selected_yes,
                                ..
                            },
                        ) => *selected_yes = index == 0,
                        Some(bfe_shell::screens::PopupState::HostClosePrompt {
                            selected, ..
                        }) => *selected = index,
                        _ => {}
                    }
                    self.app.accept(&mut self.browser);
                }
                Some(Hit::Profile(accept)) => {
                    self.app.handle_key(
                        if accept { "Enter" } else { "Escape" },
                        false,
                        false,
                        false,
                        &mut self.browser,
                    );
                }
                Some(Hit::Map(i)) => {
                    self.app.ia_focus = IaFocus::Available;
                    let delta = i as isize - self.app.ia_available_selected as isize;
                    self.app.move_vertical(delta);
                    if double {
                        self.app.accept(&mut self.browser);
                    }
                }
                Some(Hit::Entry(i)) => {
                    self.app.ia_focus = IaFocus::Selected;
                    let delta = i as isize - self.app.ia_selected_cursor as isize;
                    self.app.move_vertical(delta);
                    if double {
                        self.app.accept(&mut self.browser);
                    }
                }
                Some(Hit::Ia(focus)) => {
                    self.app.ia_focus = focus;
                    self.app.accept(&mut self.browser);
                }
                Some(Hit::Row(i)) => {
                    self.app
                        .set_selected_row_index(self.app.current_screen(), i);
                    self.app.accept(&mut self.browser);
                }
                None if self.app.current_screen() == ScreenId::Gate => self.app.unlock_gate(),
                _ => {}
            }
            if was_notice && !self.app.mp_needs_notice {
                if let Some(token) = self.browser.inner.begin_refresh(false) {
                    self.browser
                        .inner
                        .finish_refresh_ok(token, Vec::new(), 1000);
                }
                self.browser.trigger_refresh();
                self.sync_page_browser();
            }
            return self.flush_events(settings, audio_router);
        }
        if self.is_interface_options_active() {
            self.sync_options_controller(settings);
            let vw = self.size[0].max(1) as f64;
            let vh = self.size[1].max(1) as f64;
            let ev = self.options_ctrl.input.on_pointer_down(
                &mut self.options_ctrl.stack,
                &self.options_hits,
                x_px,
                y_px,
                vw,
                vh,
            );
            self.options_ctrl.apply_event(ev);
            self.options_ctrl.input.on_pointer_up();
            self.flush_options_controller(settings);
            return self.flush_events(settings, audio_router);
        }
        self.handle_cursor_move(x_px, y_px);
        let w = self.size[0].max(1) as f64;
        let h = self.size[1].max(1) as f64;
        let ix = (x_px / w) * 1024.0;
        let iy = (y_px / h) * 768.0;

        let (_, _, height) = bfe_shell::gate::gate_layout(w, h);
        let local_x = ix - 512.0;
        let local_y = (y_px / h - 0.5) * height;
        if self.app.popup.is_some()
            || self.app.profile.editing
            || self.app.current_screen() == ScreenId::DirectConnect
        {
            // Input lines retain focus; only the visible action row submits/cancels.
            if let Some(PopupState::HostClosePrompt { selected, .. }) = &mut self.app.popup {
                let pitch = self.fonts.medium.height + 4.0;
                let row = ((local_y - 10.0) / pitch).round() as isize;
                if !(0..3).contains(&row) || local_x.abs() > 240.0 {
                    return None;
                }
                *selected = row as usize;
            } else if (local_y - 63.0).abs() > 24.0 || local_x.abs() > 180.0 {
                return None;
            }
            if local_x > 5.0
                && (self.app.profile.editing
                    || self.app.current_screen() == ScreenId::DirectConnect)
            {
                self.app.back();
                return self.flush_events(settings, audio_router);
            }
            if let Some(PopupState::ConfirmExit { selected_yes, .. }) = &mut self.app.popup {
                *selected_yes = local_x < 5.0;
            }
        } else if self.app.current_screen() != ScreenId::Gate
            && iy < 690.0
            && self.app.current_screen() != ScreenId::InstantAction
            && bfe_shell::render::hit_player_menu(&self.app, &self.fonts, w, h, x_px, y_px)
                .is_none()
        {
            return None;
        }

        if self.app.popup.is_none()
            && !self.app.profile.editing
            && self.app.current_screen() != ScreenId::Gate
            && iy >= 690.0
            && ix < 220.0
        {
            self.app.back();
            return self.flush_events(settings, audio_router);
        }

        let was_notice = self.app.mp_needs_notice;
        let prev_screen = self.app.current_screen();
        let refresh_requested = matches!(prev_screen, ScreenId::MultiplayerList(_))
            && self
                .app
                .active_rows()
                .get(self.app.selected_row_index(prev_screen))
                .is_some_and(|row| row.id == "refresh");
        if self.app.current_screen() == ScreenId::Gate {
            self.app.unlock_gate();
        } else {
            self.app.accept(&mut self.browser);
        }
        if !matches!(prev_screen, ScreenId::Options | ScreenId::OptionsPage(_))
            && matches!(
                self.app.current_screen(),
                ScreenId::Options | ScreenId::OptionsPage(_)
            )
        {
            self.options_ctrl.stack.clear();
            self.sync_options_controller(settings);
        }
        let entered_list = prev_screen != self.app.current_screen()
            && matches!(self.app.current_screen(), ScreenId::MultiplayerList(_));
        if (was_notice && !self.app.mp_needs_notice)
            || refresh_requested
            || (entered_list && self.browser.rx.is_none())
        {
            self.browser.trigger_refresh();
            self.sync_page_browser();
        }
        self.flush_events(settings, audio_router)
    }

    pub fn handle_wheel(&mut self, x_px: f64, y_px: f64, dy: f64, settings: &mut AppSettingsStore) {
        if self.is_interface_options_active() {
            let event = self
                .options_ctrl
                .input
                .on_wheel(&mut self.options_ctrl.stack, if dy < 0. { -1 } else { 1 });
            self.options_ctrl.apply_event(event);
            self.flush_options_controller(settings);
        } else if self.app.current_screen() == ScreenId::InstantAction {
            self.page
                .scroll_ia(x_px, y_px, if dy < 0. { -1 } else { 1 }, &self.app);
        } else if self.browser_page() {
            self.page_browser_key(if dy < 0. { "ArrowUp" } else { "ArrowDown" });
        }
    }

    /// Uploads changed quads from `self.batch` to `self.hud` and renders them into
    /// `self.canvas.framebuffer`.
    ///
    /// ## GPU Upload and Draw Pass
    /// - Copies the dirty quad vertex range (`start..end`) from `self.batch` (`position`,
    ///   `uv`, `mask_uv`, `tint`) into `self.hud`.
    /// - Encodes up to `MAX_GROUPS` textured/masked draw groups into `self.hud.input`.
    /// - Binds `self.canvas.framebuffer`, clears color/depth, and invokes `self.hud.draw(&mut self.core)`.
    pub fn upload_and_draw_batch(&mut self) {
        self.draw_range(0, self.batch.count, true);
    }
    fn draw_range(&mut self, first: usize, last: usize, clear: bool) {
        {
            let b = &self.batch;
            let hud = &mut self.hud;
            // Page composites base and modal ranges and reuses animated row
            // snapshots. Keep the small shell mirror complete across those
            // passes; the battle HUD retains its independent dirty-span path.
            let (start, end) = (0, (b.count * 4).min(QUADS * 4));
            if end > start {
                hud.position[start * 3..end * 3].copy_from_slice(&b.position[start * 3..end * 3]);
                hud.uv[start * 2..end * 2].copy_from_slice(&b.uv[start * 2..end * 2]);
                hud.mask_uv[start * 2..end * 2].copy_from_slice(&b.mask_uv[start * 2..end * 2]);
                hud.tint[start * 4..end * 4].copy_from_slice(&b.tint[start * 4..end * 4]);
            }
            hud.input[IN_UPLOAD] = start as f64;
            hud.input[IN_UPLOAD + 1] = end as f64;
            let mut groups = 0;
            for draw in &b.draws {
                let lo = draw.first.max(first * 6);
                let hi = (draw.first + draw.count).min(last * 6);
                if hi <= lo || groups == MAX_GROUPS {
                    continue;
                }
                let o = IN_FIRST_GROUP + groups * GROUP;
                groups += 1;
                hud.input[o] = lo as f64;
                hud.input[o + 1] = (hi - lo) as f64;
                hud.input[o + 2] = draw.texture as f64;
                hud.input[o + 3] = if draw.mask == 0 {
                    draw.texture
                } else {
                    draw.mask
                } as f64;
                hud.input[o + 4] = u8::from(draw.mask != 0) as f64;
            }
            hud.input[IN_GROUPS] = groups as f64;
        }

        unsafe {
            let gl = &self.core.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.disable(glow::SCISSOR_TEST);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            if clear {
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            }
        }
        // The blur compositor and window presentation issue raw GL calls.
        // Re-establish the HUD phase's state rather than trusting that cache.
        self.core.state.reset();
        self.hud.draw(&mut self.core);
        self.frames += 1;
    }

    /// Reads the current `self.canvas.framebuffer` pixels (bottom-up RGBA8).
    ///
    /// Binds `self.canvas.framebuffer`, sets `GL_PACK_ALIGNMENT = 1`, reads `width * height * 4`
    /// RGBA8 bytes via `glReadPixels`, and verifies `glGetError() == GL_NO_ERROR`.
    pub fn pixels(&self) -> Result<Vec<u8>, String> {
        let gl = &self.core.gl;
        let mut bytes = vec![0u8; self.size[0] as usize * self.size[1] as usize * 4];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                self.size[0],
                self.size[1],
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bytes)),
            );
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("shell GL error {error}"));
            }
        }
        Ok(bytes)
    }

    /// Resizes the front-end framebuffer and HUD viewport.
    ///
    /// Recreates the offscreen `Canvas` FBO at `new_size`, updates `IN_VIEWPORT` on
    /// `self.hud`, and updates `self.interface_surface` and `self.options_ctrl.context`
    /// so `bfe-interface` screens re-layout for the new aspect ratio.
    pub fn resize(
        &mut self,
        new_size: [i32; 2],
        mut make_gl: impl FnMut() -> glow::Context,
    ) -> Result<(), String> {
        if new_size[0] <= 0 || new_size[1] <= 0 || new_size == self.size {
            return Ok(());
        }
        self.size = new_size;
        self.blur.resize(&self.core.gl, new_size);
        self.core.state.reset();
        self.canvas = Canvas::new(make_gl(), new_size)?;
        self.core.canvas_framebuffer = Some(self.canvas.framebuffer);
        self.hud.input[IN_VIEWPORT] = new_size[0] as f64;
        self.hud.input[IN_VIEWPORT + 1] = new_size[1] as f64;
        self.interface_surface = shell_textures_to_interface_surface(&self.textures, new_size);
        self.options_ctrl.context.surface_height = self.interface_surface.height;
        self.options_ctrl.context.viewport =
            Some([new_size[0].max(1) as u32, new_size[1].max(1) as u32]);
        self.options_ctrl.refresh_active_screen();
        self.frames = 0;
        Ok(())
    }

    /// Compiles the `post-hud/hud` shader program, uploads the static quad index buffer,
    /// and initializes the 3D perspective camera uniforms (`FOV = 30 deg`) on `hud`.
    ///
    /// ## Shader Uniform Layout (`post-hud/hud`)
    /// - `map`: Primary glyph/UI texture sampler (`slots[0]`).
    /// - `mask`: Secondary alpha mask texture sampler (`slots[1]`).
    /// - `masked`: `1.0` when `draw.mask != 0`, `0.0` otherwise (`slots[2]`).
    /// - `projectionMatrix`: 30-degree vertical FOV perspective matrix (`CoordinateSystem::WebGL`).
    /// - `modelViewMatrix`: Identity 4x4 view matrix (`root_anchor` places UI quads at `z = -h / (2 * tan(15 deg))`).
    fn init_hud_pass(
        core: &mut Core<glow::Context, Ranges>,
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

    /// Uploads font atlas pages from `data.pages` and constructs [`ShellFonts`] (`large`, `medium`, `small`, `tiny`).
    ///
    /// Iterates over `data.pages` to upload each font atlas PNG once, then converts
    /// the 11-element glyph metric rows (`[code, page_idx, advance, x0, x1, y0, y1, u0, v0, u1, v1]`)
    /// for `gamefont_large`, `gamefont_medium`, `gamefont_small`, and `gamefont_tiny`.
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
                        .and_then(|p| page_handles.get(p.as_str()))
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

    /// Creates a linear-filtered `CLAMP_TO_EDGE` 2D OpenGL texture from raw RGBA8 bytes.
    pub(super) fn upload_page_font(
        gl: &glow::Context,
        rgba: &[u8],
    ) -> Result<glow::Texture, String> {
        let texture = Self::upload_rgba(gl, 1024, 1024, rgba)?;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.generate_mipmap(glow::TEXTURE_2D);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );
        }
        Ok(texture)
    }

    pub(super) fn upload_rgba(
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
}

/// Helper to pump headless audio when no `cpal` stream is open.
pub fn pump_headless_audio(
    router: &SharedAudioRouter,
    status: &AudioStatus,
    scratch: &mut [f32; ::audio::FRAMES * 2],
) {
    // Advance 6 stereo 128-frame quantums per 60 Hz video frame (~46.08 kHz equivalent).
    for _ in 0..6 {
        router.pump_headless(status, scratch);
    }
}

impl Drop for ShellHost {
    fn drop(&mut self) {
        self.blur.dispose(&self.core.gl);
    }
}
