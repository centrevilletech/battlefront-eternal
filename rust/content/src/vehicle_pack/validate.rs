//! Validate references and resource limits before constructing collision storage.
use crate::value::{Object, Value};

const MAX_RECORDS: usize = 4096;
const MAX_VERTICES: usize = 1_000_000;
const MAX_INDICES: usize = 3_000_000;
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SEATS: usize = 6;
pub const MAX_WEAPONS: usize = 5;
pub const MAX_SALVO: usize = 128;

fn object<'a>(value: &'a Value, field: &str) -> Result<&'a Object, String> {
    value.as_object().filter(|o| o.len() <= MAX_RECORDS)
        .ok_or_else(|| format!("{field}: expected a bounded object"))
}

fn list<'a>(value: &'a Value, key: &str, max: usize) -> Result<&'a [Value], String> {
    match value.get(key) {
        None => Ok(&[]),
        Some(v) => v.as_array().filter(|a| a.len() <= max)
            .ok_or_else(|| format!("{key}: expected a bounded array")),
    }
}

fn integer(value: &Value, limit: usize, field: &str) -> Result<usize, String> {
    let n = value.as_f64().or_else(|| value.as_str()?.parse().ok());
    n.filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n < limit as f64)
        .map(|n| n as usize).ok_or_else(|| format!("{field}: invalid integer index"))
}

fn optional_index(v: Option<&Value>, count: usize, root: bool, field: &str) -> Result<(), String> {
    if let Some(v) = v.filter(|v| !v.is_null()) {
        if !root || v.as_f64() != Some(-1.0) { integer(v, count, field)?; }
    }
    Ok(())
}

fn vector(v: &Value, width: usize, key: &str) -> Result<(), String> {
    // Products of two admitted components must remain representable in f32.
    if v.as_array().is_some_and(|a| a.len() == width && a.iter().all(|v|
        v.as_f64().is_some_and(|n| n.is_finite() && n.abs() <= 1.0e18))) { return Ok(()); }
    Err(format!("{key}: invalid {width}-component vector"))
}

fn optional_vector(r: &Value, key: &str, width: usize) -> Result<(), String> {
    if let Some(v) = r.get(key).filter(|v| !v.is_null()) { vector(v, width, key)?; }
    Ok(())
}

fn indices(r: &Value, key: &str, bound: usize, max: usize) -> Result<(), String> {
    for id in list(r, key, max)? { integer(id, bound, key)?; }
    Ok(())
}

fn shape(r: &Value, joints: usize, total: &mut [usize; 2]) -> Result<(), String> {
    object(r, "collision shape")?;
    optional_index(r.get("joint"), joints, false, "joint")?;
    optional_index(r.get("mask"), 256, false, "mask")?;
    optional_vector(r, "matrix", 16)?;
    optional_vector(r, "size", 3)?;
    let verts = list(r, "positions", MAX_VERTICES)?;
    let tris = list(r, "indices", MAX_INDICES)?;
    total[0] += verts.len();
    total[1] += tris.len();
    if total[0] > MAX_VERTICES || total[1] > MAX_INDICES || !tris.len().is_multiple_of(3) {
        return Err("invalid vehicle triangle extent".into());
    }
    for p in verts { vector(p, 3, "position")?; }
    indices(r, "indices", verts.len(), MAX_INDICES)?;
    let mut triangle_count = 0;
    for face in list(r, "polygons", MAX_INDICES / 3)? {
        let face = face.as_array().filter(|f| (3..=MAX_RECORDS).contains(&f.len()))
            .ok_or("invalid vehicle face")?;
        triangle_count += face.len() - 2;
        if triangle_count > tris.len() / 3 { return Err("excess vehicle face triangles".into()); }
        for id in face { integer(id, verts.len(), "polygon vertex")?; }
    }
    Ok(())
}

fn model(r: &Value, total: &mut [usize; 2]) -> Result<(), String> {
    object(r, "vehicle model")?;
    let joints = list(r, "skeleton", MAX_RECORDS)?;
    let mut parents = Vec::with_capacity(joints.len());
    for j in joints {
        object(j, "joint")?;
        let parent = j.get("parent").filter(|v| !v.is_null() && v.as_f64() != Some(-1.0))
            .map(|v| integer(v, joints.len(), "parent")).transpose()?;
        parents.push(parent);
        optional_vector(j, "local", 16)?;
        optional_vector(j, "matrix", 16)?;
    }
    // Validate the parent forest even when a child precedes its parent.
    let mut marks = vec![0u8; parents.len()];
    for i in 0..parents.len() {
        let mut at = Some(i);
        while let Some(j) = at {
            match marks[j] { 1 => return Err("cyclic vehicle skeleton".into()), 2 => break, _ => {} }
            marks[j] = 1;
            at = parents[j];
        }
        at = Some(i);
        while let Some(j) = at {
            if marks[j] != 1 { break; }
            marks[j] = 2;
            at = parents[j];
        }
    }
    for m in list(r, "meshes", MAX_RECORDS)? { shape(m, joints.len(), total)?; }
    for p in list(r, "primitives", MAX_RECORDS)? {
        shape(p, joints.len(), total)?;
        let kind = integer(p.get("kind").ok_or("missing primitive kind")?, 5, "primitive kind")?;
        if !matches!(kind, 0 | 1 | 2 | 4) { return Err("unsupported vehicle primitive kind".into()); }
    }
    Ok(())
}

fn class(r: &Value, models: &Object) -> Result<(), String> {
    object(r, "vehicle class")?;
    for key in ["model", "collisionModel"] {
        if let Some(v) = r.get(key).filter(|v| !v.is_null()) {
            let name = v.as_str().ok_or("invalid vehicle model name")?;
            if !name.is_empty() && models.get(name).is_none() { return Err(format!("unknown vehicle {key}")); }
        }
    }
    let points = list(r, "points", MAX_RECORDS)?;
    for pt in points {
        object(pt, "attachment point")?;
        optional_vector(pt, "matrix", 16)?;
        if let Some(m) = super::read_matrix16(pt.get("matrix")) {
            let det = m[0] * (m[5] * m[10] - m[6] * m[9])
                - m[4] * (m[1] * m[10] - m[2] * m[9])
                + m[8] * (m[1] * m[6] - m[2] * m[5]);
            if det == 0.0 || [m[3], m[7], m[11], m[15]] != [0.0, 0.0, 0.0, 1.0] {
                return Err("invalid attachment affine transform".into());
            }
        }
        optional_vector(pt, "offset", 3)?;
        if let Some(joint) = pt.get("joint").filter(|v| !v.is_null()) {
            let m = pt.get("model").and_then(Value::as_str).and_then(|n| models.get(n))
                .ok_or("unknown attachment model")?;
            integer(joint, list(m, "skeleton", MAX_RECORDS)?.len(), "attachment joint")?;
        }
    }
    let mut count = 0;
    for seat in list(r, "seats", MAX_SEATS)? {
        object(seat, "seat")?;
        indices(seat, "points", points.len(), MAX_RECORDS)?;
        let weapons = list(seat, "weapons", MAX_WEAPONS)?;
        count += weapons.len();
        if count > MAX_WEAPONS { return Err("vehicle exceeds weapon capacity".into()); }
        for w in weapons {
            object(w, "seat weapon")?;
            if w.get("class").and_then(Value::as_str).is_none_or(|name| name.trim().is_empty()) {
                return Err("seat weapon requires a class name".into());
            }
            integer(w.get("section").ok_or("missing weapon section")?, MAX_WEAPONS+1, "weapon section")?;
            for binding in list(w, "points", MAX_RECORDS)? {
                object(binding, "weapon point")?;
                for key in ["point", "aimer", "barrel"] { optional_index(binding.get(key), points.len(), false, key)?; }
            }
            for index in list(w, "firePoints", MAX_RECORDS)? {
                let p = &points[integer(index, points.len(), "firePoints")?];
                if !["matrix", "offset"].iter().any(|key| p.get(key).is_some_and(|v| !v.is_null())) {
                    return Err("fire point requires a matrix or offset".into());
                }
            }
            for p in list(w, "extraOffsets", MAX_RECORDS)? { vector(p, 3, "muzzle offset")?; }
        }
    }
    Ok(())
}

pub(super) fn pack(v: &Value) -> Result<(), String> {
    object(v, "vehicle pack")?;
    if v.get("version").and_then(Value::as_f64) != Some(1.0) { return Err("unsupported vehicle pack version".into()); }
    let models = object(v.get("models").ok_or("missing vehicle models")?, "models")?;
    let classes = object(v.get("classes").ok_or("missing vehicle classes")?, "classes")?;
    for (kind, records) in [("class", classes), ("model", models)] {
        let mut names = std::collections::HashSet::with_capacity(records.len());
        for name in records.keys() {
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(format!("ambiguous vehicle {kind} name: {name}"));
            }
        }
    }
    let mut total = [0, 0];
    for (name, m) in models.entries() { model(m, &mut total).map_err(|e| format!("{name}: {e}"))?; }
    for (name, c) in classes.entries() { class(c, models).map_err(|e| format!("{name}: {e}"))?; }
    Ok(())
}
