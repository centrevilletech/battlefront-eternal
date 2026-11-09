//! Cold Battle, F4Live, F4Tail, connection and roster marshaling. Dynamic
//! references and reservation counts come from earlier native owners; they
//! are explicit inputs so no content walk guesses a runtime slot or id.
use super::super::{AdmitError, Options, Stage};
use bfe_content::{battle::Battle as ContentBattle, js::to_number, value::Value};
use crate::gen::*;

/// The random-stream registry the owners share through `Cx::streams`.
pub mod streams;
/// game.js 3110-3126 headless: the viewpoint's adoption, the factory's
/// reservation and the prepared stock, between the world owners and F4Live.
pub mod stock;
/// F4Live, F4Battle and F4Tail construction with the AI's activation and the
/// local human's binding (game.js 3128-3160).
pub mod live;
/// The props' and hulls' bindings attachF3 reads, from the fleet stage's receipts.
pub mod bindings;
/// The animated props' rigs, births and grip classes (props.js AnimatedProp, f4-live.js bindAnimated).
pub mod animated;
/// The command posts and createNativeAI's host-object imports (commandposts.js, post-data.js).
pub mod posts;

pub const MAX_TEAM_SIZE: u32 = 32;

/// References are already qualified by the preceding construction stages.
/// next_id includes the viewpoint, while built includes warm corpse bodies.
/// The optional construction limit uses JS's nullish rule: zero is present.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    // The viewpoint is outside the warm factory list but still owns a slot.
    pub local: Option<Ref<Unit>>,
    // The human Character survives body replacement and keeps its generation.
    pub character: Option<Ref<Character>>,
    // Order is the canonical roster, not team/class sorting of the refs.
    pub characters: Vec<Ref<Character>>,
    // One id namespace includes soldiers, objectives, vehicles and props.
    pub next_id: u32,
    // Prepared inactive stock reuses a source slot and does not increase this.
    pub built: u32,
    // A supplied zero remains supplied, unlike an absent reservation.
    pub construction_limit: Option<u32>,
    // Future hull variants require mirrors before they can become living hulls.
    pub fleet: u32,
    pub prepared: u32,
    // One scratch is reused for each pad; point_capacity computes its maximum.
    pub point_capacity: u32,
    // The existing cursor is a double, so preserve its sign and exact bits.
    pub spawn_index: f64,
    // Stream registration may have occurred during earlier owner construction.
    pub random: u32,
    pub gameplay: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Capacity {
    pub export: &'static str,
    pub arguments: Vec<f64>,
}

/// The generated setup row is separate from the canonical Battle imported
/// immediately before it. The AI owner may already have updated that Battle.
pub fn setup(character_count: usize) -> Result<F4BattleSetup, AdmitError> {
    Ok(F4BattleSetup { capacity: checked(character_count, Stage::LiveTail)?.max(8 * MAX_TEAM_SIZE) })
}

/// Ports new Battle's initial authored tables. Native AI roster admission
/// can mutate this value before F4Battle's sole construction import.
pub fn initial(battle: &ContentBattle, characters: Vec<Ref<Character>>) -> Result<Battle, AdmitError> {
    from_mission(battle.world.get("mission"), battle.policy.team, characters)
}
pub fn from_mission(mission: Option<&Value>, own: f64, characters: Vec<Ref<Character>>) -> Result<Battle, AdmitError> {
    let source = mission.and_then(|m| m.get("teams")).and_then(Value::as_object);
    let foe = source.and_then(|teams| teams.entries().find(|(key, _)| to_number(Some(&Value::str(key))) != own))
        .map(|(key, _)| to_number(Some(&Value::str(key)))).unwrap_or(2.0);
    let mut out = Battle { own, foe, characters, victory_clock: Nullable::Null, countdown: Nullable::Null, ..Battle::default() };
    // BattleStats constructs the full null killer ring even before a kill.
    out.stats.ring.killers.resize(out.stats.ring.times.0.len(), None);
    if let Some(teams) = source {
        for (key, team) in teams.entries() {
            // IntObj only admits canonical array-index keys. Reject malformed
            // content here instead of reaching its internal indexing assert.
            let id = bfe_content::value::array_index(key).ok_or_else(|| content_error(Stage::LiveTail, "team key must be an array index"))? as f64;
            let optional = |name| team.get(name).filter(|v| !matches!(v, Value::Null)).map(|v| to_number(Some(v)));
            let bleed = team.get("bleed").filter(|v| !matches!(v, Value::Null)).map(|v| pairs(v, Stage::LiveTail)).transpose()?;
            out.teams.set(id, TeamDesc { units: optional("units"), reinforcements: optional("reinforcements"), bleed: bleed.clone() });
            let reinforcements = optional("reinforcements").unwrap_or(if id >= 3.0 { 2147483647.0 } else { 0.0 });
            out.reinforcements.set(id, reinforcements); out.start.set(id, reinforcements);
            out.team_ids.push(id); out.no_post.set(id, 20.0);
            let mut rules = bleed.unwrap_or_default();
            jsrt::sort::js_sort(&mut rules, &mut |a, b| a[0] - b[0]);
            out.bleed_rules.set(id, rules);
        }
    }
    Ok(out)
}

/// Ports F4Battle's f4_battle_capacity queries in mirror-binding order.
/// Query return values are native owner results, never guessed row offsets.
pub fn battle_capacities(character_count: usize) -> Vec<Capacity> {
    let mut out = vec![Capacity { export: "f4_battle_capacity", arguments: vec![0.0] }];
    out.extend((0..character_count).map(|_| Capacity { export: "f4_battle_capacity", arguments: vec![1.0] }));
    out
}

/// F4Live reserves future held-frame mirrors up to construction.limit, even
/// when the warm bodies have not joined the registry's insertion sequence.
pub fn live(facts: &Facts) -> Result<F4LiveSetup, AdmitError> {
    // The requested unit capacity also enters the fallback limit arithmetic;
    // computing the maximum later would reserve a different registry span.
    let requested = facts.built.max(64);
    let limit = match facts.construction_limit { Some(n) => n, None => add(facts.next_id, requested, Stage::LiveTail)? };
    Ok(F4LiveSetup { local: facts.local, unit_capacity: requested.max(limit), registry_capacity: limit,
        hull_capacity: add(facts.fleet, facts.prepared, Stage::LiveTail)? })
}

/// Ports F4Tail's setup. The roster lists exclude each army's hero and
/// classes deduplicates their union in first-use order, as the JS Set does.
/// fallbackPath is the first own property, including an empty path array.
pub fn tail(battle: &ContentBattle, options: &Options, facts: &Facts, intern: &mut impl FnMut(&str) -> StrId) -> Result<F4TailSetup, AdmitError> {
    tail_with_ready(&battle.world, battle.nav.as_ref(), options.enhanced, facts, &|name| {
        // PlayerBodies.sideOf takes the first side defining a class. Its
        // ready check tests that side's loaded model, not just the roster.
        let Some(side) = battle.sides.iter().find(|side| side.units.get(name).is_some_and(Value::truthy)) else { return false };
        let model = side.units.get(name).and_then(|def| def.get("model")).and_then(Value::as_str).unwrap_or("undefined");
        battle.bodies.iter().any(|(key, _)| key == &format!("{}/{model}", side.name))
    }, intern)
}
pub fn tail_from_content(world: &Value, nav: Option<&Value>, enhanced: bool, facts: &Facts, intern: &mut impl FnMut(&str) -> StrId) -> Result<F4TailSetup, AdmitError> {
    tail_with_ready(world, nav, enhanced, facts, &|_| true, intern)
}

pub fn tail_with_ready(world: &Value, nav: Option<&Value>, enhanced: bool, facts: &Facts, ready: &dyn Fn(&str) -> bool, intern: &mut impl FnMut(&str) -> StrId) -> Result<F4TailSetup, AdmitError> {
    // Tail includes bodies already built in addition to the registry cursor;
    // its count differs from live's fallback next_id + unitCapacity rule.
    let count = facts.construction_limit.unwrap_or(0).max(add(facts.next_id, facts.built, Stage::LiveTail)?);
    // Index zero remains empty. Human spawn choices index these by team,
    // rather than consuming a compact list of just the two armies.
    let mut rosters = vec![F4RosterClasses::default()];
    let mut classes = Vec::new();
    for team in 1..=2 {
        let team = world.get("mission").and_then(|m| m.get("teams")).and_then(|t| t.get(&team.to_string()));
        let names = bfe_content::mission::playable_classes(team);
        let roster: Vec<StrId> = names.iter().filter(|s| ready(s)).map(|s| intern(s)).collect();
        for name in &roster { if !classes.contains(name) { classes.push(*name); } }
        rosters.push(F4RosterClasses { classes: roster });
    }
    let path = world.get("spawns").and_then(Value::as_object).and_then(|o| o.entries().next()).map(|(_, v)| v);
    let fallback_path = match path.filter(|v| !matches!(v, Value::Null)) {
        Some(v) => path_points(v)?, None => vec![[0.0, 10.0, 0.0, 0.0]],
    };
    Ok(F4TailSetup { local: facts.local, character: facts.character, classic: true, enhanced,
        registry_capacity: count.max(64), victim_capacity: add(add(count, facts.fleet, Stage::LiveTail)?, facts.prepared, Stage::LiveTail)?.max(64),
        command_capacity: 16, point_capacity: facts.point_capacity, spawn_index: facts.spawn_index, fallback_path,
        random: facts.random, gameplay: facts.gameplay, boundary: boundary(nav)?, rosters, classes })
}

/// boundaryFromNav and Boundary keep the closing edge first, without
/// normalizing normals. Unary negation intentionally preserves negative zero.
pub fn boundary(nav: Option<&Value>) -> Result<BfConfig, AdmitError> {
    let refs = nav.and_then(|n| n.get("boundary")).and_then(|b| b.get("references")).and_then(Value::as_array).unwrap_or(&[]);
    let paths = nav.and_then(|n| n.get("paths")).and_then(Value::as_array).unwrap_or(&[]);
    let mut polygons = Vec::new();
    for reference in refs {
        let Some(Value::Number(index)) = reference.get("path") else { continue };
        if !index.is_finite() || *index < 0.0 || index.fract() != 0.0 || *index >= paths.len() as f64 { continue; }
        let Some(points) = paths[*index as usize].get("points").and_then(Value::as_array).filter(|p| !p.is_empty()) else { continue };
        let point = |p: &Value| -> [f64; 2] {
            if p.as_array().is_some() { [to_number(p.at(0)), to_number(p.at(2))] }
            else { [to_number(p.get("x")), to_number(p.get("z"))] }
        };
        let mut edges = Vec::with_capacity(points.len());
        for i in 0..points.len() {
            let a = point(&points[(i + points.len() - 1) % points.len()]); let b = point(&points[i]);
            edges.push([a[0], a[1], -(b[1] - a[1]), b[0] - a[0]]);
        }
        polygons.push(BfPolygon { edges });
    }
    Ok(BfConfig { polygons })
}

/// F4Connections receives the same cold Character capacity as F4Battle.
pub fn connections(character_count: usize) -> Result<u32, AdmitError> { Ok(setup(character_count)?.capacity) }

/// Native initial roster publication has no JS botNames callback. In that
/// case F4Roster supplies the complete names table with a null state. A
/// caller retaining a callback can supply its generated F4RosterNames state
/// directly on BoundRows instead of reconstructing a generator.
pub fn roster(battle: &ContentBattle, factions: &[f64], local: Option<Ref<Character>>, army_size: Option<f64>, intern: &mut impl FnMut(&str) -> StrId) -> Result<F4RosterSetup, AdmitError> {
    // createNativeAI publishes a SpawningMirror without JS callback or
    // names properties. F4Roster therefore transports empty fallback lists.
    let _ = battle;
    roster_from_names(None, factions, local, army_size, intern)
}
pub fn roster_from_names(names: Option<&Value>, factions: &[f64], local: Option<Ref<Character>>, army_size: Option<f64>, intern: &mut impl FnMut(&str) -> StrId) -> Result<F4RosterSetup, AdmitError> {
    if army_size.is_some_and(|v| !v.is_finite() || v.fract() != 0.0 || v < 1.0 || v > MAX_TEAM_SIZE as f64) {
        return Err(content_error(Stage::LiveTail, "invalid army size"));
    }
    let mut rows = Vec::with_capacity(factions.len());
    for faction in factions {
        // Local and unknown factions fall back to side zero for name lists;
        // the original faction still crosses the generated boundary intact.
        let side = if (1.0..=4.0).contains(faction) { *faction } else { 0.0 };
        let source = names.and_then(|n| n.get(&bfe_content::js::number_to_string(side)));
        let strings = |key: &str, intern: &mut dyn FnMut(&str) -> StrId| -> Result<Vec<StrId>, AdmitError> {
            source.and_then(|s| s.get(key)).and_then(Value::as_array).unwrap_or(&[]).iter().map(|v| {
                v.as_str().map(&mut *intern).ok_or_else(|| content_error(Stage::LiveTail, "bot name must be a string"))
            }).collect()
        };
        let first = strings("first", intern)?; let last = strings("last", intern)?;
        let local = names.and_then(|n| n.get("local")).filter(|v| !matches!(v, Value::Null)).map(|v| {
            v.as_str().map(&mut *intern).ok_or_else(|| content_error(Stage::LiveTail, "local bot name must be a string"))
        }).transpose()?;
        rows.push(F4RosterNames { faction: *faction, state: None, first, last, local });
    }
    Ok(F4RosterSetup { classic: true, has_ai: true, army_size, local, names: rows })
}

/// F4Tail binds ordinary posts, mobile posts, the human and spawning in
/// this order. Even the no-AI branch binds an empty Spawning mirror.
pub fn tail_capacities(posts: usize, mobile_posts: usize) -> Vec<Capacity> {
    let mut out = Vec::new();
    for (kind, count) in [(F4ObjectKind::Post, posts), (F4ObjectKind::MobilePost, mobile_posts), (F4ObjectKind::Human, 1), (F4ObjectKind::Spawning, 1)] {
        out.extend((0..count).map(|i| Capacity { export: "f4_tail_capacity", arguments: vec![kind as u8 as f64, i as f64] }));
    }
    out
}
fn pairs(value: &Value, stage: Stage) -> Result<Vec<[f64; 2]>, AdmitError> {
    value.as_array().ok_or_else(|| content_error(stage, "bleed table must be an array"))?.iter().map(|v| {
        let pair = v.as_array().filter(|p| p.len() >= 2).ok_or_else(|| content_error(stage, "bleed rule needs two values"))?;
        Ok([to_number(pair.first()), to_number(pair.get(1))])
    }).collect()
}
fn path_points(value: &Value) -> Result<Vec<[f64; 4]>, AdmitError> {
    value.as_array().ok_or_else(|| content_error(Stage::LiveTail, "spawn path must be an array"))?.iter().map(|v| {
        let p = v.as_array().ok_or_else(|| content_error(Stage::LiveTail, "spawn point must be an array"))?;
        Ok(std::array::from_fn(|i| to_number(p.get(i))))
    }).collect()
}
fn content_error(stage: Stage, message: &str) -> AdmitError { AdmitError::Content { stage, message: message.into() } }
fn checked(value: usize, stage: Stage) -> Result<u32, AdmitError> { u32::try_from(value).map_err(|_| content_error(stage, "capacity exceeds the simulation boundary")) }
fn add(a: u32, b: u32, stage: Stage) -> Result<u32, AdmitError> { a.checked_add(b).ok_or_else(|| content_error(stage, "capacity exceeds the simulation boundary")) }

/// The factory fills the Facts bundle before live/tail construction. The
/// context entry cannot infer registry identities from authored class names.
pub fn build(cx: &mut super::Cx) -> Result<super::Calls, AdmitError> {
    let facts = cx.ids.battle.clone().ok_or(AdmitError::Unavailable(Stage::LiveTail))?;
    let factions = cx.ids.factions.clone();
    build_counts(cx, &facts, &factions, cx.ids.posts.len(), cx.ids.mobile_posts.len())
}

/// These inputs are made after factory and AI construction has committed
/// identities. Keeping the rows typed lets that sequence use one interner
/// and drive each owner directly, without serializing through a JS facade.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundRows {
    pub live: F4LiveSetup,
    pub battle: F4BattleSetup,
    pub tail: F4TailSetup,
    pub connections: u32,
    pub roster: F4RosterSetup,
    pub battle_capacities: Vec<Capacity>,
    pub tail_capacities: Vec<Capacity>,
}

pub fn bound(battle: &ContentBattle, options: &Options, facts: &Facts, factions: &[f64], posts: usize, mobile_posts: usize,
    intern: &mut impl FnMut(&str) -> StrId) -> Result<BoundRows, AdmitError> {
    Ok(BoundRows {
        live: live(facts)?, battle: setup(facts.characters.len())?, tail: tail(battle, options, facts, intern)?,
        connections: connections(facts.characters.len())?, roster: roster(battle, factions, facts.character, None, intern)?,
        battle_capacities: battle_capacities(facts.characters.len()), tail_capacities: tail_capacities(posts, mobile_posts),
    })
}

impl BoundRows {
    /// Relative export order from game.js: live first, then the F4Battle
    /// super-constructor inside F4Tail, its tail records, then the server's
    /// connections and roster. Mirror handle reads interleave these calls;
    /// those handles are results of the owning sequence, not content rows.
    pub fn calls(&self) -> super::Calls {
        fn record(name: &'static str, words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> (Stage, &'static str, super::Call) {
            let mut call = super::Call { row: vec![0.0; words], ..Default::default() };
            put(&mut call.row, &mut call.heap);
            (Stage::LiveTail, name, call)
        }
        let scalar = |q: &Capacity| (Stage::LiveTail, q.export, super::Call { args: q.arguments.clone(), ..Default::default() });
        let mut calls = vec![record("f4_live_init", F4LiveSetup::WORDS, |row, heap| self.live.put(row, heap)),
            record("f4_battle_init", F4BattleSetup::WORDS, |row, heap| self.battle.put(row, heap))];
        calls.extend(self.battle_capacities.iter().map(scalar));
        calls.push(record("f4_tail_init", F4TailSetup::WORDS, |row, heap| self.tail.put(row, heap)));
        calls.extend(self.tail_capacities.iter().map(scalar));
        calls.push((Stage::Session, "f4_connections_init", super::Call { args: vec![self.connections as f64], ..Default::default() }));
        let mut roster = record("f4_roster_init", F4RosterSetup::WORDS, |row, heap| self.roster.put(row, heap));
        roster.0 = Stage::Session; calls.push(roster);
        calls
    }
}

/// F4Tail scans every prepared variant as well as the current vehicle.
/// Taking the maximum, rather than summing, sizes its reused point scratch;
/// a sparse pad list and absent command-class records contribute nothing.
pub fn point_capacity<'a>(initial: u32, pads: impl IntoIterator<Item = Option<&'a PsPad>>) -> Result<u32, AdmitError> {
    let mut capacity = initial;
    for pad in pads.into_iter().flatten() {
        for hull in pad.variants.iter().chain(pad.vehicle.iter()) {
            if let Some(record) = &hull.record { capacity = capacity.max(checked(record.points.len(), Stage::LiveTail)?); }
        }
    }
    Ok(capacity)
}

/// Explicit predecessor facts keep this builder independent of the shape
/// of Cx's retained identity tables. The sequence supplies these after the
/// factory and AI owners commit; recording comparisons supply their outputs.
pub fn build_explicit(cx: &mut super::Cx, facts: &Facts, factions: &[f64], posts: &[Ref<Post>], mobile_posts: &[Ref<MobilePost>]) -> Result<super::Calls, AdmitError> {
    build_counts(cx, facts, factions, posts.len(), mobile_posts.len())
}

pub fn build_counts(cx: &mut super::Cx, facts: &Facts, factions: &[f64], posts: usize, mobile_posts: usize) -> Result<super::Calls, AdmitError> {
    let battle = cx.battle; let options = cx.options;
    let mut interns = super::Call::default();
    let bound = bound(battle, options, facts, factions, posts, mobile_posts, &mut |text| cx.intern(&mut interns, text))?;
    let mut calls = bound.calls();
    if let Some((_, _, call)) = calls.iter_mut().find(|(_, name, _)| *name == "f4_tail_init") { call.strings = interns.strings; }
    calls.retain(|(stage, _, _)| *stage == Stage::LiveTail);
    Ok(calls)
}

/// Count-only callers can use the context's already assigned ordinary post
/// table; callers with explicit identities use build_explicit above.
pub fn build_with_facts(cx: &mut super::Cx, facts: &Facts, factions: &[f64], mobile_posts: usize) -> Result<super::Calls, AdmitError> {
    build_counts(cx, facts, factions, cx.ids.posts.len(), mobile_posts)
}

/// createMultiplayerWorld constructs connections, then F4Roster with the
/// local server's null armySize. Keep this boundary separate from LiveTail
/// because parking the viewpoint happens immediately before these calls.
pub fn session_with_facts(cx: &mut super::Cx, facts: &Facts, factions: &[f64]) -> Result<super::Calls, AdmitError> {
    let mut call = super::Call { row: vec![0.0; F4RosterSetup::WORDS], ..Default::default() };
    let battle = cx.battle;
    let input = roster(battle, factions, facts.character, cx.army_size, &mut |text| cx.intern(&mut call, text))?;
    input.put(&mut call.row, &mut call.heap);
    Ok(vec![(Stage::Session, "f4_connections_init", super::Call { args: vec![connections(facts.characters.len())? as f64], ..Default::default() }),
        (Stage::Session, "f4_roster_init", call)])
}

/// Session reads the same factory/AI receipt as the live and tail owners.
/// Keeping one bundle preserves qualified refs when player binding begins.
pub fn session(cx: &mut super::Cx) -> Result<super::Calls, AdmitError> {
    let facts = cx.ids.battle.clone().ok_or(AdmitError::Unavailable(Stage::Session))?;
    let factions = cx.ids.factions.clone();
    session_with_facts(cx, &facts, &factions)
}

/// F4Battle.bindCharacter asks for snapshot capacity once per new facade.
/// The roster's returned handle supplies its identity before this query.
pub fn character_capacity() -> super::Call {
    super::Call { args: vec![1.0], ..Default::default() }
}
