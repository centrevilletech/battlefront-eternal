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
pub mod banks;
pub mod bodies;
pub mod dispatch;
pub mod kernels;
pub mod rows;
pub mod sequence;
pub mod spawn;
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

/// Whether the sequence has bodies. False until the owner construction
/// lands: the context entries then refuse Unavailable before reading input,
/// so a host's handshake can report it (abi::context_step::provided).
pub const fn provided() -> bool { false }

/// The whole path: load the battle's content as game.js fetches it, make the
/// rows the JavaScript marshals, construct every owner in its order, admit the
/// local human and the publication host. One call a battle, before any tick.
pub fn admit(world: &mut World, files: &dyn Files, options: &Options) -> Result<Admitted, AdmitError> {
    let loader = bfe_content::battle::Loader::new(files);
    let battle = bfe_content::battle::Battle::load(&loader, &options.map, &options.query())
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let content = bfe_content::content::Content::build(battle)
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    sequence::run(world, &content, files, options).map(|(admitted, _)| admitted)
}
