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
use bfe_sim::vehicles::replay::{body_of, hull_from_value, hull_to_value};
use bfe_sim::vehicles::{BodyId, Ground, Hit, Input, Polygon, VehicleWorld};
use sim_replay::{check_bool, check_num, check_vec3, diff, replay_corpus, same_number, Arena, Call, Corpus, Log, Node, Outcome, V};

fn vec3(v: V) -> Vector3 {
    let a = v.vec3().unwrap_or_else(|| panic!("not a Vector3: {}", v.show()));
    Vector3::new(a[0], a[1], a[2])
}

// A frame's calls, served from its capture.
struct Served<'f> {
    log: Log<'f>,
    hull: usize,
    water: Option<f64>,
    error: Option<String>,
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

    fn check_hull(&mut self, what: &str, v: V) {
        if v.handle_num("vehicle") != Some(self.hull as f64) {
            self.fail(format!("{what}: the capture has {}, the port hull {}", v.show(), self.hull));
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
        debug_assert_eq!(hull, self.hull);
        self.check_hull("pushWalls hull", call.arg(0));
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
        debug_assert_eq!(hull, self.hull);
        self.check_hull("springPolygons hull", call.arg(0));
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

    fn damaged(&mut self, hull: usize, amount: f64) {
        let Some(call) = self.call("world.damaged") else { return };
        debug_assert_eq!(hull, self.hull);
        self.check_hull("damaged hull", call.arg(0));
        self.check(check_num("damaged amount", call.arg(1), amount));
        if !call.arg(2).is_nullish() {
            self.fail(format!("damaged attacker: the capture has {}, the port none", call.arg(2).show()));
        }
    }

    fn destroyed_crew(&mut self, hull: usize) {
        let Some(call) = self.call("vehicles.destroyedCrew") else { return };
        debug_assert_eq!(hull, self.hull);
        self.check_hull("destroyedCrew hull", call.arg(0));
    }

    fn destroyed(&mut self, hull: usize) {
        let Some(call) = self.call("world.destroyed") else { return };
        debug_assert_eq!(hull, self.hull);
        self.check_hull("destroyed hull", call.arg(0));
    }

    fn explosion(&mut self, hull: usize, _final_explosion: bool, position: &Vector3) {
        let Some(call) = self.call("world.explosion") else { return };
        debug_assert_eq!(hull, self.hull);
        if call.arg(0).handle_str("def").is_none() {
            self.fail(format!("explosion def: the capture has {}", call.arg(0).show()));
        }
        self.check(check_vec3("explosion position", call.arg(1), vec(position)));
        self.check_hull("explosion owner", call.arg(2));
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
    let mut paths: std::collections::BTreeMap<&'static str, (usize, usize)> = Default::default();
    let report = replay_corpus(&corpus, "vehicle.tick", |_capture, frame| {
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
        let mut hull = hull_from_value(before, index);
        let path = path_of(&hull);
        let tally = paths.entry(path).or_default();
        tally.0 += 1;
        let dt = frame.arg(0).num();
        let input = frame.arg(1);
        let input = Input { forward: input_num(input, "forward"), right: input_num(input, "right"), turn: input_num(input, "turn"), brake: input.get("brake").truthy() };
        let mut served = Served { log: Log::new(frame), hull: index, water, error: None };
        hull.tick(&mut served, dt, &input);
        if let Some(e) = served.error {
            return Outcome::Failed(e);
        }
        if let Err(e) = served.log.finish() {
            return Outcome::Failed(e.0);
        }
        let mut arena = Arena::default();
        let name = before.get("name").str().unwrap_or("?").to_string();
        let mine = hull_to_value(&mut arena, &hull, &name);
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
            let mut served = Served { log: Log::new(frame), hull: index as usize, water: None, error: None };
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
