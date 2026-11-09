//! Desktop forms retain their shell-owned editing fields and accept actions.
use super::{Hit, Page};
use crate::{render::ShellTextures, screens::ShellApp};
use bfe_render::interface::{Batch, Color, Matrix};
pub(super) enum FormFace {
    Regular,
    Medium,
    Bold,
    Button,
}
impl Page {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn form_label(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        spacing: f64,
        color: Color,
        align: u32,
        face: FormFace,
    ) {
        if !self.text.contains_key(text) {
            self.text.insert(text.into(), text.encode_utf16().collect());
        }
        let font = match face {
            FormFace::Regular => &self.regular,
            FormFace::Medium => &self.medium,
            FormFace::Bold => &self.bold,
            FormFace::Button => &self.body,
        };
        Self::draw_text(
            font,
            b,
            m,
            x,
            y,
            &self.text[text],
            color,
            align,
            size,
            spacing,
        );
    }

    /// Shared text/select chrome; focused fields match the native editor owner.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn form_field(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        value: &str,
        focus: bool,
        accent: Color,
        white: Color,
        scale: f64,
    ) {
        Self::rect(b, m, r, t.solid, [0.02, 0.03, 0.04, 1.]);
        let color = if focus { accent } else { [1., 1., 1., 0.22] };
        Self::form_border(b, m, t, r, color);
        self.form_label(
            b,
            m,
            r[0] + 10.,
            r[1] + (r[3] - r[1]) * 0.67,
            value,
            16. * scale,
            0.,
            white,
            0,
            FormFace::Medium,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn direct_form(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        a: &ShellApp,
        w: f64,
        h: f64,
        _look: usize,
        accent: Color,
        ink: Color,
        white: Color,
    ) {
        self.modal_start = Some(b.count);
        self.hits.clear();
        Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.6]);
        let width = 688f64.min(w * 0.92);
        let height = 257.;
        let x = (w - width) / 2.;
        let y = (h - height) / 2.;
        let sx = width / 688.;
        Self::rect(
            b,
            m,
            [x, y, x + width, y + height],
            t.solid,
            Self::dialog_fill(&a.settings.menu_theme),
        );
        Self::form_border(b, m, t, [x, y, x + width, y + height], [1., 1., 1., 0.12]);
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 83.,
            "DIRECT CONNECTION",
            27.,
            1.62,
            white,
            0,
            FormFace::Bold,
        );
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 143.,
            "Server IPv4 or IPv6 address",
            18.,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        self.form_label(
            b,
            m,
            x + 497. * sx,
            y + 143.,
            "Port",
            18.,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        for (field, r, value) in [
            (
                0,
                [x + 274. * sx, y + 116., x + 492. * sx, y + 160.],
                a.direct_connect_draft.as_str(),
            ),
            (
                1,
                [x + 544. * sx, y + 116., x + 656. * sx, y + 160.],
                a.direct_port_draft.as_str(),
            ),
        ] {
            self.form_field(
                b,
                m,
                t,
                r,
                if field == 0 && value.is_empty() {
                    "192.0.2.10"
                } else {
                    value
                },
                a.direct_field == field,
                accent,
                if field == 0 && value.is_empty() {
                    [0.45, 0.45, 0.45, 1.]
                } else {
                    white
                },
                1.,
            );
            self.hits.push((r, Hit::DirectField(field)));
        }
        for (accept, left, right, label) in
            [(true, 33., 138., "CONNECT"), (false, 147., 242., "CANCEL")]
        {
            let r = [x + left * sx, y + 185., x + right * sx, y + 228.];
            self.form_button(
                b,
                m,
                t,
                r,
                label,
                accept,
                ink,
                white,
                1.,
                &a.settings.menu_theme,
            );
            self.hits.push((r, Hit::DirectButton(accept)));
        }
    }

    pub(super) fn dialog_fill(theme: &str) -> Color {
        match theme {
            "bf1" => [0.04, 0.10, 0.23, 1.],
            "bf2" => [0.06, 0.13, 0.25, 1.],
            _ => [0.047, 0.059, 0.082, 1.],
        }
    }
    pub(super) fn form_border(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        color: Color,
    ) {
        for line in [
            [r[0], r[1], r[2], r[1] + 1.],
            [r[0], r[3] - 1., r[2], r[3]],
            [r[0], r[1], r[0] + 1., r[3]],
            [r[2] - 1., r[1], r[2], r[3]],
        ] {
            Self::rect(b, m, line, t.solid, color);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn form_button(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        label: &str,
        accept: bool,
        ink: Color,
        white: Color,
        scale: f64,
        theme: &str,
    ) {
        if accept {
            let look = match theme {
                "bf1" => 0,
                "bf2" => 1,
                _ => 2,
            };
            Self::accent_fill(b, m, t, r, look);
        } else {
            Self::rect(b, m, r, t.solid, [1., 1., 1., 0.06]);
            Self::form_border(b, m, t, r, [1., 1., 1., 0.22]);
        }
        self.form_label(
            b,
            m,
            (r[0] + r[2]) / 2.,
            r[1] + 27. * scale,
            label,
            13. * scale,
            1.3 * scale,
            if accept { ink } else { white },
            2,
            FormFace::Button,
        );
    }
    /// onlineNotice in src/shell/online-notice.js uses ordinary paragraphs,
    /// keeping the review modal readable instead of a centered menu row list.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn online_notice_form(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        a: &ShellApp,
        w: f64,
        h: f64,
        message: &str,
        ink: Color,
        white: Color,
    ) {
        self.hits.clear();
        Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.6]);
        let width = 600f64.min(w - 32.);
        let height = 379f64.min(h - 32.);
        let x = (w - width) / 2.;
        let y = (h - height) / 2.;
        Self::rect(
            b,
            m,
            [x, y, x + width, y + height],
            t.solid,
            Self::dialog_fill(&a.settings.menu_theme),
        );
        Self::form_border(b, m, t, [x, y, x + width, y + height], [1., 1., 1., 0.12]);
        self.form_label(
            b,
            m,
            x + 32.,
            y + 43.,
            "ONLINE PRIVACY",
            13.,
            1.3,
            [0.58, 0.80, 0.91, 1.],
            0,
            FormFace::Regular,
        );
        self.form_label(
            b,
            m,
            x + 32.,
            y + 83.,
            "BEFORE YOU PLAY ONLINE",
            28.,
            1.68,
            white,
            0,
            FormFace::Bold,
        );
        let mut baseline = y + 128.;
        for paragraph in message.split("\n\n").skip(1) {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                let units: Vec<_> = candidate.encode_utf16().collect();
                if !line.is_empty()
                    && self.regular.width(&units, 17. / self.regular.height, 0.) > width - 64.
                {
                    self.form_label(
                        b,
                        m,
                        x + 32.,
                        baseline,
                        &line,
                        17.,
                        0.,
                        white,
                        0,
                        FormFace::Regular,
                    );
                    baseline += 25.5;
                    line = word.into();
                } else {
                    line = candidate;
                }
            }
            self.form_label(
                b,
                m,
                x + 32.,
                baseline,
                &line,
                17.,
                0.,
                white,
                0,
                FormFace::Regular,
            );
            baseline += 41.5;
        }
        for (index, r, label, accept) in [
            (1, [x + 33., y + 302., x + 286., y + 350.], "NOT NOW", false),
            (
                0,
                [x + 306., y + 302., x + 560., y + 350.],
                "ACCEPT AND CONTINUE",
                true,
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
                1.,
                &a.settings.menu_theme,
            );
            self.hits.push((r, Hit::Popup(index)));
        }
    }

    /// requireProfile's modal has its message above a single inline input row.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn multiplayer_profile_form(
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
        self.hits.clear();
        Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.6]);
        let width = 692f64.min(w * 0.94);
        let x = (w - width) / 2.;
        let y = (h - 244.) / 2.;
        let sx = width / 692.;
        Self::rect(
            b,
            m,
            [x, y, x + width, y + 244.],
            t.solid,
            Self::dialog_fill(&a.settings.menu_theme),
        );
        Self::form_border(b, m, t, [x, y, x + width, y + 244.], [1., 1., 1., 0.12]);
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 83.,
            "PROFILE",
            24.,
            1.44,
            white,
            0,
            FormFace::Bold,
        );
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 137.,
            &a.profile.message,
            17.,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        self.form_label(
            b,
            m,
            x + 32. * sx,
            y + 197.,
            "Player name",
            17.,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        let input = [x + 149. * sx, y + 170., x + 367. * sx, y + 214.];
        self.form_field(b, m, t, input, &a.profile.draft, true, accent, white, 1.);
        for (save, l, r, label) in [(true, 367., 444., "SAVE"), (false, 451., 547., "CANCEL")] {
            let rect = [x + l * sx, y + 170., x + r * sx, y + 214.];
            self.form_button(
                b,
                m,
                t,
                rect,
                label,
                save,
                ink,
                white,
                1.,
                &a.settings.menu_theme,
            );
            self.hits.push((rect, Hit::Profile(save)));
        }
    }
    pub(super) fn accent_fill(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        look: usize,
    ) {
        let stops: &[(f64, Color)] = match look {
            0 => &[
                (0., [1., 210. / 255., 122. / 255., 1.]),
                (0.45, [1., 164. / 255., 42. / 255., 1.]),
                (1., [245. / 255., 117. / 255., 0., 1.]),
            ],
            1 => &[
                (0., [240. / 255., 103. / 255., 90. / 255., 1.]),
                (0.48, [207. / 255., 42. / 255., 29. / 255., 1.]),
                (0.52, [165. / 255., 24. / 255., 14. / 255., 1.]),
                (1., [216. / 255., 72. / 255., 58. / 255., 1.]),
            ],
            _ => &[
                (0., [1., 190. / 255., 85. / 255., 1.]),
                (0.60, [1., 148. / 255., 25. / 255., 1.]),
                (1., [245. / 255., 117. / 255., 0., 1.]),
            ],
        };
        for pair in stops.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            let segment = if look == 2 {
                [
                    r[0] + (r[2] - r[0]) * lo.0,
                    r[1],
                    r[0] + (r[2] - r[0]) * hi.0,
                    r[3],
                ]
            } else {
                [
                    r[0],
                    r[1] + (r[3] - r[1]) * lo.0,
                    r[2],
                    r[1] + (r[3] - r[1]) * hi.0,
                ]
            };
            Self::gradient(
                b,
                m,
                segment,
                t.solid,
                bfe_render::interface::UV,
                if look == 2 {
                    [lo.1, lo.1, hi.1, hi.1]
                } else {
                    [lo.1, hi.1, lo.1, hi.1]
                },
            );
        }
    }
}
