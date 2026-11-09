//! Cold rasterization of the page button surfaces from electron/ui/looks.css.
//! Each viewport admits its own geometry, so CSS pixel corner cuts stay sharp.
pub fn button(w: usize, h: usize, look: usize, selected: bool) -> Vec<u8> {
    type Stop = (f64, [f64; 4]);
    let eternal: &[Stop] = if selected {
        &[
            (0., [255., 190., 85., 255.]),
            (0.6, [255., 148., 25., 255.]),
            (1., [245., 117., 0., 255.]),
        ]
    } else {
        &[(0., [20., 24., 32., 224.4]), (1., [20., 24., 32., 153.])]
    };
    let bf1: &[Stop] = if selected {
        &[
            (0., [255., 210., 122., 255.]),
            (0.45, [255., 164., 42., 255.]),
            (1., [245., 117., 0., 255.]),
        ]
    } else {
        &[(0., [60., 130., 240., 86.7]), (1., [14., 52., 140., 153.])]
    };
    let bf2: &[Stop] = if selected {
        &[
            (0., [255., 208., 200., 255.]),
            (0.16, [240., 103., 90., 255.]),
            (0.48, [207., 42., 29., 255.]),
            (0.52, [165., 24., 14., 255.]),
            (1., [216., 72., 58., 255.]),
        ]
    } else {
        &[
            (0., [244., 246., 248., 255.]),
            (0.16, [207., 212., 217., 255.]),
            (0.48, [163., 170., 178., 255.]),
            (0.52, [133., 140., 149., 255.]),
            (1., [179., 185., 192., 255.]),
        ]
    };
    let stops = match look {
        0 => bf1,
        1 => bf2,
        _ => eternal,
    };
    let border = match (look, selected) {
        (0, false) => [140., 200., 255., 140.25],
        (0, true) => [255., 226., 164., 255.],
        (1, false) => [35., 39., 45., 255.],
        (1, true) => [122., 13., 6., 255.],
        (_, true) => [255., 196., 107., 255.],
        _ => [255., 255., 255., 33.15],
    };
    let mut out = vec![0; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut premult = [0.; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = x as f64 + (sx as f64 + 0.5) / 4.;
                    let py = y as f64 + (sy as f64 + 0.5) / 4.;
                    let inside = if look == 2 {
                        px + py >= 14. && (w as f64 - px) + (h as f64 - py) >= 14.
                    } else {
                        let r = (if look == 1 { h as f64 / 2. } else { 3. }).min(w as f64 / 2.);
                        let dx = px - px.clamp(r, w as f64 - r);
                        let dy = py - py.clamp(r, h as f64 - r);
                        dx * dx + dy * dy <= r * r
                    };
                    if !inside {
                        continue;
                    }
                    let along = if look == 2 {
                        px / w as f64
                    } else {
                        py / h as f64
                    };
                    let (lo, hi) = stops
                        .windows(2)
                        .find_map(|pair| (along <= pair[1].0).then_some((pair[0], pair[1])))
                        .unwrap();
                    let f = ((along - lo.0) / (hi.0 - lo.0)).clamp(0., 1.);
                    let mut c =
                        std::array::from_fn::<_, 4, _>(|i| lo.1[i] + (hi.1[i] - lo.1[i]) * f);
                    let edge = if look == 1 { 2. } else { 1. };
                    let curved_edge = if look == 1 {
                        let r = (h as f64 / 2.).min(w as f64 / 2.);
                        let dx = px - px.clamp(r, w as f64 - r);
                        let dy = py - py.clamp(r, h as f64 - r);
                        dx * dx + dy * dy > (r - edge).powi(2)
                    } else {
                        false
                    };
                    if curved_edge
                        || px < edge
                        || py < edge
                        || px > w as f64 - edge
                        || py > h as f64 - edge
                    {
                        c = border;
                    } else if look == 1 && py < edge + 2. {
                        for channel in c.iter_mut().take(3) {
                            *channel = *channel * 0.15 + 255. * 0.85;
                        }
                    }
                    // The eternal button's ::before is a translucent amber
                    // edge, composited over the glass before antialiasing.
                    if look == 2 && !selected && px < 3. {
                        let behind = c[3] / 255.;
                        let a = 0.55 + behind * 0.45;
                        let edge = [255., 154., 31.];
                        for i in 0..3 {
                            c[i] = (edge[i] * 0.55 + c[i] * behind * 0.45) / a;
                        }
                        c[3] = a * 255.;
                    }
                    let alpha = c[3] / 255.;
                    for i in 0..3 {
                        premult[i] += c[i] * alpha / 16.;
                    }
                    premult[3] += alpha / 16.;
                }
            }
            let at = (y * w + x) * 4;
            if premult[3] > 0. {
                for i in 0..3 {
                    out[at + i] = (premult[i] / premult[3]).round() as u8;
                }
                out[at + 3] = (premult[3] * 255.).round() as u8;
            }
        }
    }
    out
}
#[cfg(test)]
mod tests {
    #[test]
    fn eternal_has_clipped_corners_and_an_amber_gradient() {
        let p = super::button(368, 40, 2, true);
        assert_eq!(p[3], 0);
        assert_eq!(p[(20 * 368 + 180) * 4 + 3], 255);
        assert!(p[(20 * 368 + 40) * 4 + 1] > p[(20 * 368 + 320) * 4 + 1]);
    }
}

/// The look's scanlines/blueprint grid with the page's elliptical mask.
pub fn pattern(w: usize, h: usize, look: usize) -> Vec<u8> {
    let mut out = vec![0; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let r = (((x as f64 / w as f64 - 0.5) / 0.30).powi(2)
                + ((y as f64 / h as f64 - 0.5) / 0.62).powi(2))
            .sqrt();
            let mask = if r < 0.6 {
                1. - r / 0.6 * 0.5
            } else {
                ((1. - r) / 0.4 * 0.5).max(0.)
            };
            let (rgb, alpha) = match look {
                0 => (
                    [150, 205, 255],
                    if x % 46 == 0 || y % 46 == 0 {
                        0.16 * 0.55
                    } else {
                        0.
                    },
                ),
                1 => (
                    [190, 215, 255],
                    if x % 160 == 0 || y % 160 == 0 {
                        0.2 * 0.8
                    } else if x % 32 == 0 || y % 32 == 0 {
                        0.09 * 0.8
                    } else {
                        0.
                    },
                ),
                _ => ([255, 255, 255], if y % 4 == 0 { 0.035 } else { 0. }),
            };
            let at = (y * w + x) * 4;
            out[at..at + 3].copy_from_slice(&rgb);
            out[at + 3] = (255. * alpha * mask).round() as u8;
        }
    }
    out
}

/// PageTheme's battle overlay: looks.css scrim over overlay, then the masked
/// pattern from page/style.js. Rasterize once at admission, like the buttons,
/// so scanlines do not exhaust the shared HUD quad budget every paused frame.
pub fn pause_backdrop(w: usize, h: usize, look: usize) -> Vec<u8> {
    let (base, base_alpha, center, edge, alphas) = match look {
        0 => (
            [4., 16., 46.],
            0.5,
            [6., 26., 70.],
            [4., 18., 52.],
            [0.9, 0.74],
        ),
        1 => (
            [8., 20., 42.],
            0.5,
            [16., 38., 74.],
            [12., 30., 60.],
            [0.92, 0.76],
        ),
        _ => ([3., 4., 7.], 0.52, [3., 4., 7.], [3., 4., 7.], [0.93, 0.78]),
    };
    let mut pixels = vec![0; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let nx = (x as f64 + 0.5) / w as f64 - 0.5;
            let ny = (y as f64 + 0.5) / h as f64 - 0.5;
            let r = (nx / 0.30).hypot(ny / 0.78);
            let t = (r * 2.).min(1.);
            let alpha = if r < 0.5 {
                alphas[0] + (alphas[1] - alphas[0]) * t
            } else {
                alphas[1] * ((1. - r) * 2.).clamp(0., 1.)
            };
            let mut a = alpha + base_alpha * (1. - alpha);
            let mut rgb: [f64; 3] = std::array::from_fn(|i| {
                (center[i] + (edge[i] - center[i]) * t) * alpha
                    + base[i] * base_alpha * (1. - alpha)
            });
            // Pattern mask: ellipse 30% 62%, black at 0, half at 60%,
            // transparent at the edge. These are CSS pixels in this viewport.
            let mask_r = (nx / 0.30).hypot(ny / 0.62);
            let mask = if mask_r < 0.6 {
                1. - mask_r / 0.6 * 0.5
            } else {
                ((1. - mask_r) / 0.4 * 0.5).clamp(0., 0.5)
            };
            let grid = |step| x % step == 0 || y % step == 0;
            let (pattern, color) = match look {
                0 => (if grid(46) { 0.16 * 0.55 } else { 0. }, [150., 205., 255.]),
                1 => {
                    let major = if grid(160) { 0.2 } else { 0. };
                    let minor = if grid(32) { 0.09 } else { 0. };
                    ((major + minor * (1. - major)) * 0.8, [190., 215., 255.])
                }
                _ => (if y % 4 == 0 { 0.035 } else { 0. }, [255.; 3]),
            };
            let pattern = pattern * mask;
            for i in 0..3 {
                rgb[i] = color[i] * pattern + rgb[i] * (1. - pattern);
            }
            a = pattern + a * (1. - pattern);
            let at = (y * w + x) * 4;
            for i in 0..3 {
                pixels[at + i] = (rgb[i] / a).round().clamp(0., 255.) as u8;
            }
            pixels[at + 3] = (a * 255.).round() as u8;
        }
    }
    pixels
}
