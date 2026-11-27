//! Named regions a goal can point at (a capture area, a flag base).
//!
//! Prepared 2005 maps give a region's placement as a 16-number matrix in
//! simulation space, its translation in elements 12 to 14. The older form
//! is the world file's 12-number transform, whose translation is elements 9
//! to 11 with the z axis negated. A record with neither may give a centre.
//! Names are looked up ignoring ASCII case, with the mission object index's
//! own comparison.

use crate::ai::nav::math::Vec3;
use crate::battle::rules::objects::name_order;

/// A region's centre from its record: `matrix` (16 numbers), else `xfrm`
/// (12 numbers), else `center` (3 numbers).
pub fn region_center(region: &bfe_content::value::Value) -> Option<Vec3> {
    let numbers = |key: &str, len: usize| -> Option<Vec<f64>> {
        let values = region.get(key)?.as_array()?;
        if values.len() != len {
            return None;
        }
        values.iter().map(|v| v.as_f64().filter(|n| n.is_finite())).collect()
    };
    if let Some(m) = numbers("matrix", 16) {
        return Some(Vec3::new(m[12], m[13], m[14]));
    }
    if let Some(m) = numbers("xfrm", 12) {
        return Some(Vec3::new(m[9], m[10], -m[11]));
    }
    numbers("center", 3).map(|c| Vec3::new(c[0], c[1], c[2]))
}

/// Region centres by name, sorted once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GoalRegions {
    rows: Vec<(Box<[u8]>, Vec3)>,
}

impl GoalRegions {
    /// From named centres. A name given twice is ambiguous and never found.
    pub fn new(mut rows: Vec<(Box<[u8]>, Vec3)>) -> Self {
        rows.sort_by(|a, b| name_order(&a.0, &b.0));
        Self { rows }
    }

    /// From a map's region list (`map.json` `regions`: records with `name`
    /// and a placement). Records without a usable name or placement are
    /// skipped.
    pub fn read(regions: &bfe_content::value::Value) -> Self {
        let rows = regions
            .as_array()
            .unwrap_or(&[])
            .iter()
            .filter_map(|r| {
                let name = r.get("name")?.as_str()?;
                Some((Box::<[u8]>::from(name.as_bytes()), region_center(r)?))
            })
            .collect();
        Self::new(rows)
    }

    pub fn find(&self, name: &[u8]) -> Option<Vec3> {
        let start = self.rows.partition_point(|row| name_order(&row.0, name).is_lt());
        let found = self.rows.get(start).filter(|row| name_order(&row.0, name).is_eq())?;
        if self.rows.get(start + 1).is_some_and(|row| name_order(&row.0, name).is_eq()) {
            return None;
        }
        Some(found.1)
    }
}
