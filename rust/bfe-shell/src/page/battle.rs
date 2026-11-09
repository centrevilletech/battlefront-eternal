//! Page battle panels. Spawn and scoreboard controllers still own all actions;
//! the painter publishes its rendered regions in their common interface plane.
use super::{
    battle_motion::{Group, Span},
    Page,
};
use crate::render::ShellTextures;
use bfe_battle_ui::{
    scoreboard::{
        BoardPage, BoardState, ClassicScoreboardView, ScoreboardHitTarget, ScoreboardModel,
        ScoreboardPort, SortMode,
    },
    shapes::root_anchor,
    spawn::{SpawnHitTarget, SpawnModel, SpawnPort},
};
use bfe_render::interface::{Batch, Color, Matrix};

#[derive(Clone, Copy)]
pub struct BattleLook {
    pub index: usize,
    pub accent: Color,
    pub ink: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
}
impl BattleLook {
    pub fn new(theme: &str) -> Self {
        let index = match theme {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        let (text, dim, faint, accent, ink) = match index {
            0 => (
                [220. / 255., 239. / 255., 1., 1.],
                [205. / 255., 228. / 255., 1., 0.72],
                [205. / 255., 228. / 255., 1., 0.44],
                [1., 164. / 255., 42. / 255., 1.],
                [19. / 255., 32. / 255., 74. / 255., 1.],
            ),
            1 => (
                [1.; 4],
                [225. / 255., 232. / 255., 242. / 255., 0.76],
                [225. / 255., 232. / 255., 242. / 255., 0.46],
                [224. / 255., 54. / 255., 42. / 255., 1.],
                [1.; 4],
            ),
            _ => (
                [232. / 255., 236. / 255., 242. / 255., 1.],
                [232. / 255., 236. / 255., 242. / 255., 0.66],
                [232. / 255., 236. / 255., 242. / 255., 0.4],
                [1., 154. / 255., 31. / 255., 1.],
                [23. / 255., 13. / 255., 2. / 255., 1.],
            ),
        };
        Self {
            index,
            accent,
            ink,
            text,
            dim,
            faint,
        }
    }
}
/// CSS grid from page/view.js, in framebuffer pixels. The caller converts only
/// published hit rectangles to the existing 1024-wide input plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnLayout {
    pub width: f64,
    pub height: f64,
    pub edge: f64,
    pub roster: [f64; 4],
    pub figure: [f64; 4],
    pub objectives: [f64; 4],
    pub map: [f64; 4],
    pub spawn: [f64; 4],
    pub teams: [[f64; 4]; 2],
    pub center: f64,
}
impl SpawnLayout {
    pub fn new(w: f64, h: f64, objectives: usize) -> Self {
        let edge = w * 0.03;
        let frame = w - 2. * edge;
        let left = (frame * 0.23).max(250.);
        let right = (frame * 0.33).max(300.);
        let gap = w * 0.016;
        let middle = [edge + left + gap, w - edge - right - gap];
        let center = (middle[0] + middle[1]) * 0.5;
        let top = h * 0.14;
        let figure_w = (middle[1] - middle[0]).min(h * 0.46);
        let foot = h - h * 0.028;
        let bh = (h * 0.07).clamp(50., 74.);
        let bw = (middle[1] - middle[0]).min(360.);
        let objectives_h = h * 0.038 + objectives as f64 * h * 0.031 + h * 0.011;
        let plate_w = (w * 0.24).min(340.);
        let plate_h = h * 0.074;
        let team_gap = w * 0.022;
        Self {
            width: w,
            height: h,
            edge,
            roster: [edge, top, edge + left, foot],
            figure: [
                center - figure_w * 0.5,
                top,
                center + figure_w * 0.5,
                foot - bh - h * 0.12,
            ],
            objectives: [w - edge - right, top, w - edge, top + objectives_h],
            map: [
                w - edge - right,
                top + objectives_h + h * 0.012,
                w - edge,
                foot,
            ],
            spawn: [center - bw * 0.5, foot - bh, center + bw * 0.5, foot],
            center,
            teams: [
                [
                    w * 0.5 - team_gap - plate_w,
                    h * 0.024,
                    w * 0.5 - team_gap,
                    h * 0.024 + plate_h,
                ],
                [
                    w * 0.5 + team_gap,
                    h * 0.024,
                    w * 0.5 + team_gap + plate_w,
                    h * 0.024 + plate_h,
                ],
            ],
        }
    }
    pub fn interface(&self, r: [f64; 4], ih: f64) -> [f64; 4] {
        [
            r[0] * 1024. / self.width,
            r[1] * ih / self.height,
            r[2] * 1024. / self.width,
            r[3] * ih / self.height,
        ]
    }
    pub fn class_rect(&self, index: usize, picked: bool) -> [f64; 4] {
        let h = self.height;
        let x = self.roster[0] + if picked { 10. } else { 0. };
        let y = self.roster[1] + h * 0.036 + index as f64 * h * 0.1135;
        [x, y, self.roster[2], y + h * 0.1045]
    }
}
fn matrix(w: f64, h: f64, ih: f64) -> Matrix {
    let mut m = root_anchor(ih);
    m[0] *= 1024. / w;
    m[5] *= ih / h;
    m
}
impl Page {
    // Battle panels are rectangular glass, unlike the clipped front-end card.
    fn battle_panel(b: &mut Batch, m: Matrix, t: &ShellTextures, r: [f64; 4], l: BattleLook) {
        let (top, bottom, border, radius) = match l.index {
            0 => (
                [20. / 255., 60. / 255., 140. / 255., 0.55],
                [6. / 255., 22. / 255., 60. / 255., 0.8],
                [140. / 255., 200. / 255., 1., 0.5],
                4f64,
            ),
            1 => (
                [22. / 255., 46. / 255., 86. / 255., 0.84],
                [10. / 255., 24. / 255., 48. / 255., 0.92],
                [190. / 255., 205. / 255., 225. / 255., 0.45],
                12.,
            ),
            _ => (
                [14. / 255., 17. / 255., 24. / 255., 0.88],
                [8. / 255., 10. / 255., 15. / 255., 0.92],
                [1., 1., 1., 0.12],
                0.,
            ),
        };
        if radius == 0. {
            Self::gradient(
                b,
                m,
                r,
                t.solid,
                bfe_render::interface::UV,
                [top, bottom, top, bottom],
            );
        } else {
            let height = r[3] - r[1];
            let color =
                |y: f64| std::array::from_fn(|i| top[i] + (bottom[i] - top[i]) * y / height);
            for row in 0..radius.ceil() as usize {
                let y = row as f64;
                let inset = radius
                    - (radius * radius - (radius - y - 0.5).powi(2))
                        .max(0.)
                        .sqrt();
                Self::rect(
                    b,
                    m,
                    [r[0] + inset, r[1] + y, r[2] - inset, r[1] + y + 1.],
                    t.solid,
                    color(y),
                );
                Self::rect(
                    b,
                    m,
                    [r[0] + inset, r[3] - y - 1., r[2] - inset, r[3] - y],
                    t.solid,
                    color(height - y),
                );
            }
            Self::gradient(
                b,
                m,
                [r[0], r[1] + radius, r[2], r[3] - radius],
                t.solid,
                bfe_render::interface::UV,
                [
                    color(radius),
                    color(height - radius),
                    color(radius),
                    color(height - radius),
                ],
            );
        }
        Self::battle_border(b, m, t, r, border);
    }
    fn battle_bar(
        b: &mut Batch,
        m: Matrix,
        t: &ShellTextures,
        r: [f64; 4],
        l: BattleLook,
        picked: bool,
    ) {
        let stops: &[(f64, Color)] = match (l.index, picked) {
            (0, true) => &[
                (0., [1., 210. / 255., 122. / 255., 1.]),
                (0.45, [1., 164. / 255., 42. / 255., 1.]),
                (1., [245. / 255., 117. / 255., 0., 1.]),
            ],
            (0, false) => &[
                (0., [1., 196. / 255., 92. / 255., 0.78]),
                (1., [220. / 255., 110. / 255., 10. / 255., 0.7]),
            ],
            (1, true) => &[
                (0., [240. / 255., 103. / 255., 90. / 255., 1.]),
                (0.48, [207. / 255., 42. / 255., 29. / 255., 1.]),
                (0.52, [165. / 255., 24. / 255., 14. / 255., 1.]),
                (1., [216. / 255., 72. / 255., 58. / 255., 1.]),
            ],
            (1, false) => &[
                (0., [201. / 255., 212. / 255., 226. / 255., 1.]),
                (0.5, [154. / 255., 169. / 255., 189. / 255., 1.]),
                (0.52, [132. / 255., 150. / 255., 173. / 255., 1.]),
                (1., [179. / 255., 192. / 255., 208. / 255., 1.]),
            ],
            (_, true) => &[
                (0., [1., 190. / 255., 85. / 255., 1.]),
                (0.6, [1., 148. / 255., 25. / 255., 1.]),
                (1., [245. / 255., 117. / 255., 0., 1.]),
            ],
            (_, false) => {
                Self::rect(b, m, r, t.solid, [1., 1., 1., 0.06]);
                return;
            }
        };
        for pair in stops.windows(2) {
            let (a, ca) = pair[0];
            let (z, cz) = pair[1];
            let rr = if l.index == 2 {
                [
                    r[0] + (r[2] - r[0]) * a,
                    r[1],
                    r[0] + (r[2] - r[0]) * z,
                    r[3],
                ]
            } else {
                [
                    r[0],
                    r[1] + (r[3] - r[1]) * a,
                    r[2],
                    r[1] + (r[3] - r[1]) * z,
                ]
            };
            Self::gradient(
                b,
                m,
                rr,
                t.solid,
                bfe_render::interface::UV,
                if l.index == 2 {
                    [ca, ca, cz, cz]
                } else {
                    [ca, cz, ca, cz]
                },
            );
        }
    }

    fn battle_border(b: &mut Batch, m: Matrix, t: &ShellTextures, r: [f64; 4], c: Color) {
        for line in [
            [r[0], r[1], r[2], r[1] + 1.],
            [r[0], r[3] - 1., r[2], r[3]],
            [r[0], r[1], r[0] + 1., r[3]],
            [r[2] - 1., r[1], r[2], r[3]],
        ] {
            Self::rect(b, m, line, t.solid, c);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw_spawn(
        &mut self,
        b: &mut Batch,
        t: &ShellTextures,
        model: &SpawnModel,
        port: &dyn SpawnPort,
        layout: SpawnLayout,
        ih: f64,
        theme: &str,
        turned: bool,
        regions: &mut Vec<([f64; 4], SpawnHitTarget)>,
    ) {
        let (w, h) = (layout.width, layout.height);
        let m = matrix(w, h, ih);
        let l = BattleLook::new(theme);
        regions.clear();
        self.battle.spawn.clear();
        let head_start = b.count;
        let region = |r, target| (layout.interface(r, ih), target);
        for (i, r) in layout.teams.iter().copied().enumerate() {
            let team = i as u8 + 1;
            let own = team == model.team;
            let c = if own {
                [0.31, 0.94, 0.48, 1.]
            } else {
                [1., 0.35, 0.29, 1.]
            };
            Self::battle_panel(b, m, t, r, l);
            Self::battle_border(b, m, t, r, if own { c } else { [0.35, 0.39, 0.45, 0.3] });
            Self::rect(b, m, [r[0], r[3] - 3., r[2], r[3]], t.solid, c);
            if let Some(facts) = port.team_facts(team) {
                let (x, align, nx, na) = if i == 0 {
                    (r[0] + w * 0.012, 0, r[2] - w * 0.012, 1)
                } else {
                    (r[2] - w * 0.012, 1, r[0] + w * 0.012, 0)
                };
                self.label(
                    b,
                    m,
                    x,
                    r[1] + h * 0.030,
                    &match facts.side.as_str() {
                        "all" => "REBELS".into(),
                        "imp" => "EMPIRE".into(),
                        _ => facts.name.to_uppercase(),
                    },
                    (h * 0.026).clamp(16., 28.),
                    2.2,
                    c,
                    align,
                    true,
                );
                self.label(
                    b,
                    m,
                    nx,
                    r[1] + h * 0.046,
                    &facts.reinforcements.to_string(),
                    (h * 0.028).clamp(18., 30.),
                    0.,
                    l.text,
                    na,
                    true,
                );
                let state = if own {
                    "YOUR TEAM"
                } else if model.can_switch_teams(port) {
                    "SWITCH TEAM"
                } else {
                    "TEAMS: AUTO ASSIGN"
                };
                if !own && model.can_switch_teams(port) {
                    let sw = 98.;
                    let rx = if i == 0 { x } else { x - sw };
                    Self::rect(
                        b,
                        m,
                        [rx, r[1] + h * 0.040, rx + sw, r[1] + h * 0.066],
                        t.solid,
                        l.accent,
                    );
                }
                self.label(
                    b,
                    m,
                    x,
                    r[1] + h * 0.057,
                    state,
                    9.,
                    2.,
                    if own { c } else { l.ink },
                    align,
                    false,
                );
            }
            if !own && model.can_switch_teams(port) {
                let mut hit = r;
                let emblem = (h * 0.074).clamp(46., 80.) + w * 0.007;
                if i == 0 {
                    hit[0] -= emblem;
                } else {
                    hit[2] += emblem;
                }
                regions.push(region(hit, SpawnHitTarget::Team));
            }
        }
        if model.can_switch_teams(port) {
            let r = [w * 0.5 - 16., h * 0.04, w * 0.5 + 16., h * 0.083];
            Self::battle_border(b, m, t, r, l.accent);
            self.label(b, m, w * 0.5, h * 0.068, "<>", 16., 0., l.accent, 2, false);
            regions.push(region(r, SpawnHitTarget::Team));
            self.label(
                b,
                m,
                w * 0.5,
                h * 0.124,
                "[<] [>] OR CLICK TO SWITCH TEAM",
                9.,
                1.5,
                l.dim,
                2,
                false,
            );
        }
        self.battle.spawn.push(Span {
            first: head_start,
            end: b.count,
            hits: [0, regions.len()],
            group: Group::Head,
        });
        let label_start = b.count;
        self.label(
            b,
            m,
            layout.roster[0],
            h * 0.158,
            "SELECT SOLDIER",
            11.,
            2.6,
            l.text,
            0,
            false,
        );
        Self::rect(
            b,
            m,
            [
                layout.roster[0] + 138.,
                h * 0.152,
                layout.roster[2],
                h * 0.152 + 1.,
            ],
            t.solid,
            [1., 0.6, 0.12, 0.45],
        );
        self.battle.spawn.push(Span {
            first: label_start,
            end: b.count,
            hits: [0, 0],
            group: Group::Roster,
        });
        let roster = model.roster(port);
        for (i, entry) in roster.iter().enumerate() {
            let picked = i == model.class_index;
            let first = b.count;
            let hit = regions.len();
            let r = layout.class_rect(i, picked);
            Self::battle_panel(b, m, t, r, l);
            let bar = [r[0] + 1., r[1] + 1., r[2] - 1., r[1] + h * 0.042];
            Self::battle_bar(b, m, t, bar, l, picked);
            Self::battle_border(
                b,
                m,
                t,
                r,
                if picked {
                    l.accent
                } else {
                    [0.4, 0.4, 0.4, 0.2]
                },
            );
            self.label(
                b,
                m,
                r[0] + w * 0.01,
                r[1] + h * 0.029,
                &entry.display_name.to_uppercase(),
                (h * 0.019).clamp(13., 19.),
                1.3,
                if picked { l.ink } else { l.text },
                0,
                false,
            );
            for (j, weapon) in entry.weapons.iter().take(4).enumerate() {
                let x = r[0] + w * 0.01 + (j % 2) as f64 * (r[2] - r[0]) * 0.49;
                let y = r[1] + h * (0.066 + (j / 2) as f64 * 0.023);
                let text = weapon.to_uppercase();
                self.battle_clip(
                    b,
                    m,
                    x,
                    y,
                    &text,
                    10.,
                    0.5,
                    if picked { l.text } else { l.faint },
                    (r[2] - r[0]) * 0.46,
                );
                if picked {
                    Self::rect(b, m, [x - 5., y - 5., x - 2., y - 2.], t.solid, l.accent);
                }
            }
            regions.push(region(r, SpawnHitTarget::Class(i)));
            self.battle.spawn.push(Span {
                first,
                end: b.count,
                hits: [hit, regions.len()],
                group: Group::Class(i, picked),
            });
        }
        let tactical_start = b.count;
        let r = layout.objectives;
        Self::battle_panel(b, m, t, r, l);
        Self::rect(
            b,
            m,
            [r[0] + 1., r[1] + 1., r[2] - 1., r[1] + h * 0.036],
            t.solid,
            [1., 1., 1., 0.06],
        );
        self.label(
            b,
            m,
            r[0] + w * 0.01,
            r[1] + h * 0.024,
            "OBJECTIVES",
            11.,
            2.4,
            l.text,
            0,
            false,
        );
        if let Some(facts) = port.team_facts(model.team) {
            for (i, text) in facts.objectives.iter().enumerate() {
                let y = r[1] + h * (0.063 + i as f64 * 0.031);
                Self::rect(
                    b,
                    m,
                    [r[0] + 12., y - 6., r[0] + 17., y - 1.],
                    t.solid,
                    l.accent,
                );
                self.battle_clip(
                    b,
                    m,
                    r[0] + 26.,
                    y,
                    &text.to_uppercase(),
                    11.,
                    0.8,
                    l.text,
                    r[2] - r[0] - 40.,
                );
            }
        }
        self.battle.spawn.push(Span {
            first: tactical_start,
            end: b.count,
            hits: [0, 0],
            group: Group::Tactical,
        });
        if !turned {
            self.label(
                b,
                m,
                layout.figure[2],
                layout.figure[1] + h * 0.029,
                "DRAG TO TURN",
                9.,
                2.,
                l.dim,
                1,
                false,
            );
        }
        let caption_start = b.count;
        if let Some(entry) = roster.get(model.class_index) {
            self.label(
                b,
                m,
                layout.center,
                h * 0.821,
                &entry.display_name.to_uppercase(),
                (h * 0.042).clamp(22., 46.),
                3.,
                [1.; 4],
                2,
                true,
            );
        }
        let posts = port.posts(model.team);
        let post = posts
            .iter()
            .find(|p| model.post.as_deref() == Some(p.name.as_str()));
        if let Some(post) = post {
            let name = post.display_name.to_uppercase();
            let vehicles = if post.vehicles.is_empty() {
                "NONE".into()
            } else {
                post.vehicles.join(", ").to_uppercase()
            };
            let pieces = [
                ("COMMAND POST", 10., 1.6, false, l.dim),
                (name.as_str(), 16., 1.5, true, [1.; 4]),
                ("VEHICLES", 10., 1.6, false, l.dim),
                (vehicles.as_str(), 10., 1.0, false, l.text),
            ];
            let mut widths = [0.; 4];
            for (i, (text, size, spacing, title, _)) in pieces.iter().enumerate() {
                let chars: Vec<u16> = text.encode_utf16().collect();
                widths[i] = if *title {
                    self.title.width(&chars, *size / 64., *spacing)
                } else {
                    self.body.width(&chars, *size / 64., *spacing)
                };
            }
            let gap = 8.;
            let available = layout.objectives[0] - layout.roster[2] - w * 0.032;
            let total = widths.iter().sum::<f64>() + gap * 3.;
            let wrap = total > available;
            let mut x = layout.center
                - if wrap {
                    (widths[0] + widths[1] + gap) * 0.5
                } else {
                    total * 0.5
                };
            let mut y = h * if wrap { 0.862 } else { 0.880 };
            for (i, (text, size, spacing, title, color)) in pieces.iter().enumerate() {
                if wrap && i == 2 {
                    x = layout.center - (widths[2] + widths[3] + gap) * 0.5;
                    y = h * 0.888;
                }
                self.label(b, m, x, y, text, *size, *spacing, *color, 0, *title);
                x += widths[i] + gap;
            }
        } else {
            self.label(
                b,
                m,
                layout.center,
                h * 0.880,
                "PICK A COMMAND POST",
                12.,
                1.5,
                l.dim,
                2,
                false,
            );
        }
        self.battle.spawn.push(Span {
            first: caption_start,
            end: b.count,
            hits: [0, 0],
            group: Group::Caption,
        });
        let spawn_start = b.count;
        let spawn_hit = regions.len();
        let enabled = model.post.is_some() && !roster.is_empty();
        self.widget_button(b, m, t, layout.spawn, "", l.index, true, !enabled);
        let y = (layout.spawn[1] + layout.spawn[3]) * 0.5;
        let size = (h * 0.03).clamp(20., 32.);
        self.label(
            b,
            m,
            layout.center - 34.,
            y + size * 0.35,
            "SPAWN",
            size,
            size * 0.32,
            l.ink,
            2,
            true,
        );
        let key = [layout.center + 42., y - 9., layout.center + 85., y + 9.];
        Self::battle_border(b, m, t, key, l.ink);
        self.label(
            b,
            m,
            (key[0] + key[2]) * 0.5,
            y + 3.,
            "ENTER",
            9.,
            0.9,
            l.ink,
            2,
            false,
        );
        regions.push(region(layout.spawn, SpawnHitTarget::Spawn));
        self.battle.spawn.push(Span {
            first: spawn_start,
            end: b.count,
            hits: [spawn_hit, regions.len()],
            group: Group::Spawn,
        });
    }
    // Only the clipped column's text is shortened; full model strings and
    // labels remain retained and available when its width changes.
    #[allow(clippy::too_many_arguments)]
    fn battle_clip(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        s: &str,
        size: f64,
        spacing: f64,
        c: Color,
        width: f64,
    ) {
        let text: Vec<u16> = s.encode_utf16().collect();
        if self.body.width(&text, size / 64., spacing) <= width {
            self.label(b, m, x, y, s, size, spacing, c, 0, false);
            return;
        }
        let mut end = s.len();
        while end > 0 {
            end -= 1;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            let cut = format!("{}...", &s[..end]);
            if self
                .body
                .width(&cut.encode_utf16().collect::<Vec<_>>(), size / 64., spacing)
                <= width
            {
                self.label(b, m, x, y, &cut, size, spacing, c, 0, false);
                break;
            }
        }
    }
}
impl Page {
    #[allow(clippy::too_many_arguments)]
    pub fn draw_scoreboard(
        &mut self,
        b: &mut Batch,
        t: &ShellTextures,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
        view: &ClassicScoreboardView,
        w: f64,
        h: f64,
        ih: f64,
        theme: &str,
        regions: &mut Vec<([f64; 4], ScoreboardHitTarget)>,
    ) {
        let m = matrix(w, h, ih);
        let l = BattleLook::new(theme);
        regions.clear();
        let mut region = |r: [f64; 4], target| {
            regions.push((
                [
                    r[0] * 1024. / w,
                    r[1] * ih / h,
                    r[2] * 1024. / w,
                    r[3] * ih / h,
                ],
                target,
            ))
        };
        Self::rect(b, m, [0., 0., w, h], t.solid, [0., 0., 0., 0.58]);
        self.heading(
            b,
            m,
            t,
            w * 0.5,
            h * 0.09,
            "STATS",
            (h * 0.046).clamp(26., 50.),
            l.index,
        );
        let edge = w * 0.03;
        let tab_y = h * 0.122;
        for (i, (label, page, target)) in [
            ("TABLE", BoardPage::Table, ScoreboardHitTarget::Back),
            ("DETAILS", BoardPage::Details, ScoreboardHitTarget::Details),
            ("AWARDS", BoardPage::Awards, ScoreboardHitTarget::Awards),
        ]
        .into_iter()
        .enumerate()
        {
            if page == BoardPage::Awards && model.state != BoardState::End {
                continue;
            }
            let r = [
                edge + i as f64 * 130.,
                tab_y,
                edge + (i + 1) as f64 * 130.,
                tab_y + h * 0.069,
            ];
            self.label(
                b,
                m,
                (r[0] + r[2]) * 0.5,
                tab_y + h * 0.043,
                label,
                13.,
                1.8,
                if model.page == page { l.accent } else { l.dim },
                2,
                false,
            );
            if model.page == page {
                self.widget_button(b, m, t, r, label, l.index, true, false);
            } else {
                region(r, target);
            }
        }
        if model.page == BoardPage::Table {
            let gap = w * 0.016;
            let pw = (w - 2. * edge - gap) * 0.5;
            let top = tab_y + 68.;
            let bottom = h * 0.90;
            let row_h = (h * 0.034).clamp(26., 38.);
            let pitch = row_h + 2.;
            let list_top = top + 34. + 30. + h * 0.004;
            for side in 1..=2 {
                let x = edge + (side - 1) as f64 * (pw + gap);
                let relation = port.relation(side as i32, model.team);
                let c = if relation > 0 {
                    [0.37, 0.88, 0.54, 1.]
                } else if relation < 0 {
                    [1., 0.42, 0.35, 1.]
                } else {
                    l.dim
                };
                if let Some(header) = &model.titles[side - 1] {
                    Self::rect(b, m, [x + 2., top, x + 6., top + 18.], t.solid, c);
                    self.label(
                        b,
                        m,
                        x + 16.,
                        top + 18.,
                        &header.name.to_uppercase(),
                        18.,
                        2.,
                        c,
                        0,
                        true,
                    );
                    self.label(
                        b,
                        m,
                        x + pw - 8.,
                        top + 18.,
                        &header.reinforcements.to_string(),
                        16.,
                        1.,
                        l.text,
                        1,
                        false,
                    );
                }
                self.label(
                    b,
                    m,
                    x + pw - 55.,
                    top + 18.,
                    "REINFORCEMENTS",
                    9.,
                    1.7,
                    l.dim,
                    1,
                    false,
                );
                Self::battle_panel(b, m, t, [x, top + 30., x + pw, bottom], l);
                Self::rect(
                    b,
                    m,
                    [x + 1., top + 31., x + pw - 1., top + 60.],
                    t.solid,
                    l.accent,
                );
                let numeric = [x + pw - 246., x + pw - 154., x + pw - 32.];
                self.label(b, m, x + 18., top + 51., "NAME", 12., 1.6, l.ink, 0, false);
                let sorted = [SortMode::Kills, SortMode::Deaths, SortMode::Points];
                for (i, label) in ["KILLS", "DEATHS", "CP POINTS"].iter().enumerate() {
                    self.label(
                        b,
                        m,
                        numeric[i],
                        top + 51.,
                        label,
                        12.,
                        1.4,
                        l.ink,
                        1,
                        false,
                    );
                    if model.mode == sorted[i] {
                        self.label(
                            b,
                            m,
                            numeric[i] + 14.,
                            top + 51.,
                            if (model.mode == SortMode::Deaths) != model.reverse {
                                "^"
                            } else {
                                "v"
                            },
                            10.,
                            0.,
                            l.ink,
                            2,
                            false,
                        );
                    }
                    region(
                        [numeric[i] - 72., top + 30., numeric[i] + 20., top + 60.],
                        ScoreboardHitTarget::Sort {
                            mode: sorted[i],
                            side,
                        },
                    );
                }
                let ping = port.network() && model.state == BoardState::Peek;
                for (i, row) in model.lists[side - 1].iter().enumerate().skip(model.first) {
                    let y = list_top + (i - model.first) as f64 * pitch;
                    if y + row_h > bottom {
                        break;
                    }
                    let r = [x + 4., y, x + pw - 4., y + row_h];
                    let selected = model.side == side && model.selected == i;
                    let hovered = model.hover.is_some_and(|v| v.side == side && v.index == i);
                    if row.local || selected || hovered {
                        Self::rect(
                            b,
                            m,
                            r,
                            t.solid,
                            [
                                l.accent[0],
                                l.accent[1],
                                l.accent[2],
                                if row.local { 0.22 } else { 0.12 },
                            ],
                        );
                    }
                    if selected {
                        Self::battle_border(b, m, t, r, l.accent);
                    }
                    let c = if row.local {
                        l.text
                    } else if row.human {
                        l.accent
                    } else {
                        l.dim
                    };
                    self.battle_clip(
                        b,
                        m,
                        x + 18.,
                        y + row_h * 0.70,
                        &row.name.to_uppercase(),
                        12.,
                        0.55,
                        c,
                        (numeric[0] - x - 42.).max(30.),
                    );
                    if ping {
                        self.label(
                            b,
                            m,
                            numeric[0] - 65.,
                            y + row_h * 0.70,
                            &row.ping.map_or("-".into(), |p| p.to_string()),
                            11.,
                            0.,
                            c,
                            1,
                            false,
                        );
                    }
                    for (j, n) in [row.kills, row.deaths, row.points].iter().enumerate() {
                        self.label(
                            b,
                            m,
                            numeric[j],
                            y + row_h * 0.70,
                            &n.to_string(),
                            12.,
                            0.,
                            c,
                            1,
                            false,
                        );
                    }
                    region(r, ScoreboardHitTarget::Row { side, index: i });
                }
            }
        } else {
            let r = [edge, tab_y + 48., w - edge, h * 0.90];
            Self::battle_panel(b, m, t, r, l);
            let details = model.page == BoardPage::Details;
            Self::rect(b, m, [r[0], r[1], r[2], r[1] + 27.], t.solid, l.accent);
            self.label(
                b,
                m,
                r[0] + 14.,
                r[1] + 19.,
                if details {
                    "PERSONAL STATS"
                } else {
                    "PLAYER AWARDS"
                },
                11.,
                2.,
                l.ink,
                0,
                false,
            );
            if details {
                if let Some(subject) = &model.subject {
                    self.label(
                        b,
                        m,
                        w * 0.5,
                        r[1] + 75.,
                        &subject.name.to_uppercase(),
                        24.,
                        2.4,
                        l.text,
                        2,
                        true,
                    );
                }
            }
            let rows = if details {
                &view.details_page.content
            } else {
                &view.awards_page.content
            };
            let pitch = (h * 0.042).clamp(30., 44.);
            let first = r[1] + if details { 104. } else { 60. };
            for (i, row) in rows.iter().enumerate() {
                let y = first + i as f64 * pitch;
                Self::rect(
                    b,
                    m,
                    [w * 0.15, y + pitch, w * 0.85, y + pitch + 1.],
                    t.solid,
                    [1., 1., 1., 0.08],
                );
                self.label(
                    b,
                    m,
                    w * 0.5 - w * 0.015,
                    y + pitch * 0.7,
                    row.label,
                    12.,
                    1.5,
                    l.dim,
                    1,
                    false,
                );
                self.label(
                    b,
                    m,
                    w * 0.5 + w * 0.015,
                    y + pitch * 0.7,
                    &row.value.to_uppercase(),
                    13.,
                    0.7,
                    l.text,
                    0,
                    false,
                );
            }
        }
        let bh = (h * 0.05).clamp(34., 48.);
        let bottom = h * 0.97;
        let r = [edge, bottom - bh, edge + 140., bottom];
        let done = model.state == BoardState::End && model.page == BoardPage::Table;
        self.widget_button(
            b,
            m,
            t,
            r,
            if done { "DONE" } else { "BACK" },
            l.index,
            false,
            false,
        );
        region(
            r,
            if done {
                ScoreboardHitTarget::Done
            } else {
                ScoreboardHitTarget::Back
            },
        );
        if model.page == BoardPage::Table {
            let r = [w * 0.5 - 70., bottom - bh, w * 0.5 + 70., bottom];
            self.widget_button(b, m, t, r, "DETAILS", l.index, false, false);
            region(r, ScoreboardHitTarget::Details);
            if model.state == BoardState::End {
                let r = [w - edge - 140., bottom - bh, w - edge, bottom];
                self.widget_button(b, m, t, r, "AWARDS", l.index, false, false);
                region(r, ScoreboardHitTarget::Awards);
            }
        }
    }
}
