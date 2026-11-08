//! Unified battle controller for Instant Action (`WorldThread`) and Online (`online::client::Client`)
//! battles, integrating `bfe-battle-ui` (`SpawnScreenController`, `ScoreboardController`,
//! `PauseMenu`, `ChatController`) over the native 3D `Scene` and `HudModel`.

use crate::{
    app::settings::AppSettingsStore,
    audio::SharedAudioRouter,
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    host::{World, STEP},
    input::{Input, ViewActions},
    local::{
        options::Options as LocalOptions,
        publication::Publication,
        thread::{FallbackMissionConfig, InitialPostConfig, WorldMessage, WorldThread},
    },
    online::{
        client::Client as OnlineClient,
        hud::{HudClassMeta, HudPostMeta},
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
pub enum BattleTransition {
    /// Advance to the next mission in the Instant Action playlist (or loop/return).
    NextPlaylistBattle {
        request: BattleRequest,
        next_index: usize,
    },
    /// Restart the current Instant Action mission from tick 0.
    RestartMission {
        request: BattleRequest,
        playlist_index: usize,
    },
    /// Quit from an Instant Action battle back to the front-end Main Menu.
    QuitToMainMenu,
    /// Leave an Online battle (or disconnect) back to the Multiplayer Server Browser.
    LeaveOnline { reason: Option<String> },
    /// Exit the entire application to the desktop (`EXIT TO WINDOWS`).
    ExitToWindows,
}

/// Spawn screen port backed by the live battle HUD metadata and authority.
pub struct LiveSpawnPort {
    pub human: HumanSpawnFacts,
    pub teams: [SpawnTeamFacts; 2],
    pub rosters: [Vec<SpawnClassEntry>; 2],
    pub class_registry_ids: [Vec<usize>; 2],
    pub posts: Vec<SpawnPostEntry>,
    pub post_ids: Vec<usize>,
    pub pending_spawn: Option<(u8, usize, usize, u16, u16)>,
    pub pending_suicide: bool,
}

impl LiveSpawnPort {
    pub fn from_hud_meta(
        class_meta: &[HudClassMeta],
        post_meta: &[HudPostMeta],
        strings: &std::collections::HashMap<String, String>,
        era: &str,
    ) -> Self {
        let is_gcw = era.eq_ignore_ascii_case("gcw");
        let (side1, name1, logo1, side2, name2, logo2) = if is_gcw {
            (
                "all",
                "REBEL ALLIANCE",
                "com_icon_alliance",
                "imp",
                "GALACTIC EMPIRE",
                "com_icon_imperial",
            )
        } else {
            (
                "rep",
                "REPUBLIC",
                "com_icon_republic",
                "cis",
                "CIS",
                "com_icon_cis",
            )
        };

        let objective_text = strings
            .get("ifs.spawndisplay.spawnprompt")
            .map(|s| s.to_ascii_uppercase())
            .unwrap_or_else(|| {
                "ELIMINATE ENEMY REINFORCEMENTS OR CAPTURE ALL COMMAND POSTS".to_string()
            });

        let build_roster = |target_side: &str| -> (Vec<SpawnClassEntry>, Vec<usize>) {
            let mut entries = Vec::new();
            let mut reg_ids = Vec::new();
            for c in class_meta {
                if c.side.eq_ignore_ascii_case(target_side) && entries.len() < 5 {
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

        let (roster1, ids1) = build_roster(side1);
        let (roster2, ids2) = build_roster(side2);

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

    pub fn sync_from_hud(
        &mut self,
        teams: &[[f64; 4]; 8],
        post_meta: &[HudPostMeta],
        own_alive: bool,
    ) {
        self.human.phase = if own_alive {
            "alive".to_string()
        } else {
            "awaiting".to_string()
        };
        if teams[1][0] > 0.0 || teams[2][0] > 0.0 {
            self.teams[0].reinforcements = teams[1][0].round() as i32;
            self.teams[1].reinforcements = teams[2][0].round() as i32;
        }
        for (i, pm) in post_meta.iter().enumerate() {
            if let Some(p) = self.posts.get_mut(i) {
                if pm.team == 1 || pm.team == 2 {
                    p.owner = pm.team;
                }
            }
        }
    }
}

impl SpawnPort for LiveSpawnPort {
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
            1 => self.rosters[0].clone(),
            2 => self.rosters[1].clone(),
            _ => Vec::new(),
        }
    }

    fn posts(&self, team: u8) -> Vec<SpawnPostEntry> {
        self.posts
            .iter()
            .map(|p| {
                let mut e = p.clone();
                e.eligible = p.owner == team || p.owner == 0;
                e
            })
            .collect()
    }

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
        self.pending_spawn = Some((team, class_idx, post_idx, class_reg_id, post_reg_id));
    }

    fn suicide(&mut self) {
        self.pending_suicide = true;
    }

    fn cancel(&mut self) {}

    fn camera_shots(&self) -> Vec<CameraShot> {
        Vec::new()
    }

    fn set_view_shot(&mut self, _shot: Option<CameraShot>) {}
}

/// Scoreboard port wrapping [`WorldScoreboardPort`] and tracking `DONE` / restart triggers.
pub struct LiveScoreboardPort {
    pub inner: WorldScoreboardPort,
    pub done_triggered: bool,
}

impl LiveScoreboardPort {
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
        }
    }

    pub fn sync_from_state(
        &mut self,
        profile_name: &str,
        teams: &[[f64; 4]; 8],
        game_over: bool,
        receiver: &Receiver,
        fallback: Option<&crate::local::thread::FallbackSnapshot>,
    ) {
        self.inner.human_team = receiver.own.team.clamp(1, 2) as i32;
        self.inner.state = MatchStateFlags {
            over: game_over,
            game_over,
            finished: game_over,
        };
        if let Some(t1) = &mut self.inner.teams[0] {
            t1.reinforcements = teams[1][0].round() as i32;
        }
        if let Some(t2) = &mut self.inner.teams[1] {
            t2.reinforcements = teams[2][0].round() as i32;
        }

        self.inner.characters.clear();
        if let Some(snap) = fallback {
            for p in &snap.players[..snap.player_count] {
                let name = if p.is_human {
                    profile_name.to_string()
                } else if p.team == 1 {
                    format!("Clone Trooper {}", p.slot + 1)
                } else {
                    format!("Battle Droid {}", p.slot + 1)
                };
                let mut rec = CharacterRecord::default();
                rec.slots[0].deaths = p.deaths;
                rec.slots[0].life_kills = p.kills;
                rec.slots[0].life_kills_best = p.kills;
                self.inner.characters.push(CharacterStatsEntry {
                    slot: p.slot,
                    team: p.team as i32,
                    name,
                    kills: p.kills,
                    team_kills: 0,
                    deaths: p.deaths,
                    points: p.kills * 2 + p.captures * 5,
                    human: p.is_human,
                    local: p.is_human,
                    ping: if p.is_human { Some(0) } else { None },
                    link_kind: if p.is_human {
                        "local".to_string()
                    } else {
                        String::new()
                    },
                    record: rec,
                });
            }
        } else {
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
}

impl ScoreboardPort for LiveScoreboardPort {
    fn family(&self) -> GameFamily {
        self.inner.family()
    }
    fn network(&self) -> bool {
        self.inner.network()
    }
    fn players(&self) -> Option<Vec<PlayerListRow>> {
        self.inner.players()
    }
    fn roster(&self) -> Vec<ScoreRow> {
        self.inner.roster()
    }
    fn teams(&self) -> [Option<TeamHeader>; 2] {
        self.inner.teams()
    }
    fn personal(&self, slot: usize) -> Vec<PersonalStatRow> {
        self.inner.personal(slot)
    }
    fn awards(&self) -> Vec<AwardRow> {
        self.inner.awards()
    }
    fn human_team(&self) -> i32 {
        self.inner.human_team()
    }
    fn state(&self) -> MatchStateFlags {
        self.inner.state()
    }
    fn can_restart(&self) -> bool {
        true
    }
    fn restart(&mut self) -> bool {
        self.done_triggered = true;
        true
    }
}

/// Underlying battle simulation/network authority.
#[allow(clippy::large_enum_variant)]
pub enum BattleAuthority {
    /// Local Instant Action battle stepped at 60 Hz on [`WorldThread`].
    InstantAction {
        request: BattleRequest,
        playlist_index: usize,
        map_id: String,
        era: String,
        world: WorldThread,
        _prepared: Option<crate::local::launch::Prepared>,
        receiver: Box<Receiver>,
        sender: Box<Sender>,
        replay: Box<Replay>,
        camera: Box<MapCamera>,
        now_ms: f64,
    },
    /// Online multiplayer battle connected to a remote or LAN `bfeserver`.
    Online {
        launch: MultiplayerLaunch,
        map_id: String,
        era: String,
        client: Box<OnlineClient>,
    },
}

/// Unified controller for an active Instant Action or Online battle.
pub struct BattleController {
    pub authority: BattleAuthority,
    pub scene: Box<Scene>,
    pub input: Input,
    pub input_tick: u32,
    pub input_accumulator: f64,
    pub spawn_port: LiveSpawnPort,
    pub spawn_ctrl: SpawnScreenController,
    pub scoreboard_port: LiveScoreboardPort,
    pub scoreboard_ctrl: ScoreboardController,
    pub pause_menu: PauseMenu,
    pub chat: ChatController,
    pub overlay_batch: Batch,
    pub size: [i32; 2],
    pub height: f64,
    pub aspect: f64,
    pub cursor_px: [f64; 2],
    pub frames: u64,
    pub steady_frames: u64,
    pub steady_allocs_after_warmup: u64,
    pub elapsed_s: f64,
    pub was_alive: bool,
    pub first_life_spawned: bool,
    pub overlay_was_active: bool,
}

impl BattleController {
    /// Loads and starts an Instant Action battle for `request.playlist[playlist_index]`.
    pub fn start_instant_action(
        content_dir: &Path,
        request: BattleRequest,
        playlist_index: usize,
        size: [i32; 2],
        settings: &AppSettingsStore,
        audio_router: &SharedAudioRouter,
        mut make_gl: impl FnMut() -> glow::Context,
    ) -> Result<Self, String> {
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

        let query = format!("map={map_id}&era={era}&seed=1");
        let (mut scene, camera) = Scene::load_online(content_dir, &query, size, &mut make_gl)?;

        let replicas = scene.online.as_mut().ok_or("missing online replicas")?;
        replicas.hud.auto_build = false;
        audio_router.set_shell(None);
        audio_router.set_live(replicas.sound.output.take());

        let receiver = Box::new(Receiver::new(replicas.descriptor.clone()));
        let replay = Box::new(
            replicas
                .replay
                .take()
                .ok_or("native prediction resources already admitted")?,
        );
        let mut sender = Box::new(Sender::new(&replicas.turns));
        sender.auto_spawn = false;

        let spawn_port = LiveSpawnPort::from_hud_meta(
            &replicas.hud.class_meta,
            &replicas.hud.post_meta,
            &replicas.hud.strings,
            &era,
        );

        let fallback_posts: Vec<InitialPostConfig> = replicas
            .hud
            .post_meta
            .iter()
            .enumerate()
            .map(|(idx, pm)| InitialPostConfig {
                id: pm.id,
                team: if pm.team == 1 || pm.team == 2 {
                    pm.team
                } else if idx % 2 == 0 {
                    1
                } else {
                    2
                },
                position: pm.position,
            })
            .collect();
        let tickets = settings
            .player
            .shell
            .battle_size
            .parse::<i32>()
            .unwrap_or(150)
            .clamp(25, 500);
        let fallback_config = FallbackMissionConfig {
            posts: fallback_posts,
            tickets: [tickets, tickets],
            team_classes: [
                spawn_port.class_registry_ids[0].clone(),
                spawn_port.class_registry_ids[1].clone(),
            ],
            seed: (playlist_index as u32) + 1,
        };

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
        let (world, prepared) =
            WorldThread::admit_instant_action(&local_options, fallback_config)?;

        Self::finish_init(
            BattleAuthority::InstantAction {
                request,
                playlist_index,
                map_id,
                era: era.clone(),
                world,
                _prepared: prepared,
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
    pub fn start_online(
        content_dir: &Path,
        launch: MultiplayerLaunch,
        server_address: Option<String>,
        size: [i32; 2],
        settings: &AppSettingsStore,
        audio_router: &SharedAudioRouter,
        mut make_gl: impl FnMut() -> glow::Context,
    ) -> Result<Self, String> {
        let raw_target = server_address
            .or_else(|| {
                if let (Some(addr), Some(port)) = (&launch.address, launch.port) {
                    if !addr.trim().is_empty() {
                        return Some(format!("{}:{port}", addr.trim()));
                    }
                }
                if let Some(addr) = &launch.address {
                    if !addr.trim().is_empty() {
                        return Some(addr.trim().to_string());
                    }
                }
                if launch.server_id.contains(':') && launch.server_id != "host:local" {
                    Some(launch.server_id.clone())
                } else {
                    std::env::var("BFE_SERVER_TARGET").ok()
                }
            })
            .ok_or_else(|| "No server address specified for online join".to_string())?;

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

        let scoreboard_port = LiveScoreboardPort::new(is_online, era);
        let scoreboard_ctrl =
            ScoreboardController::try_with_family(GameFamily::Swbf1, height).map_err(|e| e.to_string())?;

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
            chat,
            overlay_batch: Batch::new(),
            size,
            height,
            aspect,
            cursor_px: [size[0] as f64 * 0.5, size[1] as f64 * 0.5],
            frames: 0,
            steady_frames: 0,
            steady_allocs_after_warmup: 0,
            elapsed_s: 0.0,
            was_alive: false,
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

    /// Programmatically spawns the human player if the spawn screen is open.
    pub fn trigger_spawn(&mut self) {
        if self.spawn_ctrl.is_open() {
            self.spawn_ctrl.spawn(&mut self.spawn_port);
            self.flush_spawn_requests();
            self.spawn_ctrl.close(&mut self.spawn_port);
        }
    }

    /// Programmatically forces the match to finish so end-of-match scoreboard and
    /// playlist transitions can be exercised by `--script gate`.
    pub fn force_victory_for_gate(&mut self) {
        if let BattleAuthority::InstantAction {
            world,
            receiver,
            ..
        } = &mut self.authority
        {
            if let Some(mut snap) = world.latest_fallback.take() {
                snap.teams_tickets[1] = 0;
                snap.game_over = true;
                snap.winner = 1;
                snap.losing = false;
                world.latest_fallback = Some(snap);
            }
            if let Some(replicas) = &mut self.scene.online {
                replicas.hud.game_over = true;
            }
            self.scoreboard_port.sync_from_state(
                "Player 1",
                &[[0.0; 4]; 8],
                true,
                receiver,
                world.latest_fallback.as_deref(),
            );
        }
    }

    fn flush_spawn_requests(&mut self) {
        if let Some((team, class_idx, post_idx, class_reg_id, post_reg_id)) =
            self.spawn_port.pending_spawn.take()
        {
            self.first_life_spawned = true;
            match &mut self.authority {
                BattleAuthority::InstantAction { world, .. } => {
                    world.send(WorldMessage::Spawn {
                        team,
                        class_index: class_idx,
                        post_index: post_idx,
                    });
                }
                BattleAuthority::Online { client, .. } => {
                    client.sender.auto_spawn = true;
                    client.sender.spawned = false;
                    if client.receiver.own.team != 0 && client.receiver.own.team != team {
                        client.sender.request_team(&client.receiver, team);
                    }
                    let cid = if class_idx == 0 && client.receiver.own.class != 0 {
                        client.receiver.own.class
                    } else {
                        class_reg_id as usize
                    };
                    let pid = if class_idx == 0 {
                        0
                    } else {
                        post_reg_id as usize
                    };
                    client.sender.request_spawn(&client.receiver, cid, pid);
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
            (
                ClientCommand {
                    tick: self.input_tick as f64,
                    generation: generation as f64,
                    mouse_scale,
                    has_mouse_scale: true,
                    invert: Some(invert),
                    ..ClientCommand::default()
                },
                ClientLook::default(),
                ClientFlightCommand::default(),
                ViewActions::default(),
            )
        }
    }

    /// Steps the battle simulation/network client and renders one complete frame into
    /// `self.scene.canvas.framebuffer`.
    pub fn step_and_draw(
        &mut self,
        dt: f64,
        focused: bool,
        settings: &mut AppSettingsStore,
        _audio_router: &SharedAudioRouter,
    ) -> Result<Option<BattleTransition>, String> {
        let dt = dt.clamp(0.0, 0.1);
        self.elapsed_s += dt;

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
                BattleAuthority::Online { .. } => {
                    Ok(Some(BattleTransition::LeaveOnline { reason: None }))
                }
            };
        }

        let ui_captures_input = self.spawn_ctrl.is_open()
            || self.scoreboard_ctrl.model.is_open()
            || self.pause_menu.is_open
            || self.chat.is_open();
        let active_gameplay = focused && !ui_captures_input;
        let measure_steady_allocs = !self.is_overlay_open() && self.steady_frames >= 15;
        let alloc_before = if measure_steady_allocs {
            super::render_thread_alloc_count()
        } else {
            0
        };

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
                *now_ms += dt * 1000.0;
                world.set_paused(self.pause_menu.is_open);
                world.set_suspended(!focused);
                world.poll_publications(receiver, *now_ms)?;
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
                if world.native_admitted {
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
                if let (Some(snap), Some(replicas)) =
                    (world.latest_fallback.as_deref(), self.scene.online.as_mut())
                {
                    replicas.hud.apply_fallback(&mut self.scene.bridge, snap);
                }
                receiver.publications.clear();
                v
            }
            BattleAuthority::Online { client, .. } => {
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
                v
            }
        };

        // 4. Inspect life/death and match-over transitions.
        let (own_alive, game_over) = match &self.authority {
            BattleAuthority::InstantAction {
                receiver, world, ..
            } => {
                let over = world
                    .latest_fallback
                    .as_ref()
                    .is_some_and(|s| s.game_over)
                    || self
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

        if own_alive && !self.was_alive {
            self.was_alive = true;
            if self.spawn_ctrl.is_open() {
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

        // 6. Render the unified 3D + HUD frame.
        self.scene.draw(&view)?;
        if measure_steady_allocs && !overlay_active {
            let alloc_after = super::render_thread_alloc_count();
            self.steady_allocs_after_warmup += alloc_after.saturating_sub(alloc_before);
        }
        self.frames += 1;
        Ok(None)
    }

    fn sync_ports_cold(&mut self, profile_name: &str) {
        let Some(replicas) = &self.scene.online else {
            return;
        };
        match &self.authority {
            BattleAuthority::InstantAction {
                receiver, world, ..
            } => {
                self.spawn_port.sync_from_hud(
                    &replicas.hud.teams,
                    &replicas.hud.post_meta,
                    receiver.own.alive,
                );
                self.scoreboard_port.sync_from_state(
                    profile_name,
                    &replicas.hud.teams,
                    replicas.hud.game_over,
                    receiver,
                    world.latest_fallback.as_deref(),
                );
            }
            BattleAuthority::Online { client, .. } => {
                self.spawn_port.sync_from_hud(
                    &replicas.hud.teams,
                    &replicas.hud.post_meta,
                    client.receiver.own.alive,
                );
                self.scoreboard_port.sync_from_state(
                    profile_name,
                    &replicas.hud.teams,
                    replicas.hud.game_over,
                    &client.receiver,
                    None,
                );
            }
        }
        if self.pause_menu.show_player_list {
            self.pause_menu.players = self.scoreboard_port.players().unwrap_or_default();
        }
    }

    fn update_and_build_overlays(&mut self, dt: f64) {
        let fh = self.scene.bridge.hud_model.fonts[0].height;
        self.scene.bridge.hud_model.input[H::DT] = dt;
        self.scene.bridge.hud_model.input[H::HEIGHT] = self.height;
        self.scene.bridge.hud_model.input[H::ASPECT] = self.aspect;
        self.scene.bridge.hud_model.input[H::FOV] = 60.0;


        let spawn_open = self.spawn_ctrl.is_open();
        let menu_open = self.pause_menu.is_open;

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
                let m = root_anchor(self.height);
                surface.batch.quad(
                    m,
                    [0.0, 0.0, 1024.0, self.height],
                    A::SOLID,
                    UV,
                    [0.0, 0.02, 0.06, 0.42],
                    0.0,
                    0,
                );
                self.pause_menu.draw(&surface, &mut self.overlay_batch);
            }
            Self::append_batch(&mut self.scene.bridge.hud_model.batch, &self.overlay_batch);
            return;
        }

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
                    self.pause_menu.draw(&surface, &mut self.overlay_batch);
                }
                Self::append_batch(&mut self.scene.bridge.hud_model.batch, &self.overlay_batch);
            }
        }
    }

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
    pub fn handle_key(
        &mut self,
        code: &str,
        down: bool,
        repeat: bool,
        settings: &mut AppSettingsStore,
    ) {
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
                false,
                repeat,
                &mut self.chat,
                self.pause_menu.is_open,
                self.spawn_ctrl.is_open(),
                self.scoreboard_ctrl.model.is_open(),
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
    fn scoreboard_uses_published_characters_instead_of_visible_units() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        let bytes = [
            2, 0, 0, 0, 2, 1, 1, 7, 0, 1, 0, 3, 0, 4, 0, 1, 0, 1, 6, b'P', b'l', b'a', b'y', b'e',
            b'r', 2, 0, 2, 0, 0, 0, 5, 0, 1, 0, 255, 255, 0, 3, b'B', b'o', b't',
        ];
        receiver.roster.receive(4, 70000, &bytes).unwrap();
        receiver.roster.local = Some(0);
        let mut board = LiveScoreboardPort::new(true, "cw");
        board.sync_from_state("unused", &[[0.; 4]; 8], false, &receiver, None);
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
