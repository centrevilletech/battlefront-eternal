//! World regions as src/regions.js buildRegion builds them (capture, death,
//! kill and light regions alike), and the battle's death regions as game.js
//! picks them.

use crate::js::{js_hypot, to_number};
use crate::three::{Matrix4, Vector3};
use crate::value::Value;

#[derive(Clone, Debug)]
pub struct BuiltRegion {
    pub kind: Option<Value>,
    /// The built size (a sphere keeps every element of its SIZE).
    pub half: Vec<f64>,
    pub radius: f64,
    pub center: Vector3,
    /// World to the region's unit axes.
    pub inverse: Matrix4,
}

fn element(v: Option<&Value>, i: usize) -> f64 {
    // array[i] in arithmetic: undefined is NaN.
    match v.and_then(|a| a.at(i)) {
        Some(x) => to_number(Some(x)),
        None => f64::NAN,
    }
}

/// regions.js buildRegion(region).
pub fn build_region(region: &Value) -> BuiltRegion {
    let mut matrix = Matrix4::identity();
    let m = region.get("matrix");
    let length = match m {
        Some(Value::Array(a)) => a.len() as f64,
        Some(Value::String(s)) => s.encode_utf16().count() as f64,
        _ => f64::NAN,
    };
    if length >= 16.0 {
        if let Some(Value::Array(a)) = m {
            let values: Vec<f64> = a.iter().take(16).map(|x| to_number(Some(x))).collect();
            matrix = Matrix4::from_array(&values);
        }
    } else if region.get("center").map(Value::truthy).unwrap_or(false) {
        let c = region.get("center");
        matrix.set_position(&Vector3::new(element(c, 0), element(c, 1), element(c, 2)));
    }
    let mut axes = matrix.extract_basis();
    let scale: Vec<f64> = axes
        .iter_mut()
        .map(|axis| {
            let length = axis.length();
            if length > 0.0 {
                axis.divide_scalar(length);
            }
            if length == 0.0 || length.is_nan() {
                1.0
            } else {
                length
            }
        })
        .collect();
    let size = region.get("size");
    let mut half: Vec<f64> = (0..3).map(|i| element(size, i) * scale[i]).collect();
    let mut radius = 0.0;
    let kind = region.get("type").cloned();
    let kind_str = kind.as_ref().and_then(Value::as_str);
    if kind_str == Some("sphere") {
        half = size.and_then(Value::as_array).unwrap_or(&[]).iter().map(|x| to_number(Some(x))).collect();
        radius = js_hypot(&[element(size, 0), element(size, 1), element(size, 2)]);
    } else if kind_str == Some("cylinder") {
        radius = js_hypot(&[half[0], half[2]]);
        half = vec![radius, half[1], radius];
    }
    let mut center = Vector3::default();
    center.set_from_matrix_position(&matrix);
    let mut unit = Matrix4::make_basis(&axes[0], &axes[1], &axes[2]);
    unit.set_position(&center);
    BuiltRegion { kind, half, radius, center, inverse: unit.inverted() }
}

/// `/^deathregion/i.test(name)`: ASCII case folding is exact for this
/// pattern (no other character folds to an ASCII letter without /u).
fn starts_deathregion(name: &str) -> bool {
    name.len() >= 11 && name.as_bytes()[..11].eq_ignore_ascii_case(b"deathregion")
}

/// String(x) for a region name.
fn text(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => crate::js::number_to_string(*n),
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => crate::dump::value_text(v.unwrap()),
    }
}

/// game.js 524-527: the regions the battle's script names with
/// AddDeathRegion, and those named deathregion*, in map.json's order.
pub fn death_regions(world: &Value) -> Vec<(Value, BuiltRegion)> {
    let scripted: Vec<String> = world
        .get("mission")
        .and_then(|m| m.get("deathRegions"))
        .filter(|d| d.truthy())
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .map(|n| text(Some(n)).to_lowercase())
        .collect();
    let mut out = Vec::new();
    for region in world.get("deathRegions").filter(|d| d.truthy()).and_then(Value::as_array).unwrap_or(&[]) {
        let name = region.get("name");
        let tested = match name.filter(|n| n.truthy()) {
            Some(n) => text(Some(n)),
            None => "deathregion".into(),
        };
        if starts_deathregion(&tested) || scripted.contains(&text(name).to_lowercase()) {
            out.push((region.clone(), build_region(region)));
        }
    }
    out
}
