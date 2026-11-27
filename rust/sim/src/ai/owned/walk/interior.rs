//! Playable vertical ranges from room-sector polygons.
use bfe_content::Value;

pub(super) struct Sector {
    bottom: f64,
    top: f64,
    ring: Vec<[f64; 2]>,
}

impl Sector {
    pub fn read(records: Option<&Value>) -> Result<Vec<Self>, String> {
        let Some(rows) = records.and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        if rows.len() > 256 {
            return Err("too many navigation sectors".into());
        }
        let mut result = Vec::new();
        for row in rows {
            let number = |v: Option<&Value>| {
                v.and_then(Value::as_f64)
                    .filter(|n| n.is_finite())
                    .ok_or("invalid navigation sector coordinate")
            };
            let bottom = number(row.get("base"))?;
            let height = number(row.get("height"))?;
            let points = row
                .get("points")
                .and_then(Value::as_array)
                .ok_or("navigation sector has no polygon")?;
            if points.len() > 256 {
                return Err("navigation sector polygon exceeds vertex limit".into());
            }
            let mut ring = Vec::with_capacity(points.len());
            for point in points {
                ring.push([number(point.at(0))?, number(point.at(1))?]);
            }
            if ring.len() >= 3 && height > 0. {
                result.push(Self {
                    bottom,
                    top: bottom + height,
                    ring,
                });
            }
        }
        Ok(result)
    }
    pub fn range(&self, x: f64, z: f64) -> Option<(f64, f64)> {
        let mut inside = false;
        let mut last = self.ring[self.ring.len() - 1];
        for &p in &self.ring {
            if (p[1] > z) != (last[1] > z)
                && x < (last[0] - p[0]) * (z - p[1]) / (last[1] - p[1]) + p[0]
            {
                inside = !inside;
            }
            last = p;
        }
        inside.then_some((self.bottom, self.top))
    }
}
