//! A 2005 character's cloth pieces: the prepared rest surface skinned to its fixed points (drawn
//! when a piece cannot be simulated) and what `cloth` needs to move it.
use super::{
    assets::{image, Part},
    cloth,
};
use bfe_content::{battle::SideFiles, Value as Source};
use bfe_content_native::{
    animation2::Rig,
    geometry::{Attribute, Geometry, Group, Scalar},
    loader::{Blend, Material, MaterialKind},
    textures::Library,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use three::{Matrix4, Vector3};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Collision {
    parent: String,
    #[serde(rename = "type")]
    kind: u32,
    size: [f32; 3],
    rotation: [f64; 9],
    position: [f64; 3],
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Piece {
    name: String,
    parent: String,
    transform: [f64; 12],
    texture: String,
    positions: Vec<[f64; 3]>,
    uvs: Vec<[f64; 2]>,
    fixed_points: Vec<usize>,
    fixed_weights: Vec<String>,
    triangles: Vec<[usize; 3]>,
    #[serde(default)]
    stretch_constraints: Vec<[usize; 2]>,
    #[serde(default)]
    bend_constraints: Vec<[usize; 2]>,
    #[serde(default)]
    cross_constraints: Vec<[usize; 2]>,
    #[serde(default)]
    collision: Vec<Collision>,
}
/// The rest shape in the model's bind pose.
fn bind_points(piece: &Piece, rig: &Rig) -> Result<(usize, Vec<Vector3>), String> {
    let parent = rig
        .joint(&piece.parent)
        .filter(|j| *j < rig.mesh_joints)
        .ok_or("missing cloth attachment")?;
    if piece.positions.len() != piece.uvs.len()
        || piece.fixed_points.len() != piece.fixed_weights.len()
    {
        return Err("cloth attribute counts differ".into());
    }
    let mut bind = rig.joints[parent].inverse;
    bind.invert().multiply(&placement(piece));
    let points = piece
        .positions
        .iter()
        .map(|p| *Vector3::new(p[0], p[1], p[2]).apply_matrix4(&bind))
        .collect();
    Ok((parent, points))
}
fn placement(piece: &Piece) -> Matrix4 {
    let t = piece.transform;
    Matrix4 {
        elements: [
            t[0], t[1], t[2], 0., t[3], t[4], t[5], 0., t[6], t[7], t[8], 0., t[9], t[10], t[11],
            1.,
        ],
    }
}
fn geometry(piece: &Piece, rig: &Rig) -> Result<Geometry, String> {
    let (parent, points) = bind_points(piece, rig)?;
    let pins = piece
        .fixed_points
        .iter()
        .zip(&piece.fixed_weights)
        .map(|(&i, name)| {
            let j = rig
                .joint(name)
                .filter(|j| *j < rig.mesh_joints)
                .ok_or("missing cloth pin joint")?;
            Ok((*points.get(i).ok_or("invalid cloth pin")?, j as u16))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut normals = vec![Vector3::default(); points.len()];
    let mut indices = Vec::with_capacity(piece.triangles.len() * 3);
    for &tri in &piece.triangles {
        if tri.iter().any(|i| *i >= points.len()) {
            return Err("invalid cloth index".into());
        }
        let [a, b, c] = tri.map(|i| points[i]);
        let mut ab = b;
        ab.sub(&a);
        let mut ac = c;
        ac.sub(&a);
        let mut normal = Vector3::default();
        normal.cross_vectors(&ab, &ac);
        for i in tri {
            indices.push(i as u32);
            normals[i].add(&normal);
        }
    }
    let mut buffers = vec![Vec::new(); 5];
    for ((p, mut normal), uv) in points.iter().zip(normals).zip(&piece.uvs) {
        normal.normalize();
        for (i, v) in [p, &normal].into_iter().enumerate() {
            for c in [v.x, v.y, v.z] {
                buffers[i].extend_from_slice(&(c as f32).to_le_bytes());
            }
        }
        for &c in uv {
            buffers[2].extend_from_slice(&(c as f32).to_le_bytes());
        }
        let mut nearest: Vec<_> = pins
            .iter()
            .map(|(v, j)| (v.distance_to_squared(p), *j))
            .collect();
        nearest.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut joints = [0u16; 4];
        let mut weights = [0f64; 4];
        if nearest.is_empty() {
            joints[0] = parent as u16;
            weights[0] = 1.;
        } else if nearest[0].0 <= 1e-12 {
            joints[0] = nearest[0].1;
            weights[0] = 1.;
        } else {
            for (i, (dist, j)) in nearest.iter().take(3).enumerate() {
                joints[i] = *j;
                weights[i] = 1. / dist;
            }
        }
        let sum = weights.iter().sum::<f64>();
        for (j, w) in joints.into_iter().zip(weights) {
            buffers[3].extend_from_slice(&j.to_le_bytes());
            buffers[4].extend_from_slice(&((w / sum) as f32).to_le_bytes());
        }
    }
    let attributes = [
        ("position", 3, Scalar::Float),
        ("normal", 3, Scalar::Float),
        ("uv", 2, Scalar::Float),
        ("skinIndex", 4, Scalar::U16),
        ("skinWeight", 4, Scalar::Float),
    ]
    .into_iter()
    .enumerate()
    .map(|(buffer, (name, size, scalar))| Attribute {
        name: name.into(),
        buffer,
        size,
        scalar,
        normalized: false,
        stride: size as usize * scalar.bytes(),
        offset: 0,
    })
    .collect();
    let g = Geometry {
        buffers,
        attributes,
        vertices: points.len(),
        groups: vec![Group {
            first: 0,
            count: indices.len(),
            material: 0,
        }],
        indices,
    };
    g.validate()?;
    Ok(g)
}
/// A piece's cloth class settings, read the way the game reads them: the class and its parents'
/// ordered properties, the last assignment winning, a value that does not parse leaving the
/// previous one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    params: cloth::Params,
    /// Which pair lists run: cross, bend, stretch.
    lists: [bool; 3],
    /// HardEdge: drawn with an alpha test. Transparent: drawn blended. Neither: opaque.
    hard_edge: bool,
    transparent: bool,
}
fn class_settings(classes: Option<&serde_json::Value>, name: &str) -> Settings {
    let mut params = cloth::Params::default();
    let mut lists = [true; 3];
    let (mut hard_edge, mut transparent) = (false, false);
    let settings = |params, lists, hard_edge, transparent| Settings { params, lists, hard_edge, transparent };
    let Some(entc) = classes.and_then(|c| c.get("entc")) else {
        return settings(params, lists, hard_edge, transparent);
    };
    let find = |key: &str| {
        entc.get(key.to_ascii_lowercase()).or_else(|| {
            entc.as_object()?
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v)
        })
    };
    let mut chain = Vec::new();
    let mut at = find(name);
    while let Some(class) = at.filter(|_| chain.len() < 16) {
        chain.push(class);
        at = class["base"].as_str().and_then(find).filter(|c| !chain.iter().any(|seen| std::ptr::eq(*seen, *c)));
    }
    for class in chain.into_iter().rev() {
        for row in class["properties"].as_array().into_iter().flatten() {
            let (Some(key), Some(value)) = (row[0].as_str(), row[1].as_str()) else {
                continue;
            };
            let number = || leading_float(value);
            let switch = |bit: &mut bool, any_nonzero: bool| {
                if let Some(v) = leading_int(value) {
                    let v = v.min(1);
                    *bit = if any_nonzero { v != 0 } else { v & 1 != 0 };
                }
            };
            match key.to_ascii_lowercase().as_str() {
                "windspeed" => params.wind_speed = number().unwrap_or(params.wind_speed),
                "dampening" => params.dampening = number().unwrap_or(params.dampening),
                "drag" => params.drag = number().unwrap_or(params.drag),
                "particlemass" => params.mass = number().unwrap_or(params.mass),
                "maxacceleration" => {
                    params.max_acceleration = number().unwrap_or(params.max_acceleration)
                }
                "winddirection" => {
                    // Two angles in degrees: one from straight up, then one around it.
                    let mut angles = value.split_whitespace().map(leading_float);
                    let a = angles.next().flatten().unwrap_or(0.).to_radians();
                    let b = angles.next().flatten().unwrap_or(0.).to_radians();
                    params.wind_direction = [-a.sin() * b.cos(), a.cos(), -a.sin() * b.sin()];
                }
                "crossconstraint" => switch(&mut lists[0], true),
                "bendconstraint" => switch(&mut lists[1], false),
                "stretchconstraint" => switch(&mut lists[2], false),
                "hardedge" => switch(&mut hard_edge, false),
                "transparent" => switch(&mut transparent, false),
                _ => {}
            }
        }
    }
    settings(params, lists, hard_edge, transparent)
}
/// The number at the start of `text`, as C's `%f` reads it.
fn leading_float(text: &str) -> Option<f32> {
    let text = text.trim_start();
    let end = text
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E') || (matches!(c, '-' | '+') && (*i == 0 || text[..*i].ends_with(['e', 'E']))))
        .map(|(i, c)| i + c.len_utf8())
        .last()?;
    (0..=end).rev().find_map(|n| text.get(..n)?.parse().ok())
}
/// The integer at the start of `text`, as C's `%d` reads it.
fn leading_int(text: &str) -> Option<i32> {
    let text = text.trim_start();
    let end = text
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || (matches!(c, '-' | '+') && *i == 0))
        .map(|(i, c)| i + c.len_utf8())
        .last()?;
    text[..end].parse().ok()
}
/// The pairs of one pass in the game's order (cross, bend, stretch), the switched-off lists
/// left out, each with its rest length from the raw shape.
fn links(piece: &Piece, lists: [bool; 3]) -> Result<Vec<cloth::Link>, String> {
    let n = piece.positions.len();
    let mut links = Vec::new();
    for (on, pairs) in lists.into_iter().zip([
        &piece.cross_constraints,
        &piece.bend_constraints,
        &piece.stretch_constraints,
    ]) {
        for &[a, b] in pairs.iter().filter(|_| on) {
            if a >= n || b >= n {
                return Err("invalid cloth pair".into());
            }
            let d = [0, 1, 2].map(|c| piece.positions[b][c] - piece.positions[a][c]);
            links.push(cloth::Link {
                a: a as u16,
                b: b as u16,
                rest: (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32,
            });
        }
    }
    Ok(links)
}
/// The collision shapes on known joints; a type the game does not test is left out.
fn shapes(piece: &Piece, joint: impl Fn(&str) -> Option<usize>) -> Vec<cloth::Shape> {
    piece
        .collision
        .iter()
        .filter_map(|c| {
            let kind = match c.kind {
                0 => cloth::Kind::Sphere,
                1 => cloth::Kind::Cylinder,
                2 => cloth::Kind::Box,
                _ => return None,
            };
            let (r, p) = (c.rotation, c.position);
            Some(cloth::Shape {
                joint: joint(&c.parent)?,
                kind,
                size: c.size,
                local: Matrix4 {
                    elements: [
                        r[0], r[1], r[2], 0., r[3], r[4], r[5], 0., r[6], r[7], r[8], 0., p[0],
                        p[1], p[2], 1.,
                    ],
                },
            })
        })
        .collect()
}
fn simulated(piece: &Piece, rig: &Rig, settings: Settings) -> Result<cloth::Piece, String> {
    let (parent, points) = bind_points(piece, rig)?;
    let n = piece.positions.len();
    if n > usize::from(u16::MAX) {
        return Err("cloth piece too large".into());
    }
    let fixed = piece.fixed_points.len().min(n);
    let mut joints = Vec::new();
    let mut pins = Vec::with_capacity(fixed);
    for (i, name) in piece.fixed_weights.iter().enumerate().take(fixed) {
        let joint = rig.joint(name).ok_or("missing cloth pin joint")?;
        let k = joints.iter().position(|&j| j == joint).unwrap_or_else(|| {
            joints.push(joint);
            joints.len() - 1
        });
        let mut local = points[i];
        local.apply_matrix4(&rig.joints[joint].inverse);
        pins.push((k, [local.x as f32, local.y as f32, local.z as f32]));
    }
    let rest: Vec<cloth::V3> = piece.positions.iter().map(|p| p.map(|v| v as f32)).collect();
    // The restart table: each vertex on its three nearest fixed points' joints by inverse
    // squared distance in the bind pose (a fixed point on its own joint alone).
    let mut start = Vec::with_capacity(if pins.is_empty() { 0 } else { n });
    for p in points.iter().filter(|_| !pins.is_empty()) {
        let mut nearest: Vec<(f64, usize)> = (0..pins.len())
            .map(|i| (points[i].distance_to_squared(p), pins[i].0))
            .collect();
        nearest.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let taken: &[(f64, usize)] = if nearest[0].0 <= 1e-12 { &nearest[..1] } else { &nearest[..nearest.len().min(3)] };
        let total: f64 = taken.iter().map(|(d, _)| 1. / d.max(1e-12)).sum();
        let mut row = [(0, [0f32; 3], 0f32); 3];
        for (slot, &(d, k)) in row.iter_mut().zip(taken) {
            let mut local = *p;
            local.apply_matrix4(&rig.joints[joints[k]].inverse);
            let w = if taken.len() == 1 { 1. } else { (1. / d.max(1e-12)) / total };
            *slot = (k, [local.x as f32, local.y as f32, local.z as f32], w as f32);
        }
        start.push(row);
    }
    let Settings { params, lists, .. } = settings;
    let links = links(piece, lists)?;
    let shapes = shapes(piece, |name| rig.joint(name));
    let mut triangles = Vec::with_capacity(piece.triangles.len());
    for tri in &piece.triangles {
        if tri.iter().any(|&i| i >= n) {
            return Err("invalid cloth index".into());
        }
        triangles.push(tri.map(|i| i as u16));
    }
    Ok(cloth::Piece {
        parent,
        placement: placement(piece),
        uvs: piece.uvs.iter().map(|uv| uv.map(|v| v as f32)).collect(),
        rest,
        fixed,
        pins,
        joints,
        links,
        shapes,
        triangles,
        params,
        start,
    })
}
/// The cloth material, the same for the skinned rest surface and the moving surface: lit and
/// two-sided; opaque unless the class asks for a hard edge (alpha test) or transparency (blended).
pub(super) fn material(texture: Option<usize>, hard_edge: bool, transparent: bool) -> Material {
    Material {
        extended: None,
        kind: MaterialKind::Dynamic,
        blend: if transparent { Blend::Alpha } else { Blend::Opaque },
        cutout: hard_edge,
        double_sided: true,
        flags: 3,
        far: false,
        texture,
        bump: None,
        record: Default::default(),
    }
}
pub(super) fn admit(
    side: &SideFiles,
    unit: &Source,
    rig: &Rig,
    library: &mut Library,
    parts: &mut Vec<Part>,
    pieces: &mut Vec<cloth::Piece>,
) -> Result<(), String> {
    let Some(names) = unit
        .get("cloth")
        .and_then(Source::as_array)
        .filter(|v| !v.is_empty())
    else {
        return Ok(());
    };
    let dir = side.dir.trim_start_matches('/');
    let all: BTreeMap<String, Vec<Piece>> =
        serde_json::from_slice(&library.read(&format!("{dir}/cloth.json"))?)
            .map_err(|e| e.to_string())?;
    let classes: Option<serde_json::Value> = library
        .read(&format!("{dir}/classes.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    for piece in all.values().flatten().filter(|p| {
        names.iter().any(|n| n.as_str() == Some(&p.name))
            && !p.name.to_ascii_lowercase().contains("low1")
    }) {
        let geometry = geometry(piece, rig)?;
        let texture = image(side, &piece.texture, library)?;
        let settings = class_settings(classes.as_ref(), &piece.name);
        parts.push(Part {
            geometry,
            material: material(texture, settings.hard_edge, settings.transparent),
        });
        pieces.push(simulated(piece, rig, settings)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn class_settings_follow_the_parent_chain_last_assignment_and_c_number_reading() {
        let classes = json!({"entc": {
            "trim_base": {"name": "Trim_Base", "base": "cloth", "properties": [["Drag", "-1.5"], ["WindSpeed", "4"]]},
            "trim_a": {"name": "Trim_A", "base": "Trim_Base", "properties": [
                ["WindSpeed", "2.5 extra"], ["Dampening", "soft"], ["ParticleMass", "3"], ["WindSpeed", "1.25"],
                ["BendConstraint", "0"], ["StretchConstraint", "7"], ["CrossConstraint", "-2"], ["MaxAcceleration", "9"],
                ["HardEdge", "1"], ["Transparent", "1"], ["HardEdge", "0"]]},
        }});
        let Settings { params, lists, hard_edge, transparent } = class_settings(Some(&classes), "Trim_A");
        assert_eq!(params.wind_speed, 1.25);
        assert_eq!(params.drag, -1.5);
        assert_eq!(params.dampening, 0.5, "an unreadable value keeps the previous one");
        assert_eq!(params.mass, 3.);
        assert_eq!(params.max_acceleration, 9.);
        assert_eq!(lists, [true, false, true]);
        assert_eq!((hard_edge, transparent), (false, true));
        let missing = class_settings(Some(&classes), "trim_missing");
        assert_eq!((missing.params, missing.lists), (cloth::Params::default(), [true; 3]));
        assert!(!missing.hard_edge && !missing.transparent);
    }
    #[test]
    fn the_class_switches_choose_opaque_alpha_tested_or_blended_cloth() {
        let plain = material(None, false, false);
        assert!(plain.blend == Blend::Opaque && !plain.cutout && plain.double_sided && plain.flags & 1 != 0);
        assert!(material(None, true, false).cutout);
        assert!(material(None, false, true).blend == Blend::Alpha);
    }
    #[test]
    fn pairs_run_cross_then_bend_then_stretch_with_rest_lengths_from_the_raw_shape() {
        let piece: Piece = serde_json::from_value(json!({
            "name": "flap", "parent": "j0", "transform": [1,0,0, 0,1,0, 0,0,1, 0,0,0], "texture": "",
            "positions": [[0,0,0],[0,-1,0],[3,-1,0],[0,-1,4]], "uvs": [[0,0],[0,1],[1,1],[1,0]],
            "fixedPoints": [], "fixedWeights": [], "triangles": [[0,1,2],[1,2,3]],
            "stretchConstraints": [[0,1]], "bendConstraints": [[1,2]], "crossConstraints": [[1,3]],
            "collision": [{"parent": "j0", "type": 2, "size": [1,2,3], "rotation": [1,0,0,0,1,0,0,0,1], "position": [0,1,0]},
                          {"parent": "j0", "type": 9, "size": [1,1,1], "rotation": [1,0,0,0,1,0,0,0,1], "position": [0,0,0]},
                          {"parent": "nowhere", "type": 0, "size": [1,1,1], "rotation": [1,0,0,0,1,0,0,0,1], "position": [0,0,0]}],
        }))
        .unwrap();
        let order: Vec<_> = links(&piece, [true, false, true]).unwrap().iter().map(|l| (l.a, l.b, l.rest)).collect();
        assert_eq!(order, vec![(1, 3, 4.), (0, 1, 1.)]);
        let all: Vec<_> = links(&piece, [true; 3]).unwrap().iter().map(|l| (l.a, l.b)).collect();
        assert_eq!(all, vec![(1, 3), (1, 2), (0, 1)]);
        let found = shapes(&piece, |name| (name == "j0").then_some(4));
        assert_eq!(found.len(), 1, "unknown types and joints are left out");
        assert_eq!((found[0].joint, found[0].kind, found[0].size), (4, cloth::Kind::Box, [1., 2., 3.]));
        assert_eq!(found[0].local.elements[13], 1.);
    }
}
