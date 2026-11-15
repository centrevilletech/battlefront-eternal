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

#[cfg(test)]
mod tests {
    #[test]
    fn timing_preserves_endpoints_and_the_reference_acceleration() {
        assert!(super::ease(0., 0.2, 0.8, 0.2, 1.) < 0.001);
        assert!(super::ease(1., 0.2, 0.8, 0.2, 1.) > 0.999);
        assert!(super::ease(0.5, 0.2, 0.8, 0.2, 1.) > 0.9);
    }
}
