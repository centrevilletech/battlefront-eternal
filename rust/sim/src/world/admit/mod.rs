//! Native cold admission: building a battle's owners from the player's
//! content the way the JavaScript host builds them before the first tick
//! (game.js createGame headless, then createLocalServer), inside one owning
//! World. The host hands over files and options; every row the JavaScript
//! marshals from content (`rows`) and every owner construction call it makes
//! (`sequence`) are made here, in its order, so a natively admitted world
//! equals the module-admitted one bank for bank.
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
    pub map: String,
    pub side: String,
    /// `?ai=enhanced`; false is the Classic AI.
    pub enhanced: bool,
    pub seed: u32,
    pub heroes: bool,
    /// `?unit=`: the player's class, else the side's first.
    pub unit: Option<String>,
    /// The name createLocalServer gives its one player (the profile's
    /// playerName); None is addPlayer's default, "Player".
    pub name: Option<String>,
    /// The player's bot settings (aiMode off, aiDifficulty, battleSize).
    pub bots: Bots,
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
    /// world-hash.mjs: `['rep', 'cis'].includes(side) ? 'cw' : 'gcw'`.
    pub fn era(&self) -> &'static str {
        if self.side == "rep" || self.side == "cis" { "cw" } else { "gcw" }
    }
    /// The battle's query, word for word as the client and the harness compose it.
    pub fn query(&self) -> String {
        let mut query = format!("?map={}&side={}&era={}&ai={}&seed={}", self.map, self.side, self.era(),
            if self.enhanced { "enhanced" } else { "classic" }, self.seed);
        if self.heroes { query.push_str("&heroes=1"); }
        if let Some(unit) = &self.unit { query.push_str("&unit="); query.push_str(unit); }
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
    /// WorldOperation's setup row (the water clock), already applied.
    pub setup: WorldTickSetup,
    pub handles: Handles,
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
    /// The stage has no native body yet.
    Unavailable(Stage),
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
            AdmitError::Unavailable(stage) => write!(f, "native admission has no body for {stage:?}"),
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

fn admit_policy(world: &mut World, files: &dyn Files, options: &Options, rows: &mut Rows, screen: bool) -> Result<Admitted, AdmitError> {
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
    use super::Bots;
    #[test]
    fn off_fields_no_bots_and_sizes_scale_the_rosters() {
        let off = Bots::read("off", "hard", "small");
        assert!(!off.fielded);
        assert_eq!(off.unit_scale(), 0.0);
        let classic = Bots::read("classic", "Easy", "large");
        assert_eq!((classic.fielded, classic.tier(), classic.unit_scale()), (true, 1.0, 0.75));
        assert_eq!(Bots::read("enhanced", "", "full"), Bots::default());
    }
}
