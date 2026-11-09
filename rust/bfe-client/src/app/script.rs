//! Automated gate script (`--script gate`) for GPU VM verification and screenshot capture.
//!
//! Drives the complete standalone game lifecycle:
//! 1. Startup loader (`01-startup-loader.png`)
//! 2. Click-to-continue gate (`02-gate.png`)
//! 3. Main Menu, Singleplayer, Instant Action playlist (`nab2` -> `tat1` -> `hot1`),
//!    `bfe-interface` Options, Profile (`03-main-menu.png` .. `07-profile.png`)
//! 4. Instant Action loading (`08-ia-loading.png`)
//! 5. 3-battle Instant Action playlist (`nab2`, `tat1`, `hot1`) admitted exclusively via
//!    `Context::world_admit` and stepped via `Context::world_tick` for 5 minutes each
//!    with generated combat input, exercising Spawn Screen, 3D Battle + HUD, Scoreboard
//!    (Table, Details, Awards), Playlist progression on `DONE`, Pause Menu, and Quit to
//!    Main Menu (`09-ia-nab2-spawn.png` .. `18-main-menu-return.png`)
//! 6. Multiplayer Server Browser (`19-multiplayer-browser.png`), joining a remote
//!    `bfeserver` for 5 minutes with generated input (`20-online-spawn.png` ..
//!    `23-online-pause.png`), and leaving back to the Server Browser
//!    (`24-multiplayer-return.png`).
//!
//! When invoked as `--script ia:<MAP>` (for example `ia:nab2`, `ia:tat1`, `ia:bes1`,
//! or `ia:hot1`), launches a single Instant Action battle on `<MAP>` for
//! `BFE_GATE_BATTLE_SECONDS` (default 300 s), records pre-allocated per-frame
//! intervals without steady-state heap allocations, and exits cleanly after writing
//! `gate-summary.json` with mean FPS, 1% low FPS, frame-time percentiles, and host metrics.
//!
//! ## Verification Metrics Recorded in `gate-summary.json`
//! - `glErrors`: Cumulative OpenGL errors observed via `glGetError()` across the entire run (must be `0`).
//! - `steadyFrameAllocationsAfterWarmup`: Heap allocations on the render thread during steady-state
//!   3D gameplay after warmup (reported as a diagnostic).
//! - `worldRendererBlocks`: Number of times the render thread blocked waiting on the 60 Hz
//!   `"bfe-world-60hz"` simulation thread (must be `0`).
//! - `worldThreadTickTimeMs`: Per-map (`nab2`, `tat1`, `hot1`) 60 Hz world-thread tick-time
//!   distribution (`ticks`, `min_ms`, `p50_ms`, `p95_ms`, `p99_ms`, `max_ms`, `mean_ms`),
//!   plus host proof counts (AI hearing records fed by kind, presentation records published,
//!   whether the `Context` was poisoned, and steady allocations on the world thread after warmup).
//!
//! ## Screenshot Sequence (`01` .. `24`)
//! | # | Filename | Phase | Description |
//! |---|---|---|---|
//! | 01 | `01-startup-loader.png` | `StartupLoader` | Animated planet zoom loader screen |
//! | 02 | `02-gate.png` | `GateScreen` | Click-to-continue title gate |
//! | 03 | `03-main-menu.png` | `MainMenu` | Front-end Main Menu (`SINGLEPLAYER`, `MULTIPLAYER`, `OPTIONS`, `PROFILE`) |
//! | 04 | `04-singleplayer.png` | `SingleplayerMenu` | Singleplayer submenu |
//! | 05 | `05-instant-action.png` | `InstantActionMenu` | Instant Action 3-map playlist (`nab2`, `tat1`, `hot1`) |
//! | 06 | `06-options.png` | `OptionsMenu` | `bfe-interface` Options screen (`ClassicTheme::paint_stack`) |
//! | 07 | `07-profile.png` | `ProfileMenu` | Player profile management screen |
//! | 08 | `08-ia-loading.png` | `BattleLoading` | Mission loading screen before `nab2` admission |
//! | 09 | `09-ia-nab2-spawn.png` | `IaNab2Spawn` | `nab2` spawn screen with real admitted classes and command posts |
//! | 10 | `10-ia-nab2-battle.png` | `IaNab2Combat` | `nab2` 3D combat with bots, command posts, and HUD |
//! | 11 | `11-ia-nab2-scoreboard.png` | `IaNab2ScoreboardTable` | `nab2` scoreboard table with live kills, deaths, and points |
//! | 12 | `12-ia-nab2-scoreboard-details.png` | `IaNab2ScoreboardDetails` | `nab2` personal combat statistics tab |
//! | 13 | `13-ia-nab2-scoreboard-awards.png` | `IaNab2ScoreboardAwards` | `nab2` post-battle medals/awards tab |
//! | 14 | `14-ia-tat1-battle.png` | `IaTat1Combat` | Playlist battle 2 (`tat1` Dune Sea) admitted via `Context::world_admit` |
//! | 15 | `15-ia-bes1-battle.png` | `IaBes1Combat` | Playlist battle 3 (`hot1` Hoth) admitted via `Context::world_admit` |
//! | 16 | `16-ia-bes1-pause.png` | `IaBes1PauseMain` | In-battle `ESC` pause menu overlay |
//! | 17 | `17-ia-bes1-pause-quit.png` | `IaBes1PauseQuit` | Pause menu `QUIT TO MAIN MENU` confirmation modal |
//! | 18 | `18-main-menu-return.png` | `MainMenuAfterIa` | Clean return to Main Menu after 3 Instant Action battles |
//! | 19 | `19-multiplayer-browser.png` | `MultiplayerBrowser` | Server browser listing the live `bfeserver` instance |
//! | 20 | `20-online-spawn.png` | `OnlineSpawn` | Online multiplayer spawn screen on `bfeserver` |
//! | 21 | `21-online-battle.png` | `OnlineCombat` | Online multiplayer 3D combat on `bfeserver` |
//! | 22 | `22-online-scoreboard.png` | `OnlineScoreboard` | Online multiplayer scoreboard with ping and link status |
//! | 23 | `23-online-pause.png` | `OnlinePause` | Online multiplayer pause menu (`PLAYER LIST` enabled) |
//! | 24 | `24-multiplayer-return.png` | `MultiplayerAfterOnline` | Clean return to Multiplayer Server Browser |

use crate::{
    app::{
        battle::BattleController,
        settings::AppSettingsStore,
        shell::{ShellHost, ShellTransition},
    },
    local::thread::WorldTickStats,
};
use bfe_battle_ui::{pause::ConfirmKind, scoreboard::BoardPage};
use bfe_online::ServerBrowserService;
use bfe_shell::{
    launch::{BattleRequest, GameFamily, PlaylistEntry},
    screens::{MultiplayerKind, MultiplayerLaunch, ScreenId},
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use winit::{event::MouseButton, keyboard::KeyCode};

/// Ordered choreography phases executed by [`GateDriver`] during `--script gate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatePhase {
    /// Initial startup content loader screen (`01-startup-loader.png`).
    StartupLoader,
    /// Click-to-continue gate screen (`02-gate.png`).
    GateScreen,
    /// Main menu root screen (`03-main-menu.png`).
    MainMenu,
    /// Singleplayer submenu (`04-singleplayer.png`).
    SingleplayerMenu,
    /// Instant Action 3-map playlist setup screen (`05-instant-action.png`).
    InstantActionMenu,
    /// `bfe-interface` Options menu (`06-options.png`).
    OptionsMenu,
    /// Player profile selection and rename screen (`07-profile.png`).
    ProfileMenu,
    /// Transition trigger that launches the 3-battle Instant Action playlist.
    LaunchIaPlaylist,
    /// Instant Action battle 1 (`nab2`) spawn selection overlay (`09-ia-nab2-spawn.png`).
    IaNab2Spawn,
    /// Instant Action battle 1 (`nab2`) active 3D combat (`10-ia-nab2-battle.png`).
    IaNab2Combat,
    /// Instant Action battle 1 (`nab2`) scoreboard table tab (`11-ia-nab2-scoreboard.png`).
    IaNab2ScoreboardTable,
    /// Instant Action battle 1 (`nab2`) scoreboard details tab (`12-ia-nab2-scoreboard-details.png`).
    IaNab2ScoreboardDetails,
    /// Instant Action battle 1 (`nab2`) post-battle awards tab (`13-ia-nab2-scoreboard-awards.png`).
    IaNab2ScoreboardAwards,
    /// Instant Action battle 2 (`tat1`) spawn selection overlay.
    IaTat1Spawn,
    /// Instant Action battle 2 (`tat1`) active 3D combat (`14-ia-tat1-battle.png`).
    IaTat1Combat,
    /// Instant Action battle 3 (`hot1`) spawn selection overlay.
    IaBes1Spawn,
    /// Instant Action battle 3 (`hot1`) active 3D combat (`15-ia-bes1-battle.png`).
    IaBes1Combat,
    /// Instant Action battle 3 (`hot1`) in-battle `ESC` pause menu (`16-ia-bes1-pause.png`).
    IaBes1PauseMain,
    /// Instant Action battle 3 (`hot1`) quit-to-menu confirmation modal (`17-ia-bes1-pause-quit.png`).
    IaBes1PauseQuit,
    /// Main menu after returning from Instant Action (`18-main-menu-return.png`).
    MainMenuAfterIa,
    /// Multiplayer server browser listing the live `bfeserver` (`19-multiplayer-browser.png`).
    MultiplayerBrowser,
    /// Online multiplayer battle spawn selection overlay (`20-online-spawn.png`).
    OnlineSpawn,
    /// Online multiplayer battle active 3D combat (`21-online-battle.png`).
    OnlineCombat,
    /// Online multiplayer battle scoreboard overlay (`22-online-scoreboard.png`).
    OnlineScoreboard,
    /// Online multiplayer battle pause menu (`23-online-pause.png`).
    OnlinePause,
    /// Multiplayer server browser after leaving the online battle (`24-multiplayer-return.png`).
    MultiplayerAfterOnline,
    /// Terminal state after all 24 screenshots and `gate-summary.json` are written.
    Finished,
}

/// Automated driver for `--script gate`, `--script gate-fast`, and `--script ia:<MAP>`.
///
/// Tracks the current [`GatePhase`], per-phase frame count and elapsed wall time,
/// screenshot output paths, OpenGL error count, steady-state render-thread heap
/// allocations after warmup, world/renderer contention blocks, and the 60 Hz
/// world-thread tick-time distribution and host proof counts for every admitted
/// Instant Action battle.
pub struct GateDriver {
    movie_verification: bool,
    movie_short: bool,
    movie_stage: usize,
    movie_baseline: Option<(Instant, u64, u64, f64)>,
    movie_measurement: Option<serde_json::Value>,
    page_proof: Option<super::page_proof::PageProof>,
    pub(super) page_capture_done: bool,
    online_duel: bool,
    duel_input_tick: u32,
    spawn_posts: std::collections::BTreeMap<u8, String>,
    observed_second: u64,
    observed_health: f64,
    observed_alive: bool,
    observed_end: bool,
    audio_events: [u64; 27],
    voice_decodes: u64,
    audio_frames: u64,
    audio_peak: f32,
    observed_map: String,
    /// Current phase in the 24-screenshot verification choreography.
    pub phase: GatePhase,
    /// Timestamp when the current phase began.
    pub phase_started: Instant,
    /// Number of frames rendered within the current phase.
    pub phase_frames: u64,
    /// Target combat duration in seconds per battle (`300.0` for `gate`, `3.0` for `gate-fast`).
    pub battle_duration_s: f64,
    /// Optional single-map override when invoked as `--script ia:<MAP>`.
    pub target_map: Option<String>,
    /// Output directory where PNG screenshots and `gate-summary.json` are written.
    pub screenshot_dir: PathBuf,
    /// Ordered list of screenshot filenames written so far.
    pub screenshots_written: Vec<String>,
    /// Cumulative OpenGL errors observed via `glGetError()` across all frames.
    pub gl_errors: u64,
    /// Maximum steady-state heap allocations observed on the render thread after warmup.
    pub steady_allocs_after_warmup: u64,
    /// Total steady-state battle frames measured across all Instant Action and Online battles.
    pub steady_frames_measured: u64,
    /// Maximum contention blocks between the 60 Hz world thread and the render thread.
    pub world_renderer_blocks: u64,
    /// Per-map 60 Hz world-thread tick-time distribution (`min`, `p50`, `p95`, `p99`, `max`) and host counts.
    pub world_tick_stats: Vec<(String, WorldTickStats)>,
    /// Count of Instant Action playlist battles completed (`3` for `nab2`, `tat1`, `hot1`).
    pub ia_battles_completed: usize,
    /// Whether the online `bfeserver` battle completed cleanly.
    pub online_battle_completed: bool,
    /// Monotonic tick counter used to synthesize deterministic combat movement and fire input.
    pub input_tick: u64,
    /// Pre-allocated frame-interval samples (in milliseconds) during active combat.
    pub combat_frame_ms: Vec<f64>,
    /// Timestamp of the previous combat frame for interval measurement.
    pub last_combat_instant: Option<Instant>,
    /// Per-battle frame-rate and frame-time summary objects written to `gate-summary.json`.
    pub battles: Vec<serde_json::Value>,
}

impl GateDriver {
    /// Creates a new [`GateDriver`] for `script_name` (`"gate"`, `"gate-fast"`, or `"ia:<MAP>"`),
    /// honoring `BFE_GATE_BATTLE_SECONDS` when set in the environment.
    pub fn new(script_name: &str, screenshot_dir: Option<PathBuf>) -> Self {
        // Default to 300 seconds (5 minutes) per battle for the full gate or 3 seconds for quick smoke runs.
        let default_battle_secs = if script_name == "gate-fast" {
            3.0
        } else {
            300.0
        };
        let battle_duration_s = std::env::var("BFE_GATE_BATTLE_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(default_battle_secs);
        let target_map = script_name
            .strip_prefix("ia:")
            .filter(|m| !m.trim().is_empty())
            .map(|m| m.trim().to_string());
        let dir =
            screenshot_dir.unwrap_or_else(|| std::env::temp_dir().join("bfe-gate-screenshots"));
        Self {
            movie_verification: matches!(script_name, "movies" | "movies-page"),
            movie_short: script_name == "movies-page",
            movie_stage: 0,
            movie_baseline: None,
            movie_measurement: None,
            page_proof: (script_name == "page-parity").then(super::page_proof::PageProof::new),
            page_capture_done: false,
            online_duel: script_name == "online-duel",
            spawn_posts: std::env::var("BFE_GATE_SPAWN_POSTS")
                .ok()
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default(),
            duel_input_tick: u32::MAX,
            observed_second: u64::MAX,
            observed_health: f64::NAN,
            observed_alive: false,
            observed_end: false,
            audio_events: [0; 27],
            voice_decodes: 0,
            audio_frames: 0,
            audio_peak: 0.,
            observed_map: String::new(),
            phase: GatePhase::StartupLoader,
            phase_started: Instant::now(),
            phase_frames: 0,
            battle_duration_s,
            target_map,
            screenshot_dir: dir,
            screenshots_written: Vec::new(),
            gl_errors: 0,
            steady_allocs_after_warmup: 0,
            steady_frames_measured: 0,
            world_renderer_blocks: 0,
            world_tick_stats: Vec::new(),
            ia_battles_completed: 0,
            online_battle_completed: false,
            input_tick: 0,
            combat_frame_ms: Vec::with_capacity(32_768),
            last_combat_instant: None,
            battles: Vec::with_capacity(4),
        }
    }

    /// Transitions the driver to `next`, logging progress and resetting per-phase counters.
    pub fn transition_to(&mut self, next: GatePhase) {
        eprintln!(
            "[gate] phase {:?} -> {:?} (frames={}, steady_allocs={}, world_blocks={})",
            self.phase,
            next,
            self.phase_frames,
            self.steady_allocs_after_warmup,
            self.world_renderer_blocks
        );
        self.phase = next;
        self.phase_started = Instant::now();
        self.phase_frames = 0;
        if matches!(
            next,
            GatePhase::IaNab2Combat
                | GatePhase::IaTat1Combat
                | GatePhase::IaBes1Combat
                | GatePhase::OnlineCombat
        ) {
            self.combat_frame_ms.clear();
            self.last_combat_instant = None;
        }
    }

    /// Resets the combat measurement window after the initial combat screenshot
    /// so disk I/O and spawn warmup are excluded from frame-time percentiles.
    pub fn reset_combat_measurement_window(&mut self) {
        self.combat_frame_ms.clear();
        let now = Instant::now();
        self.last_combat_instant = Some(now);
        self.phase_started = now;
    }

    /// Records the current frame timestamp into the pre-allocated interval buffer
    /// without allocating on the heap during combat.
    pub fn record_combat_frame_interval(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_combat_instant {
            if self.combat_frame_ms.len() < self.combat_frame_ms.capacity() {
                self.combat_frame_ms
                    .push((now - prev).as_secs_f64() * 1000.0);
            }
        }
        self.last_combat_instant = Some(now);
    }

    /// Summarizes frame-rate and frame-time percentiles at the end of a combat phase.
    pub fn finish_combat_metrics(
        &mut self,
        map: &str,
        fallback_frames: u64,
        fallback_seconds: f64,
    ) {
        let (frames, seconds) = if !self.combat_frame_ms.is_empty() {
            (
                self.combat_frame_ms.len() as u64,
                self.combat_frame_ms.iter().sum::<f64>() / 1000.0,
            )
        } else {
            (fallback_frames, fallback_seconds)
        };
        let fps = if seconds > 0.0 {
            (frames as f64) / seconds
        } else {
            0.0
        };
        let mut sorted = self.combat_frame_ms.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let pct = |q: f64| -> f64 {
            if sorted.is_empty() {
                0.0
            } else {
                let idx = ((sorted.len() as f64 * q).floor() as usize).min(sorted.len() - 1);
                sorted[idx]
            }
        };
        let p50_ms = pct(0.50);
        let p95_ms = pct(0.95);
        let p99_ms = pct(0.99);
        let slow_count = ((sorted.len() as f64 * 0.01).ceil() as usize)
            .max(1)
            .min(sorted.len());
        let slow_mean_ms = if slow_count > 0 {
            sorted[sorted.len() - slow_count..].iter().sum::<f64>() / (slow_count as f64)
        } else {
            0.0
        };
        let one_percent_low_fps = if slow_mean_ms > 0.0 {
            1000.0 / slow_mean_ms
        } else {
            0.0
        };
        let p99_fps = if p99_ms > 0.0 { 1000.0 / p99_ms } else { 0.0 };
        self.battles.push(serde_json::json!({
            "map": map,
            "frames": frames,
            "seconds": seconds,
            "fps": fps,
            "onePercentLowFps": one_percent_low_fps,
            "p99Fps": p99_fps,
            "frameMsP50": p50_ms,
            "frameMsP95": p50_ms.max(p95_ms),
            "frameMsP99": p99_ms,
        }));
    }

    /// Captures the current front-end shell framebuffer and writes it as `filename`.
    pub fn save_shell_shot(&mut self, shell: &ShellHost, filename: &str) -> Result<(), String> {
        let pixels = shell.pixels()?;
        let path = self.screenshot_dir.join(filename);
        write_png(&path, shell.size[0] as u32, shell.size[1] as u32, &pixels)?;
        self.screenshots_written.push(filename.to_string());
        Ok(())
    }

    /// Captures the current 3D battle + HUD framebuffer and writes it as `filename`.
    pub fn save_battle_shot(
        &mut self,
        battle: &BattleController,
        filename: &str,
    ) -> Result<(), String> {
        let pixels = battle.scene.pixels()?;
        let path = self.screenshot_dir.join(filename);
        write_png(&path, battle.size[0] as u32, battle.size[1] as u32, &pixels)?;
        self.screenshots_written.push(filename.to_string());
        Ok(())
    }

    /// Feeds synthetic combat inputs (`KeyW`, `KeyA`/`KeyD`, mouse look, `Mouse0` fire,
    /// `Space` jump, `KeyR` reload) into `battle.input` without allocating on the heap.
    pub fn apply_generated_combat_input(&mut self, battle: &mut BattleController) {
        // If the player died and returned to the spawn screen during combat, respawn immediately.
        if battle.spawn_ctrl.is_open() {
            battle.spawn_ctrl.spawn(&mut battle.spawn_port);
            battle.flush_spawn_requests();
        }
        self.input_tick = self.input_tick.wrapping_add(1);
        let t = self.input_tick;

        if t.is_multiple_of(30) && battle.spawn_ctrl.is_open() {
            battle.trigger_spawn();
        }

        // Hold forward movement (`KeyW`) and alternate left/right strafing (`KeyA` / `KeyD`).
        battle.input.key(KeyCode::KeyW, true);
        let strafe_left = (t / 90).is_multiple_of(2);
        battle.input.key(KeyCode::KeyA, strafe_left);
        battle.input.key(KeyCode::KeyD, !strafe_left);

        // Sweep yaw and pitch smoothly so the camera scans across combat units and posts.
        let dx = ((t as f64) * 0.035).cos() * 3.2;
        let dy = ((t as f64) * 0.02).sin() * 0.6;
        battle.input.motion(dx, dy);

        // Pulse primary weapon fire (`MouseButton::Left`) in bursts.
        let firing = (t % 45) < 18;
        battle.input.mouse(MouseButton::Left, firing);

        // Periodically jump (`Space`) and reload (`KeyR`).
        if t.is_multiple_of(180) {
            battle.input.key(KeyCode::Space, true);
        } else if t % 180 == 5 {
            battle.input.key(KeyCode::Space, false);
        }

        if t.is_multiple_of(240) {
            battle.input.key(KeyCode::KeyR, true);
        } else if t % 240 == 4 {
            battle.input.key(KeyCode::KeyR, false);
        }
    }

    // A diagnostic driver uses the same pointer counts and trigger edges as
    // a human. It never changes authoritative positions, health or scores.
    fn duel_frame(
        &mut self,
        battle: &mut BattleController,
        settings: &mut AppSettingsStore,
    ) -> Result<(), String> {
        use super::battle::BattleAuthority;
        use bfe_presentation::network_layout::{net_kind as k, net_record as r};
        let second = self.phase_started.elapsed().as_secs();
        if let BattleAuthority::Online { client, map_id, .. } = &battle.authority {
            let own = client.receiver.own;
            let changed = own.health != self.observed_health
                || own.alive != self.observed_alive
                || *map_id != self.observed_map
                || battle.scoreboard_ctrl.ended != self.observed_end;
            if second != self.observed_second || changed {
                self.observed_second = second;
                self.observed_health = own.health;
                self.observed_alive = own.alive;
                self.observed_end = battle.scoreboard_ctrl.ended;
                self.observed_map = map_id.clone();
                let roster: Vec<_> = client.receiver.roster.rows().filter(|(_,c)|c.human).map(|(slot,c)|serde_json::json!({"slot":slot,"team":c.team,"name":c.name(),"kills":c.kills,"deaths":c.deaths})).collect();
                let targets: Vec<_> = client.receiver.active().filter(|row| row[r::KIND] == k::SOLDIER as f64).map(|row|serde_json::json!({"id":row[r::ID],"health":row[r::HEALTH],"position":&row[r::POSITION..r::POSITION+3]})).collect();
                let audio = battle.scene.online.as_ref().map(|p|serde_json::json!({"shots":p.sound.events,"eventCounts":p.sound.event_counts.to_vec(),"frames":p.sound.device_frames(),"peak":p.sound.peak(),"decoded":p.sound.decoded,"voiceDecoded":p.sound.voice_decoded}));
                println!(
                    "[online-observation] {}",
                    serde_json::json!({"second":second,"map":map_id,"own":client.summary(),"health":own.health,"spawnScreen":battle.spawn_ctrl.is_open(),"endScreen":battle.scoreboard_ctrl.ended,"roster":roster,"targets":targets,"audio":audio})
                );
                if changed || (battle.scoreboard_ctrl.ended && second.is_multiple_of(5)) {
                    self.save_battle_shot(
                        battle,
                        &format!("online-{second:04}-health-{}.png", own.health as i32),
                    )?;
                }
            }
        }
        if battle.scoreboard_ctrl.ended {
            if battle.scoreboard_port.next_ready {
                battle.scoreboard_port.done_triggered = true;
            }
            return Ok(());
        }
        if battle.spawn_ctrl.is_open() {
            if let BattleAuthority::Online { client, .. } = &battle.authority {
                // Wait for the server's initial correction before selecting a
                // side or consuming the spawn screen. These are ordinary
                // eligible command-post selections, not position overrides.
                if !client.receiver.complete || client.receiver.corrections == 0 {
                    return Ok(());
                }
                if let Some(post) = self.spawn_posts.get(&client.receiver.own.team) {
                    battle
                        .spawn_ctrl
                        .model
                        .select_post(&battle.spawn_port, post);
                }
            }
            battle.trigger_spawn();
            return Ok(());
        }
        // The driver runs at render frequency, while pointer counts are
        // consumed by the fixed input step. Repeating the same correction
        // between steps multiplies the turn on an uncapped renderer.
        if self.duel_input_tick == battle.input_tick {
            return Ok(());
        }
        self.duel_input_tick = battle.input_tick;
        if let BattleAuthority::Online { client, .. } = &battle.authority {
            let own = client.receiver.own;
            let target = client.receiver.active().find(|row| {
                row[r::KIND] == k::SOLDIER as f64
                    && (row[r::TEAM] == 1. || row[r::TEAM] == 2.)
                    && row[r::TEAM] != own.team as f64
                    && row[r::HEALTH] > 0.
            });
            if let Some(target) = target {
                let dx = target[r::POSITION] - own.position[0];
                let dz = target[r::POSITION + 2] - own.position[2];
                let range = dx.hypot(dz);
                // AimView.Basis faces (sin(yaw), cos(yaw)) in the XZ plane.
                let yaw = dx.atan2(dz);
                let pitch = (target[r::POSITION + 1] - own.position[1]).atan2(range);
                let delta = (yaw - client.sender.yaw + std::f64::consts::PI)
                    .rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                let turn = client.replay.body.as_ref().map_or(4., |b| b.max_turn);
                let scale = settings.player.shell.mouse_sensitivity as f64 * turn / 2048.;
                // Soldier.look caps displacement within each server step.
                // Approach the target with ordinary pointer motion instead
                // of asking for an instantaneous half-turn.
                battle.input.motion(
                    -delta.clamp(-0.04, 0.04) / scale,
                    -(pitch - client.sender.pitch).clamp(-0.04, 0.04) / scale,
                );
                battle.input.key(KeyCode::KeyW, range > 12.);
                battle.input.key(KeyCode::Space, second.is_multiple_of(7));
                // Alternate the firing side, leaving a visible victim HUD.
                // Preserve ammunition while walking between distant spawns.
                battle.input.mouse(
                    MouseButton::Left,
                    own.team == ((second / 45) % 2 + 1) as u8 && range < 30. && second % 8 < 5,
                );
                battle.input.key(KeyCode::KeyR, second % 8 == 6);
            }
        }
        Ok(())
    }

    /// Updates cumulative allocation and contention metrics from the active `BattleController`.
    pub fn accumulate_battle_metrics(&mut self, battle: &BattleController) {
        // Retained counters cover the shared online/Instant Action sound
        // adapter. Formatting is deferred until a diagnostic report is saved.
        if let Some(scene) = &battle.scene.online {
            for (total, count) in self.audio_events.iter_mut().zip(scene.sound.event_counts) {
                *total = (*total).max(count);
            }
            self.voice_decodes = self.voice_decodes.max(scene.sound.voice_decoded);
            self.audio_frames = self.audio_frames.max(scene.sound.device_frames());
            self.audio_peak = self.audio_peak.max(scene.sound.peak());
        }
        self.steady_allocs_after_warmup = self
            .steady_allocs_after_warmup
            .max(battle.steady_allocs_after_warmup);
        self.steady_frames_measured += 1;
        self.world_renderer_blocks = self
            .world_renderer_blocks
            .max(battle.world_renderer_blocks());
    }

    /// Keep allocation counts beside each battle's frame-time measurements.
    fn record_render_allocations(&mut self, battle: &BattleController) {
        if let Some(row) = self.battles.last_mut() {
            let frames = battle.steady_frames_measured;
            row["playerSpawns"] = battle.player_spawns.into();
            row["steadyRenderAllocations"] = battle.steady_allocs_after_warmup.into();
            row["steadyRenderFrames"] = frames.into();
            row["renderAllocationsPerFrame"] = if frames > 0 {
                (battle.steady_allocs_after_warmup as f64 / frames as f64).into()
            } else {
                serde_json::Value::Null
            };
        }
    }

    /// Snapshots the 60 Hz world-thread tick-time distribution and host counts for `battle.map_id()`.
    fn record_world_tick_stats(&mut self, battle: &BattleController) {
        if let Some(stats) = battle.world_tick_stats() {
            let map = battle.map_id().to_string();
            if let Some(existing) = self.world_tick_stats.iter_mut().find(|(m, _)| m == &map) {
                existing.1 = stats;
            } else {
                self.world_tick_stats.push((map, stats));
            }
        }
        // Preserve completed battle measurements even if a later join fails.
        if let Err(error) = self.write_summary_report() {
            eprintln!("cannot save partial gate summary: {error}");
        }
    }

    // Exercise the same ShellHost used by interactive menus, with enough time
    // for a full intro and a 60-second loop measurement before navigating.
    fn step_movies(&mut self, shell: &mut ShellHost) -> Result<Option<ShellTransition>, String> {
        if self.movie_short {
            let elapsed = self.phase_started.elapsed().as_secs_f64();
            let delays = [1., 2., 1., 2.];
            if self.movie_stage >= delays.len() || elapsed < delays[self.movie_stage] {
                return Ok(None);
            }
            match self.movie_stage {
                0 => shell.app.unlock_gate(),
                1 => self.save_shell_shot(shell, "movie-page-main-a.png")?,
                2 => {
                    self.save_shell_shot(shell, "movie-page-main-b.png")?;
                    shell.app.push_screen(ScreenId::InstantAction);
                }
                3 => {
                    self.save_shell_shot(shell, "movie-page-flyover.png")?;
                    let summary = serde_json::json!({"theme":shell.app.settings.menu_theme,
                        "movieFrames":shell.movies.stats.decoded,"previewFrames":shell.preview_movies.stats.decoded,
                        "glErrors":self.gl_errors});
                    std::fs::write(
                        self.screenshot_dir.join("movie-page-summary.json"),
                        serde_json::to_vec_pretty(&summary).unwrap(),
                    )
                    .map_err(|e| e.to_string())?;
                    if shell.movies.stats.decoded < 30
                        || shell.preview_movies.stats.decoded < 10
                        || self.gl_errors != 0
                    {
                        return Err(
                            "Page movie capture did not present decoded frames cleanly".into()
                        );
                    }
                    self.movie_measurement = Some(summary);
                    self.transition_to(GatePhase::Finished);
                    return Ok(Some(ShellTransition::Exit));
                }
                _ => (),
            }
            self.movie_stage += 1;
            self.phase_started = Instant::now();
            return Ok(None);
        }
        let elapsed = self.phase_started.elapsed().as_secs_f64();
        let deadlines = [1., 0.35, 2., 60., 0.2, 2.5, 0.2, 2., 3.];
        if self.movie_stage >= deadlines.len() || elapsed < deadlines[self.movie_stage] {
            return Ok(None);
        }
        match self.movie_stage {
            0 => {
                self.save_shell_shot(shell, "movie-gate.png")?;
                shell.app.unlock_gate();
            }
            1 => {
                self.save_shell_shot(shell, "movie-main-intro.png")?;
            }
            2 => {
                self.save_shell_shot(shell, "movie-main-loop-start.png")?;
                let stats = &shell.movies.stats;
                self.movie_baseline = Some((
                    Instant::now(),
                    stats.decoded,
                    stats.dropped,
                    stats.decode_ms,
                ));
                shell.movie_allocs_max = 0;
            }
            3 => {
                self.save_shell_shot(shell, "movie-main-loop.png")?;
                let (start, decoded, dropped, decode_ms) = self.movie_baseline.take().unwrap();
                let stats = &shell.movies.stats;
                let frames = stats.decoded - decoded;
                self.movie_measurement = Some(serde_json::json!({
                    "seconds": start.elapsed().as_secs_f64(),
                    "decodedFrames": frames, "droppedFrames": stats.dropped-dropped,
                    "decodeMsPerFrame": (stats.decode_ms-decode_ms)/frames.max(1) as f64,
                    "rustMovieAllocationsPerFrameMax": shell.movie_allocs_max,
                }));
                shell.app.push_screen(ScreenId::Singleplayer);
            }
            4 => {
                self.save_shell_shot(shell, "movie-forward-crossfade.png")?;
            }
            5 => {
                self.save_shell_shot(shell, "movie-singleplayer-loop.png")?;
                shell.app.back();
            }
            6 => {
                self.save_shell_shot(shell, "movie-back-crossfade.png")?;
            }
            7 => {
                self.save_shell_shot(shell, "movie-back-loop.png")?;
                shell.app.push_screen(ScreenId::InstantAction);
            }
            8 => {
                self.save_shell_shot(shell, "movie-flyover.png")?;
                let stats = &shell.movies.stats;
                let preview = &shell.preview_movies.stats;
                let summary = serde_json::json!({
                    "mainLoopMeasurement": self.movie_measurement,
                    "totalDecodedFrames": stats.decoded, "totalDroppedFrames": stats.dropped,
                    "previewFrames": preview.decoded, "previewDropped": preview.dropped,
                    "glErrors": self.gl_errors,
                });
                std::fs::write(
                    self.screenshot_dir.join("movie-summary.json"),
                    serde_json::to_vec_pretty(&summary).unwrap(),
                )
                .map_err(|e| e.to_string())?;
                if stats.decoded < 1000 || preview.decoded < 10 {
                    return Err("Movie verification did not present enough decoded frames".into());
                }
                self.transition_to(GatePhase::Finished);
                return Ok(Some(ShellTransition::Exit));
            }
            _ => (),
        }
        self.movie_stage += 1;
        self.phase_started = Instant::now();
        Ok(None)
    }

    /// Steps the gate choreography while in the front-end shell.
    pub fn step_shell(
        &mut self,
        shell: &mut ShellHost,
        settings: &mut AppSettingsStore,
    ) -> Result<Option<ShellTransition>, String> {
        if self.phase != GatePhase::StartupLoader {
            if let Some(mut proof) = self.page_proof.take() {
                let result = proof.step(self, shell, settings);
                self.page_proof = Some(proof);
                return result;
            }
        }
        self.phase_frames += 1;
        let elapsed = self.phase_started.elapsed().as_secs_f64();

        if self.movie_verification {
            return self.step_movies(shell);
        }

        match self.phase {
            GatePhase::GateScreen => {
                // Capture `02-gate.png` on the click-to-continue screen, then unlock into Main Menu.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "02-gate.png")?;
                    shell.app.unlock_gate();
                    if self.online_duel {
                        self.transition_to(GatePhase::MainMenuAfterIa);
                    } else if self.target_map.is_some() {
                        self.transition_to(GatePhase::LaunchIaPlaylist);
                    } else {
                        self.transition_to(GatePhase::MainMenu);
                    }
                }
            }
            GatePhase::MainMenu => {
                // Capture `03-main-menu.png`, then navigate into `SINGLEPLAYER`.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "03-main-menu.png")?;
                    shell.app.push_screen(ScreenId::Singleplayer);
                    self.transition_to(GatePhase::SingleplayerMenu);
                }
            }
            GatePhase::SingleplayerMenu => {
                // Capture `04-singleplayer.png`, configure the 3-map playlist (`nab2`, `tat1`, `hot1`),
                // and open `INSTANT ACTION`.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "04-singleplayer.png")?;
                    shell.app.ia_entries = vec![
                        PlaylistEntry::new("nab2", Some("cw".to_string())),
                        PlaylistEntry::new("tat1", Some("cw".to_string())),
                        PlaylistEntry::new("hot1", Some("gcw".to_string())),
                    ];
                    let _ = settings.capture_from_shell_app(&shell.app);
                    shell.app.push_screen(ScreenId::InstantAction);
                    self.transition_to(GatePhase::InstantActionMenu);
                }
            }
            GatePhase::InstantActionMenu => {
                // Capture `05-instant-action.png`, then open `bfe-interface` `OPTIONS`.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "05-instant-action.png")?;
                    shell.app.push_screen(ScreenId::Options);
                    self.transition_to(GatePhase::OptionsMenu);
                }
            }
            GatePhase::OptionsMenu => {
                // Capture `06-options.png` rendered by `bfe-interface::ClassicTheme`, then open `PROFILE`.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "06-options.png")?;
                    shell.app.push_screen(ScreenId::Profile);
                    self.transition_to(GatePhase::ProfileMenu);
                }
            }
            GatePhase::ProfileMenu => {
                // Capture `07-profile.png`, then return to `INSTANT ACTION` to launch the playlist.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "07-profile.png")?;
                    shell.app.push_screen(ScreenId::InstantAction);
                    self.transition_to(GatePhase::LaunchIaPlaylist);
                }
            }
            GatePhase::LaunchIaPlaylist => {
                // Emit `ShellTransition::StartInstantAction` for the 3-map rotation (`nab2` -> `tat1` -> `hot1`).
                if self.phase_frames >= 4 {
                    let req = if let Some(map) = &self.target_map {
                        let era = if map == "hot1" { "gcw" } else { "cw" };
                        let playlist =
                            vec![PlaylistEntry::new(map.as_str(), Some(era.to_string()))];
                        BattleRequest::from_rotation(
                            Some(map.as_str()),
                            Some(era),
                            playlist,
                            0,
                            false,
                        )?
                    } else {
                        let playlist = vec![
                            PlaylistEntry::new("nab2", Some("cw".to_string())),
                            PlaylistEntry::new("tat1", Some("cw".to_string())),
                            PlaylistEntry::new("hot1", Some("gcw".to_string())),
                        ];
                        BattleRequest::from_rotation(Some("nab2"), Some("cw"), playlist, 0, false)?
                    };
                    self.transition_to(GatePhase::IaNab2Spawn);
                    return Ok(Some(ShellTransition::StartInstantAction(req)));
                }
            }
            GatePhase::MainMenuAfterIa => {
                // Capture `18-main-menu-return.png` after quitting battle 3, then open `MULTIPLAYER`.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    if self.target_map.is_some() {
                        self.transition_to(GatePhase::Finished);
                        return Ok(Some(ShellTransition::Exit));
                    }
                    self.save_shell_shot(shell, "18-main-menu-return.png")?;
                    shell.app.mp_needs_notice = false;
                    shell.browser.trigger_refresh();
                    shell.app.push_screen(ScreenId::Multiplayer);
                    self.transition_to(GatePhase::MultiplayerBrowser);
                }
            }
            GatePhase::MultiplayerBrowser => {
                // Capture `19-multiplayer-browser.png` once the server list refreshes, then join `bfeserver`.
                if self.phase_frames >= 18 && elapsed >= 0.6 {
                    shell.app.sync_from_browser(&shell.browser);
                    self.save_shell_shot(shell, "19-multiplayer-browser.png")?;
                    // An explicit script target takes precedence over the row
                    // selected by a background LAN/browser refresh.
                    let target = shell
                        .browser
                        .server_target()
                        .map(String::from)
                        .or_else(|| std::env::var("BFE_SERVER_TARGET").ok())
                        .or_else(|| {
                            shell.browser.selected().and_then(|server| {
                                server.address.as_ref().map(|address| {
                                    format!("{address}:{}", server.port.unwrap_or(27015))
                                })
                            })
                        })
                        .unwrap_or_else(|| "127.0.0.1:27015".to_string());
                    let (host, port) = match target.rsplit_once(':') {
                        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(27015)),
                        None => (target.clone(), 27015),
                    };
                    self.transition_to(GatePhase::OnlineSpawn);
                    return Ok(Some(ShellTransition::StartOnline(MultiplayerLaunch {
                        kind: MultiplayerKind::Online,
                        server_id: target,
                        family: GameFamily::Swbf1,
                        map: "nab2".to_string(),
                        side: "rep".to_string(),
                        era: "cw".to_string(),
                        mode: "conquest".to_string(),
                        address: Some(host),
                        port: Some(port),
                        relay_always: false,
                        player_name: settings.player.profile_name.clone(),
                    })));
                }
            }
            GatePhase::MultiplayerAfterOnline if self.phase_frames >= 8 && elapsed >= 0.25 => {
                // Capture `24-multiplayer-return.png` after leaving the online battle and exit cleanly.
                self.save_shell_shot(shell, "24-multiplayer-return.png")?;
                self.transition_to(GatePhase::Finished);
                return Ok(Some(ShellTransition::Exit));
            }
            _ => {}
        }
        Ok(None)
    }

    /// Steps the gate choreography while in an active battle.
    pub fn step_battle(
        &mut self,
        battle: &mut BattleController,
        settings: &mut AppSettingsStore,
    ) -> Result<(), String> {
        self.phase_frames += 1;
        let elapsed = self.phase_started.elapsed().as_secs_f64();
        self.accumulate_battle_metrics(battle);

        if self.online_duel {
            self.duel_frame(battle, settings)?;
            if elapsed >= self.battle_duration_s {
                self.online_battle_completed = true;
                battle.pause_menu.window_close_triggered += 1;
            }
            return Ok(());
        }

        match self.phase {
            GatePhase::IaNab2Spawn => {
                // Wait for the admitted world to publish its initial spawn state (`classes` and `posts`),
                // capture `09-ia-nab2-spawn.png`, and spawn into `nab2`.
                if self.phase_frames >= 12 && elapsed >= 0.35 {
                    self.save_battle_shot(battle, "09-ia-nab2-spawn.png")?;
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::IaNab2Combat);
                }
            }
            GatePhase::IaNab2Combat => {
                // Drive real combat on `nab2`, capturing `10-ia-nab2-battle.png` once spawned.
                self.record_combat_frame_interval();
                self.apply_generated_combat_input(battle);
                if self.phase_frames == 45 {
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    let shot_name = if let Some(map) = &self.target_map {
                        format!("ia-{map}-battle.png")
                    } else {
                        "10-ia-nab2-battle.png".to_string()
                    };
                    self.save_battle_shot(battle, &shot_name)?;
                    self.reset_combat_measurement_window();
                    return Ok(());
                }
                if elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    self.finish_combat_metrics(battle.map_id(), self.phase_frames, elapsed);
                    self.record_render_allocations(battle);
                    battle.input.clear();
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    if self.target_map.is_some() {
                        battle.handle_key("Escape", true, false, settings);
                        battle.handle_key("Escape", false, false, settings);
                        self.transition_to(GatePhase::IaBes1PauseMain);
                    } else {
                        battle.set_scoreboard_page(BoardPage::Table);
                        self.transition_to(GatePhase::IaNab2ScoreboardTable);
                    }
                }
            }
            GatePhase::IaNab2ScoreboardTable => {
                // Capture `11-ia-nab2-scoreboard.png` showing real bot kills and captures, then switch to Details.
                if self.phase_frames >= 24 && elapsed >= 0.55 {
                    self.save_battle_shot(battle, "11-ia-nab2-scoreboard.png")?;
                    battle.set_scoreboard_page(BoardPage::Details);
                    self.transition_to(GatePhase::IaNab2ScoreboardDetails);
                }
            }
            GatePhase::IaNab2ScoreboardDetails => {
                // Capture `12-ia-nab2-scoreboard-details.png`, then open post-battle Awards with `DONE`.
                if self.phase_frames >= 24 && elapsed >= 0.55 {
                    self.save_battle_shot(battle, "12-ia-nab2-scoreboard-details.png")?;
                    battle.force_victory_for_gate();
                    battle.set_scoreboard_page(BoardPage::Awards);
                    self.transition_to(GatePhase::IaNab2ScoreboardAwards);
                }
            }
            GatePhase::IaNab2ScoreboardAwards => {
                // Capture `13-ia-nab2-scoreboard-awards.png`, record `nab2` world tick stats, and advance to `tat1`.
                if self.phase_frames >= 24 && elapsed >= 0.55 {
                    self.save_battle_shot(battle, "13-ia-nab2-scoreboard-awards.png")?;
                    self.record_world_tick_stats(battle);
                    self.ia_battles_completed += 1;
                    battle.scoreboard_port.done_triggered = true;
                    self.transition_to(GatePhase::IaTat1Spawn);
                }
            }
            GatePhase::IaTat1Spawn => {
                // Spawn into playlist battle 2 (`tat1`).
                if self.phase_frames >= 10 && elapsed >= 0.25 {
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::IaTat1Combat);
                }
            }
            GatePhase::IaTat1Combat => {
                // Drive real combat on `tat1`, capture `14-ia-tat1-battle.png`, and advance to `hot1`.
                self.record_combat_frame_interval();
                self.apply_generated_combat_input(battle);
                if self.phase_frames == 45 {
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    self.save_battle_shot(battle, "14-ia-tat1-battle.png")?;
                    self.reset_combat_measurement_window();
                    return Ok(());
                }
                if elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    self.finish_combat_metrics(battle.map_id(), self.phase_frames, elapsed);
                    self.record_render_allocations(battle);
                    battle.input.clear();
                    self.record_world_tick_stats(battle);
                    self.ia_battles_completed += 1;
                    battle.scoreboard_port.done_triggered = true;
                    self.transition_to(GatePhase::IaBes1Spawn);
                }
            }
            GatePhase::IaBes1Spawn => {
                // Spawn into playlist battle 3 (`hot1`).
                if self.phase_frames >= 10 && elapsed >= 0.25 {
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::IaBes1Combat);
                }
            }
            GatePhase::IaBes1Combat => {
                // Drive real combat on `hot1`, capture `15-ia-bes1-battle.png`, then open `ESC` pause.
                self.record_combat_frame_interval();
                self.apply_generated_combat_input(battle);
                if self.phase_frames == 45 {
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    self.save_battle_shot(battle, "15-ia-bes1-battle.png")?;
                    self.reset_combat_measurement_window();
                    return Ok(());
                }
                if elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    self.finish_combat_metrics(battle.map_id(), self.phase_frames, elapsed);
                    self.record_render_allocations(battle);
                    battle.input.clear();
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    if battle.scoreboard_ctrl.model.is_open() {
                        battle.scoreboard_ctrl.model.close();
                    }
                    battle.handle_key("Escape", true, false, settings);
                    battle.handle_key("Escape", false, false, settings);
                    self.transition_to(GatePhase::IaBes1PauseMain);
                }
            }
            GatePhase::IaBes1PauseMain => {
                // Capture `16-ia-bes1-pause.png`, then open the Quit-to-Main-Menu confirmation modal.
                if self.phase_frames >= 12 && elapsed >= 0.3 {
                    self.save_battle_shot(battle, "16-ia-bes1-pause.png")?;
                    battle.open_pause_confirm(ConfirmKind::Quit);
                    self.transition_to(GatePhase::IaBes1PauseQuit);
                }
            }
            GatePhase::IaBes1PauseQuit => {
                // Capture `17-ia-bes1-pause-quit.png`, record `hot1` world tick stats, and confirm quit.
                if self.phase_frames >= 10 && elapsed >= 0.25 {
                    self.save_battle_shot(battle, "17-ia-bes1-pause-quit.png")?;
                    self.record_world_tick_stats(battle);
                    self.ia_battles_completed += 1;
                    battle.handle_key("ArrowLeft", true, false, settings);
                    battle.handle_key("Enter", true, false, settings);
                    self.transition_to(GatePhase::MainMenuAfterIa);
                }
            }
            GatePhase::OnlineSpawn => {
                // Capture `20-online-spawn.png` on the live `bfeserver` match and trigger spawn.
                if self.phase_frames >= 15 && elapsed >= 0.4 {
                    self.save_battle_shot(battle, "20-online-spawn.png")?;
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::OnlineCombat);
                }
            }
            GatePhase::OnlineCombat => {
                // Drive online combat against `bfeserver` and capture `21-online-battle.png`.
                self.record_combat_frame_interval();
                self.apply_generated_combat_input(battle);
                if self.phase_frames >= 45
                    && battle.was_alive
                    && !self
                        .screenshots_written
                        .iter()
                        .any(|name| name == "21-online-battle.png")
                {
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    self.save_battle_shot(battle, "21-online-battle.png")?;
                    self.reset_combat_measurement_window();
                    return Ok(());
                }
                let captured = self
                    .screenshots_written
                    .iter()
                    .any(|name| name == "21-online-battle.png");
                if !captured && elapsed >= 15.0 {
                    return Err("online player did not spawn for the combat capture".into());
                }
                if captured && elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    self.finish_combat_metrics(battle.map_id(), self.phase_frames, elapsed);
                    self.record_render_allocations(battle);
                    battle.input.clear();
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    battle.set_scoreboard_page(BoardPage::Table);
                    self.transition_to(GatePhase::OnlineScoreboard);
                }
            }
            GatePhase::OnlineScoreboard => {
                // Capture `22-online-scoreboard.png`, close scoreboard, and open the online pause menu.
                if self.phase_frames >= 24 && elapsed >= 0.55 {
                    self.save_battle_shot(battle, "22-online-scoreboard.png")?;
                    battle.handle_key("Tab", false, false, settings);
                    battle.scoreboard_ctrl.model.close();
                    battle.handle_key("Escape", true, false, settings);
                    battle.handle_key("Escape", false, false, settings);
                    self.transition_to(GatePhase::OnlinePause);
                }
            }
            GatePhase::OnlinePause => {
                // Capture `23-online-pause.png` and confirm leaving back to the Multiplayer Server Browser.
                if self.phase_frames >= 12 && elapsed >= 0.3 {
                    self.save_battle_shot(battle, "23-online-pause.png")?;
                    self.online_battle_completed = true;
                    battle.open_pause_confirm(ConfirmKind::Quit);
                    battle.handle_key("ArrowLeft", true, false, settings);
                    battle.handle_key("Enter", true, false, settings);
                    self.transition_to(GatePhase::MultiplayerAfterOnline);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Serializes the gate verification summary (`gate-summary.json`), including GL errors,
    /// steady-state frame allocations, world/renderer blocks, and the 60 Hz world-thread
    /// tick-time distribution (`min`, `p50`, `p95`, `p99`, `max`) and host proof counts.
    pub fn write_summary_report(&self) -> Result<serde_json::Value, String> {
        let mut world_stats_map = serde_json::Map::new();
        for (map, st) in &self.world_tick_stats {
            world_stats_map.insert(
                map.clone(),
                serde_json::to_value(st).unwrap_or(serde_json::Value::Null),
            );
        }
        let last_battle = self
            .battles
            .last()
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        // An interrupted run must not leave a success receipt. Require the
        // completed choreography and actual world ticks before claiming proof.
        let expected_battles = if self.target_map.is_some() { 1 } else { 3 };
        let choreography_complete = if self.movie_verification {
            self.phase == GatePhase::Finished
        } else if self.online_duel {
            self.online_battle_completed
        } else {
            self.phase == GatePhase::Finished
                && self.ia_battles_completed == expected_battles
                && (self.target_map.is_some() || self.online_battle_completed)
                && self.battles.len() == if self.target_map.is_some() { 1 } else { 4 }
                && self.battles.iter().all(|battle| {
                    battle["playerSpawns"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                })
                && self.world_tick_stats.len() == expected_battles
                && self
                    .world_tick_stats
                    .iter()
                    .all(|(_, stats)| stats.count > 0 && !stats.context_poisoned)
        };
        let passed = choreography_complete
            && self.gl_errors == 0
            && self.world_renderer_blocks == 0
            && self.steady_frames_measured > 0;
        // Movie captures prove presentation only; keep the battle gate's
        // requirement for measured world frames intact.
        let movie_captured = self.movie_verification
            && choreography_complete
            && self.movie_measurement.is_some()
            && self.gl_errors == 0;
        let summary = serde_json::json!({
            "status": if self.page_proof.is_some() { if self.gl_errors != 0 { "FAIL" } else if self.page_capture_done { "CAPTURED" } else { "INCOMPLETE" } } else if movie_captured { "CAPTURED" } else if passed { "PASS" } else { "FAIL" },
            "captureOnly": self.page_proof.is_some() || self.movie_verification,
            "glErrors": self.gl_errors,
            "audio": {"eventCounts":self.audio_events.to_vec(),"voiceDecoded":self.voice_decodes,"frames":self.audio_frames,"peak":self.audio_peak},
            "steadyFrameAllocationsAfterWarmup": self.steady_allocs_after_warmup,
            "steadyFramesMeasured": self.steady_frames_measured,
            "worldRendererBlocks": self.world_renderer_blocks,
            "worldThreadTickTimeMs": world_stats_map,
            "iaBattlesCompleted": self.ia_battles_completed,
            "onlineBattleCompleted": self.online_battle_completed,
            "battleDurationSeconds": self.battle_duration_s,
            "battles": self.battles,
            "lastBattle": last_battle,
            "screenshotsDir": self.screenshot_dir,
            "screenshotsWritten": self.screenshots_written,
        });
        std::fs::create_dir_all(&self.screenshot_dir).map_err(|e| e.to_string())?;
        let path = self.screenshot_dir.join("gate-summary.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("{summary}");
        Ok(summary)
    }
}

/// Encodes bottom-up RGBA8 framebuffer pixels as an uncompressed PNG file.
///
/// Flips rows from OpenGL's bottom-up origin to PNG's top-down scanline order,
/// wraps scanlines in stored RFC 1950 / DEFLATE blocks with an Adler-32 checksum,
/// and writes standard `IHDR`, `IDAT`, and `IEND` chunks.
pub fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    rgba_bottom_up: &[u8],
) -> Result<(), String> {
    let row_bytes = (width as usize) * 4;
    if rgba_bottom_up.len() < row_bytes * (height as usize) {
        return Err("insufficient RGBA bytes for PNG".to_string());
    }

    // Build top-down scanlines with filter byte `0` (None) prepended to each row.
    let mut raw = Vec::with_capacity((row_bytes + 1) * (height as usize));
    for y in 0..(height as usize) {
        let src_row = height as usize - 1 - y;
        let start = src_row * row_bytes;
        raw.push(0u8);
        raw.extend_from_slice(&rgba_bottom_up[start..start + row_bytes]);
    }

    // Wrap raw scanlines in uncompressed zlib/DEFLATE blocks (`0x78 0x01`) + Adler-32.
    let mut zlib = Vec::with_capacity(raw.len() + (raw.len() / 65535 + 1) * 5 + 6);
    zlib.push(0x78);
    zlib.push(0x01);

    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i + 1 == chunks.len();
        zlib.push(if is_last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        let nlen = !len;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&nlen.to_le_bytes());
        zlib.extend_from_slice(chunk);
    }

    let adler = adler32(&raw);
    zlib.extend_from_slice(&adler.to_be_bytes());

    // Emit PNG magic header followed by `IHDR`, `IDAT`, and `IEND`.
    let mut out = Vec::with_capacity(zlib.len() + 64);
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(6);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    write_png_chunk(&mut out, b"IHDR", &ihdr);
    write_png_chunk(&mut out, b"IDAT", &zlib);
    write_png_chunk(&mut out, b"IEND", &[]);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

/// Appends a single length-prefixed, CRC32-suffixed PNG chunk (`type` + `data`) to `out`.
fn write_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc = 0xFFFF_FFFFu32;
    for &b in chunk_type.iter().chain(data.iter()) {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}

/// Computes the RFC 1950 Adler-32 checksum over `data`.
fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_tour_never_claims_a_battle_gate_pass() {
        let dir = std::env::temp_dir().join(format!("bfe-page-receipt-{}", std::process::id()));
        let mut tour = GateDriver::new("page-parity", Some(dir.clone()));
        assert_eq!(tour.write_summary_report().unwrap()["status"], "INCOMPLETE");
        tour.page_capture_done = true;
        let receipt = tour.write_summary_report().unwrap();
        assert_eq!(receipt["status"], "CAPTURED");
        assert_eq!(receipt["steadyFramesMeasured"], 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_gate_writes_failure_receipt() {
        let dir = std::env::temp_dir().join(format!("bfe-gate-receipt-{}", std::process::id()));
        let gate = GateDriver::new("gate", Some(dir.clone()));
        let summary = gate
            .write_summary_report()
            .expect("write interrupted receipt");
        assert_eq!(summary["status"], "FAIL");
        std::fs::remove_dir_all(dir).expect("remove receipt");
    }

    #[test]
    fn completed_gate_reports_allocations_without_rejecting_battles() {
        let dir = std::env::temp_dir().join(format!("bfe-gate-completed-{}", std::process::id()));
        let mut gate = GateDriver::new("gate", Some(dir.clone()));
        gate.phase = GatePhase::Finished;
        gate.ia_battles_completed = 3;
        gate.online_battle_completed = true;
        gate.steady_frames_measured = 100;
        gate.steady_allocs_after_warmup = 50;
        gate.battles = (0..4)
            .map(|_| serde_json::json!({"playerSpawns":1}))
            .collect();
        for map in ["nab2", "tat1", "hot1"] {
            gate.world_tick_stats.push((
                map.into(),
                WorldTickStats {
                    count: 1,
                    ..Default::default()
                },
            ));
        }
        let summary = gate
            .write_summary_report()
            .expect("write completed receipt");
        assert_eq!(summary["status"], "PASS");
        assert_eq!(summary["steadyFrameAllocationsAfterWarmup"], 50);
        gate.battles[3]["playerSpawns"] = 0.into();
        assert_eq!(gate.write_summary_report().unwrap()["status"], "FAIL");
        std::fs::remove_dir_all(dir).expect("remove receipt");
    }

    /// Verifies that `write_png` writes a valid PNG file with the expected magic header and IEND chunk.
    #[test]
    fn write_png_produces_valid_png_signature_and_iend() {
        let dir = std::env::temp_dir().join("bfe-client-png-test");
        let path = dir.join("test.png");
        let pixels = vec![255u8; 4 * 4 * 4];
        write_png(&path, 4, 4, &pixels).expect("write_png");
        let bytes = std::fs::read(&path).expect("read png");
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(bytes.windows(4).any(|w| w == b"IEND"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
