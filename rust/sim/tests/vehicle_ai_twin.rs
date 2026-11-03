// The vehicle AI's twin checks: pure functions of the Rust port against the
// JavaScript's answers on generated cases (tools/vehicle-ai-twin.mjs writes
// them, every number as its bits): the tow-cable orbit's steering
// (orbit.js orbitSteer), the mounted policies' goal tables and the state a
// mounted controller's request becomes (controllers.js, index.js
// mountedState), and Number.prototype.toFixed(1) as the support cache's
// key uses it. BFE_VAI_TWIN names the generator's output directory; without
// it the test passes without checking.

use bfe_content::{json, Value};
use bfe_sim::ai::vehicles::controllers::{mounted_state, Policy};
use bfe_sim::ai::vehicles::orbit::{orbit_steer, OrbitResult};
use bfe_sim::ai::vehicles::service::to_fixed_1;
use bfe_sim::ai::vehicles::world::Orbit;
use three::{Matrix4, Vector3};

fn bits(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().expect("bits"), 16).expect("hex"))
}
fn vec(v: &Value) -> Vector3 {
    Vector3::new(bits(v.at(0).unwrap()), bits(v.at(1).unwrap()), bits(v.at(2).unwrap()))
}
fn boolean(v: Option<&Value>) -> Option<bool> {
    match v {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}
fn same_vec(a: &Vector3, b: &Vector3) -> bool {
    same(a.x, b.x) && same(a.y, b.y) && same(a.z, b.z)
}

#[test]
fn vehicle_ai_twin() {
    let Some(dir) = std::env::var_os("BFE_VAI_TWIN") else { return };
    let path = std::path::Path::new(&dir).join("twin.json");
    let twin = json::parse_bytes(&std::fs::read(&path).expect("twin.json")).expect("parse twin.json");
    let mut failures = Vec::new();

    let orbits = twin.get("orbit").and_then(|v| v.as_array()).expect("orbit");
    for (i, c) in orbits.iter().enumerate() {
        let mut m = Matrix4::default();
        for (k, e) in c.get("matrix").and_then(|v| v.as_array()).unwrap().iter().enumerate() {
            m.elements[k] = bits(e);
        }
        let orbit = Orbit { target: None, offset: vec(c.get("offset").unwrap()), radius: bits(c.get("radius").unwrap()), factor: bits(c.get("factor").unwrap()) };
        let mut out = OrbitResult::default();
        orbit_steer(&vec(c.get("position").unwrap()), &vec(c.get("forward").unwrap()), &vec(c.get("hullPosition").unwrap()), &m, &orbit, &mut out);
        let ok = same_vec(&out.point, &vec(c.get("point").unwrap()))
            && same_vec(&out.centre, &vec(c.get("centre").unwrap()))
            && same(out.blend, bits(c.get("blend").unwrap()))
            && same(out.throttle, bits(c.get("throttle").unwrap()))
            && boolean(c.get("towPass")) == Some(out.tow_pass);
        if !ok && failures.len() < 10 {
            failures.push(format!("orbit {i}: port {out:?}"));
        }
    }

    let tables = twin.get("tables").and_then(|v| v.as_array()).expect("tables");
    for row in tables {
        let policy = match row.get("policy").and_then(|v| v.as_str()) {
            Some("Mobile") => Policy::Mobile,
            Some("Stationary") => Policy::Stationary,
            _ => Policy::Fly,
        };
        let kind = row.get("kind").and_then(|v| v.as_f64()).unwrap();
        let goal = policy.goal_state(kind);
        let mounted = mounted_state(Some(policy), kind);
        let agent = policy.agent_for(kind).is_some();
        if goal != row.get("goalState").and_then(|v| v.as_f64()).unwrap()
            || mounted != row.get("mounted").and_then(|v| v.as_f64()).unwrap()
            || boolean(row.get("agent")) != Some(agent)
        {
            failures.push(format!("policy {policy:?} kind {kind}: port {goal} {mounted} {agent}"));
        }
    }

    let fixed = twin.get("fixed").and_then(|v| v.as_array()).expect("fixed");
    for c in fixed {
        let x = bits(c.get("x").unwrap());
        let mine = to_fixed_1(x);
        let theirs = c.get("text").and_then(|v| v.as_str()).unwrap();
        if mine != theirs && failures.len() < 20 {
            failures.push(format!("toFixed(1) of {x:e}: JavaScript {theirs}, port {mine}"));
        }
    }

    println!("vehicle AI twin: {} orbits, {} policy rows, {} toFixed cases", orbits.len(), tables.len(), fixed.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
