//! Classic spawn screen view (`src/spawn-screen/classic/view.js`,
//! `src/spawn-screen/classic/preview.js`).
//!
//! Renders the 2004 `ifs_spawndisplay` as a layer of the battle HUD:
//! - Under the HUD's quads (`render_before`): the translucent blue grid room
//!   (`shell_cube`).
//! - In the HUD's main quad batch (`draw`): five tilted soldier class windows
//!   (`border_3_pieces`) with orange title bars (`headerbutton*`) and weapon
//!   lists; the objectives and spawn point windows on the right; and the tipped
//!   `SPAWN` button (`opaque_rect`) at the bottom.
//! - Over the HUD's quads (`render_after`): the two spinning faction logos
//!   (`com_icon_*`) at the top (the picked team's large and green on the left,
//!   the other small and red at `0.35` strength on the right) and the standing
//!   soldier body in the center.
//! - Over everything (`draw_over`): the mouse cursor (`mousecursor`).

use std::f64::consts::PI;

use bfe_render::hud_model::layout::{HudFrame as H, HudMapDraw as M, HudRing as R};
use bfe_render::hud_model::{MAP_DRAW, RINGS};
use bfe_render::interface::{alpha, rotate_y, translate, Batch, Color, Matrix, UV};

use crate::shapes::{
    draw_nine_slice_window, encode_utf16_into, inside_quad, place_interface_model,
    projected_corners, root_anchor, rotate_x, scale_xyz, wrap_lines_ranges, BattleUiAssets,
    BattleUiSurface, GameFamily, GameFamilyError, TitleIntro, BODY_UNITS, DROIDEKA_UNITS,
    FONT_TINY, LOGO_UNITS, ROOM_SCALE,
};
pub use crate::spawn::controller::SpawnScreenController;
use crate::spawn::input::SpawnHitTarget;
use crate::spawn::layout::{
    classic_layout, spawn_layout_for_family, ClassSlotLayout, ClassicSpawnLayout, TiltedBoxLayout,
    SLOTS,
};
use crate::spawn::model::{SpawnClassEntry, SpawnModel, SpawnPort, TeamSwitchSource};

/// Opaque white tint `[1.0, 1.0, 1.0, 1.0]` for active windows, icons, and labels.
const WHITE: Color = [1.0, 1.0, 1.0, 1.0];
/// Opaque black tint `[0.0, 0.0, 0.0, 1.0]` for title bar text over orange headers.
const BLACK: Color = [0.0, 0.0, 0.0, 1.0];
/// Hologram RGB color `[32/255, 223/255, 32/255]` for the currently selected team's logo (`GREEN`).
pub const LOGO_GREEN: [f64; 3] = [32.0 / 255.0, 223.0 / 255.0, 32.0 / 255.0];
/// Hologram RGB color `[223/255, 32/255, 32/255]` for the opposing team's logo (`RED`).
pub const LOGO_RED: [f64; 3] = [223.0 / 255.0, 32.0 / 255.0, 32.0 / 255.0];
/// Alpha multiplier (`128 / 255`) for unpicked soldier class windows (`UNPICKED` in `view.js`).
const UNPICKED_ALPHA: f64 = 128.0 / 255.0;
/// Stagger delay (`0.05s`) between consecutive soldier class window `TitleIntro` animations.
const SLOT_DELAY: f64 = 0.05;
/// Vertical gap (`5.0` units) between a window's top edge and the bottom of its title text line.
const TITLE_GAP: f64 = 5.0;
/// Horizontal inset (`18.0` units) from a window's left edge to the start of its title text.
const TITLE_X: f64 = 18.0;
/// Distance (`6.3` units) the orange `headerbutton*` bar extends above the title text line top.
const BAR_OVER: f64 = 6.3;
/// Distance (`0.3` units) between the bottom of the orange `headerbutton*` bar and the window top.
const BAR_UNDER: f64 = 0.3;
/// Left inset (`1.75` units) of the orange `headerbutton*` bar from the window's left edge.
const BAR_LEFT: f64 = 1.75;
/// Right inset (`3.9` units) of the orange `headerbutton*` bar from the window's right edge.
const BAR_RIGHT: f64 = 3.9;
/// Duration (`0.25s`) of the linear position/size/opacity swap between the two faction logos.
pub const LOGO_SWAP: f64 = 0.25;
/// Opacity (`0.35`) of the opposing (non-picked) faction logo on the right (`LOGO_DIM`).
pub const LOGO_DIM: f64 = 0.35;
/// Fast Y-axis spin rate (`-600 deg/s`, `OmegaY = -20`) while the faction logos are swapping places.
const SPIN_SWAP: f64 = -600.0 * PI / 180.0;
/// Steady Y-axis spin rate (`-9 deg/s`, `OmegaY = -0.3`) after the logo swap completes.
const SPIN_REST: f64 = -9.0 * PI / 180.0;
/// Initial Y-axis spin rate (`6 deg/s`, `OmegaY = 0.2`) before the first logo swap.
const SPIN_START: f64 = 6.0 * PI / 180.0;
/// Angular return rate (`15.0 rad/s`) of the HUD reinforcement counter flip after a mouse team switch.
pub const FLIP_RATE: f64 = 15.0;
/// Fade-out rate (`10.0 / s`) for deselected command post rings on the tactical spawn map.
pub const RING_FADE: f64 = 10.0;
/// Width and height (`32.0` interface units) of the `mousecursor` quad.
const POINTER_SIZE: f64 = 32.0;

/// Linearly interpolates between `a` and `b` by factor `t`.
#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Returns the fallback hologram model name for a faction side
/// (`src/spawn-screen/classic/view.js` `FALLBACK`).
///
/// Maps `"cis"` and `"geo"` to `"com_icon_cis"`, `"imp"` to `"com_icon_imperial"`,
/// `"all"` to `"com_icon_alliance"`, and Republic/native allies (`"rep"`, `"gar"`,
/// `"gun"`, `"wok"`) to `"com_icon_republic"`.
pub fn fallback_logo_for_side(side: &str) -> &'static str {
    match side {
        "cis" | "geo" => "com_icon_cis",
        "imp" => "com_icon_imperial",
        "all" => "com_icon_alliance",
        _ => "com_icon_republic",
    }
}

/// Placement of the `spawn_map()` square on the `1024xH` interface plane
/// (`src/hud.js` `spawnMapPlace` and `bfe_render::hud_model::map::spawn_map`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnMapPlacement {
    /// Horizontal center of the tactical spawn map in `1024xH` interface units.
    pub x: f64,
    /// Vertical center of the tactical spawn map (`height * 0.5`).
    pub y: f64,
    /// Width and height of the square tactical spawn map (`328 * 1024 / 800 = 419.84`).
    pub size: f64,
}

/// Computes the center `(x, y)` and side length `size` (`419.84` units) of the tactical
/// minimap square on the spawn screen (`src/hud.js` `spawnMapPlace`).
///
/// Sits inside the right edge of the 90% safe area (`0.95 * 1024 - 128 * (1024 / 800)`)
/// and vertically centered at `height * 0.5` between the objectives and post windows.
#[inline]
pub fn spawn_map_place(height: f64) -> SpawnMapPlacement {
    SpawnMapPlacement {
        x: (0.05 + 0.9) * 1024.0 - 128.0 * (1024.0 / 800.0),
        y: height * 0.5,
        size: 328.0 * (1024.0 / 800.0),
    }
}

/// World-to-map projection params matching `HudFrame::MAP_X`, `MAP_Z`, `MAP_HALF`,
/// and `postIconSize` (`6.0 * MAP_SCALE`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapProjection {
    /// World-space X center of the tactical minimap (`HudFrame::MAP_X`).
    pub map_x: f64,
    /// World-space Z center of the tactical minimap (`HudFrame::MAP_Z`).
    pub map_z: f64,
    /// World-space half-extent of the tactical minimap square (`HudFrame::MAP_HALF`).
    pub map_half: f64,
    /// Side length of a command post flag icon in interface units (`15.0` at 4:3).
    pub post_icon_size: f64,
}

impl Default for MapProjection {
    fn default() -> Self {
        Self {
            map_x: 0.0,
            map_z: 0.0,
            map_half: 256.0,
            post_icon_size: 15.0,
        }
    }
}

impl MapProjection {
    /// Projects world `(wx, wz)` into map-local offsets `(px, py)` for a map of
    /// `size` interface units (`bfe_render::hud_model::map::map_point`).
    #[inline]
    pub fn map_point(&self, wx: f64, wz: f64, size: f64) -> [f64; 2] {
        let half = self.map_half.max(1e-3);
        [
            ((wx - self.map_x) / half) * (size * 0.5),
            ((wz - self.map_z) / half) * (size * 0.5),
        ]
    }
}

/// 3D placement of a single spinning team logo on the spawn screen.
///
/// Holds the resolved hologram asset name (`com_icon_*`), world-space position on the
/// interface camera plane, uniform scale (`size * LOGO_UNITS`), Y rotation angle,
/// green/red RGB tint, opacity, and UV scroll timestamp.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnLogoPlacement {
    /// Team number (`1` or `2`) represented by this hologram.
    pub team: u8,
    /// Resolved `.msh` model name (`"com_icon_republic"`, `"com_icon_cis"`, etc.).
    pub model_name: String,
    /// Whether the logo should be drawn (`can_switch_teams && !covered`).
    pub visible: bool,
    /// World-space position in the `InterfaceCamera` scene at `z = -PLANE`.
    pub position: [f64; 3],
    /// Uniform model scale (`size * LOGO_UNITS`).
    pub scale: f64,
    /// Y-axis rotation angle in radians (`self.spin`).
    pub rot_y: f64,
    /// RGB hologram tint (`LOGO_GREEN` when picked, `LOGO_RED` when opposing).
    pub color: [f64; 3],
    /// Hologram opacity (`1.0` when picked, `LOGO_DIM = 0.35` when opposing).
    pub opacity: f64,
    /// Elapsed view time in seconds for hologram scanline UV scrolling.
    pub time: f64,
}

/// 3D placement of the `shell_cube` room, team logos, and standing soldier
/// preview body for the current frame (`renderRoom` and `renderModels` in
/// `src/spawn-screen/classic/view.js`).
///
/// The soldier preview stands with its feet at `(W / 2, H / 1.2)` facing the camera
/// without a turntable (`src/spawn-screen/classic/preview.js`), scaled to `BODY_UNITS`
/// (`199.68` units/m) or `DROIDEKA_UNITS` (`0.9 * BODY_UNITS`) for droidekas.
#[derive(Clone, Debug, PartialEq)]
pub struct Spawn3dPlacement {
    /// World-space center of the `shell_cube` background grid room.
    pub room_position: [f64; 3],
    /// Uniform scale of the `shell_cube` room (`ROOM_SCALE = 1.6 / 0.009`).
    pub room_scale: f64,
    /// Elapsed view time in seconds written to `LOOK.TIME` on room and logo materials.
    pub time: f64,
    /// Placements for team 1 and team 2 spinning faction logos.
    pub logos: [SpawnLogoPlacement; 2],
    /// Selected soldier class ODF name to display in the center preview (`None` if empty).
    pub preview_class: Option<String>,
    /// World-space feet position of the standing soldier preview (`(W/2, H/1.2)` on the plane).
    pub preview_position: [f64; 3],
    /// Uniform scale of the soldier preview (`DROIDEKA_UNITS` or `BODY_UNITS`).
    pub preview_scale: f64,
}

/// Sound effects emitted by the spawn screen (`src/spawn-screen/index.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnSound {
    /// `"SpawnDisplaySpawnPointChange"` when the post or class selection changes.
    SpawnPointChange,
    /// `"SpawnDisplaySpawnPointAccept"` when `SPAWN` is accepted.
    SpawnPointAccept,
}

/// Classic spawn screen view (`src/spawn-screen/classic/view.js` `ClassicView`).
///
/// Owns all visual animation clocks (`slots`, `objectives_title`, `post_title`,
/// logo `swap`/`spin`, reinforcement `flip`, and command post highlight `rings`)
/// plus pre-allocated UTF-16 and line-range scratch buffers so `draw` performs
/// zero heap allocations per frame once initialized.
pub struct ClassicSpawnView {
    /// Game family (`Swbf1` for 2004 Classic layout).
    pub family: GameFamily,
    /// Cumulative time in seconds since the view was created (drives hologram shaders).
    pub time: f64,
    /// Five staggered `TitleIntro` animations for the left-hand soldier class windows.
    pub slots: [TitleIntro; SLOTS],
    /// `TitleIntro` animation for the top-right `OBJECTIVES:` window header.
    pub objectives_title: TitleIntro,
    /// `TitleIntro` animation for the bottom-right command-post window header.
    pub post_title: TitleIntro,
    /// Remaining angle in radians (`PI -> 0`) for the HUD reinforcement counter flip.
    pub flip: f64,
    /// Elapsed time (`0.0..=LOGO_SWAP`) in the current team logo swap animation.
    pub swap: f64,
    /// Current Y-axis rotation angle in radians of both spinning faction logos.
    pub spin: f64,
    /// Current Y-axis angular velocity (`SPIN_START`, `SPIN_SWAP`, or `SPIN_REST`).
    pub spin_rate: f64,
    /// True while the pause menu covers the spawn screen so the software pointer hides.
    pub hide_pointer: bool,
    /// World-to-minimap projection parameters for command post hit-testing and ring sizing.
    pub map_proj: MapProjection,
    /// Resolved texture asset handles (`border_3_pieces`, `headerbutton*`, `opaque_rect`, etc.).
    pub assets: BattleUiAssets,
    /// Cached layout for the most recent `(height, fh)` pair.
    cached_layout: Option<ClassicSpawnLayout>,
    /// Retained post highlight rings (`(post_name, alpha)`), updated in place
    /// so frame updates never allocate.
    rings: Vec<(String, f64)>,
    /// Reusable UTF-16 text scratch buffer for zero-allocation drawing.
    scratch_utf16: Vec<u16>,
    /// Reusable wrapped-line slice ranges `(start, end)`.
    scratch_lines: Vec<(usize, usize)>,
    /// Reusable objectives paragraph buffer.
    scratch_paragraphs: Vec<(usize, usize)>,
}

impl Default for ClassicSpawnView {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassicSpawnView {
    /// Creates a new `ClassicSpawnView` with finished title intros and pre-allocated
    /// scratch buffers for rings, UTF-16 text, and wrapped line ranges.
    pub fn new() -> Self {
        Self {
            family: GameFamily::Swbf1,
            time: 0.0,
            slots: [TitleIntro::new(); SLOTS],
            objectives_title: TitleIntro::new(),
            post_title: TitleIntro::new(),
            flip: 0.0,
            swap: LOGO_SWAP,
            spin: 0.0,
            spin_rate: SPIN_START,
            hide_pointer: false,
            map_proj: MapProjection::default(),
            assets: BattleUiAssets::default(),
            cached_layout: None,
            rings: Vec::with_capacity(16),
            scratch_utf16: Vec::with_capacity(256),
            scratch_lines: Vec::with_capacity(16),
            scratch_paragraphs: Vec::with_capacity(16),
        }
    }

    /// Constructs a `ClassicSpawnView` for `family`, returning an explicit [`GameFamilyError`]
    /// for [`GameFamily::Swbf2`].
    pub fn try_with_family(family: GameFamily) -> Result<Self, GameFamilyError> {
        family.require_available()?;
        Ok(Self::new())
    }

    /// Returns the cached [`ClassicSpawnLayout`] for interface height `h` and
    /// `gamefont_tiny` line height `fh`.
    pub fn layout(&mut self, h: f64, fh: f64) -> ClassicSpawnLayout {
        if let Some(cached) = self.cached_layout {
            if (cached.h - h).abs() < 1e-6 && (cached.fh - fh).abs() < 1e-6 {
                return cached;
            }
        }
        let next = spawn_layout_for_family(self.family, h, fh)
            .unwrap_or_else(|_| classic_layout(h, fh));
        self.cached_layout = Some(next);
        next
    }

    /// Reacts to the spawn screen opening (`ClassicView.open`).
    ///
    /// Staggers the five class title intros by `SLOT_DELAY` (`0.05s`) per slot,
    /// restarts the objectives and command-post title intros immediately, triggers
    /// the initial logo swap when `first` is true, and zeroes any prior ring highlights.
    pub fn on_open(&mut self, first: bool) {
        for (i, intro) in self.slots.iter_mut().enumerate() {
            intro.start(SLOT_DELAY * (i as f64));
        }
        self.objectives_title.start(0.0);
        self.post_title.start(0.0);
        if first {
            self.swap_logos();
        }
        self.flip = 0.0;
        for (_, alpha) in &mut self.rings {
            *alpha = 0.0;
        }
    }

    /// Reacts to a team switch (`ClassicView.teamChanged`).
    ///
    /// Replays the staggered class title intros, starts a 0.25s logo swap at `-600 deg/s`,
    /// and if triggered by a mouse click on the logos, sets `flip = PI` so the HUD
    /// reinforcement counter flips over at `FLIP_RATE` (`15 rad/s`).
    pub fn on_team_changed(&mut self, source: TeamSwitchSource) {
        for (i, intro) in self.slots.iter_mut().enumerate() {
            intro.start(SLOT_DELAY * (i as f64));
        }
        self.swap_logos();
        if source == TeamSwitchSource::Mouse {
            self.flip = PI;
        }
    }

    /// Reacts to a command post pick change (`ClassicView.postChanged`).
    pub fn on_post_changed(&mut self) {
        self.post_title.start(0.0);
    }

    /// Starts the `0.25s` faction logo swap animation at `SPIN_SWAP` (`-600 deg/s`).
    pub fn swap_logos(&mut self) {
        self.swap = 0.0;
        self.spin_rate = SPIN_SWAP;
    }

    /// Steps animations and ring highlights by `dt` seconds (`ClassicView.update`).
    ///
    /// Advances all seven `TitleIntro` timers, transitions `spin_rate` from `SPIN_SWAP`
    /// (`-600 deg/s`) back to `SPIN_REST` (`-9 deg/s`) once `swap >= LOGO_SWAP`, decays
    /// `flip` toward `0`, and fades unselected post rings at `RING_FADE` (`10/s`) while
    /// pinning the currently selected command post's ring alpha to `1.0`.
    pub fn update(&mut self, dt: f64, model: &SpawnModel) {
        self.time += dt;
        for intro in &mut self.slots {
            intro.update(dt);
        }
        self.objectives_title.update(dt);
        self.post_title.update(dt);
        self.swap = (self.swap + dt).min(LOGO_SWAP);
        if self.swap >= LOGO_SWAP && (self.spin_rate - SPIN_SWAP).abs() < 1e-9 {
            self.spin_rate = SPIN_REST;
        }
        self.spin += self.spin_rate * dt;
        self.flip = (self.flip - FLIP_RATE * dt).max(0.0);

        for (_, a) in &mut self.rings {
            *a = (*a - RING_FADE * dt).max(0.0);
        }
        if let Some(picked) = &model.post {
            if let Some((_, a)) = self.rings.iter_mut().find(|(name, _)| name == picked) {
                *a = 1.0;
            } else {
                self.rings.push((picked.clone(), 1.0));
            }
        }
    }

    /// Writes the spawn screen's HUD layer facts (`LAYER`, `SPAWN_MAP`, `VIEW_TEAMS`,
    /// `OWN`, `FOE`, `FLIP`, and `MAP_DRAW` selection rings) into `hud_input`
    /// (`HudModel::input`).
    ///
    /// Instructs `bfe-render`'s HUD pipeline to switch from the in-play corner minimap
    /// to the large spawn-screen map (`SPAWN_MAP = 1`), display reinforcement bars from
    /// the picked team's perspective (`VIEW_TEAMS = 1`, `OWN`, `FOE`, `FLIP`), and draw
    /// pulsing/fading selection rings at each highlighted command post's world `(x, z)`.
    pub fn sync_hud_inputs(
        &self,
        model: &SpawnModel,
        port: &dyn SpawnPort,
        covered: bool,
        hud_input: &mut [f64],
    ) {
        hud_input[H::COVERED] = u8::from(covered) as f64;
        hud_input[H::LAYER] = 1.0;
        hud_input[H::SPAWN_MAP] = 1.0;
        hud_input[H::VIEW_TEAMS] = 1.0;
        hud_input[H::HIDE_TEAMS] = 0.0;
        let own = model.team;
        let foe = if own == 1 { 2 } else { 1 };
        // HUD OWN/FOE are reinforcement counts, not team identifiers.
        // Team ids here made a healthy spawn screen misleadingly show 1 / 2.
        hud_input[H::OWN] = port.team_facts(own).map_or(0.0, |t| t.reinforcements as f64);
        hud_input[H::FOE] = port.team_facts(foe).map_or(0.0, |t| t.reinforcements as f64);
        hud_input[H::FLIP] = self.flip;

        // Emit up to 128 active selection rings into the HUD shared-memory `RINGS` table
        // so the picked command post glows at alpha 1.0 and previously picked posts fade out.
        let posts = port.posts(own);
        let mut ring_count = 0usize;
        for (name, ring_alpha) in &self.rings {
            if *ring_alpha <= 0.0 || ring_count >= 128 {
                continue;
            }
            if let Some(post) = posts.iter().find(|p| &p.name == name) {
                let o = RINGS + ring_count * R::WORDS;
                if o + R::WORDS <= hud_input.len() {
                    hud_input[o + R::X] = post.position[0];
                    hud_input[o + R::Z] = post.position[2];
                    hud_input[o + R::SIZE] = self.map_proj.post_icon_size;
                    hud_input[o + R::ALPHA] = *ring_alpha;
                    ring_count += 1;
                }
            }
        }
        hud_input[MAP_DRAW + M::RINGS] = ring_count as f64;
    }

    /// Computes the 3D placements for the `shell_cube` room, the two spinning
    /// faction logos, and the standing soldier preview (`renderRoom` and
    /// `renderModels` in `src/spawn-screen/classic/view.js`).
    ///
    /// When the pause menu covers the spawn screen (`covered == true`) or team
    /// switching is disabled by the server, both faction logos hide while the
    /// standing soldier preview body remains visible in the center.
    pub fn compute_3d_placement(
        &mut self,
        model: &SpawnModel,
        port: &dyn SpawnPort,
        aspect: f64,
        height: f64,
        fh: f64,
        covered: bool,
    ) -> Spawn3dPlacement {
        let layout = self.layout(height, fh);
        let room_position = place_interface_model(512.0, height * 0.5, aspect, height);
        let t = (self.swap / LOGO_SWAP).clamp(0.0, 1.0);
        let can_switch = model.can_switch_teams(port) && !covered;

        // Interpolate each team logo linearly between its `from` and `to` slot,
        // growing the newly picked team logo to `big` (`1.0` alpha, green) while
        // shrinking the other to `small` (`0.35` alpha, red) over `LOGO_SWAP` (0.25s).
        let make_logo = |team: u8| -> SpawnLogoPlacement {
            let facts = port.team_facts(team).unwrap_or_default();
            let model_name = if !facts.logo.is_empty() {
                facts.logo
            } else {
                fallback_logo_for_side(&facts.side).to_string()
            };
            let own = team == model.team;
            let to = if own {
                layout.logos.own
            } else {
                layout.logos.other
            };
            let from = if own {
                layout.logos.other
            } else {
                layout.logos.own
            };
            let size = if own {
                lerp(layout.logos.small, layout.logos.big, t)
            } else {
                lerp(layout.logos.big, layout.logos.small, t)
            };
            let opacity = if own {
                lerp(LOGO_DIM, 1.0, t)
            } else {
                lerp(1.0, LOGO_DIM, t)
            };
            let lx = lerp(from[0], to[0], t);
            let ly = lerp(from[1], to[1], t);
            SpawnLogoPlacement {
                team,
                model_name,
                visible: can_switch,
                position: place_interface_model(lx, ly, aspect, height),
                scale: size * LOGO_UNITS,
                rot_y: self.spin,
                color: if own { LOGO_GREEN } else { LOGO_RED },
                opacity,
                time: self.time,
            }
        };

        // Place the standing soldier preview at `(W / 2, H / 1.2)`; droideka bodies
        // scale by `0.9 * BODY_UNITS` (`DROIDEKA_UNITS`) so their wider frame fits.
        let preview_class = model.class_name(port);
        let is_droideka = preview_class
            .as_deref()
            .is_some_and(|c| c.to_ascii_lowercase().contains("droideka"));
        let preview_position =
            place_interface_model(layout.preview[0], layout.preview[1], aspect, height);

        Spawn3dPlacement {
            room_position,
            room_scale: ROOM_SCALE,
            time: self.time,
            logos: [make_logo(1), make_logo(2)],
            preview_class,
            preview_position,
            preview_scale: if is_droideka {
                DROIDEKA_UNITS
            } else {
                BODY_UNITS
            },
        }
    }

    /// Builds a window frame at `(x, y)` on the `1024xH` plane rotated by `tilt`
    /// about `axis` (`'x'` or `'y'`; `ClassicView.frame`).
    #[inline]
    pub fn frame(height: f64, x: f64, y: f64, tilt: f64, axis: char) -> Matrix {
        let m = translate(root_anchor(height), x, y, 0.0);
        if tilt == 0.0 {
            m
        } else if axis == 'x' {
            rotate_x(m, tilt)
        } else {
            rotate_y(m, tilt)
        }
    }

    /// Draws the five class windows, objectives window, spawn point window,
    /// and `SPAWN` button into `surface` (`ClassicView.draw`).
    pub fn draw(
        &mut self,
        surface: &mut dyn BattleUiSurface,
        model: &SpawnModel,
        port: &dyn SpawnPort,
    ) {
        // Lazily resolve named texture handles from the surface on the first draw pass.
        if self.assets.border_3_pieces == 0 {
            self.assets = BattleUiAssets::from_surface(surface);
        }
        let h = surface.height();
        let fh = surface.font_height(FONT_TINY);
        let layout = self.layout(h, fh);
        // Draw up to 5 tilted soldier class windows; the picked class draws at
        // full opacity (`1.0`) and unpicked classes draw at half strength (`128/255`).
        let roster = model.roster(port);
        for i in 0..SLOTS {
            let Some(entry) = roster.get(i) else {
                continue;
            };
            let picked = i == model.class_index;
            let strength = if picked { 1.0 } else { UNPICKED_ALPHA };
            let intro = self.slots[i];
            self.draw_class_window(surface, &layout.slots[i], entry, &intro, strength, &layout);
        }
        self.draw_objectives(surface, model, port, &layout);
        self.draw_post_window(surface, model, port, &layout);
        self.draw_spawn(surface, &layout);
    }

    /// Draws the 3-piece orange header bar (`headerbuttonleft`, `headerbuttonmid`,
    /// `headerbuttonright`), sweeping white cursor block, and vertically unfolding
    /// black title label above a tilted window (`ClassicView.drawTitle`).
    ///
    /// Bar offsets (`BAR_OVER = 6.3`, `BAR_UNDER = 0.3`, `BAR_LEFT = 1.75`,
    /// `BAR_RIGHT = 3.9`, `TITLE_GAP = 5.0`, `TITLE_X = 18.0`) match the retail
    /// `ifs_spawndisplay` screenshots where the orange header bar sits wider and
    /// slightly lower than the Lua text box.
    #[allow(clippy::too_many_arguments)]
    fn draw_title(
        &mut self,
        surface: &mut dyn BattleUiSurface,
        m: Matrix,
        w: f64,
        h: f64,
        text_str: &str,
        intro: &TitleIntro,
        strength: f64,
        fh: f64,
    ) {
        let state = intro.frame();
        let top = -h * 0.5 - TITLE_GAP - fh;
        let y0 = -h * 0.5 - fh - BAR_OVER;
        let y1 = -h * 0.5 - BAR_UNDER;
        let x0 = -w * 0.5 + BAR_LEFT;
        let x1 = w * 0.5 - BAR_RIGHT;
        let cap = y1 - y0;
        let visible = x0 + (x1 - x0) * state.bar;
        let bar_tint = alpha(WHITE, strength);
        let assets = self.assets;

        // Stage 1+: draw the left cap, middle span, and right cap clipped to `visible`.
        if state.bar > 0.0 {
            let pieces = [
                (x0, x0 + cap, assets.headerbuttonleft),
                (x0 + cap, x1 - cap, assets.headerbuttonmid),
                (x1 - cap, x1, assets.headerbuttonright),
            ];
            let batch = surface.batch_mut();
            for (a, b, tex) in pieces {
                if visible <= a || b <= a {
                    continue;
                }
                let end = b.min(visible);
                if tex != 0 {
                    batch.quad(
                        m,
                        [a, y0, end, y1],
                        tex,
                        [0.0, 0.0, (end - a) / (b - a), 1.0],
                        bar_tint,
                        0.0,
                        0,
                    );
                } else {
                    batch.quad(
                        m,
                        [a, y0, end, y1],
                        assets.solid,
                        UV,
                        alpha([1.0, 0.55, 0.1, 1.0], strength),
                        0.0,
                        0,
                    );
                }
            }
        }

        // Stages 2 & 3: draw the sweeping white cursor block across the title text span.
        encode_utf16_into(text_str, true, &mut self.scratch_utf16);
        let tx = -w * 0.5 + TITLE_X;
        let (batch, font) = surface.batch_and_font(FONT_TINY);
        let width = font.width(&self.scratch_utf16, 1.0, 1.0);
        if let Some(block) = state.block {
            let bx = tx + block.x * (width - fh).max(0.0);
            let cursor_tex = if assets.cursor != 0 {
                assets.cursor
            } else {
                assets.solid
            };
            batch.quad(
                m,
                [bx, top, bx + fh, top + fh],
                cursor_tex,
                UV,
                alpha(WHITE, strength * block.alpha),
                0.0,
                0,
            );
        }
        // Stages 4 & 5: draw the black title text unfolding vertically from `mid`.
        if state.text > 0.01 && state.alpha > 0.0 {
            let mid = top + fh * 0.5;
            let grown = translate(
                scale_xyz(translate(m, 0.0, mid, 0.0), 1.0, state.text, 1.0),
                0.0,
                -mid,
                0.0,
            );
            font.draw(
                batch,
                grown,
                tx,
                top,
                &self.scratch_utf16,
                alpha(BLACK, strength * state.alpha),
                0,
                1.0,
                1.0,
                0.0,
            );
        }
    }

    /// Draws one of the five 35-degree tilted soldier class windows (`ClassicView.drawClassWindow`).
    ///
    /// Renders the 9-slice `border_3_pieces` frame, the class display name in the
    /// orange header bar, and the uppercase weapon list centered inside `slot.text_h`
    /// at `layout.info_scale` (`0.13 * H / 135`). Because the 2004 game's weapon list
    /// string ends with a trailing newline, `count` adds one virtual line when the box
    /// height permits (`clamp(n, n + 1)`), shifting the visible weapon lines slightly upward.
    fn draw_class_window(
        &mut self,
        surface: &mut dyn BattleUiSurface,
        slot: &ClassSlotLayout,
        entry: &SpawnClassEntry,
        intro: &TitleIntro,
        strength: f64,
        layout: &ClassicSpawnLayout,
    ) {
        let m = Self::frame(layout.h, slot.x, slot.y, slot.tilt, 'y');
        draw_nine_slice_window(
            surface.batch_mut(),
            m,
            0.0,
            0.0,
            slot.w,
            slot.h,
            self.assets.border_3_pieces,
            self.assets.solid,
            WHITE,
        );
        let title = if !entry.display_name.is_empty() {
            &entry.display_name
        } else {
            &entry.class_name
        };
        self.draw_title(surface, m, slot.w, slot.h, title, intro, strength, layout.fh);

        let scale = layout.info_scale;
        let lh = surface.font_height(FONT_TINY) * scale;
        let n = entry.weapons.len();
        let count = if slot.text_h > 0.0 && lh > 0.0 {
            ((slot.text_h / lh).floor() as usize).clamp(n, n + 1)
        } else {
            n
        };
        let color = alpha(WHITE, strength);
        for (i, weapon) in entry.weapons.iter().enumerate() {
            encode_utf16_into(weapon, true, &mut self.scratch_utf16);
            let y = -(count as f64 * lh) * 0.5 + (i as f64) * lh;
            let (batch, font) = surface.batch_and_font(FONT_TINY);
            font.draw(batch, m, 0.0, y, &self.scratch_utf16, color, 2, scale, 1.0, 0.0);
        }
    }

    /// Draws the top-right `-20 deg` tilted `OBJECTIVES:` window (`ClassicView.drawObjectives`).
    ///
    /// Word-wraps each mission objective string to `box_l.w - 16.0` in uppercase
    /// `gamefont_tiny` and left-aligns the lines at `-width * 0.5`, accounting for
    /// the original game's trailing newline via `clamp(n, n + 1)`.
    fn draw_objectives(
        &mut self,
        surface: &mut dyn BattleUiSurface,
        model: &SpawnModel,
        port: &dyn SpawnPort,
        layout: &ClassicSpawnLayout,
    ) {
        let box_l = layout.objectives;
        let m = Self::frame(layout.h, box_l.x, box_l.y, box_l.tilt, 'y');
        draw_nine_slice_window(
            surface.batch_mut(),
            m,
            0.0,
            0.0,
            box_l.w,
            box_l.h,
            self.assets.border_3_pieces,
            self.assets.solid,
            WHITE,
        );
        let intro = self.objectives_title;
        self.draw_title(
            surface,
            m,
            box_l.w,
            box_l.h,
            "OBJECTIVES:",
            &intro,
            1.0,
            layout.fh,
        );

        let objectives = port
            .team_facts(model.team)
            .map(|t| t.objectives)
            .unwrap_or_default();
        // Uppercase all objective paragraphs into `scratch_utf16` and wrap each
        // paragraph into `(start, end)` slice ranges without per-word heap allocations.
        let width = box_l.w - 16.0;
        self.scratch_utf16.clear();
        self.scratch_paragraphs.clear();
        for obj in &objectives {
            let start = self.scratch_utf16.len();
            for u in obj.encode_utf16() {
                let c = if (b'a' as u16..=b'z' as u16).contains(&u) {
                    u - 32
                } else {
                    u
                };
                self.scratch_utf16.push(c);
            }
            let end = self.scratch_utf16.len();
            self.scratch_paragraphs.push((start, end));
        }

        let mut all_lines: Vec<(usize, usize)> = Vec::new();
        {
            let font = surface.font(FONT_TINY);
            for &(p_start, p_end) in &self.scratch_paragraphs {
                wrap_lines_ranges(
                    font,
                    &self.scratch_utf16[p_start..p_end],
                    width,
                    1.0,
                    &mut self.scratch_lines,
                );
                for &(ls, le) in &self.scratch_lines {
                    all_lines.push((p_start + ls, p_start + le));
                }
            }
        }
        let lh = surface.font_height(FONT_TINY);
        let box_h = box_l.h - 16.0;
        let n = all_lines.len();
        let count = if box_h > 0.0 && lh > 0.0 {
            ((box_h / lh).floor() as usize).clamp(n, n + 1)
        } else {
            n
        };
        let (batch, font) = surface.batch_and_font(FONT_TINY);
        for (i, &(ls, le)) in all_lines.iter().enumerate() {
            let y = -(count as f64 * lh) * 0.5 + (i as f64) * lh;
            font.draw(
                batch,
                m,
                -width * 0.5,
                y,
                &self.scratch_utf16[ls..le],
                WHITE,
                0,
                1.0,
                1.0,
                0.0,
            );
        }
    }

    /// Draws the bottom-right `-20 deg` tilted command-post window (`ClassicView.drawPostWindow`).
    ///
    /// Displays the selected command post's localized title in the orange header bar
    /// and centers the comma-separated list of vehicles spawned by that post (or
    /// `"NONE"` when the post spawns no vehicles) inside the 9-slice panel.
    fn draw_post_window(
        &mut self,
        surface: &mut dyn BattleUiSurface,
        model: &SpawnModel,
        port: &dyn SpawnPort,
        layout: &ClassicSpawnLayout,
    ) {
        let box_l = layout.post;
        let m = Self::frame(layout.h, box_l.x, box_l.y, box_l.tilt, 'y');
        draw_nine_slice_window(
            surface.batch_mut(),
            m,
            0.0,
            0.0,
            box_l.w,
            box_l.h,
            self.assets.border_3_pieces,
            self.assets.solid,
            WHITE,
        );
        let posts = port.posts(model.team);
        let post = model
            .post
            .as_deref()
            .and_then(|name| posts.iter().find(|p| p.name == name));
        if let Some(p) = post {
            let title = if !p.display_name.is_empty() {
                &p.display_name
            } else {
                &p.name
            };
            if !title.is_empty() {
                let intro = self.post_title;
                self.draw_title(surface, m, box_l.w, box_l.h, title, &intro, 1.0, layout.fh);
            }
        }
        let vehicles_str = match post {
            Some(p) if !p.vehicles.is_empty() => p.vehicles.join(", "),
            _ => "NONE".to_string(),
        };
        encode_utf16_into(&vehicles_str, true, &mut self.scratch_utf16);
        let width = box_l.w - 16.0;
        let (batch, font) = surface.batch_and_font(FONT_TINY);
        wrap_lines_ranges(font, &self.scratch_utf16, width, 1.0, &mut self.scratch_lines);
        let lh = font.height;
        let count = self.scratch_lines.len();
        for (i, &(ls, le)) in self.scratch_lines.iter().enumerate() {
            let y = -(count as f64 * lh) * 0.5 + (i as f64) * lh;
            font.draw(
                batch,
                m,
                0.0,
                y,
                &self.scratch_utf16[ls..le],
                WHITE,
                2,
                1.0,
                1.0,
                0.0,
            );
        }
    }

    /// Draws the tipped `SPAWN` button background and flat `"SPAWN"` label
    /// (`ClassicView.drawSpawn`).
    /// Note that while the `opaque_rect` button background tips backward by `45 deg`
    /// around the X axis (`tipped`), the `"SPAWN"` label itself is drawn on an
    /// unrotated frame (`flat`) at the same anchor point, matching the 2004 game.
    pub fn draw_spawn(&mut self, surface: &mut dyn BattleUiSurface, layout: &ClassicSpawnLayout) {
        let box_l: TiltedBoxLayout = layout.spawn;
        let tipped = Self::frame(layout.h, box_l.x, box_l.y, box_l.tilt, 'x');
        draw_nine_slice_window(
            surface.batch_mut(),
            tipped,
            0.0,
            0.0,
            box_l.w,
            box_l.h,
            self.assets.opaque_rect,
            self.assets.solid,
            WHITE,
        );
        let flat = Self::frame(layout.h, box_l.x, box_l.y, 0.0, 'y');
        encode_utf16_into("SPAWN", true, &mut self.scratch_utf16);
        let (batch, font) = surface.batch_and_font(FONT_TINY);
        let lh = font.height;
        font.draw(
            batch,
            flat,
            0.0,
            -lh * 0.5,
            &self.scratch_utf16,
            WHITE,
            2,
            1.0,
            1.0,
            0.0,
        );
    }

    /// Draws the mouse pointer over the 3D models (`ClassicView.drawPointer`).
    ///
    /// Uses the `32x32` `mousecursor` texture when loaded, or an `8x8` solid white
    /// stand-in quad at `(cursor.x, cursor.y)` in headless/fallback environments.
    pub fn draw_pointer(&self, batch: &mut Batch, height: f64, cursor: [f64; 2]) {
        if self.hide_pointer {
            return;
        }
        let m = root_anchor(height);
        let [cx, cy] = cursor;
        if self.assets.mousecursor != 0 {
            batch.quad(
                m,
                [cx, cy, cx + POINTER_SIZE, cy + POINTER_SIZE],
                self.assets.mousecursor,
                UV,
                WHITE,
                0.0,
                0,
            );
        } else {
            batch.quad(
                m,
                [cx, cy, cx + 8.0, cy + 8.0],
                self.assets.solid,
                UV,
                WHITE,
                0.0,
                0,
            );
        }
    }

    /// Hit-tests an interface point `(x, y)` against the `SPAWN` button, the
    /// team logo hotspot, the five class windows, and the team's eligible
    /// command posts on the map (`ClassicView.hit`).
    ///
    /// Both the 45-degree X-tipped `SPAWN` button and the five 35-degree Y-tilted
    /// class windows project their local 3D bounds onto the `1024xH` interface plane
    /// via [`projected_corners`] and test membership with [`inside_quad`] so clicks
    /// on perspective-foreshortened edges match the rendered geometry.
    pub fn hit(
        &mut self,
        x: f64,
        y: f64,
        height: f64,
        fh: f64,
        model: &SpawnModel,
        port: &dyn SpawnPort,
    ) -> Option<SpawnHitTarget> {
        let layout = self.layout(height, fh);
        let spawn = layout.spawn;
        // 1. Test the perspective-projected 45-degree X-tilted SPAWN button quad.
        let spawn_m = Self::frame(height, spawn.x, spawn.y, spawn.tilt, 'x');
        let spawn_quad = projected_corners(
            spawn_m,
            -spawn.w * 0.5,
            -spawn.h * 0.5,
            spawn.w * 0.5,
            spawn.h * 0.5,
            height,
        );
        if inside_quad(x, y, &spawn_quad) {
            return Some(SpawnHitTarget::Spawn);
        }

        // 2. Test the top-center team logo switch hotspot (when team switching is allowed).
        let logos = layout.logos.hit;
        if model.can_switch_teams(port)
            && (x - logos.x).abs() <= logos.half_x
            && (y - logos.y).abs() <= logos.half_y
        {
            return Some(SpawnHitTarget::Team);
        }

        // 3. Test each populated 35-degree Y-tilted soldier class window (including its title bar).
        let roster = model.roster(port);
        for i in 0..SLOTS {
            if i >= roster.len() {
                continue;
            }
            let slot = layout.slots[i];
            let [x0, y0, x1, y1] = slot.hit;
            let slot_m = Self::frame(height, slot.x, slot.y, slot.tilt, 'y');
            let quad = projected_corners(slot_m, x0, y0, x1, y1, height);
            if inside_quad(x, y, &quad) {
                return Some(SpawnHitTarget::Class(i));
            }
        }

        // 4. Test eligible command post icons owned by the picked team on the tactical spawn map.
        let place = spawn_map_place(height);
        let half = self.map_proj.post_icon_size * 0.5;
        for post in port.posts(model.team) {
            if post.owner != model.team || !post.eligible {
                continue;
            }
            let [px, py] = self
                .map_proj
                .map_point(post.position[0], post.position[2], place.size);
            if (x - (place.x + px)).abs() <= half && (y - (place.y + py)).abs() <= half {
                return Some(SpawnHitTarget::Post(post.name));
            }
        }
        None
    }
}
