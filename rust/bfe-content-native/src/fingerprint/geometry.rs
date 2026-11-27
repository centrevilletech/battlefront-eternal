//! Binary contributions to a 2005 session. Draw indices, normals and UVs
//! have no part in a unit's clearance, gun tips or an item's pickup sphere.
use super::{digest, parsed};
use bfe_content::{
    actor::build_actor,
    bin_arrays::f32s,
    derived::droideka_guns,
    files::Files,
    three::{bounding_sphere, Positions, Vector3},
    Value,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Geometry {
    pub units: BTreeMap<String, Vec<(String, Value)>>,
    pub item_models: BTreeSet<String>,
}

impl Geometry {
    pub fn item(&mut self, item: &Value) {
        if let Some(name) = item
            .get("GeometryName")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            self.item_models.insert(name.to_string());
        }
    }

    pub fn weapon(&mut self, weapon: &Value) {
        for level in std::iter::once(weapon).chain(
            weapon
                .get("charges")
                .and_then(Value::as_array)
                .unwrap_or(&[]),
        ) {
            if let Some(item) = level.get("item") {
                self.item(item);
            }
        }
    }

    pub fn hashes(
        &self,
        files: &dyn Files,
        sides: &[&str],
    ) -> Result<BTreeMap<String, String>, String> {
        // A session has one hash for every seat. The played side supplies
        // models first; remaining packs fill holes in authored side order.
        let playable: Vec<_> = sides
            .iter()
            .copied()
            .filter(|&side| {
                self.units.get(side).is_some_and(|units| {
                    units
                        .iter()
                        .any(|(_, def)| def.get("base").and_then(Value::as_str) == Some("soldier"))
                })
            })
            .collect();
        let mut packs = BTreeMap::new();
        for &side in sides {
            if let Some(raw) = files.read(&format!("/sides/{side}/weapons.json")) {
                packs.insert(side, parsed(&raw)?);
            }
        }
        let mut spheres = BTreeMap::<&str, BTreeMap<&str, Option<[u64; 4]>>>::new();
        for name in &self.item_models {
            let model = |side: &str| packs.get(side)?.get("models")?.get(name);
            let fallback = sides.iter().copied().find(|&side| model(side).is_some());
            let sources: BTreeSet<_> = playable
                .iter()
                .copied()
                .filter_map(|side| {
                    if model(side).is_some() {
                        Some(side)
                    } else {
                        fallback
                    }
                })
                .collect();
            for side in sources {
                let path = format!("/sides/{side}/weapons.bin");
                if let Some(bytes) = files.read(&path) {
                    let sphere = item_sphere(&bytes, model(side).unwrap())
                        .map_err(|e| format!("{path} {name}: {e}"))?;
                    spheres.entry(side).or_default().insert(name, sphere);
                }
            }
        }
        let mut hashes = BTreeMap::new();
        let mut actor_hashes = BTreeMap::new();
        for &side in sides {
            let dir = format!("/sides/{side}");
            hashes.insert(
                format!("{dir}/weapons.bin"),
                json_hash(&spheres.remove(side).unwrap_or_default())?,
            );
            let Some(units) = self.units.get(side) else {
                continue;
            };
            let mut bodies = BTreeMap::<&str, BTreeSet<String>>::new();
            for (_, def) in units {
                let Some(rel) = def
                    .get("actor")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                let path = format!("{dir}/{}", rel.strip_suffix(".json").unwrap_or(rel));
                actor_hashes
                    .entry(format!("{path}.bin"))
                    .or_insert_with(|| digest(&[]));
                let model = def
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("undefined");
                // A shared model uses the earliest authored actor unless the
                // player's class loads its own actor first.
                let sources = bodies.entry(model).or_default();
                if sources.is_empty() || def.get("base").and_then(Value::as_str) == Some("soldier")
                {
                    sources.insert(path);
                }
            }
            let mut actors = BTreeMap::<&str, Vec<&Value>>::new();
            for (_, def) in units {
                if matches!(
                    def.get("base").and_then(Value::as_str),
                    Some("soldier" | "walkerdroid")
                ) {
                    let model = def
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("undefined");
                    for path in bodies.get(model).into_iter().flatten() {
                        actors.entry(path).or_default().push(def);
                    }
                }
            }
            for (path, users) in actors {
                let binary = format!("{path}.bin");
                if let (Some(meta), Some(bytes)) =
                    (files.read(&format!("{path}.json")), files.read(&binary))
                {
                    let hash = actor_hash(&parsed(&meta)?, &bytes, &users)
                        .map_err(|e| format!("{binary}: {e}"))?;
                    actor_hashes.insert(binary, hash);
                }
            }
        }
        for (path, actor) in actor_hashes {
            // The actor path may point at the side's weapon pack. Both
            // interpretations of the same bytes contribute to that row.
            if let Some(weapon) = hashes.get_mut(&path) {
                *weapon = json_hash(&(&*weapon, actor))?;
            } else {
                hashes.insert(path, actor);
            }
        }
        Ok(hashes)
    }
}

fn json_hash(value: &impl serde::Serialize) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_vec(value).map_err(|e| e.to_string())?,
    ))
}

fn vertices<'a, const STRIDE: usize>(
    bytes: &'a [u8],
    part: &Value,
) -> Result<&'a [[u8; STRIDE]], String> {
    let field = |key| {
        part.get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0. && n.fract() == 0. && *n <= bytes.len() as f64)
            .map(|n| n as usize)
            .ok_or_else(|| format!("invalid mesh {key}"))
    };
    let start = field("vertexOffset")?;
    let len = field("vertexCount")?
        .checked_mul(STRIDE)
        .ok_or("mesh vertex count overflow")?;
    let end = start
        .checked_add(len)
        .ok_or("mesh vertex offset overflow")?;
    let slice = bytes
        .get(start..end)
        .ok_or("mesh vertex range outside file")?;
    Ok(slice.as_chunks::<STRIDE>().0)
}

fn actor_hash(meta: &Value, bytes: &[u8], users: &[&Value]) -> Result<String, String> {
    let parts = meta
        .get("parts")
        .and_then(Value::as_array)
        .ok_or("missing actor parts")?;
    for part in parts {
        vertices::<36>(bytes, part)?;
    }
    bfe_content::map::validate_skeleton_strict(
        meta.get("bones").ok_or("missing actor bones")?,
        "actor bones",
    )?;
    let actor = build_actor(meta, &f32s(bytes).map_err(|e| e.to_string())?)?;
    let radius = bfe_sim::world::admit::bodies::actor_navigation_radius(&actor)
        .map_err(|e| format!("{e:?}"))?;
    let mut guns = Vec::new();
    for def in users
        .iter()
        .filter(|u| u.get("base").and_then(Value::as_str) == Some("walkerdroid"))
    {
        for gun in droideka_guns(&actor, def.get("weapons")) {
            guns.push((
                gun.bone,
                gun.tips.map(|tips| {
                    tips.into_iter()
                        .map(|p| p.to_array().map(f64::to_bits))
                        .collect::<Vec<_>>()
                }),
            ));
        }
    }
    json_hash(&(radius.to_bits(), guns))
}

struct WeaponVertices<'a>(&'a [[u8; 32]]);
impl Positions for WeaponVertices<'_> {
    fn count(&self) -> usize {
        self.0.len()
    }
    fn get(&self, index: usize) -> Vector3 {
        let words = self.0[index].as_chunks::<4>().0;
        Vector3::new(
            f32::from_le_bytes(words[0]) as f64,
            f32::from_le_bytes(words[1]) as f64,
            f32::from_le_bytes(words[2]) as f64,
        )
    }
}

// At equal radii item_radius keeps the earlier part. An empty model has no pickup
// centre. Encode exact floating-point values, including the centre's sign.
fn item_sphere(bytes: &[u8], parts: &Value) -> Result<Option<[u64; 4]>, String> {
    let mut best = None;
    for part in parts
        .as_array()
        .ok_or("weapon model parts must be an array")?
    {
        let (center, radius) = bounding_sphere(&WeaponVertices(vertices(bytes, part)?));
        if best.is_none_or(|(_, previous)| radius > previous) {
            best = Some((center, radius));
        }
    }
    Ok(best.map(|(p, r)| [p.x, p.y, p.z, r].map(f64::to_bits)))
}
