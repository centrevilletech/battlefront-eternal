//! Native cold admission: building a battle's owners from the player's
//! content the way the JavaScript host builds them before the first tick
//! (game.js createGame headless, then createLocalServer), inside one owning
//! World. The host hands over files and options; every row the JavaScript
//! marshals from content (`rows`) and every owner construction call it makes
//! (`sequence`) are made here, in its order, so a natively admitted world
//! equals the module-admitted one bank for bank.
//!
//! Three callers share this path: the whole-world replay driver
//! (`Context::world_replay_admit`, recorded assets and options), the native
//! client's local battle (`Context::world_admit`, the content folder) and the
//! tests. None of them calls back into a host; an owner the path cannot yet
//! build answers `AdmitError::Unavailable` naming its stage, never a partial
//! world presented as whole.
#[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
pub mod network;
pub mod banks;
pub mod bodies;
pub mod dispatch;
pub mod fleet;
pub mod kernels;
pub mod rows;
pub mod sequence;
pub mod session;
pub mod partial;
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
}
impl Options {
    /// world-hash.mjs: `['rep', 'cis'].includes(side) ? 'cw' : 'gcw'`.
    pub fn era(&self) -> &'static str {
        if self.side == "rep" || self.side == "cis" { "cw" } else { "gcw" }
    }
    /// The battle's query, word for word as the page and the harness compose it.
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

/// An admitted battle: what createLocalServer's welcome tells the page and
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

/// The unit registry's members (registry.js Units.list) as admission left
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
