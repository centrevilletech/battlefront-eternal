// Battlefront Eternal in-battle UI crate (`bfe-battle-ui`).
//
// Ports the Classic spawn screen (`src/spawn-screen/`), Classic scoreboard and
// battle statistics (`src/scoreboard/`, `src/battle-stats.js`), in-battle pause menu
// (`src/pause/`), and HUD input/chat publication (`src/hud-input.js`, `src/chat.js`).

pub mod chat;
pub mod hud_input;
pub mod pause;
pub mod scoreboard;
pub mod shapes;
pub mod spawn;
pub mod surface;

pub use hud_input::{
    relation_code, route_battle_key, BattleKeyAction, ChatChannel, ChatController, ChatLog,
    ChatMessage, HudAssetSlot, HudDefinitionFacts, HudInputPublisher, HudPostFact, HudTeamFact,
    HudUnitFact, HudWeaponFact, CHAT_LIMIT, HUD_ART_NAMES, MAX_ASSETS, MAX_RINGS, WORDS,
};
pub use pause::{
    pause_layout_for_family, row_bars, rows_shift, still_command, with_settings, ConfirmKind,
    LeavingRecord, PauseHitTarget, PauseHotspot, PauseLayoutConfig, PauseMenu, PauseRespawnConfig,
    PauseRestartMode, PauseRowId, PauseScreenState, PauseSettingsLookup, RowBarSpec, StillCommand,
    LAYOUT as PAUSE_LAYOUT, LIST_COUNT, LOCK_GRACE, PROMPT_DELAY,
};
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
    // ------------------------------------------------------------------------
    struct TestSpawnPort {
        human: HumanSpawnFacts,
        teams: [SpawnTeamFacts; 2],
        rosters: [Vec<SpawnClassEntry>; 2],
        posts: [Vec<SpawnPostEntry>; 2],
        requested: Vec<SpawnRequest>,
        suicides: usize,
        cancels: usize,
        shots: Vec<CameraShot>,
        active_shot: Option<CameraShot>,
    }

    impl TestSpawnPort {
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

    impl SpawnPort for TestSpawnPort {
        fn human_state(&self) -> HumanSpawnFacts {
            self.human.clone()
        }
        fn team_facts(&self, team: u8) -> Option<SpawnTeamFacts> {
            match team {
                1 => Some(self.teams[0].clone()),
                2 => Some(self.teams[1].clone()),
                _ => None,
            }
        }
        fn roster(&self, team: u8) -> Vec<SpawnClassEntry> {
            match team {
                2 => self.rosters[1].clone(),
                _ => self.rosters[0].clone(),
            }
        }
        fn posts(&self, team: u8) -> Vec<SpawnPostEntry> {
            match team {
                2 => self.posts[1].clone(),
                _ => self.posts[0].clone(),
            }
        }
        fn request(&mut self, choice: &SpawnRequest) {
            self.requested.push(choice.clone());
        }
        fn suicide(&mut self) {
            self.suicides += 1;
        }
        fn cancel(&mut self) {
            self.cancels += 1;
        }
        fn camera_shots(&self) -> Vec<CameraShot> {
            self.shots.clone()
        }
        fn set_view_shot(&mut self, shot: Option<CameraShot>) {
            self.active_shot = shot;
        }
    }

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
    // ------------------------------------------------------------------------
    #[test]
    fn test_game_family_selectors_swbf1_and_swbf2_error() {
        assert_eq!(GameFamily::parse("swbf1"), Some(GameFamily::Swbf1));
        assert_eq!(GameFamily::parse("swbf2/hot1"), Some(GameFamily::Swbf2));
        assert!(GameFamily::Swbf1.require_available().is_ok());

        let err = GameFamily::Swbf2.require_available().unwrap_err();
        assert_eq!(err.family, GameFamily::Swbf2);
        assert_eq!(err.to_string(), "game family swbf2 is not available");

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
    // ------------------------------------------------------------------------
    #[test]
    fn test_spawn_policy_defaults_and_query_overrides() {
        let mission = sample_mission();
        let p = spawn_policy_from_query("", Some(&mission));
        assert!(p.screen);
        assert_eq!(p.class_name, DEFAULT_UNIT);
        assert_eq!(p.team, 2); // Republic is team 2 on Theed
        assert_eq!(p.side, "rep");
        assert!(p.auto.is_none());

        let p_forced =
            spawn_policy_from_query("?unit=cis_inf_battledroid&spawnscreen=1", Some(&mission));
        assert!(p_forced.screen, "spawnscreen=1 forces the screen open");
        assert!(p_forced.auto.is_none());

        let p_net = spawn_policy_from_query(
            "?network=1&unit=cis_inf_battledroid&spawn=cp_3",
            Some(&mission),
        );
        assert!(p_net.screen, "network=1 ignores stale unit= and spawn=");
        assert_eq!(p_net.class_name, DEFAULT_UNIT);
        assert_eq!(p_net.post, None);

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

        assert!(model.next_class(&port));
        assert_eq!(model.class_index, 1);

        assert!(model.switch_team(&port, TeamSwitchSource::Key, 0.0));
        assert_eq!(model.team, 2);
        assert_eq!(model.post.as_deref(), Some("cp_3"));

        let req = model.commit(&port).unwrap();
        assert_eq!(model.state, SpawnState::Committed);
        assert_eq!(req.team, 2);
        assert_eq!(req.class_name, "cis_class_0");
        assert_eq!(req.post, "cp_3");
    }

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

    #[test]
    fn test_spawn_classic_layout_and_hit_testing() {
        let mut port = TestSpawnPort::new();
        let mut surface = OwnedUiSurface::new(1024, 768);
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
    fn sample_scoreboard_port() -> WorldScoreboardPort {
        let mut port = WorldScoreboardPort::new();
        port.network = true;
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
        assert_eq!(model.subject.as_ref().unwrap().name, "Droid-1");
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
    #[test]
    fn test_hud_input_retained_state_and_immediate_events() {
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

        input.publish_to_slice(&mut memory);
        assert_eq!(memory[STATES + (C::STATS as usize) * S::WORDS], 9999.0);
        assert!((memory[STATES + (C::SECOND as usize) * S::WORDS + 1] - 0.9999).abs() < 1e-9);
        assert_eq!(input.f[H::STATES], 0.0);
    }

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

        let big_posts: Vec<HudPostFact> = (0..(MAX_POSTS + 30))
            .map(|_| HudPostFact {
                counts: (0..(MAX_TEAMS + 5)).map(|t| (t as i32, 1, 0.0)).collect(),
                ..Default::default()
            })
            .collect();
        input.set_posts(&big_posts, None);
        assert_eq!(input.f[H::POSTS] as usize, MAX_POSTS);

        for i in 0..5000 {
            input.asset(&format!("extra-{i}"));
        }
        assert_eq!(input.assets.len(), 4096);
        assert_eq!(input.asset("extra-4999"), 0);
    }

    #[test]
    fn test_hud_input_transition_feedback_and_chat_routing() {
        let mut input = HudInputPublisher::new();
        input.observed_samples.clear();
        input.set_player(0.35, None);
        input.set_player(1.0, None);
        input.set_player(0.35, None);
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
