//! Loading `ai-nav.json` (+ `ai-nav.bin`), schema 1 (nav/data.js), over
//! the content crate's JSON values. Bad input is an Err with the
//! JavaScript's message. The routing words are the content crate's
//! (`bfe_content::content::route_words`, its port of data.js:13).

use super::barriers::{barrier_from_record, BarrierRecord, BarrierSet};
use super::boundary::Boundary;
use super::graph::{ArcRecord, GraphRecord, HubRecord, NavGraph};
use super::hints::{build_hints, HintPosts, HintRecord, HintSet, POOL_CAPACITY};
use super::math::Vec3;
use bfe_content::Value;

/// data.js:10.
pub const SCHEMA: f64 = 1.0;

/// A number as an error message's `${}` writes it (ECMAScript
/// Number::toString, through the content crate's port of it).
pub fn num_text(x: f64) -> String {
    bfe_content::js::number_to_string(x)
}

/// An authored path (data.js:38): its info words and points as loaded
/// number arrays.
#[derive(Clone, Debug)]
pub struct NavPathData {
    pub index: usize,
    pub name: Option<String>,
    pub info: Vec<f64>,
    pub points: Vec<Vec<f64>>,
}

/// What loadNav returns (data.js:47), without the planner (the service
/// builds that over the barriers). `map`, `source`, `regions` and
/// `warnings` pass through as loaded values.
pub struct NavData {
    pub schema: f64,
    pub map: Value,
    pub source: Value,
    pub graph: Option<NavGraph>,
    pub barriers: BarrierSet,
    pub hints: HintSet,
    pub boundary: Boundary,
    pub paths: Vec<NavPathData>,
    pub regions: Value,
    pub warnings: Vec<Value>,
    // Load-time issue records from the JavaScript's default `report`
    // (data.js:31) are the graph's own reports, on the graph.
}

fn num(v: Option<&Value>) -> f64 {
    bfe_content::js::to_number(v)
}

fn nums(v: Option<&Value>, n: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(num(v.and_then(|v| v.at(i))));
    }
    out
}

fn str_of(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// data.js:28. `posts` and `hint_capacity` are its options of those
/// names (capacity defaults to the 350-slot pool).
pub fn load_nav(json: &Value, bin: Option<&[u8]>, posts: Option<&dyn HintPosts>, hint_capacity: Option<f64>) -> Result<NavData, String> {
    let schema = json.get("schema").and_then(|v| v.as_f64());
    if schema != Some(SCHEMA) {
        let shown = match json.get("schema") {
            Some(v) if v.truthy() || matches!(v, Value::Number(_)) => match v.as_f64() {
                Some(n) => num_text(n),
                None => "undefined".to_string(),
            },
            _ => "undefined".to_string(),
        };
        return Err(format!("ai-nav: schema {shown}, expected {}", num_text(SCHEMA)));
    }
    let graph = match json.get("graph").filter(|g| g.truthy()) {
        Some(g) => {
            let words = match bfe_content::content::route_words(bin, g.get("routes")) {
                Some(Ok(words)) => words,
                Some(Err(e)) => return Err(e),
                None => Vec::new(),
            };
            let hubs = g
                .get("hubs")
                .and_then(|h| h.as_array())
                .map(|hubs| {
                    hubs.iter()
                        .map(|h| {
                            let position = nums(h.get("position"), 3);
                            HubRecord {
                                name: str_of(h.get("name")),
                                position: [position[0], position[1], position[2]],
                                radius: num(h.get("radius")),
                                arcs: h.get("arcs").and_then(|a| a.as_array()).map(|a| a.iter().map(|v| num(Some(v))).collect()).unwrap_or_default(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let arcs = g
                .get("arcs")
                .and_then(|a| a.as_array())
                .map(|arcs| {
                    arcs.iter()
                        .map(|a| {
                            let hubs = nums(a.get("hubs"), 2);
                            ArcRecord { name: str_of(a.get("name")), hubs: [hubs[0], hubs[1]], mask: num(a.get("mask")) }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let info = nums(g.get("info"), 2);
            let record = GraphRecord {
                info: [info[0], info[1]],
                hubs,
                arcs,
                tables: g.get("routes").and_then(|r| r.get("tables")).and_then(|t| t.as_f64()),
            };
            Some(NavGraph::new(&record, words)?)
        }
        None => None,
    };
    let mut barriers = Vec::new();
    if let Some(list) = json.get("barriers").and_then(|b| b.as_array()) {
        for (index, b) in list.iter().enumerate() {
            let xfrm = nums(b.get("xfrm"), 12);
            let size = nums(b.get("size"), 3);
            let record = BarrierRecord {
                name: str_of(b.get("name")),
                xfrm: xfrm.try_into().expect("12 numbers"),
                size: [size[0], size[1], size[2]],
                flags: num(b.get("flags")),
            };
            barriers.push(barrier_from_record(&record, index)?);
        }
    }
    let hint_records: Vec<HintRecord> = json
        .get("hints")
        .and_then(|h| h.as_array())
        .map(|hints| {
            hints
                .iter()
                .map(|h| {
                    let xfrm = nums(h.get("xfrm"), 12);
                    let properties = h
                        .get("properties")
                        .and_then(|p| p.as_array())
                        .map(|props| {
                            props
                                .iter()
                                .map(|p| {
                                    let hash = p.at(0).and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let value = p.at(2).and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    (hash, value)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    HintRecord { kind: str_of(h.get("kind")), name: str_of(h.get("name")), xfrm: xfrm.try_into().expect("12 numbers"), properties }
                })
                .collect()
        })
        .unwrap_or_default();
    let hints = HintSet::new(build_hints(&hint_records, posts, hint_capacity.unwrap_or(POOL_CAPACITY)));
    let mut paths = Vec::new();
    if let Some(list) = json.get("paths").and_then(|p| p.as_array()) {
        for (index, p) in list.iter().enumerate() {
            let points: Vec<Vec<f64>> = p
                .get("points")
                .and_then(|pts| pts.as_array())
                .map(|pts| pts.iter().map(|pt| pt.as_array().map(|a| a.iter().map(|v| num(Some(v))).collect()).unwrap_or_default()).collect())
                .unwrap_or_default();
            let info0 = p.get("info").and_then(|i| i.at(0)).map(|v| num(Some(v))).unwrap_or(f64::NAN);
            if info0 != points.len() as f64 {
                return Err(format!("ai-nav: path {index} count {} for {} points", num_text(info0), points.len()));
            }
            for pt in &points {
                for v in pt {
                    if !v.is_finite() {
                        return Err(format!("ai-nav: path {index} is not finite"));
                    }
                }
            }
            paths.push(NavPathData {
                index,
                name: str_of(p.get("name")),
                info: p.get("info").and_then(|i| i.as_array()).map(|a| a.iter().map(|v| num(Some(v))).collect()).unwrap_or_default(),
                points,
            });
        }
    }
    let mut polygons: Vec<Vec<Vec3>> = Vec::new();
    if let Some(refs) = json.get("boundary").and_then(|b| b.get("references")).and_then(|r| r.as_array()) {
        for r in refs {
            let Some(path_ref) = r.get("path").filter(|p| !p.is_null()) else {
                continue;
            };
            let at = num(Some(path_ref));
            let Some(path) = paths.get(at as usize).filter(|_| at >= 0.0 && at.fract() == 0.0) else {
                return Err(format!("ai-nav: boundary names path {}", num_text(at)));
            };
            polygons.push(
                path.points
                    .iter()
                    .map(|pt| Vec3::new(pt.first().copied().unwrap_or(f64::NAN), pt.get(1).copied().unwrap_or(f64::NAN), pt.get(2).copied().unwrap_or(f64::NAN)))
                    .collect(),
            );
        }
    }
    Ok(NavData {
        schema: SCHEMA,
        map: json.get("map").cloned().unwrap_or(Value::Null),
        source: json.get("source").cloned().unwrap_or(Value::Null),
        graph,
        barriers: BarrierSet::new(barriers),
        hints,
        boundary: Boundary::new(&polygons),
        paths,
        regions: json.get("regions").cloned().unwrap_or(Value::Null),
        warnings: json.get("warnings").and_then(|w| w.as_array()).map(|w| w.to_vec()).unwrap_or_default(),
    })
}
