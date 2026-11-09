//! Page spawn's small CSS shapes and fallback arrow glyphs. Keep these in the
//! retained interface batch so their pixels share the panels' entrance motion.
use super::{battle::BattleLook, Page};
use crate::render::ShellTextures;
use bfe_render::interface::{rotate_z, translate, Batch, Color, Matrix};

impl Page {
    /// A CSS rounded box, including the fully rounded swap and hint controls.
    /// The short horizontal strips retain the ordinary quad batch and atlas.
    pub(super) fn battle_round(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        radius: f64,
        fill: Color,
        border: Color,
    ) {
        let height = r[3] - r[1];
        let radius = radius.min(height * 0.5).min((r[2] - r[0]) * 0.5);
        for row in 0..height.ceil() as usize {
            let y = row as f64;
            let middle = y + 0.5;
            let distance = (radius - middle).max(middle - (height - radius)).max(0.);
            let inset = radius - (radius * radius - distance * distance).max(0.).sqrt();
            let x0 = r[0] + inset;
            let x1 = r[2] - inset;
            let bottom = (r[1] + y + 1.).min(r[3]);
            if y >= 1. && middle < height - 1. && x1 - x0 > 2. {
                Self::rect(b, m, [x0, r[1] + y, x0 + 1., bottom], t.solid, border);
                Self::rect(b, m, [x0 + 1., r[1] + y, x1 - 1., bottom], t.solid, fill);
                Self::rect(b, m, [x1 - 1., r[1] + y, x1, bottom], t.solid, border);
            } else {
                Self::rect(b, m, [x0, r[1] + y, x1, bottom], t.solid, border);
            }
        }
    }

    pub(super) fn battle_diamond(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        at: [f64; 2],
        half: f64,
        color: Color,
    ) {
        let m = rotate_z(translate(m, at[0], at[1], 0.), std::f64::consts::FRAC_PI_4);
        Self::rect(b, m, [-half, -half, half, half], t.solid, color);
    }

    fn battle_line(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        a: [f64; 2],
        z: [f64; 2],
        width: f64,
        c: Color,
    ) {
        let dx = z[0] - a[0];
        let dy = z[1] - a[1];
        let m = rotate_z(translate(m, a[0], a[1], 0.), dy.atan2(dx));
        Self::rect(
            b,
            m,
            [0., -width * 0.5, dx.hypot(dy), width * 0.5],
            t.solid,
            c,
        );
    }

    // The source uses system-font fallback for these Unicode arrows. Saira's
    // Latin subset has none, so draw their strokes without missing-glyph boxes.
    fn battle_arrow(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        p: [f64; 2],
        direction: f64,
        size: f64,
        c: Color,
    ) {
        let tip = [p[0] + direction * size * 0.5, p[1]];
        Self::battle_line(b, m, t, [p[0] - direction * size * 0.5, p[1]], tip, 1.2, c);
        for y in [-1., 1.] {
            Self::battle_line(
                b,
                m,
                t,
                tip,
                [tip[0] - direction * size * 0.32, tip[1] + y * size * 0.3],
                1.2,
                c,
            );
        }
    }

    pub(super) fn battle_swap(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        l: BattleLook,
    ) {
        Self::battle_round(
            b,
            m,
            t,
            r,
            (r[3] - r[1]) * 0.5,
            [0., 0., 0., 0.5],
            [l.accent[0], l.accent[1], l.accent[2], 0.55],
        );
        let x = (r[0] + r[2]) * 0.5;
        let y = (r[1] + r[3]) * 0.5;
        Self::battle_arrow(b, m, t, [x, y - 3.], 1., 12., l.accent);
        Self::battle_arrow(b, m, t, [x, y + 3.], -1., 12., l.accent);
    }

    pub(super) fn battle_team_keys(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        w: f64,
        h: f64,
        l: BattleLook,
    ) {
        let text = "OR CLICK TO SWITCH TEAM";
        let size = (h * 0.011).clamp(9., 11.);
        let spacing = size * 0.24;
        let width = self.body.width(
            &text.encode_utf16().collect::<Vec<_>>(),
            size / 64.,
            spacing,
        );
        let kw = size * 2.1;
        let gap = size * 0.45;
        let x = (w - width - kw * 2. - gap * 2. - size * 0.4) * 0.5;
        let y = h * 0.124;
        for (i, direction) in [-1., 1.].into_iter().enumerate() {
            let x = x + i as f64 * (kw + gap);
            let r = [x, y - size * 1.18, x + kw, y + size * 0.36];
            Self::battle_round(b, m, t, r, 3., [0.; 4], l.dim);
            Self::battle_arrow(
                b,
                m,
                t,
                [x + kw * 0.5, (r[1] + r[3]) * 0.5],
                direction,
                size * 0.85,
                l.dim,
            );
        }
        self.label(
            b,
            m,
            x + 2. * (kw + gap) + size * 0.4,
            y,
            text,
            size,
            spacing,
            l.faint,
            0,
            false,
        );
    }

    pub(super) fn battle_turn_hint(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        figure: [f64; 4],
        h: f64,
        l: BattleLook,
    ) {
        let size = (h * 0.0115).clamp(9., 11.);
        let spacing = size * 0.26;
        let text = "DRAG TO TURN";
        let width = self.body.width(
            &text.encode_utf16().collect::<Vec<_>>(),
            size / 64.,
            spacing,
        );
        let height = size * 2.2;
        let top = figure[1] + (figure[3] - figure[1]) * 0.03;
        let r = [figure[2] - width - size * 3.7, top, figure[2], top + height];
        Self::battle_round(
            b,
            m,
            t,
            r,
            height * 0.5,
            [0., 0., 0., 0.42],
            [l.accent[0], l.accent[1], l.accent[2], 0.35],
        );
        // Clockwise open circle and arrowhead for U+21BB (the turn hint).
        let center = [r[0] + size * 1.2, top + height * 0.5];
        let radius = size * 0.45;
        let mut last = [
            center[0] + radius * (-0.9f64).cos(),
            center[1] + radius * (-0.9f64).sin(),
        ];
        for i in 1..=20 {
            let angle = -0.9 + i as f64 / 20. * 5.4;
            let p = [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ];
            Self::battle_line(b, m, t, last, p, 1.3, l.accent);
            last = p;
        }
        Self::battle_line(
            b,
            m,
            t,
            [center[0] + radius, center[1] - radius],
            [center[0] + radius, center[1] + 0.5],
            1.3,
            l.accent,
        );
        Self::battle_line(
            b,
            m,
            t,
            [center[0] + radius, center[1] + 0.5],
            [center[0] + radius - size * 0.38, center[1] - 0.4],
            1.3,
            l.accent,
        );
        self.label(
            b,
            m,
            r[2] - size * 0.8,
            top + height * 0.5 + size * 0.35,
            text,
            size,
            spacing,
            l.dim,
            1,
            false,
        );
    }
}
