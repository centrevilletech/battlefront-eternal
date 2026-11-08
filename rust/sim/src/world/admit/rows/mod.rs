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
use crate::gen::{F3ColdStage, WorldTickSetup};
use crate::store::{StrId, Strings};
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
/// recomputed, never invented.
#[derive(Clone, Debug, Default)]
pub struct Ids {
    /// The collision world's kernel ids (Stage::Kernels).
    pub kernel: Option<Installed>,
    /// Combat definition ids by class or weapon name (cmb_class, cmb_weapon_set).
    pub defs: Vec<(String, f64)>,
    /// Ordnance and item definition ids by name (ord_def, items_def).
    pub ordnance: Vec<(String, f64)>,
    pub items: Vec<(String, f64)>,
    /// Hull refs by vehicle name (f2_register) and their kernel body slots.
    pub vehicles: Vec<(String, f64)>,
    pub vehicle_bodies: Vec<(String, f64)>,
    /// Post, building and turret refs by index, the F4 owner handle of each
    /// prop instance, registry identities by unit key.
    pub posts: Vec<f64>,
    pub buildings: Vec<f64>,
    pub turrets: Vec<f64>,
    pub prop_owners: Vec<f64>,
    pub registry: Vec<(String, f64)>,
    /// Unit class ids and clip set ids as the units stage assigned them.
    pub classes: Vec<(String, f64)>,
    pub clips: Vec<(String, f64)>,
    /// The collision kernel's bodies in world order: the store's body ref and
    /// its kernel slot, and the body serial the units stage starts from.
    pub bodies: Vec<(f64, u32)>,
    pub body_serial: u32,
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

/// The shared context the builders are driven through.
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
    pub definitions: Definitions,
    /// The battle's string table as the builders see it: a copy of the
    /// world's at the start of admission, advanced by every intern in call
    /// order, so the ids the builders write are the ids the world has.
    pub strings: Strings,
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
        Self { content, battle: &content.battle, defs, files, options, definitions: Definitions::default(), strings, ids: Ids::default(), returned: Vec::new(), returned_rows: Vec::new() }
    }
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
        Stage::Fleet => {
            let mut calls = vehicle_pack::build(cx)?;
            calls.extend(fleet::build(cx)?);
            Ok(calls)
        }
        Stage::Units => {
            let mut calls = units::build(cx)?;
            calls.extend(props::build(cx)?);
            calls.extend(recon::build(cx)?);
            Ok(calls)
        }
        Stage::Policies => weapon_policies::build(cx),
        Stage::Ai => ai::build(cx),
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
