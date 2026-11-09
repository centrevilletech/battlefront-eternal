//! attachF2's cold rows. Content supplies immutable class/map facts; body,
//! cache and mirror handles come from the preceding owner constructors.
//! Keeping those handles explicit preserves collision order across props,
//! hulls and dormant pad variants without inventing a second allocator.
use super::{AdmitError, Call, Cx, Options, Stage};
use super::vehicle_pack::{array, error, json, object, ExportCall, Pack};
use crate::gen::{F2Bodies, F2BodyRef, F2ColliderHeader, FfSupply, Ref, Vehicle};
use crate::store::Strings;
use bfe_content::battle::Battle;
use bfe_content::collision::MapCollision;
use bfe_content::value::Value;

pub type Calls = Vec<ExportCall>;
fn call(name: &'static str, arguments: Vec<f64>) -> ExportCall {
    ExportCall::scalar(name, arguments)
}
fn number(value: f64) -> Value { Value::Number(value) }
fn nullish(value: Option<&Value>, fallback: Value) -> Value {
    value.filter(|v| !v.is_null()).cloned().unwrap_or(fallback)
}

/// classKeys(def), including Boolean(explosion) rather than its definition.
/// Seat weapons belong to the pack and firing owner, not HullClass's keys.
pub fn class_keys(def: &Value) -> Value {
    object([
        ("settings", Some(nullish(def.get("settings"), Value::Null))),
        ("springs", Some(nullish(def.get("springs"), array([])))),
        ("fx", Some(nullish(def.get("fx"), Value::Null))),
        ("explosion", Some(Value::Bool(def.get("explosion").is_some_and(Value::truthy)))),
        ("finalExplosion", Some(Value::Bool(def.get("finalExplosion").is_some_and(Value::truthy)))),
        ("seats", Some(array(def.get("seats").and_then(Value::as_array).unwrap_or(&[]).iter()
            .map(|s| object([("settings", Some(nullish(s.get("settings"), Value::Null)))]))))),
    ])
}

/// groundMotion's turnRate: flyers import zero; walkers use MaxTurnSpeed;
/// hovers try TurnRate, then MaxTurnSpeed. Only the body's section counts.
pub fn turn_rate(def: &Value) -> f64 {
    let read = |key, fallback| bfe_content::odf::num(bfe_content::odf::setting(def.get("settings"), key), fallback);
    match def.get("base").and_then(Value::as_str).unwrap_or("hover") {
        "flyer" => 0.0,
        "walker" | "commandwalker" => read("MaxTurnSpeed", 1.7),
        _ => read("TurnRate", read("MaxTurnSpeed", 1.7)),
    }
}

/// Vehicle.spawn calls initFlight before F2 adopts a live flyer. That
/// replaces groundMotion's zero turn rate with flightHandling's body-only
/// TurnRate, clamped to 1.5. A dormant or blocked flyer keeps the zero.
pub fn spawned_turn_rate(def: &Value) -> f64 {
    if def.get("base").and_then(Value::as_str) == Some("flyer") {
        bfe_content::js::js_max(bfe_content::odf::num(bfe_content::odf::setting(def.get("settings"), "TurnRate"), 2.0), 1.5)
    } else { turn_rate(def) }
}

/// giveMapRules and WaterState's Set construction: insertion order and
/// SameValueZero deduplication matter to the canonical text, even though
/// MapRules later treats these cells as a membership set.
pub fn map_rules(world: &Value) -> Result<String, AdmitError> {
    let water = match world.get("water").filter(|v| v.truthy()) {
        None => Value::Null,
        Some(w) => {
            let grid = bfe_content::js::to_number(w.get("grid"));
            let mut cells: Vec<f64> = Vec::new();
            for cell in w.get("cells").and_then(Value::as_array).ok_or_else(|| error("water has no cell list"))? {
                let pair = cell.as_array().ok_or_else(|| error("water cell is not a pair"))?;
                let index = bfe_content::js::to_number(pair.get(1)) * grid + bfe_content::js::to_number(pair.first());
                if !cells.iter().any(|&v| v == index || v.is_nan() && index.is_nan()) {
                    cells.push(if index == 0.0 { 0.0 } else { index });
                }
            }
            object([
                ("cell", w.get("cell").cloned()), ("offset", w.get("offset").cloned()),
                ("grid", w.get("grid").cloned()), ("cells", Some(array(cells.into_iter().map(number)))),
            ])
        }
    };
    json(&object([
        ("water", Some(water)), ("deathRegions", world.get("deathRegions").cloned()),
        ("mission", Some(object([("deathRegions", world.get("mission").and_then(|m| m.get("deathRegions")).cloned())]))),
    ]))
}

fn root_bounds(positions: &[f32], indices: &[u32]) -> Option<([f64; 3], [f64; 3])> {
    if indices.is_empty() { return None; }
    // Vehicles uses MeshBVH.getBoundingBox, whose float32 bounds include
    // the builder's padding. Raw vertex bounds would lose that padding.
    let tree = bfe_content::bvh::build(bfe_content::bvh::Positions::F32(positions), indices);
    let root = tree.roots.first()?;
    let read = |i| root.get(i).map(|&v| f32::from_bits(v) as f64);
    Some(([read(0)?, read(1)?, read(2)?], [read(3)?, read(4)?, read(5)?]))
}

/// Vehicles' flightEnvironment: ceiling, the union of building/terrain BVH
/// bounds, then authored overrides. An override keeps an existing key's
/// insertion position, exactly as object spread does.
pub fn flight_environment(world: &Value, collision: &MapCollision) -> Result<(String, f64), AdmitError> {
    let height_value = nullish(world.get("mission").and_then(|m| m.get("maxFlyHeight")), number(200.0));
    let height = bfe_content::js::to_number(Some(&height_value));
    let mut bounds = root_bounds(&collision.positions, &collision.indices);
    if let Some((min, max)) = root_bounds(&collision.terrain_positions, &collision.terrain_indices) {
        match &mut bounds {
            Some((lo, hi)) => for axis in 0..3 {
                lo[axis] = bfe_content::js::js_min(lo[axis], min[axis]);
                hi[axis] = bfe_content::js::js_max(hi[axis], max[axis]);
            },
            None => bounds = Some((min, max)),
        }
    }
    let mut environment = object([("ceiling", Some(height_value.clone())), ("bounds", bounds.map(|(min, max)| object([
        ("min", Some(array([number(min[0]), number(-300.0), number(min[2])]))),
        ("max", Some(array([number(max[0]), height_value.clone(), number(max[2])]))),
    ])))]);
    if let Value::Object(ref mut target) = environment {
        if let Some(source) = world.get("flight").filter(|v| v.truthy()).and_then(Value::as_object) {
            for (key, value) in source.entries() { std::rc::Rc::make_mut(target).set(std::rc::Rc::from(key), value.clone()); }
        }
    }
    Ok((json(&environment)?, height))
}

/// FLIGHT_CLASSES uses the common properties in written order, with rollRate
/// appended for each class. These numbers are host tuning, not content ODFs.
pub fn flight_tuning() -> Vec<(&'static str, &'static str, f64)> {
    const COMMON: &[(&str, f64)] = &[
        ("boostAcceleration", 2.0), ("boostTurn", 0.62), ("stickReturn", 3.5), ("responseCurve", 1.35),
        ("landingHeight", 18.0), ("maxSlope", 0.9375), ("powerDrain", 25.0), ("powerRecharge", 12.0),
        ("trickCost", 30.0), ("trickCooldown", 1.0), ("barrelDisplacement", 12.0), ("missileTurnRate", 1.1),
        ("missileSeeker", 15.0), ("missileSpeed", 180.0), ("missileLifetime", 8.0), ("missileNavigation", 3.5),
        ("missileRollBreakRate", 8.0), ("missileEvadeRange", 300.0), ("barrelTime", 0.6), ("loopTime", 1.8),
        ("immelmannTime", 1.5), ("trickWindow", 0.3), ("terrainLookahead", 1.5), ("recoverySharpness", 1.8),
        ("lethalImpactSpeed", 60.0), ("lethalImpactCosine", 0.8), ("scrapeDamageLimit", 0.45), ("impactDamageSpeed", 120.0),
        ("assistCone", 3.0), ("assistStrength", 0.45), ("stickyScale", 0.8), ("cameraBlendTime", 0.25),
        ("lockRange", 1600.0), ("lockCone", 15.0), ("retainCone", 25.0), ("lockDwell", 1.0), ("losGrace", 0.3),
    ];
    let mut out = Vec::new();
    for (class, roll) in [("fighter", 2.8), ("scout", 3.4), ("bomber", 1.8), ("transport", 1.3)] {
        out.extend(COMMON.iter().map(|&(key, value)| (class, key, value)));
        out.push((class, "rollRate", roll));
    }
    out
}

/// Vehicle constructor order, distinct from the pack's class order. Neutral
/// pads can have no initial hull; prepareVariants still constructs their
/// unique, nonempty classes in Object.values order after f2_init.
#[derive(Clone, Debug)]
pub struct Hull {
    pub pad: usize,
    pub name: String,
    pub prepared: bool,
    pub keys: String,
    pub turn_rate: f64,
}

pub fn hulls(data: &Value, world: &Value) -> Result<Vec<Hull>, AdmitError> {
    let pads = data.get("spawns").and_then(Value::as_array).ok_or_else(|| error("vehicle pack has no spawns"))?;
    let classes = data.get("classes").and_then(Value::as_object).ok_or_else(|| error("vehicle pack has no classes"))?;
    let posts = world.get("commandPosts").and_then(Value::as_array).unwrap_or(&[]);
    let mut result = Vec::new();
    let mut seen: Vec<Vec<String>> = vec![Vec::new(); pads.len()];
    let make = |pad, name: &str, prepared| -> Result<Hull, AdmitError> {
        let def = classes.get(name).ok_or_else(|| error(format!("vehicle class {name} is missing")))?;
        Ok(Hull { pad, name: name.into(), prepared, keys: json(&class_keys(def))?, turn_rate: if prepared { turn_rate(def) } else { spawned_turn_rate(def) } })
    };
    for (pad, desc) in pads.iter().enumerate() {
        let choices = desc.get("classes").and_then(Value::as_object).ok_or_else(|| error("vehicle pad has no classes"))?;
        let post = desc.get("post").filter(|v| v.truthy()).and_then(Value::as_str)
            .and_then(|name| posts.iter().find(|p| p.get("name").and_then(Value::as_str).is_some_and(|n| n.to_lowercase() == name)));
        let team = match post {
            Some(post) => post.get("team").cloned(),
            None => desc.get("team").filter(|v| v.truthy()).cloned().or_else(|| {
                (choices.len() == 1).then(|| number(bfe_content::js::string_to_number(choices.keys().next().unwrap_or(""))))
            }).or(Some(number(0.0))),
        };
        let key = bfe_content::odf::to_text(team.as_ref());
        if let Some(name) = choices.get(&key).filter(|v| v.truthy()).and_then(Value::as_str) {
            result.push(make(pad, name, false)?); seen[pad].push(name.into());
        }
    }
    for (pad, desc) in pads.iter().enumerate() {
        let choices = desc.get("classes").and_then(Value::as_object).ok_or_else(|| error("vehicle pad has no classes"))?;
        for value in choices.values().filter(|v| v.truthy()) {
            let name = value.as_str().ok_or_else(|| error("vehicle pad class is not a name"))?;
            if !seen[pad].iter().any(|n| n == name) {
                result.push(make(pad, name, true)?); seen[pad].push(name.into());
            }
        }
    }
    Ok(result)
}

/// A resolved f2_register row. The generation belongs to the vehicle mirror
/// pool, independently of the aiTarget gameplay life and the body handle.
pub fn register(hull: Ref<Vehicle>, prepared: bool) -> ExportCall {
    call("f2_register", vec![hull.slot as f64, hull.gen as f64, if prepared { 1.0 } else { 0.0 }])
}
pub fn prepare_body(kernel: u32) -> ExportCall { call("f2_prepare_body", vec![kernel as f64]) }
pub fn collision(world: i32, blockers: i32) -> ExportCall {
    call("veh_collision", vec![world as f64, blockers as f64])
}
pub fn seats_init() -> ExportCall { call("f2_seats_init", Vec::new()) }

/// VehicleContent.collider: generated header followed by wall caches then
/// spring caches. Missing optional gather caches are the signed sentinel -1.
pub fn collider(header: F2ColliderHeader, walls: &[i32], springs: &[i32]) -> Result<ExportCall, AdmitError> {
    let wall_count = u32::try_from(walls.len()).map_err(|_| error("too many wall caches"))?;
    let spring_count = u32::try_from(springs.len()).map_err(|_| error("too many spring caches"))?;
    let mut scratch = vec![0.0; F2ColliderHeader::WORDS];
    header.put(&mut scratch, &mut Vec::new());
    scratch.extend(walls.iter().chain(springs).map(|&v| v as f64));
    Ok(ExportCall { name: "veh_collider", call: Call { args: vec![wall_count as f64, spring_count as f64], scratch: scratch.iter().flat_map(|v| v.to_le_bytes()).collect(), ..Default::default() } })
}

/// bodiesIn's callArgs uses generated reference rows. Keep the live collision
/// order supplied by CollisionWorld, including non-vehicle moving bodies.
pub fn bodies(bodies: Vec<F2BodyRef>) -> ExportCall {
    let mut data = Call { row: vec![0.0; F2Bodies::WORDS], ..Default::default() };
    F2Bodies { bodies }.put(&mut data.row, &mut data.heap);
    ExportCall { name: "f2_bodies", call: data }
}

/// F2Firing.supply is a command, not an unconditional constructor call. Use
/// this only when a PowerupStation serves a hull during admission or later.
pub fn firing_supply(hull: Ref<Vehicle>, life: f64, amount: f64) -> ExportCall {
    let mut data = Call { row: vec![0.0; FfSupply::WORDS], ..Default::default() };
    FfSupply { hull, life, amount }.put(&mut data.row, &mut data.heap);
    ExportCall { name: "f2_firing_supply", call: data }
}

/// A scalar call's strings are retained alongside their local ids. The
/// construction sequence interns these against its shared owner table and
/// resolves string argument positions for the named export before dispatch.
fn text_call(name: &'static str, row: Vec<f64>, texts: &[&str]) -> ExportCall {
    ExportCall { name, call: Call { args: row, strings: texts.iter().map(|s| s.encode_utf16().collect()).collect(), ..Default::default() } }
}

/// Immutable prefix of attachF2. Hull construction must interleave class,
/// register, seats/firing and collider rows through the helpers above before
/// calling init; prepareVariants runs after the initial fleet is published.
pub fn prefix(battle: &Battle, strings: &mut Strings) -> Result<Calls, AdmitError> {
    if battle.vehicle_def.is_none() { return Ok(Vec::new()); }
    let col = MapCollision::load(&battle.coll_buf, &battle.coll_index_buf, &battle.coll_poly_buf,
        &battle.ground_buf, &battle.ground_index_buf).map_err(error)?;
    prefix_with_collision(battle, &col, strings)
}

fn prefix_with_collision(battle: &Battle, col: &MapCollision, strings: &mut Strings) -> Result<Calls, AdmitError> {
    let Some(pack) = Pack::from_battle(battle)? else { return Ok(Vec::new()); };
    let mut calls = Vec::new();
    let map = map_rules(&battle.world)?;
    calls.push(text_call("veh_map", vec![strings.intern_str(&map).0 as f64, col.kill_y], &[&map]));
    calls.extend(pack.calls());
    let (environment, height) = flight_environment(&battle.world, col)?;
    calls.push(text_call("veh_flight_env", vec![strings.intern_str(&environment).0 as f64, height], &[&environment]));
    for (name, key, value) in flight_tuning() {
        let name_id = strings.intern_str(name).0 as f64; let key_id = strings.intern_str(key).0 as f64;
        calls.push(text_call("veh_flight_tune", vec![name_id, key_id, value], &[name, key]));
    }
    calls.push(seats_init());
    Ok(calls)
}

/// adopt emits one class row per distinct name, even when several pads share
/// that class. Pass the same seen list across active and prepared hulls.
pub fn class(hull: &Hull, strings: &mut Strings, seen: &mut Vec<String>) -> Calls {
    if seen.contains(&hull.name) { return Vec::new(); }
    seen.push(hull.name.clone());
    let name = strings.intern_str(&hull.name).0 as f64; let keys = strings.intern_str(&hull.keys).0 as f64;
    vec![text_call("veh_class", vec![name, keys, hull.turn_rate], &[&hull.name, &hull.keys])]
}

/// f2_init receives original spawn descriptions, not scheduler SlotDesc's
/// normalized values. Its initial clock is zero in Vehicles' constructor.
pub fn init(data: &Value, strings: &mut Strings) -> Result<Calls, AdmitError> {
    let pads = data.get("spawns").ok_or_else(|| error("vehicle pack has no spawns"))?;
    if pads.as_array().is_none() { return Err(error("vehicle spawns is not an array")); }
    let text = json(pads)?;
    let pads = strings.intern_str(&text).0 as f64;
    Ok(vec![text_call("f2_init", vec![pads, 0.0, 0.0], &[&text])])
}

/// attachF3 repeats giveMapRules even after attachF2 supplied them. A map
/// without hulls still needs these rules for infantry and ordnance owners.
pub fn unit_map(battle: &Battle, strings: &mut Strings) -> Result<ExportCall, AdmitError> {
    let col = MapCollision::load(&battle.coll_buf, &battle.coll_index_buf, &battle.coll_poly_buf,
        &battle.ground_buf, &battle.ground_index_buf).map_err(error)?;
    map_call(&battle.world, col.kill_y, strings)
}

/// The Cx path reuses the collision content already decoded at admission.
pub fn map_call(world: &Value, kill_y: f64, strings: &mut Strings) -> Result<ExportCall, AdmitError> {
    let map = map_rules(world)?;
    Ok(text_call("veh_map", vec![strings.intern_str(&map).0 as f64, kill_y], &[&map]))
}

/// Standalone content rows across attachF2 and attachF3's prepareVariants.
/// The latter repeats map rules before registering the dormant classes.
/// Construction-dependent handles enter through resolved() instead.
pub fn content_calls(battle: &Battle, _options: &Options) -> Result<Calls, AdmitError> {
    let mut strings = Strings::new();
    let mut calls = prefix(battle, &mut strings)?;
    if let Some(data) = &battle.vehicle_def {
        let mut seen = Vec::new();
        let hulls = hulls(data, &battle.world)?;
        for hull in hulls.iter().filter(|h| !h.prepared) { calls.extend(class(hull, &mut strings, &mut seen)); }
        calls.extend(init(data, &mut strings)?);
        calls.push(unit_map(battle, &mut strings)?);
        for hull in hulls.iter().filter(|h| h.prepared) { calls.extend(class(hull, &mut strings, &mut seen)); }
    } else { calls.push(unit_map(battle, &mut strings)?); }
    Ok(calls)
}

/// Hull construction must allocate its body/cache and store identities
/// before this owner can emit registrations. Empty fleets need no F2 calls;
/// a populated fleet uses build_bound after those constructors resolve them.
pub fn build(cx: &mut Cx) -> Result<super::Calls, AdmitError> {
    if cx.battle.vehicle_def.is_none() {
        // attachF3 installs these shared rules when there was no attachF2.
        // Keep the units-stage tag while placing them before f3_init.
        let world = cx.ids.kernel.as_ref().ok_or(AdmitError::Unavailable(Stage::Kernels))?.world;
        let mut planned_strings = cx.strings.clone();
        let map = map_call(&cx.battle.world, cx.content.collision.kill_y, &mut planned_strings)?.in_context(cx, Stage::Units)?;
        return Ok(vec![map, collision(world as i32, -1).into_row(Stage::Units)]);
    }
    let bindings = cx.ids.fleet.clone().ok_or(AdmitError::Unavailable(Stage::Fleet))?;
    build_bound(cx, &bindings)
}

/// Identity rows allocated by the collision and store owners. Bounds are
/// authored shape bounds, while cache ids belong to this collision world.
#[derive(Clone, Debug)]
pub struct HullBinding {
    pub hull: Ref<Vehicle>,
    /// The constructor's spawn result; overlapping initial pads can leave
    /// a constructed flyer dormant, so its TurnRate has not been retuned.
    pub spawned: bool,
    pub collider: F2ColliderHeader,
    pub walls: Vec<i32>,
    pub springs: Vec<i32>,
}
#[derive(Clone, Debug)]
pub struct Bindings {
    pub world: i32,
    pub blockers: i32,
    pub bodies: Vec<F2BodyRef>,
    /// Same initial-then-prepared order as hulls(), not grouped by class.
    pub hulls: Vec<HullBinding>,
}

/// All requested F2 rows, with identities resolved by preceding owners.
/// The sequence inserts rig, seat and weapon imports after each register;
/// those imports have their own generated row builders. No body or cache
/// is allocated here and no absent handle is replaced by an invented id.
pub fn resolved(battle: &Battle, strings: &mut Strings, bindings: &Bindings) -> Result<Calls, AdmitError> {
    let col = MapCollision::load(&battle.coll_buf, &battle.coll_index_buf, &battle.coll_poly_buf,
        &battle.ground_buf, &battle.ground_index_buf).map_err(error)?;
    resolved_with_collision(battle, &col, strings, bindings)
}

fn resolved_with_collision(battle: &Battle, col: &MapCollision, strings: &mut Strings, bindings: &Bindings) -> Result<Calls, AdmitError> {
    let Some(data) = &battle.vehicle_def else {
        return if bindings.hulls.is_empty() { Ok(Vec::new()) } else { Err(error("hull bindings without a vehicle pack")) };
    };
    let hulls = hulls(data, &battle.world)?;
    if hulls.len() != bindings.hulls.len() { return Err(error("vehicle hull binding count differs from content")); }
    for (index, (hull, binding)) in hulls.iter().zip(&bindings.hulls).enumerate() {
        if bindings.hulls[..index].iter().any(|other| other.hull == binding.hull || other.collider.kernel == binding.collider.kernel) {
            return Err(error("two hulls share a construction handle"));
        }
        if !hull.prepared && !bindings.bodies.iter().any(|body| body.body.slot == binding.collider.body && body.kernel == binding.collider.kernel) {
            return Err(error("active hull collider is absent from collision body order"));
        }
    }
    let mut calls = prefix_with_collision(battle, col, strings)?;
    let mut seen = Vec::new();
    for (hull, binding) in hulls.iter().zip(&bindings.hulls).filter(|(h,_)| !h.prepared) {
        calls.extend(bound_class(data, hull, binding.spawned, strings, &mut seen)?);
        calls.push(register(binding.hull, false));
    }
    calls.push(collision(bindings.world, bindings.blockers));
    calls.push(bodies(bindings.bodies.clone()));
    for (_, binding) in hulls.iter().zip(&bindings.hulls).filter(|(h,_)| !h.prepared) {
        calls.push(collider(binding.collider.clone(), &binding.walls, &binding.springs)?);
    }
    calls.extend(init(data, strings)?);
    calls.push(map_call(&battle.world, col.kill_y, strings)?);
    for (hull, binding) in hulls.iter().zip(&bindings.hulls).filter(|(h,_)| h.prepared) {
        calls.extend(bound_class(data, hull, binding.spawned, strings, &mut seen)?);
        calls.push(register(binding.hull, true));
        calls.push(collider(binding.collider.clone(), &binding.walls, &binding.springs)?);
        calls.push(prepare_body(binding.collider.kernel));
    }
    Ok(calls)
}

/// Geometry facts Vehicle's constructor uses to size wall/spring caches.
/// The collider bounds come from the collision BVH, not the rig's display
/// sphere. Its float32 padding matches createBodyShape.getBoundingBox.
#[derive(Clone, Debug, PartialEq)]
pub struct ColliderShape {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub walls: usize,
    pub springs: usize,
}
pub fn collider_shape(data: &Value, class: &str) -> Result<ColliderShape, AdmitError> {
    let def = data.get("classes").and_then(|v| v.get(class)).ok_or_else(|| error(format!("vehicle class {class} is missing")))?;
    let model = def.get("model").and_then(Value::as_str).ok_or_else(|| error("vehicle class has no model"))?;
    let shape = data.get("collision").and_then(|v| v.get(model)).ok_or_else(|| error(format!("vehicle model {model} has no collision")))?;
    let positions = shape.get("positions").and_then(Value::as_array).ok_or_else(|| error("vehicle collision has no positions"))?;
    if positions.len() % 3 != 0 { return Err(error("vehicle collision positions are not triples")); }
    let triangles = bfe_content::collision::json_shape(shape);
    let (min, max) = root_bounds(&triangles.positions, &triangles.indices)
        .ok_or_else(|| error("vehicle collision has no indexed triangles"))?;
    // Empty/invalid shapes cannot furnish finite probe counts. Report them at
    // load rather than truncating a NaN count into a plausible cache census.
    if min.iter().chain(&max).any(|v| !v.is_finite()) { return Err(error("vehicle collision has no finite bounds")); }
    let width = max[0] - min[0]; let length = max[2] - min[2];
    let radius = (width * 0.48).min(2.6).max(0.45);
    let rows = (length / (radius * 1.5)).ceil().clamp(2.0, 12.0) as usize;
    let columns = if (width * 0.5 - radius).max(0.0) > radius { 3 } else { 1 };
    let springs = match def.get("springs").filter(|v| v.truthy()) {
        None => 0,
        Some(value) => value.as_array().ok_or_else(|| error("vehicle springs are not an array"))?.len(),
    };
    Ok(ColliderShape { min, max, walls: (rows + 1) * columns, springs })
}

/// The construction sequence supplies the identities it just allocated.
/// Record the resulting hull namespace once for later unit and AI owners;
/// duplicate class names on different pads deliberately keep separate refs.
pub fn build_bound(cx: &mut Cx, bindings: &Bindings) -> Result<super::Calls, AdmitError> {
    let kernel = cx.ids.kernel.as_ref().ok_or(AdmitError::Unavailable(Stage::Kernels))?;
    if bindings.world != kernel.world as i32 { return Err(error("fleet bindings name another collision world")); }
    // A planned row retains text arguments, not intern side effects. Replay
    // those through Cx in call order so new-only string receipts stay exact.
    let mut planned_strings = cx.strings.clone();
    let calls = resolved_with_collision(cx.battle, &cx.content.collision, &mut planned_strings, bindings)?;
    let data = cx.battle.vehicle_def.as_ref().ok_or_else(|| error("bound fleet has no vehicle pack"))?;
    let hulls = hulls(data, &cx.battle.world)?;
    for (hull, binding) in hulls.iter().zip(&bindings.hulls) {
        let vehicle = (hull.name.clone(), binding.hull.slot as f64);
        let body = (hull.name.clone(), binding.collider.kernel as f64);
        if !cx.ids.vehicles.contains(&vehicle) { cx.ids.vehicles.push(vehicle); }
        if !cx.ids.vehicle_bodies.contains(&body) { cx.ids.vehicle_bodies.push(body); }
    }
    if cx.ids.bodies.is_empty() {
        cx.ids.bodies.extend(bindings.bodies.iter().map(|body| (body.body, body.kernel)));
    } else if bindings.bodies.iter().any(|body| !cx.ids.bodies.contains(&(body.body, body.kernel))) {
        return Err(error("fleet body receipt differs from the shared kernel census"));
    }
    cx.ids.fleet = Some(bindings.clone());
    retain_holders(cx)?;
    // The pack prefix was already dispatched. The repeated veh_map marks
    // attachF3's entry, after which dormant variant rows belong to Units.
    let mut phase = Stage::Fleet; let mut first_map = true;
    calls.into_iter().filter_map(|export| {
        if export.name == "veh_map" {
            if first_map { first_map = false; return None; }
            phase = Stage::Units;
        }
        if matches!(export.name, "veh_bytes" | "veh_pack") { return None; }
        Some(export.in_context(cx, phase))
    }).collect()
}

/// A class's first adopted facade supplies the mutable turn rate, including
/// whether Vehicle.spawn actually succeeded before that first adoption.
pub fn bound_class(data: &Value, hull: &Hull, spawned: bool, strings: &mut Strings, seen: &mut Vec<String>) -> Result<Calls, AdmitError> {
    let def = data.get("classes").and_then(|classes| classes.get(&hull.name)).ok_or_else(|| error("bound hull class is missing"))?;
    let mut hull = hull.clone();
    hull.turn_rate = if spawned { spawned_turn_rate(def) } else { turn_rate(def) };
    Ok(class(&hull, strings, seen))
}

/// F2Riders.ensureAll visits the active fleet, prepared variants, then the
/// stationary turrets. Build each authored rig from the shared template and
/// retain its actual rider descriptors under the constructor's qualified ref.
/// No holder identity is recovered from a class name at a unit's birth.
pub fn retain_holders(cx: &mut Cx) -> Result<(), AdmitError> {
    use super::super::bodies::{hull_holder, Holder};
    let Some(data) = cx.battle.vehicle_def.as_ref() else { return Ok(()); };
    let bindings = cx.ids.fleet.as_ref().ok_or(AdmitError::Unavailable(Stage::Fleet))?;
    let plans = hulls(data, &cx.battle.world)?;
    if plans.len() != bindings.hulls.len() { return Err(error("holder binding count differs from fleet")); }
    let mut holders = Vec::with_capacity(plans.len() + cx.holders.len());
    for (plan, binding) in plans.iter().zip(&bindings.hulls) {
        if let Some(existing) = cx.holders.iter().find(|holder| matches!(holder, Holder::Hull { hull, .. } if *hull == binding.hull)) {
            holders.push(existing.clone());
            continue;
        }
        let class = data.get("classes").and_then(|classes| classes.get(&plan.name)).ok_or_else(|| error("holder has no vehicle definition"))?;
        let model = class.get("model").and_then(Value::as_str).ok_or_else(|| error("holder class has no model"))?;
        let template = cx.content.vehicle_templates.iter().find(|template| template.name == model)
            .ok_or_else(|| error(format!("holder model {model} has no shared template")))?;
        let desc = bfe_pose::rig::content::hull_desc(class, template);
        let rig = bfe_pose::rig::hull::HullRig::new(&desc);
        holders.push(hull_holder(binding.hull, &rig));
    }
    // Existing turret construction facts stay behind the fleet's holders.
    holders.extend(cx.holders.iter().filter(|holder| matches!(holder, Holder::Turret { .. })).cloned());
    cx.holders = holders;
    Ok(())
}
