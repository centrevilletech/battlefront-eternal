//! A cold setup source admits one thread-affine Context. Native content and
//! recorded setup fixtures meet here; the runtime has no content parser or
//! alternate admission algorithm of its own. Diagnostic sources stay in tests.
use bfe_content::files::Files;
use bfe_sim::world::admit;
use bfe_sim::{
    abi::context::Context,
    gen::{WorldTickInput, WorldTickSetup},
};
use serde_json::Value;

// These are pool keys within this Context, never pointers or process-global
// identities. Keeping them beside the input template prevents a later battle
// from accidentally writing the previous battle's mailbox or publisher.
pub use admit::Handles;

pub struct Admitted {
    pub setup: WorldTickSetup,
    pub tick: WorldTickInput,
    pub handles: Handles,
    pub player_id: f64,
    pub registry: Value,
    pub query: String,
    /// Packet bytes from `createLocalServer.welcome`'s initial tick-0 publication
    /// produced during `Context::world_admit`.
    pub publication: Vec<u8>,
    pub publication_output: bfe_sim::gen::WirePublicationOutput,
    pub host_content: bfe_sim::host::Content,
    pub explosions: Vec<u32>,
}

impl Admitted {
    // world_admit is the only constructor of gameplay owners. This conversion
    // retains its actual pool keys and foreign-row count, rather than inferring
    // a player or publisher from construction order or a resource descriptor.
    // All session and presentation capture handles (combat, presentation,
    // store_source) must be non-zero pool keys produced by world_admit.
    pub fn from_world(world: admit::Admitted, options: &admit::Options) -> Result<Self, String> {
        let h = &world.handles;
        if [
            h.mailbox,
            h.player,
            h.publication,
            h.host,
            h.edges,
            h.client,
            h.combat,
            h.presentation,
            h.store_source,
        ]
        .contains(&0)
        {
            return Err("native admission returned an absent session owner".into());
        }
        if !world.player_id.is_finite()
            || !(0. ..=u32::MAX as f64).contains(&world.player_id)
            || world.player_id.fract() != 0.
        {
            return Err("native admission returned an invalid player identity".into());
        }
        Ok(Self {
            setup: world.setup,
            handles: world.handles,
            player_id: world.player_id,
            registry: world.registry,
            query: options.query(),
            publication: world.publication,
            publication_output: world.publication_output,
            host_content: Default::default(),
            explosions: Vec::new(),
            tick: WorldTickInput {
                clock: bfe_content_native::setup::battle_clock(),
                units: world.units,
                ai: true,
                live: true,
                ..Default::default()
            },
        })
    }
}

pub struct NativeSource<'a> {
    pub spawn_screen: bool,
    pub files: &'a dyn Files,
    pub options: admit::Options,
}

// createLocalServer has already admitted the local human before ticking.
// A provider must finish definitions, capacity reservations and that human's
// connection before returning. It must not install tick callbacks into Rust.
pub trait Source {
    fn admit(&self, context: &Context) -> Result<Admitted, String>;
}

impl Source for NativeSource<'_> {
    // The Context supplies Unavailable itself while admission has no bodies.
    // Propagate it unchanged before making a window or loading display assets;
    // a launch never substitutes a recording or another simulation host.
    fn admit(&self, context: &Context) -> Result<Admitted, String> {
        let world = if self.spawn_screen {
            context.world_admit_spawn_screen(self.files, &self.options)
        } else {
            context.world_admit(self.files, &self.options)
        }.map_err(describe)?;
        let mut admitted = Admitted::from_world(world, &self.options)?;
        if self.spawn_screen { admitted.query.push_str("&spawnscreen=1"); }
        let (explosions, content) = bfe_sim::host::content_tables_files(
            context,
            self.files,
            &self.options,
            admitted.handles.store_source,
        )?;
        admitted.explosions = explosions;
        admitted.host_content = content;
        Ok(admitted)
    }
}

// Preserve the unavailable entry's name, including world_admit, while making
// its kind visible in the terminal instead of printing an unexplained symbol.
pub fn describe(error: bfe_sim::abi::context::Error) -> String {
    if error.kind() == bfe_sim::abi::context::ErrorKind::Unavailable {
        format!("native world API unavailable: {}", error.message())
    } else {
        error.to_string()
    }
}
