//! A lane's stages cold over the host's recorded construction of the stages
//! before them: the replay driver (admit-replay --cold-calls --from STAGE)
//! replays the recording into the world up to the first call of `from`,
//! then this runs `from` and the stages after it natively, so each lane
//! proves its own stage against the recording while the stages before it
//! are still being ported. The context the earlier owners would have filled
//! is rebuilt by running the cold-complete stages (Content..Combat) on a
//! scratch world with the same string table, and the fleet's and the
//! props' bindings, which the fleet and session lanes derive live, come as
//! fixtures the driver decoded from the recorded rows. Nothing here is on
//! the live path: `sequence::run` is the whole admission.
use super::rows::{self, Cx};
use super::{AdmitError, Admitted, Calls, Options, Rows, Stage};
use crate::gen::{F3BodyName, F3BuildingSetup, F3HullSetup};
use crate::store::Ref;
use crate::world::owner::World;
use bfe_content::files::Files;

/// What the stages before `from` left that the later stages read from the
/// context, taken from the recorded rows rather than derived live.
#[derive(Clone, Debug, Default)]
pub struct Fixtures {
    /// The collision world's body count at attachF3 (f3_init's bodySerial).
    pub body_serial: u32,
    /// The kernel's bodies in world order, the last full f3_body_names list
    /// the host sent, and whether one followed the last hull.
    pub bodies: Vec<F3BodyName>,
    pub bodies_changed: bool,
    /// F3Props.prepare's rows in building order.
    pub buildings: Vec<F3BuildingSetup>,
    /// F3Hulls.prepare's rows in fleet order, each with its body name row
    /// and the full body list when the hull changed the kernel's topology.
    pub hulls: Vec<(F3HullSetup, F3BodyName, Option<Vec<F3BodyName>>)>,
    /// The gameplay random stream's binding (streams.draw("gameplay")).
    pub gameplay: u32,
    /// body_spawn's answer for the viewpoint's body (spawnSoldier), which
    /// the viewpoint's F3 adoption copies onto its soldier row.
    pub viewpoint_spawn: Vec<f64>,
    /// F3Factory.reserve's registry bound when the recording reserved it
    /// before `from` (the F4 owners' capacities read it).
    pub construction_limit: Option<u32>,
    /// The unit registry the host hands over before attachF3 (the
    /// KIND_UNITS import: the viewpoint, the hull and turret targets).
    pub registry: Option<crate::gen::Units>,
    /// The pose accumulators the page made at LiveObjects construction
    /// (anim-kernel.js newPose), in call order: the animated props' first.
    pub poses: Vec<u32>,
    /// createNativeAI's setup row as the host sent it: the random stream
    /// registry's ids and states and the difficulty tier are the host's
    /// F3Random and page facts, taken from here until they are native.
    pub ai_setup: Option<crate::gen::AiWorldSetup>,
    /// The animated props' built bodies as their recorded births name them.
    pub animated_bodies: Vec<Option<Ref<crate::gen::Body>>>,
}

/// The stages that already run cold and bit-exact from content alone; the
/// ones before `from` are run on a scratch world to fill the context.
/// Startup is not among them: game.js validates the startup class only for
/// a page (`!headless && !network`), so a headless host's recording has no
/// f4_startup_class and interning its row's strings would shift every id.
const COLD_PREFIX: [Stage; 6] = [Stage::Content, Stage::Kernels, Stage::Cameras, Stage::Teams, Stage::Store, Stage::Combat];

/// What the partial run produced: every call of the stages from `from`
/// on, in order, and the error that stopped it, if one did (the calls
/// before the error are still compared).
pub struct Partial {
    pub calls: Calls,
    pub stopped: Option<AdmitError>,
}

/// Runs the stages from `from` through `through` on `world`, whose recorded
/// prefix is already applied. `files` and `options` are the battle's, as
/// `admit` takes them.
pub fn run(world: &mut World, files: &dyn Files, options: &Options, from: Stage, through: Stage, fixtures: &Fixtures) -> Result<Partial, AdmitError> {
    let loader = bfe_content::battle::Loader::new(files);
    let battle = bfe_content::battle::Battle::load(&loader, &options.map, &options.query())
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let content = bfe_content::content::Content::build(battle)
        .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let defs = bfe_content::defs::Defs::build(&content).map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let at = super::sequence::ORDER.iter().position(|stage| *stage == from).ok_or(AdmitError::Unavailable(from))?;
    let end = super::sequence::ORDER.iter().position(|stage| *stage == through).ok_or(AdmitError::Unavailable(through))?;
    if end < at { return Err(AdmitError::Unavailable(through)); }
    if !COLD_PREFIX.contains(&super::sequence::ORDER[at]) && at < COLD_PREFIX.len() { return Err(AdmitError::Unavailable(from)); }
    // The scratch world starts from the recorded world's string table so the
    // ids the prefix builders write are the ids the real world has; the
    // prefix interns nothing new (the recording interned it all already).
    let mut scratch = World::new();
    scratch.sim.strings = world.sim.strings.clone();
    let mut cx = Cx::new(&content, &defs, files, options, world.sim.strings.clone());
    let mut rows = Rows { setup: rows::setup(&content.battle), ..Default::default() };
    let mut admitted = Admitted { setup: rows.setup.clone(), ..Default::default() };
    let prefix: Vec<Stage> = COLD_PREFIX.iter().copied().filter(|stage| super::sequence::ORDER.iter().position(|s| s == stage) < Some(at)).collect();
    super::sequence::run_stages(&mut scratch, &content, &mut cx, &mut rows, &mut admitted, &prefix)?;
    cx.refresh_strings(&world.sim.strings);
    // game.js hands the registry over whole before attachF3; the cold
    // sequence keeps it as members are added, the recorded prefix did not.
    if let Some(registry) = &fixtures.registry { world.sim.store.units = registry.clone(); }
    bind_definitions(&mut cx, &content)?;
    bind(&mut cx, &content, fixtures)?;
    // The stream registrations the replayed prefix made (the sequence's
    // Units and Ai seams), so the later owners' ids continue from them.
    let before = |stage: Stage| super::sequence::ORDER.iter().position(|s| *s == stage) < Some(at);
    if before(Stage::Units) {
        for key in [rows::battle::streams::GAMEPLAY, rows::battle::streams::GRENADE_SPIN, rows::battle::streams::SEAT_WEAPONS] { cx.streams.register(key); }
    }
    if before(Stage::Ai) {
        cx.streams.register(rows::battle::streams::WORLD_RANDOM);
        cx.streams.register(rows::battle::streams::WORLD_RANDOM_B);
    }
    let mut calls = Vec::new();
    for &stage in &super::sequence::ORDER[at..=end] {
        let before = rows.calls.len();
        if let Err(error) = super::sequence::run_stages(world, &content, &mut cx, &mut rows, &mut admitted, &[stage]) {
            calls.extend(rows.calls.drain(before..).flat_map(|(_, calls)| calls));
            return Ok(Partial { calls, stopped: Some(error) });
        }
        calls.extend(rows.calls.drain(before..).flat_map(|(_, calls)| calls));
    }
    Ok(Partial { calls, stopped: None })
}

/// What the fleet's and the turrets' construction registered with combat
/// and F1 before attachF3, in the host's order: for each live hull's seat
/// weapons, f2-firing.js adopt's registerWeaponClass(levels[0]) then
/// ordnance.defId(def.ordnance); for each turret, f2-seats.js
/// attachTurrets' same pair; then the prepared variants' seat weapons. The
/// definition tables then continue from the ids the recorded world holds.
/// The calls are dropped: the recorded prefix already made them.
fn bind_definitions(cx: &mut Cx, content: &bfe_content::content::Content) -> Result<(), AdmitError> {
    use bfe_content::Value;
    fn register(cx: &mut Cx, def: &Value) -> Result<(), AdmitError> {
        rows::combat::with_bindings(cx, Stage::Fleet, |bindings, strings| {
            bindings.combat.weapon(def, strings)?;
            let mut calls = std::mem::take(&mut bindings.combat.calls);
            if let Some(ordnance) = def.get("ordnance").filter(|v| v.truthy()) {
                bindings.ordnance.definition(ordnance, strings)?;
                calls.append(&mut bindings.ordnance.calls);
            }
            Ok(((), calls))
        })?;
        Ok(())
    }
    let battle = &content.battle;
    let plans = match battle.vehicle_def.as_ref() { Some(data) => rows::fleet::hulls(data, &battle.world)?, None => Vec::new() };
    let seat_weapons = |name: &str| -> Vec<Value> {
        battle.vehicle_def.as_ref().and_then(|data| data.get("classes")).and_then(|classes| classes.get(name))
            .and_then(|class| class.get("seats")).and_then(Value::as_array).unwrap_or(&[]).iter()
            .flat_map(|seat| seat.get("weapons").and_then(Value::as_array).unwrap_or(&[]).iter())
            .filter_map(|weapon| weapon.get("def").filter(|v| v.truthy()).cloned()).collect()
    };
    // F2Firing.adopt derives each tank launcher per hull (vehicleMissileDefinition):
    // a fresh weapon and ordnance identity for every hull, shared otherwise.
    for plan in plans.iter().filter(|plan| !plan.prepared) {
        for def in seat_weapons(&plan.name) { rows::combat::prepare_vehicle_weapon(cx, &def, Stage::Fleet)?; }
    }
    if let Some(props) = battle.props.as_ref() {
        let instances = props.get("instances").and_then(Value::as_array).unwrap_or(&[]);
        for source in rows::props::live_sources(battle) {
            let desc = &instances[source];
            if desc.get("base").and_then(Value::as_str) != Some("armedbuilding") { continue; }
            if let Some(weapon) = desc.get("weapon").filter(|v| v.truthy()) { register(cx, weapon)?; }
        }
    }
    for plan in plans.iter().filter(|plan| plan.prepared) {
        for def in seat_weapons(&plan.name) { rows::combat::prepare_vehicle_weapon(cx, &def, Stage::Fleet)?; }
    }
    Ok(())
}

/// The fleet's and the props' bindings from the fixtures, as the fleet and
/// session lanes' live constructors will leave them in the context.
fn bind(cx: &mut Cx, content: &bfe_content::content::Content, fixtures: &Fixtures) -> Result<(), AdmitError> {
    let error = |message: &str| AdmitError::Content { stage: Stage::Fleet, message: message.into() };
    cx.ids.body_serial = fixtures.body_serial;
    cx.ids.bodies = fixtures.bodies.iter().map(|name| (name.body, name.kernel)).collect();
    cx.ids.bodies_changed = fixtures.bodies_changed;
    // props::BuildingBinding from the recorded F3BuildingSetup rows: the
    // live constructor's identities, with the explosion left to the builder.
    let sources = rows::props::live_sources(&content.battle);
    if sources.len() != fixtures.buildings.len() { return Err(error("recorded buildings do not cover the live-object construction")); }
    for (index, (row, source)) in fixtures.buildings.iter().zip(&sources).enumerate() {
        if row.index as usize != index { return Err(error("recorded buildings are out of live-object order")); }
        let trap = row.trap.as_ref().map(|trap| rows::props::Trap {
            pad_sphere: row.trap_sphere.clone().unwrap_or_default(), point: trap.settings.point, effect_at: trap.settings.effect_at,
            build_frames: trap.settings.build_frames, trigger_frames: trap.settings.trigger_frames,
        });
        let mut bodies = [None; 3];
        for (slot, body) in bodies.iter_mut().zip(&row.bodies) { *slot = *body; }
        cx.ids.props.push(rows::props::BuildingBinding { source: *source, binding: rows::props::Binding {
            index: row.index, target: row.target, member: row.member, bodies, generation: row.state.generation,
            team: row.state.team, target_team: row.state.target_team, category: row.category, kind: row.kind,
            damaged: row.damaged, crew: row.live.crew, trap,
        } });
    }
    // units::HullBinding from the recorded F3HullSetup rows, named by the
    // fleet's plan order (initial hulls, then prepared variants).
    let Some(data) = content.battle.vehicle_def.as_ref() else {
        if fixtures.hulls.is_empty() { return bind_facts(cx, fixtures); }
        return Err(error("recorded hulls without a vehicle pack"));
    };
    let plans = rows::fleet::hulls(data, &content.battle.world)?;
    if plans.len() != fixtures.hulls.len() { return Err(error("recorded hull count differs from the fleet's plan")); }
    let mut live = 0usize;
    for (plan, (setup, body, changed)) in plans.iter().zip(&fixtures.hulls) {
        let key = if plan.prepared { String::new() } else { live += 1; format!("v{}", live - 1) };
        let mut setup = setup.clone();
        setup.explosion = None; setup.final_explosion = None;
        let vehicle = (plan.name.clone(), f64::from(setup.hull.slot));
        if !cx.ids.vehicles.contains(&vehicle) { cx.ids.vehicles.push(vehicle); }
        cx.ids.hulls.push(rows::units::HullBinding { name: plan.name.clone(), key, setup, body: body.clone(), changed_bodies: changed.clone() });
    }
    // F2Riders.ensureAll's holders: the hulls' rigs from the fleet's refs,
    // then the stationary turrets (props.js Turret) in emplacement order,
    // each TurretSeats.attachTurrets' ref in that order.
    cx.ids.fleet = Some(rows::fleet::Bindings { world: 0, blockers: 0, bodies: Vec::new(), hulls: fixtures.hulls.iter().map(|(setup, _, _)| rows::fleet::HullBinding {
        hull: setup.hull, spawned: true, collider: Default::default(), walls: Vec::new(), springs: Vec::new(),
    }).collect() });
    if let Some(props) = content.battle.props.as_ref() {
        let instances = props.get("instances").and_then(bfe_content::Value::as_array).unwrap_or(&[]);
        let models = props.get("models");
        let mut turret = 0u32;
        for source in &sources {
            let desc = &instances[*source];
            if desc.get("base").and_then(bfe_content::Value::as_str) != Some("armedbuilding") { continue; }
            let model = desc.get("model").and_then(bfe_content::Value::as_str).and_then(|name| models.and_then(|m| m.get(name)))
                .ok_or_else(|| error("turret has no model"))?;
            let geometry = super::bodies::turret_geometry(desc, model, turret as usize)?;
            // The turret store numbers its entries from generation 1.
            cx.holders.push(super::bodies::Holder::Turret { turret: Ref::new(turret, 1), rider: geometry.rider });
            turret += 1;
        }
    }
    rows::fleet::retain_holders(cx)?;
    // attachF2's weapon registrations, in the recorded cmb_weapon order:
    // each live hull's seat weapons (vehicleMissileDefinition derives the
    // launchers), then every stationary turret's weapon, then the prepared
    // hulls' seats. The calls were replayed; only the ids are needed here,
    // so the later owners (F4Live, F4Tail) name the definitions the host did.
    let classes = data.get("classes");
    let seat_weapons = |cx: &mut Cx, plan: &rows::fleet::Hull| -> Result<(), AdmitError> {
        let def = classes.and_then(|c| c.get(&plan.name)).ok_or_else(|| error("a fleet plan names a missing vehicle class"))?;
        for seat in def.get("seats").and_then(bfe_content::Value::as_array).unwrap_or(&[]) {
            for weapon in seat.get("weapons").and_then(bfe_content::Value::as_array).unwrap_or(&[]) {
                let Some(authored) = weapon.get("def").filter(|v| v.truthy()) else { continue };
                rows::combat::prepare_vehicle_weapon(cx, authored, Stage::Fleet)?;
            }
        }
        Ok(())
    };
    for plan in plans.iter().filter(|plan| !plan.prepared) { seat_weapons(cx, plan)?; }
    if let Some(props) = content.battle.props.as_ref() {
        let instances = props.get("instances").and_then(bfe_content::Value::as_array).unwrap_or(&[]);
        for source in &sources {
            let desc = &instances[*source];
            if desc.get("base").and_then(bfe_content::Value::as_str) != Some("armedbuilding") { continue; }
            if let Some(weapon) = desc.get("weapon").filter(|w| w.truthy()) { rows::combat::prepare_weapon(cx, weapon, Stage::Fleet)?; }
        }
    }
    for plan in plans.iter().filter(|plan| plan.prepared) { seat_weapons(cx, plan)?; }
    bind_facts(cx, fixtures)
}

/// The battle facts the stock's instrumentation reads (the gameplay random
/// binding); the session lane derives the rest live.
fn bind_facts(cx: &mut Cx, fixtures: &Fixtures) -> Result<(), AdmitError> {
    cx.ids.battle = Some(rows::battle::Facts { gameplay: fixtures.gameplay, construction_limit: fixtures.construction_limit, ..Default::default() });
    // The recording keeps body_spawn's output row; the native answer is its
    // whole block, header first.
    if !fixtures.viewpoint_spawn.is_empty() {
        let mut block = vec![0.0; crate::store::block::HEADER];
        block.extend_from_slice(&fixtures.viewpoint_spawn);
        cx.returned_rows.push(("body_spawn", block));
    }
    // The animated props' poses are the first the page made, one per prop
    // in instance order (LiveObjects constructs them before any body's).
    let animated = rows::battle::animated::instances(cx);
    cx.ids.animated_poses = animated.iter().copied().zip(fixtures.poses.iter().copied()).collect();
    for (instance, body) in animated.iter().zip(&fixtures.animated_bodies) {
        // The body-name rows and the birth row number the same pooled body
        // with different generations: match by its slot, keep the birth's ref.
        if let Some(body) = body {
            if let Some(kernel) = fixtures.bodies.iter().find(|n| n.body.slot == body.slot).map(|n| n.kernel) {
                cx.ids.prop_kernels.push((*instance, 0, kernel));
                if let Some(entry) = cx.ids.bodies.iter_mut().find(|(_, k)| *k == kernel) { entry.0 = *body; }
            }
        }
    }
    if let Some(setup) = &fixtures.ai_setup {
        let streams = setup.settings.streams.clone().unwrap_or_default();
        cx.ids.ai = Some(rows::ai::Receipts { setup: rows::ai::SetupFacts {
            stream_a: setup.settings.stream_a, stream_b: setup.settings.stream_b, state_a: streams.a, state_b: streams.b,
            tier: setup.settings.tier, ..Default::default() }, ..Default::default() });
    }
    Ok(())
}
