//! Front-end shell host (`bfe-shell` + `bfe-online`) and loading screen renderer.
//!
//! Owns the 2D/3D-anchored interface pass (`Core` + `HudPass` + `Batch`),
//! `ShellApp`, `ShellFonts`, `ShellTextures`, `LoadingTimeline`, and the
//! non-blocking background server browser (`LiveServerBrowser`).

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
    launch::BattleRequest,
    loading::{planet_name, LoadingTimeline, LookId},
    render::{draw_loading, draw_shell, ShellFonts, ShellTextures, StageTexture},
    screens::{IaFocus, MultiplayerLaunch, PopupState, ScreenId, ShellAction, ShellApp},
    version::{AboutInfo, DEFAULT_SIMULATION},
};
use glow::HasContext;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};
use three::{CoordinateSystem, Matrix4};

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

/// Transition requested by the front-end shell.
#[derive(Clone, Debug)]
pub enum ShellTransition {
    /// Enter an Instant Action battle (`BattleRequest` with optional playlist).
    StartInstantAction(BattleRequest),
    /// Join a remote or LAN server (`MultiplayerLaunch`).
    StartOnline(MultiplayerLaunch),
    /// Close the application (`EXIT TO WINDOWS` confirmed).
    Exit,
}

/// Non-blocking wrapper around [`bfe_online::ServerBrowser`] that executes
/// master server HTTP queries, LAN UDP broadcast discovery, and TCP/info probes
/// on background worker threads so the 60 Hz render loop never stalls.
type BrowserRefreshMessage = (u64, Result<Vec<ServerEntry>, String>);

pub struct LiveServerBrowser {
    pub inner: ServerBrowser,
    rx: Option<mpsc::Receiver<BrowserRefreshMessage>>,
    server_target: Option<String>,
}

impl LiveServerBrowser {
    pub fn new(server_target: Option<String>, _privacy_accepted: bool) -> Self {
        Self {
            inner: ServerBrowser::default(),
            rx: None,
            server_target,
        }
    }

    pub fn server_target(&self) -> Option<&str> {
        self.server_target.as_deref()
    }

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
    fn source(&self) -> Source {
        self.inner.source()
    }

    fn set_source(&mut self, source: Source) -> bool {
        let changed = self.inner.set_source(source);
        if changed {
            self.trigger_refresh();
        }
        changed
    }

    fn filters(&self) -> &Filters {
        self.inner.filters()
    }

    fn set_filter(&mut self, update: FilterUpdate) -> bool {
        self.inner.set_filter(update)
    }

    fn clear_filters(&mut self) {
        self.inner.clear_filters();
    }

    fn sort_spec(&self) -> SortSpec {
        self.inner.sort_spec()
    }

    fn sort_by(&mut self, column: SortColumn) {
        self.inner.sort_by(column);
    }

    fn rows(&self) -> Vec<&ServerEntry> {
        self.inner.rows()
    }

    fn selected(&self) -> Option<&ServerEntry> {
        self.inner.selected()
    }

    fn select(&mut self, server_id: Option<&str>) {
        self.inner.select(server_id);
    }

    fn key(&mut self, key: BrowserKey, shown_rows: usize) -> bool {
        self.inner.key(key, shown_rows)
    }

    fn describe(&self, server: &ServerEntry, maps: &[MapCatalogEntry]) -> GameInfo {
        self.inner.describe(server, maps)
    }

    fn state(&self) -> &BrowserState {
        self.inner.state()
    }

    fn needs_refresh(&self, now_ms: u64) -> Option<RefreshKind> {
        self.inner.needs_refresh(now_ms)
    }
}

/// Front-end renderer and controller for the startup loader, click gate, and shell menus.
pub struct ShellHost {
    pub canvas: Canvas,
    pub core: Core<glow::Context, Ranges>,
    pub hud: HudPass<glow::Context>,
    pub batch: Batch,
    pub fonts: ShellFonts,
    pub textures: ShellTextures,
    pub app: ShellApp,
    pub browser: LiveServerBrowser,
    pub loading_timeline: LoadingTimeline,
    pub loading_planet: String,
    pub loading_text: String,
    pub loading_time: f64,
    pub content_dir: Option<PathBuf>,
    pub size: [i32; 2],
    pub frames: u64,
}

impl ShellHost {
    /// Initializes the front-end GL pass with a built-in fallback font so the
    /// startup loader can render immediately before `common.json` is loaded.
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

        let browser = LiveServerBrowser::new(
            server_target,
            settings.player.privacy_notice_accepted,
        );

        Ok(Self {
            canvas,
            core,
            hud,
            batch: Batch::new(),
            fonts,
            textures,
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
    pub fn load_prepared_content(
        &mut self,
        content_dir: &Path,
        settings: &AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Result<(), String> {
        let data = load_shell_data_from_dir(content_dir);
        let mut lib = Library::new(content_dir, false, 4096, 0)?;

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

        let loaded_fonts = Self::load_fonts(&data, &mut upload_path);
        if !loaded_fonts.medium.glyphs.is_empty() {
            self.fonts = loaded_fonts;
        }

        let mut textures = ShellTextures {
            solid: 1,
            by_name: HashMap::new(),
            loading_stages: Vec::new(),
        };

        for &name in NAMED_TEXTURES {
            if let Some(url) = data.textures.get(name) {
                if let Some((handle, _, _)) = upload_path(&url) {
                    textures.by_name.insert(name.to_string(), handle);
                }
            }
        }
        for map in &data.maps {
            let preview_name = format!("map_{}", map.id);
            if let Some(url) = data.textures.get(preview_name.as_str()) {
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

        let about = AboutInfo::from_content_dir(content_dir, Some(env!("CARGO_PKG_VERSION")));
        let mut app = ShellApp::new(data.clone(), Some(&about));
        settings.apply_to_shell_app(&mut app);

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

    /// Configures the loading screen for a specific map (`map_id`) before transitioning
    /// into a battle.
    pub fn prepare_map_loading(&mut self, map_id: &str, audio_router: &SharedAudioRouter) {
        if let Some(map) = self.app.data.maps.iter().find(|m| m.id == map_id) {
            self.loading_planet =
                planet_name(&map.id, &self.app.data.strings, map.load.as_ref());
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
        draw_loading(
            &mut self.batch,
            &self.fonts,
            &self.textures,
            &at,
            timeline_time,
            &planet,
            &text,
            LookId::Bf1,
            w,
            h,
        );
        self.upload_and_draw_batch();
    }

    /// Renders a map-loading frame (`draw_loading`) while transitioning into a battle.
    pub fn draw_battle_loading(&mut self, dt: f64) {
        self.loading_time += dt.clamp(0.0, 0.1);
        let at = self.loading_timeline.at(self.loading_time);
        let w = self.size[0] as f64;
        let h = self.size[1] as f64;
        draw_loading(
            &mut self.batch,
            &self.fonts,
            &self.textures,
            &at,
            self.loading_time,
            &self.loading_planet,
            &self.loading_text,
            LookId::Bf1,
            w,
            h,
        );
        self.upload_and_draw_batch();
    }

    /// Updates and renders the active shell screen (`Gate`, `Main`, `InstantAction`,
    /// `Options`, `Profile`, `Multiplayer`, etc.).
    pub fn step_and_draw(
        &mut self,
        dt: f64,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        if self.browser.poll() {
            self.app.sync_from_browser(&self.browser);
        }
        if dt.is_finite() && dt > 0.0 {
            self.app.gate_elapsed += dt;
            self.app.pulse_elapsed += dt;
        }
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
        self.upload_and_draw_batch();
        self.flush_events(settings, audio_router)
    }

    /// Flushes queued shell sound cues, media transitions, settings changes,
    /// and screen transition actions.
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
                    if mp.server_id == "host:local" {
                        self.app.popup = Some(PopupState::Notice {
                            message: "HOSTING IS NOT AVAILABLE YET.".to_string(),
                        });
                        return None;
                    }
                    return Some(ShellTransition::StartOnline(mp));
                }
                ShellAction::ExitToWindows => {
                    return Some(ShellTransition::Exit);
                }
            }
        }
        None
    }

    /// Handles a keyboard input event in the front-end shell.
    pub fn handle_key(
        &mut self,
        key: &str,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        let was_notice = self.app.mp_needs_notice;
        self.app
            .handle_key(key, false, false, false, &mut self.browser);
        if was_notice && !self.app.mp_needs_notice {
            self.browser.trigger_refresh();
            self.app.sync_from_browser(&self.browser);
        }
        self.flush_events(settings, audio_router)
    }

    /// Updates hover focus when the mouse cursor moves to `(x_px, y_px)`.
    pub fn handle_cursor_move(&mut self, x_px: f64, y_px: f64) {
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

        let rows = self.app.rows_for(screen);
        if rows.is_empty() {
            return;
        }
        let p = 25.0;
        let n = rows.len() as f64;
        let center_y = 768.0 * 0.5 + 20.0;
        let y0 = center_y - (n * p) * 0.5;
        if (240.0..=784.0).contains(&ix) && iy >= y0 - p * 0.5 && iy <= y0 + n * p {
            let k = (((iy - (y0 - p * 0.5)) / p).floor() as isize).clamp(0, rows.len() as isize - 1)
                as usize;
            self.app.set_selected_row_index(screen, k);
        }
    }

    /// Handles a primary mouse button click at `(x_px, y_px)`.
    pub fn handle_mouse_click(
        &mut self,
        x_px: f64,
        y_px: f64,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        self.handle_cursor_move(x_px, y_px);
        let w = self.size[0].max(1) as f64;
        let h = self.size[1].max(1) as f64;
        let ix = (x_px / w) * 1024.0;
        let iy = (y_px / h) * 768.0;

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
        if self.app.current_screen() == ScreenId::Gate {
            self.app.unlock_gate();
        } else {
            self.app.accept(&mut self.browser);
        }
        if was_notice && !self.app.mp_needs_notice {
            self.browser.trigger_refresh();
            self.app.sync_from_browser(&self.browser);
        }
        self.flush_events(settings, audio_router)
    }

    /// Uploads changed quads from `self.batch` to `self.hud` and renders them into
    /// `self.canvas.framebuffer`.
    pub fn upload_and_draw_batch(&mut self) {
        {
            let b = &self.batch;
            let hud = &mut self.hud;
            let (start, end) = if self.frames == 0 {
                (0, (b.count * 4).min(QUADS * 4))
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
        self.frames += 1;
    }

    /// Reads the current `self.canvas.framebuffer` pixels (bottom-up RGBA8).
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
    pub fn resize(&mut self, new_size: [i32; 2], mut make_gl: impl FnMut() -> glow::Context) -> Result<(), String> {
        if new_size[0] <= 0 || new_size[1] <= 0 || new_size == self.size {
            return Ok(());
        }
        self.size = new_size;
        self.canvas = Canvas::new(make_gl(), new_size)?;
        self.core.canvas_framebuffer = Some(self.canvas.framebuffer);
        self.hud.input[IN_VIEWPORT] = new_size[0] as f64;
        self.hud.input[IN_VIEWPORT + 1] = new_size[1] as f64;
        self.frames = 0;
        Ok(())
    }

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
}

/// Helper to pump headless audio when no `cpal` stream is open.
pub fn pump_headless_audio(
    router: &SharedAudioRouter,
    status: &AudioStatus,
    scratch: &mut [f32; ::audio::FRAMES * 2],
) {
    for _ in 0..6 {
        router.pump_headless(status, scratch);
    }
}
