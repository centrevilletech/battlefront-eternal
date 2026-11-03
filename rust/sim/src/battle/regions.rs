//! World regions (src/regions.js): capture, control, kill, death and light
//! regions as the 2004 game builds and tests them (wrld regn: TYPE, XFRM,
//! SIZE). By type:
//! - sphere: SIZE as it is; its radius is that length, about its centre;
//! - box: SIZE scaled by the lengths of the XFRM's axes, half extents along
//!   its unit axes;
//! - cylinder: SIZE scaled the same way, then x and z both
//!   sqrt(x^2 + z^2): the radius about its unit y axis, y the half height.

use three::{Matrix4, Vector3};

/// A region's TYPE as the world file names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RegionType {
    Sphere,
    Box,
    Cylinder,
    /// Any other name: holds nothing.
    #[default]
    Other,
}

impl RegionType {
    pub fn named(name: &str) -> RegionType {
        match name {
            "sphere" => RegionType::Sphere,
            "box" => RegionType::Box,
            "cylinder" => RegionType::Cylinder,
            _ => RegionType::Other,
        }
    }
}

/// A region as the world loader keeps it: its type, its XFRM (a column-major
/// 4x4, when it has 16 numbers or more), else a centre, and its SIZE.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    pub kind: RegionType,
    pub matrix: Option<[f64; 16]>,
    pub center: Option<[f64; 3]>,
    pub size: [f64; 3],
}

/// buildRegion's answer: { type, half: the built size, radius, center,
/// inverse: world to the region's unit axes }.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Built {
    pub kind: RegionType,
    pub half: [f64; 3],
    pub radius: f64,
    pub center: Vector3,
    pub inverse: Matrix4,
}

/// buildRegion(region) (regions.js:23).
pub fn build_region(region: &Region) -> Built {
    let mut matrix = Matrix4::default();
    if let Some(m) = &region.matrix {
        matrix.from_array(m, 0);
    } else if let Some(c) = region.center {
        matrix.set_position(c[0], c[1], c[2]);
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
    let size = region.size;
    let mut half = [size[0] * scale[0], size[1] * scale[1], size[2] * scale[2]];
    let mut radius = 0.0;
    if region.kind == RegionType::Sphere {
        half = size;
        radius = v8math::hypot3(size[0], size[1], size[2]);
    } else if region.kind == RegionType::Cylinder {
        radius = v8math::hypot(half[0], half[2]);
        half = [radius, half[1], radius];
    }
    let mut center = Vector3::default();
    center.set_from_matrix_position(&matrix);
    let mut unit = Matrix4::default();
    unit.make_basis(&axes[0], &axes[1], &axes[2]).set_position_v3(&center);
    unit.invert();
    Built { kind: region.kind, half, radius, center, inverse: unit }
}

/// inRegion(built, point) (regions.js:50): whether a built region holds a
/// world point.
pub fn in_region(built: &Built, point: &Vector3) -> bool {
    let mut scratch = *point;
    if built.kind == RegionType::Sphere {
        return scratch.sub(&built.center).length() < built.radius;
    }
    scratch.apply_matrix4(&built.inverse);
    let [a, b, c] = built.half;
    match built.kind {
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
    pub fn new(kill_y: f64, regions: &[(Option<String>, Region)], scripted: &[String]) -> DeathRegions {
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
