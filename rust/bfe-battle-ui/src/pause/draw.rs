// Classic in-battle pause menu drawing (`ClassicTheme.draw` + `overlay.draw` in `src/menus/themes/classic/`).
//
// Ports the 2004 PC Classic menu painter (`src/menus/themes/classic/index.js` and
// `src/menus/themes/classic/draw.js`) along with the `"CLICK TO RESUME"` overlay prompt
// (`drawPrompt` in `src/pause/index.js`).
//
// Rendering architecture and 2004 game calibration:
// - The main pause menu stack (`draw_main_rows`) is anchored at safe-area coordinates `(0.6, 0.5)`,
//   shifted left by `rows_shift(260.0, pitch, 11)` (counting all 11 original Lua rows so the
//   visible stack sits at the exact horizontal position of the retail game), and rotated 25 degrees
//   around the Y axis (`make_rotation_yxz(0.0, 25deg, 0.0)`).
// - Each row bar is built from three textures (`buttonleft`, `buttonmid`, `buttonright` for rows;
//   `headerbuttonleft`, `headerbuttonmid`, `headerbuttonright` for the orange `"GAME PAUSED"` header):
//   a square left cap of width `h`, a stretched middle section, and a square right diagonal cap
//   whose V coordinates flip top-to-bottom (`flipped = i > n / 2`) below the vertical midpoint.
// - When a `Confirm` popup is pushed over `Main`, `UNDER_POPUP` in `src/menus/themes/classic/index.js`
//   hides the main menu's `rows` widget while keeping the bottom-left `BACK` button visible
//   (though non-interactive underneath the modal popup).
// - Every interactive rectangle projects its four corners through the active 4x4 `Matrix`
//   into 2D screen-plane coordinates (`to_plane_point`) so pointer hit-testing (`inside_quad`)
//   matches the 25-degree perspective-tilted geometry.

use bfe_render::interface::{anchor, mul, scale, translate, Batch, Matrix};

use super::menu::{
    row_bars, rows_shift, PauseHitTarget, PauseHotspot, PauseMenu, PauseScreenState, LAYOUT,
    LIST_COUNT, PROMPT_DELAY,
};
use crate::shapes::{
    draw_nine_slice, make_rotation_yxz, rgba, scaled, selected_color, tint, to_plane_point,
    wrap_lines, BattleUiSurface, TitleIntro, BLACK, DIMMED, FONT_NAME_LARGE, FONT_NAME_MEDIUM,
    FONT_NAME_SMALL, FONT_NAME_TINY, GREY, W, WHITE,
};

/// Side length in interface units of the Classic `mousecursor` sprite (`POINTER = 32` in `src/menus/themes/classic/index.js`).
const POINTER_SIZE: f64 = 32.0;

/// Horizontal offset where a menu row's 3-piece bar starts relative to the row origin (`BAR_X = -4.7` in `src/menus/themes/classic/index.js`).
const BAR_X: f64 = -4.7;

/// Horizontal offset where a menu row's label text starts relative to the row origin (`TEXT_X = 0.8` in `src/menus/themes/classic/index.js`).
const TEXT_X: f64 = 0.8;

/// Inset from the safe-area left edge to the `BACK` button text (`BUTTON_INSET = 21` in `src/menus/themes/classic/index.js`).
const BUTTON_INSET: f64 = 21.0;

/// Three-piece texture names for the orange `"GAME PAUSED"` title bar (`HEADER` in `src/menus/themes/classic/index.js`).
const HEADER_PIECES: [&str; 3] = ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"];

/// Three-piece texture names for the steel-blue menu row bars (`BUTTON` in `src/menus/themes/classic/index.js`).
const BUTTON_PIECES: [&str; 3] = ["buttonleft", "buttonmid", "buttonright"];

impl PauseMenu {
    /// Draws the Classic pause menu, sub-screens, confirm popups, or `"CLICK TO RESUME"` into `batch`,
    /// recording interactive hotspots for hit testing (`ClassicTheme.draw` + `overlay.draw`).
    pub fn draw(&mut self, surface: &dyn BattleUiSurface, batch: &mut Batch) {
        self.hotspots.clear();
        let h = surface.height();
        // When the menu is closed, it still owns the HUD overlay slot while `waiting_time` is `Some`
        // (waiting for a user click to re-acquire browser pointer lock after `RESUME`). Once `wait_t`
        // reaches `PROMPT_DELAY` (`0.15s`), draw the pulsing centered `"CLICK TO RESUME"` prompt.
        if !self.is_open {
            if let Some(wait_t) = self.waiting_time {
                if wait_t >= PROMPT_DELAY {
                    let m = anchor(0.5, 0.5, false, h);
                    let fh = surface.line_height(FONT_NAME_MEDIUM);
                    let alpha = 0.5 + 0.5 * self.pulse();
                    surface.draw_text(
                        batch,
                        m,
                        0.0,
                        -fh / 2.0,
                        "CLICK TO RESUME",
                        FONT_NAME_MEDIUM,
                        [1.0, 1.0, 1.0, alpha],
                        2,
                        1.0,
                    );
                }
            }
            return;
        }

        // Determine which screen currently sits on top of `self.stack`.
        // In the 2004 game (`UNDER_POPUP` in `src/menus/themes/classic/index.js`), pushing a
        // `Confirm` popup over `Main` hides the angled `rows` widget underneath while leaving
        // the bottom-left `BACK` button visible (with `interactive = false` so clicks only
        // hit the modal `YES` / `NO` buttons).
        let top_is_main = matches!(self.stack.last(), Some(PauseScreenState::Main));
        let top_is_confirm = matches!(self.stack.last(), Some(PauseScreenState::Confirm { .. }));

        // When main screen is on top, draw its angled rows + BACK button.
        // When a Confirm popup is over Main, `UNDER_POPUP` hides `rows` but keeps `BACK` visible!
        if top_is_main {
            self.draw_main_rows(surface, batch, true);
            self.draw_back_button(surface, batch, true);
        } else if top_is_confirm {
            self.draw_back_button(surface, batch, false);
            if let Some(PauseScreenState::Confirm {
                message,
                focused_button,
                ..
            }) = self.stack.last().cloned()
            {
                self.draw_confirm_popup(surface, batch, &message, focused_button);
            }
        // When `Options` or `PlayerList` sits on top of `self.stack`, draw that full-screen
        // sub-page (which dims the backdrop by half-opacity black) plus an interactive `BACK`
        // button in the bottom-left corner of the safe area.
        } else if let Some(PauseScreenState::Options { focused_row }) = self.stack.last().cloned() {
            self.draw_options_screen(surface, batch, focused_row);
            self.draw_back_button(surface, batch, true);
        } else if let Some(PauseScreenState::PlayerList { selected, first }) =
            self.stack.last().cloned()
        {
            self.draw_player_list_screen(surface, batch, selected, first);
            self.draw_back_button(surface, batch, true);
        }

        // Draw the Classic software cursor (`mousecursor`, 32x32) on top of all menu widgets,
        // or a 8x8 solid white square fallback if the cursor texture has not been loaded.
        if self.pointer_visible && self.show_pointer {
            let m = anchor(0.0, 0.0, false, h);
            let tex = surface.texture("mousecursor");
            if tex != 0 {
                batch.quad(
                    m,
                    [
                        self.pointer[0],
                        self.pointer[1],
                        self.pointer[0] + POINTER_SIZE,
                        self.pointer[1] + POINTER_SIZE,
                    ],
                    tex,
                    [0.0, 0.0, 1.0, 1.0],
                    WHITE,
                    0.0,
                    0,
                );
            } else {
                batch.quad(
                    m,
                    [
                        self.pointer[0],
                        self.pointer[1],
                        self.pointer[0] + 8.0,
                        self.pointer[1] + 8.0,
                    ],
                    surface.solid_texture(),
                    [0.0, 0.0, 1.0, 1.0],
                    WHITE,
                    0.0,
                    0,
                );
            }
        }
    }

    /// Paints the 25-degree Y-tilted `"GAME PAUSED"` header bar and visible row stack
    /// (`paintRows` in `src/menus/themes/classic/index.js`).
    ///
    /// - The container matrix `m` anchors at `LAYOUT.at` (`[0.6, 0.5]` in the safe area),
    ///   translates by `(rows_shift(260, p, 11), 20, 0)`, and multiplies by the 25-degree Y rotation.
    /// - The orange title bar sits `1.75 * p` above `y0` (`-(n * p) / 2`), spanning from
    ///   `ty - 0.25 * p` to `ty + glyph_height + 0.25 * p` (deeper than `line_height` to match
    ///   the retail game's glyph bounding box measurement) with black `"GAME PAUSED"` text.
    /// - Each visible row `k` (`0..n`) draws a 3-piece `BUTTON_PIECES` bar of width `bars[k].width + 5.0`
    ///   (pulsing at full brightness when focused, or dimmed by `DIMMED = 150/255` when unfocused)
    ///   and records a 4-corner projected hotspot quad when `interactive` is `true`.
    fn draw_main_rows(
        &mut self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        interactive: bool,
    ) {
        // Compute the vertical row pitch `p = line_height("gamefont_medium") + 5.0` and the
        // horizontal container shift from `LIST_COUNT` (`11` total rows in `ifs_pausemenu`),
        // then compose the safe-area anchor `(0.6, 0.5)`, translation `(shift, 20.0, 0.0)`,
        // and 25-degree Y-axis rotation matrix `rot`.
        let h = surface.height();
        let yh = surface.line_height(LAYOUT.font);
        let p = yh + LAYOUT.y_spacing;
        let visible = self.visible_rows(surface);
        let n = visible.len();
        let shift = rows_shift(LAYOUT.width, p, LIST_COUNT);
        let rot = make_rotation_yxz(0.0, LAYOUT.rot_y_deg.to_radians(), 0.0);
        let m = mul(
            translate(anchor(LAYOUT.at[0], LAYOUT.at[1], true, h), shift, LAYOUT.y, 0.0),
            rot,
        );
        let y0 = -(n as f64 * p) / 2.0;

        // Title bar ("GAME PAUSED")
        let title_text = surface.string("game.pause.title", "GAME PAUSED");
        let ty = y0 - 1.75 * p;
        let title_bar_w = LAYOUT.width - 1.25 * (yh - LAYOUT.y_spacing) - 0.65 * p;
        let by = ty - 0.25 * p;
        let gh = surface.glyph_height_by_name(LAYOUT.font);
        let title_frame = self.title_intro.frame();
        Self::draw_bar(
            surface,
            batch,
            m,
            HEADER_PIECES,
            BAR_X,
            by,
            title_bar_w,
            ty + gh + 0.25 * p,
            false,
            title_frame.bar,
            WHITE,
        );
        // Center the black `"GAME PAUSED"` text vertically within the orange header bar
        // by subtracting `caps_middle` from the bar's vertical midpoint, and indent it
        // horizontally by `p / 2.0 + TEXT_X`.
        let title_y = (by + ty + gh + 0.25 * p) / 2.0 - surface.caps_middle(LAYOUT.font);
        Self::draw_flashy_menu_text(
            surface,
            batch,
            m,
            p / 2.0 + TEXT_X,
            title_y,
            title_text,
            LAYOUT.font,
            BLACK,
            0,
            Some(&self.title_intro),
            1.0,
        );

        // Compute per-row bar widths and right-cap vertical flip flags across the `n` visible rows.
        let bars = row_bars(LAYOUT.width, p, n);
        let pulse = self.pulse();
        // Draw each visible menu row `k` (`0..n`):
        // - `cy` is the vertical center of row `k`, starting at `y0 = -(n * p) / 2.0`.
        // - When `focused` is true (the row is selected and `BACK` is not hovered), the row's
        //   bar and label pulse with `self.pulse()`; unfocused bars are dimmed by `DIMMED` (`150/255`).
        // - The bar spans `[cy - p / 2.0, cy + p / 2.0]` vertically and `bars[k].width + 5.0`
        //   horizontally from `BAR_X` (`-4.7`), with its right diagonal end cap flipped when `k >= n / 2`.
        for (k, (_, label)) in visible.iter().enumerate() {
            let cy = y0 + k as f64 * p;
            let focused = interactive && !self.back_focused && self.focused_row == k;
            let a = if focused { pulse } else { 1.0 };
            let intro = &self.row_intros[k.min(LIST_COUNT - 1)];
            let bar_color = if focused {
                tint(WHITE, a)
            } else {
                tint(scaled(WHITE, DIMMED), a)
            };
            Self::draw_bar(
                surface,
                batch,
                m,
                BUTTON_PIECES,
                BAR_X,
                cy - p / 2.0,
                bars[k].width + 5.0,
                cy + p / 2.0,
                bars[k].flipped,
                intro.frame().bar,
                bar_color,
            );
            Self::draw_flashy_menu_text(
                surface,
                batch,
                m,
                TEXT_X,
                cy - surface.caps_middle(LAYOUT.font),
                label,
                LAYOUT.font,
                WHITE,
                0,
                Some(intro),
                a,
            );
            // Project the unrotated row box `[0, cy - yh/2, width, cy + yh/2]` through the
            // 25-degree tilted matrix `m` into screen-plane coordinates for `inside_quad` hit testing.
            if interactive {
                let quad_corners =
                    Self::rect_corners(m, 0.0, cy - yh / 2.0, LAYOUT.width, cy + yh / 2.0, h);
                self.hotspots.push(PauseHotspot {
                    target: PauseHitTarget::MainRow(k),
                    corners: quad_corners,
                });
            }
        }
    }

    /// Paints the bottom-left `"BACK"` button (`paintButton` in `src/menus/themes/classic/index.js`,
    /// `CreateNegativeButton` in the 2004 Lua scripts).
    ///
    /// Anchored at `(0.0, 1.0)` of the safe area, inset `BUTTON_INSET` (`21.0` units) from the left
    /// edge, with its capital-letter midpoint sitting `12.0` units above the safe-area bottom.
    /// When focused, its color pulses yellow via `selected_color(self.pulse())`; when `interactive`
    /// is `true`, records a hotspot padded by `4.0` units horizontally and `12.5` units vertically
    /// around `y = -15.0`.
    fn draw_back_button(
        &mut self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        interactive: bool,
    ) {
        let h = surface.height();
        let m = anchor(0.0, 1.0, true, h);
        let text = surface.string("common.back", "BACK");
        let width = surface.text_width(FONT_NAME_MEDIUM, text, 1.0);
        // `BUTTON_INSET` (`21.0` units) places the left-aligned `"BACK"` text inside the safe-area
        // left edge, while `-12.0 - caps_middle` aligns the vertical midpoint of capital glyphs
        // 12 units above the safe-area bottom (`paintButton` in `src/menus/themes/classic/index.js`).
        // The clickable hotspot spans `[x - 4.0, x + width + 4.0]` horizontally and `25.0` units
        // vertically around `y = -15.0` (`[-27.5, -2.5]`).
        let x = BUTTON_INSET;
        let y = -12.0 - surface.caps_middle(FONT_NAME_MEDIUM);
        let focused = interactive && self.back_focused;
        let color = if focused {
            selected_color(self.pulse())
        } else {
            WHITE
        };
        Self::draw_flashy_menu_text(
            surface,
            batch,
            m,
            x,
            y,
            text,
            FONT_NAME_MEDIUM,
            color,
            0,
            Some(&self.back_intro),
            1.0,
        );
        if interactive {
            let quad_corners =
                Self::rect_corners(m, x - 4.0, -15.0 - 12.5, x + width + 4.0, -15.0 + 12.5, h);
            self.hotspots.push(PauseHotspot {
                target: PauseHitTarget::BackButton,
                corners: quad_corners,
            });
        }
    }

    /// Paints the centered `300x180` `YES`/`NO` confirmation popup (`confirmPopup` in `src/menus/popup.js`,
    /// `paintPanel` + `paintText` + `paintHorizontal` in `src/menus/themes/classic/index.js`).
    ///
    /// - Draws a nine-sliced `border_3_pieces` frame (`300.0 x 180.0`) at the screen center `(0.5, 0.5)`.
    /// - Word-wraps `message` to `260.0` interface units and renders centered lines starting at `y = -74.0`.
    /// - Places `"YES"` (`i = 0`, `x = -40.0`) and `"NO"` (`i = 1`, `x = +45.0`) `85.0` units apart
    ///   along `y = 63.0`, pulsing the focused choice yellow (`selected_color`) and recording `80x45`
    ///   button hotspots (`[x - 40, y - 22.5, x + 40, y + 22.5]`).
    fn draw_confirm_popup(
        &mut self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        message: &str,
        focused_button: usize,
    ) {
        let h = surface.height();
        let m = anchor(0.5, 0.5, false, h);
        Self::draw_frame(surface, batch, m, 0.0, 0.0, 300.0, 180.0, "border_3_pieces", WHITE);

        // Word-wrap the confirmation prompt within a 260-unit box (`w.width || 260` in
        // `paintText` for `look == "popup"`) and draw each line centered (`align = 2`)
        // starting 74 units above the popup center (`y = -74.0 + i * fh`).
        let lines = wrap_lines(message, 260.0, |s| {
            surface.text_width(FONT_NAME_MEDIUM, s, 1.0)
        });
        let fh = surface.line_height(FONT_NAME_MEDIUM);
        for (i, line) in lines.iter().enumerate() {
            surface.draw_text(
                batch,
                m,
                0.0,
                -74.0 + i as f64 * fh,
                line,
                FONT_NAME_MEDIUM,
                WHITE,
                2,
                1.0,
            );
        }

        // Lay out the horizontal `"YES"` (`i = 0`, `x = -40.0`) and `"NO"` (`i = 1`, `x = +45.0`)
        // buttons 85 units apart along `y = 63.0` (`paintHorizontal` in `src/menus/themes/classic/index.js`).
        // The focused choice pulses yellow via `selected_color(self.pulse())`.
        let yes_label = surface.string("common.yes", "YES");
        let no_label = surface.string("common.no", "NO");
        let y = 63.0;
        let caps = surface.caps_middle(FONT_NAME_MEDIUM);
        for (i, (label, intro)) in [
            (yes_label, &self.popup_yes_intro),
            (no_label, &self.popup_no_intro),
        ]
        .into_iter()
        .enumerate()
        {
            let x = -40.0 + i as f64 * 85.0;
            let focused = focused_button == i;
            let color = if focused {
                selected_color(self.pulse())
            } else {
                WHITE
            };
            Self::draw_flashy_menu_text(
                surface,
                batch,
                m,
                x,
                y - caps,
                label,
                FONT_NAME_MEDIUM,
                color,
                2,
                Some(intro),
                1.0,
            );
            let quad_corners = Self::rect_corners(m, x - 40.0, y - 22.5, x + 40.0, y + 22.5, h);
            self.hotspots.push(PauseHotspot {
                target: PauseHitTarget::ConfirmButton(i),
                corners: quad_corners,
            });
        }
    }

    /// Paints the in-battle `"OPTIONS"` sub-screen (`optionsMenu` in `src/menus/option-pages.js`).
    ///
    /// Unlike the main pause screen (which leaves the frozen world undimmed), option pages dim
    /// the backdrop with half-opacity black (`rgba(0, 0, 0, 128)`), render a centered `"OPTIONS"`
    /// heading in `gamefont_large` near the top of the safe area, and lay out the 4 category bars
    /// (`GAME OPTIONS`, `VIDEO OPTIONS`, `SOUND OPTIONS`, `CONTROL OPTIONS`) using `rows_shift`
    /// and `row_bars`.
    fn draw_options_screen(
        &mut self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        focused_row: usize,
    ) {
        let h = surface.height();
        // Option pages carry `dim: true` in `src/menus/option-pages.js`: draw a full-screen
        // `1024xH` quad of `DIM = rgba(0, 0, 0, 128)` so the frozen 3D battle world behind the
        // menu is darkened by 50%, then draw `"OPTIONS"` in `gamefont_large` at `(0.5, 0.0)`
        // of the safe area (`y = 10.0`).
        let dim_m = anchor(0.0, 0.0, false, h);
        batch.quad(
            dim_m,
            [0.0, 0.0, W, h],
            surface.solid_texture(),
            [0.0, 0.0, 1.0, 1.0],
            rgba(0, 0, 0, 128),
            0.0,
            0,
        );
        let title_m = anchor(0.5, 0.0, true, h);
        let title = surface.string("ifs.main.options", "OPTIONS");
        surface.draw_text(batch, title_m, 0.0, 10.0, title, FONT_NAME_LARGE, WHITE, 2, 1.0);

        // Render the 4 option category buttons (`GAME OPTIONS`, `VIDEO OPTIONS`, `SOUND OPTIONS`,
        // `CONTROL OPTIONS`) centered in the safe area using the same `rows_shift` and `row_bars`
        // chevron geometry as the main menu, but without a 3D Y-axis rotation.
        let yh = surface.line_height(FONT_NAME_MEDIUM);
        let p = yh + 5.0;
        let labels = ["GAME OPTIONS", "VIDEO OPTIONS", "SOUND OPTIONS", "CONTROL OPTIONS"];
        let shift = rows_shift(260.0, p, labels.len());
        let m = translate(anchor(0.5, 0.5, true, h), shift, 0.0, 0.0);
        let y0 = -(labels.len() as f64 * p) / 2.0;
        let bars = row_bars(260.0, p, labels.len());
        for (k, &label) in labels.iter().enumerate() {
            let cy = y0 + k as f64 * p;
            let focused = !self.back_focused && focused_row == k;
            let a = if focused { self.pulse() } else { 1.0 };
            let bar_color = if focused {
                tint(WHITE, a)
            } else {
                tint(scaled(WHITE, DIMMED), a)
            };
            Self::draw_bar(
                surface,
                batch,
                m,
                BUTTON_PIECES,
                BAR_X,
                cy - p / 2.0,
                bars[k].width + 5.0,
                cy + p / 2.0,
                bars[k].flipped,
                1.0,
                bar_color,
            );
            surface.draw_text(
                batch,
                m,
                TEXT_X,
                cy - surface.caps_middle(FONT_NAME_MEDIUM),
                label,
                FONT_NAME_MEDIUM,
                tint(WHITE, a),
                0,
                1.0,
            );
            let quad_corners = Self::rect_corners(m, 0.0, cy - yh / 2.0, 260.0, cy + yh / 2.0, h);
            self.hotspots.push(PauseHotspot {
                target: PauseHitTarget::OptionRow(k),
                corners: quad_corners,
            });
        }
    }

    /// Paints the network `"PLAYER LIST"` sub-screen (`ifs.pause.playerlist`, `paintList` in
    /// `src/menus/themes/classic/index.js`).
    ///
    /// Dims the backdrop with half-opacity black, draws a centered `560x340` nine-sliced
    /// `border_3_pieces` list box, renders the column headers (`NAME`, `TEAM`, `PING`, `LINK`)
    /// in `gamefont_small`, and draws up to 11 visible player rows (`first..first + 11`) in
    /// `gamefont_tiny` with a nine-sliced `listbox_cursor` highlight around the `selected` row.
    fn draw_player_list_screen(
        &mut self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        selected: usize,
        first: usize,
    ) {
        let h = surface.height();
        let dim_m = anchor(0.0, 0.0, false, h);
        batch.quad(
            dim_m,
            [0.0, 0.0, W, h],
            surface.solid_texture(),
            [0.0, 0.0, 1.0, 1.0],
            rgba(0, 0, 0, 128),
            0.0,
            0,
        );
        // Draw the `"PLAYER LIST"` heading centered at the top of the safe area (`y = 10.0`)
        // in `gamefont_large` over the 50%-dimmed backdrop, followed by the centered `560x340`
        // nine-sliced `border_3_pieces` list container (`paintList` in `src/menus/themes/classic/index.js`).
        let title_m = anchor(0.5, 0.0, true, h);
        let title = surface.string("ifs.pause.playerlist", "PLAYER LIST");
        surface.draw_text(batch, title_m, 0.0, 10.0, title, FONT_NAME_LARGE, WHITE, 2, 1.0);

        let m = anchor(0.5, 0.5, true, h);
        let box_w = 560.0;
        let box_h = 340.0;
        Self::draw_frame(surface, batch, m, 0.0, 0.0, box_w, box_h, "border_3_pieces", WHITE);

        // Inside the `560x340` player list box, inset 20 units horizontally (`inner = 520.0`)
        // and 24 units from the top (`top = -146.0`), placing column headers at `0.0` (`NAME`),
        // `0.55` (`TEAM`), `0.78` (`PING`), and `1.0` right-aligned (`LINK`).
        let left = -box_w / 2.0 + 20.0;
        let inner = box_w - 40.0;
        let top = -box_h / 2.0 + 24.0;
        let pitch = 24.0;
        surface.draw_text(batch, m, left, top, "NAME", FONT_NAME_SMALL, WHITE, 0, 1.0);
        surface.draw_text(
            batch,
            m,
            left + inner * 0.55,
            top,
            "TEAM",
            FONT_NAME_SMALL,
            WHITE,
            0,
            1.0,
        );
        surface.draw_text(
            batch,
            m,
            left + inner * 0.78,
            top,
            "PING",
            FONT_NAME_SMALL,
            WHITE,
            0,
            1.0,
        );
        surface.draw_text(
            batch,
            m,
            left + inner,
            top,
            "LINK",
            FONT_NAME_SMALL,
            WHITE,
            1,
            1.0,
        );

        // Render up to 11 visible rows (`first..first + 11`) spaced `pitch = 24.0` units apart:
        // draw the orange `listbox_cursor` frame behind the `selected` row, then draw `name`,
        // `team` (`"1"` or `"2"`), optional `ping`, and grey right-aligned `link` in `gamefont_tiny`.
        let shown = 11usize;
        for k in 0..shown {
            let idx = first + k;
            let Some(p) = self.players.get(idx) else {
                break;
            };
            let cy = top + 32.0 + k as f64 * pitch;
            if idx == selected {
                Self::draw_frame(
                    surface,
                    batch,
                    m,
                    0.0,
                    cy,
                    inner + 8.0,
                    pitch + 6.0,
                    "listbox_cursor",
                    WHITE,
                );
            }
            let ty = cy - surface.caps_middle(FONT_NAME_TINY);
            surface.draw_text(batch, m, left + 4.0, ty, &p.name, FONT_NAME_TINY, WHITE, 0, 1.0);
            let team_label = if p.team == 1 { "1" } else { "2" };
            surface.draw_text(
                batch,
                m,
                left + inner * 0.55,
                ty,
                team_label,
                FONT_NAME_TINY,
                WHITE,
                0,
                1.0,
            );
            if let Some(ping) = p.ping {
                let ps = ping.to_string();
                surface.draw_text(
                    batch,
                    m,
                    left + inner * 0.78,
                    ty,
                    &ps,
                    FONT_NAME_TINY,
                    WHITE,
                    0,
                    1.0,
                );
            }
            surface.draw_text(batch, m, left + inner, ty, &p.link, FONT_NAME_TINY, GREY, 1, 1.0);
            let quad_corners = Self::rect_corners(
                m,
                left,
                cy - pitch / 2.0,
                left + inner,
                cy + pitch / 2.0,
                h,
            );
            self.hotspots.push(PauseHotspot {
                target: PauseHitTarget::PlayerRow(idx),
                corners: quad_corners,
            });
        }
    }

    /// Draws a 3-piece horizontal Classic button or header bar (`bar` in `src/menus/themes/classic/draw.js`).
    ///
    /// A bar of height `h = y1 - y0` consists of:
    /// 1. Left cap (`names[0]`): square `[x0, x0 + h]`.
    /// 2. Middle body (`names[1]`): stretched `[x0 + h, x0 + width.max(h)]`.
    /// 3. Right diagonal cap (`names[2]`): square `[mid_end, mid_end + h]`, with V coordinates
    ///    flipped (`[0, 1, u1, 0]`) when `flipped` is `true` (`i > n / 2`).
    ///
    /// During the `TitleIntro` bar-grow phase (`grow` in `0.0..=1.0`), only the prefix up to
    /// `visible = x0 + (end - x0) * grow` is emitted, clipping `u1` proportionally on the active piece.
    #[allow(clippy::too_many_arguments)]
    fn draw_bar(
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        names: [&str; 3],
        x0: f64,
        y0: f64,
        width: f64,
        y1: f64,
        flipped: bool,
        grow: f64,
        color: [f64; 4],
    ) {
        // Compute the horizontal extents of the three bar pieces:
        // - `[x0, x0 + h]`: square left cap (`buttonleft` or `headerbuttonleft`).
        // - `[x0 + h, mid_end]`: stretched middle body (`buttonmid` or `headerbuttonmid`).
        // - `[mid_end, end]`: square right diagonal cap (`buttonright` or `headerbuttonright`).
        // Clip each piece against `visible = x0 + (end - x0) * grow` so the bar sweeps left-to-right
        // during its `TitleIntro` opening animation.
        let h = y1 - y0;
        let mid_end = x0 + width.max(h);
        let end = mid_end + h;
        let visible = x0 + (end - x0) * grow;
        let pieces = [
            (x0, x0 + h, names[0], false),
            (x0 + h, mid_end, names[1], false),
            (mid_end, end, names[2], true),
        ];
        // Emit each bar piece (`left`, `mid`, `right`) that intersects `[x0, visible]`:
        // - If `visible` stops partway through `[a, b]`, clamp the right edge to `to = b.min(visible)`
        //   and scale the horizontal texture coordinate `u1 = (to - a) / (b - a)` so the texture
        //   reveals smoothly rather than squishing.
        // - For the right-end diagonal piece (`is_right == true`) on rows past the midpoint
        //   (`flipped == true`), invert the vertical UV range (`[0.0, 1.0, u1, 0.0]`) so the
        //   diagonal bevel slopes upward toward the center row.
        for (a, b, name, is_right) in pieces {
            if visible <= a || b <= a {
                continue;
            }
            let to = b.min(visible);
            let u1 = (to - a) / (b - a);
            let uv = if is_right && flipped {
                [0.0, 1.0, u1, 0.0]
            } else {
                [0.0, 0.0, u1, 1.0]
            };
            Self::draw_piece_quad(surface, batch, m, name, a, y0, to, y1, uv, color);
        }
    }

    /// Emits one quad of a 3-piece bar (`quad` and `triangle` in `src/menus/themes/classic/draw.js`).
    ///
    /// When the named texture is loaded in `surface`, draws a single textured quad. Otherwise
    /// falls back to the calibrated steel-blue (`button*`) or orange (`headerbutton*`) stand-in
    /// colors from `FALLBACK` in `src/menus/themes/classic/draw.js`, approximating the diagonal
    /// right-end cap (`buttonright` / `headerbuttonright`) with 6 horizontal stair-step slivers.
    #[allow(clippy::too_many_arguments)]
    fn draw_piece_quad(
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        name: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        uv: [f64; 4],
        color: [f64; 4],
    ) {
        let tex = surface.texture(name);
        if tex != 0 {
            batch.quad(m, [x0, y0, x1, y1], tex, uv, color, 0.0, 0);
            return;
        }
        // When the retail texture is not loaded, look up the calibrated flat fallback color
        // (`FALLBACK` in `src/menus/themes/classic/draw.js`) and multiply by `color`.
        // For the diagonal right end cap (`buttonright` / `headerbuttonright`), approximate the
        // lower-left (or upper-left when `flipped`) triangle using 6 horizontal quad slivers.
        let fb = match name {
            "buttonleft" => rgba(57, 125, 180, 255),
            "buttonmid" => rgba(40, 95, 150, 255),
            "buttonright" => rgba(24, 72, 131, 255),
            "headerbuttonleft" => rgba(255, 165, 24, 255),
            "headerbuttonmid" => rgba(255, 133, 24, 255),
            "headerbuttonright" => rgba(255, 101, 24, 255),
            "border_3_pieces" => rgba(0, 44, 98, 180),
            "listbox_cursor" => rgba(255, 165, 24, 255),
            "listbox_hilight" => rgba(57, 125, 180, 255),
            _ => WHITE,
        };
        let c = [
            fb[0] * color[0],
            fb[1] * color[1],
            fb[2] * color[2],
            fb[3] * color[3],
        ];
        // Stand-in triangle approximation (`triangle` in `src/menus/themes/classic/draw.js`):
        // slices `[y0, y1]` into 6 horizontal strips whose width `k` ramps linearly from
        // `1/12` to `11/12` (or `11/12` down to `1/12` when vertically `flipped`), matching the
        // diagonal bevel of `buttonright` and `headerbuttonright` when art assets are absent.
        if name == "buttonright" || name == "headerbuttonright" {
            let steps = 6;
            let flipped = uv[1] > uv[3];
            for i in 0..steps {
                let a = y0 + ((y1 - y0) * i as f64) / steps as f64;
                let b = y0 + ((y1 - y0) * (i + 1) as f64) / steps as f64;
                let k = if flipped {
                    1.0 - (i as f64 + 0.5) / steps as f64
                } else {
                    (i as f64 + 0.5) / steps as f64
                };
                batch.quad(
                    m,
                    [x0, a, x0 + (x1 - x0) * k, b],
                    surface.solid_texture(),
                    [0.0, 0.0, 1.0, 1.0],
                    c,
                    0.0,
                    0,
                );
            }
        } else {
            batch.quad(
                m,
                [x0, y0, x1, y1],
                surface.solid_texture(),
                [0.0, 0.0, 1.0, 1.0],
                c,
                0.0,
                0,
            );
        }
    }

    /// Draws a nine-sliced window or selection frame centered on `(x, y)` (`frame` in `src/menus/themes/classic/draw.js`).
    ///
    /// Uses 16-unit corners when `name` (`"border_3_pieces"` or `"listbox_cursor"`) is available,
    /// or falls back to a translucent navy quad inset by 4 units when textures are not yet loaded.
    #[allow(clippy::too_many_arguments)]
    fn draw_frame(
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        name: &str,
        color: [f64; 4],
    ) {
        // When `border_3_pieces` or `listbox_cursor` is loaded, delegate to `draw_nine_slice`
        // with 16-unit corner tiles. When running headless or before textures finish loading,
        // emit a single solid quad inset by 4 units on each side in `border_3_pieces` navy blue
        // (`rgba(0, 44, 98, 180)` modulated by `color[3]`).
        let tex = surface.texture(name);
        if tex == 0 {
            batch.quad(
                m,
                [
                    x - w / 2.0 + 4.0,
                    y - h / 2.0 + 4.0,
                    x + w / 2.0 - 4.0,
                    y + h / 2.0 - 4.0,
                ],
                surface.solid_texture(),
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 44.0 / 255.0, 98.0 / 255.0, (180.0 / 255.0) * color[3]],
                0.0,
                0,
            );
            return;
        }
        draw_nine_slice(batch, m, x, y, w, h, 16.0, tex, color);
    }

    /// Draws menu text playing a 2004 `FlashyText` intro animation (`flashyText` in `src/menus/themes/classic/draw.js`).
    ///
    /// The intro (`TitleIntro` in `src/interface/flashy.js`) progresses through two visual stages:
    /// 1. A white square block (`cursor` texture, side length `fh`) sweeps left-to-right across
    ///    the text span (`left + block.x * (width - fh)`).
    /// 2. The text itself scales vertically out of its horizontal midline `mid = y + fh / 2.0`
    ///    (`scale_m[5] = state.text`) while fading in by `state.alpha * alpha`.
    #[allow(clippy::too_many_arguments)]
    fn draw_flashy_menu_text(
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &str,
        font: &str,
        color: [f64; 4],
        align: u32,
        intro: Option<&TitleIntro>,
        alpha: f64,
    ) {
        if text.is_empty() {
            return;
        }
        // Measure the full unscaled line height `fh` and text width `width`, then sample the
        // current `TitleIntro` keyframe (`block`, `text` vertical scale, and `alpha`) and compute
        // the left edge of the text box from `align` (`0` = left, `1` = right, `2` = center).
        let fh = surface.line_height(font);
        let width = surface.text_width(font, text, 1.0);
        let state = intro.map(|i| i.frame()).unwrap_or_default();
        let left = match align {
            1 => x - width,
            2 => x - width / 2.0,
            _ => x,
        };
        // Phase 1 of `TitleIntro`: draw the sweeping white `cursor` block across the text span.
        // Phase 2 of `TitleIntro`: once `state.text > 0.01`, scale the text vertically from its
        // midline `mid = y + fh / 2.0` (`scale_m[5] = state.text`) and tint by `state.alpha * alpha`.
        if let Some(block) = state.block {
            let bx = left + block.x * (width - fh).max(0.0);
            let cursor_tex = {
                let id = surface.texture("cursor");
                if id != 0 {
                    id
                } else {
                    surface.solid_texture()
                }
            };
            batch.quad(
                m,
                [bx, y, bx + fh, y + fh],
                cursor_tex,
                [0.0, 0.0, 1.0, 1.0],
                tint(WHITE, block.alpha * alpha),
                0.0,
                0,
            );
        }
        if state.text <= 0.01 || state.alpha <= 0.0 {
            return;
        }
        let mid = y + fh / 2.0;
        let mut scale_m = scale(
            [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            1.0,
        );
        scale_m[5] = state.text;
        let grown = translate(mul(translate(m, 0.0, mid, 0.0), scale_m), 0.0, -mid, 0.0);
        surface.draw_text(
            batch,
            grown,
            x,
            y,
            text,
            font,
            tint(color, state.alpha * alpha),
            align,
            1.0,
        );
    }

    /// Projects the four corners of an axis-aligned local rectangle `[x0, y0, x1, y1]` through `m`
    /// onto the `1024xH` interface plane (`corners` in `src/menus/themes/classic/draw.js`).
    ///
    /// Ordered clockwise (`top-left`, `top-right`, `bottom-right`, `bottom-left`) for [`inside_quad`](crate::shapes::inside_quad).
    fn rect_corners(
        m: Matrix,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        height: f64,
    ) -> [[f64; 2]; 4] {
        [
            to_plane_point(&m, x0, y0, height),
            to_plane_point(&m, x1, y0, height),
            to_plane_point(&m, x1, y1, height),
            to_plane_point(&m, x0, y1, height),
        ]
    }
}
