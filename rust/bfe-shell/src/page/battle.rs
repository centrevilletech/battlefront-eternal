//! Page battle layout and look metrics. Spawn and scoreboard controllers own all
//! state and actions; Slint renders the battle screens in the front end's look while the game
//! renderer places the 3D turntable, holomap, and logos using `SpawnLayout`.
use bfe_render::interface::Color;

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
                [205. / 255., 228. / 255., 0.44, 0.44],
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
/// Retained header facts from page/view.js updateTeams. Whole seconds keep
/// clock text rebuilding at display changes rather than every world tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpawnBattleStatus {
    pub countdown: Option<(u32, bool)>,
    pub start: [f64; 2],
    pub bleeding: [bool; 2],
}

/// What the spawn screen's page layout depends on besides the window: the look, the objectives, the
/// header's lines, and each class's weapon rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnFacts {
    pub look: usize,
    pub goals: usize,
    pub countdown: bool,
    /// The player may switch teams: the other plate's state is a badge and the key hint shows.
    pub switchable: bool,
    pub classes: usize,
    /// Weapon rows of each class's window.
    pub rows: [u8; MAX_CLASSES],
    /// Device pixels per logical pixel (line boxes round in device pixels).
    pub scale: f64,
    /// The picked class (it keeps its full window when the roster folds).
    pub picked: usize,
}
pub const MAX_CLASSES: usize = 16;
impl Default for SpawnFacts {
    fn default() -> Self {
        Self { look: 2, goals: 2, countdown: false, switchable: true, classes: 0, rows: [2; MAX_CLASSES], scale: 1., picked: 0 }
    }
}

/// The screen/view.js stylesheet's metrics, in logical pixels them: clamped font
/// sizes, vh/vw spacing and line boxes. Slint draws inside the boxes with the same formulas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnMetrics {
    pub vh: f64,
    pub vw: f64,
    /// Panel border width: 1px, the 2005 look 2px.
    pub border: f64,
    pub bf2: bool,
    pub scale: f64,
}
/// "line-height: normal" of the screen font (Saira) it: the font's ascent (1.135 em) and
/// descent (.439 em), each rounded to a whole device pixel. Fits every line box measured in the browser build
/// (15 px at 10 px and scale 1, 16 at scale 2; 22 at 13.68 px; 33 at 21 px, scale 2).
pub fn line_at(f: f64, scale: f64) -> f64 {
    let d = f * scale;
    ((d * 1.135).round() + (d * 0.439).round()) / scale
}
impl SpawnMetrics {
    pub fn new(w: f64, h: f64, look: usize, scale: f64) -> Self {
        Self { vh: h / 100., vw: w / 100., border: if look == 1 { 2. } else { 1. }, bf2: look == 1, scale: scale.max(0.5) }
    }
    pub fn line(&self, f: f64) -> f64 {
        line_at(f, self.scale)
    }
    fn clamp(&self, lo: f64, vh: f64, hi: f64) -> f64 {
        (vh * self.vh).clamp(lo, hi)
    }
    // .bfe-spawn-label
    pub fn label_font(&self) -> f64 {
        self.clamp(11., 1.45, 14.)
    }
    // .bfe-spawn-class-name
    pub fn class_font(&self) -> f64 {
        self.clamp(13., 1.9, 19.)
    }
    // .bfe-spawn-weapons
    pub fn weapon_font(&self) -> f64 {
        self.clamp(10., 1.3, 12.)
    }
    /// The class bar's top margin, vertical padding and side inset (the 2005 look's capsule).
    pub fn class_bar(&self) -> (f64, f64, f64) {
        if self.bf2 {
            (0.5 * self.vh, 0.35 * self.vh, 0.5 * self.vw)
        } else {
            (0., 0.55 * self.vh, 0.)
        }
    }
    pub fn class_bar_height(&self) -> f64 {
        self.class_bar().1 * 2. + self.line(self.class_font())
    }
    /// One class window: border, bar, weapon grid (padding .7vh/.8vh, rows .25em apart).
    /// A folded window (.bfe-spawn-roster.compact): its weapons on one line, padding .45vh/.55vh.
    pub fn compact_height(&self) -> f64 {
        2. * self.border + self.class_bar().0 + self.class_bar_height() + self.vh + self.line(self.weapon_font())
    }
    pub fn class_height(&self, rows: u8) -> f64 {
        let f = self.weapon_font();
        let rows = f64::from(rows.max(1));
        let grid = 1.5 * self.vh + rows * self.line(f) + (rows - 1.) * 0.25 * f;
        2. * self.border + self.class_bar().0 + self.class_bar_height() + grid
    }
    // .bfe-spawn-side-*
    pub fn team_name_font(&self) -> f64 {
        self.clamp(16., 2.6, 28.)
    }
    pub fn team_state_font(&self) -> f64 {
        self.clamp(9., 1.2, 12.)
    }
    pub fn team_count_font(&self) -> f64 {
        self.clamp(18., 2.8, 30.)
    }
    pub fn emblem(&self) -> f64 {
        self.clamp(46., 7.4, 80.)
    }
    pub fn swap(&self) -> f64 {
        self.clamp(32., 4.6, 46.)
    }
    /// A team plate's height: its name over its state (a badge while switchable) or the count.
    pub fn plate_height(&self, badge: bool) -> f64 {
        let state = self.line(self.team_state_font()) + if badge { 0.5 * self.team_state_font() } else { 0. };
        let text = self.team_name_font() + 0.4 * self.vh + state;
        2. * self.border + 1.8 * self.vh + text.max(self.line(self.team_count_font()))
    }
    // .bfe-spawn-teamkeys: kbd at 1.1em, line height 1.2, .1em padding, 1px border.
    pub fn keys_height(&self) -> f64 {
        self.clamp(9., 1.1, 11.) * 1.54 + 2.
    }
    // .bfe-spawn-class-title: line height 1, a 3px rule .3em under it.
    pub fn title_font(&self) -> f64 {
        self.clamp(22., 4.2, 46.)
    }
    pub fn at_font(&self) -> f64 {
        self.clamp(10., 1.35, 13.)
    }
    pub fn post_font(&self) -> f64 {
        self.clamp(15., 2.2, 24.)
    }
    pub fn caption_height(&self) -> f64 {
        let t = self.title_font();
        t * 1.3 + 3. + 0.7 * self.vh + self.line(self.post_font()).max(self.line(self.at_font()))
    }
    pub fn go_height(&self) -> f64 {
        self.clamp(50., 7., 74.)
    }
    // .bfe-spawn-bar, .bfe-spawn-goals
    pub fn panel_bar(&self) -> (f64, f64, f64) {
        if self.bf2 {
            (0.5 * self.vh, 0.35 * self.vh, 0.5 * self.vw)
        } else {
            (0., 0.55 * self.vh, 0.)
        }
    }
    pub fn panel_font(&self) -> f64 {
        self.clamp(11., 1.5, 14.)
    }
    pub fn goal_font(&self) -> f64 {
        self.clamp(11., 1.55, 15.)
    }
    pub fn countdown_font(&self) -> f64 {
        self.clamp(12., 1.7, 16.)
    }
}

/// CSS grid, in compositor logical pixels. The caller converts only
/// published hit rectangles to the existing 1024-wide input plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnLayout {
    pub width: f64,
    pub height: f64,
    pub edge: f64,
    pub metrics: SpawnMetrics,
    pub roster: [f64; 4],
    /// The SELECT SOLDIER label's line box.
    pub label: [f64; 4],
    pub figure: [f64; 4],
    /// The caption under him (class title, post and vehicles), across the stage column.
    pub caption: [f64; 4],
    pub objectives: [f64; 4],
    pub map: [f64; 4],
    pub spawn: [f64; 4],
    pub teams: [[f64; 4]; 2],
    /// The key hint's line under the teams (when switchable), and the countdown's.
    pub keys_y: f64,
    pub countdown_y: f64,
    pub center: f64,
    /// The classes do not fit: all but the picked one fold to a line (keys.compact).
    pub compact: bool,
    class_tops: [f64; MAX_CLASSES + 1],
}
impl SpawnLayout {
    pub fn new(w: f64, h: f64, facts: SpawnFacts) -> Self {
        let m = SpawnMetrics::new(w, h, facts.look, facts.scale);
        let (vw, vh) = (m.vw, m.vh);
        // --bfe-gutter clamp(20px, 3vw, 56px); --bfe-page-frame min(100vw - 2 gutter, 1680px), centred.
        let gutter = (3. * vw).clamp(20., 56.);
        let frame = (w - 2. * gutter).min(1680.);
        let edge = (w - frame) * 0.5;
        // grid-template-columns: minmax(250px, 23%) 1fr minmax(300px, 33%), column-gap 1.6vw.
        let left = (frame * 0.23).max(250.);
        let right = (frame * 0.33).max(300.);
        let gap = 1.6 * vw;
        let middle = [edge + left + gap, edge + frame - right - gap];
        let center = (middle[0] + middle[1]) * 0.5;
        // Header: the teams (plates centred in their row), the key hint, the countdown; .6vh apart.
        let top = 2.4 * vh;
        let own = m.plate_height(false);
        let other = m.plate_height(facts.switchable);
        let teams_h = own.max(other).max(m.emblem());
        let plate_w = (24. * vw).min(340.);
        let half = m.swap() * 0.5 + 0.9 * vw;
        let plate = |i: usize| {
            let ph = if i == 0 { other } else { own };
            let y = top + (teams_h - ph) * 0.5;
            if i == 0 {
                [w * 0.5 - half - plate_w, y, w * 0.5 - half, y + ph]
            } else {
                [w * 0.5 + half, y, w * 0.5 + half + plate_w, y + ph]
            }
        };
        let mut head = teams_h;
        let keys_y = top + head + 0.8 * vh;
        if facts.switchable {
            head += 0.8 * vh + m.keys_height();
        }
        let countdown_y = top + head + 0.6 * vh;
        if facts.countdown {
            head += 0.6 * vh + m.line(m.countdown_font());
        }
        let body = top + head;
        let foot = h - 2.8 * vh;
        // SELECT SOLDIER (padding-top 1vh), 1.1vh, then the class windows .9vh apart.
        let label_y = body + vh;
        let label_h = m.line(m.label_font());
        let first_y = label_y + label_h + 1.1 * vh;
        let stack = |compact: bool| {
            let mut tops = [0.; MAX_CLASSES + 1];
            let mut y = first_y;
            for (i, (top, &row)) in tops[..MAX_CLASSES].iter_mut().zip(facts.rows.iter()).enumerate() {
                *top = y;
                let h = if compact && i != facts.picked { m.compact_height() } else { m.class_height(row) };
                y += h + 0.9 * vh;
            }
            tops[MAX_CLASSES] = y;
            tops
        };
        let mut class_tops = stack(false);
        let n = facts.classes.min(MAX_CLASSES);
        let compact = n > 0 && class_tops[n] - 0.9 * vh > foot + 2.;
        if compact {
            class_tops = stack(true);
        }
        // The stage: his place, the caption (.4vh above it), SPAWN (1.4vh above it, at most 360 wide).
        let go_h = m.go_height();
        let go_w = (middle[1] - middle[0]).min(360.);
        let go_y = foot - go_h;
        let caption_h = m.caption_height();
        let caption_y = go_y - 1.4 * vh - caption_h;
        let figure_w = (middle[1] - middle[0]).min(46. * vh);
        let figure_bottom = caption_y - 0.4 * vh;
        // OBJECTIVES (padding-top 1vh), the map 1.2vh under it.
        let (bar_margin, bar_pad, _) = m.panel_bar();
        let goals_h = if facts.goals == 0 {
            0.
        } else {
            1.9 * vh
                + facts.goals as f64 * m.line(m.goal_font())
                + (facts.goals as f64 - 1.) * 0.5 * vh
        };
        let objectives_h = if facts.goals == 0 {
            0.
        } else {
            2. * m.border + bar_margin + 2. * bar_pad + m.line(m.panel_font()) + goals_h
        };
        let objectives_y = body + vh;
        let map_y = if facts.goals == 0 { objectives_y } else { objectives_y + objectives_h + 1.2 * vh };
        Self {
            width: w,
            height: h,
            edge,
            metrics: m,
            roster: [edge, body, edge + left, foot],
            label: [edge, label_y, edge + left, label_y + label_h],
            figure: [center - figure_w * 0.5, body, center + figure_w * 0.5, figure_bottom],
            caption: [middle[0], caption_y, middle[1], caption_y + caption_h],
            objectives: [w - edge - right, objectives_y, w - edge, objectives_y + objectives_h],
            map: [w - edge - right, map_y, w - edge, foot],
            spawn: [center - go_w * 0.5, go_y, center + go_w * 0.5, foot],
            center,
            teams: [plate(0), plate(1)],
            keys_y,
            countdown_y,
            compact,
            class_tops,
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
        let i = index.min(MAX_CLASSES - 1);
        // .bfe-spawn-class.picked: translateX(10px).
        let x = self.roster[0] + if picked { 10. } else { 0. };
        let y = self.class_tops[i];
        [x, y, self.roster[2] + if picked { 10. } else { 0. }, self.class_tops[i + 1] - 0.9 * self.metrics.vh]
    }
}

#[cfg(test)]
mod spawn_layout_tests {
    use super::*;
    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1.6, "{a} != {b}");
    }
    // Boxes the browser build measured (getBoundingClientRect) on its spawn screen.
    #[test]
    fn spawn_layout_matches_the_browser_build() {
        // The owner's 2512x1500 panel at scale 2, the 2005 look, the other team switchable.
        let l = SpawnLayout::new(1256., 750., SpawnFacts { look: 1, classes: 5, scale: 2., ..SpawnFacts::default() });
        near(l.edge, 37.7);
        near(l.label[1], 105.8);
        let first = l.class_rect(0, false);
        near(first[1], 131.6);
        near(first[3] - first[1], 81.3);
        near(l.class_rect(1, false)[1], 219.6);
        near(l.objectives[3] - l.objectives[1], 85.);
        near(l.map[1], 199.8);
        near(l.figure[3], 588.3);
        near(l.spawn[1], 676.5);
        near(l.teams[0][0], 298.);
        // 1920x1080, the Eternal look.
        let l = SpawnLayout::new(1920., 1080., SpawnFacts { look: 2, classes: 5, ..SpawnFacts::default() });
        near(l.edge, 120.);
        near(l.label[1], 144.1);
        let first = l.class_rect(0, true);
        near(first[0], 130.);
        near(first[1], 178.);
        near(first[3] - first[1], 101.);
        near(l.objectives[3] - l.objectives[1], 109.8);
        near(l.caption[1], 854.2);
        near(l.teams[0][0], 579.7);
        // 1280x720 at scale 1, the 2005 look: 22px names and 15px weapon rows make 77.9px windows.
        let l = SpawnLayout::new(1280., 720., SpawnFacts { look: 1, classes: 5, ..SpawnFacts::default() });
        near(l.label[1], 103.1);
        let first = l.class_rect(0, false);
        near(first[1], 128.);
        near(first[3] - first[1], 77.9);
        near(l.class_rect(4, false)[1], 465.5);
        near(l.objectives[3] - l.objectives[1], 82.9);
        near(l.spawn[1], 649.5);
    }
    #[test]
    fn a_roster_that_does_not_fit_folds_all_but_the_picked_class() {
        let facts = SpawnFacts { look: 1, classes: 9, picked: 2, ..SpawnFacts::default() };
        let l = SpawnLayout::new(1280., 720., facts);
        assert!(l.compact);
        let picked = l.class_rect(2, true);
        let other = l.class_rect(3, false);
        assert!(picked[3] - picked[1] > other[3] - other[1] + 10.);
        assert!(!SpawnLayout::new(1280., 720., SpawnFacts { look: 1, classes: 5, ..SpawnFacts::default() }).compact);
    }
    #[test]
    fn line_boxes_round_in_device_pixels() {
        for (f, scale, h) in [(10., 1., 15.), (10., 2., 16.), (13.68, 1., 22.), (14.25, 2., 22.5), (21., 2., 33.), (16.5, 2., 25.5)] {
            assert_eq!(line_at(f, scale), h, "{f}px at {scale}");
        }
    }
}
