//! Battlefront Eternal in-battle UI crate (`bfe-battle-ui`).
//!
//! Ports the Classic spawn screen (`src/spawn-screen/`), Classic scoreboard and
//! battle statistics (`src/scoreboard/`, `src/battle-stats.js`), in-battle pause menu
//! (`src/pause/`), and HUD input/chat publication (`src/hud-input.js`, `src/chat.js`).
//!
//! ### Architecture
//! - [`shapes`] and [`surface`]: The 2004 `1024xH` perspective interface plane
//!   (`z = -886.81`, 60-degree horizontal FOV, 90% safe area), 9-slice containers,
//!   5-stage `TitleIntro` flashy text/bar animations, and the [`BattleUiSurface`]
//!   drawing trait that decouples all views from WebGL/OpenGL.
//! - [`spawn`]: The 2004 `ifs_spawndisplay` screen — startup URL/mission policy
//!   (`spawn_policy`), pure selection state machine (`SpawnModel`), keyboard/mouse
//!   input with 1-frame nav cooldown and `Tab` post cycling (`SpawnInput`), tilted
//!   35/-20/45-degree window geometry (`ClassicSpawnLayout`), HUD/3D view
//!   (`ClassicSpawnView`), and controller (`SpawnScreenController`).
//! - [`scoreboard`]: The 2004 `ifs_teamstats`, `ifs_personalstats`, and `ifs_awardstats`
//!   screens, per-slot battle statistics (`CharacterRecord`), `TabHold` tap-vs-hold
//!   state machine, and spiral award-medallion flight paths.
//! - [`pause`]: The in-battle `ifs_pausemenu` screen stack, confirmation popups
//!   (`RESPAWN`, `RESTART MISSION`, `ABORT MISSION`), and pointer-unlock grace period.
//! - [`hud_input`] and [`chat`]: Shared-memory HUD Float64Array publication
//!   (`HudInputPublisher`) with unit-row hysteresis and in-battle chat routing.

/// In-battle text chat log, channel state (`ALL` / `TEAM`), and key routing (`src/chat.js`).
pub mod chat;
/// Shared-memory HUD Float64Array state/event publisher (`src/hud-input.js`).
pub mod hud_input;
/// In-battle `ifs_pausemenu` state stack, layout, and confirmation popups (`src/pause/`).
pub mod pause;
/// Classic `ifs_teamstats` / `ifs_personalstats` / `ifs_awardstats` scoreboard (`src/scoreboard/`).
pub mod scoreboard;
/// Classic `1024xH` interface plane geometry, 9-slice cutting, and `TitleIntro` (`src/interface/`).
pub mod shapes;
/// Classic `ifs_spawndisplay` spawn screen model, policy, input, layout, and view (`src/spawn-screen/`).
pub mod spawn;
/// `BattleUiSurface` trait, `HudBatchSurface` adapter, and `OwnedUiSurface` test surface.
pub mod surface;

// Re-export HUD input shared-memory publisher, asset/ring limits, and chat types.
pub use hud_input::{
    relation_code, route_battle_key, BattleKeyAction, ChatChannel, ChatController, ChatLog,
    ChatMessage, HudAssetSlot, HudDefinitionFacts, HudInputPublisher, HudPostFact, HudTeamFact,
    HudUnitFact, HudWeaponFact, CHAT_LIMIT, HUD_ART_NAMES, MAX_ASSETS, MAX_RINGS, WORDS,
};
// Re-export pause menu controller, layout config, confirmation dialogs, and URL helper.
pub use pause::{
    pause_layout_for_family, row_bars, rows_shift, still_command, with_settings, ConfirmKind,
    LeavingRecord, PauseHitTarget, PauseHotspot, PauseLayoutConfig, PauseMenu, PauseRespawnConfig,
    PauseRestartMode, PauseRowId, PauseScreenState, PauseSettingsLookup, RowBarSpec, StillCommand,
    LAYOUT as PAUSE_LAYOUT, LIST_COUNT, LOCK_GRACE, PROMPT_DELAY,
};
// Re-export Classic scoreboard model, view, input, battle stats, and award calculators.
pub use scoreboard::{
    awards, clock, compare_rows, link_label, personal_stats, row_at, slot_of, stats_page_layout,
    stats_page_layout_for_family, team_stats_layout, team_stats_layout_for_family, AwardRow,
    BoardPage, BoardState, CharacterRecord, CharacterStatsEntry, ClassicScoreboardView,
    DetailsSubject, Flight, FlightPoint, LogoPlacement, MatchStateFlags, NetworkEntitySummary,
    NetworkPlayerRow, PersonalStatRow, PlayerListRow, RowCursor, ScoreButtonBox, ScoreRect,
    ScoreRow, ScoreboardAction, ScoreboardController, ScoreboardEvent, ScoreboardHitTarget,
    ScoreboardInput, ScoreboardModel, ScoreboardPort, SlotRecord, SortMode, StatsPage,
    StatsPageLayout, StatsPageRow, StatsPageRowsConfig, TabHold, TeamHeader, TeamSideLayout,
    TeamStatsLayout, TimedEntry, WorldScoreboardPort, ABOVE_PLAYER, AWARDS, AWARDS_ROWS, COLUMN,
    DETAILS_ROWS, DISPLAY_PAD, DOUBLE_CLICK, HEADER, HOLD, ICON, IDLE_QUIT, IDLE_STEP, PERSONAL,
    PITCH, RING_SIZE, STRING_PAD,
};
// Re-export `1024xH` interface plane geometry, 9-slice helpers, `TitleIntro`, and surface types.
pub use shapes::{
    draw_nine_slice, draw_nine_slice_window, encode_utf16_into, font_index_by_name,
    format_i64_utf16_into, glyph_height, identity_matrix, inside_quad, interface_height,
    make_rotation_yxz, nine_slice, nine_slice_pieces, place_interface_model, plane_point,
    projected_corners, rgba, root_anchor, rotate_x, scale_xyz, scaled, selected_color, tint,
    to_interface, to_plane_point, wrap_lines, wrap_lines_ranges, BattleUiAssets, BattleUiSurface,
    FlashyBlock, FlashyFrame, GameFamily, GameFamilyError, HudBatchSurface, IntroFrame,
    OwnedUiSurface, TitleIntro, BLACK, BODY_UNITS, CUBE_UNITS, DIMMED, DROIDEKA_UNITS, FONT_LARGE,
    FONT_MEDIUM, FONT_NAME_LARGE, FONT_NAME_MEDIUM, FONT_NAME_SMALL, FONT_NAME_TINY, FONT_SMALL,
    FONT_TINY, FOV_DEG, GREY, H_4_3, IW, LOGO_UNITS, PLANE, PREVIEW_UNITS, ROOM_SCALE, SAFE,
    SAFE_SCALE, W, WHITE,
};
// Re-export Classic spawn screen controller, model, policy, input, layout, and view types.
pub use spawn::{
    choose_frontline_post, classic_layout, fallback_logo_for_side, human_entry, playable_classes,
    queue_human_command, side_of_class, spawn_layout_for_family, spawn_map_place, spawn_policy,
    spawn_policy_from_query, team_of_class, AutoSpawnPick, CameraShot, ClassSlotLayout,
    ClassicSpawnLayout, ClassicSpawnView, HumanCommand, HumanSpawnFacts, MapProjection,
    MissionSpec, MissionTeamSpec, OpenReason, PostChangeReason, Spawn3dPlacement, SpawnClassEntry,
    SpawnEvent, SpawnHitTarget, SpawnInput, SpawnInputAction, SpawnLogoPlacement,
    SpawnMapPlacement, SpawnModel, SpawnParams, SpawnPolicy, SpawnPort, SpawnPostEntry,
    SpawnRequest, SpawnScreenController, SpawnSound, SpawnState, SpawnTeamFacts, TeamSwitchSource,
    COMMIT_WAIT, DEFAULT_UNIT, HUMAN_COMMAND_LIMIT, NAME_LIMIT, SLOTS, TAB_REPEAT,
};

// ============================================================================
// Unit tests for `bfe-battle-ui`
//
// Covers:
// - `GameFamily` selector validation (`Swbf1` vs. `Swbf2` error rejection)
// - `spawn_policy` URL query resolution and `queue_human_command` deduplication
// - `SpawnModel` frontline post selection, team switching, and request commit
// - `SpawnInput` pre-held `Space` spawn and `Tab` 0.2s command-post cycling
// - `ClassicSpawnView` perspective-projected 3D quad hit-testing
// - `WorldScoreboardPort` personal stats, end-of-match awards, and sorting
// - `TabHold` tap-vs-hold state machine and double-click row inspection
// - `PauseMenu` Escape handling, 250ms pointer-unlock grace, and confirm popups
// - `HudInputPublisher` retained state, 3-frame unit hysteresis, and chat routing
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use bfe_render::hud_model::layout::{
        HudCommand as C, HudFrame as H, HudRing as R, HudState as S, HudTeam as T, HudUnit as U,
    };
    use bfe_render::hud_model::{MAX_POSTS, MAX_TEAMS, MAX_UNITS, RINGS, STATES, TEAMS, UNITS};
    use std::collections::BTreeMap;

    // ------------------------------------------------------------------------
    // Fake SpawnPort for unit tests (ports `tests/spawn-screen-fake-port.js`)
    //
    // Simulates a two-team battle (Republic vs. CIS) with 5 playable classes
    // per faction and 4 command posts (`cp_1`..`cp_4`) arranged along the
    // diagonal from `(-100, -100)` to `(100, 100)` so front-line post selection
    // (`choose_frontline_post`) deterministically picks `cp_2` for Team 1 and
    // `cp_3` for Team 2.
    // ------------------------------------------------------------------------
    struct TestSpawnPort {
        /// Current human spawn state returned by `human_state()`.
        human: HumanSpawnFacts,
        /// Per-team metadata (`[team1, team2]`) returned by `team_facts(team)`.
        teams: [SpawnTeamFacts; 2],
        /// Per-team 5-class rosters (`[team1, team2]`) returned by `roster(team)`.
        rosters: [Vec<SpawnClassEntry>; 2],
        /// Per-team command post lists (`[team1, team2]`) returned by `posts(team)`.
        posts: [Vec<SpawnPostEntry>; 2],
        /// Log of `SpawnRequest`s submitted via `request(&choice)`.
        requested: Vec<SpawnRequest>,
        /// Count of `suicide()` calls from pause menu respawn.
        suicides: usize,
        /// Count of `cancel()` calls when a committed spawn is withdrawn.
        cancels: usize,
        /// Mission overview camera shots returned by `camera_shots()`.
        shots: Vec<CameraShot>,
        /// Currently active overview camera shot set via `set_view_shot(shot)`.
        active_shot: Option<CameraShot>,
    }

    impl TestSpawnPort {
        /// Constructs a default 2-team test port with 5 classes per side and 4 diagonal posts.
        fn new() -> Self {
            let make_classes = |prefix: &str, label_prefix: &str| -> Vec<SpawnClassEntry> {
                (0..5)
                    .map(|i| SpawnClassEntry {
                        class_name: format!("{prefix}_class_{i}"),
                        display_name: format!("{label_prefix} Class {i}"),
                        weapons: vec!["Blaster Rifle".to_string(), "Thermal Detonator".to_string()],
                        eligible: true,
                    })
                    .collect()
            };
            let posts_t1 = vec![
                SpawnPostEntry {
                    name: "cp_1".to_string(),
                    display_name: "Command Post 1".to_string(),
                    position: [-100.0, 0.0, -100.0],
                    owner: 1,
                    eligible: true,
                    vehicles: vec!["AT-TE".to_string()],
                },
                SpawnPostEntry {
                    name: "cp_2".to_string(),
                    display_name: "Command Post 2".to_string(),
                    position: [-20.0, 0.0, -20.0],
                    owner: 1,
                    eligible: true,
                    vehicles: Vec::new(),
                },
                SpawnPostEntry {
                    name: "cp_3".to_string(),
                    display_name: "Command Post 3".to_string(),
                    position: [20.0, 0.0, 20.0],
                    owner: 2,
                    eligible: false,
                    vehicles: Vec::new(),
                },
                SpawnPostEntry {
                    name: "cp_4".to_string(),
                    display_name: "Command Post 4".to_string(),
                    position: [100.0, 0.0, 100.0],
                    owner: 2,
                    eligible: false,
                    vehicles: Vec::new(),
                },
            ];
            // Team 2 sees the same 4 command posts, but only posts owned by team 2 (`cp_3`, `cp_4`)
            // are marked `eligible` for spawning.
            let posts_t2: Vec<SpawnPostEntry> = posts_t1
                .iter()
                .map(|p| {
                    let mut c = p.clone();
                    c.eligible = c.owner == 2;
                    c
                })
                .collect();

            Self {
                human: HumanSpawnFacts::default(),
                teams: [
                    SpawnTeamFacts {
                        side: "rep".to_string(),
                        name: "Republic".to_string(),
                        logo: "com_icon_republic".to_string(),
                        objectives: vec!["Eliminate enemy forces".to_string()],
                        reinforcements: 150,
                    },
                    SpawnTeamFacts {
                        side: "cis".to_string(),
                        name: "CIS".to_string(),
                        logo: "com_icon_cis".to_string(),
                        objectives: vec!["Capture the palace".to_string()],
                        reinforcements: 150,
                    },
                ],
                rosters: [make_classes("rep", "Clone"), make_classes("cis", "Droid")],
                posts: [posts_t1, posts_t2],
                requested: Vec::new(),
                suicides: 0,
                cancels: 0,
                shots: vec![CameraShot {
                    position: [0.0, 50.0, 100.0],
                    quaternion: [0.0, 0.0, 0.0, 1.0],
                }],
                active_shot: None,
            }
        }
    }

    // Implements the `SpawnPort` trait over `TestSpawnPort`'s in-memory fixtures.
    impl SpawnPort for TestSpawnPort {
        /// Returns a snapshot of the simulated local human player's spawn state.
        fn human_state(&self) -> HumanSpawnFacts {
            self.human.clone()
        }
        /// Returns team metadata for Team 1 (Republic) or Team 2 (CIS).
        fn team_facts(&self, team: u8) -> Option<SpawnTeamFacts> {
            match team {
                1 => Some(self.teams[0].clone()),
                2 => Some(self.teams[1].clone()),
                _ => None,
            }
        }
        /// Returns the 5-class playable roster for `team`.
        fn roster(&self, team: u8) -> Vec<SpawnClassEntry> {
            match team {
                2 => self.rosters[1].clone(),
                _ => self.rosters[0].clone(),
            }
        }
        /// Returns the 4 command posts with `eligible` flags for `team`.
        fn posts(&self, team: u8) -> Vec<SpawnPostEntry> {
            match team {
                2 => self.posts[1].clone(),
                _ => self.posts[0].clone(),
            }
        }
        /// Appends a committed `SpawnRequest` to `self.requested`.
        fn request(&mut self, choice: &SpawnRequest) {
            self.requested.push(choice.clone());
        }
        /// Increments the pause-menu `RESPAWN` suicide counter.
        fn suicide(&mut self) {
            self.suicides += 1;
        }
        /// Increments the withdrawn-commit cancel counter.
        fn cancel(&mut self) {
            self.cancels += 1;
        }
        /// Returns the simulated mission overview camera shots.
        fn camera_shots(&self) -> Vec<CameraShot> {
            self.shots.clone()
        }
        /// Records the active overview camera shot (or `None` when released).
        fn set_view_shot(&mut self, shot: Option<CameraShot>) {
            self.active_shot = shot;
        }
    }

    /// Builds a sample Theed (`Swbf1`) mission fixture where Team 1 is CIS (with
    /// Darth Maul as non-playable hero) and Team 2 is Republic (with Mace Windu
    /// as non-playable hero and `rep_inf_clone_trooper` as `DEFAULT_UNIT`).
    fn sample_mission() -> MissionSpec {
        MissionSpec {
            family: GameFamily::Swbf1,
            sides: vec!["cis".to_string(), "rep".to_string()],
            team1: Some(MissionTeamSpec {
                name: "CIS".to_string(),
                hero: Some("cis_inf_darthmaul".to_string()),
                classes: vec![
                    "cis_inf_battledroid".to_string(),
                    "cis_inf_assault".to_string(),
                    "cis_inf_darthmaul".to_string(),
                ],
            }),
            team2: Some(MissionTeamSpec {
                name: "Republic".to_string(),
                hero: Some("rep_inf_macewindu".to_string()),
                classes: vec![
                    "rep_inf_clone_trooper".to_string(),
                    "rep_inf_arc_trooper".to_string(),
                    "rep_inf_macewindu".to_string(),
                ],
            }),
        }
    }

    // ------------------------------------------------------------------------
    // GameFamily selector tests (`MULTIPLAYER.md` lines 106-183)
    //
    // Verifies that `GameFamily::Swbf1` succeeds across all layout and controller
    // constructors while `GameFamily::Swbf2` returns an explicit `GameFamilyError`
    // instead of silently falling back to the 2004 Classic layout.
    // ------------------------------------------------------------------------
    #[test]
    fn test_game_family_selectors_swbf1_and_swbf2_error() {
        // `GameFamily::parse` accepts bare family IDs and `<family>/<mapId>` prefixes.
        assert_eq!(GameFamily::parse("swbf1"), Some(GameFamily::Swbf1));
        assert_eq!(GameFamily::parse("swbf2/hot1"), Some(GameFamily::Swbf2));
        assert!(GameFamily::Swbf1.require_available().is_ok());

        let err = GameFamily::Swbf2.require_available().unwrap_err();
        assert_eq!(err.family, GameFamily::Swbf2);
        assert_eq!(err.to_string(), "game family swbf2 is not available");

        // Every layout and controller constructor must reject `Swbf2` with `GameFamilyError`.
        assert!(spawn_layout_for_family(GameFamily::Swbf1, 768.0, 15.0).is_ok());
        assert_eq!(
            spawn_layout_for_family(GameFamily::Swbf2, 768.0, 15.0),
            Err(GameFamilyError::not_available(GameFamily::Swbf2))
        );

        assert!(team_stats_layout_for_family(GameFamily::Swbf1, 768.0).is_ok());
        assert_eq!(
            team_stats_layout_for_family(GameFamily::Swbf2, 768.0),
            Err(GameFamilyError::not_available(GameFamily::Swbf2))
        );

        assert!(stats_page_layout_for_family(GameFamily::Swbf1, 768.0, DETAILS_ROWS).is_ok());
        assert_eq!(
            stats_page_layout_for_family(GameFamily::Swbf2, 768.0, DETAILS_ROWS),
            Err(GameFamilyError::not_available(GameFamily::Swbf2))
        );

        assert!(pause_layout_for_family(GameFamily::Swbf1).is_ok());
        assert_eq!(
            pause_layout_for_family(GameFamily::Swbf2),
            Err(GameFamilyError::not_available(GameFamily::Swbf2))
        );

        assert!(SpawnScreenController::try_with_family(GameFamily::Swbf2, 768.0).is_err());
        assert!(ScoreboardController::try_with_family(GameFamily::Swbf2, 768.0).is_err());
        assert!(PauseMenu::try_with_family(GameFamily::Swbf2).is_err());
    }

    // ------------------------------------------------------------------------
    // Spawn policy tests (ports `tests/spawn-policy.test.js`)
    //
    // Checks URL query resolution for default map load (opens screen on
    // `DEFAULT_UNIT`), `?spawnscreen=1` forced screen, `?network=1` online
    // mode (ignoring stale `unit=` and `spawn=`), and offline `?side=cis&spawn=cp_3`
    // auto-spawn bypass.
    // ------------------------------------------------------------------------
    #[test]
    fn test_spawn_policy_defaults_and_query_overrides() {
        let mission = sample_mission();
        // Default empty query opens the spawn screen on `DEFAULT_UNIT` (Republic, team 2).
        let p = spawn_policy_from_query("", Some(&mission));
        assert!(p.screen);
        assert_eq!(p.class_name, DEFAULT_UNIT);
        assert_eq!(p.team, 2); // Republic is team 2 on Theed
        assert_eq!(p.side, "rep");
        assert!(p.auto.is_none());

        // `spawnscreen=1` forces the interactive spawn screen even when `unit=` is set.
        let p_forced =
            spawn_policy_from_query("?unit=cis_inf_battledroid&spawnscreen=1", Some(&mission));
        assert!(p_forced.screen, "spawnscreen=1 forces the screen open");
        assert!(p_forced.auto.is_none());

        // `network=1` ignores stale `unit=` and `spawn=` parameters from offline URLs.
        let p_net = spawn_policy_from_query(
            "?network=1&unit=cis_inf_battledroid&spawn=cp_3",
            Some(&mission),
        );
        assert!(p_net.screen, "network=1 ignores stale unit= and spawn=");
        assert_eq!(p_net.class_name, DEFAULT_UNIT);
        assert_eq!(p_net.post, None);

        // Offline `side=cis&spawn=cp_3` bypasses the screen and populates `AutoSpawnPick`.
        let p_auto = spawn_policy_from_query("?side=cis&spawn=cp_3", Some(&mission));
        assert!(!p_auto.screen);
        assert_eq!(p_auto.team, 1);
        assert_eq!(p_auto.class_name, "cis_inf_battledroid");
        assert_eq!(
            p_auto.auto,
            Some(AutoSpawnPick {
                team: 1,
                class_name: "cis_inf_battledroid".to_string(),
                post: Some("cp_3".to_string()),
            })
        );
    }

    // Verifies that `playable_classes` filters out each team's hero unit while
    // `team_of_class` still recognizes heroes, and tests `queue_human_command`
    // deduplication semantics.
    #[test]
    fn test_playable_classes_and_human_commands() {
        let mission = sample_mission();
        let t1_classes = playable_classes(mission.team(1));
        assert_eq!(
            t1_classes,
            vec![
                "cis_inf_battledroid".to_string(),
                "cis_inf_assault".to_string()
            ]
        );
        assert_eq!(side_of_class(Some(&mission), "cis_inf_battledroid"), "cis");
        assert_eq!(team_of_class(Some(&mission), "rep_inf_macewindu"), 2);

        assert_eq!(
            human_entry(&HumanCommand::Team(2)),
            Some(HumanCommand::Team(2))
        );
        assert_eq!(human_entry(&HumanCommand::Team(0)), None);

        let mut queue = Vec::new();
        assert!(queue_human_command(&mut queue, &HumanCommand::Team(1)));
        assert!(queue_human_command(
            &mut queue,
            &HumanCommand::Spawn {
                class_name: "rep_inf_clone_trooper".to_string(),
                post: Some("cp_2".to_string()),
            }
        ));
        // Repeated Team command deduplicates and moves to the back of the queue.
        assert!(queue_human_command(&mut queue, &HumanCommand::Team(2)));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[1], HumanCommand::Team(2));
    }

    // ------------------------------------------------------------------------
    // Spawn model, input, layout, view, and hit tests
    // (ports `tests/spawn-screen-model.test.js`, `input.test.js`, `hit.test.js`, `index.test.js`)
    // ------------------------------------------------------------------------
    // Verifies `SpawnModel` opening on the frontline command post (`cp_2`),
    // stepping classes, switching teams (`cp_3`), and committing a `SpawnRequest`.
    #[test]
    fn test_spawn_model_team_class_post_and_commit() {
        let port = TestSpawnPort::new();
        let mut model = SpawnModel::new();
        model.open(&port, OpenReason::Start);
        assert!(model.is_open());
        assert_eq!(model.team, 1);
        assert_eq!(model.class_index, 0);
        // Frontline post for team 1 is cp_2 (closest to enemy cp_3/cp_4).
        assert_eq!(model.post.as_deref(), Some("cp_2"));

        // Stepping to the next class updates `class_index` to slot 1.
        assert!(model.next_class(&port));
        assert_eq!(model.class_index, 1);

        // Switching to team 2 resets `class_index` to 0 and picks team 2's first eligible post (`cp_3`).
        assert!(model.switch_team(&port, TeamSwitchSource::Key, 0.0));
        assert_eq!(model.team, 2);
        assert_eq!(model.post.as_deref(), Some("cp_3"));

        // Committing transitions `model.state` to `Committed` and returns the `SpawnRequest`.
        let Some(req) = model.commit(&port) else {
            panic!("commit on valid spawn selection must return Some(SpawnRequest)");
        };
        assert_eq!(model.state, SpawnState::Committed);
        assert_eq!(req.team, 2);
        assert_eq!(req.class_name, "cis_class_0");
        assert_eq!(req.post, "cp_3");
    }

    // Verifies two subtle 2004 `ifs_spawndisplay` input rules:
    // 1. Holding `Space` before `open()` survives `input.reset()` and triggers
    //    an immediate spawn on the first `update()` frame.
    // 2. Holding `Tab` repeats command post cycling every `TAB_REPEAT` (0.2s).
    #[test]
    fn test_spawn_input_space_held_before_open_and_tab_repeat() {
        let port = TestSpawnPort::new();
        let mut model = SpawnModel::new();
        let mut input = SpawnInput::new(768.0);

        // Space held before the screen opens persists across reset() and spawns on the first frame!
        input.key(&model, "Space", true, false, false, |_, _| None);
        model.open(&port, OpenReason::Death);
        input.reset(768.0);
        assert!(input.held.has_code("Space"));
        let act = input.update(0.016, &mut model, &port, 0.0, |_, _, _| None);
        assert_eq!(act, SpawnInputAction::Spawn);

        // Tab repeat every 0.2s cycles command posts.
        input.key(&model, "Space", false, false, true, |_, _| None);
        // First Tab press lands on post_cycle[1] == cp_2, advancing counter to cp_1; second step advances to cp_1.
        input.key(&model, "Tab", true, false, true, |_, _| None);
        input.update(0.016, &mut model, &port, 0.0, |_, _, _| None);
        input.update(0.25, &mut model, &port, 0.0, |_, _, _| None);
        assert_eq!(model.post.as_deref(), Some("cp_1"));
    }

    // Verifies perspective-projected quad hit-testing (`ClassicSpawnView::hit`)
    // against both the 45-degree X-tipped `SPAWN` button and the 35-degree
    // Y-tilted soldier class slots.
    #[test]
    fn test_spawn_classic_layout_and_hit_testing() {
        let mut port = TestSpawnPort::new();
        let mut surface = OwnedUiSurface::new(1024, 768);
        // Open the controller at 4:3 (`768.0`) and run a full draw pass into `OwnedUiSurface`
        // so `ClassicSpawnView` caches the layout and resolves surface assets.
        let mut ctrl = SpawnScreenController::new(768.0);
        ctrl.open(&mut port, OpenReason::Start, 768.0);
        ctrl.view.draw(&mut surface, &ctrl.model, &port);

        let fh = surface.font_height(FONT_TINY);
        let layout = classic_layout(768.0, fh);
        assert_eq!(layout.slots.len(), SLOTS);

        // Hit-test the SPAWN button center.
        let spawn_hit = ctrl.view.hit(
            layout.spawn.x,
            layout.spawn.y,
            768.0,
            fh,
            &ctrl.model,
            &port,
        );
        assert_eq!(spawn_hit, Some(SpawnHitTarget::Spawn));

        // Hit-test the second class slot (index 1).
        let slot1 = layout.slots[1];
        let slot_hit = ctrl
            .view
            .hit(slot1.x, slot1.y, 768.0, fh, &ctrl.model, &port);
        assert_eq!(slot_hit, Some(SpawnHitTarget::Class(1)));
    }

    // ------------------------------------------------------------------------
    // Scoreboard and battle stats tests
    // (ports `tests/scoreboard-model.test.js`, `scoreboard-input.test.js`,
    //  `scoreboard-port.test.js`, `scoreboard-index.test.js`, `battle-stats.test.js`)
    // ------------------------------------------------------------------------
    /// Builds a 3-character scoreboard fixture (`Alpha` local human on Team 1,
    /// `Droid-1` AI bot on Team 2, `Bravo` remote human on Team 1) with headshots,
    /// vehicles destroyed, living streak, nemesis/bait counts, and network link kinds.
    fn sample_scoreboard_port() -> WorldScoreboardPort {
        let mut port = WorldScoreboardPort::new();
        port.network = true;
        // Slot 0 (`Alpha`): local human player on Team 1 with direct IPv4 connection.
        let mut c0 = CharacterStatsEntry {
            slot: 0,
            team: 1,
            name: "Alpha".to_string(),
            kills: 12,
            team_kills: 1,
            deaths: 3,
            points: 8,
            human: true,
            local: true,
            ping: Some(24),
            link_kind: "direct4".to_string(),
            record: CharacterRecord::default(),
        };
        c0.record.slots[0].headshots = 5;
        c0.record.slots[0].vehicles_destroyed = 2;
        c0.record.slots[0].spree = 4;
        c0.record.slots[0].life_best = 95.8;
        c0.record.slots[0].escapes = 1;
        c0.record.slots[0].victims.insert(1, 6);
        SlotRecord::add_time(
            &mut c0.record.slots[0].classes,
            5,
            "rep_inf_trooper",
            Some("Clone Trooper"),
            65.0,
        );

        // Slot 1 (`Droid-1`): AI unit on Team 2 with 9 deaths (`BANTHA FODDER`) and 3 kills on Alpha (`NEMESIS`).
        let mut c1 = CharacterStatsEntry {
            slot: 1,
            team: 2,
            name: "Droid-1".to_string(),
            kills: 7,
            team_kills: 0,
            deaths: 9,
            points: 4,
            human: false,
            local: false,
            ping: None,
            link_kind: String::new(),
            record: CharacterRecord::default(),
        };
        c1.record.slots[1].deaths = 9;
        c1.record.slots[1].victims.insert(0, 3);

        // Slot 2 (`Bravo`): remote human player on Team 1 with 15 points and relay IPv6 link.
        let c2 = CharacterStatsEntry {
            slot: 2,
            team: 1,
            name: "Bravo".to_string(),
            kills: 10,
            team_kills: 0,
            deaths: 2,
            points: 15,
            human: true,
            local: false,
            ping: Some(42),
            link_kind: "relay6".to_string(),
            record: CharacterRecord::default(),
        };

        port.characters = vec![c0, c1, c2];
        port
    }

    // Verifies `clock()` formatting, the 10-row personal stats list (including
    // the network `CONNECTION` row and leading space on `LONGEST LIVING STREAK`),
    // and the 8 end-of-battle award rows (`TANK BUSTER`, `KILLING SPREE`, etc.).
    #[test]
    fn test_battle_stats_personal_and_awards() {
        let port = sample_scoreboard_port();
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.9), "1:05");
        assert_eq!(clock(754.0), "12:34");

        let personal = port.personal(0);
        // 9 standard personal stats + 1 network CONNECTION row for human player.
        assert_eq!(personal.len(), 10);
        assert_eq!(personal[0].value, "5"); // HEAD SHOTS
        assert_eq!(personal[1].value, "Clone Trooper 1:05"); // FAVORITE CLASS
        assert_eq!(personal[4].value, " 1:35"); // LONGEST LIVING STREAK (leading space)
        assert_eq!(personal[5].value, "1"); // NARROW ESCAPES
        assert_eq!(personal[6].value, "2"); // VEHICLES DESTROYED
        assert_eq!(personal[7].value, "Droid-1 (3)"); // NEMESIS
        assert_eq!(personal[8].value, "Droid-1 (6)"); // BAIT
        assert_eq!(personal[9].label, "CONNECTION");
        assert_eq!(personal[9].value, "DIRECT IPV4");

        // Verify all 8 end-of-match awards resolve to the expected winners and counts.
        let aw = port.awards();
        assert_eq!(aw.len(), 8);
        assert_eq!(aw[0].label, "TANK BUSTER");
        assert_eq!(aw[0].text, "Alpha (2)");
        assert_eq!(aw[1].label, "KILLING SPREE");
        assert_eq!(aw[1].text, "Alpha (4)");
        assert_eq!(aw[6].label, "BANTHA FODDER");
        assert_eq!(aw[6].text, "Droid-1 (9)");
        assert_eq!(aw[7].label, "DEAD EYE");
        assert_eq!(aw[7].text, "Alpha (5)");
    }

    // Verifies `ScoreboardModel` row sorting (accounting for `-2 * team_kills`),
    // initial focus on the local human player, sorting by `Points`, flipping sides,
    // navigating between `Table` / `Details` / `Awards`, and the 30s idle auto-close.
    #[test]
    fn test_scoreboard_model_sort_flip_pages_and_idle_quit() {
        let port = sample_scoreboard_port();
        let mut model = ScoreboardModel::new();
        assert!(model.open(BoardState::End, &port));
        assert!(model.is_open());
        // Alpha has shown_kills = 12 - 2*1 = 10; Bravo has 10 kills and fewer deaths (2 < 3), so Bravo sorts first!
        assert_eq!(model.lists[0][0].name, "Bravo");
        assert_eq!(model.lists[0][1].name, "Alpha");
        // Selection opens on the local human player (Alpha, index 1 on side 1).
        assert_eq!(model.side, 1);
        assert_eq!(model.selected, 1);

        // Sort by POINTS: Bravo (15) before Alpha (8).
        assert!(model.sort_by(SortMode::Points, &port));
        assert_eq!(model.mode, SortMode::Points);
        assert!(!model.reverse);

        // Flip to side 2 (Droid-1).
        assert!(model.flip());
        assert_eq!(model.side, 2);
        assert_eq!(model.selected, 0);

        // Show DETAILS then back.
        assert!(model.show_details());
        assert_eq!(model.page, BoardPage::Details);
        assert_eq!(model.subject.as_ref().map(|s| s.name.as_str()), Some("Droid-1"));
        assert!(model.back(&port));
        assert_eq!(model.page, BoardPage::Table);

        // Show AWARDS on END scoreboard.
        assert!(model.show_awards());
        assert_eq!(model.page, BoardPage::Awards);
        assert!(model.back(&port));

        // 30s idle timer (clamped to 0.5s per update call; 60 steps reach 0.0, 61st crosses below 0.0).
        for _ in 0..60 {
            assert!(!model.update(0.5));
        }
        assert!(model.update(0.5));
    }

    // Verifies `TabHold` tap-to-latch (`< 0.5s` stays open until the next `Tab`
    // release) and double-clicking a player row within `0.4s` to open `Details`.
    #[test]
    fn test_scoreboard_tab_hold_and_double_click() {
        let mut tab = TabHold::default();
        tab.start(0.0);
        // Released at 0.2s (< 0.5s HOLD): becomes tapped, stays open.
        assert!(!tab.step(false, 0.2));
        assert!(tab.tapped());
        assert!(!tab.step(false, 0.8));
        // Pressed again at 1.0s, then released at 1.1s: closes!
        assert!(!tab.step(true, 1.0));
        assert!(!tab.tapped());
        assert!(tab.step(false, 1.1));

        let port = sample_scoreboard_port();
        let surface = OwnedUiSurface::new(1024, 768);
        let mut ctrl = ScoreboardController::new(768.0);
        assert!(ctrl.peek(&port, &surface, false, false));
        let layout = team_stats_layout(768.0);
        let row0_x = layout.sides[0].left + 20.0;
        let row0_y = layout.top;
        // Double-click row 0 within 0.4s opens DETAILS.
        ctrl.input.click(row0_x, row0_y, |x, y| {
            ctrl.view.hit(x, y, &ctrl.model, &port, &surface)
        });
        let mut port_mut = port.clone();
        ctrl.update(0.1, &mut port_mut, &surface, false, false);
        ctrl.input.click(row0_x, row0_y, |x, y| {
            ctrl.view.hit(x, y, &ctrl.model, &port_mut, &surface)
        });
        ctrl.update(0.1, &mut port_mut, &surface, false, false);
        assert_eq!(ctrl.model.page, BoardPage::Details);
    }

    // ------------------------------------------------------------------------
    // Pause menu tests (ports `tests/pause-menu.test.js`)
    // ------------------------------------------------------------------------
    // Verifies `with_settings` URL query rewriting (`tier=`, `heroes=`, `size=`),
    // `Escape` keydown/keyup open and resume-on-release, the 250ms pointer-unlock
    // grace window (`LOCK_GRACE`), and `confirm-suicide` `YES`/`NO` dialog navigation.
    #[test]
    fn test_pause_menu_escape_confirm_popups_and_with_settings() {
        let surface = OwnedUiSurface::new(1024, 768);
        let mut settings = BTreeMap::new();
        settings.insert("aiDifficulty".to_string(), "hard".to_string());
        settings.insert("aiHeroes".to_string(), "true".to_string());
        settings.insert("battleSize".to_string(), "250".to_string());

        let rewritten = with_settings(
            "http://localhost/?map=nab2&tier=easy&heroes=0&size=150&noreload#top",
            &settings,
        );
        assert_eq!(
            rewritten,
            "http://localhost/?map=nab2&tier=hard&heroes=1&size=250&noreload#top"
        );

        let mut menu = PauseMenu::new();
        menu.enable_respawn(Some(PauseRespawnConfig {
            label: None,
            prompt: None,
            available: true,
        }));

        // Escape opens the menu on keydown.
        assert!(menu.handle_key("Escape", true, false, &surface, &settings));
        assert!(menu.is_open);
        assert_eq!(menu.top(), Some(&PauseScreenState::Main));

        // Second Escape keydown arms resume_on_release, and keyup closes with resume = true.
        assert!(menu.handle_key("Escape", true, false, &surface, &settings));
        assert!(menu.is_open);
        assert!(menu.handle_key("Escape", false, false, &surface, &settings));
        assert!(!menu.is_open);
        assert_eq!(menu.close_events, vec![true]);

        // Pointer unlock grace: Escape within 250ms of open("pointer") does not close.
        menu.open("pointer", 768.0);
        menu.update(0.1);
        menu.handle_key("Escape", true, false, &surface, &settings);
        menu.handle_key("Escape", false, false, &surface, &settings);
        assert!(
            menu.is_open,
            "Escape within 250ms of pointer unlock is ignored"
        );

        // Navigate down to RESPAWN (index 2) and press Enter -> opens confirm-suicide with NO (1) focused.
        menu.handle_key("ArrowDown", true, false, &surface, &settings);
        menu.handle_key("ArrowDown", true, false, &surface, &settings);
        assert_eq!(menu.focused_row, 2);
        menu.handle_key("Enter", true, false, &surface, &settings);
        assert_eq!(menu.top().map(|s| s.id()), Some("confirm-suicide"));
        // Pressing Enter immediately chooses NO (focused_button == 1), returning to Main without respawning.
        menu.handle_key("Enter", true, false, &surface, &settings);
        assert_eq!(menu.top(), Some(&PauseScreenState::Main));
        assert_eq!(menu.respawn_triggered, 0);

        // Open confirm-suicide again, flip to YES with ArrowLeft, and press Enter -> respawns and closes.
        menu.handle_key("Enter", true, false, &surface, &settings);
        menu.handle_key("ArrowLeft", true, false, &surface, &settings);
        menu.handle_key("Enter", true, false, &surface, &settings);
        assert_eq!(menu.respawn_triggered, 1);
        assert!(!menu.is_open);
    }

    // ------------------------------------------------------------------------
    // HUD input and chat tests (ports `tests/hud-input.test.js`)
    // ------------------------------------------------------------------------
    // Verifies that `HudInputPublisher` retains the latest state values across
    // 10,000 rapid updates without overflowing the shared-memory buffer, and
    // flushes them cleanly into the target slice on `publish_to_slice`.
    #[test]
    fn test_hud_input_retained_state_and_immediate_events() {
        // Allocate a simulated shared-memory Float64 buffer covering all HUD sections
        // through the 128-entry `RINGS` table.
        let mut input = HudInputPublisher::new();
        let mut memory = vec![0.0; RINGS + 128 * R::WORDS];

        for i in 0..10_000 {
            let fill = i as f64 / 10_000.0;
            input.set_second_bar(1.0, fill, None);
            input.set_hull(1.0, fill, None);
            input.set_supply(true, false, None);
            input.set_stats(i, 0, 0, None);
            input.event(C::MESSAGE, 1.0, 0.0, 0.0, 0.0, None);
            input.event(C::KILL, 1.0, 2.0, 0.0, 0.0, None);
            input.event(C::HIT, 1.0, 0.0, 0.0, 0.0, None);
        }

        assert_eq!(input.event_count, 30_000);
        assert_eq!(memory[H::STATES], 0.0, "no frame has published yet");
        assert!((input.f[STATES + (C::SECOND as usize) * S::WORDS + 1] - 0.9999).abs() < 1e-9);

        // Flushing to the shared slice writes all retained states and resets the dirty mask.
        input.publish_to_slice(&mut memory);
        assert_eq!(memory[STATES + (C::STATS as usize) * S::WORDS], 9999.0);
        assert!((memory[STATES + (C::SECOND as usize) * S::WORDS + 1] - 0.9999).abs() < 1e-9);
        assert_eq!(input.f[H::STATES], 0.0);
    }

    // Verifies `HudInputPublisher` unit-row hysteresis (requiring 3 consecutive
    // frames of unit-set change before rebuilding rows and bumping `GENERATION`),
    // `MAX_UNITS` / `MAX_POSTS` clamping, and the 4096-entry asset table cap.
    #[test]
    fn test_hud_input_unit_churn_and_asset_caps() {
        let mut input = HudInputPublisher::new();
        let def = HudDefinitionFacts {
            scale: 12.0,
            scan: 12.0,
            transmit: 12.0,
            ..Default::default()
        };
        let mut teams = vec![HudTeamFact::default(); MAX_TEAMS];
        teams[2] = HudTeamFact {
            relation: -1.0,
            locals: true,
            tickets: 9.0,
            living: 7.0,
        };
        input.set_teams(&teams);
        assert_eq!(input.f[TEAMS + 2 * T::WORDS + T::RELATION], -1.0);
        assert_eq!(input.f[TEAMS + 2 * T::WORDS + T::LOCALS], 1.0);
        assert_eq!(input.f[TEAMS + 2 * T::WORDS + T::TICKETS], 9.0);
        assert_eq!(input.f[TEAMS + 2 * T::WORDS + T::LIVING], 7.0);

        // Seed an initial 1-unit frame so `GENERATION` is initialized.
        let old_unit = HudUnitFact {
            entity_key: 1,
            def: def.clone(),
            ..Default::default()
        };
        input.set_units(
            true,
            std::slice::from_ref(&old_unit),
            Some(1),
            Some(1),
            1,
            false,
        );
        let first_gen = input.f[UNITS + U::GENERATION];

        // Feed an oversized unit list (`MAX_UNITS + 100`) for 3 frames to cross the
        // 3-frame hysteresis threshold and verify clamping to `MAX_UNITS`.
        let big_list: Vec<HudUnitFact> = (0..(MAX_UNITS + 100))
            .map(|i| HudUnitFact {
                entity_key: (i + 100) as u64,
                def: def.clone(),
                ..Default::default()
            })
            .collect();
        for _ in 0..3 {
            input.set_units(true, &big_list, None, None, 1, false);
        }
        assert_eq!(input.row_count, MAX_UNITS);

        // Returning to the 1-unit list for 3 consecutive frames shrinks `row_count`
        // back to 1 and advances the unit table `GENERATION` counter.
        for _ in 0..3 {
            input.set_units(
                true,
                std::slice::from_ref(&old_unit),
                Some(1),
                Some(1),
                1,
                false,
            );
        }
        assert_eq!(input.row_count, 1);
        assert_ne!(input.f[UNITS + U::GENERATION], first_gen);

        // Feed an oversized command-post list (`MAX_POSTS + 30`) and verify clamping to `MAX_POSTS`.
        let big_posts: Vec<HudPostFact> = (0..(MAX_POSTS + 30))
            .map(|_| HudPostFact {
                counts: (0..(MAX_TEAMS + 5)).map(|t| (t as i32, 1, 0.0)).collect(),
                ..Default::default()
            })
            .collect();
        input.set_posts(&big_posts, None);
        assert_eq!(input.f[H::POSTS] as usize, MAX_POSTS);

        // Attempting to register 5,000 dynamic HUD assets caps the table at `MAX_ASSETS` (4096)
        // and returns slot `0` for overflow entries.
        for i in 0..5000 {
            input.asset(&format!("extra-{i}"));
        }
        assert_eq!(input.assets.len(), 4096);
        assert_eq!(input.asset("extra-4999"), 0);
    }

    // Verifies that `set_player` only records health transition samples when
    // the player health ratio actually changes, and tests `route_battle_key`
    // opening team chat on `KeyY`, swallowing `Tab`, and sanitizing control characters.
    #[test]
    fn test_hud_input_transition_feedback_and_chat_routing() {
        let mut input = HudInputPublisher::new();
        input.observed_samples.clear();
        input.set_player(0.35, None);
        input.set_player(1.0, None);
        input.set_player(0.35, None);
        // Repeating the identical player health (`0.35`) across 1,000 frames must
        // not emit duplicate `C::HEALTH` transition samples.
        for i in 0..1000 {
            input.set_player(0.35, None);
            input.set_second_bar(1.0, i as f64 / 1000.0, None);
        }
        let health_samples: Vec<(u32, f64)> = input
            .observed_samples
            .iter()
            .map(|&(k, a, _, _, _)| (k, a))
            .collect();
        assert_eq!(
            health_samples,
            vec![(C::HEALTH, 0.35), (C::HEALTH, 1.0), (C::HEALTH, 0.35)]
        );

        let mut chat = ChatController::new("Clone-1");
        assert_eq!(
            route_battle_key("KeyY", false, false, &mut chat, false, false, false, 1000.0),
            BattleKeyAction::ChatHandled
        );
        assert!(chat.is_open());
        // While chat is open, Tab is swallowed by chat and does not open the scoreboard.
        assert_eq!(
            route_battle_key("Tab", false, false, &mut chat, false, false, false, 1000.0),
            BattleKeyAction::ChatHandled
        );
        // ASCII control characters (`\x07`) in the draft are replaced with spaces on submit.
        chat.draft = "Roger\x07 that".to_string();
        assert_eq!(
            route_battle_key("Enter", false, false, &mut chat, false, false, false, 1050.0),
            BattleKeyAction::ChatHandled
        );
        assert!(!chat.is_open());
        assert_eq!(chat.log.messages.len(), 1);
        assert_eq!(
            chat.log.messages[0].formatted_line(),
            "[TEAM] Clone-1: Roger  that"
        );
    }
}
