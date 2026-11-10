//! LocalServer's cold connection and SnapshotHost admission, inside the World.
//! Content and factory owners supply their existing identities; this module
//! never assigns a second definition namespace or creates a simulation facade.
//!
//! createMultiplayerWorld first parks its dormant viewpoint, then gives the
//! roster to the network connection. addPlayer reserves the Character before
//! staging its possible class bodies. F4 owns the actual spawn and generation;
//! the players owner receives that existing Character and its own mailbox.
//!
//! createSnapshotHost comes afterwards in createLocalServer. Its wire source
//! binds protocol names to native definitions once, and its publication binds
//! a transport client to a qualified Character. Those two identities must not
//! be inferred from a shared vector index, even for a one-player match.
//!
//! The registry descriptor is content-only. It contains no native pool keys,
//! addresses or executable definitions. Its paths refer to the first visit of
//! each shared combat object, rather than every place that object appears.
//!
//! Admission may allocate. Once the host is returned, the existing retained
//! players, wire and factory owners handle input, ticks and publication; no
//! loader or preparation callback from this module runs on their live path.
use super::{rows::{Call, Calls, Cx}, AdmitError, Handles, Stage};
use crate::{gen::*, world::owner::World, store::StrId};
use bfe_content::{battle::Battle, value::{Object, Value as ContentValue}};
use serde_json::{json, Value};
use std::rc::Rc;

fn error(message: impl Into<String>) -> AdmitError {
    AdmitError::Content { stage: Stage::Session, message: message.into() }
}
fn refused(call: &'static str) -> AdmitError { AdmitError::Refused { stage: Stage::Session, call: call.to_string() } }

/// The protocol's fixed keyboard/mouse registry (net/layout.js INPUT_CODES).
/// It is independent of a user's bindings; binding profile zero is empty.
pub fn input_codes() -> Vec<String> {
    let mut codes = Vec::with_capacity(104);
    codes.extend(('A'..='Z').map(|c| format!("Key{c}")));
    codes.extend((0..10).map(|n| format!("Digit{n}")));
    codes.extend((1..=12).map(|n| format!("F{n}")));
    codes.extend((0..10).map(|n| format!("Numpad{n}")));
    codes.extend(["Space", "Enter", "Escape", "Tab", "Backquote", "CapsLock", "ShiftLeft", "ShiftRight",
        "ControlLeft", "ControlRight", "AltLeft", "AltRight", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
        "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "Minus", "Equal", "BracketLeft",
        "BracketRight", "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash", "NumpadEnter", "NumpadAdd",
        "NumpadSubtract", "NumpadMultiply", "NumpadDivide", "NumpadDecimal", "Mouse0", "Mouse1", "Mouse2", "Mouse3",
        "Mouse4", "WheelUp", "WheelDown"].into_iter().map(str::to_string));
    codes
}

/// createEntityRegistry's unsorted inputs, in authored encounter order.
/// Keep duplicates here: wire_registry_run performs the UTF-16 sort/dedup.
#[derive(Clone, Debug, Default)]
pub struct RegistryInput {
    pub classes: Vec<String>,
    pub weapons: Vec<String>,
    pub posts: Vec<String>,
    pub combat: Vec<(String, ContentValue)>,
}
impl RegistryInput {
    /// registryContent restricts sides to the mission before walking combat
    /// objects. Cloning a Value retains its identity, including shared branches.
    pub fn build(battle: &Battle) -> Result<Self, AdmitError> {
        let fielded = battle.world.get("mission").and_then(|m| m.get("sides")).and_then(ContentValue::as_array);
        let mut names: Vec<String> = match fielded.filter(|a| !a.is_empty()) {
            Some(names) => names.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| error("invalid mission side"))).collect::<Result<_, _>>()?,
            None => battle.sides.iter().map(|s| s.name.clone()).collect(),
        };
        names.sort_by(|a, b| bfe_content::mission::utf16_cmp(a, b)); names.dedup();
        let mut sides = Vec::new(); let mut side_fx = Vec::new();
        for name in names {
            let Some(side) = battle.side(&name) else { continue };
            sides.push((Rc::from(name.as_str()), object(vec![("dir", ContentValue::str(&side.dir)),
                ("units", side.units.clone()), ("items", side.items.clone()), ("firePoints", side.fire_points.clone())])));
            let fx = if name == battle.side { battle.side_fx.as_ref() } else { side.fx.as_ref() };
            if let Some(fx) = fx.filter(|v| v.truthy()) { side_fx.push((Rc::from(name.as_str()), fx.clone())); }
        }
        let sides = ContentValue::Object(Rc::new(Object::from_written(sides)));
        // createCombatRegistry sorts keys at every object before descending.
        // Top-level insertion order therefore does not change combat ids;
        // the shallow records below must still retain their child aliases.
        let mut top = vec![("common", battle.common.clone()), ("effects", battle.effects.clone()),
            ("world", battle.world.clone()), ("sides", sides.clone()),
            ("sideFx", ContentValue::Object(Rc::new(Object::from_written(side_fx))))];
        if let Some(props) = &battle.props { top.push(("props", props.clone())); }
        if let Some(vehicles) = &battle.vehicle_def { top.push(("vehicles", vehicles.clone())); }
        let mut out = Self { combat: bfe_content::registry::combat_registry(&object(top)).map_err(error)?, ..Default::default() };
        for (_, side) in entries(Some(&sides)) {
            for (class, def) in entries(side.get("units")) {
                out.classes.push(class.to_string());
                for weapon in array(def.get("weapons")) { out.weapons.push(content_name(weapon.get("class"))?); }
            }
        }
        // Vehicle classes join the infantry names after fielded sides. The
        // native registry will sort them, but encounter order still governs
        // first string interning and therefore the exact call's numeric ids.
        for (class, def) in entries(battle.vehicle_def.as_ref().and_then(|v| v.get("classes"))) {
            out.classes.push(class.to_string());
            for seat in array(def.get("seats")) {
                for weapon in array(seat.get("weapons")) { out.weapons.push(content_name(weapon.get("def").and_then(|d| d.get("class")))?); }
            }
        }
        for instance in array(battle.props.as_ref().and_then(|p| p.get("instances"))) {
            if let Some(class) = instance.get("weapon").and_then(|w| w.get("class")).filter(|v| v.truthy()) {
                out.weapons.push(content_name(Some(class))?);
            }
        }
        // CommandPosts supplies fallback names; every authored vehicle pad
        // also reserves a mobile post, even while its hull is absent.
        for (i, desc) in array(battle.world.get("commandPosts")).iter().enumerate() {
            out.posts.push(match desc.get("name").filter(|n| n.truthy()) {
                Some(name) => content_name(Some(name))?, None => format!("#{i}"),
            });
        }
        for i in 0..array(battle.vehicle_def.as_ref().and_then(|v| v.get("spawns"))).len() { out.posts.push(format!("transport-{i}")); }
        Ok(out)
    }
}
fn object(entries: Vec<(&str, ContentValue)>) -> ContentValue {
    ContentValue::Object(Rc::new(Object::from_written(entries.into_iter().map(|(k, v)| (Rc::from(k), v)).collect())))
}
fn entries(value: Option<&ContentValue>) -> Vec<(&str, &ContentValue)> {
    value.and_then(ContentValue::as_object).map(|o| o.entries().collect()).unwrap_or_default()
}
fn array(value: Option<&ContentValue>) -> &[ContentValue] { value.and_then(ContentValue::as_array).unwrap_or(&[]) }
fn content_name(value: Option<&ContentValue>) -> Result<String, AdmitError> {
    value.and_then(ContentValue::as_str).filter(|n| !n.is_empty() && n.encode_utf16().count() <= 128)
        .map(str::to_string).ok_or_else(|| error("invalid entity registry name"))
}

/// Source definition handles supplied by the earlier owners, in their own
/// order. Equal-looking loadout objects deliberately remain separate ids.
#[derive(Clone, Debug, Default)]
pub struct DefinitionValues {
    pub weapons: Vec<ContentValue>,
    pub items: Vec<ContentValue>,
    pub remotes: Vec<ContentValue>,
    pub ordnance: Vec<ContentValue>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceDefinitions {
    pub weapons: Vec<WireStoreDefinition>,
    pub items: Vec<WireStoreDefinition>,
    pub remotes: Vec<WireStoreDefinition>,
    pub ordnance: Vec<WireStoreDefinition>,
}
impl SourceDefinitions {
    /// createStoreSource.content: an unfielded side gets no protocol id.
    /// Match combat definitions by Rc identity, never by a class-name guess.
    pub fn bind(input: &RegistryInput, registry: &Registry, values: &DefinitionValues) -> Result<Self, AdmitError> {
        let mut weapons = Vec::new();
        for (definition, value) in values.weapons.iter().enumerate() {
            let class = value.get("class").and_then(ContentValue::as_str);
            if let Some(id) = class.and_then(|name| registry.weapons.iter().position(|n| n == name)) {
                weapons.push(WireStoreDefinition { definition: definition as u32, id: id as u32 + 1 });
            }
        }
        // A side loaded for a URL can be absent from the mission. Omit its
        // cold definitions, as createStoreSource does, rather than assigning
        // them protocol zero or borrowing another side's equal-looking mine.
        let combat = |values: &[ContentValue]| -> Vec<WireStoreDefinition> {
            values.iter().enumerate().filter_map(|(definition, value)| {
                input.combat.iter().position(|(_, source)| source.same(value)).map(|id|
                    WireStoreDefinition { definition: definition as u32, id: id as u32 + 1 })
            }).collect()
        };
        Ok(Self { weapons, items: combat(&values.items), remotes: combat(&values.remotes), ordnance: combat(&values.ordnance) })
    }
}

/// The immutable registry descriptor, after native validation and ordering.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registry {
    pub classes: Vec<String>, pub weapons: Vec<String>, pub posts: Vec<String>, pub combat_paths: Vec<String>,
}
impl Registry {
    fn table(names: Vec<WrName>, cx: &Cx) -> Result<Vec<String>, AdmitError> {
        names.into_iter().map(|n| n.value.map(|id| String::from_utf16_lossy(cx.strings.units(id)))
            .ok_or_else(|| error("invalid native registry result"))).collect()
    }
    /// Descriptor field order matters to JSON.stringify's registry check.
    pub fn descriptor(&self) -> Value {
        json!({"combatDefinitions": self.combat_paths, "contentEpoch": 1, "classes": pairs(&self.classes),
            "weapons": pairs(&self.weapons), "posts": pairs(&self.posts), "inputCodes": input_codes(), "bindingProfiles": [[0, {}]]})
    }
    pub fn descriptor_text(&self) -> Result<String, AdmitError> {
        Ok(format!("{{\"combatDefinitions\":{},\"contentEpoch\":1,\"classes\":{},\"weapons\":{},\"posts\":{},\"inputCodes\":{},\"bindingProfiles\":[[0,{{}}]]}}",
            encode(&json!(self.combat_paths))?, encode(&json!(pairs(&self.classes)))?, encode(&json!(pairs(&self.weapons)))?, encode(&json!(pairs(&self.posts)))?, encode(&json!(input_codes()))?))
    }
}
fn pairs(names: &[String]) -> Vec<(usize, &str)> { names.iter().enumerate().map(|(i, n)| (i + 1, n.as_str())).collect() }
fn encode(value: &Value) -> Result<String, AdmitError> { serde_json::to_string(value).map_err(|e| error(e.to_string())) }

/// LocalServer's retained connection, after the F4 spawn command returns.
/// Player ids are Unit ids on the wire; mailbox/player keys are separate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalPlayer {
    pub mailbox: u32, pub player: u32, pub character: Option<F4Handle>,
    pub unit_id: u32, pub generation: f64, pub team: f64, pub class_name: String, pub team_select: bool, pub tick: f64,
}

/// The local host receives these keys and the exact two text event packets.
/// `calls` also lets an admission comparison check this stage independently.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub handles: Handles, pub player_id: f64, pub registry: Value, pub packets: [String; 2], pub calls: Calls,
    /// The construction owner's stages, retained for Rows::cold_stages.
    pub cold_stages: Vec<F3ColdStage>,
}
impl Session {
    /// createLocalServer.welcome: registry before welcome, with the source's
    /// property order retained even when serde_json uses sorted object maps.
    pub fn welcome(registry: &Registry, player: &LocalPlayer) -> Result<[String; 2], AdmitError> {
        let descriptor = registry.descriptor_text()?;
        let class = encode(&json!(player.class_name))?;
        let number = |n: f64| if n.is_finite() { bfe_content::js::number_to_string(n) } else { "null".into() };
        Ok([format!("{{\"type\":\"entity-registry\",\"registry\":{descriptor}}}"),
            format!("{{\"type\":\"welcome\",\"epoch\":1,\"snapshotMode\":\"snapshot-delta-v1\",\"unitId\":{},\"playerId\":{},\"generation\":{},\"team\":{},\"className\":{class},\"spectator\":false,\"teamSelect\":{},\"tick\":{}}}",
                player.unit_id, player.unit_id, number(player.generation), number(player.team), player.team_select, number(player.tick))])
    }
}

// Calls enter through the same generated serializers as callArgs. Native
// owners may intern additional status names while constructing; copy that
// advanced table back before the next builder assigns a string id.
fn block(words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> Call {
    let mut call = Call { row: vec![0.0; words], ..Default::default() };
    put(&mut call.row, &mut call.heap); call
}
/// A generated record an owner answered in its output block: the block
/// header, the row, then the heap at the header's offset.
fn block_result<T>(words: &[f64], take: impl FnOnce(&[f64], &[f64]) -> T) -> T {
    let heap_at = (words[BlockHeader::HEAP] as usize).min(words.len());
    take(&words[BlockHeader::WORDS..], &words[heap_at..])
}
fn scalar(args: &[f64]) -> Call { Call { args: args.to_vec(), ..Default::default() } }
fn perform<R>(world: &mut World, cx: &mut Cx, calls: &mut Calls, name: &'static str, call: Call,
    body: impl FnOnce(&mut World) -> Result<R, AdmitError>) -> Result<R, AdmitError>
{
    super::sequence::args(world, &call)?;
    let result = body(world)?;
    cx.strings = world.sim.strings.clone();
    calls.push((Stage::Session, name, call));
    Ok(result)
}
fn key(world: &mut World, cx: &mut Cx, calls: &mut Calls, name: &'static str, call: Call,
    body: impl FnOnce(&mut World) -> usize) -> Result<usize, AdmitError>
{
    let result = perform(world, cx, calls, name, call, |world| {
        let value = body(world); if value == 0 { Err(refused(name)) } else { Ok(value) }
    })?;
    cx.returned.push((name, result as f64)); Ok(result)
}
fn intern_names(cx: &mut Cx, call: &mut Call, names: &[String]) -> Vec<WrName> {
    names.iter().map(|name| WrName { value: Some(cx.intern(call, name)) }).collect()
}

/// createEntityRegistry through wire_registry_run, rather than a parallel
/// Rust ordering algorithm. Outputs become the only protocol name tables.
pub fn create_registry(world: &mut World, cx: &mut Cx, input: &RegistryInput, calls: &mut Calls) -> Result<Registry, AdmitError> {
    let mut call = scalar(&[0.0]);
    let codes = intern_names(cx, &mut call, &input_codes());
    let row = WrInputs { epoch: 1.0, epoch_valid: true, codes_valid: true, profiles_valid: true, codes,
        profiles: vec![WrProfile { id: 0.0, id_valid: true, valid: true, bindings: Vec::new() }] };
    call.row.resize(WrInputs::WORDS, 0.0); row.put(&mut call.row, &mut call.heap);
    perform(world, cx, calls, "wire_registry_run", call, |world| {
        crate::wire::registry::wire_registry_run_in(world, 0).ok_or_else(|| refused("wire_registry_run"))?;
        let (row, heap) = world.sim.calls.args(); let result = WrResult::take(row, heap);
        if result.error.is_some() { return Err(refused("wire_registry_run")); } Ok(())
    })?;
    // The JS adapter calls native.table three times, putting each list in
    // WrTables.classes. Combining the lists into one generated record would
    // give equal maps but a different admission trace and intern sequence.
    let mut tables = Vec::new();
    for names in [&input.classes, &input.weapons, &input.posts] {
        let mut call = scalar(&[1.0]);
        let row = WrTables { classes: intern_names(cx, &mut call, names), ..Default::default() };
        call.row.resize(WrTables::WORDS, 0.0); row.put(&mut call.row, &mut call.heap);
        let result = perform(world, cx, calls, "wire_registry_run", call, |world| {
            crate::wire::registry::wire_registry_run_in(world, 1).ok_or_else(|| refused("wire_registry_run"))?;
            let (row, heap) = world.sim.calls.args(); let result = WrResult::take(row, heap);
            if result.error.is_some() { return Err(refused("wire_registry_run")); } Ok(result)
        })?;
        tables.push(Registry::table(result.classes, cx)?);
    }
    let mut tables = tables.into_iter();
    let registry = Registry { classes: tables.next().unwrap_or_default(), weapons: tables.next().unwrap_or_default(),
        posts: tables.next().unwrap_or_default(), combat_paths: input.combat.iter().map(|(path, _)| path.clone()).collect() };
    if registry.classes.is_empty() { return Err(error("entity registry has no classes")); }
    Ok(registry)
}

fn store_setup(cx: &mut Cx, registry: &Registry, definitions: &SourceDefinitions, player: &LocalPlayer) -> Result<Call, AdmitError> {
    let mut call = Call::default();
    let unbuilt = cx.intern(&mut call, "unbuilt"); let built = cx.intern(&mut call, "built"); let destroyed = cx.intern(&mut call, "destroyed");
    let mut names = |values: &[String]| values.iter().enumerate().map(|(i, name)|
        WireStoreName { id: i as u32 + 1, name: cx.intern(&mut call, name) }).collect();
    let classes = names(&registry.classes); let weapon_names = names(&registry.weapons); let posts = names(&registry.posts);
    // Capture's item slots cover both gameplay pools, independently of the
    // number of items currently alive. A mine born later must not force a
    // source reallocation or change the protocol identity of a prior sample.
    let pools = cx.battle.world.get("mission").and_then(|m| m.get("pools"));
    let capacity = |name: &str, fallback| pools.and_then(|p| p.get(name)).filter(|v| !matches!(v, ContentValue::Null))
        .map(|v| bfe_content::js::to_number(Some(v))).unwrap_or(fallback);
    let items = capacity("mine", 50.0) + capacity("powerupitem", 25.0);
    if !items.is_finite() || items < 0.0 || items > 256.0 || items.fract() != 0.0 { return Err(error("invalid item capture capacity")); }
    let seats = entries(cx.battle.vehicle_def.as_ref().and_then(|v| v.get("classes"))).into_iter()
        .map(|(_, value)| array(value.get("seats")).len()).max().unwrap_or(0);
    let row = WireStoreSetup { unbuilt, built, destroyed, classes, weapon_names, posts,
        weapons: definitions.weapons.clone(), items: definitions.items.clone(), remotes: definitions.remotes.clone(), ordnance: definitions.ordnance.clone(),
        peers: vec![WireStorePeer { id: 1, character: player.character.clone(), mailbox: player.mailbox }],
        item_capacity: items as u32, remote_capacity: 4096, entity_capacity: 256, record_capacity: 512, seat_capacity: seats as u32 };
    call.row.resize(WireStoreSetup::WORDS, 0.0); row.put(&mut call.row, &mut call.heap);
    Ok(call)
}

/// createSnapshotHost + addClient for the already spawned local player.
/// Clock facts are supplied by the host, never read from a platform clock.
/// Content can extend at admission, so source configuration runs again at
/// addClient even when this is the first and only peer.
pub fn create_host(world: &mut World, cx: &mut Cx, player: &LocalPlayer, input: &RegistryInput,
    values: &DefinitionValues, at: f64) -> Result<Session, AdmitError>
{
    let mut calls = Calls::new();
    let runtime = key(world, cx, &mut calls, "wire_create", scalar(&[]), crate::wire::module::wire_create_in)?;
    let presentation = key(world, cx, &mut calls, "wire_presentation_create", scalar(&[]), crate::wire::presentation::wire_presentation_create_in)?;
    let host = key(world, cx, &mut calls, "wire_host_create", scalar(&[1.0, f64::INFINITY, f64::INFINITY, 1.0]),
        |world| crate::wire::host::wire_host_create_in(world, 1, f64::INFINITY, f64::INFINITY, 1.0))?;
    let stagger = key(world, cx, &mut calls, "wire_stagger_create", scalar(&[]), crate::wire::stagger::wire_stagger_create_in)?;
    let registry = create_registry(world, cx, input, &mut calls)?;
    // Capture subscribes before any publication can observe a shot. The
    // combat queue shares the simulation's ordered event namespace; creating
    // another journal here would reorder immediate launch/impact callbacks.
    let shots = key(world, cx, &mut calls, "wire_shots_create", scalar(&[256.0]), |world| crate::wire::shots::wire_shots_create_in(world, 256.0))?;
    let combat = key(world, cx, &mut calls, "wire_combat_create", scalar(&[]), crate::wire::combat::wire_combat_create_in)?;
    let definitions = SourceDefinitions::bind(input, &registry, values)?;
    // bindSpawnSelection writes a valid empty selection before its peer row.
    let selection = WireSpawnFacts { valid: true, ..Default::default() };
    let mut call = block(WireSpawnFacts::WORDS, |row, heap| selection.put(row, heap)); call.args = vec![player.mailbox as f64];
    perform(world, cx, &mut calls, "input_mailbox_spawn", call, |world| {
        crate::input::module::input_mailbox_spawn_in(world, player.mailbox); Ok(())
    })?;
    let call = store_setup(cx, &registry, &definitions, player)?;
    let source = key(world, cx, &mut calls, "wire_store_source_create", call, crate::wire::store_source::wire_store_source_create_in)?;
    key(world, cx, &mut calls, "wire_shots_attach", scalar(&[shots as f64]), |world| crate::wire::shots::wire_shots_attach_in(world, shots) as usize)?;
    let seats = world.wire.as_deref().and_then(|w| w.sources.get(source)).map(|s| s.capture.seat_capacity());
    key(world, cx, &mut calls, "wire_bind_store", scalar(&[runtime as f64, source as f64]),
        |world| crate::wire::module::wire_bind_store_in(world, runtime, seats) as usize)?;
    // All sibling handles are qualified pool keys in this World. Binding
    // happens only after the source and snapshot ring accepted their limits,
    // so publication never sees a half-attached canonical producer.
    let publication = key(world, cx, &mut calls, "wire_publish_create", scalar(&[runtime as f64, source as f64, stagger as f64, shots as f64, host as f64, combat as f64]),
        |world| crate::wire::publish_module::wire_publish_create_in(world, runtime, source, stagger, shots, host, combat))?;
    let mut call = store_setup(cx, &registry, &definitions, player)?; call.args = vec![source as f64];
    key(world, cx, &mut calls, "wire_store_source_configure", call, |world| crate::wire::store_source::wire_store_source_configure_in(world, source) as usize)?;
    perform(world, cx, &mut calls, "wire_host_op", scalar(&[host as f64, 0.0, 1.0, at, 0.0]), |world| {
        crate::wire::host::wire_host_op_in(world, host, 0, 1, at, 0.0); Ok(())
    })?;
    let edges = key(world, cx, &mut calls, "wire_edges_create", scalar(&[]), crate::wire::edges::wire_edges_create_in)?;
    // Both reliable lanes turn receipts on before reset reads their serials.
    for _ in 0..2 {
        perform(world, cx, &mut calls, "players_outcomes_enable", scalar(&[player.player as f64]), |world| {
            crate::players::module::players_outcomes_enable_in(world, player.player); Ok(())
        })?;
    }
    // LaneView reads the existing reliable serials through players_lanes.
    // They are normally zero here, but reset must read the owner rather than
    // assuming it, and these are observable cold calls in the source path.
    let mut received = [0.0; 2];
    for lane in 0..2 {
        received[lane] = perform(world, cx, &mut calls, "players_lanes", scalar(&[player.player as f64, lane as f64, 0.0]), |world| {
            crate::players::module::players_lanes_in(world, player.player, lane as u32, 0).ok_or_else(|| refused("players_lanes"))?;
            let owner = &world.input.players.lanes;
            let (row, heap) = owner.read(0); let result = PlayerLanes::take(row, heap);
            Ok(if lane == 0 { result.actions_received } else { result.fire_received })
        })?;
    }
    let mut call = scalar(&[edges as f64]); let status = cx.intern(&mut call, "");
    let reset = WeCall { epoch: 1.0, generation: player.generation, capacity: 256.0, action_received: received[0], fire_received: received[1], status, ..Default::default() };
    call.row.resize(WeCall::WORDS, 0.0); reset.put(&mut call.row, &mut call.heap);
    perform(world, cx, &mut calls, "wire_edges_run", call, |world| {
        crate::wire::edges::wire_edges_run_in(world, edges).ok_or_else(|| refused("wire_edges_run"))?; Ok(())
    })?;
    key(world, cx, &mut calls, "wire_publish_bind", scalar(&[publication as f64, 1.0, 1.0, edges as f64]),
        |world| crate::wire::publish_module::wire_publish_bind_in(world, publication, 1, 1, edges) as usize)?;
    for (field, value) in [(3, 1.0), (2, 1.0)] {
        perform(world, cx, &mut calls, "wire_host_set", scalar(&[host as f64, 1.0, field as f64, value]), |world| {
            crate::wire::host::wire_host_set_in(world, host, 1, field, value); Ok(())
        })?;
    }
    let started = perform(world, cx, &mut calls, "wire_host_get", scalar(&[host as f64, 1.0, 2.0]), |world| {
        Ok(crate::wire::host::wire_host_get_in(world, host, 1, 2))
    })?;
    if started != 0.0 {
        perform(world, cx, &mut calls, "wire_host_set", scalar(&[host as f64, 1.0, 6.0, at]), |world| {
            crate::wire::host::wire_host_set_in(world, host, 1, 6, at); Ok(())
        })?;
    }
    let packets = Session::welcome(&registry, player)?;
    Ok(Session { handles: Handles { mailbox: player.mailbox, player: player.player, publication: publication as u32, host: host as u32,
        edges: edges as u32, client: 1, combat: combat as u32, presentation: presentation as u32, store_source: source as u32 }, player_id: player.unit_id as f64, registry: registry.descriptor(), packets, calls, cold_stages: Vec::new() })
}

/// The shared random bindings and recon identity assigned by earlier owners.
/// These are bindings, not seeds guessed from the player's class or slot.
#[derive(Clone, Debug)]
pub struct PlayerBindings {
    pub combat_random: u32, pub weapon_random: u32, pub recon_key: Option<u32>, pub recon_seed: u32,
    /// F3Sim.change's retained mirror fields, independent of its shared bank.
    pub command_vector: Vec3,
    pub on_shield: f64,
}
impl Default for PlayerBindings {
    fn default() -> Self {
        Self { combat_random: NO_RANDOM_STREAM, weapon_random: NO_RANDOM_STREAM, recon_key: None, recon_seed: 0,
            command_vector: [0.0; 3], on_shield: f64::NAN }
    }
}

// The connection's authored choices and the preceding owners' returned refs
// meet here. Keep these serializers shared with the recorded-row fixtures;
// their inputs carry identities, never the target call's encoded words.
fn reserve_call(cx: &mut Cx, team: f64) -> Call {
    let mut call = Call::default();
    let player_id = PlayerId::Str(cx.intern(&mut call, "local"));
    // addPlayer(id, {name = 'Player'}): the local server passes the name the
    // page's session was opened with.
    let name = cx.options.name.as_deref().unwrap_or("Player");
    let name = cx.intern(&mut call, name);
    let reserve = F4RosterCommand { op: RoOperation::Reserve, target: None, team, player_id, name };
    call.row.resize(F4RosterCommand::WORDS, 0.0); reserve.put(&mut call.row, &mut call.heap); call
}
fn binding_call(bindings: &PlayerBindings, character: crate::store::Ref<Character>) -> Call {
    let binding = F3PlayerBinding { character, body: None, local: false,
        combat_random: bindings.combat_random, weapon_random: bindings.weapon_random,
        recon_key: bindings.recon_key, recon_seed: bindings.recon_seed };
    block(F3PlayerBinding::WORDS, |row, heap| binding.put(row, heap))
}
fn connection_call(cx: &mut Cx, op: F4ConnectionOp, target: F4Handle, class_name: Option<&str>) -> Call {
    let mut call = Call::default();
    let class_name = class_name.map(|name| cx.intern(&mut call, name));
    let command = F4ConnectionCommand { op, target, class_name, ordinal: 1.0, ..Default::default() };
    call.row.resize(F4ConnectionCommand::WORDS, 0.0); command.put(&mut call.row, &mut call.heap); call
}

/// Construction seams in createMultiplayerWorld and addPlayer. The owning
/// sequence prepares its F4 session rows only after the viewpoint is parked;
/// otherwise the roster can retire the original Character too early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepare<'a> {
    /// f4_connections_init/f4_roster_init from the battle owner.
    Session,
    /// UnitFactory.prepare: load/validate this class's retained body files.
    Class(&'a str),
    /// F3Factory.prepare: construct and stage one inactive body.
    Stock(&'a str),
}

/// addPlayer's class loader and body preparation are supplied by their
/// owners. Their callback receives typed seams, never a fabricated export
/// name or a guessed id. Every returned call stays in construction order.
pub fn add_player(world: &mut World, cx: &mut Cx, bindings: &PlayerBindings, at: f64,
    mut prepare: impl FnMut(&mut World, &mut Cx, Prepare<'_>, &mut Calls) -> Result<(), AdmitError>)
    -> Result<(LocalPlayer, Calls), AdmitError>
{
    use crate::{battle::{f4_core, f4_roster}, units::{f3, f3_construction}};
    if !f3::active_in(&world.units) { return Err(refused("f3_init")); }
    let viewpoint = f3::registered_viewpoint_in(&world.units).ok_or_else(|| refused("viewpoint"))?;
    let view = world.sim.store.unit.get(viewpoint).ok_or_else(|| refused("viewpoint"))?;
    let p = view.position(); let position = [p.x, p.y, p.z]; let yaw = match &view.soldier { UnitBody::Soldier(s) => s.aim_yaw, UnitBody::Droideka(d) => d.aim_yaw };
    let key_view = F3UnitKey::of(viewpoint, view);
    let mut calls = Calls::new();
    let mut mirror = F3Change { vector: bindings.command_vector, on_shield: bindings.on_shield, ..Default::default() };
    perform(world, cx, &mut calls, "players_init", scalar(&[]), |world| { crate::players::module::players_init_in(world); Ok(()) })?;
    // createNetworkRemotes(maxPlayers) changes only owner capacity; its immutable
    // class content was registered earlier by the unit/recon owner.
    let capacity = cx.connection_capacity;
    perform(world, cx, &mut calls, "f3_recon_owners", scalar(&[capacity as f64]), |world| {
        crate::recon::module::f3_recon_owners_in(world, capacity); Ok(())
    })?;
    // parkViewpoint clears human/body and human-spawn flags natively. Keep
    // the previous position/yaw: LocalServer places the new connection there.
    change(world, cx, &mut calls, &mut mirror, F3Change { key: key_view, action: F3Action::Unspawn, flag: true, ..Default::default() })?;
    prepare(world, cx, Prepare::Session, &mut calls)?;
    cx.strings = world.sim.strings.clone();
    // The callback has just initialized the retained connections and roster.
    // Everything after this boundary belongs to the player's admission;
    // the later snapshot host still runs after the player's body is placed.
    let first_player = calls.len();
    let class_name = cx.battle.policy.class_name.clone(); let team = cx.battle.policy.team;
    let mission = cx.battle.world.get("mission");
    let roster = |team: f64| {
        let key = bfe_content::js::number_to_string(team);
        bfe_content::mission::playable_classes(mission.and_then(|m| m.get("teams")).and_then(|t| t.get(&key)))
    };
    if !roster(team).iter().any(|name| name == &class_name) { return Err(error("invalid player class")); }
    let mut classes = roster(1.0); classes.extend(roster(2.0));
    prepare_class(cx, &class_name)?;
    prepare(world, cx, Prepare::Class(&class_name), &mut calls)?;
    cx.strings = world.sim.strings.clone();
    let call = reserve_call(cx, team);
    let character = perform(world, cx, &mut calls, "f4_roster_command", call, |world| {
        // The roster answers in its own output block (header, row, heap),
        // not in the argument exchange the command was read from.
        let words = f4_roster::f4_roster_command_in(world).ok_or_else(|| refused("f4_roster_command"))?;
        let result = block_result(words, F4RosterResult::take);
        if result.full { return Err(error("team full")); } result.handle.ok_or_else(|| refused("f4_roster_command"))
    })?;
    let mailbox = create_mailbox(world, cx, &mut calls)?;
    for class in classes {
        prepare_class(cx, &class)?;
        prepare(world, cx, Prepare::Class(&class), &mut calls)?;
        cx.strings = world.sim.strings.clone();
        prepare(world, cx, Prepare::Stock(&class), &mut calls)?;
        // Constructors can intern native status names inside their bodies.
        // Subsequent classes and the wire source must see that advanced table.
        cx.strings = world.sim.strings.clone();
    }
    // The roster result carries the Character life, not just its slab slot.
    // Resolve that qualified handle before storing the factory binding. A
    // recycled bot slot cannot silently receive this connection's streams.
    let character_ref = f4_core::resolve_character_ref(&world.battle, &world.sim.store, &character).ok_or_else(|| refused("roster character"))?;
    let call = binding_call(bindings, character_ref);
    perform(world, cx, &mut calls, "f3_factory_player_bind", call, |world| { f3_construction::f3_factory_player_bind_in(world); Ok(()) })?;
    let call = connection_call(cx, F4ConnectionOp::Admit, character.clone(), Some(&class_name));
    connection(world, cx, &mut calls, call)?;
    let call = connection_call(cx, F4ConnectionOp::Spawn, character.clone(), Some(&class_name));
    let result = connection(world, cx, &mut calls, call)?;
    // F4's accepted spawn is the authority for both body identity and life.
    // The spawn-screen policy removes that body afterwards; it still keeps
    // the same connection record and sends that body's identity in welcome.
    let unit_ref = result.unit.ok_or_else(|| refused("no eligible spawn"))?;
    let auto = !cx.battle.policy.screen;
    if !auto {
        let call = connection_call(cx, F4ConnectionOp::Unspawn, character.clone(), None);
        connection(world, cx, &mut calls, call)?;
    }
    // HumanSpawn.canSwitchTeams is the mission's TEAMS: PLAYER SELECT.
    let team_select = world.sim.store.human_spawn.can_switch_teams;
    let setup = PlayerSetup { mailbox, character: Some(character.clone()), spectator: false, reliable_fire: true,
        separate_actions: true, team_select, auto_respawn: false, ordinal: 1.0, team, recon_key: bindings.recon_key, at };
    let player = create_player(world, cx, &setup, &mut calls)?;
    perform(world, cx, &mut calls, "players_auto", scalar(&[player as f64, u8::from(auto) as f64]), |world| {
        crate::players::module::players_auto_in(world, player, u32::from(auto)); Ok(())
    })?;
    let unit = world.sim.store.unit.get(unit_ref).ok_or_else(|| refused("player body"))?;
    let generation = unit.generation; let body_key = F3UnitKey::of(unit_ref, unit);
    if auto { change(world, cx, &mut calls, &mut mirror, F3Change { key: body_key, action: F3Action::BodySpawn,
        vector: position, value: yaw, ..Default::default() })?; }
    // The body already owns its gameplay reset. The JS presentation.reset
    // clears drawn interpolation only and creates no world owner or packet.
    for (stage, _, _) in &mut calls[first_player..] { *stage = Stage::Player; }
    Ok((LocalPlayer { mailbox, player, character: Some(character), unit_id: unit_ref.slot, generation, team,
        class_name, team_select, tick: 0.0 }, calls))
}
fn connection(world: &mut World, cx: &mut Cx, calls: &mut Calls, call: Call) -> Result<F4ConnectionView, AdmitError> {
    perform(world, cx, calls, "f4_connection_command", call, |world| {
        let words = crate::battle::f4_connections::f4_connection_command_in(world).ok_or_else(|| refused("f4_connection_command"))?;
        let result = block_result(words, F4ConnectionView::take);
        if !result.accepted { return Err(refused("f4_connection_command")); } Ok(result)
    })
}
fn change_call(mirror: &mut F3Change, mut change: F3Change) -> Call {
    // F3's scalar commands live in their retained generated row, outside
    // callArgs. Its receipt is a direct-bank row, as the recorder labels it.
    // F3Sim.begin resets scalars and optional owners, but deliberately leaves
    // vector and onShield in its reusable mirror. Preserve even unused bits,
    // including NaN and signed zero, for the direct command-bank boundary.
    if change.action != F3Action::BodySpawn { change.vector = mirror.vector; }
    change.on_shield = mirror.on_shield;
    change.attacker = Attacker::Null;
    *mirror = change.clone();
    block(F3Change::WORDS, |row, heap| change.put(row, heap))
}
fn change(world: &mut World, cx: &mut Cx, calls: &mut Calls, mirror: &mut F3Change, change: F3Change) -> Result<(), AdmitError> {
    let call = change_call(mirror, change);
    crate::units::f3::command_row_in(&mut world.units).copy_from_slice(&call.row);
    // The recorder labels this generated direct bank as input metadata, rather than
    // sim.calls. Keep the row in the receipt without copying it into callArgs.
    let accepted = crate::units::f3::f3_command_in(world);
    if accepted == 0 { return Err(refused("f3_command")); }
    cx.strings = world.sim.strings.clone(); cx.returned.push(("f3_command", accepted as f64));
    calls.push((Stage::Session, "f3_command", call));
    Ok(())
}

/// The whole local server after content/owner construction. The callback is
/// UnitFactory.prepare, supplied by the body owner at the class seam above.
/// Native clocks start at the supplied monotonic origin; transport stamps
/// later publications independently.
pub fn run_with(world: &mut World, cx: &mut Cx, bindings: &PlayerBindings, at: f64,
    prepare: impl FnMut(&mut World, &mut Cx, Prepare<'_>, &mut Calls) -> Result<(), AdmitError>) -> Result<Session, AdmitError>
{
    let (player, mut calls) = add_player(world, cx, bindings, at, prepare)?;
    let input = RegistryInput::build(cx.battle)?;
    // Preparing the roster can register new item/weapon/recon definitions.
    // Read the shared tables after addPlayer, exactly when SnapshotHost binds
    // its source, so late-prepared classes retain their actual definition ids.
    let values = DefinitionValues::from_context(cx)?;
    let mut session = create_host(world, cx, &player, &input, &values, at)?;
    for (_, name, call) in &calls {
        if *name == "f3_factory_stage" {
            if call.row.len() < F3ColdStage::WORDS { return Err(refused("f3_factory_stage")); }
            session.cold_stages.push(F3ColdStage::take(&call.row, &call.heap));
        }
    }
    calls.append(&mut session.calls); session.calls = calls;
    Ok(session)
}

/// InputMailbox(1, 256): the connection owns its queue before players_create
/// can reference it. The same helper admits the very first local peer.
pub fn create_mailbox(world: &mut World, cx: &mut Cx, calls: &mut Calls) -> Result<u32, AdmitError> {
    let mut call = Call::default();
    let codes = input_codes().iter().map(|code| cx.intern(&mut call, code)).collect();
    let setup = InputMailboxSetup { epoch: 1.0, capacity: 256.0, codes };
    call.row.resize(InputMailboxSetup::WORDS, 0.0); setup.put(&mut call.row, &mut call.heap);
    key(world, cx, calls, "input_mailbox_create", call, |world| crate::input::module::input_mailbox_create_in(world) as usize).map(|key| key as u32)
}

/// players_create refuses a missing or foreign mailbox. A fresh admission
/// never calls players_link, whose clearing semantics belong to reconnects.
pub fn create_player(world: &mut World, cx: &mut Cx, setup: &PlayerSetup, calls: &mut Calls) -> Result<u32, AdmitError> {
    let call = block(PlayerSetup::WORDS, |row, heap| setup.put(row, heap));
    key(world, cx, calls, "players_create", call, |world| crate::players::module::players_create_in(world) as usize).map(|key| key as u32)
}

impl DefinitionValues {
    /// The producers retained these objects in native definition-id order.
    /// Cloning Value keeps the shared Rc identity; neither names nor the
    /// independently decoded content catalog assign capture ids here.
    pub fn from_context(cx: &Cx) -> Result<Self, AdmitError> {
        use super::rows::combat::{source_definitions, DefinitionKind};
        let values = |table: &[(String, ContentValue)]| table.iter().map(|(_, value)| value.clone()).collect();
        // combat-sources.js derivedFrom: a derived definition (a hull's
        // launcher ordnance copy) takes its authored source's protocol id.
        let sources = |kind| -> Result<Vec<ContentValue>, AdmitError> {
            Ok(source_definitions(cx, kind)?.into_iter().map(|(_, value)| value).collect())
        };
        Ok(Self { weapons: values(&cx.definitions.weapons), items: sources(DefinitionKind::Item)?,
            ordnance: sources(DefinitionKind::Ordnance)?, remotes: values(&cx.definitions.recon) })
    }
}

/// UnitFactory.prepare after Battle::load has retained the same body/clip
/// files. No simulation call belongs to this step: model construction starts
/// only in F3Factory.prepare. The first side carrying the class wins in JS.
pub fn prepare_class(cx: &Cx, class_name: &str) -> Result<(), AdmitError> {
    super::spawn::load(cx, class_name)
}

/// createLocalServer.welcome immediately publishes the initial state after
/// its two text events. The local host supplies both clocks, as it does for
/// later publications; neither simulation construction nor content reads a
/// system clock. Packet spans are relative to the returned owned byte arena.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InitialPublication {
    pub output: WirePublicationOutput,
    pub bytes: Vec<u8>,
}
pub fn publish_initial(world: &mut World, cx: &mut Cx, session: &mut Session, at: f64, unix_ms: f64) -> Result<InitialPublication, AdmitError> {
    let player = session.handles.player;
    let discarded = perform(world, cx, &mut session.calls, "players_lanes", scalar(&[player as f64, 0.0, 0.0]), |world| {
        crate::players::module::players_lanes_in(world, player, 0, 0).ok_or_else(|| refused("players_lanes"))?;
        let (row, heap) = world.input.players.lanes.read(0);
        Ok(PlayerLanes::take(row, heap).actions_discarded)
    })?;
    let facts = WireWorldFacts { tick: 0.0, at, unix_ms, dropped_ticks: 0.0,
        links: vec![WirePublicationLink { id: 1, discarded_actions: discarded, rtt: f64::NAN, ..Default::default() }] };
    let mut words = vec![0.0; WireWorldFacts::WORDS]; let mut heap = Vec::new(); facts.put(&mut words, &mut heap); words.extend_from_slice(&heap);
    // writeWorldFacts writes the generated source bank directly. It is not
    // a callArgs input to wire_publish, which takes only its session handle.
    let publication = session.handles.publication;
    perform(world, cx, &mut session.calls, "wire_publish", scalar(&[publication as f64]), |world| {
        let (words, bytes) = crate::wire::publish_module::wire_publish_native_in(world, publication as usize, &words)
            .ok_or_else(|| refused("wire_publish"))?;
        let header = BlockHeader::WORDS; let heap_at = words[BlockHeader::HEAP] as usize;
        let mut output = WirePublicationOutput::take(&words[header..], &words[heap_at..]);
        output.bytes = 0;
        if !output.faults.is_empty() { return Err(refused("wire_publish")); }
        Ok(InitialPublication { output, bytes: bytes.to_vec() })
    })
}

/// The connection key createLocalServer admits (local-server.js `id`).
pub const LOCAL_CONNECTION: &str = "local";

/// The local connection's bindings as createMultiplayerWorld.addPlayer makes
/// them (multiplayer-world.js 111-117): the gameplay stream the world already
/// registered (`game.randomStream('gameplay')`), the connection spread stream
/// registered now (`game.randomStream('network-weapons')`, a new id at this
/// seam), the recon key F3Recon.admitKey gives the first admitted connection
/// (0: the local server admits one, before any other key), and reconRandom's
/// seed for that key. The F3 change mirror fields stay what the earlier
/// owners left in `cx.player`.
pub fn local_bindings(cx: &mut Cx) -> PlayerBindings {
    use super::rows::{battle::streams, recon};
    let combat_random = cx.streams.register(streams::GAMEPLAY);
    let weapon_random = cx.streams.register(streams::NETWORK_WEAPONS);
    PlayerBindings { combat_random, weapon_random, recon_key: Some(0),
        recon_seed: recon::seed(cx.options.seed, LOCAL_CONNECTION), ..cx.player.clone() }
}

/// createLocalServer's complete admission through the shared construction
/// context. All connection, hull and definition identities come from the
/// owners that ran earlier; this entry never derives a replacement id. The
/// enclosing sequence consumes the returned keys/registry and stages.
pub fn run(world: &mut World, cx: &mut Cx) -> Result<Session, AdmitError> {
    // addPlayer registers the spread stream after the roster reservation and
    // the stock bodies; nothing between here and that bind registers a new
    // stream (rider births and leases answer NO_STREAM), so the id is the same.
    let player = local_bindings(cx);
    cx.player = player.clone();
    let at = cx.at;
    // spawn::prepare lends cx.holders around each body's construction. Keep
    // that retained list on Cx so its authored riders and qualified refs also
    // remain available to the unit services called during preparation.
    run_with(world, cx, &player, at, |world, cx, seam, calls| {
        match seam {
            Prepare::Session => {
                let facts = cx.ids.battle.clone().ok_or(AdmitError::Unavailable(Stage::Session))?;
                let factions = cx.ids.factions.clone();
                let owners = super::rows::battle::session_with_facts(cx, &facts, &factions)?;
                for (stage, name, call) in owners {
                    let before = world.sim.store.character.len();
                    super::sequence::args(world, &call)?;
                    let answer = super::dispatch::call(world, stage, name, &call)?;
                    super::dispatch::record(cx, stage, name, answer)?;
                    calls.push((stage, name, call));
                    cx.refresh_strings(&world.sim.strings);
                    // F4Battle.ensureCharacter binds every Character the
                    // roster's publication names for the first time; each new
                    // facade asks the snapshot capacity once (bindCharacter).
                    for _ in before..world.sim.store.character.len() {
                        let call = super::rows::battle::character_capacity();
                        let answer = super::dispatch::call(world, stage, "f4_battle_capacity", &call)?;
                        super::dispatch::record(cx, stage, "f4_battle_capacity", answer)?;
                        calls.push((stage, "f4_battle_capacity", call));
                    }
                }
            }
            // add_player has already invoked the body's loader at this seam.
            // Loading a retained class makes no simulation export call.
            Prepare::Class(_) => {}
            Prepare::Stock(class) => {
                let prepared = super::spawn::prepare(world, cx, class)?;
                calls.extend(prepared.calls);
                cx.refresh_strings(&world.sim.strings);
            }
        }
        Ok(())
    })
}

// These fixtures are also integration tests. The host oracle alone needs
// crate access to seed the preceding owners' string bank before construction.
#[cfg(test)]
#[macro_use]
#[path = "../../../tests/admit_session.rs"]
mod tests;
#[cfg(test)]
session_host_oracle!();
