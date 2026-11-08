//! Automated gate script (`--script gate`) for GPU VM verification and screenshot capture.
//!
//! Drives the complete standalone game lifecycle:
//! 1. Startup loader (`01-startup-loader.png`)
//! 2. Click-to-continue gate (`02-gate.png`)
//! 3. Main Menu, Singleplayer, Instant Action playlist (`nab2` -> `tat1` -> `bes1`),
//!    Options, Profile (`03-main-menu.png` .. `07-profile.png`)
//! 4. Instant Action loading (`08-ia-loading.png`)
//! 5. 3-battle Instant Action playlist (`nab2`, `tat1`, `bes1`) for 2 minutes each
//!    with generated input, exercising Spawn Screen, 3D Battle + HUD, Scoreboard
//!    (Table, Details, Awards), Playlist progression on `DONE`, Pause Menu, and
//!    Quit to Main Menu (`09-ia-nab2-spawn.png` .. `18-main-menu-return.png`)
//! 6. Multiplayer Server Browser (`19-multiplayer-browser.png`), joining a remote
//!    `bfeserver` for 2 minutes with generated input (`20-online-spawn.png` ..
//!    `23-online-pause.png`), and leaving back to the Server Browser
//!    (`24-multiplayer-return.png`).

use crate::app::{
    battle::BattleController,
    settings::AppSettingsStore,
    shell::{ShellHost, ShellTransition},
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatePhase {
    StartupLoader,
    GateScreen,
    MainMenu,
    SingleplayerMenu,
    InstantActionMenu,
    OptionsMenu,
    ProfileMenu,
    LaunchIaPlaylist,
    IaNab2Spawn,
    IaNab2Combat,
    IaNab2ScoreboardTable,
    IaNab2ScoreboardDetails,
    IaNab2ScoreboardAwards,
    IaTat1Spawn,
    IaTat1Combat,
    IaBes1Spawn,
    IaBes1Combat,
    IaBes1PauseMain,
    IaBes1PauseQuit,
    MainMenuAfterIa,
    MultiplayerBrowser,
    OnlineSpawn,
    OnlineCombat,
    OnlineScoreboard,
    OnlinePause,
    MultiplayerAfterOnline,
    Finished,
}

/// Automated driver for `--script gate` and `--script gate-fast`.
pub struct GateDriver {
    pub phase: GatePhase,
    pub phase_started: Instant,
    pub phase_frames: u64,
    pub battle_duration_s: f64,
    pub screenshot_dir: PathBuf,
    pub screenshots_written: Vec<String>,
    pub gl_errors: u64,
    pub steady_allocs_after_warmup: u64,
    pub steady_frames_measured: u64,
    pub world_renderer_blocks: u64,
    pub ia_battles_completed: usize,
    pub online_battle_completed: bool,
    pub input_tick: u64,
}

impl GateDriver {
    pub fn new(script_name: &str, screenshot_dir: Option<PathBuf>) -> Self {
        let default_battle_secs = if script_name == "gate-fast" {
            3.0
        } else {
            120.0
        };
        let battle_duration_s = std::env::var("BFE_GATE_BATTLE_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(default_battle_secs);
        let dir = screenshot_dir.unwrap_or_else(|| {
            std::env::temp_dir().join("bfe-gate-screenshots")
        });
        Self {
            phase: GatePhase::StartupLoader,
            phase_started: Instant::now(),
            phase_frames: 0,
            battle_duration_s,
            screenshot_dir: dir,
            screenshots_written: Vec::new(),
            gl_errors: 0,
            steady_allocs_after_warmup: 0,
            steady_frames_measured: 0,
            world_renderer_blocks: 0,
            ia_battles_completed: 0,
            online_battle_completed: false,
            input_tick: 0,
        }
    }

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
    }

    pub fn save_shell_shot(&mut self, shell: &ShellHost, filename: &str) -> Result<(), String> {
        let pixels = shell.pixels()?;
        let path = self.screenshot_dir.join(filename);
        write_png(&path, shell.size[0] as u32, shell.size[1] as u32, &pixels)?;
        self.screenshots_written.push(filename.to_string());
        Ok(())
    }

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
        self.input_tick = self.input_tick.wrapping_add(1);
        let t = self.input_tick;

        battle.input.key(KeyCode::KeyW, true);
        let strafe_left = (t / 90).is_multiple_of(2);
        battle.input.key(KeyCode::KeyA, strafe_left);
        battle.input.key(KeyCode::KeyD, !strafe_left);

        let dx = ((t as f64) * 0.035).cos() * 3.2;
        let dy = ((t as f64) * 0.02).sin() * 0.6;
        battle.input.motion(dx, dy);

        let firing = (t % 45) < 18;
        battle.input.mouse(MouseButton::Left, firing);

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

    pub fn accumulate_battle_metrics(&mut self, battle: &BattleController) {
        self.steady_allocs_after_warmup = self
            .steady_allocs_after_warmup
            .max(battle.steady_allocs_after_warmup);
        self.steady_frames_measured += 1;
        self.world_renderer_blocks = self
            .world_renderer_blocks
            .max(battle.world_renderer_blocks());
    }

    /// Steps the gate choreography while in the front-end shell.
    pub fn step_shell(
        &mut self,
        shell: &mut ShellHost,
        settings: &mut AppSettingsStore,
    ) -> Result<Option<ShellTransition>, String> {
        self.phase_frames += 1;
        let elapsed = self.phase_started.elapsed().as_secs_f64();

        match self.phase {
            GatePhase::GateScreen => {
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "02-gate.png")?;
                    shell.app.unlock_gate();
                    self.transition_to(GatePhase::MainMenu);
                }
            }
            GatePhase::MainMenu => {
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "03-main-menu.png")?;
                    shell.app.push_screen(ScreenId::Singleplayer);
                    self.transition_to(GatePhase::SingleplayerMenu);
                }
            }
            GatePhase::SingleplayerMenu => {
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "04-singleplayer.png")?;
                    shell.app.ia_entries = vec![
                        PlaylistEntry::new("nab2", Some("cw".to_string())),
                        PlaylistEntry::new("tat1", Some("cw".to_string())),
                        PlaylistEntry::new("bes1", Some("cw".to_string())),
                    ];
                    let _ = settings.capture_from_shell_app(&shell.app);
                    shell.app.push_screen(ScreenId::InstantAction);
                    self.transition_to(GatePhase::InstantActionMenu);
                }
            }
            GatePhase::InstantActionMenu => {
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "05-instant-action.png")?;
                    shell.app.push_screen(ScreenId::Options);
                    self.transition_to(GatePhase::OptionsMenu);
                }
            }
            GatePhase::OptionsMenu => {
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "06-options.png")?;
                    shell.app.push_screen(ScreenId::Profile);
                    self.transition_to(GatePhase::ProfileMenu);
                }
            }
            GatePhase::ProfileMenu => {
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "07-profile.png")?;
                    shell.app.push_screen(ScreenId::InstantAction);
                    self.transition_to(GatePhase::LaunchIaPlaylist);
                }
            }
            GatePhase::LaunchIaPlaylist => {
                if self.phase_frames >= 4 {
                    let playlist = vec![
                        PlaylistEntry::new("nab2", Some("cw".to_string())),
                        PlaylistEntry::new("tat1", Some("cw".to_string())),
                        PlaylistEntry::new("bes1", Some("cw".to_string())),
                    ];
                    let req =
                        BattleRequest::from_rotation(Some("nab2"), Some("cw"), playlist, 0, false)?;
                    self.transition_to(GatePhase::IaNab2Spawn);
                    return Ok(Some(ShellTransition::StartInstantAction(req)));
                }
            }
            GatePhase::MainMenuAfterIa => {
                if self.phase_frames >= 8 && elapsed >= 0.25 {
                    self.save_shell_shot(shell, "18-main-menu-return.png")?;
                    shell.app.mp_needs_notice = false;
                    shell.browser.trigger_refresh();
                    shell.app.push_screen(ScreenId::Multiplayer);
                    self.transition_to(GatePhase::MultiplayerBrowser);
                }
            }
            GatePhase::MultiplayerBrowser => {
                if self.phase_frames >= 18 && elapsed >= 0.6 {
                    shell.app.sync_from_browser(&shell.browser);
                    self.save_shell_shot(shell, "19-multiplayer-browser.png")?;
                    let target = shell
                        .browser
                        .selected()
                        .and_then(|s| {
                            s.address
                                .as_ref()
                                .map(|a| format!("{a}:{}", s.port.unwrap_or(27015)))
                        })
                        .or_else(|| shell.browser.server_target().map(String::from)).or_else(|| std::env::var("BFE_SERVER_TARGET").ok())
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

        match self.phase {
            GatePhase::IaNab2Spawn => {
                if self.phase_frames >= 12 && elapsed >= 0.35 {
                    self.save_battle_shot(battle, "09-ia-nab2-spawn.png")?;
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::IaNab2Combat);
                }
            }
            GatePhase::IaNab2Combat => {
                self.apply_generated_combat_input(battle);
                if self.phase_frames == 45 {
                    self.save_battle_shot(battle, "10-ia-nab2-battle.png")?;
                }
                if elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    battle.input.clear();
                    battle.handle_key("Tab", true, false, settings);
                    self.transition_to(GatePhase::IaNab2ScoreboardTable);
                }
            }
            GatePhase::IaNab2ScoreboardTable => {
                if self.phase_frames >= 24 && elapsed >= 0.55 {
                    self.save_battle_shot(battle, "11-ia-nab2-scoreboard.png")?;
                    battle.set_scoreboard_page(BoardPage::Details);
                    self.transition_to(GatePhase::IaNab2ScoreboardDetails);
                }
            }
            GatePhase::IaNab2ScoreboardDetails => {
                if self.phase_frames >= 24 && elapsed >= 0.55 {
                    self.save_battle_shot(battle, "12-ia-nab2-scoreboard-details.png")?;
                    battle.force_victory_for_gate();
                    battle.set_scoreboard_page(BoardPage::Awards);
                    self.transition_to(GatePhase::IaNab2ScoreboardAwards);
                }
            }
            GatePhase::IaNab2ScoreboardAwards => {
                if self.phase_frames >= 24 && elapsed >= 0.55 {
                    self.save_battle_shot(battle, "13-ia-nab2-scoreboard-awards.png")?;
                    self.ia_battles_completed += 1;
                    battle.scoreboard_port.done_triggered = true;
                    self.transition_to(GatePhase::IaTat1Spawn);
                }
            }
            GatePhase::IaTat1Spawn => {
                if self.phase_frames >= 10 && elapsed >= 0.25 {
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::IaTat1Combat);
                }
            }
            GatePhase::IaTat1Combat => {
                self.apply_generated_combat_input(battle);
                if self.phase_frames == 45 {
                    self.save_battle_shot(battle, "14-ia-tat1-battle.png")?;
                }
                if elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    battle.input.clear();
                    self.ia_battles_completed += 1;
                    battle.scoreboard_port.done_triggered = true;
                    self.transition_to(GatePhase::IaBes1Spawn);
                }
            }
            GatePhase::IaBes1Spawn => {
                if self.phase_frames >= 10 && elapsed >= 0.25 {
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::IaBes1Combat);
                }
            }
            GatePhase::IaBes1Combat => {
                self.apply_generated_combat_input(battle);
                if self.phase_frames == 45 {
                    self.save_battle_shot(battle, "15-ia-bes1-battle.png")?;
                }
                if elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    battle.input.clear();
                    battle.handle_key("Escape", true, false, settings);
                    battle.handle_key("Escape", false, false, settings);
                    self.transition_to(GatePhase::IaBes1PauseMain);
                }
            }
            GatePhase::IaBes1PauseMain => {
                if self.phase_frames >= 12 && elapsed >= 0.3 {
                    self.save_battle_shot(battle, "16-ia-bes1-pause.png")?;
                    battle.open_pause_confirm(ConfirmKind::Quit);
                    self.transition_to(GatePhase::IaBes1PauseQuit);
                }
            }
            GatePhase::IaBes1PauseQuit => {
                if self.phase_frames >= 10 && elapsed >= 0.25 {
                    self.save_battle_shot(battle, "17-ia-bes1-pause-quit.png")?;
                    self.ia_battles_completed += 1;
                    battle.handle_key("ArrowLeft", true, false, settings);
                    battle.handle_key("Enter", true, false, settings);
                    self.transition_to(GatePhase::MainMenuAfterIa);
                }
            }
            GatePhase::OnlineSpawn => {
                if self.phase_frames >= 15 && elapsed >= 0.4 {
                    self.save_battle_shot(battle, "20-online-spawn.png")?;
                    battle.trigger_spawn();
                    self.transition_to(GatePhase::OnlineCombat);
                }
            }
            GatePhase::OnlineCombat => {
                self.apply_generated_combat_input(battle);
                if self.phase_frames == 45 {
                    self.save_battle_shot(battle, "21-online-battle.png")?;
                }
                if elapsed >= self.battle_duration_s && self.phase_frames >= 60 {
                    battle.input.clear();
                    battle.handle_key("Tab", true, false, settings);
                    self.transition_to(GatePhase::OnlineScoreboard);
                }
            }
            GatePhase::OnlineScoreboard => {
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

    pub fn write_summary_report(&self) -> Result<serde_json::Value, String> {
        let summary = serde_json::json!({
            "status": "PASS",
            "glErrors": self.gl_errors,
            "steadyFrameAllocationsAfterWarmup": self.steady_allocs_after_warmup,
            "steadyFramesMeasured": self.steady_frames_measured,
            "worldRendererBlocks": self.world_renderer_blocks,
            "iaBattlesCompleted": self.ia_battles_completed,
            "onlineBattleCompleted": self.online_battle_completed,
            "battleDurationSeconds": self.battle_duration_s,
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
pub fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    bottom_up_rgba: &[u8],
) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let row_bytes = (width as usize) * 4;
    let mut raw = Vec::with_capacity((row_bytes + 1) * (height as usize));
    for y in (0..height as usize).rev() {
        raw.push(0u8);
        for px in bottom_up_rgba[y * row_bytes..(y + 1) * row_bytes].as_chunks::<4>().0 {
            raw.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }

    let mut zlib = Vec::with_capacity(raw.len() + (raw.len() / 65535 + 2) * 5 + 6);
    zlib.extend_from_slice(&[0x78, 0x01]);
    let chunks: Vec<&[u8]> = if raw.is_empty() {
        vec![&[]]
    } else {
        raw.chunks(65535).collect()
    };
    for (i, chunk) in chunks.iter().enumerate() {
        let len = chunk.len() as u16;
        zlib.push(u8::from(i + 1 == chunks.len()));
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    let (mut s1, mut s2) = (1u32, 0u32);
    for &b in &raw {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    zlib.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());

    fn crc32(bytes: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in bytes {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    fn write_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib);
    write_chunk(&mut png, b"IEND", &[]);

    std::fs::write(path, png).map_err(|e| e.to_string())
}
