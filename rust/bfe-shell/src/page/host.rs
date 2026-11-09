//! The native host configuration uses the page's wide options arrangement.
//! Values and activation remain in ShellApp's existing HostDialog owner.
use super::{Hit, Page};
use crate::{
    render::ShellTextures,
    screens::{ScreenId, ShellApp},
};
use bfe_render::interface::{Batch, Color, Matrix};
impl Page {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn host_draw(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        a: &ShellApp,
        w: f64,
        h: f64,
        accent: Color,
        _ink: Color,
        white: Color,
    ) {
        let look = match a.settings.menu_theme.as_str() {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        self.heading(
            b,
            m,
            t,
            w / 2.,
            h * 0.109,
            "SET GAME OPTIONS",
            (h * 0.046).clamp(26., 50.),
            look,
        );
        let width = (w * 0.94).min(1000.);
        let left = (w - width) / 2.;
        let top = h * 0.19;
        let bottom = h * 0.84;
        Self::panel(b, m, t, [left, top, left + width, bottom], look);
        let rh = (h * 0.048).clamp(36., 50.);
        let shown = ((bottom - top - 32.) / rh).floor().max(1.) as usize;
        let selected = a.selected_cached_row_index(ScreenId::HostDialog);
        let first = selected.saturating_sub(shown).max(1);
        for (i, row) in a.active_rows().iter().enumerate().skip(first).take(shown) {
            let y = top + 16. + (i - first) as f64 * rh;
            let r = [left + 16., y, left + width - 16., y + rh];
            if i == selected {
                Self::rect(b, m, r, t.solid, [accent[0], accent[1], accent[2], 0.2]);
            }
            let (label, value) = row.label.split_once(':').unwrap_or((&row.label, ""));
            self.label(
                b,
                m,
                r[0] + 14.,
                y + rh * 0.65,
                label,
                13.,
                1.3,
                white,
                0,
                false,
            );
            self.ellipsized(
                b,
                m,
                w / 2. + 20.,
                y + rh * 0.65,
                value.trim(),
                width / 2. - 50.,
                13.,
                0.8,
                white,
            );
            self.hits.push((r, Hit::Row(i)));
        }
        let edge = (w * 0.03).clamp(20., 56.);
        let y = h * 0.923;
        let back = [edge, y, edge + 140., h * 0.97];
        self.widget_button(b, m, t, back, "BACK", look, false, false);
        self.hits.push((back, Hit::Back));
        let launch = [w - edge - 200., y, w - edge, h * 0.97];
        self.widget_button(b, m, t, launch, "LAUNCH", look, selected == 0, false);
        self.hits.push((launch, Hit::Row(0)));
    }
}
