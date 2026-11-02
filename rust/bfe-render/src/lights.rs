// Which of the frame's point lights reach a draw, as point-lights.js
// decides it for three: the masks every pass that lights with them uses.
use crate::math::{self, Mat4};

// point-lights.js lightMask: bit i for each packed light whose sphere
// reaches one of the boxes (those `visible` marks, when given).
pub fn light_mask(lights: &[f64], count: usize, boxes: &[([f64; 3], [f64; 3])], visible: Option<&[u8]>) -> u32 {
    let mut mask = 0u32;
    for i in 0..count {
        let s = &lights[8 * i..8 * i + 4];
        let range = s[3];
        if range <= 0.0 {
            mask |= 1 << i;
            continue;
        }
        let reach = range * range;
        for (b, (min, max)) in boxes.iter().enumerate() {
            if let Some(v) = visible {
                if v[b] == 0 {
                    continue;
                }
            }
            let dx = (min[0] - s[0]).max(0.0).max(s[0] - max[0]);
            let dy = (min[1] - s[1]).max(0.0).max(s[1] - max[1]);
            let dz = (min[2] - s[2]).max(0.0).max(s[2] - max[2]);
            if dx * dx + dy * dy + dz * dz <= reach {
                mask |= 1 << i;
                break;
            }
        }
    }
    mask
}

// sphereLightMask.
pub fn sphere_light_mask(lights: &[f64], count: usize, c: &[f64; 3], radius: f64) -> u32 {
    let mut mask = 0u32;
    for i in 0..count {
        let s = &lights[8 * i..8 * i + 4];
        let (dx, dy, dz) = (s[0] - c[0], s[1] - c[1], s[2] - c[2]);
        if s[3] <= 0.0 || (dx * dx + dy * dy + dz * dz).sqrt() <= s[3] + radius {
            mask |= 1 << i;
        }
    }
    mask
}

// Box3.applyMatrix4.
pub fn box_to_world(m: &Mat4, min: &[f64; 3], max: &[f64; 3]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for k in 0..8 {
        let p = [if k & 4 != 0 { max[0] } else { min[0] }, if k & 2 != 0 { max[1] } else { min[1] }, if k & 1 != 0 { max[2] } else { min[2] }];
        let (q, _) = math::sphere_to_world(m, &p, 0.0);
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
    }
    (lo, hi)
}
