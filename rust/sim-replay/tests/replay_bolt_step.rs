// The harness on a real phase: a bolt's step (Ordnance.stepBolt with
// castUnits, src/ordnance.js), transliterated here and replayed against
// every ordnance.stepBolt frame of the corpus BFE_REPLAY_CORPUS names
// (tools/sim-capture.mjs --specs ordnance). Each frame starts from the
// bolt as captured (the store's export of it: its schema's fields in order,
// rust/sim/schema/ordnance.mjs); its calls across the boundary (moving
// bodies, units, a rider behind a hull, water, effects, the impact
// callbacks) are served from the capture with their arguments checked; the
// bolt and the round's striking state it ends with are compared with the
// capture's, bit for bit.
// This proves the capture and the replay, not a port: the ordnance package
// replaces it with its own replays in rust/sim. Without a corpus it passes
// without replaying.
use sim_replay::{check_bool, check_num, check_value, check_vec3, replay_corpus, same_number, Arena, Capture, Corpus, Frame, Log, Mismatch, Outcome, Value, V};

// Math.min and Math.max as JavaScript computes them: NaN wins, -0 < +0.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && b.is_sign_negative()) {
        a
    } else {
        b
    }
}
// `x || fallback` for a number read off a definition.
fn or(v: V, fallback: f64) -> f64 {
    if v.truthy() {
        v.num()
    } else {
        fallback
    }
}
fn add_scaled(a: [f64; 3], d: [f64; 3], s: f64) -> [f64; 3] {
    // three's addScaledVector: this.x += v.x * s, and so on.
    [a[0] + d[0] * s, a[1] + d[1] * s, a[2] + d[2] * s]
}
fn fail(frame: &Frame, what: String) -> Mismatch {
    Mismatch(format!("{}: {what}", frame.place()))
}

// The round's striking state (Ordnance.striking): who a cast is for.
struct Striking {
    owner: Value,
    body: Value,
    mount: Value,
}

struct Bolt<'f> {
    frame: &'f Frame,
    capture: &'f Capture,
    log: Log<'f>,
    arena: Arena,
    striking: Striking,
}

impl<'f> Bolt<'f> {
    fn value<'a>(&'a self, v: &'a Value) -> V<'a> {
        self.arena.view(v)
    }

    // What resolveOrdnanceOwner(owner) gives for an object owner
    // (src/ordnance-owner.js): a snapshot of a launching life (its fields
    // written once in the capture, ownerSnapshot:N) gives its unit while
    // that life lasts (the same generation), anything else itself.
    fn shooter(&mut self, owner: V) -> Value {
        if let Some(n) = owner.handle_num("ownerSnapshot") {
            let Some(snapshot) = self.capture.def(&format!("ownerSnapshot:{n}")) else { return Value::Null };
            let id = snapshot.get("id").num();
            return match self.frame.unit(id) {
                Some(unit) if same_number(unit.get("generation").num(), snapshot.get("generation").num()) => Value::handle("unit", Value::Num(id)),
                _ => Value::Null,
            };
        }
        match owner.value {
            Value::Handle(..) | Value::Node(_) => self.arena.copy(owner),
            _ => Value::Null,
        }
    }

    // ordnance.js castUnits: the first unit the segment meets, if any.
    fn cast_units(&mut self, origin: [f64; 3], direction: [f64; 3], far: f64, owner: V, history_tick: Option<f64>, ignore_body: V) -> Result<V<'f>, Mismatch> {
        let state = self.frame.before();
        if !state.get("units").truthy() || far <= 0.0 {
            return Ok(self.frame.exit.arena.null());
        }
        let shooter = if owner.handle().is_some() || owner.node().is_some() { self.shooter(owner) } else { Value::Null };
        let mut filter = false;
        if state.get("canHit").truthy() || ignore_body.truthy() || self.value(&self.striking.mount).truthy() {
            self.striking.owner = self.arena.copy(owner);
            self.striking.body = self.arena.copy(ignore_body);
            filter = true;
        }
        let (name, first) = match history_tick {
            Some(_) if state.get("hitHistory").truthy() => ("hitHistory.raycast", 1),
            _ => ("units.raycast", 0),
        };
        let call = self.log.call(name).map_err(|e| fail(self.frame, e.0))?;
        if first == 1 {
            check_num("hitHistory.raycast tick", call.arg(0), history_tick.unwrap())?;
        }
        check_vec3(&format!("{name} origin"), call.arg(first), origin)?;
        check_vec3(&format!("{name} direction"), call.arg(first + 1), direction)?;
        check_num(&format!("{name} far"), call.arg(first + 2), far)?;
        check_value(&format!("{name} shooter"), call.arg(first + 3), self.arena.view(&shooter))?;
        let filtered = call.arg(first + 5);
        if filtered.function().is_some() != filter {
            return Err(fail(self.frame, format!("{name} filter: the capture has {}, the port {}", filtered.show(), if filter { "one" } else { "none" })));
        }
        check_bool(&format!("{name} riders"), call.arg(first + 6), true)?;
        Ok(call.result())
    }
}

// The calls this replay serves: a capture whose spec serves others (the
// ordnance package's own, once it changes them) is not this replay's.
const SERVED: [&str; 12] = [
    "collision.castMoving",
    "units.raycast",
    "hitHistory.raycast",
    "ordnance.riderBehind",
    "water.crossing",
    "water.ripple",
    "effects.play",
    "ordnance.onImpact",
    "ordnance.unitImpactEffect",
    "ordnance.onHit",
    "ordnance.onHitUnit",
    "ordnance.explosion",
];
fn served_here(capture: &Capture) -> bool {
    let frames = capture.header.root().get("frames");
    let Some(spec) = frames.items().find(|f| f.get("name").str() == Some("ordnance.stepBolt")) else { return false };
    SERVED.iter().all(|name| spec.get("boundaries").items().any(|b| b.str() == Some(name)))
}

// ordnance.js stepBolt (the world's: drawn is false): Ok(None) when the
// frame replays the same, Ok(Some(why)) when this replay does not take it.
fn step_bolt(capture: &Capture, frame: &Frame, nudge: bool) -> Result<Option<String>, Mismatch> {
    if !served_here(capture) {
        return Ok(Some("the capture's stepBolt boundary is not this replay's".into()));
    }
    let state = frame.before();
    let b = state.get("bolt");
    let dt = frame.arg(1).num();
    if frame.arg(2).truthy() {
        return Ok(Some("drawn (a page's step)".into()));
    }
    // historyTick = null by default.
    let history_tick = frame.arg(3).opt_num();
    let def = capture.def_of(b.get("def")).ok_or_else(|| fail(frame, format!("no definition {}", b.get("def").show())))?;
    let mut s = Bolt { frame, capture, log: Log::new(frame), arena: Arena::default(), striking: Striking { owner: Value::Null, body: Value::Null, mount: Value::Null } };
    let st = state.get("striking");
    s.striking = Striking { owner: s.arena.copy(st.get("owner")), body: s.arena.copy(st.get("body")), mount: s.arena.copy(st.get("mount")) };
    let bolt = s.arena.copy(b);
    let owner = b.get("owner");
    let ignore_body = b.get("ignoreBody");
    let direction = b.get("direction").vec3().unwrap();
    let speed = b.get("speed").num();

    // bolt.previousFlown = bolt.flown; bolt.previousDrawn = bolt.drawn;
    let (flown, drawn) = (b.get("flown").num(), b.get("drawn").num());
    s.arena.assign(&bolt, "previousFlown", Value::Num(flown));
    s.arena.assign(&bolt, "previousDrawn", Value::Num(drawn));
    let mut life = b.get("life").num();
    let step = js_min(dt, life);
    let start = b.get("position").vec3().unwrap();
    let mut position = add_scaled(start, direction, speed * step);

    // Moving bodies along the step (its world was traced at launch).
    let call = s.log.call("collision.castMoving")?;
    check_vec3("castMoving origin", call.arg(0), start)?;
    check_vec3("castMoving direction", call.arg(1), direction)?;
    check_num("castMoving far", call.arg(2), speed * step)?;
    check_value("castMoving ignoreBody", call.arg(4), ignore_body)?;
    let moving = call.result();

    // castUnits(...) ?? riderBehind(...)
    let far = if moving.truthy() { moving.get("distance").num() } else { speed * step };
    let mut unit_hit = s.cast_units(start, direction, far, owner, history_tick, ignore_body)?;
    if unit_hit.is_nullish() {
        let call = s.log.call("ordnance.riderBehind")?;
        check_vec3("riderBehind origin", call.arg(0), start)?;
        check_vec3("riderBehind direction", call.arg(1), direction)?;
        check_value("riderBehind hit", call.arg(2), moving)?;
        check_num("riderBehind far", call.arg(3), speed * life)?;
        check_value("riderBehind owner", call.arg(4), owner)?;
        check_value("riderBehind ignoreBody", call.arg(6), ignore_body)?;
        check_bool("riderBehind moving", call.arg(7), true)?;
        // What it did to the striking state (its own castUnits).
        let after = call.extra().get("striking");
        s.striking = Striking { owner: s.arena.copy(after.get("owner")), body: s.arena.copy(after.get("body")), mount: s.arena.copy(after.get("mount")) };
        unit_hit = call.result();
    }

    let mut impact: Option<Value> = None; // None: as it was
    let impact_was = s.arena.copy(b.get("impact"));
    if unit_hit.truthy() {
        let point = unit_hit.get("point").vec3().unwrap();
        position = point;
        life = 0.0;
        let (p, n) = (s.arena.vec3(point), s.arena.vec3(unit_hit.get("normal").vec3().unwrap()));
        let (unit, part) = (s.arena.copy(unit_hit.get("unit")), s.arena.copy(unit_hit.get("part")));
        impact = Some(s.arena.object("", vec![("point", p), ("normal", n), ("terrain", Value::Bool(false)), ("body", Value::Null), ("unit", unit), ("part", part)]));
        // (The store's Impact: point, normal, terrain, body, unit, part.)
    } else if moving.truthy() {
        let point = moving.get("point").vec3().unwrap();
        position = point;
        life = 0.0;
        let (p, n) = (s.arena.vec3(point), s.arena.vec3(moving.get("normal").vec3().unwrap()));
        let body = s.arena.copy(moving.get("body"));
        impact = Some(s.arena.object("", vec![("point", p), ("normal", n), ("terrain", Value::Bool(false)), ("body", body), ("unit", Value::Undefined), ("part", Value::Undefined)]));
    }
    let current_impact = impact.clone().unwrap_or_else(|| impact_was.clone());

    // Water stops a bolt where it goes in.
    if state.get("water").truthy() && life > 0.0 {
        let call = s.log.call("water.crossing")?;
        check_vec3("water.crossing from", call.arg(0), start)?;
        check_vec3("water.crossing to", call.arg(1), position)?;
        if call.result().truthy() {
            let wet = call.out(2).vec3().unwrap();
            let ripple = s.log.call("water.ripple")?;
            check_vec3("ripple at", ripple.arg(0), wet)?;
            check_num("ripple size", ripple.arg(1), 1.5)?;
            check_num("ripple strength", ripple.arg(2), 0.01)?;
            let name = def.get("ImpactEffectWater");
            let name = if name.is_nullish() { Some("watersplash_ord") } else if name.truthy() { name.str() } else { None };
            if let Some(name) = name {
                let play = s.log.call("effects.play")?;
                check_value("splash effect", play.arg(0), s.arena.view(&Value::str(name)))?;
                check_vec3("splash at", play.arg(1), wet)?;
            }
            life = 0.0;
            let impact_arg = s.log.call("ordnance.onImpact")?;
            let (p, n) = (s.arena.vec3(wet), s.arena.vec3([0.0, 1.0, 0.0]));
            let expected = s.arena.object("", vec![("point", p), ("normal", n), ("surface", Value::Num(4.0))]);
            check_value("onImpact in water", impact_arg.arg(1), s.arena.view(&expected))?;
            return finish(&mut s, &bolt, position, life, None, Some(Value::Null));
        }
    }

    let flown = flown + speed * step;
    // (The harness's own test: one ulp off must show.)
    let flown = if nudge { f64::from_bits(flown.to_bits() + 1) } else { flown };
    let glow = {
        let g = def.get("GlowLength");
        if !g.is_nullish() {
            g.num()
        } else if def.get("LaserLength").truthy() {
            def.get("LaserLength").num() * 3.0
        } else {
            0.0
        }
    };
    let drawn = js_min(drawn + speed * dt, js_max(js_max(or(def.get("LaserLength"), 1.0), glow), or(def.get("BlurLength"), 0.0)));
    life -= dt;
    let set_numbers = |s: &mut Bolt| {
        s.arena.assign(&bolt, "flown", Value::Num(flown));
        s.arena.assign(&bolt, "drawn", Value::Num(drawn));
    };
    set_numbers(&mut s);
    if life <= 0.0 {
        let imp = s.arena.view(&current_impact);
        let call = s.log.call("ordnance.onImpact")?;
        if imp.truthy() {
            let unit = imp.get("unit");
            let surface = if unit.truthy() {
                let health = unit
                    .handle_num("unit")
                    .and_then(|id| frame.unit(id))
                    .and_then(|u| capture.def_of(u.get("classDef")))
                    .map(|d| d.get("HealthType"))
                    .and_then(|h| if h.truthy() { h.str().map(|x| x.to_lowercase()) } else { None })
                    .unwrap_or_default();
                if health == "droid" { 3.0 } else { 2.0 }
            } else if imp.get("terrain").truthy() {
                0.0
            } else {
                1.0
            };
            // { ...bolt.impact, surface }: the JavaScript object's own keys
            // (the store's Impact lists unit and part, undefined where the
            // object has none).
            let mut fields: Vec<(String, Value)> = imp.fields().filter(|(_, v)| !v.is_undefined()).map(|(k, v)| (k.to_string(), v.value.clone())).collect();
            fields.push(("surface".into(), Value::Num(surface)));
            let expected = s.arena.object("", fields.iter().map(|(k, v)| (k.as_str(), v.clone())).collect());
            check_value("onImpact", call.arg(1), s.arena.view(&expected))?;
        } else {
            let (p, n) = (s.arena.vec3(position), s.arena.vec3([0.0, 1.0, 0.0]));
            let expected = s.arena.object("", vec![("point", p), ("normal", n), ("surface", Value::Num(5.0))]);
            check_value("onImpact at the end", call.arg(1), s.arena.view(&expected))?;
        }
        let imp = s.arena.view(&current_impact);
        if imp.truthy() {
            let point = imp.get("point").vec3().unwrap();
            let name: Option<Value> = if imp.get("unit").truthy() {
                let effect = s.log.call("ordnance.unitImpactEffect")?;
                check_value("unitImpactEffect def", effect.arg(0), b.get("def"))?;
                check_value("unitImpactEffect unit", effect.arg(1), imp.get("unit"))?;
                Some(effect.result().value.clone())
            } else if imp.get("terrain").truthy() {
                Some(def.get("ImpactEffectTerrain").value.clone())
            } else if def.get("ImpactEffectStatic").truthy() {
                Some(def.get("ImpactEffectStatic").value.clone())
            } else {
                Some(def.get("ImpactEffectTerrain").value.clone())
            };
            if let Some(name) = name.filter(|n| s.arena.view(n).truthy()) {
                let play = s.log.call("effects.play")?;
                check_value("impact effect", play.arg(0), s.arena.view(&name))?;
                check_vec3("impact effect at", play.arg(1), point)?;
            }
            let imp = s.arena.view(&current_impact);
            if imp.get("body").truthy() {
                let hit = s.log.call("ordnance.onHit")?;
                check_value("onHit body", hit.arg(0), imp.get("body"))?;
                check_value("onHit def", hit.arg(1), b.get("def"))?;
                check_value("onHit owner", hit.arg(2), owner)?;
                check_vec3("onHit direction", hit.arg(3), direction)?;
            }
            let imp = s.arena.view(&current_impact);
            if imp.get("unit").truthy() {
                let hit = s.log.call("ordnance.onHitUnit")?;
                check_value("onHitUnit unit", hit.arg(0), imp.get("unit"))?;
                check_value("onHitUnit def", hit.arg(1), b.get("def"))?;
                check_vec3("onHitUnit point", hit.arg(2), point)?;
                check_value("onHitUnit owner", hit.arg(3), owner)?;
                check_value("onHitUnit part", hit.arg(4), imp.get("part"))?;
                check_vec3("onHitUnit direction", hit.arg(5), direction)?;
            }
            if def.get("ExplosionImpact").truthy() {
                let normal = s.arena.view(&current_impact).get("normal").vec3().unwrap();
                let blast = s.log.call("ordnance.explosion")?;
                check_value("explosion def", blast.arg(0), def.get("ExplosionImpact"))?;
                check_vec3("explosion at", blast.arg(1), add_scaled(point, normal, 0.2))?;
                check_value("explosion owner", blast.arg(2), owner)?;
            }
            return finish(&mut s, &bolt, position, life, impact, Some(Value::Null));
        } else if def.get("ExplosionExpire").truthy() {
            let blast = s.log.call("ordnance.explosion")?;
            check_value("expiry explosion def", blast.arg(0), def.get("ExplosionExpire"))?;
            check_vec3("expiry explosion at", blast.arg(1), position)?;
            check_value("expiry explosion owner", blast.arg(2), owner)?;
        }
        return finish(&mut s, &bolt, position, life, impact, None);
    }
    finish(&mut s, &bolt, position, life, impact, None)
}

// The bolt and striking state the step ends with, against the capture's;
// `cleared` is what bolt.impact ends as when the step sets it (null).
fn finish(s: &mut Bolt, bolt: &Value, position: [f64; 3], life: f64, impact: Option<Value>, cleared: Option<Value>) -> Result<Option<String>, Mismatch> {
    let p = s.arena.vec3(position);
    s.arena.assign(bolt, "position", p);
    s.arena.assign(bolt, "life", Value::Num(life));
    if let Some(i) = cleared.or(impact) {
        s.arena.assign(bolt, "impact", i);
    }
    s.log.finish()?;
    let after = s.frame.after();
    check_value("the bolt", after.get("bolt"), s.arena.view(bolt))?;
    let st = after.get("striking");
    check_value("striking.owner", st.get("owner"), s.arena.view(&s.striking.owner))?;
    check_value("striking.body", st.get("body"), s.arena.view(&s.striking.body))?;
    check_value("striking.mount", st.get("mount"), s.arena.view(&s.striking.mount))?;
    if !s.frame.result().is_undefined() {
        return Err(fail(s.frame, format!("stepBolt returned {}", s.frame.result().show())));
    }
    Ok(None)
}

#[test]
fn bolts_step_as_captured() {
    let Some(corpus) = Corpus::from_env().expect("the corpus BFE_REPLAY_CORPUS names") else {
        eprintln!("no BFE_REPLAY_CORPUS: nothing replayed");
        return;
    };
    let report = replay_corpus(&corpus, "ordnance.stepBolt", |capture, frame| match step_bolt(capture, frame, false) {
        Ok(None) => Outcome::Same,
        Ok(Some(why)) => Outcome::Skipped(why),
        Err(e) => Outcome::Failed(e.0),
    })
    .expect("the corpus reads");
    eprintln!("{}", report.summary());
    report.assert_clean();
    assert!(report.same > 0 || report.captures == 0, "no frame replayed: {}", report.summary());
}

#[test]
fn one_ulp_off_shows() {
    let Some(corpus) = Corpus::from_env().expect("the corpus BFE_REPLAY_CORPUS names") else { return };
    let report = replay_corpus(&corpus, "ordnance.stepBolt", |capture, frame| match step_bolt(capture, frame, true) {
        Ok(None) => Outcome::Same,
        Ok(Some(why)) => Outcome::Skipped(why),
        Err(e) => Outcome::Failed(e.0),
    })
    .expect("the corpus reads");
    // Every frame whose bolt flies on (out of water) ends one ulp off in flown.
    eprintln!("{}", report.summary());
    assert!(!report.failures.is_empty(), "{}", report.summary());
    assert!(report.failures.iter().all(|f| f.contains(".flown")), "{}", report.summary());
}
