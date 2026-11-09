//! First-run page from electron/ui/prepare.html and prepare.css. It shares
//! the front end's typefaces, logo, panel and button surfaces before game data.
use super::Page;
use crate::render::ShellTextures;
use bfe_render::interface::{anchor, Batch, UV};

pub struct SetupProgress {
    pub fraction: f64,
    pub label: String,
    pub numbers: String,
    pub phases: [u8; 7],
    pub show_phases: bool,
}
pub struct SetupSound {
    pub action: usize,
    pub checked: bool,
    pub description: String,
}
#[derive(Default)]
pub struct SetupView {
    pub title: String,
    pub detail: String,
    pub facts: Vec<(String, String)>,
    pub issues: Vec<String>,
    pub actions: Vec<String>,
    pub progress: Option<SetupProgress>,
    pub code: Option<String>,
    pub sound: Option<SetupSound>,
    pub listing: bool,
    pub busy: bool,
    pub logs: Vec<String>,
    pub link_action: Option<usize>,
    pub warning: bool,
    pub error: bool,
}

impl Page {
    /// Rectangles returned here are also the input targets, in window pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_setup(
        &mut self,
        b: &mut Batch,
        t: &ShellTextures,
        v: &SetupView,
        w: f64,
        h: f64,
        look: usize,
        selected: usize,
        time: f64,
    ) -> Vec<[f64; 4]> {
        b.begin();
        self.viewport = [w, h];
        let mut m = anchor(0., 0., false, 768.);
        m[0] = 1024. / w;
        m[5] = -768. / h;
        Self::rect(
            b,
            m,
            [0., 0., w, h],
            t.solid,
            [3. / 255., 4. / 255., 6. / 255., 1.],
        );
        if self.setup_backdrops[look] != 0 {
            b.quad(
                m,
                [0., 0., w, h],
                self.setup_backdrops[look],
                UV,
                [1.; 4],
                0.,
                0,
            );
        }
        let listing = v.listing;
        let top = h * if listing { 0.06 } else { 0.13 };
        let logo_size = (w * 0.05).min(h * 0.09).min(96.);
        let chars: Vec<_> = "BATTLEFRONT".encode_utf16().collect();
        let width = self.logo.width(&chars, logo_size / 64., -logo_size * 0.01);
        let x = (w - width) / 2.;
        let baseline = top + logo_size * 0.78;
        self.word(b, m, x, baseline, "BATTLEFRONT", logo_size, 0, look);
        let tag_size = logo_size * 0.25;
        let tag_chars: Vec<_> = "ETERNAL".encode_utf16().collect();
        let tag_w = self.title.width(&tag_chars, tag_size / 64., tag_size * 0.5) + tag_size * 3.3;
        let tag_right = x + width - tag_size * 0.1;
        let tag_y = top + logo_size * 0.945;
        for row in 0..(tag_size * 1.56).ceil() as usize {
            let fraction = row as f64 / (tag_size * 1.56);
            let c = [
                1. - 0.04 * fraction,
                0.82 - 0.36 * fraction,
                0.48 * (1. - fraction),
                1.,
            ];
            Self::rect(
                b,
                m,
                [
                    tag_right - tag_w,
                    tag_y + row as f64,
                    tag_right - tag_size * 1.5 * (1. - fraction),
                    tag_y + row as f64 + 1.,
                ],
                t.solid,
                c,
            );
        }
        Self::draw_text(
            &self.title,
            b,
            m,
            tag_right - tag_w + tag_size * 0.9,
            tag_y + tag_size * 1.13,
            &tag_chars,
            [0.10, 0.063, 0.016, 1.],
            0,
            tag_size,
            tag_size * 0.5,
        );
        let panel_w = 860f64.min(w - (w * 0.03).clamp(20., 56.) * 2.);
        let left = (w - panel_w) / 2.;
        let pad = (w * 0.026).clamp(20., 40.);
        let inner = panel_w - 2. * pad;
        let panel_y = top + logo_size * 1.334 + h * if listing { 0.03 } else { 0.06 };
        let title_size = (h * 0.03).clamp(19., 29.);
        let body_size = (h * 0.0185).clamp(14., 17.);
        let title_y = panel_y + (h * 0.034).clamp(18., 34.) + title_size;
        let detail_y = title_y + title_size * 0.46 + (h * 0.02).clamp(12., 20.) + body_size;
        let lines = balanced_lines(&self.prose, &v.detail, inner, body_size, body_size * 0.015);
        let detail_h = lines.len() as f64 * body_size * 1.55;
        let detail_bottom = detail_y + detail_h - body_size - 1.2;
        let code_lines = v
            .code
            .as_ref()
            .map(|text| wrapped_lines(&self.mono, text, inner, body_size * 0.8, 0.))
            .unwrap_or_default();
        let code_height = if code_lines.is_empty() {
            0.
        } else {
            body_size * 0.35 + code_lines.len() as f64 * body_size * 0.8 * 1.45
        };
        let content_top = detail_bottom
            + code_height
            + if v.progress.is_some() {
                (h * 0.024).clamp(14., 24.)
            } else {
                (h * 0.02).clamp(12., 20.)
            };
        let fact_label_width = v
            .facts
            .iter()
            .map(|(name, _)| {
                self.body
                    .width(&name.encode_utf16().collect::<Vec<_>>(), 11. / 64., 2.42)
            })
            .fold(0., f64::max);
        let fact_offset = fact_label_width + 18.;
        let field_width = inner - fact_offset - if listing { 15. } else { 0. };
        let fact_lines: Vec<_> = v
            .facts
            .iter()
            .map(|(name, value)| {
                wrapped_lines(
                    if name == "FOLDER" {
                        &self.mono
                    } else {
                        &self.regular
                    },
                    value,
                    field_width,
                    13.,
                    if name == "FOLDER" { 0. } else { 0.195 },
                )
            })
            .collect();
        let mut content_bottom = content_top;
        for lines in &fact_lines {
            content_bottom += lines.len().max(1) as f64 * 19.5 + 6.;
        }
        if !fact_lines.is_empty() {
            content_bottom -= 6.;
        }
        let issue_lines: Vec<_> = v
            .issues
            .iter()
            .enumerate()
            .map(|(i, text)| {
                wrapped_lines(
                    &self.regular,
                    text,
                    inner
                        - 15.
                        - if i < v.issues.len().saturating_sub(2) {
                            20.
                        } else {
                            0.
                        },
                    13.,
                    0.195,
                )
            })
            .collect();
        for (i, lines) in issue_lines.iter().enumerate() {
            content_bottom += if i == 0 {
                14.
            } else if i + 2 == issue_lines.len() {
                10.
            } else {
                4.
            };
            content_bottom += lines.len() as f64 * 19.5;
        }
        let sound_lines = v
            .sound
            .as_ref()
            .map(|s| wrapped_lines(&self.regular, &s.description, inner - 45., 13., 0.195))
            .unwrap_or_default();
        let sound_height = if v.sound.is_some() {
            22. + 19.5 + 2. + sound_lines.len() as f64 * 19.5
        } else {
            0.
        };
        if v.sound.is_some() {
            content_bottom += 14. + sound_height;
        }
        if let Some(progress) = &v.progress {
            content_bottom += if progress.show_phases { 81. } else { 42. };
        }
        // The facts keep their CSS bottom margin when a pending check has
        // neither problems nor a sound option beneath them yet.
        if listing && v.busy && v.sound.is_none() {
            content_bottom += 14.;
        }
        let visible_actions =
            v.actions.len() - usize::from(v.sound.is_some()) - usize::from(v.link_action.is_some());
        let action_margin = if visible_actions > 0 {
            (h * 0.028).clamp(16., 26.)
        } else {
            0.
        };
        let y = if listing || v.progress.is_some() {
            content_bottom + action_margin
        } else {
            detail_bottom + action_margin
        };
        let gap = (h * 0.012).clamp(8., 14.);
        let bh = (h * 0.046).clamp(34., 46.);
        let fs = (h * 0.017).clamp(13., 18.);
        let mut rows: Vec<Vec<(usize, f64)>> = vec![Vec::new()];
        let mut used = 0.;
        for (i, label) in v.actions.iter().enumerate() {
            if v.sound.as_ref().is_some_and(|s| s.action == i) || v.link_action == Some(i) {
                continue;
            }
            let chars: Vec<_> = label.encode_utf16().collect();
            let bw = (self.body.width(&chars, fs / 64., fs * 0.2) + fs * 2.8 + 2.)
                .max((w * 0.11).clamp(120., 200.));
            if used + bw > inner && !rows.last().unwrap().is_empty() {
                rows.push(Vec::new());
                used = 0.;
            }
            rows.last_mut().unwrap().push((i, bw));
            used += bw + gap;
        }
        let button_y = y;
        let actions_height = if visible_actions > 0 {
            rows.len() as f64 * (bh + gap) - gap
        } else {
            0.
        };
        let details_height = if v.link_action.is_some() {
            32. + if v.logs.is_empty() {
                0.
            } else {
                30. + (v.logs.len() as f64 * 17.25 + 20.).min(h * 0.22)
            }
        } else {
            0.
        };
        let panel_bottom =
            (y + actions_height + details_height + (h * 0.03).clamp(18., 30.)).min(h - 24.);
        Self::panel(b, m, t, [left, panel_y, left + panel_w, panel_bottom], look);
        let title_color = if v.error {
            [1., 0.416, 0.353, 1.]
        } else if v.warning {
            [1., 0.702, 0.278, 1.]
        } else {
            [1.; 4]
        };
        let title: Vec<_> = v.title.encode_utf16().collect();
        Self::draw_text(
            &self.title,
            b,
            m,
            w / 2.,
            title_y - 4.5,
            &title,
            title_color,
            2,
            title_size,
            title_size * 0.06,
        );
        Self::rect(
            b,
            m,
            [
                left + pad,
                title_y + title_size * 0.32,
                left + panel_w - pad,
                title_y + title_size * 0.32 + 1.,
            ],
            t.solid,
            [1., 1., 1., 0.12],
        );
        for (row, line) in code_lines.iter().enumerate() {
            Self::draw_text(
                &self.mono,
                b,
                m,
                w / 2.,
                detail_bottom
                    + body_size * 0.35
                    + body_size * 0.8
                    + row as f64 * body_size * 0.8 * 1.45,
                line,
                [0.91, 0.925, 0.949, 0.52],
                2,
                body_size * 0.8,
                0.,
            );
        }
        for (row, line) in lines.iter().enumerate() {
            Self::draw_text(
                &self.prose,
                b,
                m,
                w / 2.,
                detail_y + row as f64 * body_size * 1.55,
                line,
                [0.91, 0.925, 0.949, 0.8],
                2,
                body_size,
                body_size * 0.015,
            );
        }
        let mut fy = content_top;
        for ((name, _), lines) in v.facts.iter().zip(&fact_lines) {
            let a: Vec<_> = name.encode_utf16().collect();
            Self::draw_text(
                &self.body,
                b,
                m,
                left + pad,
                fy + 12.,
                &a,
                [0.91, 0.925, 0.949, 0.4],
                0,
                11.,
                2.42,
            );
            let font = if name == "FOLDER" {
                &self.mono
            } else {
                &self.regular
            };
            let color = if name == "CHECK" && v.error {
                [1., 0.416, 0.353, 1.]
            } else {
                [0.91, 0.925, 0.949, 1.]
            };
            for (row, line) in lines.iter().enumerate() {
                Self::draw_text(
                    font,
                    b,
                    m,
                    left + pad + fact_offset,
                    fy + 14. + row as f64 * 19.5,
                    line,
                    color,
                    0,
                    13.,
                    0.195,
                );
            }
            fy += lines.len().max(1) as f64 * 19.5 + 6.;
        }
        if !fact_lines.is_empty() {
            fy -= 6.;
        }
        for (i, lines) in issue_lines.iter().enumerate() {
            fy += if i == 0 {
                14.
            } else if i + 2 == issue_lines.len() {
                10.
            } else {
                4.
            };
            let bullet = i < v.issues.len().saturating_sub(2);
            let color = if i + 1 == issue_lines.len() {
                [0.91, 0.925, 0.949, 0.4]
            } else if i + 2 == issue_lines.len() {
                [0.91, 0.925, 0.949, 0.66]
            } else {
                [0.91, 0.925, 0.949, 1.]
            };
            if bullet {
                Self::draw_text(
                    &self.regular,
                    b,
                    m,
                    left + pad + 4.,
                    fy + 14.,
                    &[0x2022],
                    color,
                    0,
                    13.,
                    0.195,
                );
            }
            for (row, line) in lines.iter().enumerate() {
                Self::draw_text(
                    &self.regular,
                    b,
                    m,
                    left + pad + if bullet { 20. } else { 0. },
                    fy + 14. + row as f64 * 19.5,
                    line,
                    color,
                    0,
                    13.,
                    0.195,
                );
            }
            fy += lines.len() as f64 * 19.5;
        }
        let sound_y = fy + 14.;
        if let Some(sound) = &v.sound {
            Self::rect(
                b,
                m,
                [
                    left + pad,
                    sound_y,
                    left + panel_w - pad,
                    sound_y + sound_height,
                ],
                t.solid,
                [0.031, 0.039, 0.059, 0.6],
            );
            let x0 = left + pad;
            let x1 = left + panel_w - pad;
            for r in [
                [x0, sound_y, x1, sound_y + 1.],
                [x0, sound_y + sound_height - 1., x1, sound_y + sound_height],
                [x0, sound_y, x0 + 1., sound_y + sound_height],
                [x1 - 1., sound_y, x1, sound_y + sound_height],
            ] {
                Self::rect(b, m, r, t.solid, [1., 1., 1., 0.10]);
            }
            let check = [
                left + pad + 14.,
                sound_y + 11.,
                left + pad + 32.,
                sound_y + 29.,
            ];
            Self::rect(
                b,
                m,
                check,
                t.solid,
                if sound.checked {
                    [1., 0.60, 0.12, 1.]
                } else {
                    [0.8, 0.8, 0.8, 1.]
                },
            );
            if sound.checked {
                Self::draw_text(
                    &self.body,
                    b,
                    m,
                    check[0] + 3.,
                    check[1] + 14.,
                    &[b'X' as u16],
                    [0.08, 0.05, 0.01, 1.],
                    0,
                    14.,
                    0.,
                );
            }
            let label: Vec<_> = "ENHANCED SOUND".encode_utf16().collect();
            Self::draw_text(
                &self.body,
                b,
                m,
                left + pad + 44.,
                sound_y + 24.,
                &label,
                [0.91, 0.925, 0.949, 1.],
                0,
                13.,
                2.86,
            );
            for (row, line) in sound_lines.iter().enumerate() {
                Self::draw_text(
                    &self.regular,
                    b,
                    m,
                    left + pad + 44.,
                    sound_y + 45.5 + row as f64 * 19.5,
                    line,
                    [0.91, 0.925, 0.949, 0.7],
                    0,
                    13.,
                    0.195,
                );
            }
            fy = sound_y + sound_height;
        }
        if let Some(progress) = &v.progress {
            Self::rect(
                b,
                m,
                [left + pad, fy, left + panel_w - pad, fy + 9.],
                t.solid,
                [1., 1., 1., 0.14],
            );
            Self::rect(
                b,
                m,
                [
                    left + pad,
                    fy,
                    left + pad + inner * progress.fraction.clamp(0., 1.),
                    fy + 9.,
                ],
                t.solid,
                [1., 0.60, 0.12, 1.],
            );
            self.label(
                b,
                m,
                left + pad,
                fy + 31.,
                &progress.label,
                13.,
                1.,
                [0.91, 0.925, 0.949, 1.],
                0,
                false,
            );
            self.label(
                b,
                m,
                left + panel_w - pad,
                fy + 31.,
                &progress.numbers,
                13.,
                0.,
                [0.91, 0.925, 0.949, 0.6],
                1,
                false,
            );
            if progress.show_phases {
                let gap = 8.;
                let cell = (inner - gap * 6.) / 7.;
                for (i, label) in [
                    "GALAXY",
                    "ARMIES",
                    "INTERFACE",
                    "BATTLEFIELDS",
                    "SOUND",
                    "TEXTURES",
                    "MOVIES",
                ]
                .iter()
                .enumerate()
                {
                    let x = left + pad + i as f64 * (cell + gap);
                    let status = progress.phases[i];
                    let color = if status == 2 {
                        [1., 0.60, 0.12, 1.]
                    } else if status == 1 {
                        [1., 0.60, 0.12, 0.5]
                    } else {
                        [1., 1., 1., 0.14]
                    };
                    Self::rect(b, m, [x, fy + 55., x + cell, fy + 58.], t.solid, color);
                    self.label(
                        b,
                        m,
                        x + cell / 2.,
                        fy + 78.,
                        label,
                        9.5,
                        1.14,
                        if status == 2 {
                            [0.91, 0.925, 0.949, 1.]
                        } else {
                            [0.91, 0.925, 0.949, 0.4]
                        },
                        2,
                        false,
                    );
                }
            }
        }

        let mut hits = vec![[0.; 4]; v.actions.len()];
        if let Some(sound) = &v.sound {
            hits[sound.action] = [
                left + pad,
                sound_y,
                left + panel_w - pad,
                sound_y + sound_height,
            ];
        }
        for (row, buttons) in rows.iter().enumerate() {
            let total = buttons.iter().map(|(_, bw)| bw).sum::<f64>()
                + gap * buttons.len().saturating_sub(1) as f64;
            let mut bx = (w - total) / 2.;
            let by = button_y + row as f64 * (bh + gap);
            for &(i, bw) in buttons {
                let r = [bx, by, bx + bw, by + bh];
                hits[i] = r;
                self.widget_button(b, m, t, r, "", look, i == selected, false);
                let text: Vec<_> = v.actions[i].encode_utf16().collect();
                let color = if i == selected && look != 1 {
                    [0.09, 0.05, 0.01, 1.]
                } else {
                    [0.91, 0.93, 0.95, 1.]
                };
                Self::draw_text(
                    &self.body,
                    b,
                    m,
                    (r[0] + r[2]) / 2.,
                    (r[1] + r[3]) / 2. - self.body.caps_middle() * fs / 64.,
                    &text,
                    color,
                    2,
                    fs,
                    fs * 0.2,
                );
                bx += bw + gap;
            }
        }
        if let Some(index) = v.link_action {
            let by = button_y + rows.len() as f64 * (bh + gap) + 8.;
            hits[index] = [w / 2. - 90., by, w / 2. + 90., by + 20.];
            let chars: Vec<_> = v.actions[index].encode_utf16().collect();
            Self::draw_text(
                &self.body,
                b,
                m,
                w / 2.,
                by + 12.,
                &chars,
                if index == selected {
                    [1., 0.68, 0.21, 1.]
                } else {
                    [0.91, 0.93, 0.95, 0.4]
                },
                2,
                11.,
                2.2,
            );
            if !v.logs.is_empty() {
                let log_y = by + 30.;
                let height = (v.logs.len() as f64 * 17.25 + 20.).min(h * 0.22);
                Self::rect(
                    b,
                    m,
                    [left + pad, log_y, left + panel_w - pad, log_y + height],
                    t.solid,
                    [0., 0., 0., 0.6],
                );
                let count = ((height - 20.) / 17.25) as usize;
                for (row, line) in v
                    .logs
                    .iter()
                    .rev()
                    .take(count)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .enumerate()
                {
                    let chars: Vec<_> = line.encode_utf16().collect();
                    Self::draw_text(
                        &self.mono,
                        b,
                        m,
                        left + pad + 12.,
                        log_y + 20. + row as f64 * 17.25,
                        &chars,
                        [0.7, 0.73, 0.77, 1.],
                        0,
                        11.5,
                        0.,
                    );
                }
            }
        }
        if v.busy {
            self.setup_disk(b, m, t.solid, w, h, time, look);
        }
        hits
    }
}

// CSS text-wrap: balance keeps the same minimal line count while reducing
// the widest line. Wrapping is a cold setup layout, with no truncation.
fn balanced_lines(
    font: &bfe_render::interface::Font,
    text: &str,
    width: f64,
    size: f64,
    spacing: f64,
) -> Vec<Vec<u16>> {
    if text.contains('\n') {
        return text
            .split('\n')
            .flat_map(|line| {
                if line.is_empty() {
                    vec![Vec::new()]
                } else {
                    wrapped_lines(font, line, width, size, spacing)
                }
            })
            .collect();
    }
    let wrap = |limit: f64| {
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.into()
            } else {
                format!("{current} {word}")
            };
            let chars: Vec<_> = candidate.encode_utf16().collect();
            if !current.is_empty() && font.width(&chars, size / 64., spacing) > limit {
                lines.push(current.encode_utf16().collect());
                current = word.into();
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            lines.push(current.encode_utf16().collect());
        }
        lines
    };
    let count = wrap(width).len();
    let (mut lo, mut hi) = (width / 2., width);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.;
        if wrap(mid).len() > count {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    wrap(hi)
}

fn wrapped_lines(
    font: &bfe_render::interface::Font,
    text: &str,
    width: f64,
    size: f64,
    spacing: f64,
) -> Vec<Vec<u16>> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.into()
        } else {
            format!("{current} {word}")
        };
        if !current.is_empty()
            && font.width(
                &candidate.encode_utf16().collect::<Vec<_>>(),
                size / 64.,
                spacing,
            ) > width
        {
            lines.push(current.encode_utf16().collect());
            current = word.into();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current.encode_utf16().collect());
    }
    lines
}

impl Page {
    #[allow(clippy::too_many_arguments)]
    fn setup_disk(
        &self,
        b: &mut Batch,
        m: bfe_render::interface::Matrix,
        solid: u32,
        w: f64,
        h: f64,
        time: f64,
        look: usize,
    ) {
        use bfe_render::interface::{rotate_z, scale, translate};
        let size = (w * 0.13).clamp(130., 220.);
        let factor = size / 250.;
        let cx = w - (w * 0.034).clamp(16., 64.) - size / 2.;
        let cy = h - (h * 0.04).clamp(12., 48.) - size / 2.;
        let mut face = scale(translate(m, cx, cy, 0.), factor);
        face[0] *= 0.866;
        // prepare.js drawDisk's two sets of six SVG arc segments.
        let segments = [
            [0.1, 1.0, 74., 118.],
            [1.5, 0.7, 80., 104.],
            [2.6, 1.1, 66., 96.],
            [3.9, 0.5, 78., 122.],
            [4.8, 0.9, 70., 100.],
            [5.7, 0.4, 84., 110.],
        ];
        for (radius, offset, speed) in [
            (1., 0., std::f64::consts::TAU / 18.),
            (0.75, 1., -std::f64::consts::TAU / 11.4),
        ] {
            for [start, length, inner, outer] in segments {
                for n in 0..8 {
                    sector(
                        b,
                        face,
                        solid,
                        start + offset + time * speed + length * n as f64 / 8.,
                        length / 8.,
                        inner * radius,
                        outer * radius,
                        [5. / 255., 7. / 255., 11. / 255., 0.3],
                    );
                }
            }
        }
        for radius in [60., 44.] {
            for n in 0..64 {
                sector(
                    b,
                    face,
                    solid,
                    n as f64 * std::f64::consts::TAU / 64.,
                    std::f64::consts::TAU / 64.,
                    radius - 0.8,
                    radius + 0.8,
                    [196. / 255., 204. / 255., 216. / 255., 0.75],
                );
            }
        }
        let stops = match look {
            0 => ["#0b0d0f", "#34373b", "#5c6166", "#8b8f92", "#b9bcbe"],
            1 => ["#101826", "#3d0c08", "#a5180e", "#e0362a", "#ff6450"],
            _ => ["#15100c", "#4a2407", "#b5560b", "#ff9a1f", "#ffd27a"],
        };
        for i in 0..28 {
            let angle = i as f64 * std::f64::consts::TAU / 28.;
            let led = translate(rotate_z(face, angle), 52., 0., 0.);
            let level = crate::loading::light_level(time / 1.667 - i as f64 / 28.);
            let rgb = crate::loading::mix_color_rgb(&stops, level);
            Self::rect(
                b,
                led,
                [-3.5, -3.5, 3.5, 3.5],
                solid,
                [
                    rgb[0] as f64 / 255.,
                    rgb[1] as f64 / 255.,
                    rgb[2] as f64 / 255.,
                    1.,
                ],
            );
        }
        let label = rotate_z(face, -14f64.to_radians());
        Self::draw_text(
            &self.bold,
            b,
            label,
            0.,
            -self.bold.caps_middle() * 14. / 64.,
            &[76, 79, 65, 68, 73, 78, 71],
            [232. / 255., 236. / 255., 242. / 255., 1.],
            2,
            14.,
            1.4,
        );
    }
}
// Keep the same Batch dirty range while replacing an axis-aligned quad with
// the SVG arc's four exact endpoints. All setup geometry stays in CSS pixels.
#[allow(clippy::too_many_arguments)]
fn sector(
    b: &mut Batch,
    m: bfe_render::interface::Matrix,
    texture: u32,
    start: f64,
    length: f64,
    inner: f64,
    outer: f64,
    color: [f64; 4],
) {
    let q = b.count;
    b.quad(m, [0.; 4], texture, UV, color, 0., 0);
    if b.count == q {
        return;
    }
    for (i, (radius, angle)) in [
        (outer, start),
        (outer, start + length),
        (inner, start),
        (inner, start + length),
    ]
    .into_iter()
    .enumerate()
    {
        let x = radius * angle.cos();
        let y = radius * angle.sin();
        let at = (q * 4 + i) * 3;
        b.position[at] = (m[0] * x + m[4] * y + m[12]) as f32;
        b.position[at + 1] = (m[1] * x + m[5] * y + m[13]) as f32;
    }
}
