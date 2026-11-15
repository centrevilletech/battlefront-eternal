//! `--script ia:<MAP>`: repeats Instant Action rounds on one map with
//! generated combat input until `BFE_GATE_BATTLE_SECONDS` (default 300) of
//! combat have elapsed, then quits through the pause menu and writes
//! `gate-summary.json` (GL errors, frame times, world tick times) beside the
//! screenshots (startup loader, gate, loading, spawn, battle, pause, quit).
//! `BFE_GATE_HUNT` (`droideka` or `soldier`) walks the player to the nearest
//! enemy of that kind and fires instead.

use crate::{
    app::{
        battle::BattleController,
        settings::AppSettingsStore,
        shell::{ShellHost, ShellTransition},
    },
    local::thread::WorldTickStats,
};
use bfe_battle_ui::pause::ConfirmKind;
use bfe_shell::launch::{BattleRequest, PlaylistEntry};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use winit::{event::MouseButton, keyboard::KeyCode};

/// The steps of an `ia:` run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatePhase {
    /// Startup content loader (`01-startup-loader.png`).
    StartupLoader,
    /// Click-to-continue gate (`02-gate.png`).
    GateScreen,
    /// Starts the battle on the target map.
    Launch,
    /// Spawn screen (`09-ia-nab2-spawn.png`), then spawn.
    Spawn,
    /// Combat with generated input (`ia-<MAP>-battle.png`), repeated rounds.
    Combat,
    /// Pause menu (`16-ia-bes1-pause.png`).
    Pause,
    /// Quit confirmation (`17-ia-bes1-pause-quit.png`).
    PauseQuit,
    /// Back in the menus; exits.
    Menu,
    /// Done; the summary is written.
    Finished,
}

/// Automated driver for `--script ia:<MAP>`.
///
/// Tracks the current [`GatePhase`], frame counts and times, screenshots, OpenGL
/// errors, world/renderer contention and the world thread's tick times.
pub struct GateDriver {
    /// `BFE_GATE_HUNT` (`droideka` or `soldier`): in a local battle the
    /// player walks up to the nearest living enemy of that kind and fires,
    /// so a recording of the run has what happens to it in view.
    hunt: Option<String>,
    hunt_input_tick: u32,
    audio_events: [u64; 27],
    /// Unit presentation events shown, by `presented::SHOWN`'s names.
    presented: [u64; crate::online::replicas::presented::SHOWN.len()],
    voice_decodes: u64,
    station_loops: u64,
    audio_frames: u64,
    audio_peak: f32,
    /// Current phase in the 24-screenshot verification choreography.
    pub phase: GatePhase,
    /// Timestamp when the current phase began.
    pub phase_started: Instant,
    /// Number of frames rendered within the current phase.
    pub phase_frames: u64,
    /// Combat seconds to run (`BFE_GATE_BATTLE_SECONDS`, default 300).
    pub battle_duration_s: f64,
    /// The map of `--script ia:<MAP>`.
    pub target_map: String,
    /// Combat seconds from completed rounds, excluding admission and shell time.
    target_combat_seconds: f64,
    /// Output directory where PNG screenshots and `gate-summary.json` are written.
    pub screenshot_dir: PathBuf,
    /// Ordered list of screenshot filenames written so far.
    pub screenshots_written: Vec<String>,
    /// Cumulative OpenGL errors observed via `glGetError()` across all frames.
    pub gl_errors: u64,
    /// Total steady-state battle frames measured across all Instant Action and Online battles.
    pub steady_frames_measured: u64,
    /// Maximum contention blocks between the 60 Hz world thread and the render thread.
    pub world_renderer_blocks: u64,
    /// Per-map 60 Hz world-thread tick-time distribution (`min`, `p50`, `p95`, `p99`, `max`) and host counts.
    pub world_tick_stats: Vec<(String, WorldTickStats)>,
    /// Count of Instant Action playlist battles completed (`3` for `nab2`, `tat1`, `hot1`).
    pub ia_battles_completed: usize,
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
    /// Creates a new [`GateDriver`] for `--script ia:<map>`,
    /// honoring `BFE_GATE_BATTLE_SECONDS` when set in the environment.
    pub fn new(map: &str, screenshot_dir: Option<PathBuf>) -> Self {
        // Default to 300 seconds (5 minutes) per battle for the full gate or 3 seconds for quick smoke runs.
        let default_battle_secs = 300.0;
        let battle_duration_s = std::env::var("BFE_GATE_BATTLE_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(default_battle_secs);
        let target_map = map.trim().to_string();
        let dir =
            screenshot_dir.unwrap_or_else(|| std::env::temp_dir().join("bfe-gate-screenshots"));
        Self {
            hunt: std::env::var("BFE_GATE_HUNT").ok(),
            hunt_input_tick: u32::MAX,
            audio_events: [0; 27],
            presented: [0; crate::online::replicas::presented::SHOWN.len()],
            voice_decodes: 0,
            station_loops: 0,
            audio_frames: 0,
            audio_peak: 0.,
            phase: GatePhase::StartupLoader,
            phase_started: Instant::now(),
            phase_frames: 0,
            battle_duration_s,
            target_map,
            target_combat_seconds: 0.0,
            screenshot_dir: dir,
            screenshots_written: Vec::new(),
            gl_errors: 0,
            steady_frames_measured: 0,
            world_renderer_blocks: 0,
            world_tick_stats: Vec::new(),
            ia_battles_completed: 0,
            input_tick: 0,
            combat_frame_ms: Vec::with_capacity(32_768),
            last_combat_instant: None,
            battles: Vec::with_capacity(4),
        }
    }

    /// Transitions the driver to `next`, logging progress and resetting per-phase counters.
    pub fn transition_to(&mut self, next: GatePhase) {
        eprintln!(
            "[gate] phase {:?} -> {:?} (frames={}, world_blocks={})",
            self.phase,
            next,
            self.phase_frames,
            self.world_renderer_blocks
        );
        self.phase = next;
        self.phase_started = Instant::now();
        self.phase_frames = 0;
        if next == GatePhase::Combat {
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

    /// A completed conquest round must not end a longer single-map soak.
    /// The normal scoreboard auto-advances after thirty seconds; restart through
    /// the native mission transition before it can strand the driver in menus.
    fn repeat_target_round(&self, match_over: bool, elapsed: f64) -> bool {
        match_over
            && self.target_combat_seconds + elapsed < self.battle_duration_s
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
    // The hunt: turn to the nearest living enemy of the wanted kind with
    // ordinary pointer motion, walk until it is a few metres off, and fire
    // while it is in range. Once an input step.
    fn hunt_frame(&mut self, battle: &mut BattleController, settings: &AppSettingsStore) {
        use super::battle::BattleAuthority;
        use bfe_presentation::network_layout::{net_kind as k, net_record as r};
        if self.hunt_input_tick == battle.input_tick {
            return;
        }
        self.hunt_input_tick = battle.input_tick;
        // Killed, he spawns again as soon as the screen lets him.
        if battle.spawn_ctrl.is_open() {
            battle.input.key(KeyCode::KeyW, false);
            battle.input.mouse(MouseButton::Left, false);
            battle.trigger_spawn();
            return;
        }
        let BattleAuthority::InstantAction { receiver, sender, replay, .. } = &battle.authority else {
            return;
        };
        let own = receiver.own;
        // The wanted kind counts as three times nearer than the other, so
        // a droideka across the square is chosen over a droid at his side.
        let wanted = if self.hunt.as_deref() == Some("droideka") { k::DROIDEKA } else { k::SOLDIER } as f64;
        let nearest = receiver
            .active()
            .filter(|row| {
                (row[r::KIND] == k::DROIDEKA as f64 || row[r::KIND] == k::SOLDIER as f64)
                    && (row[r::TEAM] == 1. || row[r::TEAM] == 2.)
                    && row[r::TEAM] != own.team as f64
                    && row[r::HEALTH] > 0.
            })
            .map(|row| ((row[r::POSITION] - own.position[0]).hypot(row[r::POSITION + 2] - own.position[2]), row))
            .min_by(|a, b| {
                let weight = |(range, row): &(f64, &[f64; r::STRIDE])| if row[r::KIND] == wanted { *range } else { *range * 3. };
                weight(a).total_cmp(&weight(b))
            });
        let Some((range, target)) = nearest else {
            battle.input.key(KeyCode::KeyW, false);
            battle.input.mouse(MouseButton::Left, false);
            return;
        };
        // AimView.Basis faces (sin(yaw), cos(yaw)) in the XZ plane; the eye
        // is about as far above his feet as the target's middle is above its.
        let yaw = (target[r::POSITION] - own.position[0]).atan2(target[r::POSITION + 2] - own.position[2]);
        let pitch = (target[r::POSITION + 1] - own.position[1] - 0.6).atan2(range);
        let delta = (yaw - sender.yaw + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        let turn = replay.body.as_ref().map_or(4., |b| b.max_turn);
        let scale = settings.player.shell.mouse_sensitivity as f64 * turn / 2048.;
        battle.input.motion(-delta.clamp(-0.04, 0.04) / scale, -(pitch - sender.pitch).clamp(-0.04, 0.04) / scale);
        battle.input.key(KeyCode::KeyW, range > 9.);
        battle.input.mouse(MouseButton::Left, range < 40. && delta.abs() < 0.2);
    }

    /// Updates cumulative allocation and contention metrics from the active `BattleController`.
    pub fn accumulate_battle_metrics(&mut self, battle: &BattleController) {
        // Retained counters cover the shared online/Instant Action sound
        // adapter. Formatting is deferred until a diagnostic report is saved.
        if let Some(scene) = &battle.scene.online {
            for (total, count) in self.audio_events.iter_mut().zip(scene.sound.event_counts) {
                *total = (*total).max(count);
            }
            for (total, count) in self.presented.iter_mut().zip(scene.presented.shown) {
                *total = (*total).max(count);
            }
            self.voice_decodes = self.voice_decodes.max(scene.sound.voice_decoded);
            self.station_loops = self.station_loops.max(scene.sound.station_loops);
            self.audio_frames = self.audio_frames.max(scene.sound.device_frames());
            self.audio_peak = self.audio_peak.max(scene.sound.peak());
        }
        self.steady_frames_measured += 1;
        self.world_renderer_blocks = self
            .world_renderer_blocks
            .max(battle.world_renderer_blocks());
    }

    /// Keep the spawn and frame counts beside each battle's frame-time measurements.
    fn record_battle_counts(&mut self, battle: &BattleController) {
        if let Some(row) = self.battles.last_mut() {
            row["playerSpawns"] = battle.player_spawns.into();
            row["steadyRenderFrames"] = battle.steady_frames_measured.into();
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


    /// Steps the gate choreography while in the front-end shell.
    pub fn step_shell(
        &mut self,
        shell: &mut ShellHost,
        _settings: &mut AppSettingsStore,
    ) -> Result<Option<ShellTransition>, String> {
        self.phase_frames += 1;
        let elapsed = self.phase_started.elapsed().as_secs_f64();

        match self.phase {
            GatePhase::GateScreen => {
                // Capture `02-gate.png` on the click-to-continue screen, then unlock into Main Menu.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "02-gate.png")?;
                    shell.app.unlock_gate();
                    self.transition_to(GatePhase::Launch);
                }
            }
            GatePhase::Launch => {
                // Start an Instant Action battle on the target map.
                if self.phase_frames >= 4 {
                    let map = self.target_map.as_str();
                    let era = if map == "hot1" { "gcw" } else { "cw" };
                    let playlist = vec![PlaylistEntry::new(map, Some(era.to_string()))];
                    let req = BattleRequest::from_rotation(Some(map), Some(era), playlist, 0, false)?;
                    self.transition_to(GatePhase::Spawn);
                    return Ok(Some(ShellTransition::StartInstantAction(req)));
                }
            }
            GatePhase::Menu => {
                // Back in the menus after quitting: the run is over.
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.transition_to(GatePhase::Finished);
                    return Ok(Some(ShellTransition::Exit));
                }
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

        match self.phase {
            GatePhase::Spawn => {
                // Wait for the admitted world to publish its initial spawn state (`classes` and `posts`),
                // capture `09-ia-nab2-spawn.png`, and spawn into `nab2`.
                if self.phase_frames >= 12 && elapsed >= 0.35 {
                    self.save_battle_shot(battle, "09-ia-nab2-spawn.png")?;
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::Combat);
                }
            }
            GatePhase::Combat => {
                if self.repeat_target_round(battle.scoreboard_ctrl.ended, elapsed) {
                    self.finish_combat_metrics(battle.map_id(), self.phase_frames, elapsed);
                    self.record_battle_counts(battle);
                    self.target_combat_seconds += elapsed;
                    self.ia_battles_completed += 1;
                    self.record_world_tick_stats(battle);
                    battle.input.clear();
                    battle.pause_menu.custom_restart_triggered += 1;
                    self.transition_to(GatePhase::Spawn);
                    return Ok(());
                }
                // Drive real combat on `nab2`, capturing `10-ia-nab2-battle.png` once spawned.
                self.record_combat_frame_interval();
                if self.hunt.is_some() {
                    self.hunt_frame(battle, settings);
                } else {
                    self.apply_generated_combat_input(battle);
                }
                if self.phase_frames == 45 {
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    let shot_name = format!("ia-{}-battle.png", self.target_map);
                    self.save_battle_shot(battle, &shot_name)?;
                    self.reset_combat_measurement_window();
                    return Ok(());
                }
                if self.target_combat_seconds + elapsed >= self.battle_duration_s
                    && self.phase_frames >= 60
                {
                    self.target_combat_seconds += elapsed;
                    self.finish_combat_metrics(battle.map_id(), self.phase_frames, elapsed);
                    self.record_battle_counts(battle);
                    battle.input.clear();
                    if battle.spawn_ctrl.is_open() {
                        battle.spawn_ctrl.close(&mut battle.spawn_port);
                    }
                    battle.handle_key("Escape", true, false, settings);
                    battle.handle_key("Escape", false, false, settings);
                    self.transition_to(GatePhase::Pause);
                }
            }
            GatePhase::Pause => {
                // Capture `16-ia-bes1-pause.png`, then open the Quit-to-Main-Menu confirmation modal.
                if self.phase_frames >= 12 && elapsed >= 0.3 {
                    self.save_battle_shot(battle, "16-ia-bes1-pause.png")?;
                    battle.open_pause_confirm(ConfirmKind::Quit);
                    self.transition_to(GatePhase::PauseQuit);
                }
            }
            GatePhase::PauseQuit => {
                // Capture `17-ia-bes1-pause-quit.png`, record `hot1` world tick stats, and confirm quit.
                if self.phase_frames >= 10 && elapsed >= 0.25 {
                    self.save_battle_shot(battle, "17-ia-bes1-pause-quit.png")?;
                    self.record_world_tick_stats(battle);
                    self.ia_battles_completed += 1;
                    battle.handle_key("ArrowLeft", true, false, settings);
                    battle.handle_key("Enter", true, false, settings);
                    self.transition_to(GatePhase::Menu);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Writes `gate-summary.json`: GL errors, world/renderer blocks, frame times and the
    /// world thread's tick-time distribution and host counts.
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
        // completed run and actual world ticks before claiming a pass.
        let expected_battles = self.ia_battles_completed.max(1);
        let choreography_complete = self.phase == GatePhase::Finished
            && self.ia_battles_completed == expected_battles
            && self.target_combat_seconds >= self.battle_duration_s
            && self.battles.len() == expected_battles
            && self.battles.iter().all(|battle| {
                battle["playerSpawns"].as_u64().is_some_and(|count| count > 0)
            })
            && self.world_tick_stats.len() == 1
            && self
                .world_tick_stats
                .iter()
                .all(|(_, stats)| stats.count > 0 && !stats.context_poisoned);
        let passed = choreography_complete
            && self.gl_errors == 0
            && self.world_renderer_blocks == 0
            && self.steady_frames_measured > 0;
        let summary = serde_json::json!({
            "status": if passed { "PASS" } else { "FAIL" },
            "glErrors": self.gl_errors,
            "audio": {"eventCounts":self.audio_events.to_vec(),"stationLoops":self.station_loops,"voiceDecoded":self.voice_decodes,"frames":self.audio_frames,"peak":self.audio_peak},
            "presented": crate::online::replicas::presented::SHOWN.iter().zip(self.presented).map(|(name, count)| (name.to_string(), serde_json::Value::from(count))).collect::<serde_json::Map<_, _>>(),
            "steadyFramesMeasured": self.steady_frames_measured,
            "worldRendererBlocks": self.world_renderer_blocks,
            "worldThreadTickTimeMs": world_stats_map,
            "iaBattlesCompleted": self.ia_battles_completed,
            "battleDurationSeconds": self.battle_duration_s,
            "completedCombatSeconds": self.target_combat_seconds,
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
    fn interrupted_gate_writes_failure_receipt() {
        let dir = std::env::temp_dir().join(format!("bfe-gate-receipt-{}", std::process::id()));
        let gate = GateDriver::new("nab2", Some(dir.clone()));
        let summary = gate
            .write_summary_report()
            .expect("write interrupted receipt");
        assert_eq!(summary["status"], "FAIL");
        std::fs::remove_dir_all(dir).expect("remove receipt");
    }


    #[test]
    fn single_map_soak_requires_combat_across_completed_rounds() {
        let dir = std::env::temp_dir().join(format!("bfe-gate-soak-{}", std::process::id()));
        let mut gate = GateDriver::new("tat1", Some(dir.clone()));
        gate.battle_duration_s = 1800.;
        assert!(!gate.repeat_target_round(false, 120.));
        assert!(gate.repeat_target_round(true, 120.));
        gate.target_combat_seconds = 120.;
        assert!(gate.repeat_target_round(true, 1679.));
        assert!(!gate.repeat_target_round(true, 1680.));
        gate.phase = GatePhase::Finished;
        gate.ia_battles_completed = 2;
        gate.steady_frames_measured = 100;
        gate.battles = vec![serde_json::json!({"playerSpawns": 1}); 2];
        gate.world_tick_stats.push((
            "tat1".into(),
            WorldTickStats {
                count: 1,
                ..Default::default()
            },
        ));
        // A menu which continues painting after an early match must not pass.
        assert_eq!(gate.write_summary_report().unwrap()["status"], "FAIL");
        gate.target_combat_seconds += 1680.;
        let summary = gate.write_summary_report().unwrap();
        assert_eq!(summary["status"], "PASS");
        assert_eq!(summary["completedCombatSeconds"], 1800.);
        gate.battles.pop();
        assert_eq!(gate.write_summary_report().unwrap()["status"], "FAIL");
        std::fs::remove_dir_all(dir).expect("remove soak receipt");
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
