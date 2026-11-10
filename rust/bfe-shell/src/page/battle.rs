//! Page battle layout and look metrics. Spawn and scoreboard controllers own all
//! state and actions; Slint renders the page-look battle screens while the game
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

/// CSS grid from page/view.js, in compositor logical pixels. The caller converts only
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
    /// The countdown is another flex line in page/view.js's header. Its
    /// inherited 1.5 line height and .6vh gap consume real grid space.
    pub fn with_countdown(mut self, shown: bool) -> Self {
        if shown {
            let extra = (self.height * 0.017).clamp(12., 16.) * 1.5 + self.height * 0.006;
            self.roster[1] += extra;
            self.figure[1] += extra;
            self.objectives[1] += extra;
            self.objectives[3] += extra;
            self.map[1] += extra;
        }
        self
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
