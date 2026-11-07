//! Foliage.buildGrass/buildLeaves and cold patch placement. Seeded leaves use
//! the engine generator. Grass and tint take an explicit host random stream;
//! replay may supply the recorded stream instead of inventing world facts.
use serde_json::Value;
use three::{Matrix4, Vector3};
pub struct Class {
    pub name: String,
    pub grass: bool,
    pub definition: Value,
    pub texture: String,
    pub cards: Vec<f64>,
    pub rows: Vec<f32>,
    pub bounds: Vec<f64>,
}
// Seeded.next combines the high words of two wrapping LCG steps. A floating
// multiply would lose bits before the shift; ordinary host randomness would
// make two instances of one leaf class grow different vines.
pub struct Seeded(u32);
impl Seeded {
    // The constructor consumes one warm-up value. It is part of the class
    // layout sequence, so the first emitted leaf starts with the next draw.
    pub fn new(seed: u32) -> Self {
        let mut r = Self(seed ^ 0xc71c2830);
        r.next();
        r
    }
    pub fn next(&mut self) -> f64 {
        let a = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        let b = a.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 = b;
        (((b & 0xffff0000) | (a >> 16)) >> 9) as f64 / 8388608.
    }
}
fn n(v: &Value, k: &str, default: f64) -> f64 {
    v[k].as_f64().unwrap_or(default)
}
fn xyz(p: Vector3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn point(v: &Value, default: Vector3) -> Vector3 {
    v.as_array().filter(|a| a.len() == 3).map_or(default, |a| {
        Vector3::new(
            a[0].as_f64().unwrap_or(0.),
            a[1].as_f64().unwrap_or(0.),
            a[2].as_f64().unwrap_or(0.),
        )
    })
}
// buildGrass/buildLeaves keep construction in Float64 until Cards.new packs
// the GPU vertices. Early Float32 rounding changes sunk UVs and the height
// multiplier on leaves, even when their centres appear unchanged.
pub fn cards(
    def: &Value,
    sun: Vector3,
    random: &mut impl FnMut() -> f64,
) -> Result<Vec<f64>, String> {
    let count = n(def, "NumParticles", 50.).floor().max(0.);
    if !count.is_finite() || count > bfe_render::static_cards::MAX_CARDS as f64 {
        return Err("foliage card capacity".into());
    }
    let min = n(def, "MinSize", 10.);
    let max = n(def, "MaxSize", 15.);
    let dark = n(def, "DarknessMin", 1.);
    let span = n(def, "DarknessMax", 1.) - dark;
    // This is a resource budget, not a per-frame queue. Reject an excessive
    // class before reserving its card array; a malformed prepared asset cannot
    // expand a renderer-owned fixed atlas beyond its declared capacity.
    let mut result = Vec::with_capacity(count as usize * 9);
    if def["base"] == "grasspatch" {
        let bx = def["BoxSize"][0].as_f64().unwrap_or(-1.);
        let bz = def["BoxSize"][1].as_f64().unwrap_or(-1.);
        let rmin = n(def, "RadiusFadeMin", -1.);
        let rmax = n(def, "RadiusFadeMax", -1.);
        for i in 0..count as usize {
            let (mut x, mut z, mut c) = (0., 0., 1.);
            // Grass draws Z before X. Radius rejection consumes both draws
            // even when a point is rejected; reordering these operations
            // changes every later size, grey and wind phase in the class.
            if bx > 0. || bz > 0. {
                z = (random() - 0.5) * bz;
                x = (random() - 0.5) * bx;
            } else if rmax > 0. {
                for _ in 0..50 {
                    z = (2. * random() - 1.) * rmax;
                    x = (2. * random() - 1.) * rmax;
                    let d = x.hypot(z);
                    if d >= rmax {
                        continue;
                    }
                    c = if d < rmin {
                        1.
                    } else {
                        1. - (d - rmin) / (rmax - rmin)
                    };
                    if c > 0. {
                        break;
                    }
                }
            }
            // Radius fading sinks the card instead of reducing its width.
            // The renderer subsequently extends its bottom UV for the sunk
            // portion, preserving the authored plant silhouette.
            let size = min + random() * (max - min);
            let grey = dark + random() * span;
            result.extend([
                x,
                n(def, "YOffset", 0.) - 0.5 * min * (1. - c),
                z,
                grey,
                1.1 * size,
                0.9 * size,
                10. * random(),
                (1. + 0.2 * (i % 5) as f64) * 0.8,
                (i & 3) as f64,
            ]);
        }
    } else {
        let mut rand = Seeded::new(n(def, "Seed", 0.) as u32);
        let mut leaves = Vec::<(Vector3, f64)>::new();
        if def["Vine"].is_array() {
            // A bounded recursive walk keeps branch order and draw order. Each
            // call consumes at least one leaf, so depth cannot exceed count.
            // buildLeaves visits the first branch completely before the
            // second. Both geometry order and subsequent PRNG draws depend
            // on that traversal, so a breadth-first walk would differ.
            fn vine(
                def: &Value,
                r: &mut Seeded,
                out: &mut Vec<(Vector3, f64)>,
                mut p: Vector3,
                mut d: Vector3,
                total: usize,
                min: f64,
                max: f64,
            ) {
                if total == 0 {
                    return;
                }
                let mut n = total - 1;
                let spread = super::foliage::n(def, "VineSpread", 0.6);
                let vx = def["Vine"][0].as_f64().unwrap_or(0.);
                let vz = def["Vine"][1].as_f64().unwrap_or(0.);
                let mut size = (min + r.next() * (max - min)) * ((n + 1) as f64 / 4.).min(1.);
                out.push((p, size));
                let lo = def["VineLength"][0].as_f64().unwrap_or(2.);
                let hi = def["VineLength"][1].as_f64().unwrap_or(4.);
                // A branch emits its origin first, then at most its remaining
                // leaf budget. The last four leaves shrink toward the tip;
                // n is decremented after computing each new leaf's size.
                let m = ((lo + r.next() * (hi - lo)).floor().max(0.) as usize).min(n);
                for _ in 0..m {
                    let a = (r.next() - r.next()) / 2.;
                    let b = (r.next() - r.next()) / 2.;
                    d.add(&Vector3::new(b * vx, 0.2, a * vz)).normalize();
                    p.add_scaled_vector(&d, size * spread);
                    size = (min + r.next() * (max - min)) * ((n + 1) as f64 / 4.).min(1.);
                    n -= 1;
                    out.push((p, size));
                }
                if n > 0 {
                    let a = r.next() - r.next();
                    let b = r.next() - r.next();
                    let mut d1 = d;
                    d1.lerp(&Vector3::new(b * vx, 0., a * vz), 0.8).normalize();
                    let n1 = ((0.5 * r.next() + 0.25) * n as f64).floor() as usize;
                    let mut p1 = p;
                    p1.add_scaled_vector(&d1, size * spread);
                    vine(def, r, out, p1, d1, n1, min, max);
                    let a = r.next() - r.next();
                    let b = r.next() - r.next();
                    let mut d2 = d;
                    d2.lerp(&Vector3::new(b * vx, 0., a * vz), 0.8).normalize();
                    p.add_scaled_vector(&d2, size * spread);
                    vine(def, r, out, p, d2, n - n1, min, max);
                }
            }
            vine(
                def,
                &mut rand,
                &mut leaves,
                Vector3::default(),
                Vector3::new(0., 1., 0.),
                count as usize,
                min,
                max,
            );
        // Box leaves draw Z, Y and X in that order. Unlike grass, this path
        // uses the class seed and therefore does not consume host random
        // admission facts shared by grass and placement tint.
        } else if def["BoxSize"].as_array().is_some_and(|a| a.len() == 3) {
            let box_size = point(&def["BoxSize"], Vector3::default());
            for _ in 0..count as usize {
                let z = (rand.next() - 0.5) * box_size.z;
                let y = (rand.next() - 0.5) * box_size.y;
                let x = (rand.next() - 0.5) * box_size.x;
                leaves.push((Vector3::new(x, y, z), min + rand.next() * (max - min)));
            }
        } else {
            let radius = n(def, "Radius", 5.);
            let height = n(def, "Height", 5.);
            let scale = n(def, "HeightScale", 1.);
            let cone = n(def, "ConeHeight", 0.);
            // The heap generator rotates quadrants even for rejected points.
            // Swapping X/Z happens before applying those signs, as in the
            // imported engine layout. It is not a uniform sphere sampler.
            let mut q = 0;
            if radius <= 0. || scale <= 0. {
                return Err("invalid foliage heap dimensions".into());
            }
            for _ in 0..count as usize {
                let mut p = Vector3::default();
                for _ in 0..100 {
                    let z = rand.next() * 1.5 * radius;
                    let y = rand.next() * radius;
                    let x = rand.next() * 1.5 * radius;
                    p.set(x, y, z);
                    if q < 2 {
                        p.x = -p.x
                    }
                    if q == 1 || q == 2 {
                        p.z = -p.z
                    }
                    q = (q + 1) & 3;
                    if p.length() <= radius {
                        break;
                    }
                }
                p.y *= height / (radius * scale);
                if cone > 0. && p.y > height - cone {
                    let f = (height - p.y) / cone;
                    p.x *= f;
                    p.z *= f;
                }
                leaves.push((p, min + rand.next() * (max - min)));
            }
        }
        // Leaf grey includes the direction away from the sun. Apply Offset
        // only after computing that direction; HeightScale also affects the
        // authored Y offset before the card packer scales the final vertex.
        let offset = point(&def["Offset"], Vector3::default());
        let scale = n(def, "HeightScale", 1.);
        for (i, (mut p, size)) in leaves.into_iter().enumerate() {
            let base = dark + rand.next() * span / 2.;
            let mut toward = p;
            if toward.length_sq() > 1e-8 {
                toward.normalize();
            }
            let grey = base - toward.dot(&sun) * (span / 2.);
            p.add(&Vector3::new(offset.x, offset.y / scale, offset.z));
            let [x, y, z] = xyz(p);
            result.extend([
                x,
                y,
                z,
                grey,
                size,
                0.,
                100. * rand.next(),
                0.,
                (i & 3) as f64,
            ]);
        }
    }
    if result.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite foliage card".into());
    }
    Ok(result)
}
pub fn admit(
    data: &Value,
    order: &[String],
    sun: Vector3,
    random: &mut impl FnMut() -> f64,
) -> Result<Vec<Class>, String> {
    // Foliage.constructor constructs all classes, then walks instances in
    // authored order. Keeping these passes separate preserves the random
    // stream when a map interleaves grass and leaf placements.
    let mut result = Vec::new();
    let mut templates = Vec::new();
    let Some(classes) = data["classes"].as_object() else {
        return Ok(result);
    };
    // All classes consume their card draws before placements consume tint
    // draws. Preserve authored insertion order, independently of JSON maps.
    for name in order {
        let def = classes.get(name).ok_or("missing authored foliage class")?;
        let grass = def["base"] == "grasspatch";
        let cards = cards(def, sun, random)?;
        let scale = n(def, "HeightScale", 1.);
        // Constructor bounds cover the complete swaying card rather than its
        // centre alone. Terrain placement culling adds this sphere radius to
        // its authored distance; grass placed by hand has no sphere test.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for c in cards.chunks_exact(9) {
            let center = [c[0], if grass { c[1] } else { c[1] * scale }, c[2]];
            let reach = if grass {
                c[4] + c[5]
            } else {
                c[4] * scale.max(1.)
            };
            for k in 0..3 {
                lo[k] = lo[k].min(center[k] - reach);
                hi[k] = hi[k].max(center[k] + reach);
            }
        }
        let center = std::array::from_fn::<_, 3, _>(|k| {
            if cards.is_empty() {
                0.
            } else {
                (lo[k] + hi[k]) * 0.5
            }
        });
        let radius = if cards.is_empty() {
            0.
        } else {
            ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt()
                * 0.5
        };
        templates.push((center, radius));
        // NumParts is a count in the renderer's typed class descriptor, even
        // when prepared JSON wrote it with a trailing decimal point.
        let mut definition = def.clone();
        if let Some(v) = def["NumParts"].as_f64() {
            definition["NumParts"] = Value::from(v as u32)
        }
        result.push(Class {
            name: name.clone(),
            grass,
            definition,
            texture: def["Texture"]
                .as_str()
                .ok_or("foliage texture missing")?
                .into(),
            cards,
            rows: vec![],
            bounds: vec![],
        });
    }
    if let Some(instances) = data["instances"].as_array() {
        for inst in instances {
            let Some(index) = result
                .iter()
                .position(|c| Some(c.name.as_str()) == inst["class"].as_str())
            else {
                continue;
            };
            let class = &mut result[index];
            let grass = class.grass;
            let def = &class.definition;
            let (center, radius) = templates[index];
            let rows = &mut class.rows;
            let bounds = &mut class.bounds;
            if rows.len() / 16 >= 1_048_576 {
                return Err("foliage patch capacity".into());
            }
            let m: [f64; 16] =
                serde_json::from_value(inst["matrix"].clone()).map_err(|e| e.to_string())?;
            if m.iter().any(|v| !v.is_finite()) {
                return Err("invalid foliage matrix".into());
            }
            // Even a grass patch or explicitly tinted instance consumes its
            // shade draw. Skipping that unused value would change every
            // following default leaf tint in a recorded admission stream.
            let shade = random();
            let position = [m[12], m[13], m[14]];
            // Foliage.constructor retains the imported up axis and discards
            // yaw. Rebuilding this orthonormal basis keeps vines in their
            // authored growth plane rather than rotating each card by yaw.
            let mut y = Vector3::new(m[4], m[5], m[6]);
            y.normalize();
            let mut x = Vector3::new(y.y, -y.x, 0.);
            x.normalize();
            let mut z = Vector3::default();
            z.cross_vectors(&x, &y).normalize();
            let terrain = inst["terrain"].as_bool().unwrap_or(false);
            let sphere = !grass || terrain;
            let distance = n(inst, "distance", 0.);
            let max = if terrain {
                if grass {
                    0.
                } else {
                    distance
                }
            } else if grass {
                n(def, "MaxDistance", 100.)
            } else {
                0.
            };
            let tint = if let Some(a) = inst["tint"].as_array().filter(|a| a.len() == 3) {
                [
                    a[0].as_f64().unwrap_or(1.),
                    a[1].as_f64().unwrap_or(1.),
                    a[2].as_f64().unwrap_or(1.),
                ]
            } else if grass {
                [1.; 3]
            } else {
                std::array::from_fn(|_| 0.7 + 0.2 * shade + 0.1 * random())
            };
            if !shade.is_finite() || tint.iter().any(|v| !v.is_finite()) {
                return Err("foliage random admission stream exhausted".into());
            }
            rows.extend(
                position
                    .into_iter()
                    .chain(tint)
                    .chain(xyz(x))
                    .chain(xyz(y))
                    .chain(xyz(z))
                    .chain([max])
                    .map(|v| v as f32),
            );
            let mut matrix = Matrix4::default();
            matrix.make_basis(&x, &y, &z);
            matrix.elements[12..15].copy_from_slice(&position);
            let mut c = Vector3::new(center[0], center[1], center[2]);
            c.apply_matrix4(&matrix);
            bounds.extend(position);
            bounds.extend(xyz(c));
            bounds.extend([
                if sphere { radius } else { -1. },
                if terrain {
                    1.45 * distance + if sphere { radius } else { 0. }
                } else {
                    f64::INFINITY
                },
                max,
            ]);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_excessive_cards_and_missing_random_facts() {
        let d = serde_json::json!({"base":"grasspatch","NumParticles":bfe_render::static_cards::MAX_CARDS+1});
        assert!(cards(&d, Vector3::default(), &mut || 0.5).is_err());
        let d = serde_json::json!({"classes":{"leaf":{"base":"leafpatch","Texture":"leaf","NumParticles":0}},"instances":[{"class":"leaf","matrix":[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.]}]});
        assert!(admit(&d, &["leaf".into()], Vector3::default(), &mut || f64::NAN).is_err());
    }
    #[test]
    fn large_authored_grass_class_keeps_placements_in_order() {
        // Naboo's terrain grass exceeds a 16-bit patch count. Selection stays
        // renderer-owned; cold admission must keep the entire authored set.
        let instance = serde_json::json!({"class":"grass","matrix":[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.],"tint":[1.,1.,1.],"terrain":true,"distance":100});
        let d = serde_json::json!({"classes":{"grass":{"base":"grasspatch","Texture":"grass","NumParticles":0}},"instances":vec![instance;65537]});
        let classes = admit(&d, &["grass".into()], Vector3::default(), &mut || 0.5).unwrap();
        assert_eq!(classes[0].rows.len(), 65537 * 16);
        assert_eq!(classes[0].bounds.len(), 65537 * 9);
    }
}
