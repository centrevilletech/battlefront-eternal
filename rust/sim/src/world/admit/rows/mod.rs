//! The rows the JavaScript host marshals from content at admission, one
//! builder module per owner. Builders are not independent pure functions:
//! the sequence drives them in game.js createGame's order through one shared
//! context (`Cx`: the battle, its files, the options, the battle's string
//! interner, the identity tables earlier owners assigned and the values
//! earlier calls returned), dispatches the calls they make and feeds what
//! the bodies returned back into the context for the owners after them.
//! Nothing here touches an owner or the store.
//!
//! The recorder's admission call log (tools/world-hash.mjs --record-calls)
//! is the oracle each builder is checked against, word for word: `args` are
//! the export's scalar arguments, `row`/`heap` the `bridge.callArgs` block,
//! `scratch` the bytes written first, `strings` the texts interned for the
//! call, in order. In a comparison test the recorded `returned` values stand
//! in for the live bodies' answers.
pub mod ai;
pub mod battle;
pub mod cameras;
pub mod combat;
pub mod fleet;
pub mod items;
pub mod ordnance;
pub mod props;
pub mod recon;
pub mod startup;
pub mod teams;
pub mod units;
pub mod vehicle_pack;
pub mod weapon_policies;

use super::{AdmitError, Options, Stage};
use crate::gen::{Body, F3ColdStage, WorldTickSetup};
use crate::store::{Ref, StrId, Strings};
use bfe_content::battle::Battle;
use bfe_content::content::Content;
use bfe_content::defs::Defs;
use bfe_content::files::Files;
use bfe_content::install::Installed;
use bfe_content::Value;

/// One export call as the page makes it: everything the recording has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Call {
    /// The export's scalar arguments, in order (an export without any: empty).
    pub args: Vec<f64>,
    /// `bridge.callArgs` row and heap (an export without a block: empty).
    pub row: Vec<f64>,
    pub heap: Vec<f64>,
    /// Bytes the host wrote to scratch before the call (UTF-8 JSON, the
    /// vehicle pack's buffers, collider inputs).
    pub scratch: Vec<u8>,
    /// Texts the host interned for this call, UTF-16, in order. The builder
    /// interned them through `Cx::intern`, so their ids already sit in
    /// `args`, `row` or `heap`; the sequence replays the same interns into
    /// the world's table and the recording compares them 1:1.
    pub strings: Vec<Vec<u16>>,
}

/// Calls as the sequence replays them: the stage, the export's name and its
/// arguments.
pub type Calls = Vec<(Stage, &'static str, Call)>;

/// The identity tables the owners assign at admission, in JS order: each
/// owner reads the earlier owners' entries and adds its own. Never
/// recomputed, never invented. Producers store, consumers read.
///
/// Definition ids are keyed by the definition's identity, not its name:
/// the key is `<kind>:<JSON path>` where the path (a JSON array of keys
/// from the battle's content root: sides/<side>/units/..., vehicles/...,
/// props/..., common/..., effects/..., world/..., soldier/...) names the
/// shared `bfe_content::Value` object, so two equal-looking definitions
/// stay two ids. Kinds: `weapon:`, `class:`, `blast:` in `defs`;
/// `ordnance:`, `explosion:` in `ordnance`; `item:` in `items`; `recon:` in
/// `classes`. A `<kind>-sent:<path>` marker records what the host has
/// already sent to the simulation (F3's sent scans). Plain names may sit
/// beside them for owners that look up by name.
#[derive(Default)]
pub struct Ids {
    /// The collision world's kernel ids (Stage::Kernels).
    pub kernel: Option<Installed>,
    /// The collision kernel's bodies in world order (body_new's order: the
    /// hulls' colliders, then the props' bodies): the store's body ref and
    /// its kernel slot. `body_serial` is the kernel's serial the units stage
    /// starts from (f3_init's bodySerial); `bodies_changed` whether the
    /// kernel's topology changed after the last hull, when F3Hulls.prepare's
    /// trailing body list names every body again.
    pub bodies: Vec<(Ref<Body>, u32)>,
    pub body_serial: u32,
    pub bodies_changed: bool,
    /// Combat definition ids (cmb_class, cmb_weapon_set, cmb_explosion).
    pub defs: Vec<(String, f64)>,
    /// Ordnance and item definition ids (ord_def, items_def).
    pub ordnance: Vec<(String, f64)>,
    pub items: Vec<(String, f64)>,
    /// Hull refs by vehicle name (f2_register) and their kernel body slots.
    pub vehicles: Vec<(String, f64)>,
    pub vehicle_bodies: Vec<(String, f64)>,
    /// The fleet's construction receipts (Stage::Fleet, the sequence's hull
    /// constructors): hull refs, whether each spawned, its collider and the
    /// F2 body refs, in initial-then-prepared order. fleet::build_bound
    /// reads them; the AI's hull facts and the spawn holders derive from the
    /// same constructed hulls.
    pub fleet: Option<fleet::Bindings>,
    /// attachF3's inputs from the collision, fleet and combat owners: the
    /// body serial, the ordnance values with their ids, the blast bindings
    /// and the item bindings with the model answers the simulation gave.
    pub units: Option<units::InitialBindings>,
    /// The hulls as F3Hulls.prepare registers them: identity, size, center,
    /// model sphere and body name, with the changed body list when the
    /// kernel's topology moved (Stage::Fleet, then prepared hulls at Units).
    pub hulls: Vec<units::HullBinding>,
    /// The props' resolved bindings (live construction): registry target,
    /// member, the kernel bodies, explosion, team, trap placement facts.
    pub props: Vec<props::BuildingBinding>,
    /// Post, building and turret refs by index and the F4 owner handle of
    /// each prop instance.
    pub posts: Vec<f64>,
    pub buildings: Vec<f64>,
    pub turrets: Vec<f64>,
    pub prop_owners: Vec<f64>,
    /// The roster's factions and the mobile command posts' identities, as
    /// the factory and AI construction assigned them (ordinary posts are
    /// `posts.len()`).
    pub factions: Vec<f64>,
    pub mobile_posts: Vec<f64>,
    /// What the factory and the AI construction committed, for F4Battle,
    /// the tail and the session rows (battle::build_with_facts,
    /// battle::session_with_facts).
    pub battle: Option<battle::Facts>,
    /// AI observations retained across voice and Character construction.
    pub ai: Option<ai::Receipts>,
    /// Registry identities by unit key; the key `viewpoint` is the initial
    /// viewpoint's (game.js attaches it before the fleet).
    pub registry: Vec<(String, f64)>,
    /// Beam emitter identities handed out so far (the host's mirror pool of
    /// emitter objects numbers them in marshalling order).
    pub emitters: u32,
    /// Collision body identities handed out so far (the host's mirror pool
    /// of body objects: the fleet's at its import, the rest at f2_bodies).
    pub body_pool: u32,
    /// The props' kernel bodies by instance and look (0 built, 1 destroyed,
    /// 2 unbuilt), as LiveObjects' constructor made them.
    pub prop_kernels: Vec<(usize, u8, u32)>,

    /// The pose accumulator the page's AnimatedProp made per animated prop
    /// instance (anim-kernel.js newPose), at the prop constructor's seam.
    pub animated_poses: Vec<(usize, u32)>,
    /// Unit class ids (also `recon:` paths) and clip set ids as the units
    /// stage assigned them.
    pub classes: Vec<(String, f64)>,
    pub clips: Vec<(String, f64)>,
}

/// The authored definitions the owners register, as the same shared values
/// (bfe_content::Value shares its objects) in registration order: later
/// owners bind to these objects, never to a recomputed name or handle.
#[derive(Clone, Debug, Default)]
pub struct Definitions {
    pub classes: Vec<(String, Value)>,
    pub weapons: Vec<(String, Value)>,
    pub explosions: Vec<(String, Value)>,
    pub ordnance: Vec<(String, Value)>,
    pub items: Vec<(String, Value)>,
    pub recon: Vec<(String, Value)>,
}

/// The shared context the builders are driven through. Besides the
/// content and the tables, it carries what the later seams need from the
/// earlier constructions: the bodies' holders for the spawn piece, the
/// local human's bindings and the admission clock for the session.
pub struct Cx<'a> {
    /// The battle's content, decoded once (bfe_content::content::Content
    /// owns the loaded Battle: `content.battle`), and the definitions built
    /// from it once (Defs): every owner reads the same objects.
    pub content: &'a Content,
    pub battle: &'a Battle,
    pub defs: &'a Defs,
    /// The content files, for the raw bytes the host passes as fetched
    /// (ai-nav.json, the sides' ai.json, mission scripts).
    pub files: &'a dyn Files,
    pub options: &'a Options,
    /// The registered definitions as shared values, in JS order (producers:
    /// combat for classes, weapons, explosions, ordnance and items; units
    /// for recon). The id tables key the same objects by path.
    pub definitions: Definitions,
    /// Every body a rider can sit in, in F2Riders.ensure's order: live hulls,
    /// prepared hulls, then stationary turrets, each with its authored
    /// riders (producers: the fleet and live construction; consumer:
    /// spawn::prepare, which never derives a holder from a name).
    pub holders: Vec<super::bodies::Holder>,
    /// The local human's random stream and recon bindings as the earlier
    /// owners assigned them (consumer: session::run_with).
    pub player: super::session::PlayerBindings,
    /// The admission's monotonic clock origin in milliseconds (the harness
    /// admits at 1000); the session's clocks and the first publication start
    /// from it.
    pub at: f64,
    /// The host's wall clock in milliseconds at that moment (Date.now()),
    /// stamped on the welcome publication; the harness fixes it to
    /// 1700000000000 plus the monotonic clock, a host supplies its own.
    pub unix_ms: f64,
    /// The battle's string table as the builders see it: a copy of the
    /// world's at the start of admission, advanced by every intern in call
    /// order, so the ids the builders write are the ids the world has.
    pub strings: Strings,
    /// The F3 random-stream registry (f3-random.js register): every owner
    /// registers its callbacks here in the host's order, so the stream ids
    /// in the tail setup, the AI setup and the player bindings agree.
    pub streams: battle::streams::Streams,
    pub ids: Ids,
    /// What every dispatched call returned, by export name, in order: fed
    /// live by the sequence, or from the recording in a comparison test. A
    /// body without a scalar answer records 1; a row or block answer records
    /// 1 here and its words in `returned_rows`.
    pub returned: Vec<(&'static str, f64)>,
    pub returned_rows: Vec<(&'static str, Vec<f64>)>,
}
impl<'a> Cx<'a> {
    pub fn new(content: &'a Content, defs: &'a Defs, files: &'a dyn Files, options: &'a Options, strings: Strings) -> Self {
        Self { content, battle: &content.battle, defs, files, options, definitions: Definitions::default(), holders: Vec::new(),
            player: super::session::PlayerBindings::default(), at: 1000.0, unix_ms: 1_700_000_001_000.0, strings, streams: battle::streams::Streams::default(), ids: Ids::default(), returned: Vec::new(), returned_rows: Vec::new() }
    }
    /// The world's table after a body interned on its own (the AI's voices,
    /// the roster's Character names): the builders continue from it, so the
    /// next intern gets the id the world will give.
    pub fn refresh_strings(&mut self, world: &Strings) {
        if world.list.len() > self.strings.list.len() { self.strings = world.clone(); }
    }
    /// The kernel's bodies as F3's body-name rows name them, in world order.
    pub fn body_names(&self) -> Vec<crate::gen::F3BodyName> { units::body_names(self.ids.bodies.iter().copied()) }
    /// `bridge.strings.id(text)`: the id the world gives the text. The host's
    /// StringTable interns through the module only the first time it meets a
    /// text, so only a new text is recorded on the call being built.
    pub fn intern(&mut self, call: &mut Call, text: &str) -> StrId {
        let units: Vec<u16> = text.encode_utf16().collect();
        let before = self.strings.list.len();
        let id = self.strings.intern(&units);
        if self.strings.list.len() != before { call.strings.push(units); }
        id
    }
    /// The value the latest call of `export` returned, if any.
    pub fn returned(&self, export: &str) -> Option<f64> {
        self.returned.iter().rev().find(|(name, _)| *name == export).map(|(_, value)| *value)
    }
    /// The row the latest call of `export` answered with, if any.
    pub fn returned_row(&self, export: &str) -> Option<&[f64]> {
        self.returned_rows.iter().rev().find(|(name, _)| *name == export).map(|(_, words)| words.as_slice())
    }
}

/// What the sequence built: the setup row, the cold stage rows of the
/// bodies prepared before ticking (the spawn piece's), and every stage's
/// calls as they were dispatched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rows {
    /// WorldOperation's water row (the setup builder's twin).
    pub setup: WorldTickSetup,
    /// f3-factory.js: the cold stage rows of the bodies prepared before
    /// ticking; their cookies resolve in construction order through `Cx`.
    pub cold_stages: Vec<F3ColdStage>,
    pub calls: Vec<(Stage, Calls)>,
}
impl Rows {
    /// Every stage's calls built in order without a world: the recording's
    /// `returned` values pre-fed into `cx` stand in for the bodies' answers.
    /// A comparison test's path; the live path is `sequence::run`.
    pub fn build(cx: &mut Cx) -> Result<Rows, AdmitError> {
        let mut rows = Rows { setup: setup(cx.battle), ..Default::default() };
        for stage in super::sequence::ORDER {
            let calls = build(stage, cx)?;
            if !calls.is_empty() { rows.calls.push((stage, calls)); }
        }
        Ok(rows)
    }
}

/// One stage's calls from its owners, through the shared context. The
/// stage order is the sequence's (sequence::ORDER); an owner without a body
/// answers Unavailable naming its stage.
pub fn build(stage: Stage, cx: &mut Cx) -> Result<Calls, AdmitError> {
    match stage {
        Stage::Startup => startup::build(cx),
        Stage::Cameras => cameras::build(cx),
        Stage::Teams => teams::build(cx),
        Stage::Store => {
            let mut calls = ordnance::build(cx)?;
            calls.extend(items::build(cx)?);
            Ok(calls)
        }
        Stage::Combat => combat::build(cx),
        // attachF2: the pack first, then the hulls bound to the refs and
        // kernel bodies the sequence's constructors gave (an empty fleet
        // has no bound rows).
        Stage::Fleet => {
            let mut calls = vehicle_pack::build(cx)?;
            match cx.ids.fleet.clone() {
                Some(bindings) => calls.extend(fleet::build_bound(cx, &bindings)?),
                None => calls.extend(fleet::build(cx)?),
            }
            Ok(calls)
        }
        // attachF3: the unit prefix over the earlier owners' bindings, the
        // props, then the hull and body-name suffix (the whole body list
        // again when the kernel's topology moved after the last hull). The
        // recon owner registers nothing until the session.
        Stage::Units => {
            let Some(bindings) = cx.ids.units.take() else { return units::build(cx) };
            let mut groups = units::ItemGroups::default();
            let bound = units::build_bound(cx, &bindings, &mut groups);
            cx.ids.units = Some(bindings);
            let mut calls = bound?;
            let props_bound = std::mem::take(&mut cx.ids.props);
            let bound = props::build_bound(cx, &props_bound);
            cx.ids.props = props_bound;
            calls.extend(bound?);
            let hulls = std::mem::take(&mut cx.ids.hulls);
            let names = cx.ids.bodies_changed.then(|| cx.body_names());
            let finished = units::finish_bound(cx, &hulls, names.as_deref());
            cx.ids.hulls = hulls;
            calls.extend(finished?);
            calls.extend(recon::build(cx)?);
            calls.extend(items::placed(cx)?);
            Ok(calls)
        }
        Stage::Policies => weapon_policies::build(cx),
        // createNativeAI commits the roster before the script reads it.
        Stage::Ai => {
            let mut calls = ai::build(cx)?;
            calls.extend(teams::after_build(cx, true)?);
            Ok(calls)
        }
        Stage::LiveTail => battle::build(cx),
        // Content, kernels, the world owners, the session and the player
        // take no content rows: their calls are the sequence's own.
        Stage::Content | Stage::Kernels | Stage::World | Stage::Session | Stage::Player | Stage::Host => Ok(Vec::new()),
    }
}

/// WaterState's constructor as WorldOperation stamps it: presence, the
/// surface's own clock (0 at construction), its height and whether it bobs
/// (`(look.OscillationEnable?.[0] ?? 1) > 0`). The setup builder in the
/// content-native crate computes the same row for hosts outside the module.
pub fn setup(battle: &Battle) -> WorldTickSetup {
    let Some(water) = battle.world.get("water").filter(|w| w.truthy()) else { return WorldTickSetup::default(); };
    let flag = water.get("look").and_then(|look| look.get("OscillationEnable")).and_then(|enable| enable.at(0));
    let bob = match flag {
        None | Some(bfe_content::Value::Null) => true,
        Some(value) => bfe_content::js::to_number(Some(value)) > 0.0,
    };
    WorldTickSetup { water: true, water_time: 0.0, water_height: bfe_content::js::to_number(water.get("height")), water_bob: bob }
}
