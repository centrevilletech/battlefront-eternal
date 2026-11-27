//! Native cold admission: building a battle's owners from the player's
//! content the way the JavaScript host builds them before the first tick
//! (createGame headless, then createLocalServer), inside one owning
//! World. The host hands over files and options; every row the JavaScript
//! marshals from content (`rows`) and every owner construction call it makes
//! (`sequence`) are made here, in its order, so a natively admitted world
//! equals the module-admitted one bank for bank.
pub mod mission;
pub mod network;
pub mod banks;
pub mod bodies;
pub mod dispatch;
pub mod fleet;
pub mod kernels;
pub mod rows;
pub mod sequence;
pub mod session;
pub mod spawn;
pub mod stock;
pub use dispatch::Returned;
pub use rows::{Call, Calls, Cx, Definitions, Ids, Rows};
pub use sequence::Stage;

use crate::gen::WorldTickSetup;
use crate::world::owner::World;
use bfe_content::files::Files;

/// What the battle is asked for: the setup builder's options (the battle
/// URL and tools/world-hash.mjs's arguments) as the simulation needs them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The game the battle belongs to: the 2004 game unless the battle's
    /// entry named the 2005 game. It selects the movement profile, the input
    /// tables, the wire bounds and records. Admission refuses a family this
    /// build cannot play.
    pub family: bfe_family::GameFamily,
    pub map: String,
    pub side: String,
    /// The mode (`"conquest"`; a 2005 battle may name another, such as `"eli"`).
    /// A 2004 battle carries it without using or writing it.
    pub mode: String,
    /// `?ai=enhanced`; false is the Classic AI.
    pub enhanced: bool,
    pub seed: u32,
    pub heroes: bool,
    /// `?unit=`: the player's class, else the side's first.
    pub unit: Option<String>,
    /// Opposing infantry class for a local stand-in roster.
    pub practice_target: Option<String>,
    /// The name createLocalServer gives its one player (the profile's
    /// playerName); None is addPlayer's default, "Player".
    pub name: Option<String>,
    /// The player's bot settings (aiMode off, aiDifficulty, battleSize).
    pub bots: Bots,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            family: bfe_family::GameFamily::Swbf1,
            map: "nab2".into(),
            side: "rep".into(),
            mode: bfe_family::DEFAULT_MODE.into(),
            enhanced: false,
            seed: 1,
            heroes: false,
            unit: None,
            practice_target: None,
            name: None,
            bots: Bots::default(),
        }
    }
}

/// The player's settings the world is built with: whether any bots are
/// fielded (aiMode "off" fields none: the player is alone), the Classic
/// difficulty tier (easy 1, medium 2, hard 3), the battle size's unit scale
/// in percent (full 100, reduced 75, small 50), and whether a hit from the
/// victim's own team hurts (friendlyFire, on by default as in the 2004 game).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bots {
    pub fielded: bool,
    pub tier: u8,
    pub scale_percent: u8,
    pub friendly_fire: bool,
}
impl Default for Bots {
    fn default() -> Self { Self { fielded: true, tier: 2, scale_percent: 100, friendly_fire: true } }
}
impl Bots {
    /// SetUnitCount's multiplier: none at all when no bots are fielded.
    pub fn unit_scale(&self) -> f64 {
        if self.fielded { f64::from(self.scale_percent.clamp(1, 100)) / 100.0 } else { 0.0 }
    }
    /// The difficulty tier the AI is built with, easy to hard.
    pub fn tier(&self) -> f64 { f64::from(self.tier.clamp(1, 3)) }
    /// The settings' words, as game.js reads them: aiMode ("off" fields no
    /// bots), aiDifficulty (`TIER`: easy, medium, hard, else medium) and
    /// battleSize (`BATTLE_SIZES`: full, large 3/4, small 1/2).
    pub fn read(mode: &str, difficulty: &str, size: &str) -> Self {
        let tier = match difficulty.to_ascii_lowercase().as_str() { "easy" => 1, "hard" => 3, _ => 2 };
        let scale_percent = match size { "large" => 75, "small" => 50, _ => 100 };
        Self { fielded: mode != "off", tier, scale_percent, friendly_fire: true }
    }
}
impl Options {
    /// `['rep', 'cis'].includes(side) ? 'cw' : 'gcw'`.
    pub fn era(&self) -> &'static str {
        if self.side == "rep" || self.side == "cis" { "cw" } else { "gcw" }
    }
    /// The battle's query, word for word as the client and the harness compose it.
    /// A 2005 battle adds `family=swbf2` and a mode other than conquest.
    pub fn query(&self) -> String {
        let mut query = format!("?map={}&side={}&era={}&ai={}&seed={}", self.map, self.side, self.era(),
            if self.enhanced { "enhanced" } else { "classic" }, self.seed);
        if self.heroes { query.push_str("&heroes=1"); }
        if let Some(unit) = &self.unit { query.push_str("&unit="); query.push_str(unit); }
        if self.family == bfe_family::GameFamily::Swbf2 {
            query.push_str("&family=swbf2");
            if let Some(target) = &self.practice_target { query.push_str("&practiceTarget="); query.push_str(target); }
            if self.mode != bfe_family::DEFAULT_MODE { query.push_str("&mode="); query.push_str(&self.mode); }
        }
        query
    }
}

/// Pool keys of the owners a live host drives after admission. Keys inside
/// this World only, never addresses or process-global identities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Handles {
    /// The local human's input mailbox (input_mailbox_create).
    pub mailbox: u32,
    /// The local human's players entry (players_create).
    pub player: u32,
    /// The publication session (wire_publish_create) and its host (wire_host_create).
    pub publication: u32,
    pub host: u32,
    /// The entity edge queue (wire_edges_create) and the client's store source.
    pub edges: u32,
    pub client: u32,
    /// What the per-tick host's presentation capture writes through
    /// (sim::host::Capture): the wire's combat capture (wire_combat_create),
    /// the presentation encoder (wire_presentation_create) and the store
    /// source whose registry names weapons and definitions
    /// (wire_store_source_create).
    pub combat: u32,
    pub presentation: u32,
    pub store_source: u32,
}

/// An admitted battle: what createLocalServer's welcome tells the client and
/// what the first tick row needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Admitted {
    /// The battle's game, as admitted.
    pub family: bfe_family::GameFamily,
    /// WorldOperation's setup row (the water clock), already applied.
    pub setup: WorldTickSetup,
    pub handles: Handles,
    /// Position codebook resolved from the map before the welcome snapshot.
    pub wire_bounds: crate::wire::PositionBounds,
    /// `world.player(id).playerId`: the local human's id in publications.
    pub player_id: f64,
    /// The entity registry descriptor the welcome carries (entity-registry).
    pub registry: serde_json::Value,
    /// preparePhase's foreign-row count for the first tick (WorldTickInput.units).
    pub units: u32,
    /// createLocalServer.welcome's two text events (entity-registry, then
    /// welcome) and the initial publication's packet bytes that follow them.
    pub welcome: [String; 2],
    pub publication: Vec<u8>,
    /// Exact spans and reliable records produced with the welcome byte arena.
    pub publication_output: crate::gen::WirePublicationOutput,
}

/// Why admission did not produce a world. Every variant names the stage, so
/// a host can tell a missing body (Unavailable) from content it cannot play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmitError {
    /// The selected script failed during setup and the battle is a draw.
    MissionSetup(String),
    /// The stage has no native body yet.
    Unavailable(Stage),
    /// The requested game family is not available in this build.
    Family(bfe_family::UnavailableFamily),
    /// Content the stage could not load or read (the message is the loader's).
    Content { stage: Stage, message: String },
    /// An owner refused a row, as the export would answer null.
    Refused { stage: Stage, call: String },
    /// The sequence has no body for an export of that name.
    NoBody { stage: Stage, call: String },
}
impl std::fmt::Display for AdmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdmitError::MissionSetup(message) => f.write_str(message),
            AdmitError::Unavailable(stage) => write!(f, "native admission has no body for {stage:?}"),
            AdmitError::Family(err) => write!(f, "{err}"),
            AdmitError::Content { stage, message } => write!(f, "{stage:?}: {message}"),
            AdmitError::Refused { stage, call } => write!(f, "{stage:?}: {call} refused its row"),
            AdmitError::NoBody { stage, call } => write!(f, "{stage:?}: the sequence has no body for {call}"),
        }
    }
}

/// Whether the sequence has bodies. True now that owner construction is wired.
pub const fn provided() -> bool { true }

/// The unit registry's members (Units.list) as admission left
/// them, for the replay driver's comparison with a recorded host's.
pub fn registry_members(world: &World) -> &[crate::gen::Member] { &world.sim.store.units.list }

/// A text of the world's string table, for the replay driver's comparison
/// of admitted rows with a recorded host's (each side's ids are its own).
pub fn string_text(world: &World, id: crate::store::StrId) -> String {
    world.sim.strings.list.get(id.0 as usize).map(|units| String::from_utf16_lossy(units)).unwrap_or_default()
}

/// The world's string table as admission left it (the replay driver checks
/// the host's interns against it).
pub fn string_table(world: &World) -> &crate::store::Strings { &world.sim.strings }

/// The whole path: load the battle's content as game.js fetches it, make the
/// rows the JavaScript marshals, construct every owner in its order, admit the
/// local human and the publication host. One call a battle, before any tick.
pub fn admit(world: &mut World, files: &dyn Files, options: &Options) -> Result<Admitted, AdmitError> {
    admit_with_rows(world, files, options).map(|(admitted, _)| admitted)
}

/// The same path, also answering every call the sequence dispatched: the
/// replay driver compares them with a recorded host's calls one by one.
pub fn admit_with_rows(world: &mut World, files: &dyn Files, options: &Options) -> Result<(Admitted, Rows), AdmitError> {
    let mut rows = Rows::default();
    admit_into(world, files, options, &mut rows).map(|admitted| (admitted, rows))
}

/// `admit_with_rows` keeping the rows in the caller's buffer, so a driver
/// still has the calls dispatched before a failure.
pub fn admit_into(world: &mut World, files: &dyn Files, options: &Options, rows: &mut Rows) -> Result<Admitted, AdmitError> {
    admit_policy(world, files, options, rows, false)
}

/// Menu launches keep their admitted player parked until class/post selection.
pub fn admit_spawn_screen(world: &mut World, files: &dyn Files, options: &Options) -> Result<Admitted, AdmitError> {
    admit_policy(world, files, options, &mut Rows::default(), true)
}

/// Family-explicit battle admission (developer/test seam for 2005 targets while
/// the player-facing family gate remains closed). `GameFamily::Swbf1` always
/// delegates directly to the untouched 2004 admission path.
pub fn admit_for_family(
    world: &mut World,
    files: &dyn Files,
    family: bfe_content::GameFamily,
    options: &Options,
    mission: Option<&str>,
    spawn_screen: bool,
) -> Result<Admitted, AdmitError> {
    if family == bfe_content::GameFamily::Swbf1 {
        if mission.is_some() {
            return Err(AdmitError::Content {
                stage: Stage::Content,
                message: "2004 admission does not accept a mission override".into(),
            });
        }
        return admit_policy(world, files, options, &mut Rows::default(), spawn_screen);
    }
    // The battle's game is one value: the options' family is the one loaded.
    let options = &Options { family, ..options.clone() };
    let loader = bfe_content::battle::Loader::new(files);
    let mut query = options.query();
    if let Some(m) = mission {
        query.push_str("&mission=");
        query.push_str(m);
    }
    if spawn_screen {
        query.push_str("&spawnscreen=1");
    }
    let battle = bfe_content::battle::Battle::load_for_family(&loader, family, &options.map, &query)
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let content = bfe_content::content::Content::build(battle)
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    sequence::run_into(world, &content, files, options, &mut Rows::default())
}

fn admit_policy(world: &mut World, files: &dyn Files, options: &Options, rows: &mut Rows, screen: bool) -> Result<Admitted, AdmitError> {
    options.family.require_available().map_err(AdmitError::Family)?;
    let loader = bfe_content::battle::Loader::new(files);
    let mut query = options.query();
    if screen { query.push_str("&spawnscreen=1"); }
    let battle = bfe_content::battle::Battle::load(&loader, &options.map, &query)
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let content = bfe_content::content::Content::build(battle)
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    sequence::run_into(world, &content, files, options, rows)
}

#[cfg(test)]
mod bot_settings {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingFiles(AtomicUsize);
    impl Files for CountingFiles {
        fn read(&self, _path: &str) -> Option<std::rc::Rc<[u8]>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(std::rc::Rc::from(&b"{}"[..]))
        }
    }

    #[test]
    fn off_fields_no_bots_and_sizes_scale_the_rosters() {
        let off = Bots::read("off", "hard", "small");
        assert!(!off.fielded);
        assert_eq!(off.unit_scale(), 0.0);
        let classic = Bots::read("classic", "Easy", "large");
        assert_eq!((classic.fielded, classic.tier(), classic.unit_scale()), (true, 1.0, 0.75));
        assert_eq!(Bots::read("enhanced", "", "full"), Bots::default());
    }

    #[test]
    fn unavailable_family_is_refused_before_any_content_read_locally_and_on_network() {
        let opts = Options {
            family: bfe_family::GameFamily::Swbf2,
            map: "kam9".into(),
            side: "all".into(),
            mode: "hunt".into(),
            ..Options::default()
        };
        let files = CountingFiles(AtomicUsize::new(0));
        let mut world = World::new();
        let err = admit(&mut world, &files, &opts).unwrap_err();
        assert_eq!(err.to_string(), "Battlefront II (2005) is not available in this build");
        assert_eq!(files.0.load(Ordering::SeqCst), 0);

        let ctx = crate::abi::context::Context::new();
        let net_err = ctx.world_network_admit(&files, &opts).unwrap_err();
        assert_eq!(net_err.message(), "Battlefront II (2005) is not available in this build");
        assert_eq!(files.0.load(Ordering::SeqCst), 0);
        let _ = ctx.dispose();
    }

    #[test]
    fn a_2004_query_never_carries_family_or_mode() {
        for mode in ["conquest", "duel", "ConQuest", ""] {
            let options = Options { mode: mode.into(), ..Options::default() };
            assert_eq!(options.query(), "?map=nab2&side=rep&era=cw&ai=classic&seed=1", "{mode}");
        }
        let later = Options { family: bfe_family::GameFamily::Swbf2, map: "kam9".into(), side: "all".into(), ..Options::default() };
        assert_eq!(later.query(), "?map=kam9&side=all&era=gcw&ai=classic&seed=1&family=swbf2");
        let named = Options { mode: "hunt".into(), ..later };
        assert_eq!(named.query(), "?map=kam9&side=all&era=gcw&ai=classic&seed=1&family=swbf2&mode=hunt");
    }

    #[test]
    fn swbf2_family_admission_wires_explosions_moving_props_and_network_entry() {
        use crate::abi::context::Context;
        use bfe_content::{files::MemFiles, zero_hash, GameFamily};

        let f32b = |xs: &[f32]| -> Vec<u8> { xs.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let u32b = |xs: &[u32]| -> Vec<u8> { xs.iter().flat_map(|x| x.to_le_bytes()).collect() };

        let mut files = MemFiles::default();
        files.insert("common/common.json", br#"{"family":"swbf2","textures":{}}"#.to_vec());
        let root_crc = zero_hash::crc_hex("DummyRoot");
        let idle_crc = zero_hash::crc_hex("human_rifle_stand_idle_emote_full");
        files.insert(
            "common/anim_banks.json",
            format!(
                r#"{{"fps":30,"banks":{{"human_0":{{"clips":[{{"crc":"{idle_crc}","flags":0,"frames":2,"offset":0,"tracks":["{root_crc}"]}}]}}}}}}"#
            )
            .into_bytes(),
        );
        files.insert("common/anim_banks.bin", f32b(&[0.0; 14]));
        files.insert("maps/bay1/collision.bin", f32b(&[-20.0, 0.0, -20.0, 20.0, 0.0, -20.0, 0.0, 0.0, 20.0]));
        files.insert("maps/bay1/collision_index.bin", u32b(&[0, 1, 2]));
        files.insert("maps/bay1/collision_poly.bin", u32b(&[0]));
        files.insert("maps/bay1/props.bin", f32b(&[0.0; 24]));
        files.insert("maps/bay1/props_index.bin", u32b(&[0, 1, 2]));
        files.insert(
            "maps/bay1/props_collision.bin",
            f32b(&[-1.0, 0.0, -1.0, 1.0, 0.0, -1.0, 0.0, 2.0, 1.0]),
        );
        files.insert("maps/bay1/props_collision_index.bin", u32b(&[0, 1, 2]));
        files.insert("maps/bay1/props_collision_poly.bin", u32b(&[0]));
        files.insert(
            "maps/bay1/props.json",
            br#"{
                "family": "swbf2",
                "models": {
                    "cruiser_hull": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}],
                    "static_crate": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]
                },
                "collision": {
                    "cruiser_hull": {"firstTriangle": 0, "triangleCount": 1},
                    "static_crate": {"firstTriangle": 0, "triangleCount": 1}
                },
                "instances": [
                    {"name":"orbit_ship","class":"ship_cls","base":"destructablebuilding","model":"cruiser_hull","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,50,10,0,1],"properties":[["MaxHealth","5000"]]},
                    {"name":"deck_crate","class":"crate_cls","base":"destructablebuilding","model":"static_crate","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,-10,0,0,1],"properties":[["MaxHealth","500"]]}
                ]
            }"#
            .to_vec(),
        );
        files.insert(
            "maps/bay1/map.json",
            br#"{
                "family": "swbf2",
                "bounds": {"min": [-100, -50, -100], "max": [100, 50, 100]},
                "wireBounds": {"min": [-120, -60, -120], "max": [120, 60, 120]},
                "meshes": [], "lights": [], "regions": [], "deathRegions": [], "paths": [],
                "spawns": {"sp_blue": [[0, 1, 0, 0]]},
                "commandPosts": [{"name":"cp1","team":1,"spawnPath":"sp_blue","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],
                "worldAnimations": {
                    "clips": {"orbit": {"duration": 10.0, "loop": true, "localTranslation": false, "positions": [{"time":0,"value":[0,0,0],"type":1},{"time":10,"value":[20,0,0],"type":1}], "rotations": []}},
                    "groups": {"g1": {"defaultOn": true, "bindings": [{"clip":"orbit","object":"orbit_ship"}]}},
                    "hierarchies": []
                },
                "missions": {"bay1c_ass": {"layers": [], "sides": {"rep": ["bay_demo"]}}},
                "layers": {}
            }"#
            .to_vec(),
        );
        files.insert(
            "sides/rep/units.json",
            br#"{
                "units": {
                    "bay_demo": {
                        "class": "bay_demo",
                        "base": "soldier",
                        "model": "demo_rig",
                        "actor": "actors/demo_rig.json",
                        "properties": [
                            ["MaxHealth", "300"], ["ControlSpeed", "stand 4 3 2"],
                            ["ControlSpeed", "crouch 2 1 2"], ["ControlSpeed", "jump 3 2 2"],
                            ["ControlSpeed", "jet 1 1 2"], ["ControlSpeed", "sprint 6 2 1"],
                            ["ControlSpeed", "roll 4 1 1"], ["ControlSpeed", "tumble 2 2 1"],
                            ["SprintAccelerateTime", "2"], ["EnergyBar", "18"],
                            ["EnergyRestore", "1"], ["EnergyRestoreIdle", "2"],
                            ["EnergyDrainSprint", "1"], ["EnergyMinSprint", "3"],
                            ["EnergyCostJump", "1"], ["EnergyCostRoll", "2"], ["JumpHeight", "1"],
                            ["JumpForwardSpeedFactor", "1"], ["JumpStrafeSpeedFactor", "1"],
                            ["RollSpeedFactor", "1"]
                        ],
                        "weapons": [
                            {
                                "class": "bay_rocket",
                                "base": "launcher",
                                "RoundsPerClip": "4",
                                "ordnance": {
                                    "class": "bay_rocket_ord",
                                    "base": "missile",
                                    "Velocity": "60",
                                    "LifeSpan": "4",
                                    "MaxDamage": "150",
                                    "ExplosionName": {
                                        "class": "bay_rocket_exp",
                                        "base": "explosion",
                                        "Damage": "300",
                                        "DamageRadius": "5.5"
                                    }
                                }
                            },
                            {
                                "class": "bay_seeker",
                                "base": "remote",
                                "SwitchImmediately": "1",
                                "ordnance": {
                                    "class": "bay_seeker_body",
                                    "base": "flyer",
                                    "GuidedMissile": "1",
                                    "MinSpeed": "10.0",
                                    "MaxSpeed": "20.0",
                                    "BoostSpeed": "50.0",
                                    "GuidedMissileBoostExplosionSpeedThreshold": "0.6",
                                    "ExplosionDestruct": {
                                        "class": "bay_seeker_exp",
                                        "base": "explosion",
                                        "Damage": "90",
                                        "DamageRadius": "3.5"
                                    },
                                    "ExplosionCritical": {
                                        "class": "bay_seeker_crit",
                                        "base": "explosion",
                                        "Damage": "410",
                                        "DamageRadius": "9.5"
                                    }
                                }
                            },
                            {
                                "class": "bay_detpack",
                                "base": "detonator",
                                "RoundsPerClip": "2",
                                "ordnance": {
                                    "class": "bay_detpack_mine",
                                    "base": "mine",
                                    "MaxHealth": "50",
                                    "ExplosionTrigger": {
                                        "class": "bay_detpack_exp",
                                        "base": "explosion",
                                        "Damage": "450",
                                        "DamageRadiusInner": "4.0",
                                        "DamageRadiusOuter": "7.0"
                                    }
                                }
                            }
                        ]
                    }
                }
            }"#
            .to_vec(),
        );
        files.insert("sides/rep/weapons.json", br#"{"models":{},"firePoints":{}}"#.to_vec());
        files.insert("sides/rep/weapons.bin", Vec::new());
        files.insert("sides/rep/weapons_index.bin", Vec::new());
        files.insert("sides/rep/anim_banks.json", br#"{"fps":30,"banks":{},"packages":{}}"#.to_vec());
        files.insert("sides/rep/anim_banks.bin", Vec::new());
        files.insert(
            "sides/rep/actors/demo_rig.json",
            br#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]}"#.to_vec(),
        );
        files.insert("sides/rep/actors/demo_rig.bin", f32b(&[0.0; 27]));
        files.insert("sides/rep/actors/demo_rig_index.bin", u32b(&[0, 1, 2]));

        let opts = super::Options {
            family: GameFamily::Swbf2,
            mode: "ass".into(),
            map: "bay1".into(),
            side: "rep".into(),
            enhanced: false,
            seed: 1,
            heroes: false,
            unit: None,
            practice_target: None,
            name: None,
            bots: Bots::default(),
            ..super::Options::default()
        };

        // 1. Local admission: single moving body for animated ship, single fixed body for crate,
        //    and both rocket + detpack explosions admitted into world.ordnance + content_tables_files_for_family.
        // A 2005 battle admits its soldier classes with the movement numbers
        // their prepared properties give.
        let ctx = Context::new();
        let admitted = ctx
            .world_admit_for_family(&files, GameFamily::Swbf2, &opts, Some("bay1c_ass"), false)
            .unwrap();
        let bounds = ctx.collision_body_bounds().unwrap();
        assert_eq!(bounds.len(), 2, "exactly 2 bodies (no duplicate static/moving frigate body)");
        assert!(bounds[0].1, "world-animated ship must be admitted as a moving body");
        assert!((bounds[0].2[0] - 49.0).abs() < 1e-6 && (bounds[0].2[3] - 51.0).abs() < 1e-6);
        assert!(!bounds[1].1, "static crate must remain a fixed body");
        assert!((bounds[1].2[0] - (-11.0)).abs() < 1e-6 && (bounds[1].2[3] - (-9.0)).abs() < 1e-6);

        let exps = ctx.world_explosion_defs().unwrap();
        assert!(exps.iter().any(|&(d, _, r)| (d - 300.0).abs() < 1e-6 && (r - 5.5).abs() < 1e-6));
        assert!(exps.iter().any(|&(d, _, r)| (d - 450.0).abs() < 1e-6 && (r - 7.0).abs() < 1e-6));
        // A guided rocket's ordinary and critical explosions are both admitted.
        assert!(exps.iter().any(|&(d, _, r)| (d - 90.0).abs() < 1e-6 && (r - 3.5).abs() < 1e-6));
        assert!(exps.iter().any(|&(d, _, r)| (d - 410.0).abs() < 1e-6 && (r - 9.5).abs() < 1e-6));

        let (exp_ids, _) = crate::host::content_tables_files_for_family(
            &ctx,
            &files,
            GameFamily::Swbf2,
            &opts,
            Some("bay1c_ass"),
            admitted.handles.store_source,
        )
        .unwrap();
        assert!(exp_ids.iter().filter(|&&id| id > 0).count() >= 2);
        // Every admitted explosion has a host entry, so no blast can be refused
        // by the wire's combat capture, and the critical one is named by its droid.
        assert_eq!(crate::host::unmapped_explosions(&ctx, admitted.handles.store_source, &exp_ids).unwrap(), Vec::<usize>::new());
        assert!(ctx.explosion_owners().unwrap().iter().any(|owned| {
            matches!(owned.owner, crate::abi::context_step::ExplosionOwner::ReconCritical(_))
        }));

        // 2. Network admission via world_network_admit_for_family also succeeds cleanly.
        let net_ctx = Context::new();
        let net_adm = net_ctx
            .world_network_admit_for_family(&files, GameFamily::Swbf2, &opts, Some("bay1c_ass"))
            .unwrap();
        assert!(net_adm.handles.host != 0);
        // A server's battle has its map's bounds and its space battlefield as
        // a local one has them, when it starts, when a player joins it and
        // when it starts again in a fresh world.
        let attached = |w: &mut World| (w.units.map_world_bounds, w.space.as_ref().map(|s| (s.bounds.min, s.bounds.max, s.doors.len(), s.buildings.len(), s.world_anims.objects.len())));
        let local = ctx.owned(attached).unwrap();
        assert!(local.0.is_some() && local.1.is_some_and(|space| space.4 > 0), "{local:?}");
        assert_eq!(net_ctx.owned(attached).unwrap(), local);
        let mailbox = net_ctx
            .owned(|w| {
                let codes = session::input_codes_for(GameFamily::Swbf2).iter().map(|code| w.sim.strings.intern_str(code)).collect();
                let setup = crate::gen::InputMailboxSetup { epoch: 1., capacity: 256., codes };
                w.sim.calls.result(crate::gen::InputMailboxSetup::WORDS, |row, heap| setup.put(row, heap));
                crate::input::module::input_mailbox_create_in(w)
            })
            .unwrap();
        net_ctx.world_network_join(&files, 1, "late", "Late", false, false, false, mailbox, 1000.).unwrap();
        assert_eq!(net_ctx.owned(attached).unwrap(), local);
        let again = Context::new();
        again.world_network_admit_for_family(&files, GameFamily::Swbf2, &opts, Some("bay1c_ass")).unwrap();
        assert_eq!(again.owned(attached).unwrap(), local);
        // Its bodies stay within the identities a 2005 publication keeps,
        // less those its hulls take when they are born. The AI's own
        // reservation, all that bounds a 2004 battle, is past them.
        net_ctx
            .owned(|w| {
                let unborn = crate::units::f3_hulls::unborn_in(&w.units);
                let most = network::body_ids(w).unwrap();
                assert_eq!(most as usize + unborn, crate::wire::bounds::Capacity::FULL.known);
                assert_eq!(rows::battle::stock::id_limit(w, GameFamily::Swbf2), most);
                assert!(rows::battle::stock::id_limit(w, GameFamily::Swbf1) > most);
            })
            .unwrap();
    }
}
