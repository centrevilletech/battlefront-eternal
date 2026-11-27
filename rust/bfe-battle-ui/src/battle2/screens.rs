use super::paint::{Painter, WHITE};
use super::{Action, BattleView, Caption, Hit, Menu, Outcome, Phase, Response, Style};
use crate::BattleUiSurface;
use bfe_render::{hud_model2::catalog::Catalog, interface::UV};
use std::path::Path;

struct Area {
    rect: [f64; 4],
    hit: Hit,
}

#[derive(Default)]
struct Areas(Vec<Area>);
impl Areas {
    fn add(&mut self, rect: [f64; 4], hit: Hit) {
        if self.0.len() < self.0.capacity() {
            self.0.push(Area { rect, hit });
        }
    }
    fn at(&self, x: f64, y: f64) -> Hit {
        self.0
            .iter()
            .rev()
            .find(|a| {
                let [l, t, w, h] = a.rect;
                x >= l && y >= t && x < l + w && y < t + h
            })
            .map_or(Hit::None, |a| a.hit.clone())
    }
}

/// Owns only presentation resources. A world or an online replica supplies the same borrowed view.
pub struct Screens {
    pub catalog: Catalog,
    pub menu: Menu,
    /// Localized pilot binding, refreshed at admission and when controls change.
    pub takeoff_key: String,
    pub style: Style,
    /// Geometry and glyph layers omitted by the most recent draw budget.
    pub omitted_quads: usize,
    areas: Areas,
    viewport: [f64; 2],
    stats: super::playing::Stats,
    flashes: super::space::Flashes,
    leaving: super::playing::Leaving,
}

impl Screens {
    pub fn open(root: &Path, language: &str, theme: &str) -> Result<Self, String> {
        Ok(Self::new(Catalog::open(root, language)?, theme))
    }
    pub fn new(catalog: Catalog, theme: &str) -> Self {
        Self {
            catalog,
            menu: Menu::default(),
            takeoff_key: String::new(),
            style: Style::for_theme(theme),
            omitted_quads: 0,
            areas: Areas(Vec::with_capacity(256)),
            viewport: [1024., 768.],
            stats: Default::default(),
            flashes: Default::default(),
            leaving: Default::default(),
        }
    }
    pub fn hit(&self, position: [f64; 2]) -> Hit {
        let [w, h] = self.viewport;
        let scale = (w / 1024.).min(h / 768.);
        self.areas.at(
            (position[0] - (w - 1024. * scale) * 0.5) / scale,
            (position[1] - (h - 768. * scale) * 0.5) / scale,
        )
    }
    pub fn input(&mut self, view: &BattleView<'_>, action: Action) -> Response {
        self.menu.act(view, action)
    }

    pub fn draw(
        &mut self,
        surface: &mut dyn BattleUiSurface,
        view: &BattleView<'_>,
        viewport: [f64; 2],
        fraction: f32,
    ) {
        self.areas.0.clear();
        self.omitted_quads = 0;
        if viewport.iter().any(|n| !n.is_finite() || *n <= 0.) {
            return;
        }
        self.viewport = viewport;
        let mut p = Painter::new(surface, &self.catalog, self.style, viewport);
        match view.phase {
            Phase::Ended(outcome) => {
                scoreboard(&mut p, view, &mut self.menu, &mut self.areas, Some(outcome))
            }
            // The spawn launcher, shared by both games, is drawn while the player chooses.
            Phase::Spawn => {}
            Phase::Playing if self.menu.board => {
                scoreboard(&mut p, view, &mut self.menu, &mut self.areas, None)
            }
            Phase::Playing => super::playing::draw(
                &mut p,
                view,
                fraction,
                &self.takeoff_key,
                &mut self.stats,
                &mut self.flashes,
                &mut self.leaving,
            ),
        }
        self.omitted_quads = p.omitted_quads;
    }
}

fn button(
    p: &mut Painter<'_>,
    areas: &mut Areas,
    r: [f64; 4],
    caption: Caption<'_>,
    hit: Hit,
    active: bool,
) {
    let [x, y, w, h] = r;
    let mut fill = p.style.accent;
    fill[3] = if active { 0.34 } else { 0.10 };
    p.solid(r, [0.02, 0.03, 0.05, 0.92]);
    p.solid(r, fill);
    if active {
        p.solid([x, y, 3., h], p.style.accent);
    }
    p.label(
        [x + w * 0.5, y + (h - 22.) * 0.5],
        w - 12.,
        22.,
        2,
        2,
        if active { p.style.ink } else { WHITE },
        caption,
    );
    areas.add(r, hit);
}

// The player-name field accepts 32 characters. Keep an untrusted observation within
// the same bound and reserve room for the two team headers and navigation controls.
fn display_name(s: &str) -> &str {
    s.char_indices().nth(32).map_or(s, |(end, _)| &s[..end])
}

fn scoreboard_rows(v: &BattleView<'_>, fit: usize, remaining: usize, layers: usize) -> usize {
    let digits = |n: u32| n.checked_ilog10().unwrap_or(0) as usize + 1;
    let mut pair = 0;
    for t in &v.teams {
        let largest = v
            .scores
            .iter()
            .filter(|p| p.team == t.id)
            .map(|p| {
                display_name(p.name).encode_utf16().count()
                    + digits(p.score.unsigned_abs())
                    + usize::from(p.score < 0)
                    + digits(p.kills)
                    + digits(p.deaths)
            })
            .max()
            .unwrap_or(0);
        pair += largest * layers + 1;
    }
    fit.min(remaining / pair.max(1)).max(1)
}

fn scoreboard(
    p: &mut Painter<'_>,
    v: &BattleView<'_>,
    m: &mut Menu,
    a: &mut Areas,
    outcome: Option<Outcome>,
) {
    p.glyph_limit = 32;
    p.solid([0., 0., 1024., 768.], [0., 0., 0., 0.65]);
    let art = p.art("statsscreens_bg");
    if art != 0 {
        p.image([0., 0., 1024., 768.], art, UV, [1., 1., 1., 0.68], 0);
    }
    p.panel([24., 45., 976., 88.]);
    let result = match outcome {
        Some(Outcome::Winner(w)) if w == v.team => Caption::Key("ifs.freeform.result.victory"),
        Some(Outcome::Winner(_)) => Caption::Key("ifs.freeform.result.defeat"),
        Some(Outcome::Draw) | None => Caption::Key("ifs.stats.teamstatstitle"),
    };
    p.label([48., 64.], 470., 38., 0, 3, p.style.ink, result);
    p.label([968., 76.], 390., 24., 1, 2, WHITE, v.map_name);
    let row = (15. / p.unit).max(32.);
    let columns = [
        (250., "ifs.stats.points"),
        (364., "ifs.stats.kills"),
        (464., "ifs.stats.deaths"),
    ];
    for (i, t) in v.teams.iter().enumerate() {
        let x = 26. + i as f64 * 492.;
        p.panel([x, 153., 480., 494.]);
        p.icon([x + 12., 164., 45., 45.], t.icon, p.friend(t.id == v.team));
        p.label([x + 61., 171.], 260., 27., 0, 2, p.style.ink, t.name);
        p.text(
            [x + 465., 169.],
            120.,
            29.,
            1,
            3,
            WHITE,
            format_args!("{}", t.score),
        );
        let mut left = 14.;
        for (right, key) in columns {
            p.label(
                [x + right, 218.],
                right - left,
                17.,
                1,
                0,
                WHITE,
                Caption::Key(key),
            );
            left = right + 6.;
        }
    }
    button(
        p,
        a,
        [764., 666., 231., 49.],
        Caption::Key("ifs.stats.done"),
        Hit::Done,
        true,
    );
    let layers = p.text_layers();
    let navigation = 2 * (2 + layers);
    let remaining = p
        .limit
        .saturating_sub(p.surface.batch_mut().count + navigation);
    let rows = scoreboard_rows(v, (405. / row).floor() as usize, remaining, layers);
    m.board_rows = rows;
    m.board_offset = m.board_offset.min(m.last_offset(v));
    if v.teams
        .iter()
        .any(|t| v.scores.iter().filter(|r| r.team == t.id).count() > rows)
    {
        button(
            p,
            a,
            [28., 666., 56., 43.],
            Caption::Literal("<"),
            Hit::Earlier,
            false,
        );
        button(
            p,
            a,
            [90., 666., 56., 43.],
            Caption::Literal(">"),
            Hit::Later,
            false,
        );
    }
    for (i, t) in v.teams.iter().enumerate() {
        let x = 26. + i as f64 * 492.;
        let all = v.scores.iter().filter(|r| r.team == t.id);
        let offset = m.board_offset.min(all.clone().count().saturating_sub(rows));
        for (j, r) in all.skip(offset).take(rows).enumerate() {
            let y = 243. + j as f64 * row;
            if r.player == v.player {
                let mut c = p.style.accent;
                c[3] = 0.22;
                p.solid([x + 8., y - 2., 464., row - 2.], c);
            }
            p.label(
                [x + 15., y],
                170.,
                21.,
                0,
                1,
                WHITE,
                Caption::Literal(display_name(r.name)),
            );
            let values = [i64::from(r.score), r.kills.into(), r.deaths.into()];
            for ((right, _), num) in columns.into_iter().zip(values) {
                p.text([x + right, y], 63., 21., 1, 1, WHITE, format_args!("{num}"));
            }
        }
    }
}
