//! A held map picture, a localized tip and clock-driven lights. Bitmap
//! text goes into the BF1 batch. The other looks draw their text roles
//! in the horizontal MapBackdropText overlay at the returned positions.
//! The PC layout uses a centred four-by-three frame.

mod tips;
pub use tips::{tip_pages, wrap_bitmap_tip, TipPanel};

use super::{draw_str, ShellFonts, ShellTextures, BLACK, WHITE};
use crate::loading::{mix_color_rgb, LookId, TimelineAt};
use crate::loading2::{blink, BackdropFrame, LedBar, LoadingScreenKind};
use bfe_render::interface::{anchor, Batch, UV};

/// Handles owned by a single map-backdrop presentation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BackdropTextures {
    pub solid: u32,
    pub picture: u32,
    pub tips_box: u32,
    pub panel_size: [u32; 2],
    pub led: u32,
}

impl BackdropTextures {
    pub fn load(
        solid: u32,
        selected: Option<&str>,
        fallback: Option<&str>,
        panel: Option<&str>,
        led: Option<&str>,
        mut upload: impl FnMut(&str) -> Option<(u32, u32, u32)>,
    ) -> Self {
        let picture = selected
            .and_then(&mut upload)
            .or_else(|| fallback.and_then(&mut upload))
            .unwrap_or_default()
            .0;
        let (tips_box, width, height) = panel.and_then(&mut upload).unwrap_or_default();
        Self {
            solid,
            picture,
            tips_box,
            panel_size: [width, height],
            led: led.and_then(upload).unwrap_or_default().0,
        }
    }
}

/// Decode the prepared UTF-8 line boundaries once when selecting a tip.
pub fn bitmap_tip_lines(text: &str, ends: &[usize]) -> Option<Vec<Vec<u16>>> {
    if text.encode_utf16().count() > crate::loading2::prepared::MAX_TIP_UNITS
        || !crate::loading2::prepared::valid_line_ends(text, ends)
    {
        return None;
    }
    let mut lines = Vec::with_capacity(ends.len());
    let mut start = 0;
    for &end in ends {
        lines.push(text.get(start..end)?.trim().encode_utf16().collect());
        start = end;
    }
    Some(lines)
}

/// Everything [`draw_loading2`] needs beyond the 2004 screen's inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropDraw<'a> {
    pub textures: BackdropTextures,
    /// The map backdrop's state at this frame.
    pub state: BackdropFrame,
    /// The tip to show (`state.tip` resolved by the caller), if any.
    pub tip: &'a [Vec<u16>],
    pub border: [f64; 2],
    /// Painted width of the active vector loading word, in frame units.
    pub word_width: f64,
    /// Measured vector text height in frame units.
    pub tip_height: f64,
    pub wide_tip: bool,
    pub page_controls: bool,
    /// Largest measured tip in any prepared language.
    pub max_tip_lines: usize,
    /// The LED bar's shape (the config's, via [`LedBar::default`]).
    pub leds: LedBar,
    /// Loading text blink rate in Hz (`LoadingTextBlinkRate`).
    pub blink_rate: f64,
}

impl<'a> Default for BackdropDraw<'a> {
    /// The retail bar at the screen's start, with no tip.
    fn default() -> Self {
        Self {
            textures: BackdropTextures {
                solid: 1,
                ..Default::default()
            },
            state: BackdropFrame::default(),
            tip: &[],
            max_tip_lines: 0,
            border: [0.; 2],
            word_width: 0.,
            tip_height: 0.,
            wide_tip: false,
            page_controls: false,
            leds: LedBar::default(),
            blink_rate: 1.0,
        }
    }
}

/// Draws whichever loading screen `kind` names. The 2004 branch is
/// `screens::draw_loading` itself, so a 2004 battle draws exactly what
/// it drew before this seam existed.
#[allow(clippy::too_many_arguments)]
pub fn draw_loading_screen(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    kind: LoadingScreenKind,
    zoom_state: &TimelineAt,
    show: Option<&BackdropDraw<'_>>,
    time: f64,
    planet: &str,
    loading_text: &str,
    look: LookId,
    viewport_w: f64,
    viewport_h: f64,
) -> Option<BackdropTextPlacement> {
    match kind {
        LoadingScreenKind::PlanetZoom => {
            super::screens::draw_loading(
                batch,
                fonts,
                textures,
                zoom_state,
                time,
                planet,
                loading_text,
                look,
                viewport_w,
                viewport_h,
            );
            None
        }
        LoadingScreenKind::MapBackdrop => {
            let fallback = BackdropDraw::default();
            Some(draw_loading2(
                batch,
                fonts,
                show.unwrap_or(&fallback),
                planet,
                loading_text,
                look,
                viewport_w,
                viewport_h,
            ))
        }
    }
}

/// Text rectangles shared with the theme overlay, in viewport fractions.
#[derive(Clone, Copy, Debug)]
pub struct BackdropTextPlacement {
    pub unit_width: f32,
    pub title: [f32; 2],
    pub tip: [f32; 4],
    pub word: [f32; 2],
    pub word_opacity: f32,
}

// Reference layout at 4:3. Fit this frame inside the physical viewport so
// the title and footer stay aligned with the picture on a wide display.
const FRAME: [f64; 2] = [800., 600.];
const LEFT: f64 = 40.;
const TITLE_TOP: f64 = 32.;
const FOOTER_BOTTOM: f64 = 570.;
const WORD_GAP: f64 = 10.;
const WORD_MAX_WIDTH: f64 = FRAME[0] / 3.;
const PANEL_ORIGIN: [f64; 2] = [800. / 3., 96.];
const TIP_WIDTH: f64 = 400.;
const INNER_SHADOW: f64 = 5.;
fn tip_inset(border: f64) -> f64 {
    (border + INNER_SHADOW + 1.).max(18.)
}

fn panel_axis(lo: f64, hi: f64, texels: u32, border: f64) -> ([f64; 4], [f64; 4]) {
    let size = f64::from(texels.max(1));
    // Keep the inner shadow with the bevel, leaving a stretchable centre texel.
    let slice = (border + INNER_SHADOW).min((size - 1.) / 2.);
    let dest = slice.min((hi - lo) / 2.);
    (
        [lo, lo + dest, hi - dest, hi],
        [0., slice / size, 1. - slice / size, 1.],
    )
}

fn draw_panel(
    batch: &mut Batch,
    m: bfe_render::interface::Matrix,
    rect: [f64; 4],
    show: &BackdropDraw<'_>,
) {
    let textures = show.textures;
    if textures.tips_box == 0 {
        batch.quad(m, rect, textures.solid, UV, [0., 0., 0., 0.65], 0., 0);
        return;
    }
    let (x, u) = panel_axis(rect[0], rect[2], textures.panel_size[0], show.border[0]);
    let (y, v) = panel_axis(rect[1], rect[3], textures.panel_size[1], show.border[1]);
    for row in 0..3 {
        for col in 0..3 {
            if x[col] < x[col + 1] && y[row] < y[row + 1] {
                batch.quad(
                    m,
                    [x[col], y[row], x[col + 1], y[row + 1]],
                    textures.tips_box,
                    [u[col], v[row], u[col + 1], v[row + 1]],
                    WHITE,
                    0.,
                    0,
                );
            }
        }
    }
}
const MIN_PANEL_HEIGHT: f64 = 128.;

/// Populates `batch` with the map backdrop screen: black clear, the current
/// selected backdrop (or the common fallback) and the top-left title,
/// the tips box with its tip, and the LED bar lit to `state.progress`.
#[allow(clippy::too_many_arguments)]
pub fn draw_loading2(
    batch: &mut Batch,
    fonts: &ShellFonts,
    show: &BackdropDraw<'_>,
    planet: &str,
    loading_text: &str,
    look: LookId,
    viewport_w: f64,
    viewport_h: f64,
) -> BackdropTextPlacement {
    batch.begin();
    let w = viewport_w.max(1.0);
    let h = viewport_h.max(1.0);
    let unit = (w / FRAME[0]).min(h / FRAME[1]);
    let (ux, uy) = (unit / w, unit / h);
    let (ox, oy) = ((1. - FRAME[0] * ux) * 0.5, (1. - FRAME[1] * uy) * 0.5);
    let mut m = anchor(ox, oy, false, 768.);
    m[0] = 1024. * ux;
    m[5] = -768. * uy;
    let state = &show.state;
    let textures = show.textures;

    batch.quad(
        anchor(0., 0., false, 768.),
        [0., 0., 1024., 768.],
        textures.solid,
        UV,
        BLACK,
        0.0,
        0,
    );
    if textures.picture != 0 {
        batch.quad(
            m,
            [0., 0., FRAME[0], FRAME[1]],
            textures.picture,
            UV,
            WHITE,
            0.0,
            0,
        );
    }

    let bar = &show.leds;
    let mut label = [0; 160];
    let label = super::encode_utf16_stack(loading_text, &mut label);
    let mut left = 0_f64;
    let mut right = 0_f64;
    let mut pen = 0.;
    if look == LookId::Bf1 {
        for glyph in label.iter().filter_map(|ch| fonts.medium.glyphs.get(ch)) {
            if glyph.bounds[0] < glyph.bounds[2] {
                left = left.min(pen + glyph.bounds[0].round());
                right = right.max(pen + glyph.bounds[2].round());
            }
            pen += glyph.advance.round();
        }
    }
    let bitmap_scale = (WORD_MAX_WIDTH / (right - left).max(1.)).min(1.);
    let label_width = if look == LookId::Bf1 {
        (right - left) * bitmap_scale
    } else {
        show.word_width.max(0.)
    };
    let word_bottom = FOOTER_BOTTOM;
    let bar_left = LEFT + label_width + WORD_GAP;
    let edge = look.light_edge().filter(|_| textures.led != 0);
    let rim = edge.map_or(0., |_| (bar.spacing * 0.5).min(1.5));
    let bar_scale = ((FRAME[0] - LEFT - bar_left) / (bar.total_width() + rim * 2.).max(1.)).min(1.);
    let rim = rim * bar_scale;
    let led_height = bar.height * bar_scale;
    let y0 = FOOTER_BOTTOM - led_height;
    let time = (f64::from(state.tick) + state.tick_fraction)
        / f64::from(crate::loading2::TICKS_PER_SECOND);
    let alpha = 0.55 + 0.45 * blink(time, show.blink_rate);
    let panel = TipPanel::new([w, h], show.border, bar.height, show.wide_tip);
    let scale = panel.bitmap_scale(&fonts.small);
    let line_height = fonts.small.height + 2.;
    let needed = if look == LookId::Bf1 {
        line_height * scale * show.tip.len() as f64
    } else {
        show.tip_height
    };
    let controls = if show.page_controls {
        panel.controls_height()
    } else {
        0.
    };
    let tip_height = needed
        .max(line_height * show.max_tip_lines as f64)
        .max(MIN_PANEL_HEIGHT - 2. * panel.inset[1])
        .min(panel.max_text_height - controls);
    let [bx, by] = panel.origin;
    let tip_x = bx + panel.inset[0];
    let tip_y = by + panel.inset[1];
    draw_panel(
        batch,
        m,
        [
            bx,
            by,
            bx + panel.width,
            tip_y + tip_height + controls + panel.inset[1],
        ],
        show,
    );
    if look == LookId::Bf1 {
        draw_str(&fonts.tiny, batch, m, LEFT, TITLE_TOP, planet, WHITE, 0);
        let scaled = bfe_render::interface::scale(m, scale);
        for (i, line) in show.tip.iter().enumerate() {
            fonts.small.draw(
                batch,
                scaled,
                tip_x / scale,
                tip_y / scale + i as f64 * line_height,
                line,
                WHITE,
                0,
                1.,
                0.,
                0.,
            );
        }
        let [r, g, b, _] = look.text_color();
        let tint = [
            f64::from(r) / 255.,
            f64::from(g) / 255.,
            f64::from(b) / 255.,
            alpha,
        ];
        fonts.medium.draw(
            batch,
            bfe_render::interface::scale(m, bitmap_scale),
            LEFT / bitmap_scale - left,
            word_bottom / bitmap_scale - fonts.medium.height,
            label,
            tint,
            0,
            1.,
            0.,
            0.,
        );
    }

    // The LED bar: themed light stops, the 2005 look's dark rim, the
    // LED art or a solid quad, lit by fill with the rest resting dim.
    let stops = look.light_stops();
    let step = (bar.width + bar.spacing) * bar_scale;
    let led_width = bar.width * bar_scale;
    for i in 0..bar.count.min(crate::loading2::MAX_LEDS) {
        let x0 = bar_left + rim + i as f64 * step;
        let mut inset = 0.;
        if let Some(edge) = edge {
            inset = (led_width.min(led_height) * 0.08).min(bar_scale);
            batch.quad(
                m,
                [
                    x0 - rim,
                    y0 - rim,
                    x0 + led_width + rim,
                    y0 + led_height + rim,
                ],
                textures.solid,
                UV,
                [
                    edge[0] as f64 / 255.0,
                    edge[1] as f64 / 255.0,
                    edge[2] as f64 / 255.0,
                    1.0,
                ],
                0.0,
                textures.led,
            );
        }
        let [r, g, b] = mix_color_rgb(stops, bar.level(i, state.progress));
        batch.quad(
            m,
            [
                x0 + inset,
                y0 + inset,
                x0 + led_width - inset,
                y0 + led_height - inset,
            ],
            textures.solid,
            UV,
            [r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, 1.0],
            0.0,
            textures.led,
        );
    }
    BackdropTextPlacement {
        unit_width: ux as f32,
        title: [(ox + LEFT * ux) as f32, (oy + TITLE_TOP * uy) as f32],
        tip: [
            (ox + tip_x * ux) as f32,
            (oy + tip_y * uy) as f32,
            (panel.text_width() * ux) as f32,
            (tip_height * uy) as f32,
        ],
        word: [(ox + LEFT * ux) as f32, (oy + word_bottom * uy) as f32],
        word_opacity: alpha as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{ShellFonts, ShellTextures, StageTexture};

    pub(super) fn fonts_with_glyphs() -> ShellFonts {
        let mut fonts = ShellFonts::default();
        for ch in 32u16..=126u16 {
            let glyph = bfe_render::interface::Glyph {
                page: 2,
                advance: 10.0,
                bounds: [0.0, 0.0, 9.0, 18.0],
                uv: [0.0, 0.0, 0.1, 0.1],
            };
            fonts.large.glyphs.insert(ch, glyph);
            fonts.medium.glyphs.insert(ch, glyph);
            fonts.small.glyphs.insert(ch, glyph);
            fonts.tiny.glyphs.insert(ch, glyph);
        }
        fonts
    }

    fn textures() -> BackdropTextures {
        BackdropTextures {
            solid: 1,
            picture: 3,
            tips_box: 7,
            panel_size: [60, 48],
            led: 8,
        }
    }

    /// The developer preview of the map backdrop as data: the quad list at
    /// a fixed time, checked without a window. Covers the picture, its
    /// tips box, and one quad per LED (plus a rim each
    /// only on the 2005 look).
    #[test]
    fn backdrop_draw_list_has_picture_tip_box_and_leds() {
        let fonts = ShellFonts::default(); // no glyphs: text draws nothing
        let textures = textures();
        let show = BackdropDraw {
            textures,
            state: BackdropFrame {
                picture: Some(2),
                ..Default::default()
            },
            tip: &[],
            ..BackdropDraw::default()
        };
        for (look, rims) in [(LookId::Bf1, 0), (LookId::Eternal, 0), (LookId::Bf2, 12)] {
            let mut batch = Batch::new();
            draw_loading2(
                &mut batch, &fonts, &show, "GARDEN", "LOADING", look, 1280.0, 720.0,
            );
            // clear + picture + tips box + LEDs (+ rims)
            assert_eq!(batch.count, 1 + 1 + 9 + 12 + rims, "{look:?}");
            // Draw records count indices (6 per quad) and cover them all.
            assert_eq!(
                batch.draws.iter().map(|d| d.count).sum::<usize>(),
                batch.count * 6
            );
        }
    }

    /// The two vector looks use their Slint text roles. All looks use
    /// the LED's alpha mask over the theme's unmodified light colour.
    #[test]
    fn only_the_bitmap_look_emits_text_quads_and_led_art_is_alpha_only() {
        let fonts = fonts_with_glyphs();
        let textures = textures();
        let lines = vec!["A sample tip".encode_utf16().collect()];
        let with_tip = BackdropDraw {
            textures,
            state: BackdropFrame {
                picture: Some(1),
                ..Default::default()
            },
            tip: &lines,
            ..BackdropDraw::default()
        };
        let without_tip = BackdropDraw {
            tip: &[],
            ..with_tip
        };
        let mut counts = Vec::new();
        for look in [LookId::Bf1, LookId::Eternal, LookId::Bf2] {
            let (mut tipped, mut plain) = (Batch::new(), Batch::new());
            draw_loading2(
                &mut tipped,
                &fonts,
                &with_tip,
                "GARDEN",
                "LOADING",
                look,
                1280.0,
                720.0,
            );
            draw_loading2(
                &mut plain,
                &fonts,
                &without_tip,
                "GARDEN",
                "LOADING",
                look,
                1280.0,
                720.0,
            );
            assert_eq!(tipped.count > plain.count, look == LookId::Bf1);
            let leds: usize = tipped
                .draws
                .iter()
                .filter(|d| d.mask == textures.led && d.texture == textures.solid)
                .map(|d| d.count / 6)
                .sum();
            assert_eq!(leds, if look == LookId::Bf2 { 24 } else { 12 });
            counts.push(tipped.count);
        }
        assert!(counts[0] > counts[1]);
        assert_eq!(counts[2], counts[1] + 12, "{counts:?}");
    }

    #[test]
    fn readable_tip_grows_with_the_viewport_and_uses_compact_space() {
        let fonts = fonts_with_glyphs();
        let mut batch = Batch::new();
        let note = vec![vec![b'X' as u16; 14]; 7];
        let show = BackdropDraw {
            textures: textures(),
            tip: &note,
            ..Default::default()
        };
        let mut heights = Vec::new();
        for (w, h) in [
            (320., 240.),
            (640., 360.),
            (1024., 768.),
            (1280., 800.),
            (2560., 1080.),
        ] {
            let p = draw_loading2(&mut batch, &fonts, &show, "", "", LookId::Bf1, w, h);
            let glyph = batch.draws.iter().find(|d| d.texture == 2).unwrap().first * 2;
            let ink = (batch.position[glyph + 1] - batch.position[glyph + 4]) * h as f32 / 768.;
            assert!(ink >= 10.9, "{w}x{h} ink {ink}");
            heights.push(ink);
            if w == 320. {
                assert!(p.tip[2] > 0.9);
            }
            assert!(p.tip[1] > p.title[1] && p.tip[1] + p.tip[3] < p.word[1] - 0.04);
        }
        assert!(heights[4] > heights[2]);
    }

    #[test]
    fn footer_uses_the_active_font_at_the_frame_margin() {
        let mut batch = Batch::new();
        let show = BackdropDraw {
            textures: textures(),
            word_width: 91.75,
            ..Default::default()
        };
        let mut fonts = fonts_with_glyphs();
        let label = "W".repeat(87);
        for look in [LookId::Bf1, LookId::Eternal, LookId::Bf2] {
            let p = draw_loading2(&mut batch, &fonts, &show, "", &label, look, 800., 600.);
            assert!((p.word[0] * 800. - 40.).abs() < 1e-4);
            let lamps = batch
                .draws
                .iter()
                .find(|d| d.mask == show.textures.led)
                .unwrap();
            let left = (batch.position[lamps.first * 2] + 512.) / 1.28;
            if look == LookId::Bf1 {
                let glyphs = batch.draws.iter().find(|d| d.texture == 2).unwrap();
                let right = (batch.position[(glyphs.first + glyphs.count) * 2 - 6] + 512.) / 1.28;
                assert!((left - right - 10.).abs() < 0.02);
            } else {
                assert!((left - 40. - 91.75 - 10.).abs() < 1e-4);
            }
            fonts.medium.glyphs.clear();
        }
        let p = draw_loading2(
            &mut batch,
            &fonts,
            &show,
            "",
            &label,
            LookId::Bf1,
            800.,
            600.,
        );
        assert!((p.word[0] * 800. - 40.).abs() < 1e-4);
    }

    #[test]
    fn constrained_footer_scales_both_light_axes() {
        let show = BackdropDraw {
            textures: textures(),
            leds: LedBar {
                count: 15,
                width: 44.,
                height: 33.,
                spacing: 2.,
                min_level: 0.25,
            },
            ..Default::default()
        };
        let mut batch = Batch::new();
        draw_loading2(
            &mut batch,
            &fonts_with_glyphs(),
            &show,
            "",
            "A wide loading label to fit",
            LookId::Bf1,
            1024.,
            768.,
        );
        let i = batch.draws.last().unwrap().first / 6 * 12;
        let w = batch.position[i + 6] - batch.position[i];
        let h = batch.position[i + 1] - batch.position[i + 4];
        assert!((w / h - 44. / 33.).abs() < 1e-4);
    }

    #[test]
    fn lamp_rims_follow_the_mask_and_stay_within_the_gap() {
        let mut batch = Batch::new();
        let show = BackdropDraw {
            textures: textures(),
            leds: LedBar {
                count: 3,
                width: 8.,
                height: 11.,
                spacing: 0.375,
                min_level: 0.25,
            },
            ..Default::default()
        };
        draw_loading2(
            &mut batch,
            &ShellFonts::default(),
            &show,
            "",
            "",
            LookId::Bf2,
            800.,
            600.,
        );
        let lamps = batch.draws.last().unwrap();
        assert_eq!((lamps.mask, lamps.count), (show.textures.led, 6 * 6));
        let i = lamps.first / 6 * 12;
        assert!(batch.position[i + 6] <= batch.position[i + 24] + 1e-5);
        assert!(batch.position[i] <= batch.position[i + 12]);
        assert!(batch.tint[lamps.first / 6 * 16] < batch.tint[(lamps.first / 6 + 1) * 16]);
    }

    #[test]
    fn map_backdrop_never_draws_an_online_retargets_planet_texture() {
        let mut planet = ShellTextures::default();
        planet.loading_stages.push(StageTexture {
            texture: 55,
            ..Default::default()
        });
        let textures =
            BackdropTextures::load(1, Some("absent"), Some("common"), None, None, |name| {
                (name == "common").then_some((77, 21, 15))
            });
        let show = BackdropDraw {
            textures,
            state: BackdropFrame {
                picture: Some(0),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut batch = Batch::new();
        draw_loading_screen(
            &mut batch,
            &ShellFonts::default(),
            &planet,
            LoadingScreenKind::MapBackdrop,
            &crate::loading::LoadingTimeline::default().at(1.0),
            Some(&show),
            1.,
            "WORLD",
            "LOAD",
            LookId::Bf1,
            1280.,
            720.,
        );
        assert!(batch.draws.iter().all(|draw| draw.texture != 55));
        assert!(batch.draws.iter().any(|draw| draw.texture == 77));
    }

    #[test]
    fn map_picture_has_a_four_by_three_frame_and_horizontal_footer() {
        let fonts = fonts_with_glyphs();
        let mut batch = Batch::new();
        let show = BackdropDraw {
            textures: textures(),
            ..Default::default()
        };
        let p = draw_loading2(
            &mut batch,
            &fonts,
            &show,
            "WORLD",
            "LOAD",
            LookId::Bf1,
            1280.,
            720.,
        );
        let picture = batch
            .draws
            .iter()
            .find(|d| d.texture == show.textures.picture)
            .unwrap();
        let i = picture.first / 6 * 12;
        let width = (batch.position[i + 6] - batch.position[i]) * 1280. / 1024.;
        assert_eq!(width, 960.);
        assert!(p.tip[0] > 0.35 && p.tip[1] < 0.25);
        let led = batch
            .draws
            .iter()
            .find(|d| d.mask == show.textures.led)
            .unwrap();
        let i = led.first / 6 * 12;
        let left = (batch.position[i] + 512.) / 1024.;
        assert!(p.word[0] < left && left < 0.35);
        let pitch = (batch.position[i + 12] - batch.position[i]) * 1280. / 1024.;
        assert!((24.0..25.0).contains(&pitch), "{pitch}");
    }

    #[test]
    fn panel_uses_nine_uv_pieces_and_keeps_all_glyphs_inside_the_middle() {
        let fonts = fonts_with_glyphs();
        let lines = vec![vec![b'X' as u16; 30]; 7];
        let mut batch = Batch::new();
        let show = BackdropDraw {
            textures: textures(),
            tip: &lines,
            border: [4., 7.],
            ..Default::default()
        };
        draw_loading2(&mut batch, &fonts, &show, "", "", LookId::Bf1, 1024., 768.);
        let panel = batch
            .draws
            .iter()
            .find(|d| d.texture == show.textures.tips_box)
            .unwrap();
        assert_eq!(panel.count, 9 * 6);
        let first = panel.first / 6;
        let middle = (first + 4) * 12;
        let pos = &batch.position;
        let glyphs = batch.draws.iter().find(|d| d.texture == 2).unwrap();
        assert_eq!(glyphs.count, 30 * 7 * 6);
        for v in pos[glyphs.first * 2..(glyphs.first + glyphs.count) * 2]
            .as_chunks::<3>()
            .0
        {
            assert!(v[0] > pos[middle] && v[0] < pos[middle + 6]);
            assert!(v[1] < pos[middle + 1] && v[1] > pos[middle + 4]);
        }
        let uv = &batch.uv[first * 8..first * 8 + 8];
        assert!((uv[4] - 9. / 60.).abs() < 1e-6 && (uv[3] - 12. / 48.).abs() < 1e-6);
        assert!(((pos[first * 12 + 6] - pos[first * 12]) / 1.28 - 9.).abs() < 1e-4);
    }

    #[test]
    fn complete_unequal_width_tip_fits_the_reported_panel() {
        let mut fonts = fonts_with_glyphs();
        fonts.small.height = 20.;
        for (ch, width) in [('W', 30.), ('i', 3.)] {
            let glyph = fonts.small.glyphs.get_mut(&(ch as u16)).unwrap();
            glyph.advance = width;
            glyph.bounds[2] = width;
        }
        let row = format!("{}{}", "W".repeat(11), "i".repeat(6));
        let contents = [row.as_str(); 8].join(" ");
        let ends: Vec<_> = contents
            .match_indices(' ')
            .map(|(i, _)| i + 1)
            .chain(std::iter::once(contents.len()))
            .collect();
        let lines = bitmap_tip_lines(&contents, &ends).unwrap();
        let show = BackdropDraw {
            textures: textures(),
            tip: &lines,
            max_tip_lines: lines.len(),
            ..Default::default()
        };
        let mut batch = Batch::new();
        let placement = draw_loading2(&mut batch, &fonts, &show, "", "", LookId::Bf1, 1280., 720.);
        let glyphs = batch.draws.iter().find(|d| d.texture == 2).unwrap();
        assert_eq!(glyphs.count / 6, row.len() * 8);
        let [x, y, w, h] = placement.tip;
        let start = glyphs.first / 6 * 12;
        for v in batch.position[start..start + glyphs.count / 6 * 12]
            .as_chunks::<3>()
            .0
        {
            let (px, py) = ((v[0] + 512.) / 1024., (384. - v[1]) / 768.);
            assert!(px >= x - 1e-6 && px <= x + w + 1e-6 && py >= y - 1e-6 && py <= y + h + 1e-6);
        }
    }

    #[test]
    fn bitmap_lines_consume_the_whole_tip_on_character_boundaries() {
        let text = "wide é narrow";
        let lines = bitmap_tip_lines(text, &[5, text.len()]).unwrap();
        assert_eq!(String::from_utf16(&lines[1]).unwrap(), "é narrow");
        assert!(bitmap_tip_lines(text, &[5]).is_none());
        assert!(bitmap_tip_lines(text, &[6, text.len()]).is_none());
        assert!(bitmap_tip_lines(text, &[5, 5, text.len()]).is_none());
    }
}
