//! Prepared character parts enter the same geometry and material banks as other battle objects.
use bfe_content::{battle::SideFiles, Value as Source};
use bfe_content_native::{
    animation2::{self, Class},
    geometry::{Geometry, Group},
    loader::{authored_json, Material, MaterialKind},
    BattleContent,
};
use serde_json::Value;
use three::Matrix4;

pub struct Part {
    pub geometry: Geometry,
    pub material: Material,
}
pub struct Edge {
    pub socket: Matrix4,
    pub color: [u8; 4],
    pub trail: Option<[u8; 4]>,
    pub length: f64,
    pub width: f64,
    pub texture: Option<usize>,
}
pub struct Mount {
    pub joint: usize,
    pub parts: Vec<Part>,
    pub edges: Vec<Edge>,
}
pub struct Loadout {
    pub registry: usize,
    pub mounts: Vec<Mount>,
}
pub struct Model {
    pub class: Class,
    pub registry: usize,
    pub bone_names: Vec<String>,
    pub geometry: Geometry,
    pub materials: Vec<(Group, Material)>,
    /// Cloth pieces: the rest surface drawn skinned, and beside each what moves it.
    pub parts: Vec<Part>,
    pub(super) cloth: Vec<super::cloth::Piece>,
    pub gear: Vec<Loadout>,
}
pub struct Prepared {
    pub light: crate::online::unit_light::UnitLight,
    pub models: Vec<Model>,
    pub chairs: Vec<super::rider::ChairSeat>,
}
// Repeated weapon properties follow blade order. A single property is a
// weapon-wide default; a missing per-blade override leaves other blades intact.
fn blade_property<'a>(weapon: &'a Value, key: &str, index: usize) -> Option<&'a str> {
    let mut values = weapon["properties"].as_array()?.iter().filter_map(|row| {
        (row.get(0)?.as_str()?.eq_ignore_ascii_case(key))
            .then(|| row.get(1)?.as_str())
            .flatten()
    });
    let first = values.next()?;
    if index == 0 {
        return Some(first);
    }
    if values.clone().next().is_none() {
        Some(first)
    } else {
        values.nth(index - 1)
    }
}
fn dimension(
    blade: &Value,
    key: &str,
    property: Option<&str>,
    default: f64,
) -> Result<f64, String> {
    let value = match blade.get(key).filter(|v| !v.is_null()) {
        Some(v) => v.as_f64(),
        None => property.map_or(Some(default), |v| v.parse().ok()),
    };
    value
        .filter(|n| n.is_finite() && *n > 0.)
        .ok_or_else(|| format!("invalid blade {key}"))
}
fn blade_color(blade: &Value, key: &str) -> Result<[u8; 4], String> {
    let channels = blade[key].as_array().filter(|v| v.len() == 4)
        .ok_or_else(|| format!("invalid {key}"))?;
    let mut result = [0; 4];
    for (dst, channel) in result.iter_mut().zip(channels) {
        let value = channel.as_f64()
            .filter(|n| (0.0..=255.0).contains(n) && n.fract() == 0.)
            .ok_or_else(|| format!("invalid {key} channel"))?;
        *dst = value as u8;
    }
    Ok(result)
}

fn trail_color(blade: &Value, fallback: Option<&str>) -> Result<Option<[u8; 4]>, String> {
    if blade.get("trailColor").is_some_and(|v| !v.is_null()) {
        return blade_color(blade, "trailColor").map(Some);
    }
    fallback
        .map(|s| {
            let mut values = s.split_whitespace();
            let mut rgba = [0; 4];
            for c in &mut rgba {
                *c = values
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("invalid trail color")?;
            }
            if values.next().is_some() {
                return Err("invalid trail color".into());
            }
            Ok(rgba)
        })
        .transpose()
}

pub(super) fn image(
    s: &SideFiles,
    name: &str,
    library: &mut bfe_content_native::textures::Library,
) -> Result<Option<usize>, String> {
    if name.is_empty() {
        return Ok(None);
    }
    let path = s
        .units_file
        .get("textures")
        .and_then(Source::as_object)
        .and_then(|o| {
            o.get(name).or_else(|| {
                o.entries()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|p| p.1)
            })
        })
        .and_then(Source::as_str);
    path.map(|p| library.load(&format!("{}/{}", s.dir.trim_start_matches('/'), p)))
        .transpose()
}
fn material(rec: Value, diffuse: Option<usize>, bump: Option<usize>) -> Material {
    let flags = rec["material"]["flags"]
        .as_f64()
        .or_else(|| rec["flags"].as_f64())
        .unwrap_or(1.) as u32;
    bfe_content_native::loader::material_swbf2(rec, MaterialKind::Dynamic, flags, diffuse, bump)
}
fn attributed(g: &mut Geometry, def: &Source, bytes: &[u8], bones: usize) -> Result<(), String> {
    let attrs = |name: &str| {
        g.attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.buffer)
            .ok_or("missing skin attribute")
    };
    let (joints, weights) = (attrs("skinIndex")?, attrs("skinWeight")?);
    let mut vertex = 0;
    for part in def.get("parts").and_then(Source::as_array).unwrap_or(&[]) {
        let n = part
            .get("vertexCount")
            .and_then(Source::as_f64)
            .ok_or("missing vertex count")? as usize;
        if n > g.vertices.saturating_sub(vertex) {
            return Err("character stream exceeds geometry".into());
        }
        if let Some(stream) = part.get("vertexStreams") {
            let count = part
                .get("influencesPerVertex")
                .and_then(Source::as_f64)
                .unwrap_or(1.);
            if ![1., 2., 3., 4.].contains(&count) {
                return Err("invalid skin influence count".into());
            }
            let count = count as usize;
            let extent = n.checked_mul(count * 4).ok_or("skin stream too long")?;
            let payload = |key: &str| -> Result<&[u8], String> {
                let offset = stream
                    .get(key)
                    .and_then(Source::as_f64)
                    .filter(|x| x.is_finite() && *x >= 0. && x.fract() == 0.)
                    .ok_or("missing skin stream offset")? as usize;
                bytes
                    .get(offset..offset.checked_add(extent).ok_or("skin offset overflow")?)
                    .ok_or_else(|| "truncated skin stream".into())
            };
            let js = payload("joints")?;
            let ws = (count != 1).then(|| payload("weights")).transpose()?;
            for v in 0..n {
                let mut sum = 0.;
                for i in 0..4 {
                    let at = (v * count + i) * 4;
                    let (joint, weight) = if i < count {
                        (
                            u32::from_le_bytes(js[at..at + 4].try_into().unwrap()),
                            ws.map_or(1., |w| {
                                f32::from_le_bytes(w[at..at + 4].try_into().unwrap())
                            }),
                        )
                    } else {
                        (0, 0.)
                    };
                    if joint as usize >= bones
                        || joint > u16::MAX as u32
                        || !(0.0..=1.00001).contains(&weight)
                    {
                        return Err("invalid character skin influence".into());
                    }
                    let slot = (vertex + v) * 4 + i;
                    g.buffers[joints][2 * slot..2 * slot + 2]
                        .copy_from_slice(&(joint as u16).to_le_bytes());
                    g.buffers[weights][4 * slot..4 * slot + 4]
                        .copy_from_slice(&weight.to_le_bytes());
                    sum += weight;
                }
                if (sum - 1.).abs() > 1e-4 {
                    return Err("unnormalized character skin weights".into());
                }
            }
        }
        vertex += n;
    }
    Ok(())
}
fn rigid(
    s: &SideFiles,
    name: &str,
    library: &mut bfe_content_native::textures::Library,
) -> Result<Vec<Part>, String> {
    let records = s
        .weapons
        .get("models")
        .and_then(|v| v.get(name))
        .and_then(Source::as_array)
        .unwrap_or(&[]);
    records
        .iter()
        .map(|source| {
            let mut rec = authored_json(source);
            for key in ["vertexOffset", "vertexCount", "indexOffset", "indexCount"] {
                let n = rec[key]
                    .as_f64()
                    .filter(|n| {
                        n.is_finite() && *n >= 0. && n.fract() == 0. && *n <= u32::MAX as f64
                    })
                    .ok_or_else(|| format!("invalid held model {key}"))?;
                rec[key] = Value::from(n as u32);
            }
            let geometry = Geometry::model(&rec, &s.weapon_buf, &s.weapon_index_buf)
                .map_err(|e| format!("{name}: {e}"))?;
            let diffuse = image(s, rec["texture"].as_str().unwrap_or(""), library)?;
            let bump = image(s, rec["bump"].as_str().unwrap_or(""), library)?;
            Ok(Part {
                geometry,
                material: material(rec, diffuse, bump),
            })
        })
        .collect()
}
pub fn prepare(c: &mut BattleContent) -> Result<Option<Prepared>, String> {
    if c.content.battle.swbf2.is_none() {
        return Ok(None);
    }
    let classes = animation2::admit(c)?;
    let defs = c.defs()?;
    let mut models = Vec::new();
    for class in classes {
        let side = c
            .content
            .battle
            .sides
            .iter()
            .find(|s| s.name == class.side)
            .ok_or("missing soldier side")?;
        let source = side
            .units
            .get(&class.name)
            .ok_or("missing soldier definition")?;
        let registry = defs
            .registry
            .classes
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&class.name))
            .map_or(0, |n| n + 1);
        let (_, body) = c
            .content
            .battle
            .bodies
            .iter()
            .find(|(n, _)| n == &class.model)
            .ok_or("missing soldier mesh")?;
        let mesh = c
            .actors
            .iter()
            .find(|m| m.name == class.model)
            .ok_or("missing character geometry")?;
        let mut geometry = mesh.geometry.clone();
        if let Some(file) = body.def.get("attributesFile").and_then(Source::as_str) {
            let bytes = c
                .textures
                .read(&format!("sides/{}/actors/{file}", side.name))?;
            attributed(
                &mut geometry,
                &body.def,
                &bytes,
                class.animation.rig.mesh_joints,
            )?;
        }
        let skin = c
            .skins
            .iter()
            .find(|(n, _)| *n == format!("{}/{}", class.side, class.name))
            .map_or(&mesh.textures, |v| &v.1);
        let mut materials = Vec::new();
        for (rec, group) in body
            .def
            .get("parts")
            .and_then(Source::as_array)
            .unwrap_or(&[])
            .iter()
            .zip(&geometry.groups)
        {
            let rec = authored_json(rec);
            let diffuse = skin.get(group.material).copied().flatten();
            let bump = image(side, rec["bump"].as_str().unwrap_or(""), &mut c.textures)?;
            materials.push((*group, material(rec, diffuse, bump)));
        }
        let mut parts = Vec::new();
        let mut cloth = Vec::new();
        super::drape::admit(
            side,
            source,
            &class.animation.rig,
            &mut c.textures,
            &mut parts,
            &mut cloth,
        )?;
        let mut gear = Vec::new();
        for w in source
            .get("weapons")
            .and_then(Source::as_array)
            .unwrap_or(&[])
        {
            let raw = authored_json(w);
            let mut blade_number = 0;
            let mut mounts = Vec::new();
            if let Some(hilts) = raw["saberHilts"].as_array().filter(|h| !h.is_empty()) {
                for h in hilts {
                    let a = &h["attachment"];
                    let name = a["name"]
                        .as_str()
                        .or_else(|| a["bone"].as_str())
                        .unwrap_or("");
                    let joint = class
                        .animation
                        .rig
                        .joint(name)
                        .or_else(|| {
                            a["boneCrc"]
                                .as_str()
                                .and_then(|s| u32::from_str_radix(s, 16).ok())
                                .and_then(|h| {
                                    class.animation.rig.joints.iter().position(|j| j.crc == h)
                                })
                        })
                        .ok_or_else(|| format!("{}: missing hilt attachment {name}", class.name))?;
                    let parts = rigid(
                        side,
                        h["model"].as_str().ok_or("missing hilt model")?,
                        &mut c.textures,
                    )?;
                    let mut edges = Vec::new();
                    for b in h["blades"].as_array().ok_or("missing hilt edges")? {
                        edges.push(Edge {
                            socket: Matrix4 {
                                elements: serde_json::from_value(b["transform"].clone())
                                    .map_err(|e| e.to_string())?,
                            },
                            color: blade_color(b, "color")?,
                            trail: trail_color(
                                b,
                                blade_property(&raw, "LightSaberTrailColor", blade_number),
                            )?,
                            length: dimension(
                                b,
                                "length",
                                blade_property(&raw, "LightSaberLength", blade_number),
                                1.,
                            )?,
                            width: dimension(
                                b,
                                "width",
                                blade_property(&raw, "LightSaberWidth", blade_number),
                                0.06,
                            )?,

                            texture: image(
                                side,
                                b["texture"].as_str().unwrap_or(""),
                                &mut c.textures,
                            )?,
                        });
                        blade_number += 1;
                    }
                    mounts.push(Mount {
                        joint,
                        parts,
                        edges,
                    });
                }
            } else if let Some(joint) = body
                .def
                .get("attach")
                .and_then(Source::as_str)
                .and_then(|n| class.animation.rig.joint(n))
            {
                let model = raw["GeometryName"]
                    .as_str()
                    .or_else(|| {
                        raw["properties"]
                            .as_array()?
                            .iter()
                            .find(|p| p[0] == "GeometryName")?[1]
                            .as_str()
                    })
                    .unwrap_or("");
                let parts = rigid(side, model, &mut c.textures)?;
                if !parts.is_empty() {
                    mounts.push(Mount {
                        joint,
                        parts,
                        edges: Vec::new(),
                    });
                }
            }
            let name = raw["class"]
                .as_str()
                .or(raw["weapon"].as_str())
                .unwrap_or("");
            let registry = defs
                .registry
                .weapons
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))
                .map_or(0, |i| i + 1);
            gear.push(Loadout { registry, mounts });
        }
        let bone_names = body
            .def
            .get("bones")
            .and_then(Source::as_array)
            .unwrap_or(&[])
            .iter()
            .map(|b| {
                b.get("name")
                    .and_then(Source::as_str)
                    .unwrap_or("")
                    .to_ascii_lowercase()
            })
            .collect();
        models.push(Model {
            geometry,
            materials,
            class,
            registry,
            bone_names,
            parts,
            cloth,
            gear,
        });
    }
    let world = &c.content.battle.world;
    let lumi = world
        .get("lumi")
        .and_then(|v| v.get("file"))
        .and_then(Source::as_str)
        .map(|file| {
            c.textures
                .read(&format!("maps/{}/{file}", c.content.battle.map))
        })
        .transpose()?;
    let light = crate::online::unit_light::UnitLight::new(world, lumi);
    let chairs = super::rider::chair_seats(c);
    Ok(Some(Prepared { models, light, chairs }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content_native::geometry::{Attribute, Scalar};

    #[test]
    fn all_weighted_influences_reach_the_gpu_attributes() {
        let attrs = [("skinIndex", Scalar::U16), ("skinWeight", Scalar::Float)];
        let mut g = Geometry {
            buffers: vec![vec![0; 8], vec![0; 16]],
            attributes: attrs
                .into_iter()
                .enumerate()
                .map(|(buffer, (name, scalar))| Attribute {
                    name: name.into(),
                    buffer,
                    size: 4,
                    scalar,
                    normalized: false,
                    stride: scalar.bytes() * 4,
                    offset: 0,
                })
                .collect(),
            vertices: 1,
            indices: vec![],
            groups: vec![],
        };
        let source=bfe_content::json::parse(r#"{"parts":[{"vertexCount":1,"influencesPerVertex":4,"vertexStreams":{"joints":0,"weights":16}}]}"#).unwrap();
        let weights = [0.125f32, 0.25, 0.375, 0.25];
        let bytes: Vec<_> = [5u32, 1, 2, 3]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .chain(weights.into_iter().flat_map(f32::to_le_bytes))
            .collect();
        attributed(&mut g, &source, &bytes, 6).unwrap();
        assert_eq!(
            g.buffers[0],
            [5u16, 1, 2, 3]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(g.buffers[1], bytes[16..]);
        assert!(attributed(&mut g, &source, &bytes, 5).is_err());
        assert!(attributed(&mut g, &source, &bytes[..31], 6).is_err());
    }
}

#[cfg(test)]
mod blade_defaults {
    use super::*;
    use serde_json::json;
    #[test]
    fn optional_blade_properties_and_weapon_defaults() {
        let w = json!({"properties":[["LightSaberLength","0.8"],["LightSaberWidth","0.04"],["LightSaberWidth","0.07"]]});
        let b = json!({});
        assert_eq!(
            dimension(&b, "length", blade_property(&w, "LightSaberLength", 1), 1.).unwrap(),
            0.8
        );
        assert_eq!(
            dimension(&b, "width", blade_property(&w, "LightSaberWidth", 1), 0.06).unwrap(),
            0.07
        );
        assert_eq!(trail_color(&b, None).unwrap(), None);
        assert!(trail_color(&json!({"trailColor":[0, 0, 0, 1.5]}), None).is_err());
        assert_eq!(
            trail_color(&json!({"trailColor":[3.0,5.0,7.0,9.0]}), None).unwrap(),
            Some([3, 5, 7, 9])
        );
        assert_eq!(
            trail_color(&b, Some("3 5 7 9")).unwrap(),
            Some([3, 5, 7, 9])
        );
        assert!(dimension(&json!({"width":-1}), "width", None, 0.06).is_err());
    }
}
