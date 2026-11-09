//! Look-specific title treatment from electron/ui/looks.css.
use super::Page;
use crate::render::ShellTextures;
use bfe_render::interface::{Batch, Matrix, UV};
impl Page {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn heading(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        x: f64,
        y: f64,
        label: &str,
        size: f64,
        look: usize,
    ) {
        if !self.text.contains_key(label){self.text.insert(label.into(),label.encode_utf16().collect());}
        let chars=&self.text[label];
        let font_size = size
            * match look {
                0 => 0.78,
                1 => 0.74,
                _ => 1.,
            };
        let width = self.title.width(chars, font_size / 64., font_size * 0.06);
        let text_y = if look == 2 { y } else { y - 3. };
        // Eternal and BF2 title spans explicitly use white in looks.css,
        // rather than the softer body-text color used by menu rows.
        let white = [1.; 4];
        if look == 2 {
            let color = |a| [1., 0.60, 0.12, a];
            let r = [
                x - width / 2.,
                y + size * 0.30,
                x + width / 2.,
                y + size * 0.30 + 3.,
            ];
            Self::gradient(
                b,
                m,
                [r[0], r[1], r[0] + width * 0.25, r[3]],
                t.solid,
                UV,
                [color(0.), color(0.), color(1.), color(1.)],
            );
            Self::rect(
                b,
                m,
                [r[0] + width * 0.25, r[1], r[0] + width * 0.75, r[3]],
                t.solid,
                color(1.),
            );
            Self::gradient(
                b,
                m,
                [r[0] + width * 0.75, r[1], r[2], r[3]],
                t.solid,
                UV,
                [color(1.), color(1.), color(0.), color(0.)],
            );
        } else {
            let pad = font_size * if look == 0 { 1.3 } else { 1.2 };
            let left = x - width / 2. - pad;
            let right = x + width / 2. + pad;
            let top = y - font_size * 1.10;
            let bottom = y + font_size * 0.27;
            for row in 0..(bottom - top).ceil() as usize {
                let yy = top + row as f64;
                let p = (yy - top) / (bottom - top);
                let inset = if look == 0 { font_size * (1. - p) } else { 0. };
                let a = if look == 0 {
                    [1., 0.82, 0.48, 1.]
                } else {
                    [0.89, 0.29, 0.24, 1.]
                };
                let z = if look == 0 {
                    [0.96, 0.46, 0., 1.]
                } else {
                    [0.65, 0.09, 0.05, 1.]
                };
                let c = std::array::from_fn(|i| a[i] + (z[i] - a[i]) * p);
                Self::rect(
                    b,
                    m,
                    [left, yy, right - inset, (yy + 1.).min(bottom)],
                    t.solid,
                    c,
                );
            }
        }
        self.label(
            b,
            m,
            x,
            text_y,
            label,
            font_size,
            font_size * 0.06,
            if look == 0 {
                [0.075, 0.125, 0.29, 1.]
            } else {
                white
            },
            2,
            true,
        );
    }
}

impl Page {
    pub(super) fn row_layout(h: f64, n: usize, look: usize) -> (f64, f64, f64, f64) {
        let gap = match look {
            0 => (h * 0.01).clamp(7., 12.),
            1 => (h * 0.013).clamp(8., 15.),
            _ => (h * 0.012).clamp(8., 14.),
        };
        let bh = (h * 0.056).clamp(38., 56.);
        let title = (h * 0.046).clamp(26., 50.);
        let header = title
            * match look {
                0 => 0.78 * 1.4,
                1 => 0.74 * 1.5,
                _ => 1.3,
            };
        let rows = n as f64 * (bh + gap) - gap + 8.;
        let foot = (h * 0.046).clamp(34., 46.);
        let top = h * 0.07 + (h - h * 0.07 - h * 0.06 - foot - header - h * 0.05 - rows) / 2.;
        (top + header + h * 0.05 + 4., top + title * 0.84, bh, gap)
    }
}
