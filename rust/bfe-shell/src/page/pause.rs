//! Page overlay over the live/frozen battle. PauseMenu owns keys and actions.
use super::Page;
use crate::render::ShellTextures;
use bfe_battle_ui::{
    pause::{PauseHitTarget, PauseHotspot, PauseMenu, PauseScreenState},
    shapes::BattleUiSurface,
};
use bfe_render::interface::{Batch, Matrix};
impl Page {
    #[allow(clippy::too_many_arguments)]
    pub fn draw_pause(
        &mut self,
        b: &mut Batch,
        t: &ShellTextures,
        menu: &mut PauseMenu,
        surface: &dyn BattleUiSurface,
        w: f64,
        h: f64,
        theme: &str,
    ) {
        self.viewport = [w, h];
        self.hits.clear();
        self.modal_start = None;
        let ih = surface.height();
        let mut m = bfe_battle_ui::shapes::root_anchor(ih);
        m[0] *= 1024. / w;
        m[5] *= ih / h;
        let white = [0.91, 0.93, 0.95, 1.];
        let look = match theme {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        let ink = if look == 1 {
            white
        } else {
            [0.09, 0.05, 0.01, 1.]
        };
        let accent = if look == 1 {
            [0.82, 0.16, 0.11, 1.]
        } else {
            [1., 0.60, 0.12, 1.]
        };
        let mut hits = std::mem::take(&mut self.pause_hits);
        hits.clear();
        if !menu.is_open {
            if menu.waiting_time.is_some_and(|v| v >= 0.15) {
                self.label(
                    b,
                    m,
                    w / 2.,
                    h / 2.,
                    "CLICK TO RESUME",
                    15.,
                    4.8,
                    [1., 1., 1., 0.5 + 0.5 * menu.pulse()],
                    2,
                    false,
                );
            }
            menu.set_theme_hotspots(&mut hits);
            self.pause_hits = hits;
            return;
        }
        // The client admits the same scrim/pattern as PageTheme.overlay;
        // the live battle has already gone through its six-pixel blur.
        Self::rect(b, m, [0., 0., w, h], self.pause_backdrops[look], [1.; 4]);
        self.brand(b, m, t.solid, w, h, 0., theme, ink);
        let hotspot = |r: [f64; 4], target| PauseHotspot {
            target,
            corners: [
                [r[0] * 1024. / w, r[1] * ih / h],
                [r[0] * 1024. / w, r[3] * ih / h],
                [r[2] * 1024. / w, r[3] * ih / h],
                [r[2] * 1024. / w, r[1] * ih / h],
            ],
        };
        let bw = (w * 0.30).min(400.) - 16.;
        let confirm = matches!(menu.top(), Some(PauseScreenState::Confirm { .. }));
        if matches!(
            menu.top(),
            Some(PauseScreenState::Main | PauseScreenState::Confirm { .. })
        ) {
            if self.pause_rows.is_empty() {
                self.pause_rows = menu.visible_rows(surface);
            }
            let rows = std::mem::take(&mut self.pause_rows);
            let (top, heading_y, bh, gap) = Self::row_layout(h, rows.len(), look);
            self.heading(
                b,
                m,
                t,
                w / 2.,
                heading_y,
                "GAME PAUSED",
                (h * 0.046).clamp(26., 50.),
                look,
            );
            for (i, (_, label)) in rows.iter().enumerate() {
                let y = top + i as f64 * (bh + gap);
                let r = [(w - bw) / 2., y, (w + bw) / 2., y + bh];
                self.widget_button(
                    b,
                    m,
                    t,
                    r,
                    label,
                    look,
                    i == menu.focused_row && !menu.back_focused,
                    false,
                );
                if !confirm {
                    hits.push(hotspot(r, PauseHitTarget::MainRow(i)));
                }
            }
            self.pause_rows = rows;
        }
        if confirm {
            let r = [w * 0.03, h * 0.923, w * 0.03 + 140., h * 0.97];
            self.widget_button(b, m, t, r, "BACK", look, false, false);
            self.modal_start = Some(b.count);
            Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.5]);
        }
        match menu.top() {
            Some(PauseScreenState::Main) => {}
            Some(PauseScreenState::Confirm {
                message,
                focused_button,
                ..
            }) => {
                let width = 700f64.min(w * 0.92);
                let size = (h * 0.021).clamp(15., 21.);
                let paragraph = self.paragraph_index(message, width - w * 0.06, size, size * 0.06);
                let height =
                    self.paragraphs[paragraph].lines.len() as f64 * size * 1.45 + h * 0.112 + 38.;
                let y = (h - height) / 2.;
                Self::panel(
                    b,
                    m,
                    t,
                    [(w - width) / 2., y, (w + width) / 2., y + height],
                    look,
                );
                self.paragraph(
                    b,
                    m,
                    w / 2.,
                    y + h * 0.045 + size,
                    message,
                    width - w * 0.06,
                    size,
                    size * 0.06,
                    white,
                );
                for (i, label) in ["YES", "NO"].iter().enumerate() {
                    let button_w = (w * 0.11).clamp(120., 200.);
                    let button_h = (h * 0.046).clamp(34., 46.);
                    let x = w / 2. + (i as f64 - 0.5) * (button_w + w * 0.012);
                    let by = y + height - h * 0.035 - button_h - 1.;
                    let r = [x - button_w / 2., by, x + button_w / 2., by + button_h];
                    self.widget_button(b, m, t, r, label, look, i == *focused_button, false);
                    hits.push(hotspot(r, PauseHitTarget::ConfirmButton(i)));
                }
            }
            Some(PauseScreenState::Options { focused_row }) => {
                let labels = [
                    "GAME OPTIONS",
                    "VIDEO OPTIONS",
                    "AUDIO OPTIONS",
                    "CONTROL OPTIONS",
                    "ONLINE OPTIONS",
                ];
                self.pause_option_rows(b, m, t, w, h, look, *focused_row, &labels, &mut hits, ih);
            }
            Some(PauseScreenState::PlayerList { selected, first }) => {
                self.label(
                    b,
                    m,
                    w / 2.,
                    h * 0.115,
                    "PLAYER LIST",
                    32.,
                    4.,
                    white,
                    2,
                    true,
                );
                Self::panel(b, m, t, [w * 0.14, h * 0.20, w * 0.86, h * 0.84], look);
                for (i, row) in menu.players.iter().enumerate().skip(*first).take(12) {
                    let y = h * 0.22 + (i - *first) as f64 * 32.;
                    let r = [w * 0.16, y, w * 0.84, y + 30.];
                    if i == *selected {
                        Self::rect(b, m, r, t.solid, [accent[0], accent[1], accent[2], 0.25]);
                    }
                    self.label(
                        b,
                        m,
                        r[0] + 12.,
                        y + 22.,
                        &row.name,
                        15.,
                        0.6,
                        white,
                        0,
                        false,
                    );
                    hits.push(hotspot(r, PauseHitTarget::PlayerRow(i)));
                }
            }
            None => {}
        }
        {
            self.picker(
                b,
                m,
                t.solid,
                w,
                look,
                accent,
                ink,
                [0.02, 0.03, 0.04, 1.],
                white,
            );
        }
        if !confirm {
            let r = [w * 0.03, h * 0.923, w * 0.03 + 140., h * 0.97];
            self.widget_button(b, m, t, r, "BACK", look, menu.back_focused, false);
            hits.push(hotspot(r, PauseHitTarget::BackButton));
        }
        menu.set_theme_hotspots(&mut hits);
        self.pause_hits = hits;
    }
    #[allow(clippy::too_many_arguments)]
    fn pause_option_rows(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        w: f64,
        h: f64,
        look: usize,
        focused: usize,
        labels: &[&str],
        hits: &mut Vec<PauseHotspot>,
        ih: f64,
    ) {
        let bh = (h * 0.056).clamp(38., 56.);
        let gap = (h * 0.012).clamp(8., 14.);
        let bw = (w * 0.30).min(400.) - 16.;
        let top = h / 2. - (labels.len() as f64 * (bh + gap) - gap) / 2.;
        self.label(
            b,
            m,
            w / 2.,
            top - h * 0.05,
            "OPTIONS",
            32.,
            4.,
            [0.91, 0.93, 0.95, 1.],
            2,
            true,
        );
        for (i, label) in labels.iter().enumerate() {
            let y = top + i as f64 * (bh + gap);
            let r = [(w - bw) / 2., y, (w + bw) / 2., y + bh];
            self.widget_button(b, m, t, r, label, look, i == focused, false);
            hits.push(PauseHotspot {
                target: PauseHitTarget::OptionRow(i),
                corners: [
                    [r[0] * 1024. / w, r[1] * ih / h],
                    [r[0] * 1024. / w, r[3] * ih / h],
                    [r[2] * 1024. / w, r[3] * ih / h],
                    [r[2] * 1024. / w, r[1] * ih / h],
                ],
            });
        }
    }
}
