//! Hangar exits, launch paths and home decks, read once at admission.
//!
//! An exit is an authored hangar opening (a portal between a hangar sector and
//! open space). Its outward direction points away from the pads of the team
//! whose hangar it opens. A launch path is the map's flyer path whose first
//! node lies nearest outside that opening.

use super::super::bounds::SpaceBounds;
use super::super::flight::SpaceVehiclePad;
use super::vec::{add, dist, dot, scale, sub, unit};
use bfe_content::Value;

/// A flyer path's name starts with this tag in the 2005 path chunks.
const FLYER_PATH_TAG: &str = "type_entitypath ";
/// A launch path starts no farther than this outside its opening.
pub const PROVISIONAL_LAUNCH_PATH_REACH: f64 = 450.0;
/// Waypoints inside and outside an opening, along its outward direction.
pub const PROVISIONAL_EXIT_INSIDE: f64 = 32.0;
pub const PROVISIONAL_EXIT_OUTSIDE: f64 = 70.0;

#[derive(Clone, Debug, Default)]
pub struct Exit {
    pub team: i32,
    pub center: [f64; 3],
    /// Unit vector from the hangar into open space.
    pub outward: [f64; 3],
    pub inside: [f64; 3],
    pub outside: [f64; 3],
    /// Unit vector across the opening (its width), and up through it.
    pub across: [f64; 3],
    pub up: [f64; 3],
    /// Half width and half height of the opening.
    pub half: [f64; 2],
    /// Range of `Routes::nodes` holding the launch path, empty without one.
    pub path: (u32, u32),
    /// Every launch path starting near the opening (up to four, nearest
    /// first; `path` is the first): a craft may take any of them.
    pub paths: [(u32, u32); 4],
    pub path_count: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Routes {
    pub exits: Vec<Exit>,
    pub nodes: Vec<[f64; 3]>,
}

fn flyer_paths(world: &Value) -> Vec<Vec<[f64; 3]>> {
    let mut out = Vec::new();
    for path in world.get("paths").and_then(Value::as_array).unwrap_or(&[]) {
        let name = path.get("name").and_then(Value::as_str).unwrap_or("");
        // Compared as bytes: the tag is ASCII, a name need not be.
        if !name.as_bytes().get(..FLYER_PATH_TAG.len()).is_some_and(|b| b.eq_ignore_ascii_case(FLYER_PATH_TAG.as_bytes())) {
            continue;
        }
        let nodes: Vec<[f64; 3]> = path
            .get("nodes")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(|node| {
                let p = node.get("position")?.as_array()?;
                let v: Vec<f64> = p.iter().filter_map(Value::as_f64).collect();
                (v.len() == 3 && v.iter().all(|x| x.is_finite())).then(|| [v[0], v[1], v[2]])
            })
            .collect();
        if nodes.len() >= 2 {
            out.push(nodes);
        }
    }
    out
}

impl Exit {
    /// How far `p` is inside the hangar along the opening's axis (negative outside).
    pub fn depth(&self, p: [f64; 3]) -> f64 {
        dot(sub(self.center, p), self.outward)
    }
    /// The track of `p` across the opening, kept `margin` inside its edges.
    pub fn track(&self, p: [f64; 3], margin: f64) -> f64 {
        let half = v8math::max(0.0, self.half[0] - margin);
        v8math::min(half, v8math::max(-half, dot(sub(p, self.center), self.across)))
    }
    /// The point of a track at a depth (negative: outside), at the opening's height.
    pub fn track_point(&self, track: f64, depth: f64) -> [f64; 3] {
        add(add(self.center, scale(self.across, track)), scale(self.outward, -depth))
    }
    /// Whether `p` is near the opening: within its width and height and a
    /// margin, from `reach` outside to anywhere inside.
    pub fn near(&self, p: [f64; 3], reach: f64) -> bool {
        let rel = sub(p, self.center);
        dot(rel, self.across).abs() < self.half[0] + 20.0 && dot(rel, self.up).abs() < self.half[1] + 10.0 && self.depth(p) > -reach
    }
}

impl Routes {
    /// Exits from the authored openings, each with the nearest launch path.
    pub fn build(bounds: &SpaceBounds, pads: &[SpaceVehiclePad], world: &Value) -> Self {
        let mut centroids: Vec<(i32, [f64; 3], f64)> = Vec::new();
        for pad in pads.iter().filter(|p| p.team > 0) {
            match centroids.iter_mut().find(|c| c.0 == pad.team) {
                Some(c) => {
                    c.1 = add(c.1, pad.position);
                    c.2 += 1.0;
                }
                None => centroids.push((pad.team, pad.position, 1.0)),
            }
        }
        for c in &mut centroids {
            c.1 = scale(c.1, 1.0 / c.2);
        }
        let paths = flyer_paths(world);
        let mut routes = Routes::default();
        for field in bounds.forcefields.iter().filter(|f| f.authored_opening) {
            let m = &field.matrix;
            let normal = unit([m[8], m[9], m[10]]);
            let Some(&(team, home, _)) = centroids
                .iter()
                .min_by(|a, b| dist(a.1, field.center).total_cmp(&dist(b.1, field.center)))
            else {
                continue;
            };
            let outward = if dot(sub(field.center, home), normal) >= 0.0 { normal } else { scale(normal, -1.0) };
            let outside = add(field.center, scale(outward, PROVISIONAL_EXIT_OUTSIDE));
            let mut exit = Exit {
                team,
                center: field.center,
                outward,
                inside: sub(field.center, scale(outward, PROVISIONAL_EXIT_INSIDE)),
                outside,
                across: unit([m[0], m[1], m[2]]),
                up: unit([m[4], m[5], m[6]]),
                half: [field.half_extents[0], field.half_extents[1]],
                path: (0, 0),
                paths: [(0, 0); 4],
                path_count: 0,
            };
            let mut near: Vec<(f64, &Vec<[f64; 3]>)> = paths
                .iter()
                .filter(|nodes| dot(sub(nodes[0], field.center), outward) > 0.0)
                .map(|nodes| (dist(nodes[0], outside), nodes))
                .filter(|(d, _)| *d <= PROVISIONAL_LAUNCH_PATH_REACH)
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, nodes) in near.into_iter().take(4) {
                let start = routes.nodes.len() as u32;
                routes.nodes.extend_from_slice(nodes);
                exit.paths[usize::from(exit.path_count)] = (start, routes.nodes.len() as u32);
                exit.path_count += 1;
            }
            exit.path = exit.paths[0];
            routes.exits.push(exit);
        }
        routes
    }

    /// The exit of `team` nearest `point`.
    pub fn nearest_exit(&self, team: i32, point: [f64; 3]) -> Option<usize> {
        self.exits
            .iter()
            .enumerate()
            .filter(|(_, e)| e.team == team)
            .min_by(|a, b| dist(a.1.center, point).total_cmp(&dist(b.1.center, point)))
            .map(|(i, _)| i)
    }
}
