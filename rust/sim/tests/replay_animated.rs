//! Actual animated prop poses and creature holds. Foreign soldier
//! callbacks validate their entry state; the prop's pose is always native.
#![cfg(feature = "replay")]
use bfe_sim::battle::animated::{Animated, Body, Grab, Held, Settings, Trigger, Victim, World};
use bfe_sim::pose::{Clip, Pose};
use bfe_sim::seats::turrets::TurretRig;
use bfe_sim::store::{StrId, Strings};
use sim_replay::{check_num, check_value, replay_corpus, Arena, Call, Capture, Corpus, Event, Frame, Log, Node, Outcome, V, Value};
use three::{Matrix4, Quaternion, Vector3};

fn vec(v: V) -> Vector3 { Vector3::new(v.at(0).num(), v.at(1).num(), v.at(2).num()) }
fn quat(v: V) -> Quaternion { Quaternion::new(v.at(0).num(), v.at(1).num(), v.at(2).num(), v.at(3).num()) }
fn nums(v: V) -> Vec<f64> { v.items().map(|v| v.num()).collect() }
fn matrix(v: V) -> Matrix4 { let mut m = Matrix4::default(); m.from_array(&nums(v), 0); m }
fn string(v: V, strings: &mut Strings) -> StrId {
    match v.value { Value::Str(s) => strings.intern_str(s), Value::Str16(s) => strings.intern(s), _ => panic!("expected string, {}", v.show()) }
}
fn optional_string(v: V, strings: &mut Strings) -> Option<StrId> { (!v.is_nullish()).then(|| string(v, strings)) }
fn clip(v: V, capture: &Capture) -> Option<Clip> {
    if v.is_nullish() { return None; }
    let v = capture.def_of(v).unwrap_or(v);
    Some(Clip { frames: v.get("frames").num() as usize, fps: v.get("fps").num(), root_motion: v.get("rootMotion").bool(),
        tracks: v.get("tracks").items().map(|x| x.num() as i32).collect(), data: {
            let data = v.get("data").typed().expect("clip data"); (0..data.len()).map(|i| data.get(i).unwrap() as f32).collect()
        } })
}
fn settings(v: V, capture: &Capture, strings: &mut Strings) -> Settings {
    let door = v.get("doorClip"); let door_def = capture.def_of(door).unwrap_or(door);
    Settings { door: v.get("door").bool(), clips: v.get("clips").items().map(|c| clip(c, capture)).collect(), door_clip: clip(door, capture),
        door_duration: door_def.get("duration").opt_num().unwrap_or(0.0), idle_delay: v.get("idleDelay").num(),
        open_slots: v.get("openSlots").items().map(|v| v.bool()).collect(), position: vec(v.get("position")), kill_sound: optional_string(v.get("killSound"), strings),
        triggers: v.get("triggers").items().map(|t| Trigger { slot: t.get("slot").num() as i32, point: vec(t.get("point")), radius: t.get("radius").num(),
            sound: optional_string(t.get("sound"), strings), opens: t.get("opens").bool() }).collect(),
        grabs: v.get("grabs").items().map(|g| Grab { slot: g.get("slot").num() as i32, window: g.get("window").num(), radius: g.get("radius").num(),
            node: g.get("node").opt_num().map(|n| n as usize), clip: string(g.get("clip"), strings) }).collect() }
}
fn body(v: V, key: usize) -> Body {
    Body { key, id: v.get("id").num(), generation: v.get("generation").num(), soldier: v.get("soldier").bool(), walker: v.get("walker").bool(),
        removed: v.get("removed").bool(), alive: v.get("alive").bool(), dead: v.get("dead").bool(), seated: v.get("seated").bool(),
        held: (!v.get("held").is_nullish()).then(|| { let h = v.get("held"); Held { prop: h.get("prop").num() as usize, grab: h.get("grab").num() as usize,
            since: h.get("since").num(), frame: (!h.get("frame").is_null()).then(|| matrix(h.get("frame"))), time: h.get("time").opt_num() } }),
        combat: v.get("combat").bool(), mounted: v.get("mounted").bool(), position: vec(v.get("position")),
        previous: (!v.get("previous").is_null()).then(|| vec(v.get("previous"))), settled: (!v.get("settled").is_null()).then(|| vec(v.get("settled"))),
        velocity: vec(v.get("velocity")), air: v.get("air").bool(), dive: v.get("dive").num(), tick_count: v.get("tickCount").opt_num(),
        let_go: v.get("letGo").bool(), remove: v.get("remove").bool(), rider_clips: v.get("riderClips").items().map(|v| v.bool()).collect() }
}
fn prop(v: V, bodies: V) -> Animated {
    let bones: Vec<_> = v.get("bones").items().map(|b| {
        let parent = b.get("parent").num(); ((parent >= 0.0).then_some(parent as usize), vec(b.get("p")), quat(b.get("q")), vec(b.get("s")))
    }).collect();
    let world: Vec<[f64; 16]> = v.get("bones").items().map(|b| nums(b.get("world")).try_into().unwrap()).collect();
    let root: [f64; 16] = nums(v.get("root")).try_into().unwrap();
    let mut rig = TurretRig::new(&root, &bones, Some(&world));
    rig.tree.node_mut(rig.root).matrix_world = matrix(v.get("rootWorld"));
    let pose = (!v.get("pose").is_nullish()).then(|| {
        let v = v.get("pose"); let mut pose = Pose::new(v.get("w").len(), v.get("root").num() as i32);
        pose.q = nums(v.get("q")).into_iter().map(|x| x as f32).collect(); pose.t = nums(v.get("t")).into_iter().map(|x| x as f32).collect();
        pose.w = nums(v.get("w")).into_iter().map(|x| x as f32).collect(); pose
    });
    Animated { index: v.get("index").num() as usize, slot: v.get("slot").num() as i32, time: v.get("time").num(), weight: v.get("weight").num(),
        looping: v.get("loop").bool(), hold: v.get("hold").num(), started: v.get("started").num(), start_time: v.get("startTime").num(),
        victim: (!v.get("victim").is_nullish()).then(|| { let t = v.get("victim"); Victim {
            unit: bodies.items().position(|b| check_value("victim", b.get("self"), t.get("unit")).is_ok()).expect("victim body"),
            generation: t.get("generation").num(), grab: t.get("grab").num() as usize, since: t.get("since").num() } }),
        openness: v.get("openness").num(), body_enabled: v.get("bodyEnabled").opt_bool(), stand_q: nums(v.get("standQ")).into_iter().map(|x| x as f32).collect(),
        stand_t: nums(v.get("standT")).into_iter().map(|x| x as f32).collect(), pose, rig,
        bind: v.get("bind").items().map(|b| (quat(b.get("q")), vec(b.get("p")))).collect() }
}
fn array(arena: &mut Arena, values: impl IntoIterator<Item = f64>) -> Value { arena.array(values.into_iter().map(Value::Num).collect()) }
fn vector(arena: &mut Arena, v: Vector3) -> Value { array(arena, [v.x, v.y, v.z]) }
fn quaternion(arena: &mut Arena, q: Quaternion) -> Value { array(arena, [q.x, q.y, q.z, q.w]) }
fn body_value(body: &Body, source: V, arena: &mut Arena) -> Value {
    let value = arena.copy(source);
    for (k, v) in [("id", body.id), ("generation", body.generation), ("dive", body.dive)] { arena.assign(&value, k, Value::Num(v)); }
    for (k, v) in [("soldier", body.soldier), ("walker", body.walker), ("removed", body.removed), ("alive", body.alive), ("dead", body.dead),
        ("seated", body.seated), ("combat", body.combat), ("mounted", body.mounted), ("air", body.air), ("letGo", body.let_go), ("remove", body.remove)] { arena.assign(&value, k, Value::Bool(v)); }
    for (k, v) in [("position", Some(body.position)), ("previous", body.previous), ("settled", body.settled), ("velocity", Some(body.velocity))] {
        let point = v.map(|v| vector(arena, v)).unwrap_or(Value::Null); arena.assign(&value, k, point);
    }
    arena.assign(&value, "tickCount", body.tick_count.map(Value::Num).unwrap_or(Value::Null));
    let held = body.held.as_ref().map(|h| {
        let frame = h.frame.as_ref().map(|m| array(arena, m.elements)).unwrap_or(Value::Null);
        arena.object("", vec![("prop", Value::Num(h.prop as f64)), ("grab", Value::Num(h.grab as f64)), ("since", Value::Num(h.since)),
            ("frame", frame), ("time", h.time.map(Value::Num).unwrap_or(Value::Null))])
    }).unwrap_or(Value::Null);
    arena.assign(&value, "held", held); value
}
fn prop_value(p: &Animated, source: V, bodies: &[Body], sources: V, arena: &mut Arena) -> Value {
    let value = arena.copy(source);
    for (k, v) in [("slot", p.slot as f64), ("time", p.time), ("weight", p.weight), ("hold", p.hold), ("started", p.started), ("startTime", p.start_time), ("openness", p.openness)] { arena.assign(&value, k, Value::Num(v)); }
    arena.assign(&value, "loop", Value::Bool(p.looping)); arena.assign(&value, "bodyEnabled", p.body_enabled.map(Value::Bool).unwrap_or(Value::Null));
    let victim = p.victim.as_ref().map(|v| {
        let unit = arena.copy(sources.at(v.unit).get("self"));
        arena.object("", vec![("unit", unit), ("generation", Value::Num(v.generation)), ("grab", Value::Num(v.grab as f64)), ("since", Value::Num(v.since))])
    }).unwrap_or(Value::Null); arena.assign(&value, "victim", victim);
    let q = array(arena, p.stand_q.iter().map(|&n| n as f64)); let t = array(arena, p.stand_t.iter().map(|&n| n as f64));
    arena.assign(&value, "standQ", q); arena.assign(&value, "standT", t);
    if let Some(pose) = &p.pose {
        let q = array(arena, pose.q.iter().map(|&n| n as f64)); let t = array(arena, pose.t.iter().map(|&n| n as f64)); let w = array(arena, pose.w.iter().map(|&n| n as f64));
        let pose = arena.object("", vec![("root", Value::Num(pose.root as f64)), ("q", q), ("t", t), ("w", w)]); arena.assign(&value, "pose", pose);
    }
    let bones = p.rig.bones.iter().enumerate().map(|(i, &id)| {
        let node = p.rig.tree.node(id); let value = arena.copy(source.get("bones").at(i));
        let point = vector(arena, node.position); let turn = quaternion(arena, node.quaternion); let scale = vector(arena, node.scale); let world = array(arena, node.matrix_world.elements);
        for (k, v) in [("p", point), ("q", turn), ("s", scale), ("world", world)] { arena.assign(&value, k, v); } value
    }).collect();
    let bones = arena.array(bones); arena.assign(&value, "bones", bones);
    let world = array(arena, p.rig.tree.node(p.rig.root).matrix_world.elements); arena.assign(&value, "rootWorld", world);
    let record = array(arena, p.state_record(bodies)); arena.assign(&value, "record", record); value
}
struct Served<'f> { log: Log<'f>, source: V<'f>, sources: V<'f>, strings: Strings, failure: Option<String>, callbacks: usize, samples: usize }
impl<'f> Served<'f> {
    fn fail(&mut self, e: impl ToString) { self.failure.get_or_insert_with(|| e.to_string()); }
    fn call(&mut self, name: &str) -> Option<&'f Call> {
        match self.log.call(&format!("animated.{name}")) { Ok(c) => { self.callbacks += 1; Some(c) }, Err(e) => { self.fail(e); None } }
    }
    fn check_body(&mut self, b: &Body, expected: V) {
        let mut arena = Arena::default(); let actual = body_value(b, expected, &mut arena);
        if let Err(e) = check_value("held body", expected, arena.view(&actual)) { self.fail(e); }
    }
    fn unit_source(&self, b: &Body) -> V<'f> { self.sources.at(b.key).get("self") }
    fn callback(&mut self, name: &str, prop: &Animated, b: &mut Body) -> Option<&'f Call> {
        let call = self.call(name)?;
        if let Err(e) = check_value("callback unit", call.extra().get("before").get("self"), self.unit_source(b)) { self.fail(e); }
        self.check_body(b, call.extra().get("before"));
        let mut bodies: Vec<_> = self.sources.items().enumerate().map(|(i, v)| body(v, i)).collect();
        bodies[b.key] = b.clone();
        let mut arena = Arena::default(); let value = prop_value(prop, self.source, &bodies, self.sources, &mut arena);
        for key in ["propBefore", "propAfter"] {
            if let Err(e) = check_value("prop across callback", call.extra().get(key), arena.view(&value)) { self.fail(e); }
        }
        // Unit methods own their own state. Prop-owned values above are
        // prevalidated and remain untouched by this restoration.
        *b = body(call.extra().get("after"), b.key); Some(call)
    }
}
impl World for Served<'_> {
    fn sound(&mut self, _prop: &Animated, name: StrId, position: Vector3) {
        if let Some(c) = self.call("sound") {
            if c.arg(0).str() != Some("sound") { self.fail("sound event type"); }
            let same = match c.arg(1).value { Value::Str(s) => s.encode_utf16().eq(self.strings.units(name).iter().copied()),
                Value::Str16(s) => s.as_slice() == self.strings.units(name), _ => false };
            if !same { self.fail("sound name"); }
            if let Err(e) = sim_replay::check_vec3("sound point", c.arg(2), [position.x, position.y, position.z]) { self.fail(e); }
        }
    }
    fn let_go(&mut self, p: &Animated, b: &mut Body) { self.callback("letGo", p, b); }
    fn damage(&mut self, p: &Animated, b: &mut Body, amount: f64) {
        if let Some(c) = self.callback("damage", p, b) { if let Err(e) = check_num("damage", c.arg(0), amount) { self.fail(e); } }
    }
    fn remove(&mut self, p: &Animated, b: &mut Body) { self.callback("remove", p, b); }
    fn rider_root(&mut self, _prop: &Animated, unit: &Body, clip: StrId, time: f64) -> Option<(Vector3, Quaternion)> {
        let c = self.call("rider")?; self.samples += 1;
        if let Err(e) = check_value("rider unit", c.arg(0), self.unit_source(unit)) { self.fail(e); }
        let text = match c.arg(1).value { Value::Str(s) => s.encode_utf16().eq(self.strings.units(clip).iter().copied()), Value::Str16(s) => s.as_slice() == self.strings.units(clip), _ => false };
        if !text { self.fail("rider clip"); }
        if let Err(e) = check_num("rider time", c.arg(2), time) { self.fail(e); }
        c.arg(3).truthy().then(|| {
            let p = c.arg(4).vec3().unwrap(); let Some(Node::Quat(q)) = c.arg(5).node() else { panic!("rider root turn") };
            (Vector3::new(p[0], p[1], p[2]), Quaternion::new(q[0], q[1], q[2], q[3]))
        })
    }
}
fn replay(capture: &Capture, frame: &Frame) -> Result<usize, String> {
    let before = frame.before(); let mut strings = Strings::default(); let settings = settings(before.get("settings"), capture, &mut strings);
    let mut prop = prop(before.get("prop"), before.get("bodies")); let mut bodies: Vec<_> = before.get("bodies").items().enumerate().map(|(i, v)| body(v, i)).collect();
    let positions: Vec<_> = before.get("positions").items().map(vec).collect(); let order: Vec<_> = before.get("order").items().map(|v| v.num() as usize).collect();
    let mut world = Served { log: Log::new(frame), source: before.get("prop"), sources: before.get("bodies"), strings, failure: None, callbacks: 0, samples: 0 };
    prop.step(frame.arg(0).num(), &positions, &mut bodies, &order, frame.arg(3).num(), &settings, &mut world);
    world.log.finish().map_err(|e| e.0)?;
    let mut arena = Arena::default(); let value = prop_value(&prop, before.get("prop"), &bodies, before.get("bodies"), &mut arena);
    check_value("prop", frame.after().get("prop"), arena.view(&value)).map_err(|e| e.0)?;
    for (i, b) in bodies.iter().enumerate() { world.check_body(b, frame.after().get("bodies").at(i)); }
    for key in ["settings", "order", "positionOwners"] { check_value(key, frame.after().get(key), before.get(key)).map_err(|e| e.0)?; }
    // Door proximity inputs can alias a carried soldier's position. The
    // native carry writes that soldier, so compare the resulting aliases.
    for (i, position) in positions.into_iter().enumerate() {
        let owner = before.get("positionOwners").at(i).opt_num().unwrap_or(-1.0);
        let actual = vector(&mut arena, if owner >= 0.0 { bodies[owner as usize].position } else { position });
        check_value("position alias", frame.after().get("positions").at(i), arena.view(&actual)).map_err(|e| e.0)?;
    }
    if let Some(e) = world.failure { Err(e) } else { Ok(world.samples) }
}
#[test]
fn animated_props_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut samples = 0;
    let report = replay_corpus(&corpus, "AnimatedProp.step", |capture, frame| match replay(capture, frame) {
        Ok(n) => { samples += n; Outcome::Same }, Err(e) => Outcome::Failed(e),
    }).unwrap();
    eprintln!("{}; {samples} checked rider samples", report.summary()); assert!(report.replayed() > 0, "no animated frames"); report.assert_clean();
}

#[test]
fn animated_replay_rejects_pose_and_callback_mutations() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut pose = false; let mut call = false; let mut before = false;
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for frame in capture.frames("AnimatedProp.step") {
            let frame = frame.unwrap(); replay(&capture, &frame).expect("unmodified animated frame");
            if !pose && !frame.after().get("prop").get("standT").is_empty() {
                let mut changed = frame.clone();
                let id = changed.after().get("prop").get("standT").node_id().unwrap();
                if let Node::Array(values) = &mut changed.exit.arena.nodes[id as usize] { values[0] = Value::Num(12345.0); }
                assert!(replay(&capture, &changed).is_err(), "wrong native pose accepted"); pose = true;
            }
            if !call && frame.calls().any(|c| c.name == "animated.rider") {
                let mut changed = frame.clone(); let i = changed.events.iter().position(|e| matches!(e, Event::Call(c) if c.name == "animated.rider")).unwrap();
                changed.events.remove(i); assert!(replay(&capture, &changed).is_err(), "missing rider call accepted"); call = true;
            }
            if !before {
                let mut changed = frame.clone();
                if let Some(Event::Call(c)) = changed.events.iter_mut().find(|e| matches!(e, Event::Call(c) if c.extra().has("propBefore"))) {
                    let id = c.extra().get("propBefore").node_id().unwrap();
                    if let Node::Object { fields, .. } = &mut c.ret.arena.nodes[id as usize] { fields.iter_mut().find(|(key, _)| key == "time").unwrap().1 = Value::Num(12345.0); }
                    assert!(replay(&capture, &changed).is_err(), "wrong callback entry pose clock accepted"); before = true;
                }
            }
            if pose && call && before { return; }
        }
    }
    panic!("missing animated mutation coverage: pose={pose},call={call},before={before}");
}
