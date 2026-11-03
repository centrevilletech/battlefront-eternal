//! World regions (src/regions.js): capture, control, kill, death and light
//! regions as the 2004 game builds and tests them (wrld regn: TYPE, XFRM,
//! SIZE). By type:
//! - sphere: SIZE as it is; its radius is that length, about its centre;
//! - box: SIZE scaled by the lengths of the XFRM's axes, half extents along
//!   its unit axes;
//! - cylinder: SIZE scaled the same way, then x and z both
//!   sqrt(x^2 + z^2): the radius about its unit y axis, y the half height.

use crate::gen::{Built, RegionDesc, RegionType};
use three::{Matrix4, Vector3};

/// A region's built inverse as a three.js matrix.
fn inverse_of(built: &Built) -> Matrix4 {
    let mut m = Matrix4::default();
    m.from_array(&built.inverse, 0);
    m
}

/// A region as a world file gives it, from its TYPE, its XFRM's 16 numbers
/// (when it has them), a centre and its SIZE (the twin's and the death
/// regions' loader).
pub fn region_desc(kind: RegionType, matrix: Option<&[f64]>, center: Option<&[f64]>, size: [f64; 3]) -> RegionDesc {
    RegionDesc { r#type: kind, matrix: matrix.map(|m| m.to_vec()), center: center.map(|c| c.to_vec()), size: size.to_vec() }
}

/// buildRegion(region) (regions.js:23).
pub fn build_region(region: &RegionDesc) -> Built {
    let mut matrix = Matrix4::default();
    // `region.matrix?.length >= 16`, else `region.center` (any array).
    if let Some(m) = region.matrix.as_ref().filter(|m| m.len() >= 16) {
        matrix.from_array(m, 0);
    } else if let Some(c) = &region.center {
        let at = |i: usize| c.get(i).copied().unwrap_or(f64::NAN);
        matrix.set_position(at(0), at(1), at(2));
    }
    let mut axes = [Vector3::default(), Vector3::default(), Vector3::default()];
    {
        let [x, y, z] = &mut axes;
        matrix.extract_basis(x, y, z);
    }
    let mut scale = [0.0; 3];
    for (i, axis) in axes.iter_mut().enumerate() {
        let length = axis.length();
        if length > 0.0 {
            axis.divide_scalar(length);
        }
        // `length || 1`: 0 and NaN give 1.
        scale[i] = if length != 0.0 && !length.is_nan() { length } else { 1.0 };
    }
    let size = [0, 1, 2].map(|i| region.size.get(i).copied().unwrap_or(f64::NAN));
    let mut half = [size[0] * scale[0], size[1] * scale[1], size[2] * scale[2]];
    let mut radius = 0.0;
    if region.r#type == RegionType::Sphere {
        half = size;
        radius = v8math::hypot3(size[0], size[1], size[2]);
    } else if region.r#type == RegionType::Cylinder {
        radius = v8math::hypot(half[0], half[2]);
        half = [radius, half[1], radius];
    }
    let mut center = Vector3::default();
    center.set_from_matrix_position(&matrix);
    let mut unit = Matrix4::default();
    unit.make_basis(&axes[0], &axes[1], &axes[2]).set_position_v3(&center);
    unit.invert();
    Built { r#type: region.r#type, half, radius, center: [center.x, center.y, center.z], inverse: unit.elements }
}

/// inRegion(built, point) (regions.js:50): whether a built region holds a
/// world point.
pub fn in_region(built: &Built, point: &Vector3) -> bool {
    let mut scratch = *point;
    if built.r#type == RegionType::Sphere {
        let c = Vector3::new(built.center[0], built.center[1], built.center[2]);
        return scratch.sub(&c).length() < built.radius;
    }
    scratch.apply_matrix4(&inverse_of(built));
    let [a, b, c] = built.half;
    match built.r#type {
        RegionType::Cylinder => v8math::hypot(scratch.x, scratch.z) < built.radius && v8math::abs(scratch.y) < b,
        RegionType::Box => v8math::abs(scratch.x) < a && v8math::abs(scratch.y) < b && v8math::abs(scratch.z) < c,
        _ => false,
    }
}

/// The regions that kill (src/game.js inDeathRegion): below the map's
/// lowest ground less 10 m (killY), or inside a death region: one named
/// deathregion* (any case; an unnamed one counts) or one the battle's script
/// names with AddDeathRegion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeathRegions {
    pub kill_y: f64,
    pub regions: Vec<Built>,
}

impl DeathRegions {
    /// The world's regions (name, region) and the script's names.
    pub fn new(kill_y: f64, regions: &[(Option<String>, RegionDesc)], scripted: &[String]) -> DeathRegions {
        let scripted: Vec<String> = scripted.iter().map(|n| n.to_lowercase()).collect();
        let regions = regions
            .iter()
            .filter(|(name, _)| {
                // /^deathregion/i.test(region.name || "deathregion")
                let named = name.as_deref().filter(|n| !n.is_empty()).unwrap_or("deathregion");
                let prefix = named.len() >= 11 && named.as_bytes()[..11].eq_ignore_ascii_case(b"deathregion");
                // scripted.has(String(region.name).toLowerCase())
                let listed = scripted.contains(&name.as_deref().unwrap_or("undefined").to_lowercase());
                prefix || listed
            })
            .map(|(_, r)| build_region(r))
            .collect();
        DeathRegions { kill_y, regions }
    }

    /// inDeathRegion(p).
    pub fn contains(&self, p: &Vector3) -> bool {
        p.y < self.kill_y || self.regions.iter().any(|r| in_region(r, p))
    }
}
