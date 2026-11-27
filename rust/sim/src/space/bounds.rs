//! Space battlefield spatial bounds, oriented landing regions, hangar bay
//! forcefields, and coordinate precision auditing.
//!
//! Space maps have no terrain heightfield and span several kilometres across
//! all three axes. Landing regions (such as capital ship hangars) are oriented
//! boxes admitted strictly via explicit registration (`AddLandingRegion`), and
//! hangar bay forcefields admit starfighters (`UseVCollForFlyers = 1`) while
//! blocking foot soldiers and infantry ordnance.

use crate::collision::{CollisionOwner, Ray};
use bfe_content::js::to_number;
use bfe_content::map::is_auxiliary_region;
use bfe_content::Value;
use three::{Matrix4, Vector3};

/// Provisional margin inside the outer boundary within which a bot pilot turns
/// back toward the middle of the battlefield (metres). Nothing turns a person's
/// craft; not yet measured against the 2005 game.
pub const PROVISIONAL_BOUNDARY_TURN_MARGIN: f64 = 150.0;

/// How long a crewed craft may stay outside the space boundary before it is
/// destroyed (seconds), as in the 2005 game.
pub const OUT_OF_BOUNDS_TIME: f64 = 6.5;
/// The same for a player steering a guided rocket outside the battlefield:
/// the rocket is lost when it has run out.
pub const GUIDED_OUT_OF_BOUNDS_TIME: f64 = 13.0 / 6.0;

/// Default spatial half-extent when a mission does not call `SetWorldExtents`.
pub const DEFAULT_WORLD_EXTENTS: f64 = 1024.0;

/// Default vertical space ceiling/floor half-extent when map bounds omit Y.
pub const DEFAULT_SPACE_VERTICAL_EXTENT: f64 = 1800.0;

/// Provisional fallback local half-extents for a hangar bay forcefield plane
/// when the prop model box is absent (metres).
pub const PROVISIONAL_FORCEFIELD_PORTAL_TOLERANCE: f64 = 1.0;
pub const PROVISIONAL_FORCEFIELD_DEFAULT_HALF_EXTENTS: [f64; 3] = [35.0, 18.0, 1.5];

/// Provisional lower local Y bound for hangar forcefield flyer pass-through
/// matching (metres).
pub const PROVISIONAL_FORCEFIELD_LOCAL_Y_MIN: f64 = -4.0;

/// Provisional upper local Y bound for hangar forcefield flyer pass-through
/// matching (metres).
pub const PROVISIONAL_FORCEFIELD_LOCAL_Y_MAX: f64 = 32.0;

/// Provisional thickness tolerance around a hangar forcefield plane for flyer
/// pass-through matching (metres).
pub const PROVISIONAL_FORCEFIELD_THICKNESS_TOL: f64 = 4.0;

/// Provisional minimum normal alignment cosine for hangar forcefield flyer
/// pass-through matching.
pub const PROVISIONAL_FORCEFIELD_NORMAL_DOT: f64 = 0.85;

/// Provisional minimum half-width clamp for hangar forcefield flyer
/// pass-through matching (metres).
pub const PROVISIONAL_FORCEFIELD_MIN_WIDTH: f64 = 60.0;

/// Provisional extra vertical ray length when sampling hangar deck elevation
/// inside a landing region (metres).
pub const PROVISIONAL_FLOOR_RAY_EXTRA: f64 = 10.0;

/// Oriented box region used for hangar landing zones (`AddLandingRegion`) and
/// hangar containment queries.
#[derive(Clone, Debug)]
pub struct OrientedRegion {
    pub name: String,
    pub team: i32,
    pub center: [f64; 3],
    pub matrix: [f64; 16],
    pub inverse: [f64; 16],
    pub half_extents: [f64; 3],
    /// Deck elevation from the registration ray. An empty ray cannot authorize landing.
    pub floor_y: Option<f64>,
}

impl OrientedRegion {
    pub fn from_matrix(
        name: String,
        team: i32,
        matrix: [f64; 16],
        half_extents: [f64; 3],
        floor_y: impl Into<Option<f64>>,
    ) -> Self {
        let mut inv = Matrix4 { elements: matrix };
        inv.invert();
        Self {
            name,
            team,
            center: [matrix[12], matrix[13], matrix[14]],
            matrix,
            inverse: inv.elements,
            half_extents,
            floor_y: floor_y.into().filter(|y| y.is_finite()),
        }
    }

    /// Tests whether a world-space point lies inside this oriented box.
    #[inline]
    pub fn contains(&self, point: [f64; 3]) -> bool {
        let mut local = Vector3::new(point[0], point[1], point[2]);
        let inv = Matrix4 { elements: self.inverse };
        local.apply_matrix4(&inv);
        local.x.abs() <= self.half_extents[0]
            && local.y.abs() <= self.half_extents[1]
            && local.z.abs() <= self.half_extents[2]
    }
}

/// Hangar bay forcefield volume (`UseVCollForFlyers = 1`).
///
/// Starfighters pass cleanly through these forcefields when taking off or
/// returning to a hangar, while foot soldiers and infantry ordnance treat them
/// as solid barriers.
#[derive(Clone, Debug)]
pub struct HangarForcefield {
    pub name: String,
    pub team: i32,
    pub center: [f64; 3],
    pub matrix: [f64; 16],
    pub inverse: [f64; 16],
    /// Local half-extents of the shield plane for soldier/ordnance blocking.
    pub half_extents: [f64; 3],
    /// Kernel body slot if the shield mesh also has a collision body in
    /// `world.collision`, so flyer sweeps ignore it while soldiers hit it.
    pub kernel_body: Option<u32>,
    pub authored_opening: bool,
}

impl HangarForcefield {
    /// Tests whether a collision hit `(code, point, normal)` belongs to this
    /// hangar bay forcefield slab (`UseVCollForFlyers = 1`), matching either a
    /// dedicated `kernel_body` or static world collision triangles aligned with
    /// the authored shield plane (local X or local Z normal).
    pub fn matches_flyer_passthrough(&self, code: i32, point: [f64; 3], normal: [f64; 3]) -> bool {
        if code >= 0 && self.kernel_body == Some(code as u32) {
            return true;
        }
        let mut local = Vector3::new(point[0], point[1], point[2]);
        let inv = Matrix4 { elements: self.inverse };
        local.apply_matrix4(&inv);
        if self.authored_opening {
            let m = &self.matrix;
            let alignment = (normal[0]*m[8] + normal[1]*m[9] + normal[2]*m[10]).abs();
            return code == crate::collision::STATIC && alignment >= PROVISIONAL_FORCEFIELD_NORMAL_DOT
                && local.x.abs() <= self.half_extents[0] && local.y.abs() <= self.half_extents[1]
                && local.z.abs() <= self.half_extents[2];
        }
        if local.y < PROVISIONAL_FORCEFIELD_LOCAL_Y_MIN
            || local.y > PROVISIONAL_FORCEFIELD_LOCAL_Y_MAX
        {
            return false;
        }
        let m = &self.matrix;
        let dot_x = (normal[0] * m[0] + normal[1] * m[1] + normal[2] * m[2]).abs();
        let dot_z = (normal[0] * m[8] + normal[1] * m[9] + normal[2] * m[10]).abs();
        let width_limit = v8math::max(
            PROVISIONAL_FORCEFIELD_MIN_WIDTH,
            v8math::max(self.half_extents[0], self.half_extents[2]),
        );
        if dot_x > PROVISIONAL_FORCEFIELD_NORMAL_DOT
            && local.x.abs() <= PROVISIONAL_FORCEFIELD_THICKNESS_TOL
            && local.z.abs() <= width_limit
        {
            return true;
        }
        if dot_z > PROVISIONAL_FORCEFIELD_NORMAL_DOT
            && local.z.abs() <= PROVISIONAL_FORCEFIELD_THICKNESS_TOL
            && local.x.abs() <= width_limit
        {
            return true;
        }
        false
    }

    /// Tests whether a world-space segment `(from -> to)` crosses this shield
    /// plane. Used to block soldiers and infantry ordnance while flyers pass free.
    pub fn blocks_segment(&self, from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
        let inv = Matrix4 { elements: self.inverse };
        let mut a = Vector3::new(from[0], from[1], from[2]);
        let mut b = Vector3::new(to[0], to[1], to[2]);
        a.apply_matrix4(&inv);
        b.apply_matrix4(&inv);

        // Choose thin axis (local Z by default, or local X if authored narrower).
        let thin_x = self.half_extents[0] < self.half_extents[2];
        let (an, bn, au, bu, thick, width) = if thin_x {
            (
                a.x,
                b.x,
                a.z,
                b.z,
                self.half_extents[0],
                self.half_extents[2],
            )
        } else {
            (
                a.z,
                b.z,
                a.x,
                b.x,
                self.half_extents[2],
                self.half_extents[0],
            )
        };
        let dn = bn - an;
        if an.abs() <= thick && bn.abs() <= thick {
            if au.abs() <= width && a.y.abs() <= self.half_extents[1] {
                return Some(from);
            }
            return None;
        }
        if (an > 0.0 && bn > 0.0) || (an < 0.0 && bn < 0.0) || dn.abs() < 1e-9 {
            return None;
        }
        let t = -an / dn;
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        let lu = au + (bu - au) * t;
        let ly = a.y + (b.y - a.y) * t;
        if lu.abs() <= width && ly.abs() <= self.half_extents[1] {
            Some([
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
                from[2] + (to[2] - from[2]) * t,
            ])
        } else {
            None
        }
    }
}

/// Space boundary check result at a given world position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryCheck {
    /// True when the point is outside the boundary across the ground plane (x
    /// and z only, as the game tests it; the floor and ceiling are the fly
    /// heights, which level a craft off instead).
    pub outside: bool,
    /// True when the point is within the margin inside the edge, floor or
    /// ceiling where a bot pilot turns back.
    pub warning: bool,
    /// Signed distance inside the boundary, floor and ceiling (positive
    /// inside, negative outside).
    pub signed_margin: f64,
    /// Unit vector pointing back toward the playable center.
    pub inward: [f64; 3],
}

/// Space battlefield spatial envelope, authored non-auxiliary box regions,
/// registered landing zones, and hangar forcefields.
#[derive(Clone, Debug)]
pub struct SpaceBounds {
    /// World AABB minimum `[x, y, z]`.
    pub min: [f64; 3],
    /// World AABB maximum `[x, y, z]`.
    pub max: [f64; 3],
    /// Center of the playable space volume.
    pub center: [f64; 3],
    /// Spatial world half-extent from `SetWorldExtents` (defaults to `1024.0`).
    pub world_extents: f64,
    /// Authored 2D XZ boundary polygon from `boundaryPaths`, if present.
    pub boundary_polygon: Vec<[f64; 2]>,
    /// Non-auxiliary authored box regions from `map.json` eligible for explicit
    /// `AddLandingRegion` registration.
    pub authored_regions: Vec<OrientedRegion>,
    /// Active oriented landing zones registered explicitly via
    /// `register_landing_region` / `add_landing_region` (empty until registered).
    pub landing_zones: Vec<OrientedRegion>,
    /// Hangar bay forcefields (`UseVCollForFlyers = 1`).
    pub forcefields: Vec<HangarForcefield>,
}

/// Precision audit at the farthest reachable coordinate on the battlefield.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacePrecisionReport {
    pub farthest_coord_m: f64,
    pub f64_ulp_m: f64,
    pub f32_vertex_ulp_m: f64,
}

impl SpacePrecisionReport {
    /// Measures `f64` simulation ULP and `f32` collision vertex ULP at
    /// `farthest_coord_m`.
    pub fn measure(farthest_coord_m: f64) -> Self {
        let abs_c = farthest_coord_m.abs();
        let f64_ulp_m = f64::from_bits(abs_c.to_bits() + 1) - abs_c;
        let f32_c = abs_c as f32;
        let f32_ulp_m = (f32::from_bits(f32_c.to_bits() + 1) - f32_c) as f64;
        Self {
            farthest_coord_m: abs_c,
            f64_ulp_m,
            f32_vertex_ulp_m: f32_ulp_m,
        }
    }
}

fn read_vec3(v: Option<&Value>, fallback: [f64; 3]) -> [f64; 3] {
    let Some(arr) = v.and_then(Value::as_array) else {
        return fallback;
    };
    if arr.len() < 3 {
        return fallback;
    }
    [
        {
            let x = to_number(arr.first());
            if x.is_finite() { x } else { fallback[0] }
        },
        {
            let y = to_number(arr.get(1));
            if y.is_finite() { y } else { fallback[1] }
        },
        {
            let z = to_number(arr.get(2));
            if z.is_finite() { z } else { fallback[2] }
        },
    ]
}

fn read_matrix16(v: Option<&Value>) -> [f64; 16] {
    let Some(arr) = v.and_then(Value::as_array) else {
        return Matrix4::default().elements;
    };
    if arr.len() < 16 {
        return Matrix4::default().elements;
    }
    std::array::from_fn(|i| {
        let x = to_number(arr.get(i));
        if x.is_finite() { x } else { 0.0 }
    })
}

/// Parses an oriented box region record from `map.json` `regions`.
pub fn parse_region(reg: &Value) -> Option<OrientedRegion> {
    let name = reg.get("name").and_then(Value::as_str)?.to_string();
    let half_extents = read_vec3(reg.get("size"), [10.0, 10.0, 10.0]);
    let mut matrix = read_matrix16(reg.get("matrix"));
    // Some region records store `position` separately when `matrix` is omitted.
    if reg.get("matrix").is_none() {
        let pos = read_vec3(reg.get("position"), [0.0, 0.0, 0.0]);
        matrix[12] = pos[0];
        matrix[13] = pos[1];
        matrix[14] = pos[2];
    }
    let lower = name.to_ascii_lowercase();
    let team = if lower.contains("rep") || lower.contains("all") {
        1
    } else if lower.contains("cis") || lower.contains("imp") {
        2
    } else {
        0
    };
    Some(OrientedRegion::from_matrix(
        name,
        team,
        matrix,
        half_extents,
        None,
    ))
}

fn is_box_region(reg: &Value) -> bool {
    match reg.get("type").and_then(Value::as_str) {
        Some(t) => t.eq_ignore_ascii_case("box"),
        None => true,
    }
}

/// Computes the signed 2D distance of `(x, z)` from a closed polygon:
/// positive inside, negative outside.
fn polygon_signed_margin(poly: &[[f64; 2]], x: f64, z: f64) -> f64 {
    let n = poly.len();
    if n < 3 {
        return f64::INFINITY;
    }
    let mut inside = false;
    let mut min_dist_sq = f64::INFINITY;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, zi) = (poly[i][0], poly[i][1]);
        let (xj, zj) = (poly[j][0], poly[j][1]);
        // Ray-casting point-in-polygon parity test.
        if ((zi > z) != (zj > z)) && (x < (xj - xi) * (z - zi) / (zj - zi) + xi) {
            inside = !inside;
        }
        // Distance squared to segment (j -> i).
        let dx = xi - xj;
        let dz = zi - zj;
        let len_sq = dx * dx + dz * dz;
        let t = if len_sq > 1e-12 {
            v8math::min(
                1.0,
                v8math::max(0.0, ((x - xj) * dx + (z - zj) * dz) / len_sq),
            )
        } else {
            0.0
        };
        let px = xj + dx * t;
        let pz = zj + dz * t;
        let dsq = (x - px) * (x - px) + (z - pz) * (z - pz);
        if dsq < min_dist_sq {
            min_dist_sq = dsq;
        }
        j = i;
    }
    let dist = min_dist_sq.sqrt();
    if inside { dist } else { -dist }
}

impl SpaceBounds {
    /// Extracts space bounds, boundary polygon, non-auxiliary authored box
    /// regions, and hangar forcefields from an admitted `map.json` view.
    ///
    /// `landing_zones` always starts EMPTY and is populated only by explicit
    /// `register_landing_region` / `add_landing_region` calls (equivalent to
    /// mission script `AddLandingRegion`), preventing audio or lighting volumes
    /// from ever masquerading as hangar decks.
    pub fn from_world_json(world: &Value) -> Self {
        let mut min = read_vec3(
            world.get("bounds").and_then(|b| b.get("min")),
            [
                -DEFAULT_WORLD_EXTENTS,
                -DEFAULT_SPACE_VERTICAL_EXTENT,
                -DEFAULT_WORLD_EXTENTS,
            ],
        );
        let mut max = read_vec3(
            world.get("bounds").and_then(|b| b.get("max")),
            [
                DEFAULT_WORLD_EXTENTS,
                DEFAULT_SPACE_VERTICAL_EXTENT,
                DEFAULT_WORLD_EXTENTS,
            ],
        );
        // The floor and ceiling are the mission's fly heights when it sets them.
        let mission = world.get("mission");
        let fly_height = |key: &str| mission.and_then(|m| m.get(key)).and_then(Value::as_f64).filter(|h| h.is_finite());
        if let Some(h) = fly_height("minFlyHeight") {
            min[1] = h;
        }
        if let Some(h) = fly_height("maxFlyHeight") {
            max[1] = h;
        }
        let mut center = [
            0.5 * (min[0] + max[0]),
            0.5 * (min[1] + max[1]),
            0.5 * (min[2] + max[2]),
        ];

        // Parse authored boundary polygon from `boundaryPaths` + `paths`.
        let mut boundary_polygon = Vec::new();
        let boundary_names: Vec<String> = world
            .get("boundaryPaths")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if !boundary_names.is_empty() {
            if let Some(paths) = world.get("paths").and_then(Value::as_array) {
                for bname in &boundary_names {
                    let Some(path) = paths.iter().find(|p| {
                        p.get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|n| n.eq_ignore_ascii_case(bname))
                    }) else {
                        continue;
                    };
                    if let Some(nodes) = path.get("nodes").and_then(Value::as_array) {
                        for node in nodes {
                            let pos = read_vec3(node.get("position"), [0.0; 3]);
                            boundary_polygon.push([pos[0], pos[2]]);
                        }
                        if boundary_polygon.len() >= 3 {
                            break;
                        }
                    }
                }
            }
        }

        // Parse `mapbounds` center and collect non-auxiliary authored box
        // regions for explicit `AddLandingRegion` lookup. Never auto-admit
        // landing zones by name substring.
        let mut authored_regions = Vec::new();
        if let Some(regions) = world.get("regions").and_then(Value::as_array) {
            for reg in regions {
                let Some(parsed) = parse_region(reg) else {
                    continue;
                };
                if parsed.name.eq_ignore_ascii_case("mapbounds") {
                    center[0] = parsed.center[0];
                    center[2] = parsed.center[2];
                    continue;
                }
                if is_auxiliary_region(&parsed.name) || !is_box_region(reg) {
                    continue;
                }
                authored_regions.push(parsed);
            }
        }

        // Parse hangar bay forcefields (`UseVCollForFlyers = 1` or shield door instances).
        let mut forcefields = Vec::new();
        if let Some(instances) = world.get("instances").and_then(Value::as_array) {
            let classes = world.get("classes").and_then(Value::as_object);
            for inst in instances {
                let cls = inst.get("class").and_then(Value::as_str).unwrap_or("");
                let name = inst.get("name").and_then(Value::as_str).unwrap_or(cls);
                let is_flyer_shield = cls.to_ascii_lowercase().contains("shield")
                    || classes
                        .and_then(|c| c.get(cls).or_else(|| c.get(&cls.to_ascii_lowercase())))
                        .and_then(|crec| crec.get("properties"))
                        .and_then(Value::as_array)
                        .is_some_and(|pairs| {
                            pairs.iter().any(|p| {
                                p.at(0)
                                    .and_then(Value::as_str)
                                    .is_some_and(|k| k.eq_ignore_ascii_case("UseVCollForFlyers"))
                                    && p.at(1).and_then(Value::as_str) == Some("1")
                            })
                        });
                if !is_flyer_shield {
                    continue;
                }
                let matrix = read_matrix16(inst.get("matrix"));
                let mut inv = Matrix4 { elements: matrix };
                inv.invert();
                let lower = name.to_ascii_lowercase();
                let team = if lower.contains("rep") || lower.contains("all") {
                    1
                } else if lower.contains("cis") || lower.contains("imp") {
                    2
                } else {
                    0
                };
                forcefields.push(HangarForcefield {
                    name: name.to_string(),
                    team,
                    center: [matrix[12], matrix[13], matrix[14]],
                    matrix,
                    inverse: inv.elements,
                    half_extents: PROVISIONAL_FORCEFIELD_DEFAULT_HALF_EXTENTS,
                    kernel_body: None,
                    authored_opening: false,
                });
            }
        }

        Self {
            min,
            max,
            center,
            world_extents: DEFAULT_WORLD_EXTENTS,
            boundary_polygon,
            authored_regions,
            landing_zones: Vec::new(),
            forcefields,
        }
    }

    /// Applies mission `SetWorldExtents(half_extent)`.
    pub fn set_world_extents(&mut self, half_extent: f64) -> bool {
        if !half_extent.is_finite() || half_extent <= 0.0 {
            return false;
        }
        self.world_extents = half_extent;
        if self.boundary_polygon.len() < 3 {
            self.min[0] = self.center[0] - half_extent;
            self.max[0] = self.center[0] + half_extent;
            self.min[2] = self.center[2] - half_extent;
            self.max[2] = self.center[2] + half_extent;
        }
        true
    }

    /// Registers a named non-auxiliary authored box region as an active landing
    /// zone (equivalent to mission `AddLandingRegion`). Returns its landing
    /// zone index if found, or `None` if no authored gameplay box matches.
    pub fn register_landing_region(&mut self, name: &str) -> Option<usize> {
        let region = self
            .authored_regions
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(name))?
            .clone();
        Some(self.add_landing_region(region))
    }

    /// Registers an explicit landing zone directly and returns its index.
    pub fn add_landing_region(&mut self, region: OrientedRegion) -> usize {
        if let Some(idx) = self
            .landing_zones
            .iter()
            .position(|z| z.name.eq_ignore_ascii_case(&region.name))
        {
            self.landing_zones[idx] = region;
            idx
        } else {
            let idx = self.landing_zones.len();
            self.landing_zones.push(region);
            idx
        }
    }

    /// Unregisters a landing zone by name (equivalent to removing a landing
    /// region when a hangar is disabled). Returns true if removed.
    pub fn remove_landing_region(&mut self, name: &str) -> bool {
        let Some(idx) = self
            .landing_zones
            .iter()
            .position(|z| z.name.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        self.landing_zones.remove(idx);
        true
    }

    /// Samples the actual hangar deck floor Y inside landing zone `zone_idx`
    /// using a downward collision raycast against `collision`.
    pub fn sample_landing_zone_floor(
        &mut self,
        zone_idx: usize,
        collision: &mut CollisionOwner,
        collision_world: usize,
    ) {
        let Some(zone) = self.landing_zones.get_mut(zone_idx) else {
            return;
        };
        let from = [
            zone.center[0],
            zone.center[1] + zone.half_extents[1] * 0.5,
            zone.center[2],
        ];
        let far = zone.half_extents[1] * 2.0 + PROVISIONAL_FLOOR_RAY_EXTRA;
        let ray = Ray { ox: from[0], oy: from[1], oz: from[2], dx: 0.0, dy: -1.0, dz: 0.0 };
        // These mission boxes are fixed in the world. A parked or passing fighter
        // in the registration ray cannot supply their deck elevation.
        let Some(world) = collision.state.worlds.get(collision_world) else {
            zone.floor_y = None;
            return;
        };
        let mut candidates = vec![0; world.order.len()];
        let count = crate::collision::world_candidates_into(
            &collision.state, collision_world, &ray, -1, true, false, &mut candidates,
        );
        candidates.truncate(count);
        let enabled: Vec<_> = candidates.iter().map(|place|
            collision.gathers.places[world.order[*place as usize] as usize]
        ).collect();
        let (code, hit) = crate::collision::world_cast_ray_in(
            &mut collision.state, collision_world, &ray, far, 0.0, &candidates, &enabled,
        );
        zone.floor_y = (code != crate::collision::MISS && hit.py.is_finite()).then_some(hit.py);
    }

    /// Returns true if a collision hit `(code, point, normal)` lies on an
    /// authored hangar bay forcefield that starfighters fly through.
    pub fn is_forcefield_hit(&self, code: i32, point: [f64; 3], normal: [f64; 3]) -> bool {
        self.forcefields
            .iter()
            .any(|ff| ff.matches_flyer_passthrough(code, point, normal))
    }

    /// Tests whether a world-space segment `(from -> to)` is blocked by any
    /// hangar bay forcefield (used for infantry shots and boarding actors).
    pub fn forcefield_blocks_segment(&self, from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
        for ff in &self.forcefields {
            if let Some(hit) = ff.blocks_segment(from, to) {
                return Some(hit);
            }
        }
        None
    }

    /// Returns the landing zone containing `point`, if any.
    pub fn landing_zone_at(&self, point: [f64; 3]) -> Option<&OrientedRegion> {
        self.landing_zones.iter().find(|z| z.contains(point))
    }

    /// Evaluates space boundary containment, soft warning margin, and inward
    /// recovery direction at `point`.
    pub fn check_boundary(&self, point: [f64; 3]) -> BoundaryCheck {
        let margin_y = v8math::min(point[1] - self.min[1], self.max[1] - point[1]);
        let margin_xz = if self.boundary_polygon.len() >= 3 {
            polygon_signed_margin(&self.boundary_polygon, point[0], point[2])
        } else {
            let mx = v8math::min(point[0] - self.min[0], self.max[0] - point[0]);
            let mz = v8math::min(point[2] - self.min[2], self.max[2] - point[2]);
            v8math::min(mx, mz)
        };
        let signed_margin = v8math::min(margin_xz, margin_y);
        let dx = self.center[0] - point[0];
        let dy = self.center[1] - point[1];
        let dz = self.center[2] - point[2];
        let len = v8math::hypot3(dx, dy, dz);
        let inward = if len > 1e-6 {
            [dx / len, dy / len, dz / len]
        } else {
            [0.0, 0.0, 1.0]
        };
        BoundaryCheck {
            outside: margin_xz < 0.0,
            warning: signed_margin < PROVISIONAL_BOUNDARY_TURN_MARGIN,
            signed_margin,
            inward,
        }
    }

    /// Measures coordinate precision at the farthest reachable corner of the
    /// space battlefield.
    pub fn precision_report(&self) -> SpacePrecisionReport {
        let mut farthest = self.world_extents;
        for &c in self.min.iter().chain(self.max.iter()) {
            let a = c.abs();
            if a > farthest {
                farthest = a;
            }
        }
        for p in &self.boundary_polygon {
            for &c in p {
                let a = c.abs();
                if a > farthest {
                    farthest = a;
                }
            }
        }
        SpacePrecisionReport::measure(farthest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content::json::parse;

    #[test]
    fn oriented_hangar_region_distinguishes_rotated_corners() {
        // Rotate 45 deg about Y around (100, -140, 200) with half-extents (50, 15, 20).
        let a = std::f64::consts::FRAC_PI_4;
        let (s, c) = (v8math::sin(a), v8math::cos(a));
        let matrix = [
            c, 0.0, -s, 0.0,
            0.0, 1.0, 0.0, 0.0,
            s, 0.0, c, 0.0,
            100.0, -140.0, 200.0, 1.0,
        ];
        let rgn = OrientedRegion::from_matrix(
            "hangar_south".into(),
            2,
            matrix,
            [50.0, 15.0, 20.0],
            -152.0,
        );
        // Along rotated local +X (40 m): inside.
        assert!(rgn.contains([100.0 + 40.0 * c, -140.0, 200.0 - 40.0 * s]));
        // Along rotated local +Z (35 m > 20 m): outside, even though an AABB would accept it.
        assert!(!rgn.contains([100.0 + 35.0 * s, -140.0, 200.0 + 35.0 * c]));
    }

    #[test]
    fn landing_regions_require_explicit_registration_and_reject_audio_volumes() {
        let map = parse(
            r#"{
                "regions": [
                    {"name": "mapbounds", "type": "box", "size": [1500, 10, 1500], "position": [0, 0, 0]},
                    {"name": "bay_north_rgn", "type": "box", "size": [40, 12, 30], "position": [0, 0, -500]},
                    {"name": "bay_south_rgn", "type": "box", "size": [50, 14, 35], "position": [0, 0, 500]},
                    {"name": "soundstream hangar_amb 1.0", "type": "sphere", "size": [90, 0, 0], "position": [0, 0, -500]},
                    {"name": "soundspace hangar_reverb", "type": "box", "size": [60, 20, 60], "position": [0, 0, 500]},
                    {"name": "hangar_sphere_only", "type": "sphere", "size": [30, 0, 0], "position": [0, 0, 0]}
                ]
            }"#,
        )
        .unwrap();

        let mut bounds = SpaceBounds::from_world_json(&map);
        assert_eq!(bounds.world_extents, DEFAULT_WORLD_EXTENTS);
        assert_eq!(bounds.world_extents, 1024.0);
        // No auto-admission from substrings!
        assert_eq!(bounds.landing_zones.len(), 0);
        // Only the two real non-auxiliary box regions are eligible.
        assert_eq!(bounds.authored_regions.len(), 2);

        // Registering audio or sphere volumes fails.
        assert_eq!(bounds.register_landing_region("soundstream hangar_amb 1.0"), None);
        assert_eq!(bounds.register_landing_region("soundspace hangar_reverb"), None);
        assert_eq!(bounds.register_landing_region("hangar_sphere_only"), None);
        assert_eq!(bounds.landing_zones.len(), 0);

        // Explicit registration admits exactly the requested authored boxes.
        assert_eq!(bounds.register_landing_region("bay_north_rgn"), Some(0));
        assert_eq!(bounds.register_landing_region("BAY_SOUTH_RGN"), Some(1));
        // Re-registering is idempotent.
        assert_eq!(bounds.register_landing_region("bay_north_rgn"), Some(0));
        assert_eq!(bounds.landing_zones.len(), 2);
        assert!(bounds.landing_zone_at([0.0, 0.0, -500.0]).is_some());
        assert!(bounds.remove_landing_region("bay_north_rgn"));
        assert_eq!(bounds.landing_zones.len(), 1);
    }

    #[test]
    fn space_boundary_warns_near_edge_and_flags_outside() {
        let mut bounds = SpaceBounds {
            min: [-2000.0, -1800.0, -2000.0],
            max: [2000.0, 1800.0, 2000.0],
            center: [0.0, 0.0, 0.0],
            world_extents: DEFAULT_WORLD_EXTENTS,
            boundary_polygon: vec![
                [-2000.0, -2000.0],
                [2000.0, -2000.0],
                [2000.0, 2000.0],
                [-2000.0, 2000.0],
            ],
            authored_regions: vec![],
            landing_zones: vec![],
            forcefields: vec![],
        };
        assert!(bounds.set_world_extents(2000.0));
        assert_eq!(bounds.world_extents, 2000.0);

        let mid = bounds.check_boundary([0.0, 0.0, 0.0]);
        assert!(!mid.outside && !mid.warning);

        let near = bounds.check_boundary([1920.0, 0.0, 0.0]);
        assert!(!near.outside && near.warning);
        assert!(near.inward[0] < -0.99);

        let out = bounds.check_boundary([2100.0, 0.0, 0.0]);
        assert!(out.outside && out.warning && out.signed_margin < 0.0);
        // Over the ceiling is not outside: the fly height levels a craft off there.
        let high = bounds.check_boundary([0.0, bounds.max[1] + 50.0, 0.0]);
        assert!(!high.outside && high.warning && high.signed_margin < 0.0);

        let report = bounds.precision_report();
        assert!(report.f64_ulp_m < 1e-11, "f64 ULP at 2000m should be < 1e-11m, got {}", report.f64_ulp_m);
        assert!(report.f32_vertex_ulp_m < 1e-3, "f32 vertex ULP at 2000m should be sub-mm, got {}", report.f32_vertex_ulp_m);

        bounds.add_landing_region(OrientedRegion::from_matrix(
            "hangar_north".into(),
            1,
            Matrix4::default().elements,
            [50.0, 15.0, 40.0],
            -2.0,
        ));
        assert!(bounds.landing_zone_at([0.0, 0.0, 0.0]).is_some());
    }

    #[test]
    fn the_floor_and_ceiling_are_the_missions_fly_heights() {
        let read = |text: &str| SpaceBounds::from_world_json(&bfe_content::json::parse(text).unwrap());
        let b = read(r#"{"bounds":{"min":[-100,-50,-100],"max":[100,50,100]},"mission":{"minFlyHeight":-30,"maxFlyHeight":40}}"#);
        assert_eq!((b.min[1], b.max[1]), (-30.0, 40.0));
        let b = read(r#"{"bounds":{"min":[-100,-50,-100],"max":[100,50,100]},"mission":{"maxFlyHeight":null}}"#);
        assert_eq!((b.min[1], b.max[1]), (-50.0, 50.0));
    }
}
