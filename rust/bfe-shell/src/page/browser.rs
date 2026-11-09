//! Page server-browser presentation. Discovery, filters and joins use the
//! existing ServerBrowserService and ShellApp; no network state lives here.
use super::{Hit, Page};
use crate::{render::ShellTextures, screens::ShellApp};
use bfe_online::browser::{BrowserState, Filters, GameInfo, ServerBrowserService, Source};
use bfe_render::interface::{Batch, Color, Matrix, UV};

#[derive(Default)]
pub struct BrowserView {
    pub source: Option<Source>,
    pub filters: Filters,
    pub state: BrowserState,
    pub selected: Option<String>,
    pub rows: Vec<[String; 7]>,
    pub info: Option<GameInfo>,
    pub search: bool,
    pub focus: usize,
    pub first: usize,
    pub choice: Option<usize>,
    pub choices: Vec<(String, String)>,
    pub choice_cursor: usize,
    pub choice_rect: [f64; 4],
    count_label: String,
}
impl BrowserView {
    pub fn sync(&mut self, browser: &dyn ServerBrowserService) {
        self.source = Some(browser.source());
        self.filters = browser.filters().clone();
        self.state = browser.state().clone();
        let selected = browser.selected();
        self.selected = selected.map(|s| s.id.clone());
        self.info = selected.map(|s| browser.describe(s, &[]));
        self.rows = browser
            .rows()
            .into_iter()
            .map(|s| {
                let b = s.battle.as_ref();
                [
                    s.name.clone(),
                    format!("{}/{}", s.players, s.max_players),
                    b.map(|v| {
                        if v.map_name.is_empty() {
                            v.map.clone()
                        } else {
                            v.map_name.clone()
                        }
                    })
                    .unwrap_or_default(),
                    b.map(|v| v.mode.to_uppercase()).unwrap_or_default(),
                    b.map(|v| {
                        if v.era == "cw" {
                            "CLONE WARS"
                        } else {
                            "GALACTIC CIVIL WAR"
                        }
                        .into()
                    })
                    .unwrap_or_default(),
                    s.latency_ms
                        .map(|v| format!("{v} MS"))
                        .unwrap_or_else(|| "—".into()),
                    s.location.clone(),
                ]
            })
            .collect();
        self.count_label = format!("{} SERVERS", self.rows.len());
    }
}
impl Page {
    fn browser_panel(b: &mut Batch, m: Matrix, t: &ShellTextures, r: [f64; 4], look: usize) {
        // The browser uses border-radius, without the generic Page panel clip.
        if look != 2 {
            Self::panel(b, m, t, r, look);
            return;
        }
        Self::gradient(
            b,
            m,
            r,
            t.solid,
            UV,
            [
                [14. / 255., 17. / 255., 24. / 255., 0.88],
                [8. / 255., 10. / 255., 15. / 255., 0.92],
                [14. / 255., 17. / 255., 24. / 255., 0.88],
                [8. / 255., 10. / 255., 15. / 255., 0.92],
            ],
        );
        for line in [
            [r[0], r[1], r[2], r[1] + 1.],
            [r[0], r[3] - 1., r[2], r[3]],
            [r[0], r[1], r[0] + 1., r[3]],
            [r[2] - 1., r[1], r[2], r[3]],
        ] {
            Self::rect(b, m, line, t.solid, [1., 1., 1., 0.12]);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn browser_draw(
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
        let mut view = std::mem::take(&mut self.browser);
        let edge = (w * 0.03).clamp(20., 56.);
        let right = w - edge;
        let look = match a.settings.menu_theme.as_str() {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        let dim = [white[0], white[1], white[2], 0.55];
        self.heading(
            b,
            m,
            t,
            w / 2.,
            h * 0.09,
            "MULTIPLAYER",
            (h * 0.046).clamp(26., 50.),
            look,
        );
        // Tabs stay owned by the same multiplayer screen. CREATE invokes HOST.
        for (i, label) in ["JOIN", "CREATE", "LEADERBOARD   PLANNED"]
            .iter()
            .enumerate()
        {
            let x = edge + [0., 110., 250.][i];
            let r = [x, h * 0.122, x + if i == 2 { 260. } else { 100. }, h * 0.19];
            if i == 0 {
                Self::rect(b, m, r, t.solid, accent);
            }
            self.label(
                b,
                m,
                x + 30.,
                r[1] + 30.,
                label,
                14.,
                3.,
                if i == 0 { ink } else { dim },
                0,
                false,
            );
            if i < 2 {
                self.hits.push((r, Hit::BrowserTab(i)));
            }
        }
        let bar = [edge, h * 0.202, right, h * 0.264];
        Self::browser_panel(b, m, t, bar, look);
        let gap = w * 0.008;
        let widths = [
            w * 0.09,
            w * 0.25,
            w * 0.14,
            w * 0.128,
            w * 0.10,
            w * 0.077,
            w * 0.077,
        ];
        let map = a
            .data
            .maps
            .iter()
            .find(|v| v.id == view.filters.map)
            .map(|v| v.name.as_str())
            .unwrap_or("ALL MAPS");
        let era = match view.filters.era.as_str() {
            "cw" => "CLONE WARS",
            "gcw" => "GALACTIC CIVIL WAR",
            _ => "ALL ERAS",
        };
        let labels = [
            if view.source == Some(Source::Lan) {
                "LAN"
            } else {
                "INTERNET"
            },
            if view.filters.name.is_empty() {
                "SEARCH SERVERS"
            } else {
                &view.filters.name
            },
            map,
            era,
            view.filters.ping.label(),
            "HIDE FULL",
            "HIDE EMPTY",
        ];
        let mut x = edge + 12.;
        for (i, label) in labels.iter().enumerate() {
            let r = [
                x,
                bar[1] + 6.,
                (x + widths[i]).min(right - 12.),
                bar[3] - 6.,
            ];
            if i < 5 {
                Self::rect(
                    b,
                    m,
                    r,
                    t.solid,
                    if i == 0 { accent } else { [0., 0., 0., 0.35] },
                );
                for line in [[r[0], r[1], r[2], r[1] + 1.], [r[0], r[3] - 1., r[2], r[3]]] {
                    Self::rect(b, m, line, t.solid, [1., 1., 1., 0.22]);
                }
                self.label(
                    b,
                    m,
                    x + 10.,
                    r[1] + 20.,
                    label,
                    11.,
                    1.,
                    if i == 0 { ink } else { white },
                    0,
                    false,
                );
                if i != 1 {
                    for step in 0..6 {
                        let dx = step as f64;
                        let cy = r[1] + 13. + dx;
                        for px in [r[2] - 21. + dx, r[2] - 11. - dx] {
                            Self::rect(
                                b,
                                m,
                                [px, cy, px + 1.5, cy + 1.5],
                                t.solid,
                                if i == 0 { ink } else { accent },
                            );
                        }
                    }
                }
            } else {
                let on = if i == 5 {
                    view.filters.hide_full
                } else {
                    view.filters.hide_empty
                };
                Self::rect(
                    b,
                    m,
                    [x, r[1] + 9., x + 23., r[1] + 21.],
                    t.solid,
                    if on { accent } else { [1., 1., 1., 0.14] },
                );
                Self::rect(
                    b,
                    m,
                    [
                        x + if on { 13. } else { 1. },
                        r[1] + 10.,
                        x + if on { 22. } else { 10. },
                        r[1] + 20.,
                    ],
                    t.solid,
                    white,
                );
                self.label(b, m, x + 29., r[1] + 20., label, 10., 1., dim, 0, false);
            }
            self.hits.push((r, Hit::BrowserFilter(i)));
            if view.choice == Some(i) {
                view.choice_rect = r;
            }
            x += widths[i] + gap;
        }
        let table = [edge, h * 0.276, right, h * 0.612];
        Self::browser_panel(b, m, t, table, look);
        Self::rect(
            b,
            m,
            [table[0], table[1], table[2], table[1] + 30.],
            t.solid,
            accent,
        );
        let columns = [3.1, 0.95, 1.9, 1., 1.55, 0.85, 1.35];
        let total = columns.iter().sum::<f64>();
        let available = right - edge - 28. - 6. * w * 0.01;
        let mut starts = [0.; 7];
        let mut ends = [0.; 7];
        let mut x = edge + 14.;
        for (i, label) in [
            "GAME NAME",
            "PLAYERS",
            "MAP",
            "MODE",
            "ERA",
            "PING",
            "LOCATION",
        ]
        .iter()
        .enumerate()
        {
            starts[i] = x;
            ends[i] = x + columns[i] / total * available;
            self.form_label(
                b,
                m,
                x,
                table[1] + 20.,
                label,
                16.,
                2.,
                ink,
                0,
                super::forms::FormFace::Regular,
            );
            self.hits
                .push(([x, table[1], ends[i], table[1] + 30.], Hit::BrowserSort(i)));
            x = ends[i] + w * 0.01;
        }
        let visible = ((table[3] - table[1] - 36.) / 30.).floor().max(1.) as usize;
        if let Some(selected) = view
            .selected
            .as_ref()
            .and_then(|id| a.mp_servers.iter().position(|s| &s.id == id))
        {
            if selected < view.first {
                view.first = selected;
            }
            if selected >= view.first + visible {
                view.first = selected + 1 - visible;
            }
        }
        if view.rows.is_empty() {
            let (message,hint)=match &view.state{
                BrowserState::Notice=>("ONLINE PLAY NEEDS YOUR OK FIRST","Read how the online service handles your connection, then accept to see the servers."),
                BrowserState::Loading=>("SEARCHING FOR SERVERS…",""),
                BrowserState::Error(e)=>(e.as_str(),"REFRESH TO TRY AGAIN"),
                _ if view.filters.is_filtered()=>("NO SERVERS MATCH THESE FILTERS","Try changing the filters."),
                _=>("NO SERVERS ONLINE","Refresh or try LAN to find a local game."),
            };
            self.label(
                b,
                m,
                w / 2.,
                table[1] + 104.,
                message,
                15.,
                2.,
                dim,
                2,
                false,
            );
            self.label(b, m, w / 2., table[1] + 138., hint, 14., 0.8, dim, 2, false);
            if matches!(view.state, BrowserState::Notice) {
                let r = [w / 2. - 85., table[1] + 152., w / 2. + 85., table[1] + 186.];
                self.widget_button(b, m, t, r, "REVIEW NOTICE", look, false, false);
                self.hits.push((r, Hit::BrowserNotice));
            }
        } else {
            for (index, row) in view.rows.iter().enumerate().skip(view.first).take(visible) {
                let y = table[1] + 34. + (index - view.first) as f64 * 30.;
                let selected = a
                    .mp_servers
                    .get(index)
                    .map(|s| Some(&s.id) == view.selected.as_ref())
                    .unwrap_or(false);
                if selected {
                    Self::rect(
                        b,
                        m,
                        [edge + 4., y, right - 4., y + 28.],
                        t.solid,
                        [accent[0], accent[1], accent[2], 0.20],
                    );
                }
                for i in 0..7 {
                    self.ellipsized(
                        b,
                        m,
                        starts[i],
                        y + 20.,
                        &row[i],
                        ends[i] - starts[i],
                        12.,
                        0.5,
                        white,
                    );
                }
                self.hits
                    .push(([edge, y, right, y + 28.], Hit::BrowserServer(index)));
            }
        }
        if !view.rows.is_empty() {
            self.label(
                b,
                m,
                edge + 5.,
                table[3] + 15.,
                &view.count_label,
                11.,
                1.6,
                dim,
                0,
                false,
            );
        }
        let info = [edge, h * 0.634, right, h * 0.893];
        Self::browser_panel(b, m, t, info, look);
        Self::rect(
            b,
            m,
            [info[0], info[1], info[2], info[1] + 25.],
            t.solid,
            accent,
        );
        self.label(
            b,
            m,
            edge + 14.,
            info[1] + 17.,
            "GAME INFO",
            11.,
            2.,
            ink,
            0,
            false,
        );
        if let Some(details) = &view.info {
            let y = info[1] + 36.;
            let ph = info[3] - y - 28.;
            let pw = ph * 1.5;
            let texture = t.map_preview_for(&details.map_id);
            if texture != 0 {
                b.quad(
                    m,
                    [edge + 15., y, edge + 15. + pw, y + ph],
                    texture,
                    UV,
                    [1.; 4],
                    0.,
                    0,
                );
            }
            self.label(
                b,
                m,
                edge + 15. + pw / 2.,
                info[3] - 8.,
                &details.preview_caption,
                11.,
                0.5,
                white,
                2,
                false,
            );
            if let Some(server) = a
                .mp_servers
                .iter()
                .find(|s| Some(&s.id) == view.selected.as_ref())
            {
                self.label(
                    b,
                    m,
                    edge + pw + 36.,
                    y + 16.,
                    &server.name,
                    16.,
                    0.3,
                    white,
                    0,
                    false,
                );
                self.ellipsized(
                    b,
                    m,
                    edge + pw + 36.,
                    y + 38.,
                    &server.description,
                    w * 0.62,
                    12.,
                    0.,
                    dim,
                );
            }
            let mut cx = edge + pw + 36.;
            for chip in &details.chips {
                let width = self.text_width(chip, 10., 1.) + 16.;
                self.label(b, m, cx + 8., y + 64., chip, 10., 1., accent, 0, false);
                cx += width + 8.;
            }
            for (i, (label, value)) in details.facts.iter().take(6).enumerate() {
                let x = edge + pw + 36. + (i % 3) as f64 * w * 0.19;
                let fy = y + 90. + (i / 3) as f64 * 21.;
                self.label(b, m, x, fy, label, 10., 1., dim, 0, false);
                self.ellipsized(b, m, x + 65., fy, value, w * 0.13, 11., 0., white);
            }
        } else {
            self.label(
                b,
                m,
                w / 2.,
                (info[1] + info[3]) / 2. + 14.,
                "PICK A SERVER TO SEE ITS GAME INFO",
                15.,
                2.,
                dim,
                2,
                false,
            );
        }
        for (i, label, r) in [
            (0, "BACK", [edge, h * 0.923, edge + 140., h * 0.97]),
            (
                1,
                "REFRESH",
                [w / 2. - 166., h * 0.923, w / 2. - 26., h * 0.97],
            ),
            (
                2,
                "DIRECT CONNECT",
                [w / 2. - 12., h * 0.923, w / 2. + 168., h * 0.97],
            ),
            (3, "JOIN", [right - 140., h * 0.923, right, h * 0.97]),
        ] {
            let disabled = i == 3 && view.selected.is_none();
            self.widget_button(b, m, t, r, label, look, view.focus == i + 1, disabled);
            if !disabled {
                self.hits.push((r, Hit::BrowserFooter(i)));
            }
        }
        if let Some(field) = view.choice {
            let r = view.choice_rect;
            let count = view.choices.len().min(12);
            let start = view.choice_cursor.saturating_sub(count.saturating_sub(1));
            for (i, (_, label)) in view.choices.iter().enumerate().skip(start).take(count) {
                let y = r[3] + (i - start) as f64 * 28.;
                let rr = [r[0], y, (r[0] + 220.).min(right), y + 28.];
                Self::rect(
                    b,
                    m,
                    rr,
                    t.solid,
                    if i == view.choice_cursor {
                        accent
                    } else {
                        [0.02, 0.03, 0.04, 1.]
                    },
                );
                self.ellipsized(
                    b,
                    m,
                    rr[0] + 9.,
                    y + 20.,
                    label,
                    rr[2] - rr[0] - 18.,
                    12.,
                    0.,
                    if i == view.choice_cursor { ink } else { white },
                );
                self.hits.push((rr, Hit::BrowserChoice(field, i)));
            }
        }
        self.browser = view;
    }
    pub(super) fn text_width(&self, text: &str, size: f64, spacing: f64) -> f64 {
        text.encode_utf16()
            .map(|ch| {
                self.body
                    .glyphs
                    .get(&ch)
                    .map(|g| g.advance * size / self.body.height + spacing)
                    .unwrap_or(0.)
            })
            .sum::<f64>()
            - spacing
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ellipsized(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &str,
        width: f64,
        size: f64,
        spacing: f64,
        c: Color,
    ) {
        // Cache the admitted, shortened label rather than clipping glyphs across columns.
        let mut end = text.len();
        let fits = |s: &str| self.text_width(s, size, spacing) <= width;
        while end > 0 && !fits(&text[..end]) {
            end -= 1;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
        }
        self.label(b, m, x, y, &text[..end], size, spacing, c, 0, false);
    }
}
