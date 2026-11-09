//! Unified battle controller for Instant Action (`WorldThread`) and Online (`online::client::Client`)
//! battles, integrating `bfe-battle-ui` (`SpawnScreenController`, `ScoreboardController`,
//! `PauseMenu`, `ChatController`) over the native 3D `Scene` and `HudModel`.
//!
//! ## Architecture and Invariants
//! - **Single Unified 3D + HUD Framebuffer**: Both Instant Action and Online battles
//!   render 3D world geometry, sky, terrain, units, particles, and 2D/3D-anchored HUD
//!   and battle overlays (`SpawnScreenController`, `ScoreboardController`, `PauseMenu`,
//!   `ChatController`) into `Scene.canvas.framebuffer` via a single `scene.draw(&view)`
//!   pass per frame.
//! - **Exclusive `Context::world_admit` Admission**: Instant Action admits the mission
//!   simulation exclusively through `WorldThread::admit_instant_action` (`Prepared::new` ->
//!   `Context::world_admit`) on a dedicated `"bfe-world-60hz"` thread; no stand-in or
//!   fallback authority exists. If admission fails, the error is surfaced to the player.
//! - **Non-blocking Render/World Coupling**: The render thread drains `Publication`
//!   batches via `try_recv` and sends `WorldMessage::Step` via `mpsc::Sender::send`,
//!   guaranteeing `0` render-thread blocks on the 60 Hz world thread.
//!
//! ## Coordinate Spaces and HUD / Overlay Composition
//! - **3D World Pass (`Scene::draw`)**:
//!   Renders skybox, terrain heightmap/blendmap, static props, water reflections,
//!   skeletal/skinned soldiers and vehicles, ordnances, explosions, particles, and
//!   post-processing into `Scene.canvas.framebuffer` using the interpolated [`View`]
//!   camera matrix.
//! - **In-Battle HUD (`HudModel`)**:
//!   Maintains a pre-allocated 2048-quad batch (`scene.bridge.hud_model.batch`) in a
//!   1024-wide logical interface space (`height = interface_height(w, h)`).
//!   During steady combat (all 2D overlays closed), `hud_model_update()` and
//!   `hud_model.build(0)` update only dirty quad vertex ranges (`start..end`),
//!   and `bridge.hud_sync_batch(false)` copies only the modified slice into
//!   `scene.core`'s HUD pass with zero heap allocations.
//! - **Battle UI Overlays (`bfe-battle-ui`)**:
//!   When `SpawnScreenController`, `ScoreboardController`, `PauseMenu`, or
//!   `ChatController` is open:
//!   1. [`BattleController::sync_ports_cold`] copies the latest reinforcement ticket
//!      counts, command post owners, player life state, and scoreboard roster rows
//!      from `replicas.hud` and `Receiver` into [`LiveSpawnPort`] and [`LiveScoreboardPort`].
//!   2. [`BattleController::update_and_build_overlays`] updates the active controller
//!      and emits its quads into `hud_model.batch` (either directly via `HudBatchSurface`
//!      or by compositing `self.overlay_batch` via [`BattleController::append_batch`]).
//!   3. `bridge.hud_sync_batch(true)` uploads the full active quad range for that frame.
//!
//! ## Instant Action vs Online Authority Parity
//! | Aspect | `BattleAuthority::InstantAction` | `BattleAuthority::Online` |
//! |---|---|---|
//! | World Admission | `WorldThread::admit_instant_action` (`Context::world_admit`) | Remote/local `bfeserver` (`GET /info` + UDP/WebRTC) |
//! | Simulation Thread | Dedicated 60 Hz `"bfe-world-60hz"` thread | Server-authoritative 60 Hz loop |
//! | Snapshot Delivery | Lock-free `mpsc::Receiver<Publication>` (`poll_publications`) | Network datagrams (`OnlineClient::receive`) |
//! | Input Delivery | `WorldMessage::Step { command, look, flight, actions, ack }` | `OnlineClient::step` + `OnlineClient::send` |
//! | Client Prediction | `Sender::tick` + `Replay::frame` + `MapCamera::follow` | `OnlineClient::step` + `OnlineClient::present` |
//! | Spawn / Respawn | `WorldMessage::Spawn` / `WorldMessage::Respawn` | `Sender::request_spawn` / `Sender::request_suicide` |
//! | Pause Behavior | Pauses `"bfe-world-60hz"` (`world.set_paused(true)`) | Simulation continues on server; shows `PLAYER LIST` |
//! | Restart Mission | Re-admits current playlist entry from tick 0 | Hidden in online mode (`PauseRestartMode::Hidden`) |
//!
//! ## Steady-State Allocation Accounting
//! - Frames `0..45` with overlays closed warm up any lazy internal buffers and the
//!   first-tick welcome snapshot / initial combat presentation caches, matching
//!   `phase_frames == 45` in `script.rs` (`reset_combat_measurement_window`).
//! - Once `self.steady_frames >= 45` and `!self.is_overlay_open()`,
//!   `BattleController::step_and_draw` samples `super::render_thread_alloc_count()`
//!   before and after the entire simulation poll, input step, HUD build, and
//!   `scene.draw(&view)` pass, accumulating any delta into `steady_allocs_after_warmup`.
//! - The `--script gate` harness reports `steady_allocs_after_warmup` across
//!   all three 2-minute Instant Action battles (`nab2`, `tat1`, `bes1`) and the
//!   2-minute Online battle.
//!
//! ## Scoreboard Pages and Post-Match Flow (`LiveScoreboardPort` + `ScoreboardController`)
//! - **`BoardPage::Table`**:
//!   Displays the two faction headers (`REPUBLIC` vs `CIS` or `REBEL ALLIANCE` vs
//!   `GALACTIC EMPIRE`), live reinforcement ticket counts, and sorted per-unit rows
//!   with `Name`, `Kills`, `Deaths`, `Team Kills`, `Points`, and online `Ping`.
//! - **`BoardPage::Details`**:
//!   Displays personal match/career statistics (`PersonalStatRow`) for the highlighted
//!   player or AI unit slot.
//! - **`BoardPage::Awards`**:
//!   Available when the match ends (`BoardState::End`); displays post-battle medals
//!   and awards (`AwardRow`) together with the `DONE` action button.
//! - **Playlist Progression on `DONE`**:
//!   Activating `DONE` invokes [`ScoreboardPort::restart`] on [`LiveScoreboardPort`],
//!   which sets `done_triggered = true`. On the next [`BattleController::step_and_draw`]
//!   tick, `done_triggered` emits [`BattleTransition::NextPlaylistBattle`] in Instant
//!   Action (advancing `playlist_index -> playlist_index + 1` in `request.playlist`)
//!   or [`BattleTransition::LeaveOnline`] in Online mode.
//!
//! ## Spawn Screen Tactical Map and Class Selection (`LiveSpawnPort` + `SpawnScreenController`)
//! - **Class Roster (`rosters[0..2]`)**:
//!   Populated from `replicas.hud.class_meta` with up to 5 playable classes per faction,
//!   preserving each class's network registry ID (`class_registry_ids`) and localized
//!   weapon list (`BLASTER RIFLE`, `BLASTER PISTOL`, `THERMAL DETONATOR`, etc.).
//! - **Command Posts (`posts` & `post_ids`)**:
//!   Populated from `replicas.hud.post_meta` with world-space `[x, y, z]` coordinates
//!   projected onto the overhead map via `MapProjection` (`map_x`, `map_z`, `map_half`,
//!   `post_icon_size`). Posts owned by the player's team (or neutral) are marked
//!   `eligible = true`.
//! - **Spawn Dispatch (`PendingSpawnChoice`)**:
//!   Confirming spawn (`Enter`, `Space`, or clicking `SPAWN`) records a
//!   [`PendingSpawnChoice`] on [`LiveSpawnPort`], which [`BattleController::flush_spawn_requests`]
//!   sends as `WorldMessage::Spawn { team, class_name, post_name }` in Instant Action
//!   or `Sender::request_spawn(class_reg_id, post_reg_id)` in Online mode.
//!
//! ## Escape Pause Menu (`PauseMenu`)
//! - **Instant Action Mode**: Exposes `RESUME`, `RESPAWN`, `RESTART MISSION`
//!   (`PauseRestartMode::Custom`), `OPTIONS`, `CONTROLS`, `QUIT TO MAIN MENU`,
//!   and `EXIT TO WINDOWS`. While open, pauses the 60 Hz world thread (`world.set_paused(true)`).
//! - **Online Mode**: Hides `RESTART MISSION` (`PauseRestartMode::Hidden`), enables
//!   `PLAYER LIST` (`show_player_list = true`), and keeps the network session
//!   running while the overlay is open.

use crate::{
    app::settings::AppSettingsStore,
    audio::SharedAudioRouter,
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    host::{World, STEP},
    input::{Input, ViewActions},
    local::{
        options::Options as LocalOptions,
        publication::Publication,
        thread::{WorldMessage, WorldThread, WorldTickStats},
    },
    online::{
        client::Client as OnlineClient,
        hud::{Hud, HudClassMeta, HudOwnSummary, HudPostMeta},
        input::Sender,
        receiver::Receiver,
        replay::Replay,
    },
    scene::{MapCamera, Scene, View},
};
use bfe_battle_ui::{
    hud_input::{route_battle_key, BattleKeyAction, ChatController},
    pause::{ConfirmKind, PauseMenu, PauseRespawnConfig, PauseRestartMode},
    scoreboard::{
        AwardRow, BoardPage, BoardState, CharacterRecord, CharacterStatsEntry, MatchStateFlags,
        PersonalStatRow, PlayerListRow, ScoreRow, ScoreboardAction, ScoreboardController,
        ScoreboardPort, TeamHeader, WorldScoreboardPort,
    },
    shapes::{
        interface_height, root_anchor, to_interface, BattleUiAssets, GameFamily, HudBatchSurface,
    },
    spawn::{
        CameraShot, HumanSpawnFacts, MapProjection, OpenReason, SpawnClassEntry, SpawnInputAction,
        SpawnPort, SpawnPostEntry, SpawnRequest, SpawnScreenController, SpawnTeamFacts,
    },
};
use bfe_presentation::{aim::View as AimView, cameras::math::V8};
use bfe_render::{
    hud as render_hud,
    hud_model::layout::{HudArt as A, HudFrame as H},
    interface::{alpha, translate, Batch, Color, Draw, UV},
};
use bfe_shell::{launch::BattleRequest, screens::MultiplayerLaunch};
use std::path::Path;
use winit::{event::MouseButton, keyboard::KeyCode};

/// Transition out of the active battle.
#[derive(Clone, Debug)]
///
/// Returned by [`BattleController::step_and_draw`] when the active battle concludes
/// (via post-match scoreboard `DONE`), restarts (`PauseMenu` `RESTART MISSION`),
/// returns to the front-end (`QUIT TO MAIN MENU` / `LEAVE GAME`), or exits to desktop.
pub enum BattleTransition {
    /// Advance to the next mission in the Instant Action playlist (or loop/return).
    NextPlaylistBattle {
        /// Launch parameters and playlist for the Instant Action session.
        request: BattleRequest,
        /// Zero-based index of the next playlist entry to admit.
        next_index: usize,
    },
    /// Restart the current Instant Action mission from tick 0.
    RestartMission {
        /// Launch parameters and playlist for the Instant Action session.
        request: BattleRequest,
        /// Zero-based index of the mission to re-admit.
        playlist_index: usize,
    },
    /// Quit from an Instant Action battle back to the front-end Main Menu.
    QuitToMainMenu,
    /// Leave an Online battle (or disconnect) back to the Multiplayer Server Browser.
    LeaveOnline {
        /// Optional disconnect or network fault message to display in a popup.
        reason: Option<String>,
    },
    /// Rejoin the ready server match after the end table.
    NextOnlineBattle { launch: MultiplayerLaunch },
    /// Exit the entire application to the desktop (`EXIT TO WINDOWS`).
    ExitToWindows,
}

/// Pending spawn selection queued from `LiveSpawnPort::request`.
#[derive(Clone, Debug)]
///
/// Created by [`LiveSpawnPort::request`] when the player clicks `SPAWN` or presses
/// `Enter`/`Space` on the spawn screen, and consumed on the next frame by
/// [`BattleController::flush_spawn_requests`].
pub struct PendingSpawnChoice {
    /// Selected faction team index (`1` or `2`).
    pub team: u8,
    /// Zero-based class index within the team's spawn roster.
    pub class_idx: usize,
    /// Zero-based command post index within the mission's post list.
    pub post_idx: usize,
    /// Network registry ID of the selected unit class (`OnlineClient` spawn request).
    pub class_reg_id: u16,
    /// Network registry ID of the selected command post (`OnlineClient` spawn request).
    pub post_reg_id: u16,
    /// Engine class name (e.g. `"rep_inf_trooper"`, `"cis_inf_battledroid"`).
    pub class_name: String,
    /// Engine command post identifier (e.g. `"cp1"`, `"cp2"`).
    pub post_name: String,
}

/// Spawn screen port backed by the live battle HUD metadata and authority.
///
/// Implements [`bfe_battle_ui::spawn::SpawnPort`] by bridging the mission's replicated
/// [`HudClassMeta`], [`HudPostMeta`], team reinforcement ticket counters, and local
/// player life phase into `SpawnScreenController`.
pub struct LiveSpawnPort {
    /// Current human player life phase, selected team, class, and command post.
    pub human: HumanSpawnFacts,
    /// Team headers, faction icons, objectives, and reinforcement ticket counts for teams 1 and 2.
    pub teams: [SpawnTeamFacts; 2],
    /// Selectable unit class rosters for teams 1 and 2.
    pub rosters: [Vec<SpawnClassEntry>; 2],
    /// Network registry IDs corresponding to each entry in `rosters`.
    pub class_registry_ids: [Vec<usize>; 2],
    /// Command posts displayed on the overhead tactical map.
    pub posts: Vec<SpawnPostEntry>,
    /// Network registry IDs corresponding to each entry in `posts`.
    pub post_ids: Vec<usize>,
    /// Spawn selection queued when the player clicks `SPAWN` or presses `Enter`/`Space`.
    pub pending_spawn: Option<PendingSpawnChoice>,
    /// Suicide / respawn flag queued when the player triggers respawn.
    pub pending_suicide: bool,
}

impl LiveSpawnPort {
    /// Constructs a [`LiveSpawnPort`] from the mission's extracted HUD class metadata,
    /// command post metadata, localization strings, and era (`"cw"` or `"gcw"`).
    ///
    /// ## Roster and Command Post Extraction
    /// 1. Selects faction side identifiers (`"rep"` vs `"cis"` for Clone Wars;
    ///    `"all"` vs `"imp"` for Galactic Civil War), display titles, and faction icon names.
    /// 2. Extracts up to 5 playable unit classes per faction from `class_meta`, preserving
    ///    their network registry IDs (`class_registry_ids`) and weapon loadout labels.
    /// 3. Populates the overhead map command post list (`posts` and `post_ids`) with
    ///    world-space `[x, y, z]` coordinates and initial team ownership.
    pub fn from_hud_meta(
        class_meta: &[HudClassMeta],
        post_meta: &[HudPostMeta],
        strings: &std::collections::HashMap<String, String>,
        era: &str,
    ) -> Self {
        // Resolve faction side keys, display titles, and emblem texture names for the era.
        let is_gcw = era.eq_ignore_ascii_case("gcw");
        // The mission, rather than the era, assigns faction to team number.
        // Keep the era defaults only for content-free interface fixtures.
        let side1 = class_meta
            .iter()
            .find(|c| c.team == 1)
            .map(|c| c.side.as_str())
            .unwrap_or(if is_gcw { "all" } else { "rep" });
        let side2 = class_meta
            .iter()
            .find(|c| c.team == 2)
            .map(|c| c.side.as_str())
            .unwrap_or(if is_gcw { "imp" } else { "cis" });
        let faction = |side: &str| match side {
            "rep" => ("REPUBLIC", "com_icon_republic"),
            "cis" => ("CIS", "com_icon_cis"),
            "all" => ("REBEL ALLIANCE", "com_icon_alliance"),
            "imp" => ("GALACTIC EMPIRE", "com_icon_imperial"),
            _ => ("", ""),
        };
        let (name1, logo1) = faction(side1);
        let (name2, logo2) = faction(side2);

        let objective_text = strings
            .get("ifs.spawndisplay.spawnprompt")
            .map(|s| s.to_ascii_uppercase())
            .unwrap_or_else(|| {
                "ELIMINATE ENEMY REINFORCEMENTS OR CAPTURE ALL COMMAND POSTS".to_string()
            });

        let build_roster =
            |target_team: u8, target_side: &str| -> (Vec<SpawnClassEntry>, Vec<usize>) {
                let mut entries = Vec::new();
                let mut reg_ids = Vec::new();
                let mut classes: Vec<_> = class_meta
                    .iter()
                    .filter(|c| c.team == target_team)
                    .collect();
                classes.sort_by_key(|c| c.roster_order);
                for c in classes {
                    if entries.len() < 5 {
                        let weapons = if c.weapons.is_empty() {
                            vec!["BLASTER RIFLE".to_string(), "BLASTER PISTOL".to_string()]
                        } else {
                            c.weapons.clone()
                        };
                        entries.push(SpawnClassEntry {
                            class_name: c.class_name.clone(),
                            display_name: c.display_name.to_ascii_uppercase(),
                            weapons,
                            eligible: true,
                        });
                        reg_ids.push(c.registry_id);
                    }
                }
                if entries.is_empty() {
                    for (i, c) in class_meta.iter().take(5).enumerate() {
                        entries.push(SpawnClassEntry {
                            class_name: c.class_name.clone(),
                            display_name: c.display_name.to_ascii_uppercase(),
                            weapons: if c.weapons.is_empty() {
                                vec!["BLASTER RIFLE".to_string()]
                            } else {
                                c.weapons.clone()
                            },
                            eligible: true,
                        });
                        reg_ids.push(c.registry_id.max(i + 1));
                    }
                }
                if entries.is_empty() {
                    entries.push(SpawnClassEntry {
                        class_name: format!("{target_side}_inf_trooper"),
                        display_name: format!("{name1} TROOPER"),
                        weapons: vec![
                            "BLASTER RIFLE".to_string(),
                            "BLASTER PISTOL".to_string(),
                            "THERMAL DETONATOR (4)".to_string(),
                        ],
                        eligible: true,
                    });
                    reg_ids.push(1);
                }
                (entries, reg_ids)
            };

        let (roster1, ids1) = build_roster(1, side1);
        let (roster2, ids2) = build_roster(2, side2);

        // Populate command posts and their world-space `[x, y, z]` coordinates.
        let mut posts = Vec::new();
        let mut post_ids = Vec::new();
        for (idx, pm) in post_meta.iter().enumerate() {
            let owner = if pm.team == 1 || pm.team == 2 {
                pm.team
            } else if idx % 2 == 0 {
                1
            } else {
                2
            };
            posts.push(SpawnPostEntry {
                name: pm.name.clone(),
                display_name: pm.display_name.to_ascii_uppercase(),
                position: pm.position,
                owner,
                eligible: owner == 1,
                vehicles: Vec::new(),
            });
            post_ids.push(pm.id);
        }
        if posts.is_empty() {
            posts.push(SpawnPostEntry {
                name: "cp1".to_string(),
                display_name: "COMMAND POST 1".to_string(),
                position: [-35.0, 0.0, -35.0],
                owner: 1,
                eligible: true,
                vehicles: Vec::new(),
            });
            post_ids.push(1);
            posts.push(SpawnPostEntry {
                name: "cp2".to_string(),
                display_name: "COMMAND POST 2".to_string(),
                position: [35.0, 0.0, 35.0],
                owner: 2,
                eligible: false,
                vehicles: Vec::new(),
            });
            post_ids.push(2);
        }

        Self {
            human: HumanSpawnFacts {
                phase: "unspawned".to_string(),
                team: 1,
                class_name: roster1.first().map(|c| c.class_name.clone()),
                class_index: 0,
                post: posts.first().map(|p| p.name.clone()),
                can_switch_teams: true,
                spectator: false,
                over: false,
            },
            teams: [
                SpawnTeamFacts {
                    side: side1.to_string(),
                    name: name1.to_string(),
                    logo: logo1.to_string(),
                    objectives: vec![objective_text.clone()],
                    reinforcements: 150,
                },
                SpawnTeamFacts {
                    side: side2.to_string(),
                    name: name2.to_string(),
                    logo: logo2.to_string(),
                    objectives: vec![objective_text],
                    reinforcements: 150,
                },
            ],
            rosters: [roster1, roster2],
            class_registry_ids: [ids1, ids2],
            posts,
            post_ids,
            pending_spawn: None,
            pending_suicide: false,
        }
    }

    /// Synchronizes reinforcement counts, command post ownership, and the local
    /// player's life phase from the latest HUD publication state.
    ///
    /// ## Live HUD Synchronization
    /// - Updates `self.human.phase` (`"alive"`, `"dead"`, `"ended"`, or `"awaiting"`)
    ///   from `own_alive` and `own_summary`.
    /// - Updates `self.teams[0..2].reinforcements` from `teams[1][0]` and `teams[2][0]`.
    /// - Updates command post ownership (`p.owner`) as posts are captured or neutralized.
    pub fn sync_from_hud(
        &mut self,
        teams: &[[f64; 4]; 8],
        post_meta: &[HudPostMeta],
        own_summary: &HudOwnSummary,
        own_alive: bool,
        own_team: u8,
    ) {
        // The spawn port follows the server-selected side across death; the
        // default team must not turn an opposing human into a teammate.
        if (1..=2).contains(&own_team) {
            self.human.team = own_team;
        }
        self.human.phase = if own_alive {
            "alive".to_string()
        } else if own_summary.valid {
            match own_summary.phase {
                0 => "alive".to_string(),
                1 => "dead".to_string(),
                3 => "ended".to_string(),
                _ => "awaiting".to_string(),
            }
        } else {
            "awaiting".to_string()
        };
        if teams[1][0] > 0.0 || teams[2][0] > 0.0 {
            self.teams[0].reinforcements = teams[1][0].round() as i32;
            self.teams[1].reinforcements = teams[2][0].round() as i32;
        }
        for (i, pm) in post_meta.iter().enumerate() {
            if let Some(p) = self.posts.get_mut(i) {
                p.owner = pm.team;
            }
        }
    }
}

// Implementation of `bfe_battle_ui::spawn::SpawnPort` for `LiveSpawnPort`.
// Bridges the live battle authority (`WorldThread` or `OnlineClient`) to
// `SpawnScreenController` without coupling `bfe-battle-ui` to network or simulation types.
impl SpawnPort for LiveSpawnPort {
    /// Returns the human player's current spawn state.
    fn human_state(&self) -> HumanSpawnFacts {
        self.human.clone()
    }

    /// Returns faction metadata and reinforcement ticket count for `team` (`1` or `2`).
    fn team_facts(&self, team: u8) -> Option<SpawnTeamFacts> {
        match team {
            1 => Some(self.teams[0].clone()),
            2 => Some(self.teams[1].clone()),
            _ => None,
        }
    }

    /// Returns the selectable unit classes for `team` (`1` or `2`).
    fn roster(&self, team: u8) -> Vec<SpawnClassEntry> {
        match team {
            1 => self.rosters[0].clone(),
            2 => self.rosters[1].clone(),
            _ => Vec::new(),
        }
    }

    /// Returns all command posts, marking posts owned by `team` (or neutral) as eligible.
    fn posts(&self, team: u8) -> Vec<SpawnPostEntry> {
        self.posts
            .iter()
            .map(|p| {
                let mut e = p.clone();
                e.eligible = p.owner == team;
                e
            })
            .collect()
    }

    /// Queues a [`PendingSpawnChoice`] for the selected team, class, and command post.
    ///
    /// Resolves `choice.team` (`1` or `2`), looks up the zero-based `class_idx` and
    /// network `class_reg_id` in `self.rosters`, looks up `post_idx` and `post_reg_id`
    /// in `self.posts`, and stores the resulting [`PendingSpawnChoice`] in `self.pending_spawn`.
    fn request(&mut self, choice: &SpawnRequest) {
        let team = choice.team.clamp(1, 2);
        let t_idx = (team - 1) as usize;
        let class_idx = self.rosters[t_idx]
            .iter()
            .position(|c| c.class_name == choice.class_name)
            .unwrap_or(0);
        let class_reg_id = self.class_registry_ids[t_idx]
            .get(class_idx)
            .copied()
            .unwrap_or(1) as u16;
        let post_idx = self
            .posts
            .iter()
            .position(|p| p.name == choice.post)
            .unwrap_or(0);
        let post_reg_id = self.post_ids.get(post_idx).copied().unwrap_or(1) as u16;

        self.human.team = team;
        self.human.class_name = Some(choice.class_name.clone());
        self.human.class_index = class_idx;
        self.human.post = Some(choice.post.clone());
        self.pending_spawn = Some(PendingSpawnChoice {
            team,
            class_idx,
            post_idx,
            class_reg_id,
            post_reg_id,
            class_name: choice.class_name.clone(),
            post_name: choice.post.clone(),
        });
    }

    /// Queues a suicide / respawn request.
    fn suicide(&mut self) {
        self.pending_suicide = true;
    }

    /// Cancels spawn selection without action.
    fn cancel(&mut self) {}

    /// Returns custom camera shots if any (handled natively by `MapCamera`).
    fn camera_shots(&self) -> Vec<CameraShot> {
        Vec::new()
    }

    /// Sets the active spawn screen camera preview shot.
    fn set_view_shot(&mut self, _shot: Option<CameraShot>) {}
}

/// Scoreboard port wrapping [`WorldScoreboardPort`] and tracking `DONE` / restart triggers.
///
/// Implements [`bfe_battle_ui::scoreboard::ScoreboardPort`] over [`WorldScoreboardPort`],
/// synchronizing live unit kill/death/capture rows from [`Hud::roster`] and recording
/// when the player activates `DONE` at the end of a match (`done_triggered`).
pub struct LiveScoreboardPort {
    /// Underlying `bfe-battle-ui` scoreboard data model.
    pub inner: WorldScoreboardPort,
    /// Set to `true` when the player activates `DONE` on the end-of-match scoreboard.
    pub done_triggered: bool,
    pub next_ready: bool,
}

impl LiveScoreboardPort {
    /// Creates a new [`LiveScoreboardPort`] for the given network mode and era (`"cw"` or `"gcw"`).
    ///
    /// Initializes the two faction [`TeamHeader`] entries (`REBEL ALLIANCE` / `GALACTIC EMPIRE`
    /// for `"gcw"`, or `REPUBLIC` / `CIS` for `"cw"`) with 150 initial reinforcement tickets.
    pub fn new(network: bool, era: &str) -> Self {
        let mut inner = WorldScoreboardPort::new();
        inner.family = GameFamily::Swbf1;
        inner.network = network;
        inner.human_team = 1;
        let is_gcw = era.eq_ignore_ascii_case("gcw");
        inner.teams = if is_gcw {
            [
                Some(TeamHeader {
                    name: "REBEL ALLIANCE".to_string(),
                    reinforcements: 150,
                    side: Some("all".to_string()),
                    logo: Some("com_icon_alliance".to_string()),
                }),
                Some(TeamHeader {
                    name: "GALACTIC EMPIRE".to_string(),
                    reinforcements: 150,
                    side: Some("imp".to_string()),
                    logo: Some("com_icon_imperial".to_string()),
                }),
            ]
        } else {
            [
                Some(TeamHeader {
                    name: "REPUBLIC".to_string(),
                    reinforcements: 150,
                    side: Some("rep".to_string()),
                    logo: Some("com_icon_republic".to_string()),
                }),
                Some(TeamHeader {
                    name: "CIS".to_string(),
                    reinforcements: 150,
                    side: Some("cis".to_string()),
                    logo: Some("com_icon_cis".to_string()),
                }),
            ]
        };
        Self {
            inner,
            done_triggered: false,
            next_ready: !network,
        }
    }

    /// Synchronizes team reinforcement counts, match-over flags, and per-character
    /// kill/death/capture rows from the replicated [`Hud`] and [`Receiver`].
    ///
    /// ## Scoreboard Row Construction
    /// - Updates `human_team`, `MatchStateFlags` (`over`, `game_over`, `finished`), and
    ///   team reinforcement counts (`hud.teams[1][0]`, `hud.teams[2][0]`).
    /// - When `hud.roster_total > 0`, populates `self.inner.characters` from the authoritative
    ///   replicated roster (`kills`, `deaths`, `team_kills`, `captures`, `points = kills*2 - team_kills*2 + captures*5`,
    ///   `rtt`, and network link kind).
    /// - Before the first roster packet arrives, synthesizes character rows from active
    ///   soldier/droideka slots in `receiver.active_slots()`.
    pub fn sync_from_state(&mut self, profile_name: &str, hud: &Hud, receiver: &Receiver) {
        self.inner.human_team = receiver.own.team.clamp(1, 2) as i32;
        self.inner.state = MatchStateFlags {
            over: hud.game_over,
            game_over: hud.game_over,
            finished: hud.game_over,
        };
        if let Some(t1) = &mut self.inner.teams[0] {
            t1.reinforcements = hud.teams[1][0].round() as i32;
        }
        if let Some(t2) = &mut self.inner.teams[1] {
            t2.reinforcements = hud.teams[2][0].round() as i32;
        }

        // Populate character scoreboard rows from the authoritative HUD roster when present,
        // or synthesize rows from active network slots prior to the first roster packet.
        self.inner.characters.clear();
        if hud.roster_total > 0 {
            let own_slot = hud.own_summary.slot as usize;
            for (slot, entry) in hud.roster[..hud.roster_total].iter().enumerate() {
                let is_local = if own_slot > 0 {
                    (slot + 1) == own_slot
                } else {
                    slot == 0 && entry.attached
                };
                let raw_name = entry.name_str();
                let name = if is_local {
                    profile_name.to_string()
                } else if !raw_name.is_empty() {
                    raw_name.to_string()
                } else if entry.team == 1 {
                    format!("Trooper {}", slot + 1)
                } else {
                    format!("Droid {}", slot + 1)
                };
                let mut rec = CharacterRecord::default();
                rec.slots[0].deaths = entry.deaths;
                rec.slots[0].life_kills = entry.kills;
                rec.slots[0].life_kills_best = entry.kills.max(0);
                let link_kind = match entry.link_code {
                    1 => "direct4",
                    2 => "direct6",
                    3 => "relay",
                    _ if entry.attached => "local",
                    _ => "",
                };
                self.inner.characters.push(CharacterStatsEntry {
                    slot,
                    team: entry.team.clamp(1, 2) as i32,
                    name,
                    kills: entry.kills,
                    team_kills: entry.team_kills,
                    deaths: entry.deaths,
                    points: entry.kills * 2 - entry.team_kills * 2 + entry.captures * 5,
                    human: entry.attached,
                    local: is_local,
                    ping: if entry.attached {
                        Some(entry.rtt as i32)
                    } else {
                        None
                    },
                    link_kind: link_kind.to_string(),
                    record: rec,
                });
            }
        } else {
            self.sync_published_roster(receiver);
        }
    }

    fn sync_published_roster(&mut self, receiver: &Receiver) {
        self.inner.characters.clear();
        // scoreboardPort.roster reads the server's Character slots, not
        // visible unit rows. Spawn schema 3 identifies the local slot even
        // when that player's unit is dead or outside snapshot visibility.
        for (slot, c) in receiver.roster.rows() {
            if c.team != 1 && c.team != 2 {
                continue;
            }
            self.inner.characters.push(CharacterStatsEntry {
                slot,
                team: c.team as i32,
                name: c.name().into_owned(),
                kills: c.kills as i32,
                team_kills: c.team_kills as i32,
                deaths: c.deaths as i32,
                points: c.captures as i32,
                human: c.human,
                local: receiver.roster.local == Some(slot),
                ping: if c.human { c.ping.map(i32::from) } else { None },
                link_kind: crate::online::roster::LINKS[c.link].into(),
                record: CharacterRecord::default(),
            });
        }
    }
}

// Implementation of `bfe_battle_ui::scoreboard::ScoreboardPort` for `LiveScoreboardPort`.
// Delegates table/details/awards queries to `self.inner` (`WorldScoreboardPort`)
// and captures the post-match `DONE` button action in `self.done_triggered`.
impl ScoreboardPort for LiveScoreboardPort {
    /// Returns the game family (`GameFamily::Swbf1`).
    fn family(&self) -> GameFamily {
        self.inner.family()
    }
    /// Returns `true` when connected to a multiplayer server.
    fn network(&self) -> bool {
        self.inner.network()
    }
    /// Returns the online player list rows for the pause menu player list dialog.
    fn players(&self) -> Option<Vec<PlayerListRow>> {
        self.inner.players()
    }
    /// Returns the sorted team scoreboard rows.
    fn roster(&self) -> Vec<ScoreRow> {
        self.inner.roster()
    }
    /// Returns the two faction team headers.
    fn teams(&self) -> [Option<TeamHeader>; 2] {
        self.inner.teams()
    }
    /// Returns personal career/match stat rows for `slot`.
    fn personal(&self, slot: usize) -> Vec<PersonalStatRow> {
        self.inner.personal(slot)
    }
    /// Returns post-match medal/award rows.
    fn awards(&self) -> Vec<AwardRow> {
        self.inner.awards()
    }
    /// Returns the local player's team number (`1` or `2`).
    fn human_team(&self) -> i32 {
        self.inner.human_team()
    }
    /// Returns match-over flags (`over`, `game_over`, `finished`).
    fn state(&self) -> MatchStateFlags {
        self.inner.state()
    }
    /// Returns `true` so the end-of-match `DONE` button advances or exits the mission.
    fn can_restart(&self) -> bool {
        self.next_ready
    }
    /// Marks `done_triggered = true` when the player activates `DONE` at match end.
    fn restart(&mut self) -> bool {
        self.done_triggered = true;
        true
    }
}

/// Underlying battle simulation/network authority.
#[allow(clippy::large_enum_variant)]
///
/// Encapsulates the two supported battle backends:
/// - [`BattleAuthority::InstantAction`]: Local 60 Hz mission simulation admitted
///   exclusively through `Context::world_admit` on [`WorldThread`].
/// - [`BattleAuthority::Online`]: Remote or locally hosted `bfeserver` session
///   connected via [`OnlineClient`].
pub enum BattleAuthority {
    /// Local Instant Action battle admitted via `Context::world_admit` and stepped at 60 Hz on [`WorldThread`].
    InstantAction {
        /// Original launch request and mission playlist.
        request: BattleRequest,
        /// Index of the active mission within `request.playlist`.
        playlist_index: usize,
        /// Mission map identifier (e.g. `"nab2"`, `"tat1"`, `"bes1"`).
        map_id: String,
        /// Era code (`"cw"` or `"gcw"`).
        era: String,
        /// Dedicated 60 Hz background world simulation thread.
        world: WorldThread,
        /// Local snapshot/event publication receiver.
        receiver: Box<Receiver>,
        /// Client input command builder and history ring.
        sender: Box<Sender>,
        /// Client-side prediction replay state.
        replay: Box<Replay>,
        /// Interpolated 3D camera controller.
        camera: Box<MapCamera>,
        /// Monotonic elapsed simulation time in milliseconds.
        now_ms: f64,
    },
    /// Online multiplayer battle connected to a remote or LAN `bfeserver`.
    Online {
        /// Multiplayer server launch descriptor.
        launch: MultiplayerLaunch,
        /// Mission map identifier reported by `/info`.
        map_id: String,
        /// Era code (`"cw"` or `"gcw"`).
        era: String,
        /// Live WebRTC / UDP online client session.
        client: Box<OnlineClient>,
    },
}

/// Unified controller for an active Instant Action or Online battle.
///
/// Owns the active [`BattleAuthority`] (`InstantAction` or `Online`), the 3D [`Scene`],
/// all four `bfe-battle-ui` controllers (`SpawnScreenController`, `ScoreboardController`,
/// `PauseMenu`, `ChatController`), and per-frame steady allocation counters.
pub struct BattleController {
    /// Active simulation or network authority (`InstantAction` or `Online`).
    pub authority: BattleAuthority,
    /// Native 3D scene, terrain/sky/unit passes, and HUD bridge.
    pub scene: Box<Scene>,
    /// Keyboard and mouse 3D gameplay input accumulator.
    pub input: Input,
    /// Monotonic 60 Hz client input tick counter.
    pub input_tick: u32,
    /// Fractional frame time accumulator for 60 Hz input stepping.
    pub input_accumulator: f64,
    /// Live data port feeding the spawn screen.
    pub spawn_port: LiveSpawnPort,
    /// `bfe-battle-ui` spawn screen state machine and renderer.
    pub spawn_ctrl: SpawnScreenController,
    /// Live data port feeding the scoreboard.
    pub scoreboard_port: LiveScoreboardPort,
    /// `bfe-battle-ui` scoreboard state machine and renderer (`Table`, `Details`, `Awards`).
    pub scoreboard_ctrl: ScoreboardController,
    /// `bfe-battle-ui` in-battle Escape pause menu (`Resume`, `Respawn`, `Restart`, `Quit`, etc.).
    pub pause_menu: PauseMenu,
    pause_page: super::page_pause::PagePause,
    /// `bfe-battle-ui` in-battle text chat controller (`All` / `Team`).
    pub chat: ChatController,
    // Retained UTF-16 storage for the native chat text field.
    chat_text: Vec<u16>,
    /// Scratch quad batch used when compositing overlays onto `scene.bridge.hud_model.batch`.
    pub overlay_batch: Batch,
    /// Current viewport dimensions `[width, height]` in physical pixels.
    pub size: [i32; 2],
    /// Logical 1024-wide interface height (`interface_height(w, h)`).
    pub height: f64,
    /// Viewport aspect ratio (`width / height`).
    pub aspect: f64,
    /// Last observed mouse cursor position `[x_px, y_px]` in physical window pixels.
    pub cursor_px: [f64; 2],
    /// Total frames rendered in the current battle.
    pub frames: u64,
    /// Count of steady gameplay frames rendered with all 2D overlays closed.
    pub steady_frames: u64,
    /// Cumulative heap allocations on the render thread during steady gameplay after warmup.
    pub steady_allocs_after_warmup: u64,
    /// Frames included in the allocation sample, excluding warmup and overlays.
    pub steady_frames_measured: u64,
    /// Total elapsed seconds in the current battle.
    pub elapsed_s: f64,
    /// Whether the local player was alive on the previous frame (used to detect death -> spawn screen).
    pub was_alive: bool,
    /// Confirmed transitions into a living controlled body.
    pub player_spawns: u64,
    /// Whether the local player has spawned at least once in this battle.
    pub first_life_spawned: bool,
    /// Whether a 2D overlay was open on the previous frame (triggers full HUD quad buffer upload).
    pub overlay_was_active: bool,
}

impl BattleController {
    /// Admits and starts an Instant Action battle for `request.playlist[playlist_index]`
    /// exclusively through `Context::world_admit`.
    ///
    /// ## Instant Action Admission Pipeline
    /// 1. Resolves `(map_id, era)` from `request.playlist[playlist_index]` (or `request.map`).
    /// 2. Constructs [`LocalOptions`] (`--map`, `--side`, `--seed`) and calls
    ///    [`WorldThread::admit_instant_action`], which runs `Prepared::new` -> `Context::world_admit`
    ///    and spawns the dedicated `"bfe-world-60hz"` simulation thread.
    /// 3. Loads the 3D scene (`Scene::load_online`) for `admitted.query` and attaches
    ///    the live battle audio stream to `audio_router`.
    /// 4. Polls the initial tick-0 world publication so `replicas.hud` contains the
    ///    mission's authoritative command posts, unit classes, and reinforcement counts.
    /// 5. Unpauses the 60 Hz world thread and initializes `bfe-battle-ui` controllers
    ///    via [`Self::finish_init`].
    pub fn start_instant_action(
        content_dir: &Path,
        request: BattleRequest,
        playlist_index: usize,
        size: [i32; 2],
        settings: &AppSettingsStore,
        audio_router: &SharedAudioRouter,
        mut make_gl: impl FnMut() -> glow::Context,
    ) -> Result<Self, String> {
        // Resolve the target map ID and era from `request.playlist[playlist_index]` or `request`.
        let default_era = request.era.clone().unwrap_or_else(|| "cw".to_string());
        let (map_id, era) = if let Some(entry) = request
            .playlist
            .as_ref()
            .and_then(|p| p.entries.get(playlist_index))
        {
            (
                entry.map.clone(),
                entry.era.clone().unwrap_or(default_era),
            )
        } else {
            (request.map.clone(), default_era)
        };

        // Build the mission CLI flags (`--map`, `--side`, `--seed`) for `Prepared::new`.
        let side_flag = if era.eq_ignore_ascii_case("gcw") {
            "all"
        } else {
            "rep"
        };
        let local_options = LocalOptions {
            root: content_dir.to_path_buf(),
            battle: vec![
                "--map".to_string(),
                map_id.clone(),
                "--side".to_string(),
                side_flag.to_string(),
                "--seed".to_string(),
                ((playlist_index as u32) + 1).to_string(),
            ],
            audio: true,
            duration: None,
        };

        // Admit the mission exclusively through `Context::world_admit` on the 60 Hz world thread
        // and load the 3D scene (`Scene::load_online`) for the admitted mission query.
        let (mut world, admitted) = WorldThread::admit_instant_action(&local_options)?;
        let (mut scene, camera) =
            Scene::load_online(content_dir, &admitted.query, size, &mut make_gl)?;

        // Route the live battle sound mixer into `SharedAudioRouter`.
        let replicas = scene.online.as_mut().ok_or("missing online replicas")?;
        replicas.hud.auto_build = false;
        audio_router.set_shell(None);
        audio_router.set_live(replicas.sound.output.take());

        // Initialize the local `Receiver`, client-side `Replay`, and `Sender`.
        let mut receiver = Box::new(Receiver::new(admitted.registry));
        receiver.registry = true;
        receiver.welcomed = true;
        receiver.own.id = admitted.player_id;

        let replay = Box::new(
            replicas
                .replay
                .take()
                .ok_or("native prediction resources already admitted")?,
        );
        let mut sender = Box::new(Sender::new(&replicas.turns));
        sender.auto_spawn = false;

        // Drain the initial tick-0 publication so `replicas.hud` has the mission's
        // authoritative command posts, unit classes, and reinforcement counts.
        world.poll_publications(&mut receiver, 0.0)?;
        eprintln!("local welcome received: welcomed={} packets={} corrections={} alive={}",
            receiver.welcomed, receiver.packets, receiver.corrections, receiver.own.alive);
        replicas
            .hud
            .frame(&mut scene.bridge, &receiver, &replay, 0.0);
        receiver.publications.clear();

        let mut spawn_port = LiveSpawnPort::from_hud_meta(
            &replicas.hud.class_meta,
            &replicas.hud.post_meta,
            &replicas.hud.strings,
            &era,
        );
        spawn_port.sync_from_hud(
            &replicas.hud.teams,
            &replicas.hud.post_meta,
            &replicas.hud.own_summary,
            receiver.own.alive,
            receiver.own.team,
        );

        world.set_paused(false);

        Self::finish_init(
            BattleAuthority::InstantAction {
                request,
                playlist_index,
                map_id,
                era: era.clone(),
                world,
                receiver,
                sender,
                replay,
                camera: Box::new(camera),
                now_ms: 0.0,
            },
            scene,
            spawn_port,
            false,
            &era,
            size,
            settings,
        )
    }

    /// Connects to a remote or LAN server (`launch`) and starts an Online battle.
    ///
    /// ## Online Connection Pipeline
    /// 1. Resolves the target `HOST:PORT` from `server_address`, `launch.address`,
    ///    `launch.server_id`, or `BFE_SERVER_TARGET`.
    /// 2. Queries `GET /info` and prepares the player identity and UDP/WebRTC endpoint
    ///    via `crate::online::info::prepare`.
    /// 3. Loads the 3D scene (`Scene::load_online`) for `server_info.query` and
    ///    installs the live battle audio output stream into `audio_router`.
    /// 4. Connects [`OnlineClient`] and initializes `bfe-battle-ui` controllers
    ///    via [`Self::finish_init`].
    pub fn start_online(
        content_dir: &Path,
        launch: MultiplayerLaunch,
        server_address: Option<String>,
        size: [i32; 2],
        settings: &AppSettingsStore,
        audio_router: &SharedAudioRouter,
        mut make_gl: impl FnMut() -> glow::Context,
    ) -> Result<Self, String> {
        // Resolve the target `HOST:PORT` from explicit override, `launch.address`, or `BFE_SERVER_TARGET`.
        let raw_target = launch.direct_target().or(server_address)
            .or_else(|| {
                if launch.server_id.contains(':') && launch.server_id != "host:local" {
                    Some(launch.server_id.clone())
                } else { std::env::var("BFE_SERVER_TARGET").ok() }
            })
            .ok_or_else(|| "No server address specified for online join".to_string())?;

        // Probe `/info` and prepare the authenticated endpoint (`online::info::prepare`).
        let id_path = crate::online::info::default_identity()?;
        let (server_info, endpoint) = crate::online::info::prepare(
            &raw_target,
            content_dir,
            &id_path,
            &settings.player.profile_name,
        )?;

        let selection = bfe_content::mission::Query::parse(&server_info.query);
        let map_id = if selection.get("map").is_empty() {
            if launch.map.is_empty() {
                "nab2".to_string()
            } else {
                launch.map.clone()
            }
        } else {
            selection.get("map").to_string()
        };
        let era = if selection.get("era").is_empty() {
            if launch.era.is_empty() {
                "cw".to_string()
            } else {
                launch.era.clone()
            }
        } else {
            selection.get("era").to_string()
        };

        // Load the 3D scene and HUD replicas for the server's active mission query.
        let (mut scene, camera) =
            Scene::load_online(content_dir, &server_info.query, size, &mut make_gl)?;

        let replicas = scene.online.as_mut().ok_or("missing online replicas")?;
        replicas.hud.auto_build = false;
        audio_router.set_shell(None);
        audio_router.set_live(replicas.sound.output.take());

        let spawn_port = LiveSpawnPort::from_hud_meta(
            &replicas.hud.class_meta,
            &replicas.hud.post_meta,
            &replicas.hud.strings,
            &era,
        );

        // Open the WebRTC / UDP session to `bfeserver`.
        let mut client = Box::new(OnlineClient::connect(endpoint, replicas, camera)?);
        client.sender.auto_spawn = false;

        Self::finish_init(
            BattleAuthority::Online {
                launch,
                map_id,
                era: era.clone(),
                client,
            },
            scene,
            spawn_port,
            true,
            &era,
            size,
            settings,
        )
    }

    /// Shared initialization for `SpawnScreenController`, `ScoreboardController`, `PauseMenu`,
    /// and `ChatController` once the `Scene` and `BattleAuthority` are ready.
    ///
    /// ## UI Controller Initialization
    /// - Computes logical interface height (`interface_height(w, h)`) for the 1024-wide
    ///   3D HUD coordinate space.
    /// - Initializes `SpawnScreenController` (`GameFamily::Swbf1`), copying `MapProjection`
    ///   from `replicas.hud.map` and `BattleUiAssets` from `scene.bridge.hud_model`,
    ///   and opens the spawn screen (`OpenReason::Start`).
    /// - Initializes `ScoreboardController`, `PauseMenu` (with `RESTART MISSION` in
    ///   Instant Action and `PLAYER LIST` in Online), and `ChatController`.
    fn finish_init(
        authority: BattleAuthority,
        mut scene: Scene,
        mut spawn_port: LiveSpawnPort,
        is_online: bool,
        era: &str,
        size: [i32; 2],
        settings: &AppSettingsStore,
    ) -> Result<Self, String> {
        let aspect = size[0].max(1) as f64 / size[1].max(1) as f64;
        let height = interface_height(size[0] as f64, size[1] as f64);

        // Configure `SpawnScreenController` with the mission's overhead map projection and UI assets.
        let mut spawn_ctrl =
            SpawnScreenController::try_with_family(GameFamily::Swbf1, height).map_err(|e| e.to_string())?;
        if let Some(replicas) = &scene.online {
            if let Some(map_arr) = replicas.hud.map {
                spawn_ctrl.view.map_proj = MapProjection {
                    map_x: map_arr[0],
                    map_z: map_arr[1],
                    map_half: map_arr[2],
                    post_icon_size: 6.0 * map_arr[3],
                };
            }
            let surface = HudBatchSurface {
                height,
                aspect,
                batch: &mut scene.bridge.hud_model.batch,
                fonts: &scene.bridge.hud_model.fonts,
                assets: &scene.bridge.hud_model.assets,
                textures: Some(&replicas.hud.names),
                strings: Some(&replicas.hud.strings),
            };
            spawn_ctrl.view.assets = BattleUiAssets::from_surface(&surface);
        }
        spawn_ctrl.open(&mut spawn_port, OpenReason::Start, height);
        eprintln!("spawn screen shown: initial class and post selection");

        let mut scoreboard_port = LiveScoreboardPort::new(is_online, era);
        for (header, team) in scoreboard_port.inner.teams.iter_mut().zip(&spawn_port.teams) {
            if let Some(header) = header {
                header.name=team.name.clone();header.side=Some(team.side.clone());header.logo=Some(team.logo.clone());
            }
        }
        let scoreboard_ctrl =
            ScoreboardController::try_with_family(GameFamily::Swbf1, height).map_err(|e| e.to_string())?;

        // Configure `PauseMenu` (`RESTART MISSION` enabled in Instant Action, `PLAYER LIST` in Online).
        let mut pause_menu =
            PauseMenu::try_with_family(GameFamily::Swbf1).map_err(|e| e.to_string())?;
        pause_menu.enable_respawn(Some(PauseRespawnConfig {
            label: None,
            prompt: None,
            available: true,
        }));
        pause_menu.show_player_list = is_online;
        pause_menu.restart_mode = if is_online {
            PauseRestartMode::Hidden
        } else {
            PauseRestartMode::Custom {
                label: None,
                prompt: None,
            }
        };

        let chat = ChatController::new(&settings.player.profile_name);
        let pause_page = super::page_pause::PagePause::admit(&mut scene,size,settings)?;

        Ok(Self {
            authority,
            scene: Box::new(scene),
            input: Input::default(),
            input_tick: 0,
            input_accumulator: 0.0,
            spawn_port,
            spawn_ctrl,
            scoreboard_port,
            scoreboard_ctrl,
            pause_menu,
            pause_page,
            chat,
            chat_text: Vec::with_capacity(512),
            overlay_batch: Batch::new(),
            size,
            height,
            aspect,
            cursor_px: [size[0] as f64 * 0.5, size[1] as f64 * 0.5],
            frames: 0,
            steady_frames: 0,
            steady_allocs_after_warmup: 0,
            steady_frames_measured: 0,
            elapsed_s: 0.0,
            was_alive: false,
            player_spawns: 0,
            first_life_spawned: false,
            overlay_was_active: true,
        })
    }

    /// Returns the active map ID (`"nab2"`, `"tat1"`, `"bes1"`, etc.).
    pub fn map_id(&self) -> &str {
        match &self.authority {
            BattleAuthority::InstantAction { map_id, .. } => map_id,
            BattleAuthority::Online { map_id, .. } => map_id,
        }
    }

    /// Returns true if any 2D/3D battle overlay (`SpawnScreen`, `Scoreboard`, `PauseMenu`, `Chat`)
    /// is currently open and capturing or overlaying the battle view.
    #[inline]
    pub fn is_overlay_open(&self) -> bool {
        self.spawn_ctrl.is_open()
            || self.scoreboard_ctrl.model.is_open()
            || self.pause_menu.is_open
            || self.pause_menu.waiting()
            || self.chat.is_open()
    }

    /// Returns true when the window should lock/grab the mouse cursor for 3D camera look.
    #[inline]
    pub fn wants_pointer_lock(&self) -> bool {
        !self.spawn_ctrl.is_open()
            && !self.scoreboard_ctrl.model.is_open()
            && !self.pause_menu.is_open
            && !self.chat.is_open()
    }

    /// Number of times the render thread blocked on the 60 Hz world thread (invariant: `0`).
    pub fn world_renderer_blocks(&self) -> u64 {
        match &self.authority {
            BattleAuthority::InstantAction { world, .. } => world.renderer_blocks(),
            BattleAuthority::Online { .. } => 0,
        }
    }

    /// Returns the 60 Hz world thread's tick-time distribution when running Instant Action.
    pub fn world_tick_stats(&self) -> Option<WorldTickStats> {
        match &self.authority {
            BattleAuthority::InstantAction { world, .. } => Some(world.tick_stats()),
            BattleAuthority::Online { .. } => None,
        }
    }

    /// Programmatically spawns the human player if the spawn screen is open.
    pub fn trigger_spawn(&mut self) {
        if self.spawn_ctrl.is_open() {
            // SpawnController.spawn can refuse an unavailable selection.
            // Keep choosing until a request exists, as the JS spawn screen
            // does while authoritative eligibility is still arriving.
            if self.spawn_ctrl.spawn(&mut self.spawn_port).is_some() {
                self.flush_spawn_requests();
                self.spawn_ctrl.close(&mut self.spawn_port);
            }
        }
    }

    /// Programmatically transitions the scoreboard into end-of-match mode so
    /// awards and playlist transitions can be exercised by `--script gate`.
    pub fn force_victory_for_gate(&mut self) {
        if let BattleAuthority::InstantAction { receiver, .. } = &mut self.authority {
            if let Some(replicas) = &mut self.scene.online {
                replicas.hud.game_over = true;
                self.scoreboard_port
                    .sync_from_state("Player 1", &replicas.hud, receiver);
            }
        }
    }

    /// Dispatches any queued [`PendingSpawnChoice`] or suicide request from [`LiveSpawnPort`]
    /// to either the local `WorldThread` (`WorldMessage::Spawn` / `Respawn`) or `OnlineClient`.
    ///
    /// ## Spawn and Respawn Dispatch
    /// - In `BattleAuthority::InstantAction`, sends `WorldMessage::Spawn { team, class_name, post_name }`
    ///   or `WorldMessage::Respawn` over the non-blocking channel to `"bfe-world-60hz"`.
    /// - In `BattleAuthority::Online`, sends `request_team` (if changing teams) and
    ///   `request_spawn(class_reg_id, post_reg_id)` or `request_suicide` via `client.sender`.
    pub fn flush_spawn_requests(&mut self) {
        if let Some(choice) = self.spawn_port.pending_spawn.take() {
            eprintln!("spawn requested: team={} class={} post={}", choice.team, choice.class_name, choice.post_name);
            self.first_life_spawned = true;
            match &mut self.authority {
                BattleAuthority::InstantAction { world, .. } => {
                    world.send(WorldMessage::Spawn {
                        team: choice.team,
                        class_name: choice.class_name,
                        post_name: choice.post_name,
                    });
                }
                BattleAuthority::Online { client, .. } => {
                    client.sender.auto_spawn = true;
                    client.sender.spawned = false;
                    if client.receiver.own.team != 0 && client.receiver.own.team != choice.team {
                        client.sender.request_team(&client.receiver, choice.team);
                    }
                    // SpawnModel.commit names the selected authored class and
                    // post. Index zero is a real first class, not an instruction
                    // to discard its post and ask the server to pick another.
                    client.sender.request_spawn(&client.receiver, choice.class_reg_id as usize, choice.post_reg_id as usize);
                }
            }
        }
        if self.spawn_port.pending_suicide {
            self.spawn_port.pending_suicide = false;
            match &mut self.authority {
                BattleAuthority::InstantAction { world, .. } => {
                    world.send(WorldMessage::Respawn);
                }
                BattleAuthority::Online { client, .. } => {
                    client.sender.request_suicide(&client.receiver);
                }
            }
        }
    }

    /// Samples one 60 Hz [`ClientCommand`], [`ClientLook`], [`ClientFlightCommand`], and [`ViewActions`]
    /// from `self.input` (or zeroes movement when a 2D overlay captures input).
    ///
    /// Applies the user's persisted `mouse_sensitivity` (`cmd.mouse_scale`) and
    /// vertical look inversion (`cmd.invert`) from `settings.player.shell`.
    /// When `active_gameplay` is `false` (unfocused window or open 2D overlay),
    /// clears held movement keys and returns zeroed look/flight/view deltas.
    fn take_step_command(
        &mut self,
        generation: u32,
        active_gameplay: bool,
        settings: &AppSettingsStore,
    ) -> (ClientCommand, ClientLook, ClientFlightCommand, ViewActions) {
        let mouse_scale = settings.player.shell.mouse_sensitivity as f64;
        let invert = settings.player.shell.invert_y;
        self.input_tick = self.input_tick.wrapping_add(1);
        if active_gameplay {
            let (mut cmd, look) = self.input.take(self.input_tick, generation);
            cmd.mouse_scale = mouse_scale;
            cmd.has_mouse_scale = true;
            cmd.invert = Some(invert);
            let flight = self.input.take_flight();
            let actions = self.input.take_view();
            (cmd, look, flight, actions)
        } else {
            self.input.clear();
            let (mut cmd, _) = self.input.take(self.input_tick, generation);
            cmd.mouse_scale = mouse_scale;
            cmd.has_mouse_scale = true;
            cmd.invert = Some(invert);
            (
                cmd,
                ClientLook::default(),
                ClientFlightCommand::default(),
                ViewActions::default(),
            )
        }
    }

    /// Steps the battle simulation/network client and renders one complete frame into
    /// `self.scene.canvas.framebuffer`.
    ///
    /// ## Six-Stage Per-Frame Battle Pipeline
    /// 1. **Spawn Request Flush**: Forwards any pending `SPAWN` or `RESPAWN` action to
    ///    the local `WorldThread` or `OnlineClient`.
    /// 2. **Overlay Transition Check**: Inspects `PauseMenu` (`ExitToWindows`, `RestartMission`,
    ///    `QuitToMainMenu`, `LeaveOnline`) and `ScoreboardPort::done_triggered` (`NextPlaylistBattle`).
    /// 3. **Simulation / Network Step & Camera Interpolation**:
    ///    - Accumulates `dt` into 60 Hz `STEP` (`1/60 s`) increments and samples up to 4
    ///      [`ClientCommand`] steps via [`Self::take_step_command`].
    ///    - In `InstantAction`, drains non-blocking `Publication` frames from `WorldThread`,
    ///      sends `WorldMessage::Step`, steps client-side `Replay`, updates `MapCamera`,
    ///      and calls `scene.present_network`.
    ///    - In `Online`, calls `client.receive`, `client.step`, `client.present`, `client.send`,
    ///      and `scene.online_frame`.
    /// 4. **Life/Death & Match-Over Transitions**: Closes the spawn screen upon first spawn,
    ///    re-opens the spawn screen (`OpenReason::Death`) upon death, and synchronizes
    ///    ports when `game_over` becomes `true`.
    /// 5. **HUD & Battle UI Batch Assembly**: Builds either the active 2D overlay batch
    ///    ([`Self::update_and_build_overlays`]) or the live combat HUD (`hud_model_update` +
    ///    `hud_model.build(0)`), then synchronizes vertex ranges via `bridge.hud_sync_batch`.
    /// 6. **Unified 3D + HUD Draw & Allocation Accounting**: Invokes `scene.draw(&view)`
    ///    and records steady-state render-thread heap allocations after warmup.

    pub fn step_and_draw(
        &mut self,
        dt: f64,
        focused: bool,
        settings: &mut AppSettingsStore,
        _audio_router: &SharedAudioRouter,
    ) -> Result<Option<BattleTransition>, String> {
        let dt = dt.clamp(0.0, 0.1);
        self.pause_page.save(settings);
        self.elapsed_s += dt;

        if let BattleAuthority::Online { client, launch, .. } = &mut self.authority {
            client.poll_rotation(launch.address.as_deref().unwrap_or(""), launch.port.unwrap_or(18451));
            self.scoreboard_port.next_ready = client.rotation_ready;
        }

        // 1. Flush any pending spawn/respawn requests from UI interactions.
        self.flush_spawn_requests();

        // 2. Check pause/leave/restart triggers from PauseMenu and ScoreboardController.
        if self.pause_menu.window_close_triggered > 0 {
            self.pause_menu.window_close_triggered = 0;
            return Ok(Some(BattleTransition::ExitToWindows));
        }
        if self.pause_menu.custom_restart_triggered > 0 {
            self.pause_menu.custom_restart_triggered = 0;
            if let BattleAuthority::InstantAction {
                request,
                playlist_index,
                ..
            } = &self.authority
            {
                return Ok(Some(BattleTransition::RestartMission {
                    request: request.clone(),
                    playlist_index: *playlist_index,
                }));
            }
        }
        if self.pause_menu.respawn_triggered > 0 {
            self.pause_menu.respawn_triggered = 0;
            match &mut self.authority {
                BattleAuthority::InstantAction { world, .. } => {
                    world.send(WorldMessage::Respawn);
                }
                BattleAuthority::Online { client, .. } => {
                    client.sender.request_suicide(&client.receiver);
                }
            }
        }
        if let Some(leaving) = self.pause_menu.leaving.take() {
            if leaving.why == "restart" {
                if let BattleAuthority::InstantAction {
                    request,
                    playlist_index,
                    ..
                } = &self.authority
                {
                    return Ok(Some(BattleTransition::RestartMission {
                        request: request.clone(),
                        playlist_index: *playlist_index,
                    }));
                }
            }
            return match &self.authority {
                BattleAuthority::InstantAction { .. } => Ok(Some(BattleTransition::QuitToMainMenu)),
                BattleAuthority::Online { .. } => {
                    Ok(Some(BattleTransition::LeaveOnline { reason: None }))
                }
            };
        }
        if self.scoreboard_port.done_triggered {
            self.scoreboard_port.done_triggered = false;
            return match &self.authority {
                BattleAuthority::InstantAction {
                    request,
                    playlist_index,
                    ..
                } => Ok(Some(BattleTransition::NextPlaylistBattle {
                    request: request.clone(),
                    next_index: playlist_index + 1,
                })),
                BattleAuthority::Online { client, launch, .. } => {
                    if client.rotation_ready {
                        let mut launch = launch.clone();
                        if let Some(next) = &client.receiver.next_match {
                            let query = bfe_content::mission::Query::parse(&next.query);
                            launch.map = query.get("map").into();
                            launch.side = query.get("side").into();
                        }
                        Ok(Some(BattleTransition::NextOnlineBattle { launch }))
                    } else { Ok(None) }
                }
            };
        }

        let ui_captures_input = self.spawn_ctrl.is_open()
            || self.scoreboard_ctrl.model.is_open()
            || self.pause_menu.is_open
            || self.chat.is_open();
        let active_gameplay = focused && !ui_captures_input;
        // Align render-thread steady allocation accounting with the 45-frame combat
        // warm-up window (`phase_frames == 45` in `script.rs`) so initial spawn and
        // welcome-snapshot presentation warm-up completes before steady counting starts.
        let measure_steady_allocs = !self.is_overlay_open() && self.steady_frames >= 45;
        let alloc_before = if measure_steady_allocs {
            super::render_thread_alloc_count()
        } else {
            0
        };

                //
        // The render loop runs at display refresh rate while simulation and network
        // input ticks run at a fixed 60 Hz (`STEP = 1.0 / 60.0` seconds).
        // We accumulate `dt` (clamped to `0.25s` max catch-up), consume up to 4
        // discrete 60 Hz steps per rendered frame, and use the residual fraction
        // `alpha_interp = (input_accumulator / STEP)` to smoothly interpolate the
        // 3D camera between the previous and current 60 Hz simulation states.
        // 3. Pump publications / packets, step input, and compute camera View.
        self.input_accumulator = (self.input_accumulator + dt).min(0.25);
        let mut steps = 0usize;
        while self.input_accumulator >= STEP && steps < 4 {
            self.input_accumulator -= STEP;
            steps += 1;
        }
        let alpha_interp = (self.input_accumulator / STEP).clamp(0.0, 1.0);

        let generation = match &self.authority {
            BattleAuthority::InstantAction { receiver, .. } => receiver.own.generation as u32,
            BattleAuthority::Online { client, .. } => client.generation(),
        };
        let mut step_cmds: [Option<(
            ClientCommand,
            ClientLook,
            ClientFlightCommand,
            ViewActions,
        )>; 4] = [None, None, None, None];
        for slot in step_cmds.iter_mut().take(steps) {
            *slot = Some(self.take_step_command(generation, active_gameplay, settings));
        }

        let view: View = match &mut self.authority {
            BattleAuthority::InstantAction {
                world,
                receiver,
                sender,
                replay,
                camera,
                now_ms,
                ..
            } => {
                // Drain non-blocking publications from `"bfe-world-60hz"`, forward input steps,
                // run client-side prediction replay, and interpolate the 3D camera.
                *now_ms += dt * 1000.0;
                world.set_paused(self.pause_menu.is_open);
                world.set_suspended(!focused);
                world.poll_publications(receiver, *now_ms)?;
                if self.frames == 60 {
                    eprintln!("local HUD publications: focused={} welcomed={} packets={} corrections={} alive={} active={} visible={}",
                        focused, receiver.welcomed, receiver.packets, receiver.corrections, receiver.own.alive,
                        receiver.active().count(), self.scene.online.as_ref().map_or(0, |r| r.visible));
                }
                sender.feedback(receiver);
                receiver.clock.advance(dt).map_err(str::to_string)?;

                let ack = Publication::acknowledgement(receiver);
                for (cmd, look, flight, actions) in step_cmds.into_iter().flatten() {
                    sender.tick(&cmd, &look, &flight, receiver, 0.0, *now_ms);
                    world.send(WorldMessage::Step {
                        command: cmd,
                        look,
                        flight,
                        actions,
                        ack,
                    });
                }

                let mut own = receiver.own;
                sender.preview(receiver);
                if let Err(err) = replay.frame(receiver, sender) {
                    receiver.fault = Some(err);
                }
                if let Some(hull) = &replay.fleet.state {
                    own.position = hull.position;
                }
                if replay.fleet.identity.is_some() {
                    sender.yaw = replay.fleet.aim[0];
                    sender.pitch = replay.fleet.aim[1];
                }
                if let Some(body) = &replay.body {
                    own.position = body.position;
                    own.posture = body.posture as usize;
                    own.air = body.air.map_or(0, |a| a as u8 + 1);
                    sender.yaw = body.aim_yaw;
                    sender.pitch = body.aim_pitch;
                } else {
                    sender.yaw = own.yaw;
                    sender.pitch = own.pitch;
                }
                if receiver.corrections != 0 {
                    let pos =
                        three::Vector3::new(own.position[0], own.position[1], own.position[2]);
                    let aim = AimView {
                        position: pos,
                        feet: pos,
                        yaw: sender.yaw,
                        pitch: sender.pitch,
                        posture: own.posture,
                        air: own.air != 0,
                        zoom: own.zoom,
                        ..Default::default()
                    }
                    .sample::<V8>();
                    camera.follow([aim.eye.x, aim.eye.y, aim.eye.z], sender.yaw, sender.pitch, dt);
                }
                let v = *camera.present(alpha_interp);
                self.scene
                    .present_network(receiver, replay, &sender.history, camera, dt);
                receiver.publications.clear();
                v
            }
            BattleAuthority::Online { client, .. } => {
                // Receive network packets, step client prediction, send input datagrams,
                // and present the interpolated online scene.
                if let Err(err) = client.receive(dt) {
                    return Ok(Some(BattleTransition::LeaveOnline { reason: Some(err) }));
                }
                for (cmd, look, flight, actions) in step_cmds.into_iter().flatten() {
                    if let Err(err) = client.step(&cmd, &look, &flight, &actions, STEP) {
                        return Ok(Some(BattleTransition::LeaveOnline { reason: Some(err) }));
                    }
                }
                let v = *client.present(alpha_interp);
                if let Err(err) = client.send() {
                    return Ok(Some(BattleTransition::LeaveOnline { reason: Some(err) }));
                }
                self.scene.online_frame(client, dt);
                if self.frames == 60 {
                    eprintln!("online HUD publications: welcomed={} packets={} corrections={} alive={} active={} visible={}",
                        client.receiver.welcomed, client.receiver.packets, client.receiver.corrections,
                        client.receiver.own.alive, client.receiver.active().count(),
                        self.scene.online.as_ref().map_or(0, |r| r.visible));
                }
                v
            }
        };

                //
        // Track transitions of the local player's `own_alive` flag across frames:
        // - `false -> true`: Close `SpawnScreenController` once the first life spawns.
        // - `true -> false` (while `!game_over`): Re-open `SpawnScreenController`
        //   with `OpenReason::Death` so the player can select a new class/post.
        // - `game_over == true`: Synchronize `LiveScoreboardPort` so the post-battle
        //   scoreboard (`BoardState::End`) displays final tickets and awards.
        // 4. Inspect life/death and match-over transitions.
        let (own_alive, game_over) = match &self.authority {
            BattleAuthority::InstantAction { receiver, .. } => {
                let over = self
                    .scene
                    .online
                    .as_ref()
                    .is_some_and(|r| r.hud.game_over);
                (receiver.own.alive, over)
            }
            BattleAuthority::Online { client, .. } => {
                let over = self
                    .scene
                    .online
                    .as_ref()
                    .is_some_and(|r| r.hud.game_over);
                (client.receiver.own.alive, over)
            }
        };

        // Admission may already publish a living first body before the user
        // commits the screen. A real click must close it even without a new
        // false-to-true alive edge (the scripted driver used to close it itself).
        if own_alive && self.first_life_spawned && self.spawn_ctrl.is_open() {
            eprintln!("spawn answer: authoritative player alive; closing spawn screen");
            self.spawn_ctrl.close(&mut self.spawn_port);
        }
        if own_alive && !self.was_alive {
            self.was_alive = true;
            self.player_spawns += 1;
            if self.spawn_ctrl.is_open() && self.first_life_spawned {
                self.spawn_ctrl.close(&mut self.spawn_port);
            }
        } else if !own_alive && self.was_alive && !game_over {
            self.was_alive = false;
            if !self.spawn_ctrl.is_open() {
                self.sync_ports_cold(&settings.player.profile_name);
                self.spawn_ctrl
                    .open(&mut self.spawn_port, OpenReason::Death, self.height);
            }
        }

        if game_over && !self.scoreboard_ctrl.ended {
            self.sync_ports_cold(&settings.player.profile_name);
        }

                //
        // When an overlay (`SpawnScreen`, `Scoreboard`, `PauseMenu`, `Chat`) is open,
        // `update_and_build_overlays` populates `scene.bridge.hud_model.batch` and we
        // upload the full batch (`full_sync = true`).
        // When all overlays are closed, `hud_model_update()` + `hud_model.build(0)`
        // updates only the dirty in-battle HUD quad ranges in place with zero heap
        // allocations, and `steady_frames` increments after frame 2.
        // 5. Build HUD + battle UI overlay batch before `scene.draw(&view)`.
        let overlay_active = self.is_overlay_open() || (game_over && !self.scoreboard_ctrl.ended);
        reset_hud_layer(&mut self.scene.bridge.hud_model.input);
        if overlay_active {
            self.sync_ports_cold(&settings.player.profile_name);
            self.update_and_build_overlays(dt);
        } else {
            self.scene.bridge.hud_model_update();
            self.scene.bridge.hud_model.build(0);
            if self.frames > 2 {
                self.steady_frames += 1;
            }
        }

        let full_sync = self.frames == 0 || self.overlay_was_active || overlay_active;
        self.overlay_was_active = overlay_active;
        self.scene.bridge.hud_sync_batch(full_sync);

        // PageTheme is a compositor over the finished battle, just as its
        // CSS overlay is over the canvas. Defer only the pause UI until its
        // backdrop has been filtered; the renderer still draws the world.
        let pause_groups=self.scene.bridge.r.hud.input[render_hud::IN_GROUPS];
        if self.pause_menu.is_open{self.scene.bridge.r.hud.input[render_hud::IN_GROUPS]=0.;}
        self.scene.draw(&view)?;
        if self.pause_menu.is_open {
            self.pause_page.blur.apply(&self.scene.bridge.r.core.gl,self.scene.canvas.framebuffer,6.);
            self.scene.bridge.r.core.state.reset();
            self.scene.bridge.r.hud.input[render_hud::IN_GROUPS]=pause_groups;
            self.scene.bridge.r.hud.input[render_hud::IN_UPLOAD]=0.;
            self.scene.bridge.r.hud.input[render_hud::IN_UPLOAD+1]=(self.scene.bridge.hud_model.batch.count*4)as f64;
            if let Some(modal) = self.pause_page.page.modal_start {
                super::page_pause::PagePause::draw_range(&mut self.scene,0,modal);
                self.pause_page.blur.apply(&self.scene.bridge.r.core.gl,self.scene.canvas.framebuffer,4.);
                let end=self.scene.bridge.hud_model.batch.count;
                super::page_pause::PagePause::draw_range(&mut self.scene,modal,end);
            } else {
                self.scene.bridge.r.hud.draw(&mut self.scene.bridge.r.core);
            }
        }
        if measure_steady_allocs && !overlay_active {
            let alloc_after = super::render_thread_alloc_count();
            self.steady_allocs_after_warmup += alloc_after.saturating_sub(alloc_before);
            self.steady_frames_measured += 1;
        }
        self.frames += 1;
        Ok(None)
    }

    /// Synchronizes [`LiveSpawnPort`], [`LiveScoreboardPort`], and the pause menu player list
    /// from the latest HUD state when an overlay is open or opening.
    ///
    /// Called on overlay open/update transitions (never on steady 3D gameplay frames)
    /// so string/vector cloning inside `ScoreboardPort` and `SpawnPort` never allocates
    /// during steady-state combat rendering.
    fn sync_ports_cold(&mut self, profile_name: &str) {
        let Some(replicas) = &self.scene.online else {
            return;
        };
        match &self.authority {
            BattleAuthority::InstantAction { receiver, .. } => {
                self.spawn_port.sync_from_hud(
                    &replicas.hud.teams,
                    &replicas.hud.post_meta,
                    &replicas.hud.own_summary,
                    receiver.own.alive,
                    receiver.own.team,
                );
                self.scoreboard_port
                    .sync_from_state(profile_name, &replicas.hud, receiver);
            }
            BattleAuthority::Online { client, .. } => {
                let previous_team = self.spawn_port.human.team;
                self.spawn_port.sync_from_hud(
                    &replicas.hud.teams,
                    &replicas.hud.post_meta,
                    &replicas.hud.own_summary,
                    client.receiver.own.alive,
                    client.receiver.own.team,
                );
                // spawn-screen/index.js follows the authoritative teamChanged
                // event even when its initial screen opened before welcome.
                if self.spawn_port.human.team != previous_team {
                    self.spawn_ctrl.notify_team_changed(&self.spawn_port, self.spawn_port.human.team, 0.25);
                }
                self.scoreboard_port
                    .sync_from_state(profile_name, &replicas.hud, &client.receiver);
            }
        }
        if self.pause_menu.show_player_list {
            self.pause_menu.players = self.scoreboard_port.players().unwrap_or_default();
        }
    }

    /// Updates and composites the active 2D/3D battle overlay (`PauseMenu`, `SpawnScreenController`,
    /// or `ScoreboardController`) into `self.scene.bridge.hud_model.batch`.
    ///
    /// ## Overlay Rendering Priority
    /// 1. **Escape `PauseMenu` (`pause_menu.hides_hud()`)**: Clears the HUD batch, draws
    ///    a translucent dark scrim over the 3D scene, and renders the pause menu modal
    ///    (buttons, options sliders, controls table, player list, or confirmation prompt).
    /// 2. **`SpawnScreenController` (`spawn_open`)**: Updates the spawn screen model,
    ///    synchronizes tactical map HUD inputs (`sync_hud_inputs`), builds the base HUD
    ///    pass (overhead map + command post icons), draws faction emblems, renders the
    ///    class/post selection panels, and draws the cursor pointer.
    /// 3. **`ScoreboardController` (`scoreboard_ctrl.model.is_open()`)**: Builds the base
    ///    combat HUD, updates the scoreboard controller, and composites `draw_under` +
    ///    `draw_over` (`Table`, `Details`, or `Awards` page) onto `hud_model.batch`.
    fn update_and_build_overlays(&mut self, dt: f64) {
        let fh = self.scene.bridge.hud_model.fonts[0].height;
        self.scene.bridge.hud_model.input[H::DT] = dt;
        self.scene.bridge.hud_model.input[H::HEIGHT] = self.height;
        self.scene.bridge.hud_model.input[H::ASPECT] = self.aspect;
        self.scene.bridge.hud_model.input[H::FOV] = 60.0;


        let spawn_open = self.spawn_ctrl.is_open();
        let menu_open = self.pause_menu.is_open;

        // When the Escape pause menu is open, it hides the live HUD and draws over a dark scrim.
        if self.pause_menu.hides_hud() {
            self.scene.bridge.hud_model.batch.begin();
            self.pause_menu.update(dt);
            self.overlay_batch.begin();
            if let Some(replicas) = &self.scene.online {
                let surface = HudBatchSurface {
                    height: self.height,
                    aspect: self.aspect,
                    batch: &mut self.scene.bridge.hud_model.batch,
                    fonts: &self.scene.bridge.hud_model.fonts,
                    assets: &self.scene.bridge.hud_model.assets,
                    textures: Some(&replicas.hud.names),
                    strings: Some(&replicas.hud.strings),
                };
                self.pause_page.page.draw_pause(&mut self.overlay_batch,&self.pause_page.textures,&mut self.pause_menu,&surface,self.size[0] as f64,self.size[1] as f64,&self.pause_page.theme);
            }
            Self::append_batch(&mut self.scene.bridge.hud_model.batch, &self.overlay_batch);
            return;
        }

        // When the spawn screen is open, synchronize HUD inputs, draw the tactical map,
        // team emblems, class/post panels, and cursor pointer.
        if spawn_open {
            self.spawn_ctrl.update(
                dt,
                &mut self.spawn_port,
                self.height,
                fh,
                false,
                0.25,
            );
            self.spawn_ctrl.view.sync_hud_inputs(
                &self.spawn_ctrl.model,
                &self.spawn_port,
                false,
                &mut self.scene.bridge.hud_model.input,
            );
            self.scene.bridge.hud_model_update();
            self.scene.bridge.hud_model.build(0);
            self.draw_spawn_team_emblems();
            if let Some(replicas) = &self.scene.online {
                let mut surface = HudBatchSurface {
                    height: self.height,
                    aspect: self.aspect,
                    batch: &mut self.scene.bridge.hud_model.batch,
                    fonts: &self.scene.bridge.hud_model.fonts,
                    assets: &self.scene.bridge.hud_model.assets,
                    textures: Some(&replicas.hud.names),
                    strings: Some(&replicas.hud.strings),
                };
                self.spawn_ctrl
                    .view
                    .draw(&mut surface, &self.spawn_ctrl.model, &self.spawn_port);
            }
            self.spawn_ctrl.view.draw_pointer(
                &mut self.scene.bridge.hud_model.batch,
                self.height,
                self.spawn_ctrl.input.cursor,
            );
            return;
        }

        self.scene.bridge.hud_model_update();
        self.scene.bridge.hud_model.build(0);

        // Update and draw the Tab / end-of-match Scoreboard (`Table`, `Details`, `Awards`).
        if let Some(replicas) = &self.scene.online {
            {
                let surface = HudBatchSurface {
                    height: self.height,
                    aspect: self.aspect,
                    batch: &mut self.scene.bridge.hud_model.batch,
                    fonts: &self.scene.bridge.hud_model.fonts,
                    assets: &self.scene.bridge.hud_model.assets,
                    textures: Some(&replicas.hud.names),
                    strings: Some(&replicas.hud.strings),
                };
                self.scoreboard_ctrl.update(
                    dt,
                    &mut self.scoreboard_port,
                    &surface,
                    spawn_open,
                    menu_open,
                );
            }
            if self.scoreboard_ctrl.model.is_open() {
                self.overlay_batch.begin();
                {
                    let surface = HudBatchSurface {
                        height: self.height,
                        aspect: self.aspect,
                        batch: &mut self.scene.bridge.hud_model.batch,
                        fonts: &self.scene.bridge.hud_model.fonts,
                        assets: &self.scene.bridge.hud_model.assets,
                        textures: Some(&replicas.hud.names),
                        strings: Some(&replicas.hud.strings),
                    };
                    self.scoreboard_ctrl
                        .view
                        .draw_under(&surface, &mut self.overlay_batch);
                    self.scoreboard_ctrl.view.draw_over(
                        &surface,
                        &mut self.overlay_batch,
                        &self.scoreboard_ctrl.model,
                        &self.scoreboard_port,
                        Some(self.scoreboard_ctrl.input.cursor),
                    );
                }
                Self::append_batch(&mut self.scene.bridge.hud_model.batch, &self.overlay_batch);
            }
            if self.pause_menu.waiting() {
                self.pause_menu.update(dt);
                self.overlay_batch.begin();
                {
                    let surface = HudBatchSurface {
                        height: self.height,
                        aspect: self.aspect,
                        batch: &mut self.scene.bridge.hud_model.batch,
                        fonts: &self.scene.bridge.hud_model.fonts,
                        assets: &self.scene.bridge.hud_model.assets,
                        textures: Some(&replicas.hud.names),
                        strings: Some(&replicas.hud.strings),
                    };
                    self.pause_page.page.draw_pause(&mut self.overlay_batch,&self.pause_page.textures,&mut self.pause_menu,&surface,self.size[0] as f64,self.size[1] as f64,&self.pause_page.theme);
                }
                Self::append_batch(&mut self.scene.bridge.hud_model.batch, &self.overlay_batch);
            }
        }
        if self.chat.is_open() {
            // The controller previously swallowed keys without presenting its draft.
            // Draw the native text field after other overlays so typed text stays visible.
            self.chat_text.clear();
            let prefix = if self.chat.active_channel == Some(bfe_battle_ui::ChatChannel::Team) {
                "TEAM: "
            } else { "ALL: " };
            self.chat_text.extend(prefix.encode_utf16());
            self.chat_text.extend(self.chat.draft.encode_utf16());
            let hud = &mut self.scene.bridge.hud_model;
            hud.fonts[0].draw(&mut hud.batch, root_anchor(self.height), 24.0,
                self.height - 70.0, &self.chat_text, [1.0; 4], 0, 1.0, 1.0, 0.0);
        }
    }

    /// Renders the faction emblems and team labels in the top-left corner of the spawn screen.
    ///
    /// Draws the primary (`own_team`, green tint) and secondary (`other_team`, red tint)
    /// faction emblem quads (`rep_icon` / `cis_icon`) and centered UTF-16 team name
    /// labels at `layout.logos.own` and `layout.logos.other`.
    fn draw_spawn_team_emblems(&mut self) {
        let fh = self.scene.bridge.hud_model.fonts[0].height;
        let layout = self.spawn_ctrl.view.layout(self.height, fh);
        let root = root_anchor(self.height);
        let own_team = self.spawn_ctrl.model.team;
        let other_team = if own_team == 1 { 2 } else { 1 };
        let Some(replicas) = &self.scene.online else {
            return;
        };
        for (team, pos, radius, color) in [
            (
                own_team,
                layout.logos.own,
                layout.logos.big * 0.5,
                [0.125, 0.875, 0.125, 0.95] as Color,
            ),
            (
                other_team,
                layout.logos.other,
                layout.logos.small * 0.5,
                [0.875, 0.125, 0.125, 0.55] as Color,
            ),
        ] {
            let m = translate(root, pos[0], pos[1], 0.0);
            let name = if team == 1 { "rep_icon" } else { "cis_icon" };
            let tex = replicas
                .hud
                .names
                .get(name)
                .copied()
                .unwrap_or(A::HUD_FLAG_ICON);
            self.scene.bridge.hud_model.batch.quad(
                m,
                [-radius, -radius, radius, radius],
                tex,
                UV,
                color,
                0.0,
                0,
            );
            let label = if team == 1 {
                self.spawn_port.teams[0].name.as_str()
            } else {
                self.spawn_port.teams[1].name.as_str()
            };
            let utf16: Vec<u16> = label.encode_utf16().collect();
            self.scene.bridge.hud_model.fonts[1].draw(
                &mut self.scene.bridge.hud_model.batch,
                m,
                0.0,
                radius + 2.0,
                &utf16,
                alpha([1.0, 1.0, 1.0, 1.0], color[3]),
                2,
                0.85,
                1.0,
                0.0,
            );
        }
    }

    /// Appends all quads and draw groups from `src` onto `dst` (`hud_model.batch`).
    ///
    /// Copies vertex attributes (`position`, `uv`, `mask_uv`, `tint`) for `src.count` quads
    /// into `dst` starting at `dst.count`, offsetting each draw group's `first` index
    /// by `base_quad * 6`.
    fn append_batch(dst: &mut Batch, src: &Batch) {
        let base_quad = dst.count;
        let count = src.count.min(render_hud::QUADS.saturating_sub(base_quad));
        if count == 0 {
            return;
        }
        let (v0, v1, sv1) = (base_quad * 4, (base_quad + count) * 4, count * 4);
        dst.position[v0 * 3..v1 * 3].copy_from_slice(&src.position[..sv1 * 3]);
        dst.uv[v0 * 2..v1 * 2].copy_from_slice(&src.uv[..sv1 * 2]);
        dst.mask_uv[v0 * 2..v1 * 2].copy_from_slice(&src.mask_uv[..sv1 * 2]);
        dst.tint[v0 * 4..v1 * 4].copy_from_slice(&src.tint[..sv1 * 4]);
        let index_offset = base_quad * 6;
        for draw in &src.draws {
            dst.draws.push(Draw {
                first: draw.first + index_offset,
                ..*draw
            });
        }
        dst.count += count;
        dst.end = v1;
    }

    /// Routes a keyboard key press/release through PauseMenu, Chat, SpawnScreen, Scoreboard,
    /// and 3D gameplay input (`Input`).
    ///
    /// ## Keyboard Input Priority Chain
    /// 1. **`PauseMenu` / `Escape`**: Always gets first priority so the player can open,
    ///    navigate, or dismiss the pause menu from any battle state.
    /// 2. **`ChatController` (`KeyT` / `KeyY` / open chat)**: Captures text entry and
    ///    `Enter`/`Escape` when in-battle chat is active.
    /// 3. **`SpawnScreenController`**: Handles `Enter`/`Space` (`Spawn`), `ArrowUp`/`ArrowDown`
    ///    (class selection), `ArrowLeft`/`ArrowRight` (post selection), and team switch keys.
    /// 4. **`ScoreboardController` (`Tab` or post-match end screen)**: Opens or navigates
    ///    `Table`, `Details`, `Awards`, and `DONE`.
    /// 5. **3D Gameplay (`Input`)**: Handles `KeyM` (open spawn screen) and forwards
    ///    movement/action keys (`WASD`, `Space`, `KeyE`, `KeyR`, `KeyC`, `F1..F4`) to `self.input`.
    pub fn handle_key(
        &mut self,
        code: &str,
        down: bool,
        repeat: bool,
        settings: &mut AppSettingsStore,
    ) {
        if self.pause_menu.is_open&&down&&self.pause_page.key(code){return;}
        // 1. Escape or open PauseMenu always gets priority.
        if self.pause_menu.is_open || code == "Escape" {
            self.sync_ports_cold(&settings.player.profile_name);
            if let Some(replicas) = &self.scene.online {
                let surface = HudBatchSurface {
                    height: self.height,
                    aspect: self.aspect,
                    batch: &mut self.scene.bridge.hud_model.batch,
                    fonts: &self.scene.bridge.hud_model.fonts,
                    assets: &self.scene.bridge.hud_model.assets,
                    textures: Some(&replicas.hud.names),
                    strings: Some(&replicas.hud.strings),
                };
                if self.pause_menu.handle_key(
                    code,
                    down,
                    repeat,
                    &surface,
                    &settings.player.pause_kv,
                ) {
                    self.input.clear();
                    return;
                }
            }
        }

        // 2. Chat overlay when open or activated via Enter/T/Y.
        if down && !repeat && (self.chat.is_open() || matches!(code, "KeyT" | "KeyY")) {
            let act = route_battle_key(
                code,
                repeat,
                false,
                &mut self.chat,
                self.spawn_ctrl.is_open(),
                self.scoreboard_ctrl.model.is_open(),
                self.pause_menu.is_open,
                self.elapsed_s * 1000.0,
            );
            if act == BattleKeyAction::ChatHandled {
                self.input.clear();
                return;
            }
        }

        // 3. Spawn screen input when open.
        if self.spawn_ctrl.is_open() {
            let fh = self.scene.bridge.hud_model.fonts[0].height;
            let (height, model, view, port) = (
                self.height,
                &self.spawn_ctrl.model,
                &mut self.spawn_ctrl.view,
                &self.spawn_port,
            );
            let (_, action) = self.spawn_ctrl.input.key(
                model,
                code,
                down,
                repeat,
                true,
                |x, y| view.hit(x, y, height, fh, model, port),
            );
            if action == SpawnInputAction::Spawn {
                self.spawn_ctrl.spawn(&mut self.spawn_port);
                self.flush_spawn_requests();
            }
            return;
        }

        // 4. Scoreboard toggle on Tab or navigation when open.
        if code == "Tab" && down && !repeat && !self.scoreboard_ctrl.model.is_open() {
            self.sync_ports_cold(&settings.player.profile_name);
            if let Some(replicas) = &self.scene.online {
                let surface = HudBatchSurface {
                    height: self.height,
                    aspect: self.aspect,
                    batch: &mut self.scene.bridge.hud_model.batch,
                    fonts: &self.scene.bridge.hud_model.fonts,
                    assets: &self.scene.bridge.hud_model.assets,
                    textures: Some(&replicas.hud.names),
                    strings: Some(&replicas.hud.strings),
                };
                if self.scoreboard_ctrl.peek(
                    &self.scoreboard_port,
                    &surface,
                    false,
                    self.pause_menu.is_open,
                ) {
                    self.input.clear();
                    return;
                }
            }
        }

        if self.scoreboard_ctrl.model.is_open() {
            if let Some(replicas) = &self.scene.online {
                let surface = HudBatchSurface {
                    height: self.height,
                    aspect: self.aspect,
                    batch: &mut self.scene.bridge.hud_model.batch,
                    fonts: &self.scene.bridge.hud_model.fonts,
                    assets: &self.scene.bridge.hud_model.assets,
                    textures: Some(&replicas.hud.names),
                    strings: Some(&replicas.hud.strings),
                };
                let (model, view, port) = (
                    &self.scoreboard_ctrl.model,
                    &self.scoreboard_ctrl.view,
                    &self.scoreboard_port,
                );
                self.scoreboard_ctrl.input.key(
                    code,
                    down,
                    repeat,
                    true,
                    true,
                    |x, y| view.hit(x, y, model, port, &surface),
                );
                let pending: Vec<ScoreboardAction> =
                    std::mem::take(&mut self.scoreboard_ctrl.input.actions);
                for act in pending {
                    self.scoreboard_ctrl
                        .act(act, &mut self.scoreboard_port, self.height);
                }
            }
            return;
        }

        // 5. Normal 3D gameplay input (`M` opens spawn screen).
        if code == "KeyM" && down && !repeat {
            self.sync_ports_cold(&settings.player.profile_name);
            self.spawn_ctrl
                .open(&mut self.spawn_port, OpenReason::Start, self.height);
            self.input.clear();
            return;
        }
        if !repeat {
            if let Some(kc) = dom_code_to_keycode(code) {
                self.input.key(kc, down);
            }
        }
    }

    /// Updates 2D overlay cursor position on `CursorMoved`.
    ///
    /// Stores the raw window pixel coordinates in `self.cursor_px`, projects them
    /// into the 1024-wide logical interface space via `to_interface`, and updates
    /// pointer hover state on whichever overlay (`PauseMenu`, `SpawnScreen`, `Scoreboard`) is open.
    pub fn handle_cursor_move(&mut self, x_px: f64, y_px: f64) {
        self.cursor_px = [x_px, y_px];
        let pt = to_interface(
            x_px,
            y_px,
            self.size[0] as f64,
            self.size[1] as f64,
            self.height,
        );
        if self.pause_menu.is_open {
            self.pause_menu.move_pointer(pt[0], pt[1], self.height);
        } else if self.spawn_ctrl.is_open() {
            self.spawn_ctrl.input.move_cursor(pt[0], pt[1], self.height);
        } else if self.scoreboard_ctrl.model.is_open() {
            self.scoreboard_ctrl.input.move_cursor(pt[0], pt[1]);
        }
    }

    /// Routes mouse button events to the active 2D overlay or 3D weapon trigger.
    ///
    /// ## Mouse Button Routing
    /// - Converts `self.cursor_px` from window pixels into 1024-wide logical interface
    ///   coordinates via `to_interface`.
    /// - Routes primary clicks (`button == 0`) to `PauseMenu::click`, `SpawnScreenController`,
    ///   or `ScoreboardController` when any of those overlays is open.
    /// - Otherwise forwards `MouseButton::Left` (primary fire), `Right` (secondary/zoom),
    ///   and `Middle` to `self.input.mouse`.
    pub fn handle_mouse_button(
        &mut self,
        button: u8,
        down: bool,
        settings: &AppSettingsStore,
    ) {
        let pt = to_interface(
            self.cursor_px[0],
            self.cursor_px[1],
            self.size[0] as f64,
            self.size[1] as f64,
            self.height,
        );
        if self.pause_menu.is_open {
            if button==0&&down&&self.pause_page.pointer(self.cursor_px[0],self.cursor_px[1]){return;}
            if button == 0 && down {
                if let Some(replicas) = &self.scene.online {
                    let surface = HudBatchSurface {
                        height: self.height,
                        aspect: self.aspect,
                        batch: &mut self.scene.bridge.hud_model.batch,
                        fonts: &self.scene.bridge.hud_model.fonts,
                        assets: &self.scene.bridge.hud_model.assets,
                        textures: Some(&replicas.hud.names),
                        strings: Some(&replicas.hud.strings),
                    };
                    self.pause_menu
                        .click(pt[0], pt[1], &surface, &settings.player.pause_kv);
                }
            }
            return;
        }
        if self.spawn_ctrl.is_open() {
            if button == 0 && down {
                let fh = self.scene.bridge.hud_model.fonts[0].height;
                let (height, model, view, port) = (
                    self.height,
                    &self.spawn_ctrl.model,
                    &mut self.spawn_ctrl.view,
                    &self.spawn_port,
                );
                let (_, action) = self.spawn_ctrl.input.click(
                    model,
                    pt[0],
                    pt[1],
                    height,
                    true,
                    |x, y| view.hit(x, y, height, fh, model, port),
                );
                if action == SpawnInputAction::Spawn {
                    self.spawn_ctrl.spawn(&mut self.spawn_port);
                    self.flush_spawn_requests();
                }
            }
            return;
        }
        if self.scoreboard_ctrl.model.is_open() {
            if button == 0 && down {
                if let Some(replicas) = &self.scene.online {
                    let surface = HudBatchSurface {
                        height: self.height,
                        aspect: self.aspect,
                        batch: &mut self.scene.bridge.hud_model.batch,
                        fonts: &self.scene.bridge.hud_model.fonts,
                        assets: &self.scene.bridge.hud_model.assets,
                        textures: Some(&replicas.hud.names),
                        strings: Some(&replicas.hud.strings),
                    };
                    let (model, view, port) = (
                        &self.scoreboard_ctrl.model,
                        &self.scoreboard_ctrl.view,
                        &self.scoreboard_port,
                    );
                    self.scoreboard_ctrl.input.click(pt[0], pt[1], |x, y| {
                        view.hit(x, y, model, port, &surface)
                    });
                    let pending: Vec<ScoreboardAction> =
                        std::mem::take(&mut self.scoreboard_ctrl.input.actions);
                    for act in pending {
                        self.scoreboard_ctrl
                            .act(act, &mut self.scoreboard_port, self.height);
                    }
                }
            }
            return;
        }

        let mb = match button {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            _ => return,
        };
        self.input.mouse(mb, down);
    }

    /// Routes mouse wheel scroll to the active 2D overlay or 3D weapon cycle.
    ///
    /// Scrolls the spawn class/post list when `SpawnScreenController` is open,
    /// scrolls the scoreboard table when `ScoreboardController` is open, or cycles
    /// the player's active weapon slot in `self.input` during 3D combat.
    pub fn handle_wheel(&mut self, delta_y: f64) {
        if self.spawn_ctrl.is_open() {
            self.spawn_ctrl
                .input
                .wheel(&self.spawn_ctrl.model, delta_y, true);
        } else if self.scoreboard_ctrl.model.is_open() {
            self.scoreboard_ctrl
                .model
                .scroll(if delta_y > 0.0 { 1 } else { -1 });
        } else if !self.pause_menu.is_open {
            self.input.wheel(delta_y);
        }
    }

    /// Opens the pause menu on `Escape` or focus loss, or switches scoreboard pages for the gate.
    ///
    /// Used by `--script gate` to deterministically open the scoreboard and cycle through
    /// [`BoardPage::Table`] (`11-ia-nab2-scoreboard.png`), [`BoardPage::Details`]
    /// (`12-ia-nab2-scoreboard-details.png`), and [`BoardPage::Awards`]
    /// (`13-ia-nab2-scoreboard-awards.png`).
    pub fn set_scoreboard_page(&mut self, page: BoardPage) {
        self.sync_ports_cold("Player 1");
        if !self.scoreboard_ctrl.model.is_open() {
            if let Some(replicas) = &self.scene.online {
                let surface = HudBatchSurface {
                    height: self.height,
                    aspect: self.aspect,
                    batch: &mut self.scene.bridge.hud_model.batch,
                    fonts: &self.scene.bridge.hud_model.fonts,
                    assets: &self.scene.bridge.hud_model.assets,
                    textures: Some(&replicas.hud.names),
                    strings: Some(&replicas.hud.strings),
                };
                self.scoreboard_ctrl.peek(
                    &self.scoreboard_port,
                    &surface,
                    false,
                    false,
                );
            }
        }
        match page {
            BoardPage::Table => {
                if self.scoreboard_ctrl.model.page != BoardPage::Table {
                    self.scoreboard_ctrl
                        .act(ScoreboardAction::Back, &mut self.scoreboard_port, self.height);
                }
            }
            BoardPage::Details => {
                self.scoreboard_ctrl.act(
                    ScoreboardAction::Details,
                    &mut self.scoreboard_port,
                    self.height,
                );
            }
            BoardPage::Awards => {
                if self.scoreboard_ctrl.model.state != BoardState::End {
                    self.scoreboard_port.inner.state = MatchStateFlags {
                        over: true,
                        game_over: true,
                        finished: true,
                    };
                    self.scoreboard_ctrl.ended = true;
                    self.scoreboard_ctrl
                        .model
                        .open(BoardState::End, &self.scoreboard_port);
                }
                self.scoreboard_ctrl.act(
                    ScoreboardAction::Awards,
                    &mut self.scoreboard_port,
                    self.height,
                );
            }
        }
    }

    /// Opens the pause confirmation popup (`ConfirmKind`) for automated testing/screenshots.
    ///
    /// Ensures `self.pause_menu` is open and invokes `PauseMenu::ask(kind, &surface)`
    /// to display the `YES` / `NO` confirmation modal (e.g. `ConfirmKind::Quit` for
    /// `17-ia-bes1-pause-quit.png`).
    pub fn open_pause_confirm(&mut self, kind: ConfirmKind) {
        if !self.pause_menu.is_open {
            self.pause_menu.open("key", self.height);
        }
        if let Some(replicas) = &self.scene.online {
            let surface = HudBatchSurface {
                height: self.height,
                aspect: self.aspect,
                batch: &mut self.scene.bridge.hud_model.batch,
                fonts: &self.scene.bridge.hud_model.fonts,
                assets: &self.scene.bridge.hud_model.assets,
                textures: Some(&replicas.hud.names),
                strings: Some(&replicas.hud.strings),
            };
            self.pause_menu.ask(kind, &surface);
        }
    }
}

/// Maps a DOM `KeyboardEvent.code` string (e.g. `"KeyW"`, `"Space"`, `"F1"`) to a
/// `winit::keyboard::KeyCode` for 3D movement, vehicle controls, and camera view toggles.
///
/// Translates DOM `KeyboardEvent.code` strings back into `winit::keyboard::KeyCode`
/// values consumed by `crate::input::Input::key` for 3D soldier/vehicle controls.
fn dom_code_to_keycode(code: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match code {
        "KeyW" => KeyW,
        "KeyA" => KeyA,
        "KeyS" => KeyS,
        "KeyD" => KeyD,
        "KeyE" => KeyE,
        "KeyF" => KeyF,
        "KeyG" => KeyG,
        "KeyC" => KeyC,
        "KeyX" => KeyX,
        "KeyZ" => KeyZ,
        "KeyR" => KeyR,
        "KeyQ" => KeyQ,
        "KeyV" => KeyV,
        "KeyM" => KeyM,
        "Space" => Space,
        "Enter" => Enter,
        "ArrowUp" => ArrowUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        "ShiftLeft" => ShiftLeft,
        "ShiftRight" => ShiftRight,
        "ControlLeft" => ControlLeft,
        "ControlRight" => ControlRight,
        "AltLeft" => AltLeft,
        "AltRight" => AltRight,
        "CapsLock" => CapsLock,
        "Backquote" => Backquote,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        _ => return None,
    })
}

// HudInput.facts in src/hud-input.js rewrites layer facts every frame. A
// closed spawn screen releases its battlefield map so HudModel draws the
// bottom-left minimap. Online and Instant Action share this boundary.
fn reset_hud_layer(input: &mut [f64]) {
    for at in [
        H::LAYER,
        H::SPAWN_MAP,
        H::VIEW_TEAMS,
        H::HIDE_TEAMS,
        H::FLIP,
        H::COVERED,
    ] {
        input[at] = 0.;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mission_team_order_drives_spawn_classes() {
        let class = |team, side: &str, name: &str, id, order| HudClassMeta {
            team,
            side: side.into(),
            class_name: name.into(),
            registry_id: id,
            roster_order: order,
            display_name: name.into(),
            weapons: vec![],
        };
        let classes = [
            class(2, "rep", "rep_soldier", 4, 0),
            class(1, "cis", "cis_assault", 8, 1),
            class(1, "cis", "cis_soldier", 7, 0),
        ];
        let port =
            LiveSpawnPort::from_hud_meta(&classes, &[], &std::collections::HashMap::new(), "cw");
        assert_eq!(port.teams[0].side, "cis");
        assert_eq!(port.teams[1].side, "rep");
        assert_eq!(port.class_registry_ids[0], vec![7, 8]);
        assert_eq!(port.rosters[1][0].class_name, "rep_soldier");
    }

    #[test]
    fn respawn_keeps_the_authoritative_side() {
        let mut port =
            LiveSpawnPort::from_hud_meta(&[], &[], &std::collections::HashMap::new(), "cw");
        port.sync_from_hud(&[[0.; 4]; 8], &[], &HudOwnSummary::default(), false, 2);
        assert_eq!(port.human_state().team, 2);
        assert_eq!(port.human_state().phase, "awaiting");
    }

    #[test]
    fn scoreboard_uses_published_characters_instead_of_visible_units() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        let bytes = [
            2, 0, 0, 0, 2, 1, 1, 7, 0, 1, 0, 3, 0, 4, 0, 1, 0, 1, 6, b'P', b'l', b'a', b'y', b'e',
            b'r', 2, 0, 2, 0, 0, 0, 5, 0, 1, 0, 255, 255, 0, 3, b'B', b'o', b't',
        ];
        receiver.roster.receive(4, 70000, &bytes).unwrap();
        receiver.roster.local = Some(0);
        let mut board = LiveScoreboardPort::new(true, "cw");
        board.sync_published_roster(&receiver);
        let rows = board.roster();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Player");
        assert_eq!((rows[0].kills, rows[0].deaths, rows[0].points), (5, 3, 4));
        assert!(rows[0].human && rows[0].local);
        assert_eq!(rows[0].ping, Some(1));
        assert_eq!(board.players().unwrap().len(), 1);
        assert!(!rows[1].human);
    }

    #[test]
    fn spawn_close_restores_javascript_minimap_at_both_resolutions() {
        use bfe_render::hud_model::HudModel;
        for (width, height) in [(1280., 720.), (1920., 1080.)] {
            let mut hud = HudModel::new();
            let h = interface_height(width, height);
            hud.input[H::HEIGHT] = h;
            hud.input[H::MAP] = 1.;
            hud.input[H::PLAYER] = 1.;
            hud.input[H::MAP_IMAGE] = A::MAP_MASK as f64;
            hud.input[H::MAP_HALF] = 500.;
            for asset in &mut hud.assets {
                asset.ready = true;
            }
            // A frame after spawn used to retain these, drawing the 419.84
            // unit spawn map on the right instead of the 143.36 unit minimap.
            hud.input[H::LAYER] = 1.;
            hud.input[H::SPAWN_MAP] = 1.;
            reset_hud_layer(&mut hud.input);
            hud.build(0);
            let draw = hud
                .batch
                .draws
                .iter()
                .find(|d| d.texture == A::MAP_MASK)
                .unwrap();
            let first = draw.first / 6 * 12;
            let positions = &hud.batch.position[first..first + 12];
            // Frozen interface/space.js anchorMatrix and HudModel.map math:
            // left = 5%, width = 143.36 / 1024, bottom = 95% of the canvas.
            let xs: Vec<f64> = positions
                .as_chunks::<3>().0.iter()
                .map(|p| (p[0] as f64 / 1024. + 0.5) * width)
                .collect();
            let ys: Vec<f64> = positions
                .as_chunks::<3>().0.iter()
                .map(|p| (0.5 - p[1] as f64 / 768.) * height)
                .collect();
            let left = xs.iter().copied().fold(f64::INFINITY, f64::min);
            let right = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let bottom = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            assert!((left - width * 0.05).abs() < 0.001);
            assert!((right - left - width * 143.36 / 1024.).abs() < 0.001);
            assert!((bottom - height * 0.95).abs() < 0.001);
        }
    }
}

impl Drop for BattleController {
    fn drop(&mut self){self.pause_page.blur.dispose(&self.scene.bridge.r.core.gl);}
}
