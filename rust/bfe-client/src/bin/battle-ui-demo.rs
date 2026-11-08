//! Interactive and screenshot harness for `bfe-battle-ui` over the native map
//! inspection scene (`bfe_client::scene::Scene::load`).
//!
//! Usage:
//!   battle-ui-demo CONTENT_DIR --map nab2 [--screenshot FILE] [--screenshot-all DIR] [--mode MODE]
use bfe_battle_ui::{
    hud_input::ChatController, pause::{ConfirmKind, PauseMenu, PauseRespawnConfig}, scoreboard::{
        BoardPage, CharacterRecord, CharacterStatsEntry, MatchStateFlags, ScoreboardAction,
        ScoreboardController, ScoreboardPort, SlotRecord, TeamHeader, TimedEntry, WorldScoreboardPort, },
    shapes::{ interface_height, root_anchor, to_interface, BattleUiAssets, GameFamily, HudBatchSurface, }, spawn::{
        CameraShot, HumanSpawnFacts, MapProjection, OpenReason, SpawnClassEntry, SpawnInputAction,
        SpawnPort, SpawnPostEntry, SpawnRequest, SpawnScreenController, SpawnTeamFacts, }, };
use bfe_client::{ host::Host, render_bridge::Bridge, resources::{Ranges, Resources}, scene::{MapCamera, Scene}, };
use bfe_content_native::{loader::authored_json, BattleContent, Loader};
use bfe_render::{
    hud as render_hud, hud_model::{ self, layout::{HudArt as A, HudFrame as H, HudGlyph as G, HudPost as P, HudTeam as T}, },
    interface::{alpha, translate, Batch, Color, Draw, UV}, };
use glow::HasContext;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

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
use glutin::{
    config::ConfigTemplateBuilder, context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay}, prelude::*, surface::{Surface, SwapInterval, WindowSurface}, };
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{ collections::{BTreeMap, HashMap}, num::NonZeroU32, path::{Path, PathBuf}, time::{Duration, Instant}, };
use three::{CoordinateSystem, Matrix4};
use winit::{
    application::ApplicationHandler, dpi::PhysicalSize, event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop}, keyboard::{KeyCode, PhysicalKey}, window::{Window, WindowId}, };
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DemoMode { Spawn, ScoreboardTable, ScoreboardDetails, ScoreboardAwards, PauseMain, PauseConfirm, }
impl DemoMode {
    fn from_arg(s: &str) -> Result<Self, String> {
        match s {
            "spawn" => Ok(Self::Spawn), "scoreboard" | "table" => Ok(Self::ScoreboardTable),
            "details" | "scoreboard-details" => Ok(Self::ScoreboardDetails), "awards" | "scoreboard-awards" => Ok(Self::ScoreboardAwards),
            "pause" => Ok(Self::PauseMain), "pause-confirm" | "confirm" => Ok(Self::PauseConfirm),
            other => Err(format!("unknown --mode '{other}'")), } }
    fn filename(self) -> &'static str {
        match self {
            Self::Spawn => "spawn.png", Self::ScoreboardTable => "scoreboard.png", Self::ScoreboardDetails => "scoreboard-details.png",
            Self::ScoreboardAwards => "scoreboard-awards.png", Self::PauseMain => "pause.png", Self::PauseConfirm => "pause-confirm.png", }
    } }
struct DemoOptions {
    root: PathBuf, map: String, mode: DemoMode, screenshot: Option<PathBuf>, screenshot_all: Option<PathBuf>, seconds: Option<f64>, desktop_gl: bool,
    size: [i32; 2], }
impl DemoOptions {
    fn parse() -> Result<Self, String> {
        let mut root: Option<PathBuf> = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from);
        let mut map = String::from("nab2");
        let mut mode = DemoMode::Spawn;
        let mut screenshot = None;
        let mut screenshot_all = None;
        let mut seconds = None;
        let mut desktop_gl = false;
        let mut args = std::env::args().skip(1);
        let usage = "usage: battle-ui-demo CONTENT_DIR --map nab2 [--screenshot FILE] [--screenshot-all DIR] [--mode MODE]";
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--map" => map = args.next().ok_or("missing value for --map")?,
                "--mode" => mode = DemoMode::from_arg(&args.next().ok_or("missing value for --mode")?)?,
                "--screenshot" => screenshot = Some(PathBuf::from(args.next().ok_or("missing value for --screenshot")?)),
                "--screenshot-all" => screenshot_all = Some(PathBuf::from(args.next().ok_or("missing value for --screenshot-all")?)),
                "--seconds" => seconds = Some(args.next().ok_or("missing value for --seconds")?.parse::<f64>().map_err(|e| e.to_string())?),
                "--desktop-gl" => desktop_gl = true, "--help" | "-h" => return Err(usage.into()),
                other if other.starts_with('-') => return Err(format!("unknown option {other}")),
                positional => root = Some(PathBuf::from(positional)), } }
        Ok(Self { root: root.ok_or(usage)?, map, mode, screenshot, screenshot_all, seconds, desktop_gl, size: [1280, 720] }) } }
const FIXED_HUD_ART: [&str; 15] = [ "hud1", "hud2", "hud_health", "hud_vehiclehealth", "hud_flag_timer",
    "hud_flag_icon", "player_icon", "map_mask", "ambush_icon", "hud_jetpack_icon",
    "troop_icon", "hit_indicator", "stats_kills", "stats_deaths", "stats_cpp", ];
const SAMPLING_UI: &str = r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#;
struct DemoSpawnPort {
    human: HumanSpawnFacts, teams: [SpawnTeamFacts; 2], rosters: [Vec<SpawnClassEntry>; 2], posts: Vec<SpawnPostEntry>, }
impl DemoSpawnPort {
    fn from_map(world: &serde_json::Value, common: &serde_json::Value) -> Self {
        let mut posts = Vec::new();
        if let Some(cp_arr) = world["commandPosts"].as_array() {
            for (idx, cp) in cp_arr.iter().enumerate() {
                let name = cp["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("cp_{idx}"));
                let display_name = cp["displayName"].as_str().or_else(|| cp["label"].as_str())
                    .map(str::to_string).unwrap_or_else(|| format!("COMMAND POST {}", idx + 1));
                let x = cp["matrix"][12].as_f64().unwrap_or(0.0);
                let y = cp["matrix"][13].as_f64().unwrap_or(0.0);
                let z = cp["matrix"][14].as_f64().unwrap_or(0.0);
                let raw_team = cp["team"].as_u64().unwrap_or(0) as u8;
                let owner = if raw_team == 1 || raw_team == 2 { raw_team } else if idx % 2 == 0 { 1 } else { 2 };
                let vehicles = if idx == 0 { vec!["TX-130 SABER TANK".into(), "74-Z SPEEDER BIKE".into()] } else { Vec::new() };
                posts.push(SpawnPostEntry { name, display_name: display_name.to_uppercase(), position: [x, y, z], owner, eligible: true, vehicles });
            } }
        if posts.is_empty() {
            posts.push(SpawnPostEntry { name: "cp1".into(), display_name: "PALACE PLAZA".into(), position: [-40.0, 0.0, -40.0], owner: 1, eligible: true, vehicles: vec!["TX-130 SABER TANK".into()] });
            posts.push(SpawnPostEntry { name: "cp2".into(), display_name: "THEED HANGAR".into(), position: [55.0, 0.0, 45.0], owner: 2, eligible: true, vehicles: Vec::new() });
        }
        let objective_text = common["strings"]["ifs.spawndisplay.spawnprompt"]
            .as_str().unwrap_or("ELIMINATE ENEMY REINFORCEMENTS OR CAPTURE ALL COMMAND POSTS").to_uppercase();
        let make_class = |class_name: &str, display_name: &str, weapons: &[&str]| SpawnClassEntry {
            class_name: class_name.into(), display_name: display_name.into(),
            weapons: weapons.iter().map(|s| (*s).to_string()).collect(), eligible: true, };
        let rep_roster = vec![
            make_class("rep_inf_clone_trooper", "CLONE TROOPER", &["BLASTER RIFLE", "BLASTER PISTOL", "THERMAL DETONATOR (4)"]),
            make_class("rep_inf_arc_trooper", "ARC TROOPER", &["MISSILE LAUNCHER", "BLASTER PISTOL", "THERMAL DETONATOR (4)", "MINES (4)"]),
            make_class("rep_inf_clone_pilot", "CLONE PILOT", &["BOLT CASTER", "BLASTER PISTOL", "FUSION CUTTER", "HEALTH/AMMO DISPENSER (5)"]),
            make_class("rep_inf_clone_sharpshooter", "CLONE SHARPSHOOTER", &["SNIPER RIFLE", "BLASTER PISTOL", "THERMAL DETONATOR (4)", "RECON DROID"]),
            make_class("rep_inf_jet_trooper", "JET TROOPER", &["EMP LAUNCHER", "COMMANDO PISTOL", "THERMAL DETONATOR (4)"]), ];
        let cis_roster = vec![ make_class("cis_inf_battledroid", "SUPER BATTLE DROID", &["WRIST BLASTER", "TRI-SHOT", "WRIST ROCKET (3)"]),
            make_class("cis_inf_assault", "ASSAULT DROID", &["MISSILE LAUNCHER", "BLASTER PISTOL", "THERMAL DETONATOR (4)", "MINES (4)"]),
            make_class("cis_inf_pilotdroid", "DROID PILOT", &["RADIATION LAUNCHER", "BLASTER PISTOL", "FUSION CUTTER", "HEALTH/AMMO DISPENSER (5)"]),
            make_class("cis_inf_sniper", "ASSASSIN DROID", &["SNIPER RIFLE", "BLASTER PISTOL", "THERMAL DETONATOR (4)", "RECON DROID"]),
            make_class("cis_inf_droideka", "DROIDEKA", &["REPEATING BLASTERS", "SHIELD EMITTER"]), ];
        Self {
            human: HumanSpawnFacts::default(), teams: [
                SpawnTeamFacts { side: "rep".into(), name: "REPUBLIC".into(), logo: "com_icon_republic".into(), objectives: vec![objective_text.clone()], reinforcements: 142 },
                SpawnTeamFacts { side: "cis".into(), name: "CIS".into(), logo: "com_icon_cis".into(), objectives: vec![objective_text], reinforcements: 118 },
            ], rosters: [rep_roster, cis_roster], posts, } } }
impl SpawnPort for DemoSpawnPort { fn human_state(&self) -> HumanSpawnFacts { self.human.clone() }
    fn team_facts(&self, team: u8) -> Option<SpawnTeamFacts> {
        match team { 1 => Some(self.teams[0].clone()), 2 => Some(self.teams[1].clone()), _ => None } }
    fn roster(&self, team: u8) -> Vec<SpawnClassEntry> {
        match team { 1 => self.rosters[0].clone(), 2 => self.rosters[1].clone(), _ => Vec::new() } }
    fn posts(&self, team: u8) -> Vec<SpawnPostEntry> {
        self.posts.iter().map(|p| { let mut e = p.clone(); e.eligible = p.owner == team; e }).collect() }
    fn request(&mut self, choice: &SpawnRequest) {
        self.human.team = choice.team;
        self.human.class_name = Some(choice.class_name.clone());
        self.human.post = Some(choice.post.clone()); }
    fn suicide(&mut self) {}
    fn cancel(&mut self) {}
    fn camera_shots(&self) -> Vec<CameraShot> { Vec::new() }
    fn set_view_shot(&mut self, _shot: Option<CameraShot>) {} }
fn build_demo_scoreboard_port() -> WorldScoreboardPort {
    let mut port = WorldScoreboardPort::new();
    port.family = GameFamily::Swbf1;
    port.network = true;
    port.human_team = 1;
    port.state = MatchStateFlags { over: true, game_over: true, finished: true };
    port.teams = [
        Some(TeamHeader { name: "REPUBLIC".into(), reinforcements: 142, side: Some("rep".into()), logo: Some("com_icon_republic".into()) }),
        Some(TeamHeader { name: "CIS".into(), reinforcements: 118, side: Some("cis".into()), logo: Some("com_icon_cis".into()) }), ];
    let player_slot0 = SlotRecord {
        team_kills: 0, vehicles_destroyed: 4, spree: 2, survivalist: 3, life_kills: 8, life_kills_best: 9,
        headshots: 6, deaths: 3, life: 95.0, life_best: 194.0, camp: 12.0, camp_best: 22.0, escapes: 3,
        victims: BTreeMap::from([(6, 5), (7, 4), (8, 3), (9, 2)]),
        classes: vec![TimedEntry { key: "rep_inf_clone_trooper".into(), label: Some("CLONE TROOPER".into()), time: 240.0 }],
        weapons: vec![TimedEntry { key: "rep_weap_inf_rifle".into(), label: Some("BLASTER RIFLE".into()), time: 210.0 }],
        vehicles: vec![TimedEntry { key: "rep_hover_fightertank".into(), label: Some("TX-130 SABER TANK".into()), time: 85.0 }], };
    #[allow(clippy::too_many_arguments)]
    let make_entry = |slot: usize, team: i32, name: &str, kills: i32, tk: i32, deaths: i32, points: i32, human: bool, local: bool, ping: Option<i32>, link: &str, vd: i32, spree: i32, camp: f64, surv: i32, lk: i32, hs: i32, victim: Option<(usize, i32)>| -> CharacterStatsEntry {
        let mut victims = BTreeMap::new();
        if let Some((v_slot, count)) = victim { victims.insert(v_slot, count); }
        let s0 = if local { player_slot0.clone() } else {
            SlotRecord {
                team_kills: tk, vehicles_destroyed: vd, spree, survivalist: surv, life_kills: lk, life_kills_best: lk,
                headshots: hs, deaths, life: 60.0, life_best: 140.0, camp, camp_best: camp, escapes: surv, victims,
                classes: vec![TimedEntry { key: "rep_inf_arc_trooper".into(), label: Some("ARC TROOPER".into()), time: 180.0 }],
                weapons: vec![TimedEntry { key: "rep_weap_inf_torpedo".into(), label: Some("MISSILE LAUNCHER".into()), time: 160.0 }],
                vehicles: Vec::new(), } };
        CharacterStatsEntry {
            slot, team, name: name.into(), kills, team_kills: tk, deaths, points, human, local, ping, link_kind: link.into(),
            record: CharacterRecord { slots: [s0, SlotRecord::default()], anchor: None, health: 255, low: false }, } };
    port.characters = vec![ make_entry(0, 1, "Player 1", 14, 0, 3, 24, true, true, Some(18), "direct4", 4, 2, 22.0, 3, 9, 6, None),
        make_entry(1, 1, "CC-2224 Cody", 11, 0, 4, 21, true, false, Some(34), "direct6", 6, 1, 18.0, 2, 7, 4, None),
        make_entry(2, 1, "CT-7567 Rex", 9, 0, 3, 18, true, false, Some(42), "relay4", 2, 3, 15.0, 5, 6, 8, None),
        make_entry(3, 1, "CT-5555 Fives", 7, 1, 6, 14, false, false, None, "", 1, 0, 30.0, 1, 4, 2, None),
        make_entry(4, 1, "CT-1409 Echo", 6, 0, 5, 12, false, false, None, "", 0, 0, 48.0, 1, 4, 1, None),
        make_entry(5, 1, "RC-1138 Boss", 6, 0, 4, 11, false, false, None, "", 1, 1, 10.0, 2, 5, 3, None),
        make_entry(6, 2, "OOM-9", 10, 0, 5, 19, true, false, Some(29), "direct4", 3, 2, 25.0, 2, 6, 5, Some((0, 2))),
        make_entry(7, 2, "B2-4410", 8, 0, 6, 16, false, false, None, "", 2, 1, 95.0, 1, 5, 2, Some((0, 1))),
        make_entry(8, 2, "IG-102", 6, 0, 8, 13, false, false, None, "", 1, 0, 20.0, 1, 3, 1, None),
        make_entry(9, 2, "BX-0918", 5, 0, 7, 11, false, false, None, "", 0, 0, 15.0, 1, 3, 2, None),
        make_entry(10, 2, "B1-2688", 4, 2, 9, 8, false, false, None, "", 0, 0, 60.0, 0, 2, 0, None),
        make_entry(11, 2, "B1-7104", 3, 0, 11, 6, false, false, None, "", 0, 0, 40.0, 0, 2, 0, None), ];
    port }
fn upload_rgba_texture(resources: &mut Resources, bridge: &mut Bridge, width: u32, height: u32, pixels: &[u8]) -> Result<u32, String> {
    let spec = serde_json::json!({ "width": width, "height": height, "depth": 1, "target": glow::TEXTURE_2D, "format": 1023, "dataType": 1009, "srgb": false });
    let src = resources.source(&spec.to_string())?;
    resources.source_pixels(src, pixels)?;
    let smp = resources.sample(src, SAMPLING_UI)?;
    resources.upload(smp)?;
    Ok(bridge.r.core.add_texture(resources.texture(smp)?) as u32) }
struct BattleUiOverlay {
    bridge: Bridge,
    #[allow(dead_code)]
    resources: Resources, texture_map: HashMap<String, u32>, string_map: HashMap<String, String>, spawn_port: DemoSpawnPort,
    spawn_ctrl: SpawnScreenController, scoreboard_port: WorldScoreboardPort, scoreboard_ctrl: ScoreboardController, pause_menu: PauseMenu,
    pause_settings: BTreeMap<String, String>,
    #[allow(dead_code)]
    chat: ChatController, mode: DemoMode, size: [i32; 2], height: f64, aspect: f64, cursor_px: [f64; 2], dirty: bool,
}
impl BattleUiOverlay {
    fn load( root: &Path, map: &str, size: [i32; 2], canvas_fb: glow::Framebuffer, mut make_gl: impl FnMut() -> glow::Context,
        initial_mode: DemoMode, ) -> Result<Self, String> {
        let mut loader = Loader::new(root);
        loader.compressed = false;
        let mut content = loader.load(map, &format!("map={map}&seed=1"))?;
        let mut resources = Resources::new(make_gl());
        resources.canvas = Some(canvas_fb);
        let mut bridge = Bridge::new(make_gl(), Ranges::new(make_gl()));
        bridge.r.core.canvas_framebuffer = Some(canvas_fb);
        let common = authored_json(&content.content.battle.common);
        let world = authored_json(&content.content.battle.world);
        let mut string_map = HashMap::new();
        if let Some(obj) = common["strings"].as_object() {
            for (k, v) in obj { if let Some(s) = v.as_str() { string_map.insert(k.clone(), s.to_string()); } } }
        let solid_core_tex = upload_rgba_texture(&mut resources, &mut bridge, 1, 1, &[255, 255, 255, 255])?;
        let mut texture_map: HashMap<String, u32> = HashMap::new();
        let mut next_asset_id: u32 = 1;
        bridge.hud_model_asset(0, solid_core_tex as i32, 1.0, 1.0, true);
        let load_named_texture = |name: &str, explicit_id: Option<u32>, content: &mut BattleContent, resources: &mut Resources,
                                  bridge: &mut Bridge, texture_map: &mut HashMap<String, u32>, next_id: &mut u32|
         -> Result<u32, String> {
            let key_lower = name.to_ascii_lowercase();
            if explicit_id.is_none() {
                if let Some(&existing) = texture_map.get(name).or_else(|| texture_map.get(&key_lower)) { return Ok(existing); } }
            let rel_path = common["textures"][name]
                .as_str()
                .or_else(|| common["textures"][&key_lower].as_str())
                .map(|s| format!("common/{s}"))
                .or_else(|| {
                    world["textures"][name]
                        .as_str()
                        .or_else(|| world["textures"][&key_lower].as_str())
                        .map(|s| format!("maps/{map}/{s}"))
                });
            let id = explicit_id.unwrap_or_else(|| {
                let id = *next_id;
                *next_id += 1;
                id
            });
            if id >= *next_id { *next_id = id + 1; }
            if let Some(path) = rel_path {
                if let Ok(img_idx) = content.textures.load(&path) {
                    let img = &content.textures.images[img_idx];
                    if img.format == bfe_render::texture_format::Format::Rgba {
                        let core_tex = upload_rgba_texture(resources, bridge, img.width, img.height, &img.data)?;
                        bridge.hud_model_asset(id, core_tex as i32, img.width as f64, img.height as f64, true);
                        texture_map.insert(name.to_string(), id);
                        texture_map.insert(key_lower, id);
                        return Ok(id); } } }
            bridge.hud_model_asset(id, solid_core_tex as i32, 1.0, 1.0, false);
            Ok(0) };
        for (idx, &art_name) in FIXED_HUD_ART.iter().enumerate() {
            load_named_texture(art_name, Some((idx + 1) as u32), &mut content, &mut resources, &mut bridge, &mut texture_map, &mut next_asset_id)?;
        }
        bridge.hud_model_asset(A::SOLID, solid_core_tex as i32, 1.0, 1.0, true);
        next_asset_id = A::SOLID + 1;
        if let Some(tex_obj) = common["textures"].as_object() {
            let keys: Vec<String> = tex_obj.keys().cloned().collect();
            for k in keys {
                let _ = load_named_texture(&k, None, &mut content, &mut resources, &mut bridge, &mut texture_map, &mut next_asset_id); } }
        for (font_idx, font_name) in ["gamefont_tiny", "gamefont_small", "gamefont_medium", "gamefont_large"].iter().enumerate() {
            let font_json = &common["fonts"][*font_name];
            let mut pages = Vec::new();
            if let Some(page_list) = font_json["pages"].as_array() {
                for page in page_list {
                    let page_id = load_named_texture(page.as_str().unwrap_or(""), None, &mut content, &mut resources, &mut bridge, &mut texture_map, &mut next_asset_id)?;
                    pages.push(page_id); } }
            let mut rows = Vec::new();
            if let Some(glyphs) = font_json["glyphs"].as_array() {
                for g in glyphs {
                    if let Some(arr) = g.as_array().filter(|a| a.len() == G::WORDS) {
                        let mut row: [f64; G::WORDS] = std::array::from_fn(|i| arr[i].as_f64().unwrap_or(0.0));
                        row[G::PAGE] = pages.get(row[G::PAGE] as usize).copied().unwrap_or(A::SOLID) as f64;
                        rows.extend(row); } } }
            let line_h = font_json["lineHeight"].as_f64().unwrap_or(18.0);
            bridge.hud_model_font(font_idx as u32, line_h, &rows); }
        let mut map_proj = MapProjection::default();
        if let Some(map_def) = world.get("minimap").filter(|m| !m.is_null()) {
            let cx = map_def["center"][0].as_f64().unwrap_or(0.0);
            let cz = map_def["center"][1].as_f64().unwrap_or(0.0);
            let half = map_def["half"].as_f64().unwrap_or(256.0);
            let map_scale = world["commandPost"]["mapScale"].as_f64().unwrap_or(2.5);
            let mut map_img_id = 0u32;
            if let Some(tex_rel) = map_def["texture"].as_str() {
                let full_rel = format!("maps/{map}/{tex_rel}");
                if let Ok(img_idx) = content.textures.load(&full_rel) {
                    let img = &content.textures.images[img_idx];
                    if img.format == bfe_render::texture_format::Format::Rgba {
                        let core_tex = upload_rgba_texture(&mut resources, &mut bridge, img.width, img.height, &img.data)?;
                        let id = next_asset_id;
                        bridge.hud_model_asset(id, core_tex as i32, img.width as f64, img.height as f64, true);
                        texture_map.insert("minimap".into(), id);
                        map_img_id = id; } } }
            bridge.hud_model.input[H::MAP] = 1.0;
            bridge.hud_model.input[H::MAP_X] = cx;
            bridge.hud_model.input[H::MAP_Z] = cz;
            bridge.hud_model.input[H::MAP_HALF] = half;
            bridge.hud_model.input[H::MAP_SCALE] = map_scale;
            bridge.hud_model.input[H::MAP_IMAGE] = map_img_id as f64;
            map_proj = MapProjection { map_x: cx, map_z: cz, map_half: half, post_icon_size: 6.0 * map_scale }; }
        let indices: Vec<u32> = (0..render_hud::QUADS as u32)
            .flat_map(|i| [i * 4, i * 4 + 1, i * 4 + 2, i * 4 + 2, i * 4 + 1, i * 4 + 3])
            .collect();
        bridge.hud_init(&indices);
        let program = bridge.add_owned_program("post-hud/hud")?;
        bridge.r.hud.prepare_program(program as usize, bfe_render::programs::spec("post-hud/hud")?.uniforms);
        let mut projection = Matrix4::default();
        let tan = (std::f64::consts::PI / 6.0).tan();
        projection.make_perspective(-tan, tan, 0.75 * tan, -0.75 * tan, 1.0, 10000.0, CoordinateSystem::WebGL);
        {
            let input = &mut bridge.r.hud.input;
            input[render_hud::IN_PROGRAM] = program as f64;
            for (i, m) in [projection, Matrix4::default()].iter().enumerate() {
                let at = render_hud::IN_CAMERA + i * 18;
                input[at] = i as f64;
                input[at + 1] = 16.0;
                input[at + 2..at + 18].copy_from_slice(&m.elements); }
            input[render_hud::IN_SLOTS..render_hud::IN_SLOTS + 3].copy_from_slice(&[2.0, 3.0, 4.0]);
            input[render_hud::IN_VIEWPORT..render_hud::IN_VIEWPORT + 2].copy_from_slice(&[size[0] as f64, size[1] as f64]); }
        let mut spawn_port = DemoSpawnPort::from_map(&world, &common);
        Self::populate_hud_posts_and_teams(&mut bridge.hud_model, &spawn_port, 1);
        let aspect = size[0].max(1) as f64 / size[1].max(1) as f64;
        let height = interface_height(size[0] as f64, size[1] as f64);
        let fh = bridge.hud_model.fonts[0].height;
        let mut spawn_ctrl = SpawnScreenController::try_with_family(GameFamily::Swbf1, height).map_err(|e| e.to_string())?;
        spawn_ctrl.view.map_proj = map_proj;
        spawn_ctrl.open(&mut spawn_port, OpenReason::Start, height);
        for _ in 0..12 { spawn_ctrl.update(0.05, &mut spawn_port, height, fh, false, 0.25); }
        let scoreboard_port = build_demo_scoreboard_port();
        let scoreboard_ctrl = ScoreboardController::try_with_family(GameFamily::Swbf1, height).map_err(|e| e.to_string())?;
        let mut pause_menu = PauseMenu::try_with_family(GameFamily::Swbf1).map_err(|e| e.to_string())?;
        pause_menu.enable_respawn(Some(PauseRespawnConfig { label: None, prompt: None, available: true }));
        pause_menu.show_player_list = true;
        pause_menu.players = scoreboard_port.players().unwrap_or_default();
        let pause_settings = BTreeMap::from([ ("aiDifficulty".into(), "medium".into()), ("aiHeroes".into(), "true".into()),
            ("battleSize".into(), "150".into()), ]);
        let mut chat = ChatController::new("Player 1");
        chat.log.receive("server", "", "Welcome to Battlefront Eternal (Naboo: Theed)", 0.0);
        chat.log.receive("team", "CC-2224 Cody", "Capture the Palace Plaza command post!", 100.0);
        {
            let surface = HudBatchSurface {
                height, aspect, batch: &mut bridge.hud_model.batch, fonts: &bridge.hud_model.fonts, assets: &bridge.hud_model.assets,
                textures: Some(&texture_map), strings: Some(&string_map), };
            spawn_ctrl.view.assets = BattleUiAssets::from_surface(&surface); }
        let mut overlay = Self {
            bridge, resources, texture_map, string_map, spawn_port, spawn_ctrl, scoreboard_port, scoreboard_ctrl, pause_menu,
            pause_settings, chat, mode: DemoMode::Spawn, size, height, aspect, cursor_px: [size[0] as f64 * 0.5, size[1] as f64 * 0.5], dirty: true, };
        overlay.set_mode(initial_mode);
        Ok(overlay) }
    fn populate_hud_posts_and_teams(hud_model: &mut hud_model::HudModel, port: &DemoSpawnPort, own_team: u8) {
        let x = &mut hud_model.input;
        x[H::BATTLE] = 1.0;
        x[H::COUNTDOWN] = f64::NAN;
        x[H::POSTS] = port.posts.len().min(hud_model::MAX_POSTS) as f64;
        for (i, post) in port.posts.iter().take(hud_model::MAX_POSTS).enumerate() {
            let o = hud_model::POSTS + i * P::WORDS;
            x[o + P::X] = post.position[0];
            x[o + P::Z] = post.position[2];
            x[o + P::MAPPED] = 1.0;
            x[o + P::ACTIVE] = 1.0;
            x[o + P::TEAM] = post.owner as f64;
            x[o + P::RELATION] = if post.owner == 0 { 0.0 } else if post.owner == own_team { 1.0 } else { 2.0 };
            x[o + P::SCALE] = 2.5; }
        for (team_id, tickets) in [(1usize, 142.0), (2usize, 118.0)] {
            let o = hud_model::TEAMS + team_id * T::WORDS;
            x[o + T::RELATION] = if team_id as u8 == own_team { 1.0 } else { 2.0 };
            x[o + T::TICKETS] = tickets;
            x[o + T::LIVING] = 16.0; }
        x[H::OWN] = 142.0;
        x[H::OWN_START] = 150.0;
        x[H::FOE] = 118.0;
        x[H::FOE_START] = 150.0; }
    fn set_mode(&mut self, mode: DemoMode) {
        self.mode = mode;
        let fh = self.bridge.hud_model.fonts[0].height;
        match mode {
            DemoMode::Spawn => {
                self.pause_menu.close(false);
                self.scoreboard_ctrl.model.close();
                if !self.spawn_ctrl.is_open() { self.spawn_ctrl.open(&mut self.spawn_port, OpenReason::Start, self.height); }
                for _ in 0..12 { self.spawn_ctrl.update(0.05, &mut self.spawn_port, self.height, fh, false, 0.25); } }
            DemoMode::ScoreboardTable | DemoMode::ScoreboardDetails | DemoMode::ScoreboardAwards => {
                self.pause_menu.close(false);
                self.scoreboard_ctrl.ended = false;
                self.scoreboard_ctrl.model.close();
                let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                self.scoreboard_ctrl.update(0.01, &mut self.scoreboard_port, &surface, false, false);
                if mode == DemoMode::ScoreboardDetails {
                    self.scoreboard_ctrl.act(ScoreboardAction::Details, &mut self.scoreboard_port, self.height);
                } else if mode == DemoMode::ScoreboardAwards { self.scoreboard_ctrl.act(ScoreboardAction::Awards, &mut self.scoreboard_port, self.height); }
                for _ in 0..15 { self.scoreboard_ctrl.update(0.05, &mut self.scoreboard_port, &surface, false, false); } }
            DemoMode::PauseMain | DemoMode::PauseConfirm => {
                self.scoreboard_ctrl.model.close();
                self.pause_menu.close(false);
                self.pause_menu.open("key", self.height);
                if mode == DemoMode::PauseConfirm {
                    let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                    self.pause_menu.focused_row = 2;
                    self.pause_menu.ask(ConfirmKind::Suicide, &surface); }
                for _ in 0..20 { self.pause_menu.update(0.1); } } } }
    fn update_and_draw(&mut self, dt: f64) {
        if !self.dirty { self.upload_and_draw_batch(); return; }
        self.dirty = false;
        let dt = dt.clamp(0.0, 0.1);
        let fh = self.bridge.hud_model.fonts[0].height;
        self.bridge.hud_model.input[H::DT] = dt;
        self.bridge.hud_model.input[H::HEIGHT] = self.height;
        self.bridge.hud_model.input[H::ASPECT] = self.aspect;
        self.bridge.hud_model.input[H::FOV] = 60.0;
        match self.mode {
            DemoMode::Spawn => {
                if !self.spawn_ctrl.is_open() { self.spawn_ctrl.open(&mut self.spawn_port, OpenReason::Death, self.height); }
                self.spawn_ctrl.update(dt, &mut self.spawn_port, self.height, fh, false, 0.25);
                Self::populate_hud_posts_and_teams(&mut self.bridge.hud_model, &self.spawn_port, self.spawn_ctrl.model.team);
                self.spawn_ctrl.view.sync_hud_inputs(&self.spawn_ctrl.model, &self.spawn_port, false, &mut self.bridge.hud_model.input);
                self.bridge.hud_model_update();
                self.bridge.hud_model.build(0);
                self.draw_spawn_team_emblems();
                {
                    let mut surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                    self.spawn_ctrl.view.draw(&mut surface, &self.spawn_ctrl.model, &self.spawn_port); }
                self.spawn_ctrl.view.draw_pointer(&mut self.bridge.hud_model.batch, self.height, self.spawn_ctrl.input.cursor); }
            DemoMode::ScoreboardTable | DemoMode::ScoreboardDetails | DemoMode::ScoreboardAwards => {
                self.bridge.hud_model.batch.begin();
                let mut sb_batch = Batch::new();
                sb_batch.begin();
                {
                    let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                    if !self.scoreboard_ctrl.model.is_open() { self.scoreboard_ctrl.ended = false; }
                    self.scoreboard_ctrl.update(dt, &mut self.scoreboard_port, &surface, false, false);
                    self.scoreboard_ctrl.view.draw_under(&surface, &mut sb_batch);
                    self.scoreboard_ctrl.view.draw_over( &surface, &mut sb_batch, &self.scoreboard_ctrl.model, &self.scoreboard_port,
                        Some(self.scoreboard_ctrl.input.cursor), ); }
                Self::append_batch(&mut self.bridge.hud_model.batch, &sb_batch); }
            DemoMode::PauseMain | DemoMode::PauseConfirm => {
                self.bridge.hud_model.batch.begin();
                self.pause_menu.update(dt);
                if !self.pause_menu.is_open { self.pause_menu.open("key", self.height); }
                let mut menu_batch = Batch::new();
                menu_batch.begin();
                {
                    let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                    let m = root_anchor(self.height);
                    surface.batch.quad(m, [0.0, 0.0, 1024.0, self.height], A::SOLID, UV, [0.0, 0.02, 0.06, 0.35], 0.0, 0);
                    self.pause_menu.draw(&surface, &mut menu_batch); }
                Self::append_batch(&mut self.bridge.hud_model.batch, &menu_batch); } }
        self.upload_and_draw_batch(); }
    fn draw_spawn_team_emblems(&mut self) {
        let fh = self.bridge.hud_model.fonts[0].height;
        let layout = self.spawn_ctrl.view.layout(self.height, fh);
        let root = root_anchor(self.height);
        let own_team = self.spawn_ctrl.model.team;
        let other_team = if own_team == 1 { 2 } else { 1 };
        for (team, pos, radius, color) in [ (own_team, layout.logos.own, layout.logos.big * 0.5, [0.125, 0.875, 0.125, 0.95] as Color),
            (other_team, layout.logos.other, layout.logos.small * 0.5, [0.875, 0.125, 0.125, 0.55] as Color), ] {
            let m = translate(root, pos[0], pos[1], 0.0);
            let name = if team == 1 { "rep_icon" } else { "cis_icon" };
            let tex = self.texture_map.get(name).copied().unwrap_or(A::HUD_FLAG_ICON);
            self.bridge.hud_model.batch.quad(m, [-radius, -radius, radius, radius], tex, UV, color, 0.0, 0);
            let label = if team == 1 { "REPUBLIC" } else { "CIS" };
            let utf16: Vec<u16> = label.encode_utf16().collect();
            self.bridge.hud_model.fonts[1].draw( &mut self.bridge.hud_model.batch, m, 0.0, radius + 2.0, &utf16,
                alpha([1.0, 1.0, 1.0, 1.0], color[3]), 2, 0.85, 1.0, 0.0, ); } }
    fn append_batch(dst: &mut Batch, src: &Batch) {
        let base_quad = dst.count;
        let count = src.count.min(render_hud::QUADS.saturating_sub(base_quad));
        if count == 0 { return; }
        let (v0, v1, sv1) = (base_quad * 4, (base_quad + count) * 4, count * 4);
        dst.position[v0 * 3..v1 * 3].copy_from_slice(&src.position[..sv1 * 3]);
        dst.uv[v0 * 2..v1 * 2].copy_from_slice(&src.uv[..sv1 * 2]);
        dst.mask_uv[v0 * 2..v1 * 2].copy_from_slice(&src.mask_uv[..sv1 * 2]);
        dst.tint[v0 * 4..v1 * 4].copy_from_slice(&src.tint[..sv1 * 4]);
        let index_offset = base_quad * 6;
        for draw in &src.draws { dst.draws.push(Draw { first: draw.first + index_offset, ..*draw }); }
        dst.count += count;
        dst.end = v1; }
    fn upload_and_draw_batch(&mut self) {
        use bfe_render::hud::*;
        let b = &self.bridge.hud_model.batch;
        let h = &mut self.bridge.r.hud;
        let end = (b.count * 4).min(QUADS * 4);
        if end > 0 {
            h.position[..end * 3].copy_from_slice(&b.position[..end * 3]);
            h.uv[..end * 2].copy_from_slice(&b.uv[..end * 2]);
            h.mask_uv[..end * 2].copy_from_slice(&b.mask_uv[..end * 2]);
            h.tint[..end * 4].copy_from_slice(&b.tint[..end * 4]); }
        h.input[IN_UPLOAD] = 0.0;
        h.input[IN_UPLOAD + 1] = end as f64;
        let groups = b.draws.len().min(MAX_GROUPS);
        h.input[IN_GROUPS] = groups as f64;
        for (i, draw) in b.draws.iter().take(groups).enumerate() {
            let o = IN_FIRST_GROUP + i * GROUP;
            h.input[o] = draw.first as f64;
            h.input[o + 1] = draw.count as f64;
            let tex = self.bridge.hud_model.asset(draw.texture).texture;
            let mask_id = if draw.mask == 0 { draw.texture } else { draw.mask };
            let mask = self.bridge.hud_model.asset(mask_id).texture;
            h.input[o + 2] = tex as f64;
            h.input[o + 3] = mask as f64;
            h.input[o + 4] = u8::from(draw.mask != 0) as f64; }
        self.bridge.r.core.begin_phase();
        self.bridge.r.hud.draw(&mut self.bridge.r.core);
        let _ = self.bridge.r.core.end_phase(); }
    fn handle_key(&mut self, key: KeyCode, down: bool, repeat: bool) { self.dirty = true;
        if down && !repeat {
            match key {
                KeyCode::Digit1 => return self.set_mode(DemoMode::Spawn),
                KeyCode::Digit2 => return self.set_mode(DemoMode::ScoreboardTable),
                KeyCode::Digit3 => return self.set_mode(DemoMode::ScoreboardDetails),
                KeyCode::Digit4 => return self.set_mode(DemoMode::ScoreboardAwards),
                KeyCode::Digit5 => return self.set_mode(DemoMode::PauseMain),
                KeyCode::Digit6 => return self.set_mode(DemoMode::PauseConfirm), _ => {} } }
        let Some(code_str) = keycode_to_dom_code(key) else { return; };
        let fh = self.bridge.hud_model.fonts[0].height;
        match self.mode {
            DemoMode::Spawn => {
                let (height, model, view, port) = (self.height, &self.spawn_ctrl.model, &mut self.spawn_ctrl.view, &self.spawn_port);
                let (_, action) = self.spawn_ctrl.input.key(model, code_str, down, repeat, true, |x, y| view.hit(x, y, height, fh, model, port));
                if action == SpawnInputAction::Spawn { self.spawn_ctrl.spawn(&mut self.spawn_port); } }
            DemoMode::ScoreboardTable | DemoMode::ScoreboardDetails | DemoMode::ScoreboardAwards => {
                let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                let (model, view, port) = (&self.scoreboard_ctrl.model, &self.scoreboard_ctrl.view, &self.scoreboard_port);
                self.scoreboard_ctrl.input.key(code_str, down, repeat, true, true, |x, y| view.hit(x, y, model, port, &surface)); }
            DemoMode::PauseMain | DemoMode::PauseConfirm => {
                let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                self.pause_menu.handle_key(code_str, down, repeat, &surface, &self.pause_settings); } } }
    fn handle_cursor_move(&mut self, x_px: f64, y_px: f64) { self.dirty = true;
        self.cursor_px = [x_px, y_px];
        let pt = to_interface(x_px, y_px, self.size[0] as f64, self.size[1] as f64, self.height);
        match self.mode {
            DemoMode::Spawn => self.spawn_ctrl.input.move_cursor(pt[0], pt[1], self.height),
            DemoMode::ScoreboardTable | DemoMode::ScoreboardDetails | DemoMode::ScoreboardAwards => { self.scoreboard_ctrl.input.move_cursor(pt[0], pt[1]); }
            DemoMode::PauseMain | DemoMode::PauseConfirm => { self.pause_menu.move_pointer(pt[0], pt[1], self.height); } } }
    fn handle_mouse_click(&mut self) { self.dirty = true;
        let pt = to_interface(self.cursor_px[0], self.cursor_px[1], self.size[0] as f64, self.size[1] as f64, self.height);
        let fh = self.bridge.hud_model.fonts[0].height;
        match self.mode {
            DemoMode::Spawn => {
                let (height, model, view, port) = (self.height, &self.spawn_ctrl.model, &mut self.spawn_ctrl.view, &self.spawn_port);
                let (_, action) = self.spawn_ctrl.input.click(model, pt[0], pt[1], height, true, |x, y| view.hit(x, y, height, fh, model, port));
                if action == SpawnInputAction::Spawn { self.spawn_ctrl.spawn(&mut self.spawn_port); } }
            DemoMode::ScoreboardTable | DemoMode::ScoreboardDetails | DemoMode::ScoreboardAwards => {
                let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                let (model, view, port) = (&self.scoreboard_ctrl.model, &self.scoreboard_ctrl.view, &self.scoreboard_port);
                self.scoreboard_ctrl.input.click(pt[0], pt[1], |x, y| view.hit(x, y, model, port, &surface));
                let pending = std::mem::take(&mut self.scoreboard_ctrl.input.actions);
                for act in pending { self.scoreboard_ctrl.act(act, &mut self.scoreboard_port, self.height); }
                self.mode = match self.scoreboard_ctrl.model.page {
                    BoardPage::Details => DemoMode::ScoreboardDetails, BoardPage::Awards => DemoMode::ScoreboardAwards,
                    BoardPage::Table => DemoMode::ScoreboardTable, }; }
            DemoMode::PauseMain | DemoMode::PauseConfirm => {
                let surface = HudBatchSurface { height: self.height, aspect: self.aspect, batch: &mut self.bridge.hud_model.batch, fonts: &self.bridge.hud_model.fonts, assets: &self.bridge.hud_model.assets, textures: Some(&self.texture_map), strings: Some(&self.string_map) };
                self.pause_menu.click(pt[0], pt[1], &surface, &self.pause_settings); } } }
    fn handle_wheel(&mut self, delta_y: f64) { self.dirty = true;
        match self.mode { DemoMode::Spawn => { self.spawn_ctrl.input.wheel(&self.spawn_ctrl.model, delta_y, true); }
            DemoMode::ScoreboardTable | DemoMode::ScoreboardDetails | DemoMode::ScoreboardAwards => {
                self.scoreboard_ctrl.model.scroll(if delta_y > 0.0 { 1 } else { -1 }); }
            DemoMode::PauseMain | DemoMode::PauseConfirm => {} } } }
fn keycode_to_dom_code(key: KeyCode) -> Option<&'static str> {
    match key {
        KeyCode::ArrowUp => Some("ArrowUp"), KeyCode::ArrowDown => Some("ArrowDown"),
        KeyCode::ArrowLeft => Some("ArrowLeft"), KeyCode::ArrowRight => Some("ArrowRight"),
        KeyCode::KeyW => Some("KeyW"), KeyCode::KeyS => Some("KeyS"), KeyCode::KeyA => Some("KeyA"), KeyCode::KeyD => Some("KeyD"),
        KeyCode::Enter => Some("Enter"), KeyCode::NumpadEnter => Some("NumpadEnter"),
        KeyCode::Space => Some("Space"), KeyCode::Tab => Some("Tab"),
        KeyCode::Escape => Some("Escape"), KeyCode::Backspace => Some("Backspace"),
        KeyCode::PageUp => Some("PageUp"), KeyCode::PageDown => Some("PageDown"),
        KeyCode::Home => Some("Home"), KeyCode::End => Some("End"),
        KeyCode::KeyM => Some("KeyM"), KeyCode::KeyT => Some("KeyT"), KeyCode::KeyY => Some("KeyY"), _ => None, } }
fn write_png(path: &Path, width: u32, height: u32, bottom_up_rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let row_bytes = (width as usize) * 4;
    let mut raw = Vec::with_capacity((row_bytes + 1) * (height as usize));
    for y in (0..height as usize).rev() {
        raw.push(0u8);
        for px in bottom_up_rgba[y * row_bytes..(y + 1) * row_bytes].chunks_exact(4) { raw.extend_from_slice(&[px[0], px[1], px[2], 255]); }
    }
    let mut zlib = Vec::with_capacity(raw.len() + (raw.len() / 65535 + 2) * 5 + 6);
    zlib.extend_from_slice(&[0x78, 0x01]);
    let chunks: Vec<&[u8]> = if raw.is_empty() { vec![&[]] } else { raw.chunks(65535).collect() };
    for (i, chunk) in chunks.iter().enumerate() {
        let len = chunk.len() as u16;
        zlib.push(u8::from(i + 1 == chunks.len()));
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk); }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw { a = (a + byte as u32) % 65521; b = (b + a) % 65521; }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut png = Vec::with_capacity(zlib.len() + 128);
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    for (kind, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &zlib[..]), (b"IEND", &[][..])] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let mut crc = 0xffff_ffffu32;
        for &byte in kind.iter().chain(data.iter()) {
            crc ^= byte as u32;
            for _ in 0..8 { crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 }; } }
        png.extend_from_slice(&(!crc).to_be_bytes()); }
    std::fs::write(path, png).map_err(|e| format!("failed to write {}: {e}", path.display())) }
struct RunningDemo {
    scene: Scene, host: Host<MapCamera>, overlay: BattleUiOverlay, surface: Surface<WindowSurface>, context: PossiblyCurrentContext,
    window: Window, start: Instant, last: Instant, next: Instant, refresh: Duration, frames: u32, steady_frames: u64, steady_allocs: u64, gl_errors: u64,
}
impl RunningDemo {
    fn new(el: &ActiveEventLoop, options: &DemoOptions) -> Result<Self, String> {
        let [w, h] = options.size;
        let attrs = Window::default_attributes()
            .with_title("Battlefront Eternal - Battle UI Demo")
            .with_visible(false)
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(w as u32, h as u32));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build( el, ConfigTemplateBuilder::new().with_alpha_size(8).with_depth_size(24).with_stencil_size(8),
                |c| c.min_by_key(|c| c.num_samples()).expect("no GL configuration"), )
            .map_err(|e| e.to_string())?;
        let window = window.ok_or("window creation failed")?;
        let display = config.display();
        let raw = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let api = if options.desktop_gl { ContextApi::OpenGl(Some(Version::new(4, 5))) } else {
            ContextApi::Gles(Some(Version::new(3, 0))) };
        let ctx_attrs = ContextAttributesBuilder::new().with_context_api(api).build(Some(raw));
        let context = unsafe { display.create_context(&config, &ctx_attrs) }.map_err(|e| e.to_string())?;
        let surface = unsafe {
            display.create_window_surface( &config, &window.build_surface_attributes(Default::default()).map_err(|e| e.to_string())?, ) }
        .map_err(|e| e.to_string())?;
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        surface
            .set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()))
            .map_err(|e| e.to_string())?;
        let make_gl = || unsafe { glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name) as *const _) };
        let (scene, camera) = Scene::load(&options.root, &options.map, options.size, make_gl)?;
        let canvas_fb = scene.canvas.framebuffer;
        let overlay = BattleUiOverlay::load(&options.root, &options.map, options.size, canvas_fb, make_gl, options.mode)?;
        window.set_visible(true);
        let now = Instant::now();
        Ok(Self {
            scene, host: Host::new(camera), overlay, surface, context, window, start: now, last: now, next: now, refresh: Duration::from_millis(16), frames: 0, steady_frames: 0, steady_allocs: 0, gl_errors: 0, }) }
    fn redraw(&mut self, options: &DemoOptions) -> Result<bool, String> {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().clamp(0.001, 0.05);
        self.last = now;
        if let Some(dir) = &options.screenshot_all {
            let modes = [ DemoMode::Spawn, DemoMode::ScoreboardTable, DemoMode::ScoreboardDetails, DemoMode::ScoreboardAwards,
                DemoMode::PauseMain, DemoMode::PauseConfirm, ];
            let view = self.host.frame(0.016)?;
            for mode in modes {
                self.overlay.set_mode(mode);
                self.scene.draw(view)?;
                self.overlay.update_and_draw(0.016);
                let pixels = self.scene.pixels()?;
                let out_path = dir.join(mode.filename());
                write_png(&out_path, options.size[0] as u32, options.size[1] as u32, &pixels)?;
                eprintln!("wrote screenshot {}", out_path.display()); }
            if let Some(single_path) = &options.screenshot {
                if !modes.iter().any(|m| dir.join(m.filename()) == *single_path) {
                    self.overlay.set_mode(options.mode);
                    self.scene.draw(view)?;
                    self.overlay.update_and_draw(0.016);
                    let pixels = self.scene.pixels()?;
                    write_png(single_path, options.size[0] as u32, options.size[1] as u32, &pixels)?;
                    eprintln!("wrote screenshot {}", single_path.display()); } }
            self.scene.canvas.show();
            self.window.pre_present_notify();
            self.surface.swap_buffers(&self.context).map_err(|e| e.to_string())?;
            return Ok(true); }
        let warm = self.frames >= 4;
        if !warm { self.overlay.dirty = true; }
        let alloc_before = ALLOC_COUNT.load(Ordering::Relaxed);
        let view = self.host.frame(dt)?;
        self.scene.draw(view)?;
        self.overlay.update_and_draw(dt);
        let alloc_after = ALLOC_COUNT.load(Ordering::Relaxed);
        if warm {
            self.steady_allocs += alloc_after.saturating_sub(alloc_before);
            self.steady_frames += 1;
        }
        unsafe {
            if self.overlay.bridge.r.core.gl.get_error() != glow::NO_ERROR {
                self.gl_errors += 1;
            }
        }
        self.frames += 1;
        if let Some(path) = &options.screenshot {
            if self.frames >= 2 {
                let pixels = self.scene.pixels()?;
                write_png(path, options.size[0] as u32, options.size[1] as u32, &pixels)?;
                eprintln!("wrote screenshot {}", path.display());
                self.scene.canvas.show();
                self.window.pre_present_notify();
                self.surface.swap_buffers(&self.context).map_err(|e| e.to_string())?;
                return Ok(true); } }
        self.scene.canvas.show();
        self.window.pre_present_notify();
        self.surface.swap_buffers(&self.context).map_err(|e| e.to_string())?;
        if let Some(secs) = options.seconds {
            let elapsed = self.start.elapsed().as_secs_f64();
            if elapsed >= secs {
                let allocs_per_frame = if self.steady_frames > 0 { self.steady_allocs as f64 / self.steady_frames as f64 } else { 0.0 };
                println!("{}", serde_json::json!({
                    "seconds": elapsed,
                    "frames": self.frames,
                    "steadyFrames": self.steady_frames,
                    "steadyAllocs": self.steady_allocs,
                    "allocsPerFrame": allocs_per_frame,
                    "glErrors": self.gl_errors,
                    "quads": self.overlay.bridge.hud_model.batch.count,
                    "draws": self.overlay.bridge.hud_model.batch.draws.len(),
                }));
                return Ok(true);
            }
        }
        self.next = (self.next + self.refresh).max(Instant::now());
        Ok(false) } }
struct DemoApp { options: DemoOptions, running: Option<RunningDemo>, error: Option<String>, }
impl DemoApp { fn stop(&mut self, el: &ActiveEventLoop, error: Option<String>) { self.error = error; self.running.take(); el.exit(); } }
impl ApplicationHandler for DemoApp {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.running.is_some() { return; }
        match RunningDemo::new(el, &self.options) { Ok(r) => { r.window.request_redraw(); self.running = Some(r); }
            Err(e) => self.stop(el, Some(e)), } }
    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else { return; };
        match event {
            WindowEvent::CloseRequested => self.stop(el, None), WindowEvent::RedrawRequested => match r.redraw(&self.options) {
                Ok(true) => self.stop(el, None), Ok(false) => {}
                Err(e) => self.stop(el, Some(e)), }, WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key { r.overlay.handle_key(key, event.state == ElementState::Pressed, event.repeat); }
            }
            WindowEvent::CursorMoved { position, .. } => { r.overlay.handle_cursor_move(position.x, position.y); }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => { r.overlay.handle_mouse_click(); }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as f64 * 40.0, MouseScrollDelta::PixelDelta(p) => -p.y, };
                r.overlay.handle_wheel(y); }
            _ => {} } }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(r) = &self.running {
            if Instant::now() >= r.next { r.window.request_redraw(); } else { el.set_control_flow(ControlFlow::WaitUntil(r.next)); } } } }
fn main() {
    let result = (|| -> Result<(), String> {
        let options = DemoOptions::parse()?;
        let mut app = DemoApp { options, running: None, error: None };
        let el = EventLoop::new().map_err(|e| e.to_string())?;
        el.run_app(&mut app).map_err(|e| e.to_string())?;
        app.error.map_or(Ok(()), Err)
    })();
    if let Err(e) = result { eprintln!("{e}"); std::process::exit(1); } }
