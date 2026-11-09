//! CSS cubic-bezier timing with x inversion, shared by native page motion.
pub(super) fn ease(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let t = t.clamp(0., 1.);
    let mut lo = 0.;
    let mut hi = 1.;
    let mut s = t;
    let curve = |s: f64, a: f64, b: f64| {
        3. * (1. - s).powi(2) * s * a + 3. * (1. - s) * s * s * b + s.powi(3)
    };
    for _ in 0..16 {
        if curve(s, x1, x2) < t {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.;
    }
    curve(s, y1, y2)
}
pub(super) struct RowSpan {
    pub first: usize,
    pub end: usize,
    pub index: usize,
    pub selected: bool,
}
pub(super) struct LeavingQuad {
    position: [f32; 12],
    uv: [f32; 8],
    mask_uv: [f32; 8],
    tint: [f32; 16],
    texture: u32,
    mask: u32,
    index: usize,
    selected: bool,
    center: [f32; 2],
}
pub(super) fn retain_rows(
    b: &bfe_render::interface::Batch,
    rows: &[RowSpan],
    out: &mut Vec<LeavingQuad>,
) {
    out.clear();
    for row in rows {
        if row.end <= row.first {
            continue;
        }
        let first = &b.position[row.first * 12..row.first * 12 + 12];
        let last = &b.position[(row.end - 1) * 12..row.end * 12];
        let center = [(first[0] + last[6]) * 0.5, (first[1] + last[7]) * 0.5];
        for q in row.first..row.end {
            let Some(draw) = b
                .draws
                .iter()
                .find(|d| q * 6 >= d.first && q * 6 < d.first + d.count)
            else {
                continue;
            };
            out.push(LeavingQuad {
                position: b.position[q * 12..q * 12 + 12].try_into().unwrap(),
                uv: b.uv[q * 8..q * 8 + 8].try_into().unwrap(),
                mask_uv: b.mask_uv[q * 8..q * 8 + 8].try_into().unwrap(),
                tint: b.tint[q * 16..q * 16 + 16].try_into().unwrap(),
                texture: draw.texture,
                mask: draw.mask,
                index: row.index,
                selected: row.selected,
                center,
            });
        }
    }
}
pub(super) fn leaving(
    b: &mut bfe_render::interface::Batch,
    rows: &[LeavingQuad],
    elapsed: f64,
    forward: bool,
) {
    for quad in rows {
        let duration = if quad.selected { 0.36 } else { 0.24 };
        let delay = quad.index.min(8) as f64 * 0.018;
        let p = ((elapsed - delay) / duration).clamp(0., 1.);
        if p >= 1. {
            continue;
        }
        let eased = ease(p, 0.55, 0., 0.75, 0.2);
        let shift = eased * 1024. * 0.05 * if forward { -1. } else { 1. };
        let scale = if quad.selected { 1. + 0.1 * p } else { 1. };
        let brightness = if quad.selected {
            1. + 0.7 * (1. - ((p - 0.3) / 0.3).abs()).max(0.)
        } else {
            1.
        };
        let q = b.count;
        if (q + 1) * 12 > b.position.len() {
            break;
        }
        b.position[q * 12..q * 12 + 12].copy_from_slice(&quad.position);
        b.uv[q * 8..q * 8 + 8].copy_from_slice(&quad.uv);
        b.mask_uv[q * 8..q * 8 + 8].copy_from_slice(&quad.mask_uv);
        b.tint[q * 16..q * 16 + 16].copy_from_slice(&quad.tint);
        for k in 0..4 {
            let v = q * 4 + k;
            b.position[v * 3] = (quad.center[0] as f64
                + (b.position[v * 3] as f64 - quad.center[0] as f64) * scale
                + shift) as f32;
            b.position[v * 3 + 1] = (quad.center[1] as f64
                + (b.position[v * 3 + 1] as f64 - quad.center[1] as f64) * scale)
                as f32;
            for c in 0..3 {
                b.tint[v * 4 + c] *= brightness as f32;
            }
            b.tint[v * 4 + 3] *= (1. - p) as f32;
        }
        b.draws.push(bfe_render::interface::Draw {
            first: q * 6,
            count: 6,
            texture: quad.texture,
            mask: quad.mask,
        });
        b.count += 1;
        b.start = b.start.min(q * 4);
        b.end = b.end.max((q + 1) * 4);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn timing_preserves_endpoints_and_the_reference_acceleration() {
        assert!(super::ease(0., 0.2, 0.8, 0.2, 1.) < 0.001);
        assert!(super::ease(1., 0.2, 0.8, 0.2, 1.) > 0.999);
        assert!(super::ease(0.5, 0.2, 0.8, 0.2, 1.) > 0.9);
    }
}
