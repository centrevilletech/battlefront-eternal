//! Paint the existing native menu widgets with the page layouts from
//! src/menus/themes/page. Widget state, navigation and actions stay in ScreenStack.
use super::Page;
use crate::render::ShellTextures;
use bfe_interface::{
    options::OptionKind,
    stack::ScreenStack,
    theme::{HitRegion, HitTarget},
    widgets::{TextLook, Widget},
};
use bfe_render::interface::{anchor, Batch, Matrix, UV};

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
                (self.viewport[1] * 0.02).clamp(14., 21.)
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

    // Retain hit records rather than allocating widget identifiers per frame.
    fn widget_hit(
        hits: &mut Vec<HitRegion>,
        used: &mut usize,
        m: Matrix,
        rect: [f64; 4],
        target: HitTarget,
    ) {
        if *used == hits.len() {
            hits.push(HitRegion { m, rect, target });
        } else {
            hits[*used] = HitRegion { m, rect, target };
        }
        *used += 1;
    }
    #[allow(clippy::too_many_arguments)]
    pub fn paint_widgets(
        &mut self,
        b: &mut Batch,
        t: &ShellTextures,
        stack: &mut ScreenStack,
        w: f64,
        h: f64,
        look: &str,
        hits: &mut Vec<HitRegion>,
    ) {
        let mut m = anchor(0., 0., false, h);
        m[0] *= 1024. / w;
        let look = match look {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        let white = [0.91, 0.93, 0.95, 1.];
        let accent = match look {
            0 => [1., 0.64, 0.16, 1.],
            1 => [0.82, 0.16, 0.11, 1.],
            _ => [1., 0.60, 0.12, 1.],
        };
        let edge = (w * 0.03).clamp(20., 56.);
        let width = (w - 2. * edge).min(1000.);
        let left = (w - width) / 2.;
        let start = stack.screens.iter().rposition(|s| !s.modal).unwrap_or(0);
        let mut used = 0;
        let screen_count = stack.screens.len();
        if let Some(top) = stack.top() {
            if self.widget_screen != top.id {
                if !self.widget_screen.is_empty() {
                    std::mem::swap(&mut self.widget_leaving, &mut self.widget_snapshot);
                }
                self.widget_screen.clone_from(&top.id);
                self.widget_forward = screen_count >= self.widget_depth;
                self.widget_depth = screen_count;
            }
            super::motion::leaving(b, &self.widget_leaving, top.elapsed, self.widget_forward);
        }
        self.widget_spans.clear();
        for (si, screen) in stack.screens.iter_mut().enumerate().skip(start) {
            let active = si + 1 == screen_count;
            let modal = screen.modal;
            if modal {
                self.modal_start = Some(b.count);
                Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.5]);
                used = 0;
            }
            let has_panel = screen
                .widgets
                .iter()
                .any(|v| matches!(v, Widget::Options(_) | Widget::List(_)));
            let other = screen.widgets.iter().any(|v| matches!(v, Widget::List(_)));
            let rh = (h * 0.048).clamp(36., 50.);
            let rows = screen
                .widgets
                .iter()
                .filter_map(|widget| {
                    if let Widget::Options(options) = widget {
                        Some(options.items.iter().filter(|i| !i.hidden).count())
                    } else {
                        None
                    }
                })
                .sum::<usize>();
            let body_h = if other {
                h * 0.67
            } else {
                (rows as f64 * rh + h * 0.048 + 2.).min(h * 0.67)
            };
            let panel_y = (h * 0.07 + (h - h * 0.06 - (h * 0.046).clamp(34., 46.))) / 2.
                + (h * 0.046 * 1.3 + h * 0.036) / 2.
                - (body_h + 26.) / 2.;
            let top = if modal {
                h * 0.36
            } else if has_panel {
                panel_y + h * 0.024
            } else {
                h * 0.20
            };
            let bottom = if modal {
                h * 0.66
            } else if has_panel {
                panel_y + body_h - h * 0.024
            } else {
                h * 0.84
            };
            let screen_first = b.count;
            if has_panel || modal {
                Self::panel(
                    b,
                    m,
                    t,
                    [
                        left,
                        if modal { top - 18. } else { panel_y },
                        left + width,
                        if modal {
                            bottom + 18.
                        } else {
                            panel_y + body_h
                        },
                    ],
                    look,
                );
            }
            let mut flow = top;
            for (wi, widget) in screen.widgets.iter_mut().enumerate() {
                if widget.is_hidden() {
                    continue;
                }
                let focused = screen.focused == Some(wi) && active;
                match widget {
                    Widget::Rows(rows) => {
                        let n = rows.rows.iter().filter(|r| !r.hidden).count();
                        let (y, heading_y, bh, gap) = Self::row_layout(h, n, look);
                        let bw = (w * 0.30).min(400.) - 16.;
                        let x = (w - bw) / 2.;
                        self.heading(
                            b,
                            m,
                            t,
                            w / 2.,
                            heading_y,
                            &rows.title,
                            (h * 0.046).clamp(26., 50.),
                            look,
                        );
                        for (row, item) in rows.rows.iter().filter(|r| !r.hidden).enumerate() {
                            let r = [
                                x,
                                y + row as f64 * (bh + gap),
                                x + bw,
                                y + row as f64 * (bh + gap) + bh,
                            ];
                            let first = b.count;
                            self.widget_button(
                                b,
                                m,
                                t,
                                r,
                                &item.label,
                                look,
                                focused && row == rows.selected,
                                item.disabled,
                            );
                            let progress = ((screen.elapsed - 0.12 - row.min(10) as f64 * 0.038)
                                / 0.42)
                                .clamp(0., 1.);
                            let eased = super::motion::ease(progress, 0.2, 0.8, 0.2, 1.);
                            self.widget_spans.push(super::motion::RowSpan {
                                first,
                                end: b.count,
                                index: row,
                                selected: focused && row == rows.selected,
                            });
                            if progress < 1. {
                                for q in first..b.count {
                                    for v in 0..4 {
                                        let at = q * 4 + v;
                                        b.position[at * 3] += ((1. - eased) * 1024. * 0.06) as f32;
                                        b.tint[at * 4 + 3] *= eased as f32;
                                    }
                                }
                                b.start = b.start.min(first * 4);
                                b.end = b.end.max(b.count * 4);
                            }
                            if active && !item.disabled {
                                Self::widget_hit(
                                    hits,
                                    &mut used,
                                    m,
                                    r,
                                    HitTarget::Row {
                                        widget_index: wi,
                                        row_index: row,
                                    },
                                );
                            }
                        }
                    }
                    Widget::Text(text) => {
                        let (y, size, spacing, ink) = match text.look {
                            TextLook::Title => (h * 0.108, (h * 0.046).clamp(26., 50.), 2., white),
                            TextLook::Note => (h * 0.89, 12., 1., accent),
                            _ => (if modal { h * 0.46 } else { h * 0.15 }, 16., 0.5, white),
                        };
                        if modal && text.look != TextLook::Title {
                            self.paragraph(
                                b,
                                m,
                                w / 2.,
                                y,
                                &text.text,
                                width - 2. * w * 0.03,
                                size,
                                spacing,
                                ink,
                            );
                            continue;
                        }
                        for (line, s) in text.text.lines().enumerate() {
                            self.label(
                                b,
                                m,
                                w / 2.,
                                y + line as f64 * (size * 1.5),
                                s,
                                size,
                                spacing,
                                ink,
                                2,
                                text.look == TextLook::Title,
                            );
                        }
                    }
                    Widget::Button(button) => {
                        let size = (h * 0.017).clamp(13., 18.);
                        let bw = (w * 0.11)
                            .clamp(120., 200.)
                            .max(self.text_width(&button.text, size, size * 0.2) + size * 2.8);
                        let bh = (h * 0.046).clamp(34., 46.);
                        let x = match button.place {
                            "back" => {
                                if has_panel {
                                    left
                                } else {
                                    edge
                                }
                            }
                            "accept" => {
                                if has_panel {
                                    left + width - bw
                                } else {
                                    w - edge - bw
                                }
                            }
                            _ => (w - bw) / 2.,
                        };
                        let y = h - h * 0.03 - bh;
                        let r = [x, y, x + bw, y + bh];
                        self.widget_button(
                            b,
                            m,
                            t,
                            r,
                            &button.text,
                            look,
                            focused,
                            button.disabled,
                        );
                        if active && !button.disabled {
                            Self::widget_hit(
                                hits,
                                &mut used,
                                m,
                                r,
                                HitTarget::Button { widget_index: wi },
                            );
                        }
                    }
                    Widget::Options(options) => {
                        let rh = (h * 0.048).clamp(36., 50.);
                        let visible = options.items.iter().filter(|r| !r.hidden).count();
                        let shown = if other {
                            visible
                        } else {
                            ((bottom - flow) / rh).floor() as usize
                        }
                        .max(1);
                        options.shown = shown;
                        let first = options.first.min(visible.saturating_sub(shown));
                        options.first = first;
                        for (display, (i, item)) in options
                            .items
                            .iter()
                            .enumerate()
                            .filter(|(_, r)| !r.hidden)
                            .skip(first)
                            .take(shown)
                            .enumerate()
                        {
                            let y = flow + display as f64 * rh;
                            let r = [left + w * 0.022, y, left + width - w * 0.022, y + rh];
                            let selected = focused && i == options.selected;
                            if selected {
                                Self::gradient(
                                    b,
                                    m,
                                    r,
                                    t.solid,
                                    UV,
                                    [
                                        [accent[0], accent[1], accent[2], 0.20],
                                        [accent[0], accent[1], accent[2], 0.20],
                                        [accent[0], accent[1], accent[2], 0.03],
                                        [accent[0], accent[1], accent[2], 0.03],
                                    ],
                                );
                            }
                            let c = if item.disabled || item.dim {
                                [white[0], white[1], white[2], 0.42]
                            } else if selected && look != 0 {
                                accent
                            } else {
                                white
                            };
                            let label_ink = [
                                white[0],
                                white[1],
                                white[2],
                                if item.disabled {
                                    0.28
                                } else if selected {
                                    1.
                                } else {
                                    0.66
                                },
                            ];
                            if selected {
                                Self::rect(b, m, [r[0], y, r[0] + 3., y + rh], t.solid, accent);
                            }
                            self.label(
                                b,
                                m,
                                r[0] + 15.,
                                y + rh * 0.65,
                                &item.label,
                                13.,
                                1.56,
                                label_ink,
                                0,
                                false,
                            );
                            let choice_width =
                                117f64.max(self.text_width(&item.value_text, 13., 1.04));
                            let mut slider = None;
                            if item.kind == OptionKind::Slider {
                                let x1 = r[2] - 92.;
                                let x0 = (x1 - 300.).max(left + width * 0.55);
                                let cy = y + rh * 0.5;
                                Self::rect(
                                    b,
                                    m,
                                    [x0, cy - 4., x1, cy + 4.],
                                    t.solid,
                                    [1., 1., 1., 0.14],
                                );
                                Self::rect(
                                    b,
                                    m,
                                    [x0, cy - 4., x0 + (x1 - x0) * item.ratio, cy + 4.],
                                    t.solid,
                                    accent,
                                );
                                let x = x0 + (x1 - x0) * item.ratio;
                                Self::rect(
                                    b,
                                    m,
                                    [x - 7., cy - 11., x + 7., cy + 11.],
                                    t.solid,
                                    white,
                                );
                                slider = Some([x0, x1]);
                            }
                            if item.label.is_empty() {
                                let action_width =
                                    self.text_width(&item.value_text, 13., 1.82) + 2. * 13. * 1.3;
                                let ar = [
                                    w / 2. - action_width / 2.,
                                    y + 1.,
                                    w / 2. + action_width / 2.,
                                    y + rh - 1.,
                                ];
                                if selected {
                                    Self::rect(b, m, ar, t.solid, accent);
                                } else {
                                    for line in [
                                        [ar[0], ar[1], ar[2], ar[1] + 1.],
                                        [ar[0], ar[3] - 1., ar[2], ar[3]],
                                        [ar[0], ar[1], ar[0] + 1., ar[3]],
                                        [ar[2] - 1., ar[1], ar[2], ar[3]],
                                    ] {
                                        Self::rect(b, m, line, t.solid, accent);
                                    }
                                }
                                self.label(
                                    b,
                                    m,
                                    w / 2.,
                                    y + rh * 0.65,
                                    &item.value_text,
                                    13.,
                                    1.82,
                                    if selected {
                                        if look == 0 {
                                            white
                                        } else {
                                            [0.03, 0.03, 0.03, 1.]
                                        }
                                    } else {
                                        accent
                                    },
                                    2,
                                    false,
                                );
                            } else {
                                self.label(
                                    b,
                                    m,
                                    if item.label.is_empty() {
                                        w / 2.
                                    } else {
                                        r[2] - w * 0.012
                                            - if matches!(
                                                item.kind,
                                                OptionKind::Choice | OptionKind::Toggle
                                            ) {
                                                22.5 + choice_width / 2.
                                            } else {
                                                0.
                                            }
                                    },
                                    y + rh * 0.65,
                                    &item.value_text,
                                    13.,
                                    1.04,
                                    c,
                                    if matches!(item.kind, OptionKind::Choice | OptionKind::Toggle)
                                    {
                                        2
                                    } else {
                                        1
                                    },
                                    false,
                                );
                            }
                            if !item.label.is_empty()
                                && matches!(item.kind, OptionKind::Choice | OptionKind::Toggle)
                            {
                                let xr = r[2] - w * 0.012 - 8.;
                                let xl = xr - 29. - choice_width;
                                for (x, sign) in [(xl, -1.), (xr, 1.)] {
                                    for offset in -5i32..=5 {
                                        let dx = (offset.abs() as f64) * -sign;
                                        Self::rect(
                                            b,
                                            m,
                                            [
                                                x + dx,
                                                y + rh / 2. + offset as f64,
                                                x + dx + 1.5,
                                                y + rh / 2. + offset as f64 + 1.,
                                            ],
                                            t.solid,
                                            if selected { accent } else { [1., 1., 1., 0.4] },
                                        );
                                    }
                                }
                                if active && !item.disabled {
                                    self.hits.push((
                                        [xl - 12., y, xl + 12., y + rh],
                                        super::Hit::OptionBack(wi, i),
                                    ));
                                }
                            }
                            Self::rect(
                                b,
                                m,
                                [r[0], r[3] - 1., r[2], r[3]],
                                t.solid,
                                [1., 1., 1., 0.06],
                            );
                            if active && !item.disabled {
                                Self::widget_hit(
                                    hits,
                                    &mut used,
                                    m,
                                    r,
                                    HitTarget::OptionsItem {
                                        widget_index: wi,
                                        item_index: i,
                                        slider_x: slider,
                                    },
                                );
                            }
                            if selected && !item.note.is_empty() {
                                self.label(
                                    b,
                                    m,
                                    w / 2.,
                                    panel_y + body_h + h * 0.032,
                                    &item.note,
                                    12.,
                                    1.,
                                    accent,
                                    2,
                                    false,
                                );
                            }
                        }
                        flow += (visible.min(shown) as f64) * rh + h * 0.01;
                    }
                    Widget::Tabs(tabs) => {
                        let tw = width / tabs.tabs.len().max(1) as f64;
                        let bh = 34.;
                        for (i, tab) in tabs.tabs.iter().enumerate() {
                            let r = [
                                left + i as f64 * tw,
                                flow,
                                left + (i + 1) as f64 * tw,
                                flow + bh,
                            ];
                            self.widget_button(
                                b,
                                m,
                                t,
                                r,
                                &tab.label,
                                look,
                                tab.id == tabs.active,
                                false,
                            );
                            if active && !tabs.disabled {
                                Self::widget_hit(
                                    hits,
                                    &mut used,
                                    m,
                                    r,
                                    HitTarget::Tab {
                                        widget_index: wi,
                                        tab_index: i,
                                    },
                                );
                            }
                        }
                        flow += bh + 8.;
                    }
                    Widget::List(list) => {
                        let rh = (h * 0.04).clamp(30., 42.);
                        let shown = ((bottom - flow) / rh).floor().max(1.) as usize;
                        list.look.box_h = shown as f64 * list.look.pitch + list.look.inset * 2.;
                        list.shown = shown;
                        let first = list.first.min(list.rows.len().saturating_sub(shown));
                        for (i, row) in list.rows.iter().enumerate().skip(first).take(shown) {
                            let y = flow + (i - first) as f64 * rh;
                            let r = [left + 16., y, left + width - 16., y + rh];
                            if i == list.selected {
                                Self::rect(
                                    b,
                                    m,
                                    r,
                                    t.solid,
                                    [accent[0], accent[1], accent[2], 0.20],
                                );
                            }
                            self.label(
                                b,
                                m,
                                r[0] + 14.,
                                y + rh * 0.68,
                                &row.cells[0],
                                12.,
                                0.96,
                                white,
                                0,
                                false,
                            );
                            self.label(
                                b,
                                m,
                                r[2] - 14.,
                                y + rh * 0.68,
                                &row.cells[1],
                                12.,
                                0.96,
                                white,
                                1,
                                false,
                            );
                            if active && !row.disabled {
                                Self::widget_hit(
                                    hits,
                                    &mut used,
                                    m,
                                    r,
                                    HitTarget::ListRow {
                                        widget_index: wi,
                                        row_index: i,
                                        slider_x: None,
                                    },
                                );
                            }
                        }
                    }
                    Widget::HorizontalButtons(buttons) => {
                        for (i, button) in buttons.buttons.iter().enumerate() {
                            let x = w / 2.
                                + (i as f64 - (buttons.buttons.len() - 1) as f64 / 2.) * 180.;
                            let r = [x - 80., h * 0.56, x + 80., h * 0.56 + 38.];
                            self.widget_button(
                                b,
                                m,
                                t,
                                r,
                                &button.label,
                                look,
                                focused && buttons.selected == i,
                                button.disabled,
                            );
                            if active && !button.disabled {
                                Self::widget_hit(
                                    hits,
                                    &mut used,
                                    m,
                                    r,
                                    HitTarget::HorizontalButton {
                                        widget_index: wi,
                                        button_index: i,
                                    },
                                );
                            }
                        }
                    }
                    Widget::OptionRow(row) => {
                        self.label(
                            b,
                            m,
                            left + 25.,
                            flow + 25.,
                            &row.label,
                            13.,
                            1.56,
                            white,
                            0,
                            false,
                        );
                        self.label(
                            b,
                            m,
                            left + width - 25.,
                            flow + 25.,
                            &row.value_text,
                            13.,
                            1.04,
                            white,
                            1,
                            false,
                        );
                        flow += 36.;
                    }
                    Widget::Panel(_) => {}
                }
            }
            if has_panel && !modal {
                self.widget_spans.push(super::motion::RowSpan {
                    first: screen_first,
                    end: b.count,
                    index: 0,
                    selected: false,
                });
                let p = ((screen.elapsed - 0.08) / 0.55).clamp(0., 1.);
                if p < 1. {
                    let e = super::motion::ease(p, 0.2, 0.8, 0.2, 1.);
                    for q in screen_first..b.count {
                        for corner in 0..4 {
                            let at = q * 4 + corner;
                            b.position[at * 3] *= (0.97 + 0.03 * e) as f32;
                            b.position[at * 3 + 1] = b.position[at * 3 + 1]
                                * (0.97 + 0.03 * e) as f32
                                - ((1. - e) * h * 0.02) as f32;
                            b.tint[at * 4 + 3] *= e as f32;
                        }
                    }
                    b.start = b.start.min(screen_first * 4);
                    b.end = b.end.max(b.count * 4);
                }
            }
        }
        super::motion::retain_rows(b, &self.widget_spans, &mut self.widget_snapshot);
        hits.truncate(used);
        if stack.top().is_some_and(|screen| screen.modal) {
            self.hits.clear();
            self.dropdown = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_interface::{OptionsContext, OptionsController};
    use bfe_render::interface::Font;
    #[test]
    fn page_pointer_activates_the_existing_options_widget_action() {
        let mut controller = OptionsController::new(Default::default(), OptionsContext::default());
        let mut page = Page::new(Font::default(), Font::default());
        let mut batch = Batch::default();
        let mut hits = Vec::new();
        page.paint_widgets(
            &mut batch,
            &ShellTextures::default(),
            &mut controller.stack,
            1280.,
            720.,
            "eternal",
            &mut hits,
        );
        let r = hits
            .iter()
            .find(|h| matches!(h.target, HitTarget::Row { row_index: 0, .. }))
            .unwrap()
            .rect;
        let x = (r[0] + r[2]) / 2.;
        let y = (r[1] + r[3]) / 2.;
        let event =
            controller
                .input
                .on_pointer_down(&mut controller.stack, &hits, x, y, 1280., 720.);
        controller.apply_event(event);
        assert_eq!(controller.stack.top().unwrap().id, "options-game");
    }
}
