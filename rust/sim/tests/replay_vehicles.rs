// The hulls' tick on the ground (src/vehicles.js Vehicle.tick with
// src/vehicle-hover.js), replayed against every vehicle.tick frame of the
// corpus BFE_REPLAY_CORPUS names (tools/sim-capture.mjs --specs vehicles).
// Each frame starts from the hull as captured (tools/sim-capture/specs.mjs
// hullState); its calls to the rest of the battle (src/vehicles-world.js:
// the collision world, the water, the death regions, the battle's hooks)
// are served from the capture with every argument checked bit for bit; the
// hull it ends with is compared with the capture's. A flyer's tick is the
// flight package's: its frames are counted, not replayed. Without a corpus
// the test passes without replaying.
#![cfg(feature = "replay")]

use three::{Matrix4, Quaternion, Vector3};
use bfe_sim::gen::{OrdnanceOwner, Replay, Vehicle};
use bfe_sim::vehicles::class::HullClass;
use bfe_sim::vehicles::replay::body_of;
use bfe_sim::vehicles::Hull;
use bfe_sim::vehicles::{BodyId, Ground, Hit, Input, Polygon, VehicleWorld};
use sim_replay::{check_bool, check_num, check_vec3, diff, replay_corpus, same_number, Arena, Call, Corpus, Log, Node, Outcome, V};

fn vec3(v: V) -> Vector3 {
    let a = v.vec3().unwrap_or_else(|| panic!("not a Vector3: {}", v.show()));
    Vector3::new(a[0], a[1], a[2])
}

// A frame's calls, served from its capture.
struct Served<'f> {
    log: Log<'f>,
    water: Option<f64>,
    error: Option<String>,
    /// Each hull's store handle (slot, generation) to its place in the battle's list.
    vehicles: std::collections::HashMap<(u32, u32), usize>,
}

impl<'f> Served<'f> {
    fn fail(&mut self, what: String) {
        if self.error.is_none() {
            self.error = Some(format!("{}: {what}", self.log.frame.place()));
        }
    }

    // The next call, which must be `name` (None once the replay has parted).
    fn call(&mut self, name: &str) -> Option<&'f Call> {
        if self.error.is_some() {
            return None;
        }
        match self.log.call(name) {
            Ok(call) => Some(call),
            Err(e) => {
                self.error = Some(e.0);
                None
            }
        }
    }

    fn check(&mut self, result: Result<(), sim_replay::Mismatch>) {
        if let Err(e) = result {
            self.fail(e.0);
        }
    }

    fn check_hull(&mut self, what: &str, v: V, hull: usize) {
        if v.handle_num("vehicle") != Some(hull as f64) {
            self.fail(format!("{what}: the capture has {}, the port hull {hull}", v.show()));
        }
    }

    fn check_body(&mut self, what: &str, v: V, mine: Option<BodyId>) {
        let theirs = if v.is_nullish() { None } else { Some(body_of(v)) };
        if theirs != mine {
            self.fail(format!("{what}: the capture has {}, the port {mine:?}", v.show()));
        }
    }

    fn check_quat(&mut self, what: &str, v: V, q: &Quaternion) {
        match v.node() {
            Some(Node::Quat(c)) if same_number(c[0], q.x) && same_number(c[1], q.y) && same_number(c[2], q.z) && same_number(c[3], q.w) => {}
            _ => self.fail(format!("{what}: the capture has {}, the port {q:?}", v.show())),
        }
    }
}

fn vec(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

impl<'f> VehicleWorld for Served<'f> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        let call = self.call("collision.cast")?;
        self.check(check_vec3("cast origin", call.arg(0), vec(origin)));
        self.check(check_vec3("cast direction", call.arg(1), vec(direction)));
        self.check(check_num("cast far", call.arg(2), far));
        self.check_body("cast ignoreBody", call.arg(4), ignore);
        let r = call.result();
        if r.is_null() {
            return None;
        }
        let body = r.get("body");
        Some(Hit {
            distance: r.get("distance").num(),
            point: vec3(r.get("point")),
            normal: vec3(r.get("normal")),
            terrain: r.get("terrain").truthy(),
            body: if body.is_nullish() { None } else { Some(body_of(body)) },
        })
    }

    fn push_walls(&mut self, hull: usize, probes: &[Vector3], radius: f64, q: &Quaternion, own: Option<&Quaternion>, previous: &Vector3, position: &mut Vector3, velocity: &mut Vector3, terrain: bool) -> bool {
        let Some(call) = self.call("world.pushWalls") else { return false };
        self.check_hull("pushWalls hull", call.arg(0), hull);
        let theirs = call.arg(1);
        if theirs.len() != probes.len() {
            self.fail(format!("pushWalls probes: the capture has {}, the port {}", theirs.len(), probes.len()));
        }
        for (i, p) in probes.iter().enumerate() {
            self.check(check_vec3(&format!("pushWalls probe {i}"), theirs.at(i), vec(p)));
        }
        self.check(check_num("pushWalls radius", call.arg(2), radius));
        self.check_quat("pushWalls q", call.arg(3), q);
        match own {
            None if call.arg(4).is_null() => {}
            Some(own) => self.check_quat("pushWalls own", call.arg(4), own),
            None => self.fail(format!("pushWalls own: the capture has {}, the port none", call.arg(4).show())),
        }
        self.check(check_vec3("pushWalls previous", call.arg(5), vec(previous)));
        self.check(check_vec3("pushWalls position", call.arg(6), vec(position)));
        self.check(check_vec3("pushWalls velocity", call.arg(7), vec(velocity)));
        self.check(check_bool("pushWalls terrain", call.arg(8), terrain));
        *position = vec3(call.out(6));
        *velocity = vec3(call.out(7));
        call.result().truthy()
    }

    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<Ground> {
        let call = self.call("collision.ground")?;
        self.check(check_num("ground x", call.arg(0), x));
        self.check(check_num("ground z", call.arg(1), z));
        self.check(check_num("ground top", call.arg(2), top));
        let r = call.result();
        if r.is_null() {
            return None;
        }
        Some(Ground { height: r.get("height").num(), normal: vec3(r.get("normal")) })
    }

    fn spring_polygons(&mut self, hull: usize, index: usize, center: &Vector3, radius: f64, out: &mut Vec<Polygon>) {
        out.clear();
        let Some(call) = self.call("world.springPolygons") else { return };
        self.check_hull("springPolygons hull", call.arg(0), hull);
        self.check(check_num("springPolygons index", call.arg(1), index as f64));
        self.check(check_vec3("springPolygons center", call.arg(2), vec(center)));
        self.check(check_num("springPolygons radius", call.arg(3), radius));
        for p in call.result().items() {
            out.push(Polygon {
                id: p.get("id").num(),
                moving: p.get("moving").truthy(),
                triangles: p.get("triangles").items().map(|t| [vec3(t.get("a")), vec3(t.get("b")), vec3(t.get("c"))]).collect(),
            });
        }
    }

    fn move_body(&mut self, body: BodyId, matrix: &Matrix4) {
        let Some(call) = self.call("collision.moveBody") else { return };
        self.check_body("moveBody body", call.arg(0), Some(body));
        match call.arg(1).node() {
            Some(Node::Mat4(e)) if e.iter().zip(matrix.elements.iter()).all(|(a, b)| same_number(*a, *b)) => {}
            _ => self.fail(format!("moveBody matrix: the capture has {}, the port {:?}", call.arg(1).show(), matrix.elements)),
        }
    }

    fn body_enabled(&mut self, body: BodyId) -> bool {
        let Some(call) = self.call("world.bodyEnabled") else { return false };
        self.check_body("bodyEnabled body", call.arg(0), Some(body));
        call.result().truthy()
    }

    fn enable_body(&mut self, body: BodyId, enabled: bool) {
        let Some(call) = self.call("world.enableBody") else { return };
        self.check_body("enableBody body", call.arg(0), Some(body));
        self.check(check_bool("enableBody enabled", call.arg(1), enabled));
    }

    fn water_over(&mut self, x: f64, z: f64) -> bool {
        if self.water.is_none() {
            return false;
        }
        let Some(call) = self.call("water.over") else { return false };
        self.check(check_num("water.over x", call.arg(0), x));
        self.check(check_num("water.over z", call.arg(1), z));
        call.result().truthy()
    }

    fn water_level(&mut self) -> f64 {
        self.water.expect("the water's level, on a map without water")
    }

    fn has_water(&mut self) -> bool {
        self.water.is_some()
    }

    fn in_death_region(&mut self, position: &Vector3) -> bool {
        let Some(call) = self.call("world.inDeathRegion") else { return false };
        self.check(check_vec3("inDeathRegion position", call.arg(0), vec(position)));
        call.result().truthy()
    }

    fn damaged(&mut self, hull: usize, amount: f64, attacker: &OrdnanceOwner) {
        let Some(call) = self.call("world.damaged") else { return };
        self.check_hull("damaged hull", call.arg(0), hull);
        self.check(check_num("damaged amount", call.arg(1), amount));
        let theirs = call.arg(2);
        let same = match attacker {
            OrdnanceOwner::Null => theirs.is_nullish(),
            OrdnanceOwner::Player => theirs.str() == Some("player"),
            OrdnanceOwner::Unit(r) => theirs.handle_num("unit") == Some(r.slot as f64),
            OrdnanceOwner::Vehicle(r) => self.vehicles.get(&(r.slot, r.gen)).is_some_and(|&i| theirs.handle_num("vehicle") == Some(i as f64)),
            OrdnanceOwner::OwnerSnapshot(_) => theirs.handle().is_some_and(|(k, _)| k == "ownerSnapshot"),
        };
        if !same {
            self.fail(format!("damaged attacker: the capture has {}, the port {attacker:?}", theirs.show()));
        }
    }

    fn destroyed_crew(&mut self, hull: usize) {
        let Some(call) = self.call("vehicles.destroyedCrew") else { return };
        self.check_hull("destroyedCrew hull", call.arg(0), hull);
    }

    fn destroyed(&mut self, hull: usize) {
        let Some(call) = self.call("world.destroyed") else { return };
        self.check_hull("destroyed hull", call.arg(0), hull);
    }

    fn explosion(&mut self, hull: usize, _final_explosion: bool, position: &Vector3) {
        let Some(call) = self.call("world.explosion") else { return };
        if call.arg(0).handle_str("def").is_none() {
            self.fail(format!("explosion def: the capture has {}", call.arg(0).show()));
        }
        self.check(check_vec3("explosion position", call.arg(1), vec(position)));
        self.check_hull("explosion owner", call.arg(2), hull);
    }

    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<f64> {
        let call = self.call("units.raycast")?;
        self.check(check_vec3("units.raycast origin", call.arg(0), vec(origin)));
        self.check(check_vec3("units.raycast direction", call.arg(1), vec(direction)));
        self.check(check_num("units.raycast far", call.arg(2), far));
        let r = call.result();
        if r.is_nullish() {
            None
        } else {
            Some(r.get("distance").num())
        }
    }

    fn tow_released(&mut self, hull: usize, reason: &str) {
        let Some(call) = self.call("world.sound") else { return };
        if call.arg(0).str() != Some("towRelease") || call.arg(3).str() != Some(reason) {
            self.fail(format!("tow released: the capture has {} {}, the port towRelease {reason}", call.arg(0).show(), call.arg(3).show()));
        }
        self.check_hull("towRelease hull", call.arg(1), hull);
    }

    fn clock(&mut self) -> f64 {
        match self.call("world.clock") {
            Some(call) => call.result().num(),
            None => 0.0,
        }
    }

    fn effect(&mut self, name: &str, position: &Vector3) {
        let Some(call) = self.call("world.effect") else { return };
        if call.arg(0).str() != Some(name) {
            self.fail(format!("effect name: the capture has {}, the port {name}", call.arg(0).show()));
        }
        self.check(check_vec3("effect position", call.arg(1), vec(position)));
    }
}

// An input field as the tick reads it (a missing one is NaN, which `|| 0` makes 0).
fn input_num(v: V, key: &str) -> f64 {
    v.get(key).opt_num().unwrap_or(f64::NAN)
}

// What a replayed tick ran: (frames, of which the same) by path.
fn path_of(hull: &bfe_sim::vehicles::Hull) -> &'static str {
    if !hull.alive || hull.sleeping == Some(true) && !hull.occupied() {
        "asleep or dead (no step)"
    } else if !hull.walking {
        match &hull.hover_class {
            Some(h) if h.springs.is_empty() => "hover without springs",
            _ if hull.wheeled => "hover on springs, wheeled",
            _ => "hover on springs",
        }
    } else if hull.creature {
        "riding animal"
    } else if hull.legs.is_some() {
        "walker on its legs"
    } else {
        "walker on its probes"
    }
}

#[test]
fn hulls_tick_on_the_ground_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    if std::env::var_os("BFE_CONTENT_DIR").is_none() {
        eprintln!("hulls_tick_on_the_ground_as_captured: no BFE_CONTENT_DIR (the hulls' classes are the content's): not replayed");
        return;
    }
    let mut paths: std::collections::BTreeMap<&'static str, (usize, usize)> = Default::default();
    let mut content: Option<(String, bfe_content::content::Content)> = None;
    let mut classes: std::collections::HashMap<String, HullClass> = Default::default();
    let report = replay_corpus(&corpus, "vehicle.tick", |capture, frame| {
        let label = capture.label();
        if content.as_ref().map(|(l, _)| l != &label).unwrap_or(true) {
            content = Some((label.clone(), content_of(capture.header.root()).expect("the capture's battle")));
            classes.clear();
        }
        let before = frame.before().get("hull");
        if before.get("flying").bool() {
            return Outcome::Skipped("a flyer's tick (the flight package's)".into());
        }
        let Some(index) = frame.receiver().handle_num("vehicle") else {
            return Outcome::Failed(format!("{}: no hull receiver ({})", frame.place(), frame.receiver().show()));
        };
        let index = index as usize;
        let water = frame.before().get("water");
        let water = if water.is_null() { None } else { Some(water.get("level").num()) };
        let mut cx = Replay::default();
        let r = match Vehicle::value_in(before, &mut cx, "hull") {
            Ok(r) => r,
            Err(e) => return Outcome::Failed(format!("{}: {e}", frame.place())),
        };
        let stored = cx.store.vehicle.get(r).unwrap().clone();
        let name = before.get("name").str().unwrap_or("?").to_string();
        let c = &content.as_ref().unwrap().1;
        let class = classes.entry(name.clone()).or_insert_with(|| {
            let def = c.battle.vehicle_def.as_ref().and_then(|v| v.get("classes")).and_then(|cl| cl.get(&name)).expect("the hull's class in the content");
            HullClass::new(def, stored.turn_rate)
        });
        let mut hull = Hull::from_store(r, &stored, class, index, &cx.strings);
        // The collision body by the serial the capture names it by.
        hull.body = body_of(before.get("body"));
        let path = path_of(&hull);
        let tally = paths.entry(path).or_default();
        tally.0 += 1;
        let dt = frame.arg(0).num();
        let input = frame.arg(1);
        let input = Input { forward: input_num(input, "forward"), right: input_num(input, "right"), turn: input_num(input, "turn"), brake: input.get("brake").truthy() };
        let mut served = Served { log: Log::new(frame), water, error: None, vehicles: [((r.slot, r.gen), index)].into_iter().collect() };
        hull.tick(&mut served, dt, &input);
        if let Some(e) = served.error {
            return Outcome::Failed(e);
        }
        if let Err(e) = served.log.finish() {
            return Outcome::Failed(e.0);
        }
        hull.to_store(cx.store.vehicle.get_mut(r).unwrap(), &mut cx.strings);
        let mut arena = Arena::default();
        let mine = Vehicle::value_out(r, &mut cx, &mut arena);
        match diff(frame.after().get("hull"), arena.view(&mine)) {
            None => {
                paths.get_mut(path).unwrap().1 += 1;
                Outcome::Same
            }
            Some(d) => Outcome::Failed(format!("{}: hull {d} (capture | port)", frame.place())),
        }
    })
    .unwrap();
    eprintln!("{}", report.summary());
    for (path, (frames, same)) in &paths {
        eprintln!("  {path}: {frames} frames, {same} the same");
    }
    report.assert_clean();
}

/// The battle's content as rust/content loads it (BFE_CONTENT_DIR), for
/// the hulls' rigs: the capture's battle, loaded as world-hash asked for it.
fn content_of(header: V) -> Option<bfe_content::content::Content> {
    use bfe_content::battle::{Battle, Loader};
    use bfe_content::files::DirFiles;
    let root = std::env::var_os("BFE_CONTENT_DIR")?;
    let b = header.get("battle");
    let (map, side, seed) = (b.get("map").str()?.to_string(), b.get("side").str()?.to_string(), b.get("seed").num());
    let ai = b.get("ai").str().unwrap_or("classic").to_string();
    let era = if side == "rep" || side == "cis" { "cw" } else { "gcw" };
    let query = format!("?map={map}&side={side}&era={era}&ai={ai}&seed={seed}&rays=js&movement=js");
    let files = DirFiles::new(std::path::Path::new(&root));
    let loader = Loader::new(&files);
    let battle = Battle::load(&loader, &map, &query).expect("the battle's content");
    Some(bfe_content::content::Content::build(battle).expect("the battle's content"))
}

type Built = (bfe_sim::rig::hull::HullRig, Option<bfe_sim::vehicles::pose::WalkClips>, Option<bfe_sim::vehicles::pose::LegPairs>);

/// A hull class's rig, built from the content as the Vehicle constructor
/// builds it, with its walk clips and its legs.
fn rig_of(content: &bfe_content::content::Content, class: &str) -> Option<Built> {
    let def = content.battle.vehicle_def.as_ref()?.get("classes")?.get(class)?;
    let model = def.get("model").and_then(|m| m.as_str())?;
    let template = content.vehicle_templates.iter().find(|t| t.name == model)?;
    let rig = bfe_sim::rig::hull::HullRig::new(&bfe_sim::rig::content::hull_desc(def, template));
    Some((rig, bfe_sim::vehicles::pose::walk_clips(template), bfe_sim::vehicles::pose::leg_pairs(def)))
}

#[test]
fn hulls_pose_as_captured() {
    use bfe_sim::vehicles::replay::{pose_from_value, pose_to_value};
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    if std::env::var_os("BFE_CONTENT_DIR").is_none() {
        eprintln!("hulls_pose_as_captured: no BFE_CONTENT_DIR (the rigs are built from the content): not replayed");
        return;
    }
    for name in ["vehicle.aimTick", "vehicle.render"] {
        let mut content: Option<(String, bfe_content::content::Content)> = None;
        let mut rigs: std::collections::HashMap<String, Option<Built>> = Default::default();
        let mut posed_same = 0usize;
        let report = replay_corpus(&corpus, name, |capture, frame| {
            let label = capture.label();
            if content.as_ref().map(|(l, _)| l != &label).unwrap_or(true) {
                content = Some((label.clone(), content_of(capture.header.root()).expect("the capture's battle")));
                rigs.clear();
            }
            let before = frame.before().get("hull");
            if before.get("flying").bool() {
                return Outcome::Skipped("a flyer's pose (the flight package's)".into());
            }
            let class = before.get("name").str().unwrap_or("?").to_string();
            let c = &content.as_ref().unwrap().1;
            let Some((rig, clips, legs)) = rigs.entry(class.clone()).or_insert_with(|| rig_of(c, &class)).clone() else {
                return Outcome::Failed(format!("{}: no rig for {class} in the content", frame.place()));
            };
            let Some(index) = frame.receiver().handle_num("vehicle") else {
                return Outcome::Failed(format!("{}: no hull receiver", frame.place()));
            };
            let posed = !before.get("rig").is_null();
            let mut hull = pose_from_value(before, index as usize, rig);
            let pose = hull.pose.as_mut().unwrap();
            pose.clips = clips;
            pose.leg_pairs = legs;
            let (alpha, dt) = (frame.arg(0).num(), frame.arg(1).num());
            let mut served = Served { log: Log::new(frame), water: None, error: None, vehicles: Default::default() };
            if name == "vehicle.aimTick" {
                hull.aim_tick(&mut served, alpha, dt);
            } else {
                hull.render(&mut served, alpha, dt);
            }
            if let Some(e) = served.error {
                return Outcome::Failed(e);
            }
            if let Err(e) = served.log.finish() {
                return Outcome::Failed(e.0);
            }
            let mut arena = Arena::default();
            let mine = pose_to_value(&mut arena, &mut hull, &class, posed);
            match diff(frame.after().get("hull"), arena.view(&mine)) {
                None => {
                    if posed {
                        posed_same += 1;
                    }
                    Outcome::Same
                }
                Some(d) => Outcome::Failed(format!("{}: hull {d} (capture | port)", frame.place())),
            }
        })
        .unwrap();
        eprintln!("{}\n  posed: {posed_same} of the same frames", report.summary());
        report.assert_clean();
    }
}

/// A hull from its store value in a frame, its class from the content.
fn load_hull(
    cx: &mut Replay,
    v: V,
    index: usize,
    content: &bfe_content::content::Content,
    classes: &mut std::collections::HashMap<String, HullClass>,
) -> Result<(bfe_sim::store::Ref<Vehicle>, Hull), String> {
    let r = Vehicle::value_in(v, cx, "hull")?;
    let stored = cx.store.vehicle.get(r).unwrap().clone();
    let name = v.get("name").str().unwrap_or("?").to_string();
    let class = classes.entry(name.clone()).or_insert_with(|| {
        let def = content.battle.vehicle_def.as_ref().and_then(|d| d.get("classes")).and_then(|cl| cl.get(&name)).expect("the hull's class in the content");
        HullClass::new(def, stored.turn_rate)
    });
    let mut hull = Hull::from_store(r, &stored, class, index, &cx.strings);
    hull.body = body_of(v.get("body"));
    Ok((r, hull))
}

/// A battle's content, kept while the corpus's frames are of its capture.
struct Battles {
    content: Option<(String, bfe_content::content::Content)>,
    classes: std::collections::HashMap<String, HullClass>,
}

impl Battles {
    fn new() -> Battles {
        Battles { content: None, classes: Default::default() }
    }
    fn of(&mut self, capture: &sim_replay::Capture) -> (&bfe_content::content::Content, &mut std::collections::HashMap<String, HullClass>) {
        let label = capture.label();
        if self.content.as_ref().map(|(l, _)| l != &label).unwrap_or(true) {
            self.content = Some((label, content_of(capture.header.root()).expect("the capture's battle")));
            self.classes.clear();
        }
        (&self.content.as_ref().unwrap().1, &mut self.classes)
    }
}

#[test]
fn hulls_dying_and_tick_start_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    if std::env::var_os("BFE_CONTENT_DIR").is_none() {
        return;
    }
    for name in ["vehicle.captureTick", "vehicle.dyingTick"] {
        let mut battles = Battles::new();
        let report = replay_corpus(&corpus, name, |capture, frame| {
            let (content, classes) = battles.of(capture);
            let Some(index) = frame.receiver().handle_num("vehicle") else {
                return Outcome::Failed(format!("{}: no hull receiver", frame.place()));
            };
            let index = index as usize;
            let mut cx = Replay::default();
            let (r, mut hull) = match load_hull(&mut cx, frame.before().get("hull"), index, content, classes) {
                Ok(x) => x,
                Err(e) => return Outcome::Failed(format!("{}: {e}", frame.place())),
            };
            if name == "vehicle.captureTick" && hull.flying {
                return Outcome::Skipped("a flyer's tick start (its quaternion is the flight package's)".into());
            }
            let mut served = Served { log: Log::new(frame), water: None, error: None, vehicles: [((r.slot, r.gen), index)].into_iter().collect() };
            if name == "vehicle.captureTick" {
                hull.capture_tick();
            } else {
                hull.dying_tick(&mut served, frame.arg(0).num());
            }
            if let Some(e) = served.error {
                return Outcome::Failed(e);
            }
            if let Err(e) = served.log.finish() {
                return Outcome::Failed(e.0);
            }
            hull.to_store(cx.store.vehicle.get_mut(r).unwrap(), &mut cx.strings);
            let mut arena = Arena::default();
            let mine = Vehicle::value_out(r, &mut cx, &mut arena);
            match diff(frame.after().get("hull"), arena.view(&mine)) {
                None => Outcome::Same,
                Some(d) => Outcome::Failed(format!("{}: hull {d} (capture | port)", frame.place())),
            }
        })
        .unwrap();
        eprintln!("{}", report.summary());
        report.assert_clean();
    }
}

#[test]
fn tow_cables_as_captured() {
    use bfe_sim::vehicles::tethers::{update_tethers, Tether};
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    if std::env::var_os("BFE_CONTENT_DIR").is_none() {
        return;
    }
    let mut battles = Battles::new();
    let mut held = 0usize;
    let report = replay_corpus(&corpus, "vehicles.updateTethers", |capture, frame| {
        let (content, classes) = battles.of(capture);
        let before = frame.before();
        if before.get("tethers").is_empty() {
            return Outcome::Skipped("no cable".into());
        }
        let mut cx = Replay::default();
        // The hulls at the cables' ends, by their place in the battle's list.
        let mut indices = Vec::new();
        let mut hulls: Vec<Hull> = Vec::new();
        let mut refs = Vec::new();
        for h in before.get("hulls").items() {
            let index = h.get("index").num() as usize;
            match load_hull(&mut cx, h.get("hull"), index, content, classes) {
                Ok((r, hull)) => {
                    indices.push(index);
                    refs.push(r);
                    hulls.push(hull);
                }
                Err(e) => return Outcome::Failed(format!("{}: {e}", frame.place())),
            }
        }
        // Hull i of the battle is at its place in `hulls`: the tethers name them so.
        let at = |index: f64| indices.iter().position(|&i| i as f64 == index);
        let mut tethers: Vec<Tether> = before
            .get("tethers")
            .items()
            .map(|t| Tether {
                source: at(t.get("source").num()).expect("a cable's hull"),
                target: t.get("target").opt_num().and_then(at),
                seat: t.get("seat").num() as usize,
                length: t.get("length").opt_num().unwrap_or(f64::NAN),
                has_def: t.get("hasDef").bool(),
                start: vec3(t.get("start")),
                end: vec3(t.get("end")),
            })
            .collect();
        let mut guns = vec![None; hulls.len()];
        for t in before.get("tethers").items() {
            if let (Some(s), false) = (at(t.get("source").num()), t.get("gun").is_null()) {
                guns[s] = Some(vec3(t.get("gun")));
            }
        }
        // The hulls' own `index` is their place in the battle's list; the
        // trait's calls name them by it.
        if tethers.iter().any(|t| hulls[t.source].tow_target >= 0.0) {
            held += 1;
        }
        let mut served = Served { log: Log::new(frame), water: None, error: None, vehicles: refs.iter().zip(&indices).map(|(r, &i)| ((r.slot, r.gen), i)).collect() };
        update_tethers(&mut hulls, &mut tethers, &guns, &mut served);
        if let Some(e) = served.error {
            return Outcome::Failed(e);
        }
        if let Err(e) = served.log.finish() {
            return Outcome::Failed(e.0);
        }
        for (hull, r) in hulls.iter().zip(&refs) {
            hull.to_store(cx.store.vehicle.get_mut(*r).unwrap(), &mut cx.strings);
        }
        let after = frame.after();
        let mut arena = Arena::default();
        for (k, h) in after.get("hulls").items().enumerate() {
            let mine = Vehicle::value_out(refs[k], &mut cx, &mut arena);
            if let Some(d) = diff(h.get("hull"), arena.view(&mine)) {
                return Outcome::Failed(format!("{}: hull {} {d} (capture | port)", frame.place(), indices[k]));
            }
        }
        for (k, t) in after.get("tethers").items().enumerate() {
            let mine = &tethers[k];
            for (what, v, m) in [("start", t.get("start"), mine.start), ("end", t.get("end"), mine.end)] {
                if let Err(e) = check_vec3(&format!("cable {k} {what}"), v, vec(&m)) {
                    return Outcome::Failed(format!("{}: {}", frame.place(), e.0));
                }
            }
        }
        Outcome::Same
    })
    .unwrap();
    eprintln!("{}\n  with a cable held: {held}", report.summary());
    report.assert_clean();
}
