//! Building admission from F3Props.prepare (f3-props.js). Immutable look
//! geometry and the initial F4 life frame are encoded together so combat and
//! the live owner do not retain divergent copies of building health.
use super::{Call, units::{call, error}};
use super::super::{AdmitError, Stage};
use bfe_content::{js::{parse_float, number_to_string}, Value};
use crate::gen::*;

/// Collision and mixed-registry identities were allocated before attachF3.
/// Trap placement includes the native downward collision query; its answer
/// is supplied here, never approximated by the authored pad translation.
/// The explosion is not here: F3Props.prepare asks F1 for the id of the
/// instance's `explosion` definition at that point (ordnance.sim
/// .explosionId), which registers it lazily, so the builder does that.
#[derive(Clone, Debug)]
pub struct Binding {
    pub index: u32, pub target: RepairTarget, pub member: Option<Member>,
    pub bodies: [Option<Ref<Body>>;3],
    pub generation: Option<f64>, pub team: f64, pub target_team: f64,
    pub category: u32, pub kind: f64, pub damaged: bool, pub crew: bool,
    pub trap: Option<Trap>,
}
#[derive(Clone, Debug)]
pub struct Trap {
    pub pad_sphere: Bounds, pub point: Vec3, pub effect_at: Vec3,
    pub build_frames: Option<f64>, pub trigger_frames: Option<f64>,
}

/// props.js num reads only an array's first element, uses parseFloat rather
/// than Number, and rejects infinite results. "12m" therefore means 12.
fn num(value: Option<&Value>, fallback: f64) -> f64 {
    let value = value.and_then(|v| if let Value::Array(a) = v { a.first() } else { Some(v) });
    let n = match value {
        Some(Value::Number(n)) => parse_float(&number_to_string(*n)),
        Some(Value::String(s)) => parse_float(s), _ => f64::NAN,
    };
    if n.is_finite() { n } else { fallback }
}

fn array<const N: usize>(value: &Value) -> Result<[f64;N], AdmitError> {
    let values = value.as_array().ok_or_else(|| error("building geometry is not an array"))?;
    // A short fromArray read becomes undefined/NaN in the JS host.
    Ok(std::array::from_fn(|i| bfe_content::js::to_number(values.get(i))))
}

/// F3Props.prepare reads bounds from model records, not rendered vertices.
/// Missing looks stay null in all three positions: built, destroyed, unbuilt.
fn model(value: &Value) -> Result<PbModel, AdmitError> {
    let sphere = value.get("sphere").filter(|v| v.truthy()).map(array::<4>).transpose()?;
    let bounds = value.get("box").filter(|v| v.truthy()).map(|v| -> Result<_,AdmitError> {
        let lo = v.at(0).ok_or_else(|| error("building box has no lower corner"))?;
        let hi = v.at(1).ok_or_else(|| error("building box has no upper corner"))?;
        Ok((array::<3>(lo)?, array::<3>(hi)?))
    }).transpose()?;
    Ok(PbModel { sphere, low: bounds.map(|v|v.0), high: bounds.map(|v|v.1) })
}

/// Ports Building, PowerupStation and F3Props.prepare's cold assignments.
/// The key uses LiveObjects.buildings order, which skips animated props and
/// definitions whose model is missing, rather than props.json's raw index.
pub fn building(desc: &Value, models: &Value, binding: Binding, explosion: Option<u32>,
    mut intern: impl FnMut(&str) -> StrId) -> Result<F3BuildingSetup, AdmitError> {
    let base = desc.get("base").and_then(Value::as_str).unwrap_or("");
    let s = desc.get("settings").ok_or_else(|| error("building has no settings"))?;
    let trap_mode = base == "trap";
    if trap_mode != binding.trap.is_some() { return Err(error("trap building requires prepared placement facts")); }
    let lookup = |name: Option<&Value>| name.and_then(Value::as_str).and_then(|n| models.get(n));
    let built = lookup(desc.get("model"));
    if built.is_none() { return Err(error("building has no model")); }
    // Trap constructs its Building base with only the built rig. Its pad is
    // separate, so it does not set hasUnbuilt or populate the third model slot.
    let destroyed = if trap_mode { None } else { lookup(desc.get("destroyed").and_then(|v|v.get("model"))) };
    let unbuilt = if trap_mode { None } else { lookup(desc.get("unbuilt").and_then(|v|v.get("model"))) };
    let kind = match base { "armedbuilding" => PbKind::Turret, "commandpost" => PbKind::Objective, _ => PbKind::Building };
    let state = if trap_mode || unbuilt.is_some() { PbStateName::Unbuilt } else { PbStateName::Built };
    let max_health = num(s.get("MaxHealth"),f64::INFINITY);
    // An unbuilt pad starts at zero even when MaxHealth is infinite. Building
    // assigns full health only after choosing a built initial look.
    let health = if state == PbStateName::Built { max_health } else { 0.0 };
    let matrix = desc.get("matrix").ok_or_else(|| error("building has no placement matrix")).and_then(array::<16>)?;
    let settings = PbSettings { kind, max_health, respawn_time: num(s.get("RespawnTime"),f64::INFINITY),
        built_add_health: { let n = num(s.get("AddHealth"),0.0); if n == 0.0 {0.0} else {n} },
        regenerates: base == "armedbuilding", has_destroyed: destroyed.is_some(), has_unbuilt: unbuilt.is_some(),
        has_explosion: explosion.is_some(), matrix,
        models: [built,destroyed,unbuilt].into_iter().map(|m|m.map(model).transpose()).collect::<Result<_,_>>()? };
    let trap_state = TrState { built: false, health: 0.0, phase: TrPhase::Down,
        started: Some(0.0), time: 0.0, sprung: false, tick: 0.0, crushed: 0.0 };
    // Turret's constructor initializes lastAttacker to null; ordinary
    // Building leaves it undefined. Both tags survive in the live birth.
    let attacker = if kind == PbKind::Turret {Attacker::Null} else {Attacker::Undefined};
    let initial = F3BuildingState { index: binding.index, state, health, add_health: 0.0,
        attacker, generation: binding.generation, team: binding.team,
        target_team: binding.target_team, gunner: None, trap_mode,
        trap_state: if trap_mode {trap_state.clone()} else {unused_trap_state()} };
    let supply = if base == "powerupstation" { Some(PbSupply {
        position: [matrix[12],matrix[13],matrix[14]],
        radius: desc.get("radius").filter(|v|v.truthy()).map(|v|bfe_content::js::to_number(Some(v))).unwrap_or(5.0),
        delay: num(s.get("PowerupDelay"),0.5), health: num(s.get("SoldierHealth"),0.0),
        ammo: num(s.get("SoldierAmmo"),0.0), vehicle_health: num(s.get("VehicleHealth"),0.0),
        vehicle_ammo: num(s.get("VehicleAmmo"),0.0) }) } else {None};
    // The same settings are serialized into combat and F4 life. Their model
    // lists each get a generated heap span; sharing a Rust pointer is not ABI.
    let live = F4BuildingBirth { settings: settings.clone(), state: PbStation {
        building: PbState { state, health, add_health: 0.0, last_attacker: PbIdentity {kind:if kind==PbKind::Turret {1.0}else{0.0},id:0.0},
            target_generation: binding.generation }, mode: PbMode::Waiting, ..Default::default() },
        supply, crew: binding.crew, damaged: binding.damaged, attacker };
    let key = intern(&format!("l{}",binding.index));
    let trap = binding.trap.as_ref().map(|facts| {
        let t = desc.get("trap").unwrap_or(&Value::Null);
        let opt_string = |key: &str, intern: &mut dyn FnMut(&str)->StrId| t.get(key).filter(|v| !matches!(v, Value::Null))
            .map(|v| intern(v.as_str().unwrap_or("undefined")));
        F4TrapBirth { state: trap_state, attacker: Attacker::default(), settings: TrSettings {
            max_health, reach: t.get("radius").and_then(Value::as_f64).filter(|n|n.is_finite()).unwrap_or(3.0),
            min_radius: t.get("minRadius").filter(|v|v.truthy()).map(|v|bfe_content::js::to_number(Some(v))).unwrap_or(0.0),
            death_time: t.get("deathTime").and_then(Value::as_f64).filter(|n|n.is_finite()).unwrap_or(-1.0),
            build_frames: facts.build_frames, trigger_frames: facts.trigger_frames,
            trigger_team: opt_string("triggerTeam",&mut intern), sound: opt_string("sound",&mut intern),
            death_effect: opt_string("deathEffect",&mut intern), point: facts.point, effect_at: facts.effect_at } }
    });
    Ok(F3BuildingSetup { index: binding.index, target: binding.target, member: binding.member, key,
        settings, state: initial, live, trap, bodies: binding.bodies.to_vec(), explosion,
        damaged: binding.damaged, category: binding.category, kind: binding.kind,
        trap_sphere: binding.trap.map(|t|t.pad_sphere) })
}

/// Generated JS mirror defaults for an unused trap frame contain NaNs.
/// Rust's derived Default uses zeros, which would change the recorded words.
fn unused_trap_state() -> TrState {
    TrState { built: false, health: f64::NAN, phase: TrPhase::Down, started: None,
        time: f64::NAN, sprung: false, tick: f64::NAN, crushed: f64::NAN }
}

/// F3Props reuses one setup mirror across all buildings. Its unused category,
/// kind and trap frame retain the preceding assignment, even for a different
/// kind of building. Keep that scratch history for bit-exact call recording.
pub struct Buildings { category: u32, kind: f64, trap_state: TrState }
impl Default for Buildings {
    fn default() -> Self { Self {category:0,kind:f64::NAN,trap_state:unused_trap_state()} }
}
impl Buildings {
    pub fn prepare(&mut self, desc: &Value, models: &Value, mut binding: Binding, explosion: Option<u32>,
        intern: impl FnMut(&str)->StrId) -> Result<F3BuildingSetup,AdmitError> {
        // F3Props only assigns classification when an AI target exists. An
        // ordinary station following a turret retains that scratch vocabulary.
        if binding.member.is_some() { self.category = binding.category; self.kind = binding.kind; }
        binding.category = self.category; binding.kind = self.kind;
        let mut row = building(desc,models,binding,explosion,intern)?;
        if row.state.trap_mode { self.trap_state = row.state.trap_state.clone(); }
        else { row.state.trap_state = self.trap_state.clone(); }
        Ok(row)
    }
}

/// Preserve the building order supplied by LiveObjects, including the trap
/// and station rows whose generated heaps contain nested model records.
pub fn calls(buildings: &[F3BuildingSetup]) -> Vec<(Stage,&'static str,Call)> {
    buildings.iter().map(|b| (Stage::Units,"f3_building",call(F3BuildingSetup::WORDS,|r,h|b.put(r,h)))).collect()
}


/// The shared sequence fills runtime identities before this owner is built.
pub fn build(cx: &mut super::Cx<'_>) -> Result<super::Calls, AdmitError> {
    let bindings=std::mem::take(&mut cx.ids.props);
    let result=build_bound(cx,&bindings);
    cx.ids.props=bindings;
    result
}

/// Building identities and collision facts come from Cx's earlier owners;
/// all newly used keys and trap names advance its shared string namespace.
/// F3Props.prepare's `explosionId(building.explosion)` comes first: F1
/// registers an explosion it has not sent (ord_explosion), combat follows
/// (cmb_explosion, f3_blast_binding), then the building row goes.
pub fn in_context(cx: &mut super::Cx<'_>, buildings: &mut Buildings,
    desc: &Value, models: &Value, binding: Binding) -> Result<super::Calls,AdmitError> {
    let (explosion, mut calls) = explosion_id(cx, desc.get("explosion").filter(|v| v.truthy()))?;
    calls.push(super::units::in_context(cx,|cx,call|buildings.prepare(desc,models,binding,explosion,|text|cx.intern(call,text))
        .map(super::units::Entry::Building))?);
    Ok(calls)
}

/// ordnance.sim.explosionId(def) at a unit-owner construction seam: F1's id
/// for the definition, registering it (and, through F1's service observer,
/// its combat blast and F3 binding) when F1 has not sent it yet. The calls
/// come back in the order the host made them.
pub fn explosion_id(cx: &mut super::Cx<'_>, def: Option<&Value>) -> Result<(Option<u32>, super::Calls), AdmitError> {
    let Some(def) = def else { return Ok((None, Vec::new())); };
    let content = cx.content;
    let mut groups = super::units::load_groups(cx)?;
    let (id, calls) = super::combat::with_bindings(cx, Stage::Units, |bindings, strings| {
        let groups = &mut groups; let combat = &mut bindings.combat;
        let id = bindings.ordnance.explosion_with(def, strings, &mut |kind, id, def, strings, calls| super::units::prepared(content, groups, combat, kind, id, def, strings, calls))?;
        Ok((id.0, std::mem::take(&mut bindings.ordnance.calls)))
    })?;
    super::units::save_groups(cx, &groups);
    Ok((Some(id), calls))
}

/// LiveObjects.buildings: the props.json instances that become buildings
/// (the four bases), in instance order, skipping a model that is missing;
/// the position in this list is the building's compact index (`l<index>`).
pub fn live_sources(battle: &bfe_content::battle::Battle) -> Vec<usize> {
    let Some(data) = battle.props.as_ref() else { return Vec::new(); };
    let Some(instances) = data.get("instances").and_then(Value::as_array) else { return Vec::new(); };
    instances.iter().enumerate().filter(|(_, desc)| {
        let base = desc.get("base").and_then(Value::as_str).unwrap_or("");
        ["trap","powerupstation","armedbuilding","commandpost"].contains(&base)
            && desc.get("model").and_then(Value::as_str).and_then(|name| data.get("models").and_then(|models| models.get(name))).is_some_and(Value::truthy)
    }).map(|(index, _)| index).collect()
}

/// The live constructor has already assigned these identities. `source` is
/// the props.json instance, while Binding.index is the compact building list.
#[derive(Clone,Debug)]
pub struct BuildingBinding { pub source: usize, pub binding: Binding }
pub fn build_bound(cx: &mut super::Cx<'_>, bindings: &[BuildingBinding]) -> Result<super::Calls,AdmitError> {
    // LiveObjects only assigns compact building ids to these four bases.
    // A missing receipt must refuse rather than quietly omit authored owners.
    let expected=live_sources(cx.battle).len();
    if bindings.len()!=expected {return Err(error("building receipts do not cover the live-object construction"));}
    if bindings.is_empty() {return Ok(Vec::new())}
    let data=cx.battle.props.as_ref().ok_or_else(||error("building bindings have no prop content"))?.clone();
    let models=data.get("models").ok_or_else(||error("building bindings have no models"))?;
    let mut buildings=Buildings::default();let mut calls=Vec::new();
    // LiveObjects skips missing models and animated props before indexing its
    // buildings. Preserve that compact index alongside the raw source index.
    for (index,binding) in bindings.iter().enumerate() {
        if binding.binding.index as usize!=index {return Err(error("building bindings are out of live-object order"))}
        let desc=data.get("instances").and_then(|v|v.at(binding.source)).ok_or_else(||error("building source instance is missing"))?;
        calls.extend(in_context(cx,&mut buildings,desc,models,binding.binding.clone())?);
    }
    Ok(calls)
}
