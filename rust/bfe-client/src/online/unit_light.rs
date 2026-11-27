//! A soldier's light, as game.js lightAt sets it each frame from the middle of his body (0.9 m over his feet).
//! Light regions weigh in over a 2.5 m band at their edges: the sun is scaled by the least of
//! 1 - (1 - directional) x weight, and an ambient factor under 1 pulls the ambient toward black (between 1 and 2,
//! toward white). Maps with a LUMI map also darken by min(LUMI / 255 x 1.2 + 0.3, 1).
//! The result fills an instance record's look: [sunScale, ambientPull.x, ambientPull.y, brightness].
use bfe_content::regions::{build_region, BuiltRegion};
use bfe_content::three::Vector3;
use bfe_content::Value;

const BAND: f64 = 2.5;

struct Region {
    built: BuiltRegion,
    sphere: bool,
    cylinder: bool,
    directional: Option<f64>,
    ambient: Option<f64>,
}

struct Lumi {
    size: usize,
    unit: f64,
    origin: [f64; 2],
    data: Vec<u8>,
}

#[derive(Default)]
pub struct UnitLight {
    regions: Vec<Region>,
    lumi: Option<Lumi>,
}

fn ramp(x: f64) -> f64 {
    (x / (2. * BAND)).clamp(0., 1.)
}

impl UnitLight {
    /// world: the map's map.json; lumi: its LUMI file's bytes, when the map has one.
    pub fn new(world: &Value, lumi: Option<Vec<u8>>) -> Self {
        let regions = world
            .get("lightRegions")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .map(|region| {
                let kind = region.get("type").and_then(Value::as_str).unwrap_or("");
                Region {
                    built: build_region(region),
                    sphere: kind == "sphere",
                    cylinder: kind == "cylinder",
                    directional: region.get("directional").and_then(Value::as_f64),
                    ambient: region.get("ambient").and_then(Value::as_f64),
                }
            })
            .collect();
        let lumi = world.get("lumi").zip(lumi).and_then(|(info, data)| {
            let size = info.get("size").and_then(Value::as_f64)? as usize;
            let origin = info.get("origin")?;
            let lumi = Lumi {
                size,
                unit: info.get("unit").and_then(Value::as_f64)?,
                origin: [origin.at(0).and_then(Value::as_f64)?, origin.at(1).and_then(Value::as_f64)?],
                data,
            };
            (size > 0 && lumi.data.len() >= size * size).then_some(lumi)
        });
        Self { regions, lumi }
    }

    /// 2005 unit lighting: falls back to `shadow ...` entries in `world["regions"]`
    /// when `lightRegions` is empty.
    pub fn new_swbf2(world: &Value, lumi: Option<Vec<u8>>) -> Self {
        let mut light = Self::new(world, lumi);
        if light.regions.is_empty() {
            if let Some(raw_regions) = world.get("regions").and_then(Value::as_array) {
                for region in raw_regions {
                    let Some(name) = region.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let mut parts = name.split_ascii_whitespace();
                    if !parts.next().is_some_and(|h| h.eq_ignore_ascii_case("shadow")) {
                        continue;
                    }
                    let mut directional = None;
                    let mut ambient = None;
                    for token in parts {
                        if let Some((k, v)) = token.split_once('=') {
                            if k.eq_ignore_ascii_case("directional") {
                                directional = v.parse::<f64>().ok();
                            } else if k.eq_ignore_ascii_case("ambient") {
                                ambient = v.parse::<f64>().ok();
                            }
                        }
                    }
                    if directional.is_some() || ambient.is_some() {
                        let kind = region.get("type").and_then(Value::as_str).unwrap_or("");
                        light.regions.push(Region {
                            built: build_region(region),
                            sphere: kind == "sphere",
                            cylinder: kind == "cylinder",
                            directional,
                            ambient,
                        });
                    }
                }
            }
        }
        light
    }

    fn weight(region: &Region, p: &Vector3) -> f64 {
        let mut local = *p;
        local.apply_matrix4(&region.built.inverse);
        let half = &region.built.half;
        let (b, c) = (half.get(1).copied().unwrap_or(0.), half.get(2).copied().unwrap_or(0.));
        if region.sphere {
            return ramp(region.built.radius + BAND - local.length());
        }
        if region.cylinder {
            let a = region.built.radius;
            let r = local.x.hypot(local.z);
            let y = local.y.abs();
            if r > a + BAND || y > b + BAND {
                return 0.;
            }
            let deep_r = r <= a - BAND;
            let deep_y = y <= b - BAND;
            if deep_r && deep_y {
                return 1.;
            }
            // As the original game has it: with only one axis in its band the
            // ramp uses the distance from the axis, not from the surface.
            if deep_y {
                return ramp(BAND - r);
            }
            if deep_r {
                return ramp(BAND - y);
            }
            let wr = ramp(a + BAND - r);
            let wy = ramp(b + BAND - y);
            return (1. - (1. - wr).hypot(1. - wy)).max(0.);
        }
        // A box (any other type keeps its built half extents, as a box).
        let a = half.first().copied().unwrap_or(0.);
        let dx = local.x.abs() - a;
        let dy = local.y.abs() - b;
        let dz = local.z.abs() - c;
        if dx < 0. && dy < 0. && dz < 0. {
            return ramp((-dx).min(-dy).min(-dz) + BAND);
        }
        ramp(BAND - (dx.max(0.).powi(2) + dy.max(0.).powi(2) + dz.max(0.).powi(2)).sqrt())
    }

    fn lumi_at(lumi: &Lumi, x: f64, z: f64) -> f64 {
        let fx = (x - lumi.origin[0]) / lumi.unit;
        let fz = (z - lumi.origin[1]) / lumi.unit;
        let last = lumi.size - 1;
        let x0 = fx.floor().clamp(0., last as f64) as usize;
        let z0 = fz.floor().clamp(0., last as f64) as usize;
        let x1 = (x0 + 1).min(last);
        let z1 = (z0 + 1).min(last);
        let tx = (fx - x0 as f64).clamp(0., 1.);
        let tz = (fz - z0 as f64).clamp(0., 1.);
        let at = |i: usize, k: usize| f64::from(lumi.data[i * lumi.size + k]);
        let near = at(x0, z0) + (at(x1, z0) - at(x0, z0)) * tx;
        let far = at(x0, z1) + (at(x1, z1) - at(x0, z1)) * tx;
        near + (far - near) * tz
    }

    /// The look of a body whose feet are at `feet`: [sun, darken, lighten, brightness].
    pub fn at(&self, feet: [f64; 3]) -> [f64; 4] {
        let center = Vector3::new(feet[0], feet[1] + 0.9, feet[2]);
        let (mut sun, mut darken, mut lighten) = (1f64, 0f64, 0f64);
        for region in &self.regions {
            let w = Self::weight(region, &center);
            if w <= 0. {
                continue;
            }
            if let Some(d) = region.directional {
                sun = sun.min(1. - (1. - d) * w);
            }
            if let Some(a) = region.ambient {
                if a < 1. {
                    darken = darken.max((1. - a) * w);
                }
                if a > 1. && a < 2. {
                    lighten = lighten.max((a - 1.) * w);
                }
            }
        }
        let brightness = self.lumi.as_ref().map_or(1., |lumi| (Self::lumi_at(lumi, feet[0], feet[2]) / 255. * 1.2 + 0.3).min(1.));
        [sun, darken, lighten, brightness]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_region_and_lumi_darken_a_body_inside_only() {
        // Bespin's shadow02: a cylinder that takes the sun away (directional 0) and keeps the ambient (1).
        let world = bfe_content::json::parse(
            r#"{"lightRegions":[{"type":"cylinder","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"size":[20,5,0.1],"ambient":0.5,"directional":0}],
                "lumi":{"file":"lumi.bin","size":2,"unit":8,"origin":[-8,-8]}}"#,
        )
        .unwrap();
        let light = UnitLight::new(&world, Some(vec![0, 0, 0, 0]));
        let inside = light.at([0., -0.9, 0.]);
        assert_eq!(inside, [0., 0.5, 0., 0.3]);
        let outside = light.at([100., 0., 0.]);
        assert_eq!(outside[..3], [1., 0., 0.]);
        assert_eq!(UnitLight::default().at([0.; 3]), [1., 0., 0., 1.]);
    }

    #[test]
    fn swbf2_shadow_region_in_regions_table_darkens_directional_inside() {
        let world = bfe_content::json::parse(
            r#"{"regions":[{"type":"sphere","name":"shadow directional=0.4","size":[10,0,0]}]}"#,
        )
        .unwrap();
        // Released 2004 constructor ignores `regions` when `lightRegions` is absent.
        let classic = UnitLight::new(&world, None);
        assert_eq!(classic.at([0., 0., 0.])[0], 1.);
        // 2005 constructor scales directional light inside the sphere only.
        let light = UnitLight::new_swbf2(&world, None);
        assert!((light.at([0., 0., 0.])[0] - 0.4).abs() < 1e-6);
        assert_eq!(light.at([25., 0., 0.])[0], 1.);
    }
}
