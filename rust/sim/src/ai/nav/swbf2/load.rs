//! Loader for 2005 navigation payloads (`schema: 2`), constructed during cold
//! battle admission from `map.json` (`plan`, `planningGroups`, optional
//! `barriers` and `hints`, `boundaryPaths`, `paths`, `regions`) plus optional
//! `planning.bin` bytes.

use super::super::barriers::{barrier_from_record, BarrierRecord, BarrierSet};
use super::super::boundary::Boundary;
use super::super::data::NavPathData;
use super::super::hints::{build_hints, HintRecord, HintSet, POOL_CAPACITY};
use super::super::math::Vec3;
use super::graph::{
    DeltaBlockMeta, PlanningGroupsInput, Swbf2ArcInput, Swbf2Graph, Swbf2HubInput, ARC_JET_JUMP,
    ARC_JUMP, ARC_ONE_WAY, TABLE_COUNT,
};
use bfe_content::js::to_number;
use bfe_content::zero_hash;
use bfe_content::Value;

pub const SCHEMA_SWBF2: f64 = 2.0;

/// 2005-specific hint type bits beyond the 2004 set (`0..6`).
pub const HINT_BIT_TYPE7: u32 = 1 << 6;
pub const HINT_BIT_TYPE8: u32 = 1 << 7;

/// Dynamic barrier enable/disable state for 2005 maps (`EnableBarriers` /
/// `DisableBarriers` / `ToggleBarriers`). Stored beside `BarrierSet` so the
/// 2004 barrier implementation stays completely unchanged.
#[derive(Clone, Debug, Default)]
pub struct BarrierToggles {
    /// Original traversal flags (bits 0..4) for each barrier index.
    original_flags: Vec<u32>,
    /// Case-insensitive FNV-1a name hash for each barrier index.
    name_hashes: Vec<u32>,
    /// Current enabled state (2005 barriers start enabled).
    enabled: Vec<bool>,
}

impl BarrierToggles {
    pub fn from_barriers(barriers: &BarrierSet) -> Self {
        let n = barriers.list.len();
        let mut original_flags = Vec::with_capacity(n);
        let mut name_hashes = Vec::with_capacity(n);
        let mut enabled = Vec::with_capacity(n);
        for b in &barriers.list {
            original_flags.push(b.flags & 0x1f);
            name_hashes.push(b.name.as_deref().map_or(0, zero_hash::fnv1a));
            enabled.push(true);
        }
        Self { original_flags, name_hashes, enabled }
    }

    pub fn is_enabled(&self, index: usize) -> bool {
        self.enabled.get(index).copied().unwrap_or(false)
    }

    pub fn set_enabled(&mut self, barriers: &mut BarrierSet, name: &str, on: bool) -> bool {
        self.set_enabled_hash(barriers, zero_hash::fnv1a(name), on)
    }

    pub fn set_enabled_hash(&mut self, barriers: &mut BarrierSet, hash: u32, on: bool) -> bool {
        if hash == 0 {
            return false;
        }
        let mut matched = false;
        for (i, &h) in self.name_hashes.iter().enumerate() {
            if h == hash {
                matched = true;
                self.enabled[i] = on;
                if let Some(b) = barriers.list.get_mut(i) {
                    b.flags = if on { self.original_flags[i] } else { 0 };
                }
            }
        }
        matched
    }

    pub fn toggle(&mut self, barriers: &mut BarrierSet, name: &str) -> bool {
        self.toggle_hash(barriers, zero_hash::fnv1a(name))
    }

    pub fn toggle_hash(&mut self, barriers: &mut BarrierSet, hash: u32) -> bool {
        if hash == 0 {
            return false;
        }
        let mut matched = false;
        for (i, &h) in self.name_hashes.iter().enumerate() {
            if h == hash {
                matched = true;
                let next = !self.enabled[i];
                self.enabled[i] = next;
                if let Some(b) = barriers.list.get_mut(i) {
                    b.flags = if next { self.original_flags[i] } else { 0 };
                }
            }
        }
        matched
    }
}

/// Decoded 2005 navigation assets ready to construct `Nav`.
pub struct Swbf2NavData {
    pub map: Value,
    pub graph: Option<Swbf2Graph>,
    pub barriers: BarrierSet,
    pub barrier_toggles: BarrierToggles,
    pub hints: HintSet,
    pub boundary: Boundary,
    pub boundary_polygon_count: usize,
    pub paths: Vec<NavPathData>,
    pub regions: Value,
}

fn num(v: Option<&Value>) -> f64 {
    to_number(v)
}

fn opt_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_string)
}

fn parse_u32_or_hex(v: &Value) -> Option<u32> {
    if let Some(s) = v.as_str() {
        let trimmed = s.trim().strip_prefix("0x").or_else(|| s.trim().strip_prefix("0X")).unwrap_or(s.trim());
        u32::from_str_radix(trimmed, 16).ok()
    } else if let Some(n) = v.as_f64() {
        if n.is_finite() && n >= 0.0 && n <= f64::from(u32::MAX) && n.fract() == 0.0 {
            Some(n as u32)
        } else {
            None
        }
    } else {
        None
    }
}

fn parse_graph(json: &Value, bin: Option<&[u8]>) -> Option<Swbf2Graph> {
    let plan = json.get("plan").filter(|v| v.as_object().is_some())?;
    let branch_count = plan
        .get("branchCount")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
        .map_or(TABLE_COUNT, |v| (v as usize).min(TABLE_COUNT));

    let hub_inputs: Vec<Swbf2HubInput> = plan
        .get("hubs")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|h| {
                    let pos = h.get("position").and_then(Value::as_array);
                    let position = [
                        num(pos.and_then(|p| p.first())),
                        num(pos.and_then(|p| p.get(1))),
                        num(pos.and_then(|p| p.get(2))),
                    ];
                    let slots = h
                        .get("slots")
                        .or_else(|| h.get("arcs"))
                        .and_then(Value::as_array)
                        .map(|s| s.iter().map(|v| num(Some(v))).collect())
                        .unwrap_or_default();
                    let mut weight_counts = [0u8; TABLE_COUNT];
                    if let Some(wc) = h.get("weightCounts").and_then(Value::as_array) {
                        for (i, dst) in weight_counts.iter_mut().enumerate() {
                            let val = num(wc.get(i));
                            *dst = if val.is_finite() && (0.0..=255.0).contains(&val) {
                                val as u8
                            } else {
                                0
                            };
                        }
                    }
                    let weights = h
                        .get("weights")
                        .and_then(Value::as_array)
                        .map(|w| {
                            w.iter()
                                .map(|v| {
                                    let n = num(Some(v));
                                    if n.is_finite() && (0.0..=255.0).contains(&n) {
                                        n as u8
                                    } else {
                                        0
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Swbf2HubInput {
                        name: opt_str(h.get("name")),
                        position,
                        radius: num(h.get("radius")),
                        slots,
                        weight_counts,
                        weights,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let arc_inputs: Vec<Swbf2ArcInput> = plan
        .get("arcs")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|a| {
                    let start = a
                        .get("start")
                        .or_else(|| a.get("hubs").and_then(|h| h.at(0)));
                    let end = a
                        .get("end")
                        .or_else(|| a.get("hubs").and_then(|h| h.at(1)));
                    let filter_flags = a
                        .get("filterFlags")
                        .or_else(|| a.get("mask"))
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .map_or(0, v8math::to_uint32);
                    let mut type_flags = a
                        .get("typeFlags")
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .map_or(0, |v| (v8math::to_uint32(v) & 0xff) as u8);
                    if matches!(a.get("oneWay"), Some(Value::Bool(true))) {
                        type_flags |= ARC_ONE_WAY;
                    }
                    if matches!(a.get("jump"), Some(Value::Bool(true))) {
                        type_flags |= ARC_JUMP;
                    }
                    if matches!(a.get("jetJump"), Some(Value::Bool(true))) {
                        type_flags |= ARC_JET_JUMP;
                    }
                    Swbf2ArcInput {
                        name: opt_str(a.get("name")),
                        start: num(start),
                        end: num(end),
                        filter_flags,
                        type_flags,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let groups_input = json
        .get("planningGroups")
        .filter(|v| v.as_object().is_some())
        .map(|pg| {
            let group_counts = pg
                .get("groupCounts")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|v| {
                            let n = num(Some(v));
                            if n.is_finite() && n >= 0.0 {
                                n as usize
                            } else {
                                0
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let connection_hashes = pg
                .get("connectionHashes")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(parse_u32_or_hex).collect())
                .unwrap_or_default();
            let blocks = pg
                .get("blocks")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|b| {
                            let usize_of = |key: &str| {
                                let n = num(b.get(key));
                                if n.is_finite() && n >= 0.0 {
                                    n as usize
                                } else {
                                    0
                                }
                            };
                            DeltaBlockMeta {
                                offset: usize_of("offset"),
                                size: usize_of("size"),
                                unpacked_size: usize_of("unpackedSize"),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            PlanningGroupsInput {
                group_counts,
                connection_hashes,
                blocks,
                raw_bin: bin.map(<[u8]>::to_vec).unwrap_or_default(),
            }
        });

    Some(Swbf2Graph::new(branch_count, &hub_inputs, &arc_inputs, groups_input))
}

fn parse_barriers(json: &Value) -> Result<(BarrierSet, BarrierToggles), String> {
    let mut grid_entries = 0usize;
    let mut enabled = Vec::new();
    let mut barriers = Vec::new();
    if let Some(list) = json.get("barriers").and_then(Value::as_array) {
        for b in list {
            let mut xfrm = [0.0; 12];
            if let Some(arr) = b.get("xfrm").and_then(Value::as_array) {
                for (i, dst) in xfrm.iter_mut().enumerate() {
                    *dst = num(arr.get(i));
                }
            }
            let mut size = [0.0; 3];
            if let Some(arr) = b.get("size").and_then(Value::as_array) {
                for (i, dst) in size.iter_mut().enumerate() {
                    *dst = num(arr.get(i));
                }
            }
            let record = BarrierRecord {
                name: opt_str(b.get("name")),
                xfrm,
                size,
                flags: f64::from(v8math::to_uint32(num(b.get("flags"))) & 31),
            };
            // Skip malformed non-finite barriers without crashing the map load.
            if let Ok(barrier) = barrier_from_record(&record, barriers.len()) {
                // Bound the number of cells materialized by the common
                // spatial grid, including overlapping and very large records.
                let axis = |centre: f64| {
                    let lo = v8math::floor((centre - barrier.bound) / 16.);
                    let hi = v8math::floor((centre + barrier.bound) / 16.);
                    ((lo.abs() < (1u64 << 48) as f64) && (hi.abs() < (1u64 << 48) as f64))
                        .then_some((hi - lo + 1.) as usize)
                };
                let cells = axis(barrier.cx).zip(axis(barrier.cz))
                    .and_then(|(w,h)|w.checked_mul(h)).ok_or("barrier bounds exceed grid range")?;
                grid_entries = grid_entries.checked_add(cells).filter(|n|*n <= 1_048_576)
                    .ok_or("barriers exceed grid capacity")?;
                enabled.push(!matches!(b.get("enabled"), Some(Value::Bool(false))));
                barriers.push(barrier);
            }
        }
    }
    let mut geometry = BarrierSet::new(barriers);
    let mut toggles = BarrierToggles::from_barriers(&geometry);
    toggles.enabled = enabled;
    for (barrier, enabled) in geometry.list.iter_mut().zip(&toggles.enabled) {
        if !enabled { barrier.flags = 0; }
    }
    Ok((geometry, toggles))
}

fn parse_hints(json: &Value, hint_capacity: Option<f64>) -> HintSet {
    let Some(list) = json.get("hints").and_then(Value::as_array) else {
        return HintSet::default();
    };
    let capacity = hint_capacity.unwrap_or(POOL_CAPACITY);
    let mut standard_records = Vec::new();
    let mut extra_types = Vec::with_capacity(list.len());

    for h in list {
        let mut xfrm = [0.0; 12];
        if let Some(arr) = h.get("xfrm").and_then(Value::as_array) {
            for (i, dst) in xfrm.iter_mut().enumerate() {
                *dst = num(arr.get(i));
            }
        }
        let kind = match h.get("type").and_then(Value::as_str) {
            Some("0") => Some("snipe"), Some("1") => Some("patrol"),
            Some("2") => Some("cover"), Some("4") => Some("jetjump"),
            Some("5") => Some("minefield"), Some("6") => Some("land"),
            Some("7") => Some("type7"), Some("8") => Some("type8"),
            Some(_) => None, None => h.get("kind").and_then(Value::as_str),
        };
        let extra = match kind {
            Some("type7") => Some(("type7", HINT_BIT_TYPE7)),
            Some("type8") => Some(("type8", HINT_BIT_TYPE8)),
            _ => None,
        };
        extra_types.push(extra);
        // These types share the ordinary property and radius-ring rules.
        let kind_str = if !xfrm.iter().all(|v| v.is_finite()) { None }
            else if extra.is_some() { Some("patrol") } else { kind };
        let properties = h
            .get("properties")
            .and_then(Value::as_array)
            .map(|props| {
                props
                    .iter()
                    .map(|p| {
                        let hash = p.at(0).and_then(Value::as_str).unwrap_or("").to_string();
                        let value = p
                            .at(2)
                            .or_else(|| p.at(1))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        (hash, value)
                    })
                    .collect()
            })
            .unwrap_or_default();
        standard_records.push(HintRecord {
            kind: kind_str.map(str::to_owned),
            name: opt_str(h.get("name")),
            xfrm,
            properties,
        });
    }

    let mut hints = build_hints(&standard_records, None, capacity);
    for hint in &mut hints {
        if let Some((name, bit)) = extra_types[hint.source] {
            hint.kind = name.to_owned();
            hint.bit = bit;
        }
    }
    HintSet::new(hints)
}

fn extract_path_points(p: &Value) -> Vec<Vec<f64>> {
    if let Some(nodes) = p.get("nodes").and_then(Value::as_array) {
        return nodes
            .iter()
            .filter_map(|node| {
                let pos = node.get("position").and_then(Value::as_array)?;
                let x = num(pos.first());
                let y = num(pos.get(1));
                let z = num(pos.get(2));
                if !(x.is_finite() && y.is_finite() && z.is_finite()) {
                    return None;
                }
                let mut pt = vec![x, y, z];
                if let Some(rot) = node.get("rotation").and_then(Value::as_array) {
                    for i in 0..4 {
                        let v = num(rot.get(i));
                        pt.push(if v.is_finite() { v } else { 0.0 });
                    }
                }
                Some(pt)
            })
            .collect();
    }
    if let Some(points) = p.get("points").and_then(Value::as_array) {
        return points
            .iter()
            .filter_map(|pt| {
                let arr = pt.as_array()?;
                let nums: Vec<f64> = arr.iter().map(|v| num(Some(v))).collect();
                if nums.len() >= 3 && nums.iter().all(|v| v.is_finite()) {
                    Some(nums)
                } else {
                    None
                }
            })
            .collect();
    }
    Vec::new()
}

fn parse_paths_and_boundary(json: &Value) -> (Vec<NavPathData>, Boundary, usize) {
    let mut paths = Vec::new();
    if let Some(list) = json.get("paths").and_then(Value::as_array) {
        for (index, p) in list.iter().enumerate() {
            let points = extract_path_points(p);
            paths.push(NavPathData {
                index,
                name: opt_str(p.get("name")),
                info: vec![points.len() as f64],
                points,
            });
        }
    }

    let mut polygons: Vec<Vec<Vec3>> = Vec::new();
    if let Some(bnames) = json.get("boundaryPaths").and_then(Value::as_array) {
        for b in bnames {
            let Some(target_name) = b.as_str() else {
                continue;
            };
            if let Some(path) = paths.iter().find(|p| {
                p.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(target_name))
            }) {
                if path.points.len() >= 3 {
                    polygons.push(
                        path.points
                            .iter()
                            .map(|pt| Vec3::new(pt[0], pt[1], pt[2]))
                            .collect(),
                    );
                }
            }
        }
    }
    let poly_count = polygons.len();
    (paths, Boundary::new(&polygons), poly_count)
}

/// Build `BfConfig` boundary edges for `F4TailSetup` from a 2005 map's
/// `boundaryPaths` and composed `paths`.
pub fn boundary_edges_from_swbf2(
    boundary_paths: Option<&Value>,
    paths: &[Value],
) -> Vec<Vec<[f64; 4]>> {
    let mut polygons = Vec::new();
    let Some(bnames) = boundary_paths.and_then(Value::as_array) else {
        return polygons;
    };
    for b in bnames {
        let Some(target_name) = b.as_str() else {
            continue;
        };
        let Some(path) = paths.iter().find(|p| {
            p.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(target_name))
        }) else {
            continue;
        };
        let pts = extract_path_points(path);
        if pts.len() < 3 {
            continue;
        }
        let mut edges = Vec::with_capacity(pts.len());
        for i in 0..pts.len() {
            let a = &pts[(i + pts.len() - 1) % pts.len()];
            let b = &pts[i];
            edges.push([a[0], a[2], -(b[2] - a[2]), b[0] - a[0]]);
        }
        polygons.push(edges);
    }
    polygons
}

/// Load a 2005 navigation payload (`schema: 2`) plus optional `planning.bin`.
pub fn load_swbf2_nav(
    json: &Value,
    bin: Option<&[u8]>,
    hint_capacity: Option<f64>,
) -> Result<Swbf2NavData, String> {
    if json.get("schema").and_then(Value::as_f64) != Some(SCHEMA_SWBF2) {
        return Err("ai-nav: expected schema 2 for 2005 navigation".into());
    }
    let list = |value: Option<&Value>| value.and_then(Value::as_array).map_or(0, <[Value]>::len);
    if let Some(plan) = json.get("plan") {
        let n = list(plan.get("hubs"));
        if n > 256 || list(plan.get("arcs")) > u16::MAX as usize {
            return Err("planning graph exceeds binary index range".into());
        }
        for hub in plan.get("hubs").and_then(Value::as_array).unwrap_or(&[]) {
            if list(hub.get("weights")) > 4 * TABLE_COUNT * n || list(hub.get("slots")) > 8 {
                return Err("planning hub exceeds routing table width".into());
            }
        }
    }
    if let Some(groups) = json.get("planningGroups").filter(|v| !v.is_null()) {
        let count = list(groups.get("groupCounts"));
        let hashes = list(groups.get("connectionHashes"));
        let mut represented = 0usize;
        for number in groups.get("groupCounts").and_then(Value::as_array).unwrap_or(&[]) {
            let size = number.as_f64().filter(|v| v.fract() == 0. && (0.0..=255.0).contains(v))
                .ok_or("invalid planning group count")?;
            represented = represented.checked_add(size as usize).ok_or("invalid planning group count")?;
        }
        if represented != hashes || groups.get("connectionHashes").and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|v| parse_u32_or_hex(v).is_none())) {
            return Err("planning group counts do not match the connection hashes".into());
        }
        if count > 16 || list(groups.get("blocks")) != 1 << count
            || list(groups.get("connectionHashes")) > 255
            || bin.is_some_and(|b| b.len() > super::graph::MAX_DELTA_BYTES) {
            return Err("planning groups exceed supported sizes".into());
        }
    }
    super::records::RecordLimits::check_composed(json)?;
    let graph = parse_graph(json, bin);
    let (barriers, barrier_toggles) = parse_barriers(json)?;
    let hints = parse_hints(json, hint_capacity);
    let (paths, boundary, boundary_polygon_count) = parse_paths_and_boundary(json);
    Ok(Swbf2NavData {
        map: json.get("map").cloned().unwrap_or(Value::Null),
        graph,
        barriers,
        barrier_toggles,
        hints,
        boundary,
        boundary_polygon_count,
        paths,
        regions: json.get("regions").cloned().unwrap_or(Value::Null),
    })
}
