//! hostDialog (src/shell/host-dialog.js), in the page theme's dialog chrome.
//! The modal owns no launch settings: every painted control edits ShellApp.
use super::{forms::FormFace, Hit, Page};
use crate::{
    render::ShellTextures,
    screens::{HostControl, ShellApp},
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
        ink: Color,
        white: Color,
    ) {
        // The compositor blurs the browser before painting this modal range.
        self.modal_start = Some(b.count);
        self.hits.clear();
        Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.6]);
        let width = 747f64.min(w * 0.92);
        let height = 624f64.min(h * 0.94);
        let x = (w - width) / 2.;
        let y = (h - height) / 2.;
        let sx = width / 747.;
        let sy = height / 624.;
        let rect = |l: f64, top: f64, r: f64, bottom: f64| {
            [x + l * sx, y + top * sy, x + r * sx, y + bottom * sy]
        };
        Self::rect(
            b,
            m,
            rect(0., 0., 747., 624.),
            t.solid,
            Self::dialog_fill(&a.settings.menu_theme),
        );
        Self::form_border(b, m, t, rect(0., 0., 747., 624.), [1., 1., 1., 0.12]);
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 79. * sy,
            "HOST A BATTLE",
            24. * sy,
            1.44 * sy,
            white,
            0,
            FormFace::Bold,
        );
        let rows = [
            (0, "Server name", 137., rect(143., 110., 361., 154.)),
            (1, "Visibility", 194., rect(112., 166., 250., 210.)),
            (2, "Map", 251., rect(80., 223., 300., 267.)),
            (3, "Side", 308., rect(80., 280., 186., 324.)),
            (
                4,
                "Human players (total)",
                365.,
                rect(215., 337., 285., 381.),
            ),
            (5, "Connection", 469., rect(132., 440., 270., 484.)),
        ];
        for (field, label, baseline, r) in rows {
            self.form_label(
                b,
                m,
                x + 32. * sx,
                y + baseline * sy,
                label,
                17. * sy,
                0.,
                white,
                0,
                FormFace::Regular,
            );
            let value = match field {
                0 => a.host_config.server_name.clone(),
                4 => a.host_form.players.clone(),
                _ => a
                    .host_choices(field)
                    .into_iter()
                    .find(|(value, _)| value == a.host_value(field))
                    .map(|(_, label)| label)
                    .unwrap_or_default(),
            };
            self.form_field(
                b,
                m,
                t,
                r,
                &value,
                a.host_form.focus == field,
                accent,
                white,
                sy,
            );
            let action = if field == 0 || field == 4 {
                HostControl::Field(field)
            } else {
                HostControl::Select(field)
            };
            if field != 0 && field != 4 {
                self.form_label(
                    b,
                    m,
                    r[2] - 12. * sx,
                    r[1] + 27. * sy,
                    "v",
                    16. * sy,
                    0.,
                    white,
                    2,
                    FormFace::Regular,
                );
            }
            self.hits.push((r, Hit::Host(action)));
        }
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 416. * sy,
            "Up to 32 per team, including bots. Human players replace bots.",
            17. * sy,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        let status = a.host_form.error.as_deref().unwrap_or(
            "Hosting uses this computer and your imported game. It keeps running when minimized.",
        );
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 520. * sy,
            status,
            17. * sy,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        for (r, label, action, accept) in [
            (
                rect(33., 552., 186., 595.),
                "HOST AND JOIN",
                HostControl::Start,
                true,
            ),
            (
                rect(194., 552., 278., 595.),
                "CLOSE",
                HostControl::Cancel,
                false,
            ),
        ] {
            self.form_button(
                b,
                m,
                t,
                r,
                label,
                accept,
                ink,
                white,
                sy,
                &a.settings.menu_theme,
            );
            self.hits.push((r, Hit::Host(action)));
        }
        // Native selects retain browser ordering and scroll long map lists.
        if let Some(field) = a.host_form.choice {
            let r = rows.iter().find(|(f, _, _, _)| *f == field).unwrap().3;
            let choices = a.host_choices(field);
            let shown = choices.len().min(10);
            let first = a
                .host_form
                .choice_cursor
                .saturating_sub(shown.saturating_sub(1));
            let top = r[3].min(h - shown as f64 * 30. * sy - 12.);
            for (index, (_, label)) in choices.iter().enumerate().skip(first).take(shown) {
                let rr = [
                    r[0],
                    top + (index - first) as f64 * 30. * sy,
                    r[2].max(r[0] + 220. * sx),
                    top + (index - first + 1) as f64 * 30. * sy,
                ];
                Self::rect(
                    b,
                    m,
                    rr,
                    t.solid,
                    if index == a.host_form.choice_cursor {
                        accent
                    } else {
                        [0.04, 0.06, 0.09, 1.]
                    },
                );
                self.form_label(
                    b,
                    m,
                    rr[0] + 10.,
                    rr[1] + 21. * sy,
                    label,
                    16. * sy,
                    0.,
                    if index == a.host_form.choice_cursor {
                        ink
                    } else {
                        white
                    },
                    0,
                    FormFace::Medium,
                );
                self.hits.push((rr, Hit::Host(HostControl::Choice(index))));
            }
        }
    }
}
