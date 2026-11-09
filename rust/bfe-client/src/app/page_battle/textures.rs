//! The Page stage's small procedural CanvasTextures, rasterized at admission.
//! Ring widths, dash lengths and tick counts are the authored UI shapes.
use std::f64::consts::TAU;
fn blend(alpha: &mut f64, coverage: f64) {
    *alpha = 1. - (1. - *alpha) * (1. - coverage.clamp(0., 1.));
}
fn distance(x: f64, y: f64, a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let t = ((x - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy);
    (x - a[0] - dx * t.clamp(0., 1.)).hypot(y - a[1] - dy * t.clamp(0., 1.))
}
pub(super) fn ring(size: usize, map: bool) -> Vec<u8> {
    let s = size as f64;
    let rings: &[(f64, f64, f64, f64, f64)] = if map {
        &[
            (0.43, 4., 0.9, 0., 0.),
            (0.43 * 1.045, 1.5, 0.5, 3., 9.),
            (0.43 * 1.1, 2.5, 0.65, 0., 0.),
        ]
    } else {
        &[
            (0.47, 6., 1., 0., 0.),
            (0.43, 2., 0.6, 18., 10.),
            (0.33, 3., 0.8, 0., 0.),
            (0.29, 1.5, 0.5, 4., 8.),
            (0.16, 2., 0.45, 0., 0.),
        ]
    };
    let ticks = if map { 120 } else { 60 };
    let mut lines = Vec::with_capacity(ticks);
    for i in 0..ticks {
        let a = i as f64 / ticks as f64 * TAU;
        let long = i % if map { 10 } else { 5 } == 0;
        let (r0, r1, width, alpha) = if map {
            (
                0.43 * if long { 1.055 } else { 1.065 },
                0.43 * 1.09,
                if long { 4. } else { 1.5 },
                if long { 0.95 } else { 0.4 },
            )
        } else {
            (
                if long { 0.37 } else { 0.385 },
                0.41,
                if long { 3. } else { 1.5 },
                if long { 0.9 } else { 0.45 },
            )
        };
        lines.push((
            [a.cos() * r0 * s, a.sin() * r0 * s],
            [a.cos() * r1 * s, a.sin() * r1 * s],
            width,
            alpha,
        ));
    }
    let mut out = vec![255; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let px = x as f64 + 0.5 - s * 0.5;
            let py = y as f64 + 0.5 - s * 0.5;
            let r = px.hypot(py);
            let a = py.atan2(px).rem_euclid(TAU);
            let mut alpha = 0.;
            for &(radius, width, opacity, dash, gap) in rings {
                if dash > 0. && (a * radius * s) % (dash + gap) > dash {
                    continue;
                }
                blend(
                    &mut alpha,
                    (width * 0.5 + 0.5 - (r - radius * s).abs()).clamp(0., 1.) * opacity,
                );
            }
            for &(p, q, width, opacity) in &lines {
                if (r - (p[0].hypot(p[1]) + q[0].hypot(q[1])) * 0.5).abs() > s * 0.04 {
                    continue;
                }
                blend(
                    &mut alpha,
                    (width * 0.5 + 0.5 - distance(px, py, p, q)).clamp(0., 1.) * opacity,
                );
            }
            let arcs = if map { 4. } else { 3. };
            let start = if map { 0.3 } else { 0. };
            let sweep = if map { 0.42 } else { 0.5 };
            let radius = if map { 0.43 * 1.1 } else { 0.47 };
            let width = if map { 9. } else { 10. };
            if (a - start).rem_euclid(TAU / arcs) < sweep {
                blend(
                    &mut alpha,
                    (width * 0.5 + 0.5 - (r - radius * s).abs()).clamp(0., 1.),
                );
            }
            out[(y * size + x) * 4 + 3] = (alpha * 255.).round() as u8;
        }
    }
    out
}
pub(super) fn dot(size: usize) -> Vec<u8> {
    let mut out = vec![255; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let r = ((x as f64 + 0.5) / size as f64 * 2. - 1.)
                .hypot((y as f64 + 0.5) / size as f64 * 2. - 1.);
            let a = if r < 0.35 {
                1. - r / 0.35 * 0.55
            } else {
                0.45 * (1. - (r - 0.35) / 0.65).max(0.)
            };
            out[(y * size + x) * 4 + 3] = (a * 255.).round() as u8;
        }
    }
    out
}
pub(super) fn pin(size: usize) -> Vec<u8> {
    let mut out = vec![255; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let across = 1. - ((x as f64 + 0.5) / size as f64 * 2. - 1.).abs();
            let up = (y as f64 + 0.5) / size as f64;
            let a = if up < 0.7 {
                1. - up / 0.7 * 0.65
            } else {
                0.35 * (1. - (up - 0.7) / 0.3)
            };
            out[(y * size + x) * 4 + 3] = (across * a * 255.).round() as u8;
        }
    }
    out
}
