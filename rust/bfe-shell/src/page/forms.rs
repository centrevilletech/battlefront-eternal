//! Desktop forms retain their shell-owned editing fields and accept actions.
use super::{Hit, Page};
use crate::{render::ShellTextures, screens::ShellApp};
use bfe_render::interface::{Batch, Color, Matrix};
pub(super) enum FormFace {
    Regular,
    Medium,
    Bold,
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn direct_form(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        a: &ShellApp,
        w: f64,
        h: f64,
        look: usize,
        accent: Color,
        ink: Color,
        white: Color,
    ) {
        self.hits.clear();
        Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.5]);
        let width = 940f64.min(w * 0.92);
        let height = 240.;
        let x = (w - width) / 2.;
        let y = (h - height) / 2.;
        let fill = match look {
            0 => [0.04, 0.10, 0.23, 1.],
            1 => [0.06, 0.13, 0.25, 1.],
            _ => [0.047, 0.059, 0.082, 1.],
        };
        Self::rect(b, m, [x, y, x + width, y + height], t.solid, fill);
        self.form_label(
            b,
            m,
            x + 32.,
            y + 70.,
            "DIRECT CONNECTION",
            27.,
            1.,
            white,
            0,
            FormFace::Bold,
        );
        self.form_label(
            b,
            m,
            x + 32.,
            y + 116.,
            "Server IPv4 or IPv6 address",
            18.,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        let address = [x + 270., y + 88., x + width - 210., y + 132.];
        let port = [x + width - 134., y + 88., x + width - 32., y + 132.];
        self.form_label(
            b,
            m,
            x + width - 200.,
            y + 116.,
            "Port",
            18.,
            0.,
            white,
            0,
            FormFace::Regular,
        );
        for (i, r, value) in [
            (0, address, a.direct_connect_draft.as_str()),
            (1, port, a.direct_port_draft.as_str()),
        ] {
            Self::rect(b, m, r, t.solid, [0.02, 0.03, 0.04, 1.]);
            let color = if a.direct_field == i {
                accent
            } else {
                [1., 1., 1., 0.22]
            };
            for line in [
                [r[0], r[1], r[2], r[1] + 1.],
                [r[0], r[3] - 1., r[2], r[3]],
                [r[0], r[1], r[0] + 1., r[3]],
                [r[2] - 1., r[1], r[2], r[3]],
            ] {
                Self::rect(b, m, line, t.solid, color);
            }
            self.label(
                b,
                m,
                r[0] + 10.,
                r[1] + 28.,
                if value.is_empty() && i == 0 {
                    "192.0.2.10"
                } else {
                    value
                },
                16.,
                0.,
                if value.is_empty() {
                    [0.5, 0.5, 0.5, 1.]
                } else {
                    white
                },
                0,
                false,
            );
            self.hits.push((r, Hit::DirectField(i)));
        }
        for (accept, left, label) in [
            (true, w / 2. - 125., "CONNECT"),
            (false, w / 2. + 15., "CANCEL"),
        ] {
            let r = [left, y + 166., left + 120., y + 210.];
            Self::rect(
                b,
                m,
                r,
                t.solid,
                if accept { accent } else { [1., 1., 1., 0.06] },
            );
            self.label(
                b,
                m,
                left + 60.,
                r[1] + 28.,
                label,
                13.,
                1.3,
                if accept { ink } else { white },
                2,
                false,
            );
            self.hits.push((r, Hit::DirectButton(accept)));
        }
    }
}
