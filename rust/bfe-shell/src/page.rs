//! The front end's own look laid over ShellApp's screen stack: geometry and
//! looks of the three themes (bf1, bf2, eternal).
mod battle;
mod browser;
mod media;
mod motion;
use crate::{
    render::ShellTextures,
    screens::{PopupState, ScreenId, ShellApp},
};
pub use battle::{BattleLook, SpawnBattleStatus, SpawnFacts, SpawnLayout, SpawnMetrics, MAX_CLASSES};
use bfe_render::interface::{anchor, Batch, Color, Font, Matrix, UV};
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
}

pub struct Page {
    pub browser: BrowserView,
    pub media: MediaTextures,
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
    pub dropdown: bool,
    pub dropdown_cursor: usize,
    pub discord_offer: bool,
    pub discord_cursor: usize,
    pub discord_asked: bool,
    pub buttons: [u32; 6],
    pub button_source: [f64; 2],
    pub patterns: [u32; 3],
    pub modal_start: Option<usize>,
    hits: Vec<([f64; 4], Hit)>,
    text: HashMap<String, Vec<u16>>,
    screen: ScreenId,
    pub entered: f64,
    depth_from: f64,
    depth_target: f64,
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
            prose: Font {height: regular.height,glyphs:regular.glyphs.clone()},
            mono: Font {height: regular.height,glyphs:regular.glyphs.clone()},
            regular,
            medium,
            bold,
            viewport: [1280., 720.],
            browser: BrowserView::default(),
            media: MediaTextures::default(),
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
            modal_start: None,
            hits: Vec::with_capacity(64),
            text: HashMap::new(),
            screen: ScreenId::Gate,
            entered: 0.,
            depth_from: 0.,
            depth_target: 0.,
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
            let drift = motion::ease((a.pulse_elapsed - self.entered) / 1.6, 0.2, 0.8, 0.2, 1.);
            self.depth_from += (self.depth_target - self.depth_from) * drift;
            self.depth_target = a.stack.len().saturating_sub(1).min(4) as f64;
            self.screen = id;
            self.entered = a.pulse_elapsed;
            self.text.clear();
            self.dropdown = false;
            self.discord_offer = false;
        }
        b.begin();
        // anchor uses a 1024-wide plane. Scale its two axes independently so
        // CSS viewport units retain their actual pixel positions at any aspect.
        let mut m = anchor(0., 0., false, h);
        m[0] *= 1024. / w;
        let solid = t.solid;
        let bg = match a.settings.menu_theme.as_str() {
            "bf1" => [0.01, 0.03, 0.09, 1.],
            "bf2" => [16. / 255., 40. / 255., 79. / 255., 1.],
            _ => [0.012, 0.016, 0.024, 1.],
        };
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
        let look_index = look_index(&a.settings.menu_theme);
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
                | ScreenId::Options
                | ScreenId::OptionsPage(_)
                | ScreenId::Singleplayer
                | ScreenId::InstantAction
                | ScreenId::InstantActionEra
        ) {
            Self::rect(b, m, [0., 0., w, h], solid, [0., 0., 0., 0.5]);
        }
    }
}

/// The `patterns` slot of a menu theme: bf1, bf2, else Eternal.
pub fn look_index(menu_theme: &str) -> usize {
    match menu_theme {
        "bf1" => 0,
        "bf2" => 1,
        _ => 2,
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
