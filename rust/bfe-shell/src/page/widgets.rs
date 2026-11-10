//! Paint the existing native menu widgets with the page layouts from
//! src/menus/themes/page. Widget state, navigation and actions stay in ScreenStack.
use super::Page;
use crate::render::ShellTextures;
use bfe_render::interface::{Batch, Matrix};

impl Page {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn widget_button(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        label: &str,
        look: usize,
        selected: bool,
        disabled: bool,
    ) {
        let mut r = r;
        if look == 1 && selected {
            let cx = (r[0] + r[2]) / 2.;
            let cy = (r[1] + r[3]) / 2.;
            r = [
                cx + (r[0] - cx) * 1.03,
                cy + (r[1] - cy) * 1.03,
                cx + (r[2] - cx) * 1.03,
                cy + (r[3] - cy) * 1.03,
            ];
        }
        let small = (r[3] - r[1]) < (self.viewport[1] * 0.056).clamp(38., 56.) - 1.;
        let surface = self.buttons[look * 2 + usize::from(selected)];
        let color = if disabled { [1., 1., 1., 0.4] } else { [1.; 4] };
        if surface != 0 {
            let cap = match look {
                0 => 3.,
                1 => self.button_source[1] / 2.,
                _ => 14.,
            };
            let target_cap = if look == 2 && (r[3] - r[1]) < 38. {
                10.
            } else {
                cap
            };
            let dx = target_cap.min((r[2] - r[0]) / 2.);
            let dy = target_cap.min((r[3] - r[1]) / 2.);
            let xs = [r[0], r[0] + dx, r[2] - dx, r[2]];
            let ys = [r[1], r[1] + dy, r[3] - dy, r[3]];
            let us = [
                0.,
                cap / self.button_source[0],
                1. - cap / self.button_source[0],
                1.,
            ];
            let vs = [
                0.,
                cap / self.button_source[1],
                1. - cap / self.button_source[1],
                1.,
            ];
            for x in 0..3 {
                for y in 0..3 {
                    if xs[x + 1] > xs[x] && ys[y + 1] > ys[y] {
                        b.quad(
                            m,
                            [xs[x], ys[y], xs[x + 1], ys[y + 1]],
                            surface,
                            [us[x], vs[y], us[x + 1], vs[y + 1]],
                            color,
                            0.,
                            0,
                        );
                    }
                }
            }
        } else {
            Self::rect(b, m, r, t.solid, [0.05, 0.06, 0.08, 1.]);
        }
        let ink = if selected && look != 1 {
            [0.09, 0.05, 0.01, color[3]]
        } else {
            [0.91, 0.93, 0.95, color[3]]
        };
        if look == 1 {
            if !self.button_text.contains_key(label) {
                let chars = {
                    let mut first = true;
                    let mut output = String::with_capacity(label.len());
                    for ch in label.chars() {
                        if first {
                            output.extend(ch.to_uppercase());
                        } else {
                            output.extend(ch.to_lowercase());
                        }
                        first = ch.is_whitespace();
                    }
                    output.encode_utf16().collect()
                };
                self.button_text.insert(label.into(), chars);
            }
            let chars = &self.button_text[label];
            let size = if small {
                (self.viewport[1] * 0.019).clamp(14., 19.)
            } else {
                (self.viewport[1] * 0.022).clamp(15., 23.)
            };
            let x = (r[0] + r[2]) / 2.;
            let y = r[1] + (r[3] - r[1]) * 0.64;
            for (dx, dy) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.), (0., 2.)] {
                Self::draw_text(
                    &self.bold,
                    b,
                    m,
                    x + dx,
                    y + dy,
                    chars,
                    [0.06, 0.06, 0.06, color[3]],
                    2,
                    size,
                    size * 0.02,
                );
            }
            Self::draw_text(&self.bold, b, m, x, y, chars, ink, 2, size, size * 0.02);
        } else {
            let size = if small {
                (self.viewport[1] * 0.017).clamp(13., 18.)
            } else {
                // page/style.js's .bfe-page button { font: inherit } wins
                // over the full-size .bfe-btn clamp in Eternal and BF1.
                // Small buttons and BF2 have more specific size rules.
                16.
            };
            self.label(
                b,
                m,
                (r[0] + r[2]) / 2.,
                r[1] + (r[3] - r[1]) * 0.64,
                label,
                size,
                size * if look == 0 {
                    0.16
                } else if small {
                    0.20
                } else {
                    0.24
                },
                ink,
                2,
                false,
            );
        }
    }
}
