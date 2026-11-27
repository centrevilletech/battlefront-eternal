//! Interior sector and portal visibility culling for maps with authored
//! `portals` (`sectors` and `portals` arrays in `map.json`, e.g. capital-ship
//! hangars and corridors). All scratch buffers are preallocated at admission so
//! per-frame updates perform zero heap allocations.
use bfe_wide::Wide;
use bfe_render::math::Frustum;
use serde_json::Value;
use std::collections::HashMap;
use three::{Quaternion, Vector3};

const MAX_SECTORS: usize = 64;

struct Sector {
    base: f64,
    top: f64,
    points: Vec<[f64; 2]>,
    min_x: f64,
    max_x: f64,
    min_z: f64,
    max_z: f64,
}

struct Portal {
    corners: [[f64; 3]; 4],
    center: [f64; 3],
    radius: f64,
    sector1: usize,
    sector2: Option<usize>,
}

pub struct PortalCuller {
    sectors: Vec<Sector>,
    portals: Vec<Portal>,
    /// Bitmask of sectors each prop record belongs to (0 = exterior / unassigned).
    record_sectors: Vec<Wide>,
    visible_mask: Wide,
    // Preallocated scratch buffers for zero-allocation BFS per frame.
    sector_rects: Vec<[f64; 4]>,
    queue: [usize; MAX_SECTORS],
}

impl Default for PortalCuller {
    fn default() -> Self {
        Self {
            sectors: Vec::new(),
            portals: Vec::new(),
            record_sectors: Vec::new(),
            visible_mask: Wide::ZERO,
            sector_rects: Vec::new(),
            queue: [0; MAX_SECTORS],
        }
    }
}

fn point_in_polygon(x: f64, z: f64, poly: &[[f64; 2]]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, zi) = (poly[i][0], poly[i][1]);
        let (xj, zj) = (poly[j][0], poly[j][1]);
        if ((zi > z) != (zj > z)) && (x < (xj - xi) * (z - zi) / (zj - zi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Project a 3D portal quad into clip space, clipping edges against the near plane
/// (`z + w >= 0`), and return its clamped 2D NDC bounding box `[min_x, min_y, max_x, max_y]`.
fn project_portal_ndc(corners: &[[f64; 3]; 4], vp: &[f64; 16]) -> Option<[f64; 4]> {
    let mut clip = [[0.0f64; 4]; 4];
    for (i, c) in corners.iter().enumerate() {
        for r in 0..4 {
            clip[i][r] = vp[r] * c[0] + vp[4 + r] * c[1] + vp[8 + r] * c[2] + vp[12 + r];
        }
    }
    // Clip polygon against near plane (z + w >= 0) using Sutherland-Hodgman into a fixed array.
    let mut poly = [[0.0f64; 4]; 8];
    let mut count = 0usize;
    for i in 0..4 {
        let a = clip[i];
        let b = clip[(i + 1) & 3];
        let da = a[2] + a[3];
        let db = b[2] + b[3];
        let a_in = da >= 0.0 && a[3] > 1e-6;
        let b_in = db >= 0.0 && b[3] > 1e-6;
        if a_in && count < 8 {
            poly[count] = a;
            count += 1;
        }
        if (da >= 0.0) != (db >= 0.0) {
            let denom = da - db;
            if denom.abs() > 1e-12 {
                let t = (da / denom).clamp(0.0, 1.0);
                let p = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
                if p[3] > 1e-6 && count < 8 {
                    poly[count] = p;
                    count += 1;
                }
            }
        } else if !a_in && b_in && a[3] <= 1e-6 && b[3] > 1e-6 {
            let t = ((1e-5 - a[3]) / (b[3] - a[3])).clamp(0.0, 1.0);
            let p = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
            if p[2] + p[3] >= 0.0 && count < 8 {
                poly[count] = p;
                count += 1;
            }
        }
    }
    if count == 0 {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in &poly[..count] {
        let inv_w = 1.0 / p[3];
        let x = p[0] * inv_w;
        let y = p[1] * inv_w;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if max_x < -1.0 || min_x > 1.0 || max_y < -1.0 || min_y > 1.0 {
        return None;
    }
    Some([
        min_x.max(-1.0),
        min_y.max(-1.0),
        max_x.min(1.0),
        max_y.min(1.0),
    ])
}

fn intersect_rect(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (r[0] < r[2] && r[1] < r[3]).then_some(r)
}

impl PortalCuller {
    pub fn new(portals_val: &Value, record_names: &[Option<String>]) -> Self {
        let Some(raw_sectors) = portals_val.get("sectors").and_then(Value::as_array) else {
            return Self::default();
        };
        let Some(raw_portals) = portals_val.get("portals").and_then(Value::as_array) else {
            return Self::default();
        };
        if raw_sectors.is_empty() || raw_portals.is_empty() {
            return Self::default();
        }

        let mut sectors = Vec::with_capacity(raw_sectors.len().min(MAX_SECTORS));
        let mut name_to_sector: HashMap<String, usize> = HashMap::new();
        let mut obj_to_mask: HashMap<String, Wide> = HashMap::new();

        for s in raw_sectors.iter().take(MAX_SECTORS) {
            let Some(name) = s.get("name").and_then(Value::as_str) else {
                continue;
            };
            let base = s.get("base").and_then(Value::as_f64).unwrap_or(0.0);
            let height = s.get("height").and_then(Value::as_f64).unwrap_or(0.0);
            let points: Vec<[f64; 2]> = s
                .get("points")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|p| {
                    let a = p.as_array()?;
                    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
                })
                .collect();
            if points.len() < 3 || height <= 0.0 {
                continue;
            }
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_z = f64::INFINITY;
            let mut max_z = f64::NEG_INFINITY;
            for &[px, pz] in &points {
                min_x = min_x.min(px);
                max_x = max_x.max(px);
                min_z = min_z.min(pz);
                max_z = max_z.max(pz);
            }
            let idx = sectors.len();
            name_to_sector.insert(name.to_ascii_lowercase(), idx);
            let bit = Wide::bit(idx as u32);
            if let Some(objects) = s.get("objects").and_then(Value::as_array) {
                for obj in objects.iter().filter_map(Value::as_str) {
                    let mask = obj_to_mask.entry(obj.to_ascii_lowercase()).or_insert(Wide::ZERO);
                    *mask = mask.or(bit);
                }
            }
            sectors.push(Sector {
                base,
                top: base + height,
                points,
                min_x,
                max_x,
                min_z,
                max_z,
            });
        }

        let mut portals = Vec::with_capacity(raw_portals.len());
        for p in raw_portals {
            let Some(s1_name) = p.get("sector1").and_then(Value::as_str) else {
                continue;
            };
            let Some(&sector1) = name_to_sector.get(&s1_name.to_ascii_lowercase()) else {
                continue;
            };
            let sector2 = p
                .get("sector2")
                .and_then(Value::as_str)
                .and_then(|n| name_to_sector.get(&n.to_ascii_lowercase()).copied());
            let Some(pos) = p.get("position").and_then(Value::as_array) else {
                continue;
            };
            let Some(rot) = p.get("rotation").and_then(Value::as_array) else {
                continue;
            };
            if pos.len() < 3 || rot.len() < 4 {
                continue;
            }
            let center = [
                pos[0].as_f64().unwrap_or(0.0),
                pos[1].as_f64().unwrap_or(0.0),
                pos[2].as_f64().unwrap_or(0.0),
            ];
            // Zero engine stores portal quaternions as [w, x, y, z].
            let mut q = Quaternion::new(
                rot[1].as_f64().unwrap_or(0.0),
                rot[2].as_f64().unwrap_or(0.0),
                rot[3].as_f64().unwrap_or(0.0),
                rot[0].as_f64().unwrap_or(1.0),
            );
            q.normalize();
            let hw = p.get("width").and_then(Value::as_f64).unwrap_or(0.0) * 0.5;
            let hh = p.get("height").and_then(Value::as_f64).unwrap_or(0.0) * 0.5;
            if hw <= 0.0 || hh <= 0.0 {
                continue;
            }
            let mut right = Vector3::new(hw, 0.0, 0.0);
            let mut up = Vector3::new(0.0, hh, 0.0);
            right.apply_quaternion(&q);
            up.apply_quaternion(&q);
            let corner = |sr: f64, su: f64| {
                [
                    center[0] + right.x * sr + up.x * su,
                    center[1] + right.y * sr + up.y * su,
                    center[2] + right.z * sr + up.z * su,
                ]
            };
            let corners = [
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ];
            let radius = hw.hypot(hh);
            portals.push(Portal {
                corners,
                center,
                radius,
                sector1,
                sector2,
            });
        }

        if sectors.is_empty() || portals.is_empty() {
            return Self::default();
        }

        let record_sectors: Vec<Wide> = record_names
            .iter()
            .map(|name| {
                name.as_ref()
                    .and_then(|n| obj_to_mask.get(&n.to_ascii_lowercase()).copied())
                    .unwrap_or(Wide::ZERO)
            })
            .collect();
        let n = sectors.len();
        Self {
            sectors,
            portals,
            record_sectors,
            visible_mask: Wide::MAX,
            sector_rects: vec![[1.0, 1.0, -1.0, -1.0]; n],
            queue: [0; MAX_SECTORS],
        }
    }

    pub fn is_active(&self) -> bool {
        !self.sectors.is_empty()
    }

    fn contains(s: &Sector, eye: [f64; 3]) -> bool {
        !(eye[1] < s.base - 0.5 || eye[1] > s.top + 0.5)
            && !(eye[0] < s.min_x || eye[0] > s.max_x || eye[2] < s.min_z || eye[2] > s.max_z)
            && point_in_polygon(eye[0], eye[2], &s.points)
    }

    pub fn camera_sector(&self, eye: [f64; 3]) -> Option<usize> {
        self.sectors.iter().position(|s| Self::contains(s, eye))
    }

    /// Recompute visible interior sectors for this frame. Zero heap allocations.
    pub fn update(&mut self, eye: [f64; 3], frustum: &Frustum, view_proj: &[f64; 16]) {
        if self.sectors.is_empty() {
            self.visible_mask = Wide::MAX;
            return;
        }
        self.visible_mask = Wide::ZERO;
        for r in &mut self.sector_rects {
            *r = [1.0, 1.0, -1.0, -1.0];
        }
        // Fixed-capacity ring buffer with `in_queue` deduplication so each sector
        // appears at most once in the pending queue regardless of how many portals
        // widen its screen-space rect.
        let mut head = 0usize;
        let mut tail = 0usize;
        let mut in_queue = Wide::ZERO;
        let enqueue = |s: usize, queue: &mut [usize; MAX_SECTORS], tail: &mut usize, in_queue: &mut Wide| {
            let bit = Wide::bit(s as u32);
            if in_queue.and(bit).is_zero() {
                *in_queue = in_queue.or(bit);
                queue[*tail] = s;
                *tail = (*tail + 1) % MAX_SECTORS;
            }
        };

        // Authored sectors overlap (a room's box reaches into the hall beside
        // it). The 2005 game starts from every sector that holds the eye, not
        // only the one listed earliest.
        for (start, sector) in self.sectors.iter().enumerate() {
            if Self::contains(sector, eye) {
                self.visible_mask = self.visible_mask.or(Wide::bit(start as u32));
                self.sector_rects[start] = [-1.0, -1.0, 1.0, 1.0];
                enqueue(start, &mut self.queue, &mut tail, &mut in_queue);
            }
        }
        if in_queue.is_zero() {
            // Exterior camera: enter any sector whose exterior portal intersects the frustum.
            for p in &self.portals {
                if p.sector2.is_some() {
                    continue;
                }
                if !frustum.sphere(&p.center, p.radius) {
                    continue;
                }
                if let Some(rect) = project_portal_ndc(&p.corners, view_proj) {
                    let s = p.sector1;
                    let cur = &mut self.sector_rects[s];
                    let prev_empty = cur[0] > cur[2];
                    let next = if prev_empty {
                        rect
                    } else {
                        [
                            cur[0].min(rect[0]),
                            cur[1].min(rect[1]),
                            cur[2].max(rect[2]),
                            cur[3].max(rect[3]),
                        ]
                    };
                    if prev_empty || next != *cur {
                        *cur = next;
                        self.visible_mask = self.visible_mask.or(Wide::bit(s as u32));
                        enqueue(s, &mut self.queue, &mut tail, &mut in_queue);
                    }
                }
            }
        }

        let max_steps = self.sectors.len().saturating_mul(16).max(64);
        let mut steps = 0usize;
        while !in_queue.is_zero() {
            if steps >= max_steps {
                // Fail visible if a pathological cyclic graph does not converge.
                self.visible_mask = Wide::MAX;
                return;
            }
            steps += 1;
            let u = self.queue[head];
            head = (head + 1) % MAX_SECTORS;
            in_queue = in_queue.and(Wide::bit(u as u32).not());
            let u_rect = self.sector_rects[u];
            for p in &self.portals {
                let next_sector = if p.sector1 == u {
                    p.sector2
                } else if p.sector2 == Some(u) {
                    Some(p.sector1)
                } else {
                    None
                };
                let Some(v) = next_sector else {
                    continue;
                };
                if !frustum.sphere(&p.center, p.radius) {
                    continue;
                }
                let Some(portal_rect) = project_portal_ndc(&p.corners, view_proj) else {
                    continue;
                };
                let Some(clipped) = intersect_rect(u_rect, portal_rect) else {
                    continue;
                };
                let cur = &mut self.sector_rects[v];
                let prev_empty = cur[0] > cur[2];
                let merged = if prev_empty {
                    clipped
                } else {
                    [
                        cur[0].min(clipped[0]),
                        cur[1].min(clipped[1]),
                        cur[2].max(clipped[2]),
                        cur[3].max(clipped[3]),
                    ]
                };
                if prev_empty || merged != *cur {
                    *cur = merged;
                    self.visible_mask = self.visible_mask.or(Wide::bit(v as u32));
                    enqueue(v, &mut self.queue, &mut tail, &mut in_queue);
                }
            }
        }
    }

    #[inline]
    pub fn record_visible(&self, record_index: usize) -> bool {
        let mask = self.record_sectors.get(record_index).copied().unwrap_or(Wide::ZERO);
        mask.is_zero() || !mask.and(self.visible_mask).is_zero()
    }

    #[cfg(test)]
    pub fn visible_mask(&self) -> Wide {
        self.visible_mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use three::{CoordinateSystem, Matrix4};

    fn make_vp(eye: [f64; 3], target: [f64; 3]) -> ([f64; 16], Frustum) {
        let mut proj = Matrix4::default();
        let top = 0.3 * (60.0f64.to_radians() * 0.5).tan();
        let right = top * (16.0 / 9.0);
        proj.make_perspective(-right, right, top, -top, 0.3, 2000.0, CoordinateSystem::WebGL);
        let mut view = Matrix4::default();
        view.look_at(
            &Vector3::new(eye[0], eye[1], eye[2]),
            &Vector3::new(target[0], target[1], target[2]),
            &Vector3::new(0.0, 1.0, 0.0),
        );
        view.set_position(eye[0], eye[1], eye[2]);
        view.invert();
        let vp = bfe_render::math::mul(&proj.elements, &view.elements);
        let frustum = Frustum::from_matrix(&vp);
        (vp, frustum)
    }

    #[test]
    fn interior_sectors_cull_when_facing_away_and_open_through_portals() {
        let data = json!({
            "sectors": [
                {
                    "name": "bay",
                    "base": 0.0, "height": 20.0,
                    "points": [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]],
                    "objects": ["bay_prop"]
                },
                {
                    "name": "hall",
                    "base": 0.0, "height": 10.0,
                    "points": [[-5.0, 10.0], [5.0, 10.0], [5.0, 30.0], [-5.0, 30.0]],
                    "objects": ["hall_prop"]
                }
            ],
            "portals": [
                {"name": "p_ext", "rotation": [1.0, 0.0, 0.0, 0.0], "position": [0.0, 10.0, -10.0], "width": 16.0, "height": 16.0, "sector1": "bay"},
                {"name": "p_hall", "rotation": [1.0, 0.0, 0.0, 0.0], "position": [0.0, 4.0, 10.0], "width": 6.0, "height": 6.0, "sector1": "bay", "sector2": "hall"}
            ]
        });
        let names = vec![Some("bay_prop".into()), Some("hall_prop".into()), Some("ship_hull".into())];
        let mut culler = PortalCuller::new(&data, &names);
        assert!(culler.is_active());

        // Inside bay looking away from hall (-Z): bay visible, hall culled, exterior prop always eligible.
        let (vp, f) = make_vp([0.0, 5.0, 0.0], [0.0, 5.0, -20.0]);
        culler.update([0.0, 5.0, 0.0], &f, &vp);
        assert_eq!(culler.camera_sector([0.0, 5.0, 0.0]), Some(0));
        assert!(culler.record_visible(0));
        assert!(!culler.record_visible(1));
        assert!(culler.record_visible(2));

        // Inside bay looking toward hall (+Z): both bay and hall visible through p_hall.
        let (vp2, f2) = make_vp([0.0, 5.0, 0.0], [0.0, 5.0, 20.0]);
        culler.update([0.0, 5.0, 0.0], &f2, &vp2);
        assert!(culler.record_visible(0));
        assert!(culler.record_visible(1));

        // Outside bay facing away (-Z from z = -50): both interior sectors culled.
        let (vp3, f3) = make_vp([0.0, 5.0, -50.0], [0.0, 5.0, -100.0]);
        culler.update([0.0, 5.0, -50.0], &f3, &vp3);
        assert_eq!(culler.camera_sector([0.0, 5.0, -50.0]), None);
        assert_eq!(culler.visible_mask(), Wide::ZERO);
        assert!(!culler.record_visible(0));
        assert!(!culler.record_visible(1));
        assert!(culler.record_visible(2));
    }

    #[test]
    fn an_eye_where_two_sectors_overlap_sees_both_without_their_portal() {
        // A long hall listed first whose box reaches into the room beside it;
        // the only way between them is a small door far to one side.
        let data = json!({
            "sectors": [
                {"name": "hall", "base": 0.0, "height": 12.0,
                 "points": [[-30.0, -8.0], [30.0, -8.0], [30.0, 6.0], [-30.0, 6.0]], "objects": ["hall_bench"]},
                {"name": "room", "base": 0.0, "height": 6.0,
                 "points": [[-12.0, 2.0], [12.0, 2.0], [12.0, 20.0], [-12.0, 20.0]], "objects": ["room_seat"]}
            ],
            "portals": [
                {"name": "door", "rotation": [1.0, 0.0, 0.0, 0.0], "position": [10.0, 2.0, 2.0],
                 "width": 2.0, "height": 3.0, "sector1": "hall", "sector2": "room"}
            ]
        });
        let names = vec![Some("hall_bench".into()), Some("room_seat".into())];
        let mut culler = PortalCuller::new(&data, &names);
        // Standing in the strip both boxes cover, looking into the room away from the door.
        let eye = [-6.0, 1.7, 4.0];
        let (vp, f) = make_vp(eye, [-6.0, 1.7, 30.0]);
        culler.update(eye, &f, &vp);
        assert!(culler.record_visible(1), "the room holding the eye must be drawn");
        assert!(culler.record_visible(0));
        // Deeper in the room only the room holds the eye; the hall stays behind its door.
        let eye = [-6.0, 1.7, 12.0];
        let (vp, f) = make_vp(eye, [-6.0, 1.7, 30.0]);
        culler.update(eye, &f, &vp);
        assert!(culler.record_visible(1));
        assert!(!culler.record_visible(0));
    }

    #[test]
    fn multiple_parallel_portals_deduplicate_queue_without_starving_deep_sectors() {
        // Sector 0 connects to Sector 1 through 6 parallel windows that each widen
        // Sector 1's screen rect; Sector 1 connects to Sector 2 through the rightmost
        // window. Without deduplication a tiny queue budget would drop Sector 2.
        let mut portals = Vec::new();
        for i in 0..6 {
            let x = -5.0 + i as f64 * 2.0;
            portals.push(json!({
                "name": format!("w_{i}"),
                "rotation": [1.0, 0.0, 0.0, 0.0],
                "position": [x, 4.0, 10.0],
                "width": 1.6,
                "height": 4.0,
                "sector1": "s0",
                "sector2": "s1"
            }));
        }
        portals.push(json!({
            "name": "deep",
            "rotation": [1.0, 0.0, 0.0, 0.0],
            "position": [5.0, 4.0, 20.0],
            "width": 2.0,
            "height": 4.0,
            "sector1": "s1",
            "sector2": "s2"
        }));
        let data = json!({
            "sectors": [
                {"name": "s0", "base": 0.0, "height": 10.0, "points": [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]], "objects": ["o0"]},
                {"name": "s1", "base": 0.0, "height": 10.0, "points": [[-10.0, 10.0], [10.0, 10.0], [10.0, 20.0], [-10.0, 20.0]], "objects": ["o1"]},
                {"name": "s2", "base": 0.0, "height": 10.0, "points": [[-10.0, 20.0], [10.0, 20.0], [10.0, 30.0], [-10.0, 30.0]], "objects": ["o2"]}
            ],
            "portals": portals
        });
        let names = vec![Some("o0".into()), Some("o1".into()), Some("o2".into())];
        let mut culler = PortalCuller::new(&data, &names);
        let (vp, f) = make_vp([0.0, 4.0, -2.0], [0.0, 4.0, 30.0]);
        culler.update([0.0, 4.0, -2.0], &f, &vp);
        assert!(culler.record_visible(0));
        assert!(culler.record_visible(1));
        assert!(culler.record_visible(2), "deep sector s2 must remain visible");
    }
}
