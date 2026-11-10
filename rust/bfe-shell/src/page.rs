//! Native page layout over ShellApp's existing screen stack. Geometry follows
//! src/menus/themes/page/style.js; looks follow electron/ui/looks.css.
mod battle;
mod setup;
pub use setup::{SetupView,SetupProgress,SetupSound};
mod battle_chrome;
mod battle_motion;
mod browser;
mod forms;
mod heading;
mod host;
mod media;
mod motion;
mod pause;
mod widgets;
use crate::{
    render::ShellTextures,
    screens::{PopupState, ScreenId, ShellApp},
};
pub use battle::{BattleLook, SpawnBattleStatus, SpawnLayout};
use bfe_render::interface::{anchor, rotate_z, translate, Batch, Color, Font, Matrix, UV};
pub use browser::BrowserView;
pub use media::{FrameTexture, MediaTextures};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Row(usize),
    Back,
    Theme,
    Look(usize),
    Popup(usize),
    BrowserTab(usize),
    BrowserFilter(usize),
    BrowserChoice(usize, usize),
    BrowserSort(usize),
    BrowserServer(usize),
    BrowserFooter(usize),
    BrowserNotice,
    OptionBack(usize, usize),
    DirectField(usize),
    Host(crate::screens::HostControl),
    DirectButton(bool),
}

struct Paragraph {
    source: String,
    width: f64,
    size: f64,
    spacing: f64,
    lines: Vec<Vec<u16>>,
}
pub struct Page {
    battle: battle_motion::BattlePaintState,
    pub browser: BrowserView,
    pub media: MediaTextures,
    pub pause_backdrops: [u32; 3],
    pause_hits: Vec<bfe_battle_ui::pause::PauseHotspot>,
    pause_rows: Vec<(bfe_battle_ui::pause::PauseRowId, String)>,
    input_state: Option<(ScreenId, u8)>,
    pub body: Font,
    pub logo: Font,
    pub title: Font,
    pub bold: Font,
    pub regular: Font,
    pub medium: Font,
    pub prose: Font,
    pub mono: Font,
    viewport: [f64; 2],
    button_text: HashMap<String, Vec<u16>>,
    pub dropdown: bool,
    pub dropdown_cursor: usize,
    pub discord_offer: bool,
    pub discord_cursor: usize,
    pub discord_asked: bool,
    pub buttons: [u32; 6],
    pub button_source: [f64; 2],
    pub patterns: [u32; 3],
    pub setup_backdrops:[u32;3],
    pub modal_start: Option<usize>,
    hits: Vec<([f64; 4], Hit)>,
    text: HashMap<String, Vec<u16>>,
    screen: ScreenId,
    pub entered: f64,
    row_spans: Vec<motion::RowSpan>,
    leaving: Vec<motion::LeavingQuad>,
    widget_screen: String,
    widget_spans: Vec<motion::RowSpan>,
    widget_snapshot: Vec<motion::LeavingQuad>,
    widget_leaving: Vec<motion::LeavingQuad>,
    widget_depth: usize,
    widget_forward: bool,
    stack_depth: usize,
    forward: bool,
    brand_from: f64,
    brand_target: f64,
    brand_started: f64,
    depth_from: f64,
    depth_target: f64,
    paragraphs: Vec<Paragraph>,
}
impl Page {
    pub fn new(body: Font, logo: Font) -> Self {
        let title = Font {
            height: logo.height,
            glyphs: logo.glyphs.clone(),
        };
        let bold = Font {
            height: body.height,
            glyphs: body.glyphs.clone(),
        };
        let regular = Font {
            height: body.height,
            glyphs: body.glyphs.clone(),
        };
        let medium = Font {
            height: body.height,
            glyphs: body.glyphs.clone(),
        };
        Self {
            battle: battle_motion::BattlePaintState::default(),
            prose: Font {height: regular.height,glyphs:regular.glyphs.clone()},
            mono: Font {height: regular.height,glyphs:regular.glyphs.clone()},
            regular,
            medium,
            bold,
            viewport: [1280., 720.],
            button_text: HashMap::new(),
            browser: BrowserView::default(),
            media: MediaTextures::default(),
            pause_backdrops: [0; 3],
            pause_hits: Vec::with_capacity(16),
            pause_rows: Vec::new(),
            input_state: None,
            title,
            body,
            logo,
            dropdown: false,
            dropdown_cursor: 2,
            discord_offer: false,
            discord_cursor: 0,
            discord_asked: false,
            buttons: [0; 6],
            button_source: [368., 40.],
            patterns: [0; 3],
            setup_backdrops:[0;3],
            modal_start: None,
            hits: Vec::with_capacity(64),
            text: HashMap::new(),
            screen: ScreenId::Gate,
            entered: 0.,
            row_spans: Vec::with_capacity(16),
            leaving: Vec::with_capacity(256),
            widget_screen: String::new(),
            widget_spans: Vec::with_capacity(16),
            widget_snapshot: Vec::with_capacity(512),
            widget_leaving: Vec::with_capacity(512),
            widget_depth: 0,
            widget_forward: true,
            stack_depth: 1,
            forward: true,
            brand_from: 1.,
            brand_target: 1.,
            brand_started: 0.,
            depth_from: 0.,
            depth_target: 0.,
            paragraphs: Vec::with_capacity(8),
        }
    }
    /// Native select keyboard behavior stays local to the theme control;
    /// it must not move a shell row or dismiss the underlying screen.
    pub fn dropdown_key(&mut self, key: &str) -> Option<Option<usize>> {
        if !self.dropdown {
            return None;
        }
        match key {
            "ArrowDown" => {
                self.dropdown_cursor = (self.dropdown_cursor + 1) % 3;
                Some(None)
            }
            "ArrowUp" => {
                self.dropdown_cursor = (self.dropdown_cursor + 2) % 3;
                Some(None)
            }
            "Home" => {
                self.dropdown_cursor = 0;
                Some(None)
            }
            "End" => {
                self.dropdown_cursor = 2;
                Some(None)
            }
            "Enter" | " " => {
                self.dropdown = false;
                Some(Some(self.dropdown_cursor))
            }
            "Escape" | "Tab" => {
                self.dropdown = false;
                Some(None)
            }
            _ => Some(None),
        }
    }
    pub fn discord_key(&mut self, key: &str) -> Option<Option<&'static str>> {
        if !self.discord_offer {
            return None;
        }
        match key {
            "ArrowRight" | "Tab" => {
                self.discord_cursor = (self.discord_cursor + 1) % 3;
                Some(None)
            }
            "ArrowLeft" => {
                self.discord_cursor = (self.discord_cursor + 2) % 3;
                Some(None)
            }
            "Home" => {
                self.discord_cursor = 0;
                Some(None)
            }
            "End" => {
                self.discord_cursor = 2;
                Some(None)
            }
            "Enter" | " " => {
                self.discord_offer = false;
                Some(Some(["off", "game", "full"][self.discord_cursor]))
            }
            "Escape" => {
                self.discord_offer = false;
                Some(Some("off"))
            }
            _ => Some(None),
        }
    }
    fn input_state(a: &ShellApp) -> (ScreenId, u8) {
        (
            a.current_screen(),
            if a.profile.editing {
                4
            } else {
                match a.popup {
                    None => 0,
                    Some(PopupState::OnlineNotice { .. }) => 5,
                    Some(PopupState::Notice { .. }) => 1,
                    Some(PopupState::ConfirmExit { .. }) => 2,
                    Some(PopupState::HostClosePrompt { .. }) => 3,
                }
            },
        )
    }
    pub fn input_matches(&self, a: &ShellApp) -> bool {
        self.input_state == Some(Self::input_state(a))
    }
    pub fn hit(&self, x: f64, y: f64) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3])
            .map(|(_, h)| *h)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_text(
        f: &Font,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &[u16],
        c: Color,
        align: u32,
        size: f64,
        spacing: f64,
    ) {
        let scale = size / f.height;
        let width = text
            .iter()
            .filter_map(|ch| f.glyphs.get(ch))
            .map(|g| g.advance * scale + spacing)
            .sum::<f64>();
        let mut pen = x - width
            * match align {
                1 => 1.,
                2 => 0.5,
                _ => 0.,
            };
        for ch in text {
            if let Some(g) = f.glyphs.get(ch) {
                let r = g.bounds.map(|v| v * scale);
                if r[0] < r[2] {
                    b.quad(
                        m,
                        [pen + r[0], y + r[1], pen + r[2], y + r[3]],
                        g.page,
                        g.uv,
                        c,
                        0.,
                        0,
                    );
                }
                pen += g.advance * scale + spacing;
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn label(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        s: &str,
        size: f64,
        spacing: f64,
        c: Color,
        align: u32,
        logo: bool,
    ) {
        if !self.text.contains_key(s) {
            self.text.insert(s.into(), s.encode_utf16().collect());
        }
        let f = if logo { &self.title } else { &self.body };
        Self::draw_text(f, b, m, x, y, &self.text[s], c, align, size, spacing);
    }
    fn rect(b: &mut Batch, m: Matrix, r: [f64; 4], solid: u32, c: Color) {
        b.quad(m, r, solid, UV, c, 0., 0);
    }
    // Batch's public vertex tint mirror is RGBA per corner. Retain the same
    // dirty-span contract when a page gradient changes an individual corner.
    fn gradient(
        b: &mut Batch,
        m: Matrix,
        r: [f64; 4],
        texture: u32,
        uv: [f64; 4],
        colors: [Color; 4],
    ) {
        let first = b.count;
        b.quad(m, r, texture, uv, colors[0], 0., 0);
        if b.count == first {
            return;
        }
        for (corner, color) in colors.iter().enumerate() {
            let at = (first * 4 + corner) * 4;
            b.tint[at..at + 4].copy_from_slice(&color.map(|v| v as f32));
        }
        b.start = b.start.min(first * 4);
        b.end = b.end.max((first + 1) * 4);
    }
    #[allow(clippy::too_many_arguments)]
    fn word(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        s: &str,
        size: f64,
        align: u32,
        look: usize,
    ) {
        // The logo's eight extrusion layers and text-clipped gradient are the
        // .bfe-logo .word rules in electron/ui/looks.css, not a system font.
        if !self.text.contains_key(s) {
            self.text.insert(s.into(), s.encode_utf16().collect());
        }
        let depth_colors = match look {
            0 => [
                [111., 151., 216.],
                [93., 134., 204.],
                [76., 117., 191.],
                [60., 100., 176.],
                [47., 85., 160.],
                [36., 71., 143.],
                [26., 58., 125.],
                [18., 47., 107.],
            ],
            1 => [
                [154., 163., 173.],
                [134., 144., 155.],
                [115., 125., 137.],
                [97., 107., 119.],
                [80., 89., 101.],
                [64., 72., 83.],
                [49., 56., 66.],
                [35., 41., 49.],
            ],
            _ => [
                [138., 146., 158.],
                [119., 128., 140.],
                [100., 109., 122.],
                [83., 91., 103.],
                [67., 74., 85.],
                [53., 59., 69.],
                [41., 46., 54.],
                [30., 34., 42.],
            ],
        };
        let first = b.count;
        for depth in (1..=8).rev() {
            let c = depth_colors[depth - 1];
            Self::draw_text(
                &self.logo,
                b,
                m,
                x,
                y + depth as f64,
                &self.text[s],
                [c[0] / 255., c[1] / 255., c[2] / 255., 1.],
                align,
                size,
                -size * 0.01,
            );
        }
        let stops: &[(f64, [f64; 3])] = match look {
            0 => &[
                (0., [255., 255., 255.]),
                (0.38, [226., 240., 255.]),
                (0.70, [159., 198., 245.]),
                (1., [111., 158., 227.]),
            ],
            1 => &[
                (0., [255., 255., 255.]),
                (0.30, [228., 232., 236.]),
                (0.52, [169., 177., 186.]),
                (0.74, [217., 222., 227.]),
                (1., [255., 255., 255.]),
            ],
            _ => &[
                (0., [255., 255., 255.]),
                (0.40, [241., 244., 248.]),
                (0.72, [185., 194., 207.]),
                (1., [142., 152., 167.]),
            ],
        };
        let chars = &self.text[s];
        let font = &self.logo;
        let scale = size / 64.;
        let spacing = -size * 0.01;
        let width = chars
            .iter()
            .filter_map(|ch| font.glyphs.get(ch))
            .map(|g| g.advance * scale + spacing)
            .sum::<f64>();
        let mut pen = match align {
            2 => x - width / 2.,
            1 => x - width,
            _ => x,
        };
        let Some(a) = font.glyphs.get(&65) else {
            return;
        };
        let top = y + a.bounds[1] * scale;
        let height = (a.bounds[3] - a.bounds[1]) * scale;
        for ch in chars {
            if let Some(g) = font.glyphs.get(ch) {
                let r = g.bounds.map(|v| v * scale);
                let y0 = y + r[1];
                let y1 = y + r[3];
                if r[0] < r[2] && y1 > y0 {
                    for pair in stops.windows(2) {
                        let lo = (top + height * pair[0].0).max(y0);
                        let hi = (top + height * pair[1].0).min(y1);
                        if hi <= lo {
                            continue;
                        }
                        let tint = |py: f64| {
                            let f = ((py - top) / height - pair[0].0) / (pair[1].0 - pair[0].0);
                            let c = std::array::from_fn::<_, 3, _>(|i| {
                                (pair[0].1[i] + (pair[1].1[i] - pair[0].1[i]) * f) / 255.
                            });
                            [c[0], c[1], c[2], 1.]
                        };
                        let v = |py: f64| g.uv[1] + (g.uv[3] - g.uv[1]) * (py - y0) / (y1 - y0);
                        Self::gradient(
                            b,
                            m,
                            [pen + r[0], lo, pen + r[2], hi],
                            g.page,
                            [g.uv[0], v(lo), g.uv[2], v(hi)],
                            [tint(lo), tint(hi), tint(lo), tint(hi)],
                        );
                    }
                }
                pen += g.advance * scale + spacing;
            }
        }
        // CSS perspective(1100px) rotateX(16deg), around the word's centre.
        let cx = x + width
            * match align {
                1 => -0.5,
                2 => 0.,
                _ => 0.5,
            };
        let cy = y + font.caps_middle() * scale;
        let angle = 16f64.to_radians();
        for quad in first..b.count {
            for corner in 0..4 {
                let at = (quad * 4 + corner) * 3;
                let px = (b.position[at] as f64 - m[12]) / m[0];
                let py = (b.position[at + 1] as f64 - m[13]) / m[5];
                let dy = py - cy;
                let perspective = 1. / (1. - dy * angle.sin() / 1100.);
                b.position[at] = ((cx + (px - cx) * perspective) * m[0] + m[12]) as f32;
                b.position[at + 1] = ((cy + dy * angle.cos() * perspective) * m[5] + m[13]) as f32;
            }
        }
        b.start = b.start.min(first * 4);
        b.end = b.end.max(b.count * 4);
    }
    fn paragraph_index(&mut self, text: &str, width: f64, size: f64, spacing: f64) -> usize {
        if let Some(i) = self.paragraphs.iter().position(|p| {
            p.source == text && p.width == width && p.size == size && p.spacing == spacing
        }) {
            return i;
        }
        if self.paragraphs.len() >= 32 {
            self.paragraphs.clear();
        }
        let mut lines = Vec::new();
        for block in text.split('\n') {
            let mut line = Vec::new();
            for word in block.split_whitespace() {
                let old = line.len();
                if old != 0 {
                    line.push(32);
                }
                line.extend(word.encode_utf16());
                if old != 0 && self.body.width(&line, size / self.body.height, spacing) > width {
                    let next = line.split_off(old + 1);
                    line.truncate(old);
                    lines.push(line);
                    line = next;
                }
            }
            lines.push(line);
        }
        self.paragraphs.push(Paragraph {
            source: text.into(),
            width,
            size,
            spacing,
            lines,
        });
        self.paragraphs.len() - 1
    }
    #[allow(clippy::too_many_arguments)]
    fn paragraph(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &str,
        width: f64,
        size: f64,
        spacing: f64,
        color: Color,
    ) -> f64 {
        let i = self.paragraph_index(text, width, size, spacing);
        for (row, line) in self.paragraphs[i].lines.iter().enumerate() {
            self.body.draw(
                b,
                m,
                x,
                y + row as f64 * size * 1.45,
                line,
                color,
                2,
                size / self.body.height,
                spacing,
                0.,
            );
        }
        self.paragraphs[i].lines.len() as f64 * size * 1.45
    }
    fn panel(b: &mut Batch, m: Matrix, t: &ShellTextures, r: [f64; 4], look: usize) {
        let (top, bottom) = match look {
            0 => (
                [20. / 255., 60. / 255., 140. / 255., 0.55],
                [6. / 255., 22. / 255., 60. / 255., 0.8],
            ),
            1 => (
                [22. / 255., 46. / 255., 86. / 255., 0.84],
                [10. / 255., 24. / 255., 48. / 255., 0.92],
            ),
            _ => (
                [14. / 255., 17. / 255., 24. / 255., 0.88],
                [8. / 255., 10. / 255., 15. / 255., 0.92],
            ),
        };
        let height = r[3] - r[1];
        let cut = if look == 2 {
            18.
        } else if look == 0 {
            4.
        } else {
            8.
        };
        let color =
            |y: f64| std::array::from_fn::<_, 4, _>(|i| top[i] + (bottom[i] - top[i]) * y / height);
        for row in 0..cut as usize {
            let y = row as f64;
            let inset = if look == 2 {
                cut - y - 0.5
            } else {
                cut - (cut * cut - (cut - y - 0.5).powi(2)).max(0.).sqrt()
            };
            let l = if look == 2 { inset } else { inset.max(0.) };
            let right = if look == 2 { 0. } else { inset.max(0.) };
            Self::gradient(
                b,
                m,
                [r[0] + l, r[1] + y, r[2] - right, r[1] + y + 1.],
                t.solid,
                UV,
                [color(y), color(y + 1.), color(y), color(y + 1.)],
            );
            Self::gradient(
                b,
                m,
                [r[0] + right, r[3] - y - 1., r[2] - l, r[3] - y],
                t.solid,
                UV,
                [
                    color(height - y - 1.),
                    color(height - y),
                    color(height - y - 1.),
                    color(height - y),
                ],
            );
        }
        Self::gradient(
            b,
            m,
            [r[0], r[1] + cut, r[2], r[3] - cut],
            t.solid,
            UV,
            [
                color(cut),
                color(height - cut),
                color(cut),
                color(height - cut),
            ],
        );
        let border = [1., 1., 1., if look == 0 { 0.5 } else { 0.12 }];
        for line in [
            [r[0] + cut, r[1], r[2] - cut, r[1] + 1.],
            [r[0] + cut, r[3] - 1., r[2] - cut, r[3]],
            [r[0], r[1] + cut, r[0] + 1., r[3] - cut],
            [r[2] - 1., r[1] + cut, r[2], r[3] - cut],
        ] {
            Self::rect(b, m, line, t.solid, border);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn brand(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        solid: u32,
        w: f64,
        h: f64,
        home: f64,
        theme: &str,
        ink: Color,
    ) {
        // PageTheme's brand is shared between the gate/home and corner.
        let large = (w * 0.055).min(h * 0.10).min(104.);
        let small = (w * 0.017).min(h * 0.032).min(32.);
        let logo_size = small + (large - small) * home;
        if !self.text.contains_key("BATTLEFRONT") {
            self.text
                .insert("BATTLEFRONT".into(), "BATTLEFRONT".encode_utf16().collect());
        }
        let chars = &self.text["BATTLEFRONT"];
        let width = chars
            .iter()
            .filter_map(|ch| self.logo.glyphs.get(ch))
            .map(|g| g.advance * logo_size / 64. - logo_size * 0.01)
            .sum::<f64>();
        let lx = w * 0.03 + (w * 0.5 - width / 2. - w * 0.03) * home;
        let ly = (h * 0.026 + small) * (1. - home) + (h * 0.16 + large * 0.78) * home;
        let look = match theme {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        self.word(b, m, lx, ly, "BATTLEFRONT", logo_size, 0, look);
        if !self.text.contains_key("ETERNAL") {
            self.text
                .insert("ETERNAL".into(), "ETERNAL".encode_utf16().collect());
        }
        let tag_size = logo_size * 0.25;
        let tag_spacing = tag_size * 0.5;
        let tag_w = self
            .title
            .width(&self.text["ETERNAL"], tag_size / 64., tag_spacing)
            + tag_size * 3.3;
        let brand_width = self.logo.width(
            &self.text["BATTLEFRONT"],
            logo_size / 64.,
            -logo_size * 0.01,
        );
        let tag_right = lx + brand_width - tag_size * 0.1;
        let tag_x = tag_right - tag_w;
        let tag_y = ly + logo_size * 0.06 * home + tag_size * 0.42;
        let tag_h = tag_size * 1.56;
        // The tag's sloping right edge, padding and gradient are its CSS
        // clip-path, not the chevron used for ordinary page buttons.
        for row in 0..tag_h.ceil() as usize {
            let top = row as f64;
            let bottom = (top + 1.).min(tag_h);
            let tint = |y: f64| {
                let f = y / tag_h;
                let (a, z, t) = if look == 1 {
                    if f < 0.5 {
                        ([240., 85., 74.], [210., 40., 28.], f / 0.5)
                    } else {
                        ([210., 40., 28.], [148., 21., 12.], (f - 0.5) / 0.5)
                    }
                } else if f < 0.42 {
                    ([255., 210., 122.], [255., 164., 42.], f / 0.42)
                } else {
                    ([255., 164., 42.], [245., 117., 0.], (f - 0.42) / 0.58)
                };
                let c = std::array::from_fn::<_, 3, _>(|i| (a[i] + (z[i] - a[i]) * t) / 255.);
                [c[0], c[1], c[2], 1.]
            };
            let cut = tag_size * 1.5 * (1. - (top + bottom) / 2. / tag_h);
            Self::gradient(
                b,
                m,
                [tag_x, tag_y + top, tag_right - cut, tag_y + bottom],
                solid,
                UV,
                [tint(top), tint(bottom), tint(top), tint(bottom)],
            );
        }
        let baseline = tag_y + tag_h / 2. - self.title.caps_middle() * tag_size / 64.;
        self.label(
            b,
            m,
            tag_x + tag_size * 0.9,
            baseline,
            "ETERNAL",
            tag_size,
            tag_spacing,
            ink,
            0,
            true,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn picker(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        solid: u32,
        w: f64,
        look: usize,
        accent: Color,
        ink: Color,
        bg: Color,
        white: Color,
    ) {
        // All looks are page surface sets; this dropdown never changes the
        // shell stack or starts another menu implementation.
        let tr = [w - w * 0.03 - 176., 18., w - w * 0.03, 56.];
        Self::rect(b, m, tr, solid, [0.02, 0.03, 0.04, 0.9]);
        self.label(
            b,
            m,
            tr[0] - 12.,
            tr[1] + 25.,
            "THEME",
            11.,
            3.,
            white,
            1,
            false,
        );
        let names = ["Battlefront (2004)", "Battlefront II (2005)", "Eternal"];
        self.form_label(
            b,
            m,
            tr[0] + 12.,
            tr[1] + 25.,
            names[look],
            13.,
            0.5,
            white,
            0,
            forms::FormFace::Medium,
        );
        let border = [1., 1., 1., 0.28];
        for r in [
            [tr[0], tr[1], tr[2], tr[1] + 1.],
            [tr[0], tr[3] - 1., tr[2], tr[3]],
            [tr[0], tr[1], tr[0] + 1., tr[3]],
            [tr[2] - 1., tr[1], tr[2], tr[3]],
        ] {
            Self::rect(b, m, r, solid, border);
        }
        let arrow_x = tr[2] - 22.;
        let arrow_y = (tr[1] + tr[3]) / 2. - 2.;
        let l = rotate_z(
            translate(m, arrow_x, arrow_y, 0.),
            std::f64::consts::FRAC_PI_4,
        );
        let r = rotate_z(
            translate(m, arrow_x + 6., arrow_y + 6., 0.),
            -std::f64::consts::FRAC_PI_4,
        );
        Self::rect(b, l, [0., -1., 8.5, 1.], solid, white);
        Self::rect(b, r, [0., -1., 8.5, 1.], solid, white);
        self.hits.push((tr, Hit::Theme));
        if self.dropdown {
            for (i, name) in names.iter().enumerate() {
                let y = tr[3] + i as f64 * 30.;
                let r = [tr[0], y, tr[2], y + 30.];
                Self::rect(
                    b,
                    m,
                    r,
                    solid,
                    if i == self.dropdown_cursor {
                        accent
                    } else {
                        bg
                    },
                );
                self.label(
                    b,
                    m,
                    r[0] + 12.,
                    y + 21.,
                    name,
                    13.,
                    0.5,
                    if i == self.dropdown_cursor {
                        ink
                    } else {
                        white
                    },
                    0,
                    false,
                );
                self.hits.push((r, Hit::Look(i)));
            }
        }
    }
    pub fn draw(&mut self, b: &mut Batch, t: &ShellTextures, a: &ShellApp, w: f64, h: f64) {
        self.viewport = [w, h];
        self.modal_start = None;
        self.hits.clear();
        self.input_state = Some(Self::input_state(a));
        let id = if a.current_screen() == ScreenId::Popup {
            a.stack.last().copied().unwrap_or(ScreenId::Main)
        } else {
            a.current_screen()
        };
        let id = if id == ScreenId::DirectConnect {
            a.stack
                .iter()
                .rev()
                .skip(1)
                .copied()
                .find(|id| *id != ScreenId::Popup)
                .unwrap_or(ScreenId::Multiplayer)
        } else {
            id
        };
        if self.screen != id {
            if matches!(self.screen, ScreenId::Options | ScreenId::OptionsPage(_))
                && !self.widget_snapshot.is_empty()
            {
                std::mem::swap(&mut self.leaving, &mut self.widget_snapshot);
            } else {
                motion::retain_rows(b, &self.row_spans, &mut self.leaving);
            }
            self.widget_screen.clear();
            self.widget_snapshot.clear();
            self.widget_leaving.clear();
            self.forward = a.stack.len() >= self.stack_depth;
            self.stack_depth = a.stack.len();
            let flight = motion::ease(
                (a.pulse_elapsed - self.brand_started) / 0.62,
                0.7,
                0.,
                0.25,
                1.,
            );
            self.brand_from += (self.brand_target - self.brand_from) * flight;
            self.brand_target = if matches!(id, ScreenId::Main | ScreenId::Gate) {
                1.
            } else {
                0.
            };
            self.brand_started = a.pulse_elapsed;
            let drift = motion::ease((a.pulse_elapsed - self.entered) / 1.6, 0.2, 0.8, 0.2, 1.);
            self.depth_from += (self.depth_target - self.depth_from) * drift;
            self.depth_target = a.stack.len().saturating_sub(1).min(4) as f64;
            self.screen = id;
            self.entered = a.pulse_elapsed;
            self.text.clear();
            self.paragraphs.clear();
            self.dropdown = false;
            self.discord_offer = false;
        }
        b.begin();
        self.row_spans.clear();
        // anchor uses a 1024-wide plane. Scale its two axes independently so
        // CSS viewport units retain their actual pixel positions at any aspect.
        let mut m = anchor(0., 0., false, h);
        m[0] *= 1024. / w;
        let solid = t.solid;
        let (accent, ink, bg) = match a.settings.menu_theme.as_str() {
            "bf1" => (
                [1., 0.64, 0.16, 1.],
                [0.07, 0.13, 0.29, 1.],
                [0.01, 0.03, 0.09, 1.],
            ),
            "bf2" => (
                [210. / 255., 40. / 255., 28. / 255., 1.],
                [1., 1., 1., 1.],
                [16. / 255., 40. / 255., 79. / 255., 1.],
            ),
            _ => (
                [1., 0.60, 0.12, 1.],
                [0.09, 0.05, 0.01, 1.],
                [0.012, 0.016, 0.024, 1.],
            ),
        };
        let white = [0.91, 0.93, 0.95, 1.];
        Self::rect(b, m, [0., 0., w, h], solid, bg);
        // MovieBackdrop's current left/right textures. Arrival, fade and depth
        // belong to Page; the provider owns playback and can change either
        // image/handle every frame without changing this presentation.
        let points = [(0., 1.), (0.30, 1.), (0.58, 0.6), (0.94, 0.), (1., 0.)];
        for (side, frame) in self.media.backdrops.into_iter().enumerate() {
            for pair in points.windows(2) {
                let (u, alpha) = pair[0];
                let (next, end_alpha) = pair[1];
                let (x0, x1, uv, a0, a1) = if side == 0 {
                    (
                        u * w * 0.56,
                        next * w * 0.56,
                        [u, 0., next, 1.],
                        alpha,
                        end_alpha,
                    )
                } else {
                    (
                        w - next * w * 0.56,
                        w - u * w * 0.56,
                        [1. - next, 0., 1. - u, 1.],
                        end_alpha,
                        alpha,
                    )
                };
                let drift = motion::ease((a.pulse_elapsed - self.entered) / 1.6, 0.2, 0.8, 0.2, 1.);
                let depth = self.depth_from + (self.depth_target - self.depth_from) * drift;
                let scale = 1. + depth * 0.035;
                let uv = uv.map(|v| 0.5 + (v - 0.5) / scale);
                let arrival = motion::ease(
                    (a.pulse_elapsed - if side == 0 { 0.1 } else { 0.5 }) / 2.6,
                    0.16,
                    0.84,
                    0.24,
                    1.,
                );
                let shift = (1. - arrival) * w * 0.09 * if side == 0 { -1. } else { 1. };
                Self::gradient(
                    b,
                    m,
                    [x0 + shift, 0., x1 + shift, h],
                    frame.texture,
                    frame.sample(uv),
                    [
                        [1., 1., 1., a0 * arrival],
                        [1., 1., 1., a0 * arrival],
                        [1., 1., 1., a1 * arrival],
                        [1., 1., 1., a1 * arrival],
                    ],
                );
            }
        }
        let wash = match a.settings.menu_theme.as_str() {
            "bf1" => [(0., 0.24), (0.35, 0.22), (0.65, 0.24), (1., 0.3)],
            "bf2" => [(0., 0.3), (0.35, 0.05), (0.65, 0.20), (1., 0.45)],
            _ => [(0., 0.35), (0.35, 0.05), (0.65, 0.10), (1., 0.55)],
        };
        for pair in wash.windows(2) {
            let tint = if a.settings.menu_theme == "bf1" {
                [0.08, 0.24, 0.62]
            } else {
                [0.; 3]
            };
            let a = [tint[0], tint[1], tint[2], pair[0].1];
            let z = [tint[0], tint[1], tint[2], pair[1].1];
            Self::gradient(
                b,
                m,
                [0., pair[0].0 * h, w, pair[1].0 * h],
                solid,
                UV,
                [a, z, a, z],
            );
        }
        // The centre scrim is elliptical in viewport space. A retained mesh
        // interpolates its radial alpha without stretching either side panel.
        for row in 0..16 {
            for col in 0..24 {
                let alpha = |x: f64, y: f64| {
                    let r = (((x - 0.5) / 0.30).powi(2) + ((y - 0.5) / 0.78).powi(2)).sqrt();
                    let a = if r < 0.5 {
                        0.93 + (0.78 - 0.93) * r / 0.5
                    } else {
                        (0.78 * (1. - r) / 0.5).max(0.)
                    };
                    [bg[0], bg[1], bg[2], a]
                };
                let x = col as f64 / 24.;
                let nx = (col + 1) as f64 / 24.;
                let y = row as f64 / 16.;
                let ny = (row + 1) as f64 / 16.;
                Self::gradient(
                    b,
                    m,
                    [x * w, y * h, nx * w, ny * h],
                    solid,
                    UV,
                    [alpha(x, y), alpha(x, ny), alpha(nx, y), alpha(nx, ny)],
                );
            }
        }
        let look_index = match a.settings.menu_theme.as_str() {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        if self.patterns[look_index] != 0 {
            b.quad(
                m,
                [0., 0., w, h],
                self.patterns[look_index],
                UV,
                [1.; 4],
                0.,
                0,
            );
        }
        if id == ScreenId::Main
            && !self.discord_asked
            && (a.pulse_elapsed - self.entered) >= 1.5
            && a.popup.is_none()
            && !a.profile.editing
        {
            self.discord_asked = true;
            self.discord_offer = true;
            self.discord_cursor = 0;
        }
        if !matches!(
            id,
            ScreenId::Gate
                | ScreenId::Main
                | ScreenId::Singleplayer
                | ScreenId::InstantAction
                | ScreenId::InstantActionEra
        ) {
            Self::rect(b, m, [0., 0., w, h], solid, [0., 0., 0., 0.5]);
        }
        if !matches!(
            id,
            ScreenId::Gate
                | ScreenId::Main
                | ScreenId::Profile
                | ScreenId::Singleplayer
                | ScreenId::InstantAction
                | ScreenId::InstantActionEra
        ) {
            let flight = motion::ease(
                (a.pulse_elapsed - self.brand_started) / 0.62,
                0.7,
                0.,
                0.25,
                1.,
            );
            let brand = self.brand_from + (self.brand_target - self.brand_from) * flight;
            self.brand(b, m, solid, w, h, brand, &a.settings.menu_theme, ink);
            motion::leaving(
                b,
                &self.leaving,
                a.pulse_elapsed - self.entered,
                self.forward,
            );
            if matches!(
                id,
                ScreenId::Multiplayer
                    | ScreenId::MultiplayerList(_)
                    | ScreenId::HostDialog
                    | ScreenId::DirectConnect
            ) {
                self.browser_draw(b, m, t, a, w, h, accent, ink, white);
            } else if matches!(id, ScreenId::Options | ScreenId::OptionsPage(_)) {
                // Existing interface widgets are painted by paint_widgets.
            } else {
                let rows = a.active_rows();
                let n = rows.len();
                let look = match a.settings.menu_theme.as_str() {
                    "bf1" => 0,
                    "bf2" => 1,
                    _ => 2,
                };
                let (top, heading_y, bh, gap) = Self::row_layout(h, n, look);
                let bw = (w * 0.30).min(400.).min(w * 0.86) - 16.;
                self.heading(
                    b,
                    m,
                    t,
                    w / 2.,
                    heading_y,
                    a.screen_title_str(id),
                    (h * 0.046).clamp(26., 50.),
                    look,
                );
                let selected = a.selected_cached_row_index(id);
                for (i, row) in rows.iter().enumerate() {
                    let progress = ((a.pulse_elapsed - self.entered - 0.12 - i.min(10) as f64 * 0.038)
                        / 0.42)
                        .clamp(0., 1.);
                    let eased = motion::ease(progress, 0.2, 0.8, 0.2, 1.);
                    let x =
                        (w - bw) / 2. + (1. - eased) * w * 0.06 * if self.forward { 1. } else { -1. };
                    let first = b.count;
                    let y = top + i as f64 * (bh + gap);
                    let r = [x, y, x + bw, y + bh];
                    let focused = i == selected && !row.disabled;
                    let label = if look != 1 && row.label.bytes().any(|c| c.is_ascii_lowercase()) {
                        std::borrow::Cow::Owned(row.label.to_uppercase())
                    } else {
                        std::borrow::Cow::Borrowed(row.label.as_str())
                    };
                    self.widget_button(b, m, t, r, &label, look, focused, row.disabled);
                    self.row_spans.push(motion::RowSpan {
                        first,
                        end: b.count,
                        index: i,
                        selected: focused,
                    });
                    for quad in first..b.count {
                        for vertex in 0..4 {
                            b.tint[(quad * 4 + vertex) * 4 + 3] *= eased as f32;
                        }
                    }
                    b.start = b.start.min(first * 4);
                    b.end = b.end.max(b.count * 4);
                    if !row.disabled && progress > 0.15 {
                        self.hits.push((r, Hit::Row(i)));
                    }
                }
                let edge = (w * 0.03).clamp(20., 56.);
                let r = [edge, h * 0.923, edge + 140., h * 0.97];
                self.widget_button(b, m, t, r, "BACK", look, false, false);
                self.hits.push((r, Hit::Back));
            }
        }
        let look = match a.settings.menu_theme.as_str() {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        if id == ScreenId::HostDialog {
            self.host_draw(b, m, t, a, w, h, accent, ink, white);
        }
        if matches!(a.popup, Some(PopupState::OnlineNotice { .. }))
            || a.current_screen() == ScreenId::DirectConnect
        {
            self.modal_start = Some(b.count);
        }
        if let Some(PopupState::OnlineNotice { message, .. }) = &a.popup {
            self.online_notice_form(b, m, t, a, w, h, message, ink, white);
        } else if a.current_screen() == ScreenId::DirectConnect {
            self.direct_form(b, m, t, a, w, h, look, accent, ink, white);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_frames_can_replace_handles_and_crops_without_named_artwork() {
        let mut app = ShellApp::new(crate::data::ShellData::default(), None);
        app.unlock_gate();
        app.pulse_elapsed = 4.;
        let mut page = Page::new(Font::default(), Font::default());
        let mut textures = ShellTextures::default();
        textures.by_name.insert("coreship_screen".into(), 99);
        let mut batch = Batch::default();
        for pair in [[101, 102], [201, 202]] {
            page.media.backdrops = pair.map(FrameTexture::new);
            page.draw(&mut batch, &textures, &app, 1280., 720.);
            assert!(pair
                .iter()
                .all(|id| batch.draws.iter().any(|d| d.texture == *id)));
            assert!(!batch.draws.iter().any(|d| d.texture == 99));
        }
        let crop = FrameTexture {
            texture: 201,
            uv: [0.25, 0., 0.75, 1.],
        };
        assert_eq!(crop.sample([0., 0., 1., 1.]), crop.uv);
        page.media = MediaTextures::default();
        page.draw(&mut batch, &textures, &app, 1280., 720.);
        assert!(!batch
            .draws
            .iter()
            .any(|d| [99, 101, 102, 201, 202].contains(&d.texture)));
    }
    #[test]
    fn pointer_targets_follow_the_page_viewport() {
        let mut app = ShellApp::new(crate::data::ShellData::default(), None);
        app.unlock_gate();
        app.mp_needs_notice = false;
        app.push_screen(ScreenId::Multiplayer);
        let mut page = Page::new(Font::default(), Font::default());
        let textures = ShellTextures::default();
        let mut batch = Batch::default();
        page.draw(&mut batch, &textures, &app, 1280., 720.);
        app.pulse_elapsed += 1.;
        for (w, h) in [(1280., 720.), (1920., 1080.)] {
            page.draw(&mut batch, &textures, &app, w, h);
            let edge = (w * 0.03).clamp(20., 56.);
            assert_eq!(
                page.hit(edge + 40., h * 0.946),
                Some(Hit::BrowserFooter(0))
            );
            assert_eq!(page.hit(1., h * 0.8), None);
        }
    }
    #[test]
    fn discord_offer_keys_and_dismissal() {
        let mut app = ShellApp::new(crate::data::ShellData::default(), None);
        app.unlock_gate();
        let mut page = Page::new(Font::default(), Font::default());
        let textures = ShellTextures::default();
        let mut batch = Batch::default();
        page.draw(&mut batch, &textures, &app, 1280., 720.);
        assert!(!page.discord_offer);
        app.pulse_elapsed = 1.6;
        page.draw(&mut batch, &textures, &app, 1280., 720.);
        assert!(page.discord_offer);
        assert_eq!(page.discord_key("ArrowRight"), Some(None));
        assert_eq!(page.discord_cursor, 1);
        assert_eq!(page.discord_key("Enter"), Some(Some("game")));
        assert!(!page.discord_offer);
    }
}
