//! Collision windows for a hangar shield shared by several exterior portals.
use super::bounds::{HangarForcefield, SpaceBounds, PROVISIONAL_FORCEFIELD_PORTAL_TOLERANCE};
use bfe_content::Value;
use std::collections::{BTreeMap, BTreeSet};
use three::{Matrix4, Quaternion};

fn rows<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], String> {
    let rows = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("invalid portal {field}"))?;
    if rows.len() > 4096 {
        return Err(format!("too many portal {field}"));
    }
    Ok(rows)
}

fn vector<const N: usize>(row: &Value, field: &str) -> Option<[f64; N]> {
    let src = row.get(field)?.as_array()?;
    if src.len() != N {
        return None;
    }
    let mut out = [0.; N];
    for (v, item) in out.iter_mut().zip(src) {
        *v = item
            .as_f64()
            .filter(|n| n.is_finite() && (*n as f32).is_finite())?;
    }
    Some(out)
}

pub(super) fn validate(map: &Value) -> Result<(), String> {
    let Some(table) = map.get("portals") else {
        return Ok(());
    };
    for sector in rows(table, "sectors")? {
        if sector.get("name").and_then(Value::as_str).is_none() {
            return Err("unnamed portal sector".into());
        }
        let objects = rows(sector, "objects")?;
        if objects.iter().any(|s| s.as_str().is_none()) {
            return Err("invalid portal sector object".into());
        }
    }
    for p in rows(table, "portals")? {
        let rot = vector::<4>(p, "rotation").ok_or("invalid portal rotation")?;
        if rot.iter().map(|x| x * x).sum::<f64>() <= f64::EPSILON
            || vector::<3>(p, "position").is_none()
        {
            return Err("invalid portal placement".into());
        }
        for key in ["width", "height"] {
            if !p
                .get(key)
                .and_then(Value::as_f64)
                .is_some_and(|n| n > 0. && (n as f32).is_finite())
            {
                return Err(format!("invalid portal {key}"));
            }
        }
    }
    Ok(())
}

pub(super) fn attach(bounds: &mut SpaceBounds, map: &Value) {
    let Some(table) = map.get("portals") else {
        return;
    };
    let (Ok(sectors), Ok(portals)) = (rows(table, "sectors"), rows(table, "portals")) else {
        return;
    };
    let mut owners = BTreeMap::new();
    let by_name: BTreeMap<_, _> = bounds
        .forcefields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.as_str(), i))
        .collect();
    for sector in sectors {
        let Some(name) = sector.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Ok(objects) = rows(sector, "objects") else {
            continue;
        };
        if let Some(&index) = objects
            .iter()
            .filter_map(Value::as_str)
            .find_map(|n| by_name.get(n))
        {
            owners.insert(name, index);
        }
    }
    let mut replaced = BTreeSet::new();
    let mut fields = Vec::new();
    for p in portals {
        let left = p.get("sector1").and_then(Value::as_str);
        let right = p.get("sector2").and_then(Value::as_str);
        let exterior = match (left, right) {
            (Some(n), None) | (None, Some(n)) => n,
            _ => continue,
        };
        let Some(&owner) = owners.get(exterior) else {
            continue;
        };
        let (Some([w, x, y, z]), Some(center), Some(width), Some(height)) = (
            vector::<4>(p, "rotation"),
            vector::<3>(p, "position"),
            p.get("width").and_then(Value::as_f64),
            p.get("height").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let mut rotation = Quaternion { x, y, z, w };
        rotation.normalize();
        let mut matrix = Matrix4::default();
        matrix.make_rotation_from_quaternion(&rotation);
        matrix.elements[12..15].copy_from_slice(&center);
        let mut inverse = matrix;
        inverse.invert();
        fields.push(HangarForcefield {
            name: bounds.forcefields[owner].name.clone(),
            team: bounds.forcefields[owner].team,
            center,
            matrix: matrix.elements,
            inverse: inverse.elements,
            half_extents: [
                width * 0.5,
                height * 0.5,
                PROVISIONAL_FORCEFIELD_PORTAL_TOLERANCE,
            ],
            kernel_body: None,
            authored_opening: true,
        });
        replaced.insert(owner);
    }
    if fields.is_empty() {
        return;
    }
    let mut index = 0;
    bounds.forcefields.retain(|_| {
        let keep = !replaced.contains(&index);
        index += 1;
        keep
    });
    bounds.forcefields.extend(fields);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content::json::parse;

    #[test]
    fn only_static_shield_triangles_inside_the_exterior_opening_admit_flyers() {
        let source=parse(r#"{"instances":[{"class":"fixture_shield","name":"blue_sheet"}],"portals":{
            "sectors":[{"name":"dock","objects":["blue_sheet"]}],
            "portals":[{"sector1":"dock","width":14,"height":6,"position":[17,9,60],"rotation":[1,0,0,0]},
                {"sector1":"dock","sector2":"lounge","width":4,"height":6,"position":[3,9,12],"rotation":[1,0,0,0]}]}}"#).unwrap();
        validate(&source).unwrap();
        let mut bounds = SpaceBounds::from_world_json(&source);
        attach(&mut bounds, &source);
        assert_eq!(bounds.forcefields.len(), 1);
        let contact = |body, pos| bounds.is_forcefield_hit(body, pos, [0., 0., 1.]);
        assert!(contact(crate::collision::STATIC, [17., 9., 60.]));
        assert!(!contact(crate::collision::STATIC, [25., 9., 60.]));
        assert!(!contact(crate::collision::STATIC, [17., 13., 60.]));
        assert!(!contact(crate::collision::STATIC, [3., 9., 12.]));
        assert!(!contact(7, [17., 9., 60.]));
        assert!(bounds
            .forcefield_blocks_segment([17., 9., 56.], [17., 9., 66.])
            .is_some());
    }
}
