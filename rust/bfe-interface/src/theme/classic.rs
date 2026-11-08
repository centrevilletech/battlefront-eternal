//! Widget and screen painting implementation for [`ClassicTheme`] (`src/menus/themes/classic/index.js`
//! and `rust/bfe-shell/src/render.rs`).
//!
//! Every painter method emits textured quads directly into a [`Batch`] in the `1024 x H`
//! 2004 interface space and optionally records [`HitRegion`]s for 3D perspective hit-testing:
//! - [`ClassicTheme::paint_rows_slice`]: angled menu-row stack under an orange `headerbutton*` bar.
//! - [`ClassicTheme::paint_button`]: bottom-edge `BACK` / `RESET TO DEFAULTS` / `ACCEPT` text buttons.
//! - [`ClassicTheme::paint_list`]: 9-slice list box with pulsing `listbox_cursor`, `listbox_hilight`, and scrollbar.
//! - [`ClassicTheme::paint_text`]: titles, headings, 260-unit word-wrapped popup prompts, and notes.
//! - [`ClassicTheme::paint_panel`]: 9-slice `border_3_pieces` window frames.
//! - [`ClassicTheme::paint_horizontal_buttons`]: centered `YES` / `NO` / `OK` popup buttons.
//! - [`ClassicTheme::paint_tabs`]: 3-piece orange header-bar tabs (`INFANTRY` / `VEHICLE` / `STARFIGHTER`).
//! - [`ClassicTheme::paint_options`]: two-column `label | value` option pages (`game`, `video`, `audio`, `controls`).
//! - [`ClassicTheme::paint_option_row`]: standalone single option row.
//! - [`ClassicTheme::paint_screen`] & [`ClassicTheme::paint_stack`]: full screen and modal stack composition.

use crate::fonts::{Fonts, FONT_LARGE, FONT_MEDIUM, FONT_SMALL, FONT_TINY};
use crate::options::OptionKind;
use crate::screen::MenuScreen;
use crate::space::{anchor, mul, rotate_y, translate, Batch, UV};
use crate::stack::ScreenStack;
use crate::surface::Surface;
use crate::widgets::{
    ButtonWidget, HorizontalButtonsWidget, ListLook, ListWidget, OptionRowWidget,
    OptionsLayoutKind, OptionsWidget, PanelWidget, RowItem, RowsLayout, TabsWidget, TextLook,
    TextWidget, Widget,
};

use super::{
    cursor_pulse, draw_bar, draw_frame, draw_scroll_bar, draw_slider_bar, draw_str, menu_pulse,
    piece_quad, rotation_euler, row_bar_at, rows_shift, scaled_color, selected_color, str_width,
    tint_color, ClassicTheme, HitRegion, HitTarget, BAR_X, BLACK, BUTTON_INSET, DIM, DIMMED, GREY,
    NOTE_YELLOW, TEXT_X, WHITE,
};

impl ClassicTheme {
    /// Draws an angled stack of menu rows under an orange header bar, matching
    /// `draw_rows` in `rust/bfe-shell/src/render.rs` and `paintRows` in
    /// `src/menus/themes/classic/index.js` pixel-for-pixel.
    ///
    /// Geometry details (`paintRows` in `src/menus/themes/classic/index.js`):
    /// - Row pitch `p = font.height + layout.y_spacing`; the vertical stack of `n` visible rows
    ///   is centered around `y = 0` so the top row center sits at `y0 = -(n * p) * 0.5`.
    /// - The container is shifted horizontally by `rows_shift(layout.width, p, list_count)`
    ///   and rotated in 3D by `rotation_euler(rot_x, rot_y, rot_z, xyz_order)` (`rot_y = 35` degrees
    ///   on the top-level options screen, giving the classic perspective tilt).
    /// - Each row bar uses the 3-piece `buttonleft` / `buttonmid` / `buttonright` art starting at
    ///   `BAR_X = -4.7`, with width `bar_w + 5.0` and its right triangle cap flipped vertically
    ///   for rows below the vertical midpoint (`k >= n / 2`).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_rows_slice(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        title: &str,
        rows: &[RowItem],
        selected: usize,
        focused_widget: bool,
        pulse: f64,
        layout: RowsLayout,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        // Row pitch `p` is the bitmap font line height plus `layout.y_spacing` (`5.0` units
        // by default, matching `paintRows` in `src/menus/themes/classic/index.js`).
        let font = fonts.by_name(layout.font);
        let yh = font.height;
        let y_spacing = layout.y_spacing;
        let p = yh + y_spacing;
        // Count visible rows (`!r.hidden`); if every row is hidden, skip both the header bar
        // and the row loop.
        let n = rows.iter().filter(|r| !r.hidden).count();
        if n == 0 {
            return;
        }
        // Shift the container left by half of the widest bar in the full row list (`list_count`
        // includes console-only rows hidden on PC, replicating `AddVerticalButtons` in `common.lvl`),
        // then apply the 3D Euler rotation (`rot_y = 35` degrees on the main options menu).
        let shift = rows_shift(layout.width, p, layout.list_count.unwrap_or(n));
        let base = anchor(layout.at[0], layout.at[1], true, surface.height);
        let moved = translate(base, shift + layout.x, layout.y, 0.0);
        let m = mul(
            moved,
            rotation_euler(layout.rot_x, layout.rot_y, layout.rot_z, layout.xyz_order),
        );
        let y0 = -(n as f64 * p) * 0.5;

        // Draw the orange `headerbutton*` title bar 1.75 pitches above the first row (`y0`),
        // spanning from `0.25 * p` above the title text to `0.25 * p` below its glyph depth,
        // with the title string rendered in solid black (`BLACK`) centered on `caps_middle`.
        if !title.is_empty() {
            let ty = y0 - 1.75 * p;
            let width = layout.width - 1.25 * (yh - y_spacing) - 0.65 * p;
            let by = ty - 0.25 * p;
            let gh = font.height;
            // The header bar width shrinks by `1.25 * (yh - y_spacing) + 0.65 * p` relative to
            // `layout.width` so its slanted right cap lines up with the angled chevron slope
            // of the first menu row underneath it (`paintRows` in `src/menus/themes/classic/index.js`).
            draw_bar(
                batch,
                surface,
                m,
                ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"],
                BAR_X,
                by,
                width,
                ty + gh + 0.25 * p,
                false,
                WHITE,
            );
            // Inset the black header title by `0.5 * p + TEXT_X` from the left edge and center
            // its capital letters vertically inside `[by, ty + gh + 0.25 * p]`.
            draw_str(
                font,
                batch,
                m,
                p * 0.5 + TEXT_X,
                (by + ty + gh + 0.25 * p) * 0.5 - font.caps_middle(),
                title,
                BLACK,
                0,
            );
        }

        // Paint each visible row's 3-piece blue bar (`buttonleft`/`buttonmid`/`buttonright`).
        // Focused enabled rows pulse with `pulse` (`0.3..1.0`); unfocused rows are darkened
        // by `DIMMED = 150 / 255` (`draw.js`).
        let mut k = 0usize;
        for row in rows {
            if row.hidden {
                continue;
            }
            // Vertical center `cy` of visible row `k`; a row only pulses when both its
            // parent `RowsWidget` is focused on the screen and `k == selected`.
            let cy = y0 + (k as f64) * p;
            let focused = focused_widget && k == selected && !row.disabled;
            let a = if focused { pulse } else { 1.0 };
            let bar_color = if focused {
                tint_color(WHITE, a)
            } else {
                scaled_color(WHITE, DIMMED, a)
            };
            // Compute the chevron bar width and whether the right triangle cap is flipped
            // vertically (`k >= n / 2`), adding 5 units to `bar_w` to match `bars[k].width + 5` in `classic/index.js`.
            let (bar_w, flipped) = row_bar_at(layout.width, p, n, k);
            draw_bar(
                batch,
                surface,
                m,
                ["buttonleft", "buttonmid", "buttonright"],
                BAR_X,
                cy - p * 0.5,
                bar_w + 5.0,
                cy + p * 0.5,
                flipped,
                bar_color,
            );
            // Enabled row labels draw in `WHITE`; disabled rows (`ONLINE`, `INTEGRATIONS`) draw in `GREY`.
            let text_color = if !row.disabled {
                tint_color(WHITE, a)
            } else {
                tint_color(GREY, a)
            };
            // Place the row label at `TEXT_X = 0.8` units right of the origin, vertically
            // centered on `cy` via `font.caps_middle()` (`paintRows` in `classic/index.js`).
            draw_str(
                font,
                batch,
                m,
                TEXT_X,
                cy - font.caps_middle(),
                &row.label,
                text_color,
                0,
            );
            // Record the row's local bounding box `[BAR_X, cy - p/2, BAR_X + bar_w + 5 + p, cy + p/2]`
            // under the rotated matrix `m` so `MenuInput::hit_test` can intersect pointer rays in 3D.
            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m,
                    rect: [BAR_X, cy - p * 0.5, BAR_X + bar_w + 5.0 + p, cy + p * 0.5],
                    target: HitTarget::Row {
                        widget_index,
                        row_index: k,
                    },
                });
            }
            k += 1;
        }
    }

    /// Draws a bottom-edge or positioned button, matching `draw_bottom_button` in
    /// `rust/bfe-shell/src/render.rs` and `paintButton` in `src/menus/themes/classic/index.js`.
    ///
    /// Replicates the 2004 `CreateNegativeButton` (`place = "back"`), `CreateMiddleButton`
    /// (`place = "middle"`), and `CreatePositiveButton` (`place = "accept"`) text buttons:
    /// - Anchored to the bottom of the 90% safe area (`ay = 1.0`) in `gamefont_medium`.
    /// - Left/right buttons sit `BUTTON_INSET = 21.0` units inside the safe-area edges;
    ///   their capital midline sits `12.0` units above the safe-area bottom.
    /// - Focused buttons pulse yellow via [`selected_color`]; unfocused enabled buttons are white.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_button(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        button: &ButtonWidget,
        focused: bool,
        pulse: f64,
        widget_index: usize,
        hits: Option<&mut Vec<HitRegion>>,
    ) {
        if button.hidden {
            return;
        }
        // Resolve the safe-area bottom-edge anchor and alignment from `button.place`
        // (`BUTTON_PLACES` in `src/menus/themes/classic/index.js`: `back` = bottom-left,
        // `middle` = bottom-center, `accept` = bottom-right).
        let (ax, ay, default_align) = match button.place {
            "accept" => (1.0, 1.0, 1u32),
            "middle" => (0.5, 1.0, 2u32),
            _ => (0.0, 1.0, 0u32),
        };
        // Allow individual buttons to override their anchor `at` and horizontal `align`
        // while defaulting to the standard safe-area bottom-bar placement.
        let at = button.at.unwrap_or([ax, ay]);
        let align = button.align.unwrap_or(default_align);
        let m = anchor(at[0], at[1], true, surface.height);
        // Inset left/right corner buttons by `BUTTON_INSET = 21.0` units from the safe-area edge
        // and position the capital midline `12.0` units above the safe-area bottom (`y = -12 - capsMiddle`).
        let x = button.x.unwrap_or(if align == 0 {
            BUTTON_INSET
        } else if align == 1 {
            -BUTTON_INSET
        } else {
            0.0
        });
        let font = fonts.by_name(button.font);
        // Subtract `font.caps_middle()` from the baseline offset (`-12.0` by default) so
        // uppercase button labels sit vertically centered on the target line.
        let y = button.y.unwrap_or(-12.0) - font.caps_middle();
        // Corner buttons have no backing bar in the 2004 PC shell; focus is shown purely
        // by pulsing the glyph tint with `selected_color(pulse)`.
        let color = if button.disabled {
            GREY
        } else if focused {
            selected_color(pulse)
        } else {
            WHITE
        };
        draw_str(font, batch, m, x, y, &button.text, color, align);

        // Emit a 25-unit-tall hotspot (`-27.5..-2.5`) padded by 4 units on each side of the text
        // (`corners(ctx, m, left - 4, -15 - 12.5, left + width + 4, -15 + 12.5)` in `classic/index.js`).
        if let Some(h) = hits {
            // Enforce a minimum 48-unit hit width so short labels (`"OK"`, `"NO"`) remain easy
            // to click, and align `[x0, x0 + w]` with the rendered text bounds.
            let w = str_width(font, &button.text).max(48.0);
            let x0 = match align {
                1 => x - w,
                2 => x - w * 0.5,
                _ => x,
            };
            h.push(HitRegion {
                m,
                rect: [x0 - 4.0, -27.5, x0 + w + 4.0, -2.5],
                target: HitTarget::Button { widget_index },
            });
        }
    }

    /// Draws a [`ListWidget`] inside a 9-slice `border_3_pieces` frame with `listbox_cursor`
    /// selection highlight, `listbox_hilight` hover highlight, and scrollbar, matching
    /// `paintList` in `src/menus/themes/classic/index.js` and `draw_instant_action` in `render.rs`.
    ///
    /// Layout and highlight rules (`paintList` in `src/menus/themes/classic/index.js`):
    /// - The outer `border_3_pieces` frame is centered on `(look.box_x, look.box_y)` with size
    ///   `(look.box_w, look.box_h)` under the safe-area anchor `look.at`.
    /// - Visible rows `[first .. first + shown)` are spaced by `look.pitch` starting at
    ///   `top = box_y - box_h * 0.5 + look.inset`.
    /// - The selected row draws a 9-slice `listbox_cursor` frame (`inner + 6` wide, `pitch + 10` tall)
    ///   that pulses `1.0 -> 0.5 -> 1.0` via [`cursor_pulse`] while the list has focus.
    /// - A non-selected hovered row draws a static `listbox_hilight` 9-slice frame of the same size.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_list(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        list: &ListWidget,
        focused: bool,
        elapsed: f64,
        _pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if list.hidden {
            return;
        }
        // Build the list container transform from `look.at`, `(look.x, look.y)`, and `look.rot_y`,
        // then draw the outer 9-slice `border_3_pieces` window frame if `look.show_frame` is true.
        let look: ListLook = list.look;
        let base = anchor(look.at[0], look.at[1], true, surface.height);
        let m = rotate_y(
            translate(base, look.x, look.y, 0.0),
            look.rot_y.to_radians(),
        );
        // Draw the 9-slice `border_3_pieces` window around the list box unless disabled
        // by `look.show_frame == false` (`look.window !== false` in `classic/index.js`).
        if look.show_frame {
            draw_frame(
                batch,
                surface,
                m,
                look.box_x,
                look.box_y,
                look.box_w,
                look.box_h,
                "border_3_pieces",
                WHITE,
            );
        }
        // When `count > shown`, reserve `26.0` units on the right inside the frame for the
        // vertical scrollbar so row highlights and right-aligned text do not overlap the track.
        let count = list.rows.len();
        let shown = list.shown.max(1);
        let scroll = count > shown;
        // Compute the inner content rectangle `[left, left + inner]` and top row edge `top`
        // after subtracting `inset`, `pad_left`, `pad_right`, and the 26-unit scrollbar gutter.
        let first = list.first.min(count.saturating_sub(1));
        let top = look.box_y - look.box_h * 0.5 + look.inset;
        let left = look.box_x - look.box_w * 0.5 + look.pad_left;
        let inner = look.box_w - look.pad_left - look.pad_right - if scroll { 26.0 } else { 0.0 };
        let font = fonts.by_name(look.font);

        // Iterate over at most `shown` visible slots starting at `first`, stopping early
        // when the list has fewer than `shown` total entries.
        for k in 0..shown {
            let idx = first + k;
            let Some(entry) = list.rows.get(idx) else {
                break;
            };
            // Draw the selected row's 9-slice `listbox_cursor` frame (pulsing `1.0 -> 0.5 -> 1.0`
            // once per second via `cursor_pulse` while the list widget is focused) or the hovered
            // row's static `listbox_hilight` frame.
            let cy = top + (k as f64) * look.pitch + look.pitch * 0.5;
            if idx == list.selected {
                let a = if focused {
                    cursor_pulse(elapsed)
                } else {
                    1.0
                };
                draw_frame(
                    batch,
                    surface,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    look.pitch + 10.0,
                    "listbox_cursor",
                    tint_color(WHITE, a),
                );
            // When the pointer hovers a non-selected row, highlight it with `listbox_hilight`
            // at constant full opacity (`1.0`).
            } else if list.hover_row == Some(idx) {
                draw_frame(
                    batch,
                    surface,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    look.pitch + 10.0,
                    "listbox_hilight",
                    WHITE,
                );
            }
            // Disabled list rows render in `GREY`; enabled rows render in `WHITE` regardless
            // of whether the selection cursor is on them (since `listbox_cursor` provides the highlight).
            let text_color = if entry.disabled { GREY } else { WHITE };
            let ty = cy - font.caps_middle();

            // Render 1-column or 2-column cell strings (`action | bound_keys` on `CONTROL OPTIONS`),
            // offsetting each column by `left + 8.0 + column.width * inner` (`paintList` in `classic/index.js`).
            if entry.cell_count >= 2 && list.columns.len() >= 2 {
                let col0_x = left + 8.0 + list.columns[0].width * inner;
                draw_str(
                    font,
                    batch,
                    m,
                    col0_x,
                    ty,
                    &entry.cells[0],
                    text_color,
                    list.columns[0].align,
                );
                let col1_x = left + 8.0 + list.columns[1].width * inner;
                draw_str(
                    font,
                    batch,
                    m,
                    col1_x,
                    ty,
                    &entry.cells[1],
                    text_color,
                    list.columns[1].align,
                );
            // Fallback 2-column split when explicit `ListColumn` widths were not supplied:
            // place the first column at `left + 8.0` and the second at `40%` of `inner`.
            } else if entry.cell_count >= 2 {
                draw_str(font, batch, m, left + 8.0, ty, &entry.cells[0], text_color, 0);
                draw_str(
                    font,
                    batch,
                    m,
                    left + 8.0 + 0.4 * inner,
                    ty,
                    &entry.cells[1],
                    text_color,
                    0,
                );
            // Single-column list entry (e.g., map list or profile list).
            } else {
                draw_str(font, batch, m, left + 8.0, ty, &entry.cells[0], text_color, 0);
            }

            // Draw any trailing `16.9 x 16.9` era/faction icons (`rep_icon`, `cis_icon`, `all_icon`, `imp_icon`)
            // flush against the right edge of `inner` with a 3-unit gap between icons.
            if !entry.icons.is_empty() {
                let size = 16.9;
                for (icon_i, icon_name) in entry.icons.iter().enumerate() {
                    let right =
                        left + inner - ((entry.icons.len() - icon_i - 1) as f64) * (size + 3.0);
                    piece_quad(
                        batch,
                        surface,
                        m,
                        icon_name,
                        right - size,
                        cy - size * 0.5,
                        right,
                        cy + size * 0.5,
                        UV,
                        WHITE,
                    );
                }
            }

            // Record the list row's hit rectangle across `[left, left + inner]` and attach optional
            // slider bounds `[left + 0.55 * inner, left + inner - 8.0]` for list rows with a slider.
            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m,
                    rect: [left, cy - look.pitch * 0.5, left + inner, cy + look.pitch * 0.5],
                    target: HitTarget::ListRow {
                        widget_index,
                        row_index: idx,
                        slider_x: entry
                            .slider_ratio
                            .map(|_| [left + inner * 0.55, left + inner - 8.0]),
                    },
                });
            }
        }

        // Place the 16-unit-wide vertical scrollbar 34 units inside the right border of the list box
        // (`x + width / 2 - 34` in `paintList` in `src/menus/themes/classic/index.js`).
        if scroll {
            draw_scroll_bar(
                batch,
                surface,
                m,
                look.box_x + look.box_w * 0.5 - 34.0,
                top,
                (shown as f64) * look.pitch,
                first,
                shown,
                count,
            );
        }
    }

    /// Draws a [`TextWidget`] (`title`, `heading`, `popup`, `subtitle`, `note`, `label`), matching
    /// `paintText` in `src/menus/themes/classic/index.js` and `draw_str` in `render.rs`.
    ///
    /// Handles two distinct layout modes:
    /// 1. [`TextLook::Popup`]: centered on the full viewport (`anchor(0.5, 0.5, false, H)`) in
    ///    `gamefont_medium`, greedily word-wrapping each paragraph to `260.0` units without
    ///    heap allocation and stepping downward from `y = -74.0` by `font.height` per line.
    /// 2. All other [`TextLook`] variants: anchored at `text.at` (`safe = text.safe`) using
    ///    `text.font` (or the look's default font) and stepping downward from `text.y`.
    pub fn paint_text(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        text: &TextWidget,
    ) {
        // Skip hidden or empty text widgets before computing anchor matrices or font lookups.
        if text.hidden || text.text.is_empty() {
            return;
        }
        // Popup prompts (`TextLook::Popup`) are centered on the full screen (`safe = false`) starting
        // at `y = -74.0` and word-wrapped to `260.0` units without heap allocation by slicing
        // each raw line on ASCII spaces.
        if text.look == TextLook::Popup {
            let font = fonts.by_name(FONT_MEDIUM);
            let m = anchor(0.5, 0.5, false, surface.height);
            let color = tint_color(text.color.unwrap_or(WHITE), text.alpha);
            let mut y = -74.0;
            for raw_line in text.text.lines() {
                let bytes = raw_line.as_bytes();
                let mut line_start = 0usize;
                let mut line_end = 0usize;
                let mut i = 0usize;
                // Scan ASCII words in-place so `paint_text` never allocates a `Vec<String>`
                // during per-frame rendering of confirmation or key-capture popups.
                while i < bytes.len() {
                    while i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        break;
                    }
                    let word_start = i;
                    while i < bytes.len() && bytes[i] != b' ' {
                        i += 1;
                    }
                    let word_end = i;
                    // First word on the current wrapped line always fits (even if > 260 units);
                    // subsequent words are appended if `str_width(candidate) <= 260.0`, or flushed
                    // to `batch` to start a new line at `y + font.height`.
                    if line_start == line_end {
                        line_start = word_start;
                        line_end = word_end;
                    } else {
                        let candidate = &raw_line[line_start..word_end];
                        if str_width(font, candidate) <= 260.0 {
                            line_end = word_end;
                        } else {
                            draw_str(
                                font,
                                batch,
                                m,
                                0.0,
                                y,
                                &raw_line[line_start..line_end],
                                color,
                                2,
                            );
                            y += font.height;
                            line_start = word_start;
                            line_end = word_end;
                        }
                    }
                }
                // Flush the trailing wrapped line of the current paragraph.
                if line_end > line_start {
                    draw_str(
                        font,
                        batch,
                        m,
                        0.0,
                        y,
                        &raw_line[line_start..line_end],
                        color,
                        2,
                    );
                    y += font.height;
                }
            }
            return;
        }
        // Non-popup text widgets pick their default bitmap font from `text.look` (`gamefont_large`
        // for titles/headings, `gamefont_medium` for subtitles/labels, `gamefont_small` for notes)
        // and advance `y` by `font.height` for each `\n`-separated line.
        let default_font = match text.look {
            TextLook::Title | TextLook::Heading => FONT_LARGE,
            TextLook::Popup | TextLook::Subtitle | TextLook::Label => FONT_MEDIUM,
            TextLook::Note => FONT_SMALL,
        };
        let font_name = text.font.unwrap_or(default_font);
        let font = fonts.by_name(font_name);
        let m = anchor(text.at[0], text.at[1], text.safe, surface.height);
        let base_color = text.color.unwrap_or(WHITE);
        let color = tint_color(base_color, text.alpha);

        let mut y = text.y;
        for line in text.text.lines() {
            draw_str(font, batch, m, text.x, y, line, color, text.align);
            y += font.height;
        }
    }

    /// Draws a 9-slice [`PanelWidget`] (`paintPanel` in `src/menus/themes/classic/index.js`).
    ///
    /// Emits a `border_3_pieces` (or custom `panel.texture`) 9-slice frame centered at
    /// `(panel.x, panel.y)` of size `panel.w x panel.h`, plus an optional centered caption above it.
    pub fn paint_panel(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        panel: &PanelWidget,
    ) {
        // Skip hidden panels; otherwise build the safe-area or full-screen anchor matrix
        // (`panel.safe == false` on centered `300 x 180` and `360 x 180` popup windows).
        if panel.hidden {
            return;
        }
        let m = anchor(panel.at[0], panel.at[1], panel.safe, surface.height);
        draw_frame(
            batch,
            surface,
            m,
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            panel.texture,
            panel.color,
        );
        // If the panel has a title caption, draw it centered 4 units above the top border.
        if !panel.title.is_empty() {
            let font = fonts.by_name(FONT_MEDIUM);
            draw_str(
                font,
                batch,
                m,
                panel.x,
                panel.y - panel.h * 0.5 - font.height - 4.0,
                &panel.title,
                WHITE,
                2,
            );
        }
    }

    /// Draws a centered row of horizontal buttons (`ADD` / `REMOVE`, `YES` / `NO`, `OK`),
    /// matching `draw_popup_box` and `draw_instant_action` in `rust/bfe-shell/src/render.rs`
    /// and `paintHorizontal` in `src/menus/themes/classic/index.js`.
    ///
    /// Each button is drawn centered (`align = 2`) at `by = hb.y - font.caps_middle()` with an
    /// `80 x 45` hotspot (`[x - 40, y - 22.5, x + 40, y + 22.5]`).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_horizontal_buttons(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        hb: &HorizontalButtonsWidget,
        focused: bool,
        pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if hb.hidden || hb.buttons.is_empty() {
            return;
        }
        // Anchor popup answer buttons to the full-screen center (`safe = false`).
        // A single button (`OK`) sits at `x = 0.0`; two buttons (`YES` / `NO` with `spacing = 85.0`)
        // sit at `x = -40.0` and `x = +45.0` (`-40 + i * 85 - (count - 2) * 42.5` in `classic/index.js`).
        let m = anchor(0.5, 0.5, false, surface.height);
        let font = fonts.by_name(hb.font);
        let by = hb.y - font.caps_middle();
        let count = hb.buttons.len();
        for (i, item) in hb.buttons.iter().enumerate() {
            // Compute each horizontal button's X center: `0.0` for a lone `OK` button, or
            // symmetric steps of `hb.spacing` (`85.0`) around `-40.0 / +45.0` for two or more buttons.
            let x = if count == 1 {
                0.0
            } else {
                -40.0 + (i as f64) * hb.spacing - ((count as f64) - 2.0) * (hb.spacing * 0.5)
            };
            // Only the currently selected horizontal button (`i == hb.selected`) pulses yellow
            // when the `HorizontalButtonsWidget` itself holds screen focus.
            let color = if item.disabled {
                GREY
            } else if focused && i == hb.selected {
                selected_color(pulse)
            } else {
                WHITE
            };
            draw_str(font, batch, m, x, by, &item.label, color, 2);
            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m,
                    rect: [x - 40.0, hb.y - 22.5, x + 40.0, hb.y + 22.5],
                    target: HitTarget::HorizontalButton {
                        widget_index,
                        button_index: i,
                    },
                });
            }
        }
    }

    /// Draws the 3-piece horizontal orange header-bar tabs (`INFANTRY` / `VEHICLE` / `STARFIGHTER`
    /// on `CONTROL OPTIONS`), matching `paintTabs` in `src/menus/themes/classic/index.js`.
    ///
    /// Tab bar colors (`paintTabs` in `src/menus/themes/classic/index.js`):
    /// - Focused tab label: pulsing yellow (`selected_color(pulse)`) at full alpha `1.0`.
    /// - Active (unfocused) tab: solid orange bar (`alpha = 1.0`) with `WHITE` label.
    /// - Inactive (unfocused) tab: dimmed orange bar (`alpha = 0.4`) with light grey (`200/255`) label.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_tabs(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        tabs: &TabsWidget,
        focused: bool,
        pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if tabs.hidden {
            return;
        }
        // Each tab is drawn as an orange 3-piece `headerbutton*` bar spanning `y - 13.0..y + 11.0`
        // (`24.0` units tall), spaced by `1/3` of the safe width (`sw = 0.9 * 1024 = 921.6`) with
        // bar width `0.30 * sw` (`CONTROLS_TABS` in `option-pages.js` and `paintTabs` in `classic/index.js`).
        // The active tab bar draws at full alpha (`1.0`); inactive tabs dim to `0.4` alpha.
        let m = anchor(0.0, 0.0, true, surface.height);
        let font = fonts.by_name(tabs.font);
        let caps = font.caps_middle();
        let sw = 0.9 * 1024.0;
        let y0 = tabs.y - 13.0;
        let y1 = tabs.y + 11.0;
        // Step across the 3 `CONTROL OPTIONS` tabs (`INFANTRY`, `VEHICLE`, `STARFIGHTER`)
        // at `x = 16.0 + i * (sw / 3.0)`, drawing each tab's orange `headerbutton*` bar
        // from `x - 8.0` and its label 4 units inside `x` (`paintTabs` in `classic/index.js`).
        for (i, tab) in tabs.tabs.iter().enumerate() {
            let x = 16.0 + (i as f64) * (1.0 / 3.0) * sw;
            let width = 0.30 * sw;
            let active = tab.id == tabs.active;
            let is_focused = focused && i == tabs.selected;
            let a = if active { 1.0 } else { 0.4 };
            draw_bar(
                batch,
                surface,
                m,
                ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"],
                x - 8.0,
                y0,
                width - (y1 - y0),
                y1,
                false,
                tint_color(WHITE, a),
            );
            // Focused tab text pulses yellow at full opacity; unfocused active tab text is
            // solid white; inactive tab text uses `rgba(200, 200, 200)` multiplied by `a = 0.4`.
            let base_color = if is_focused {
                selected_color(pulse)
            } else if active {
                WHITE
            } else {
                [200.0 / 255.0, 200.0 / 255.0, 200.0 / 255.0, 1.0]
            };
            let text_color = tint_color(base_color, if is_focused { 1.0 } else { a });
            draw_str(
                font,
                batch,
                m,
                x + 4.0,
                (y0 + y1) * 0.5 - caps,
                &tab.label,
                text_color,
                0,
            );
            // Record the tab bar's bounding box `[x - 8.0, y0, x + width, y1]` for pointer hover/click.
            if let Some(hr) = hits.as_deref_mut() {
                hr.push(HitRegion {
                    m,
                    rect: [x - 8.0, y0, x + width, y1],
                    target: HitTarget::Tab {
                        widget_index,
                        tab_index: i,
                    },
                });
            }
        }
    }

    /// Draws the two-column `label | value` [`OptionsWidget`] (`game`, `video`, `audio`, `controls`),
    /// matching `paintOptions` and `OPTION_LOOKS` in `src/menus/themes/classic/index.js` pixel-for-pixel.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_options(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        opts: &OptionsWidget,
        focused: bool,
        _elapsed: f64,
        pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if opts.hidden {
            return;
        }
        // Resolve the per-page `OPTION_LOOKS` geometry from `src/menus/themes/classic/index.js`:
        // - `Game` (`ifs_opt_general`): `gamefont_medium`, centered `0.5` split (`-15` / `+15`), `top = 150`, `pitch = 40`.
        // - `Video` (`ifs_opt_pcvideo`): `gamefont_medium`, centered `0.5` split, `top = 32`, `pitch = 30`,
        //   `bottom = 60` (paginated with scrollbar at `[0.5, 380]`), 110-unit sliders at `[0.5, 15]`.
        // - `Audio` (`ifs_opt_sound`): `gamefont_medium`, `0.4` split (`0` / `+35`), `top = 55`, `pitch = 35`,
        //   wide sliders spanning from `[0.4, 20]` to `[1.0, -176.8]` with numbers right-aligned at `[1.0, 0]`.
        // - `Controls` (`ifs_opt_pccontrols` top rows): `gamefont_tiny`, left-aligned labels at `[0.15, 0]`,
        //   `top = 121`, `pitch = 24.5`, relative `0.25 * safe_w` sliders at `[0.4, 0]`, numbers at `[1.0, -2]`.
        let safe_h = 0.9 * surface.height;
        let safe_w = 0.9 * 1024.0;
        let (
            font_name,
            labels_at,
            values_at,
            top,
            pitch,
            row_h,
            text_dy,
            bottom_opt,
            slider_cfg,
            number_at,
            scroll_at,
            align_left,
        ) = match opts.layout {
            // `GAME OPTIONS`: centered `0.5` split, labels 15 units left of center, values 15 units
            // right of center, starting 150 units below the safe-area top (`pitch = 40`, `text_dy = 2`).
            OptionsLayoutKind::Game => (
                FONT_MEDIUM,
                [0.5, -15.0],
                Some([0.5, 15.0]),
                150.0,
                40.0,
                35.0,
                2.0,
                None,
                None,
                None,
                None,
                false,
            ),
            // `VIDEO OPTIONS`: centered `0.5` split starting at `top = 32` (`pitch = 30`, `bottom = 60`),
            // with 110-unit sliders at `[0.5, 15]`, slider readouts at `[0.5, 140]`, and scrollbar at `[0.5, 380]`.
            OptionsLayoutKind::Video => (
                FONT_MEDIUM,
                [0.5, -15.0],
                Some([0.5, 15.0]),
                32.0,
                30.0,
                28.0,
                0.0,
                Some(60.0),
                Some(([0.5, 15.0], None, 110.0, 22.0, false)),
                Some([0.5, 140.0]),
                Some([0.5, 380.0]),
                false,
            ),
            // `AUDIO OPTIONS`: `0.4` split starting at `top = 55` (`pitch = 35`), with wide sliders
            // from `[0.4, 20]` to `[1.0, -176.8]` and right-aligned percentage readouts at `[1.0, 0]`.
            OptionsLayoutKind::Audio => (
                FONT_MEDIUM,
                [0.4, 0.0],
                Some([0.4, 35.0]),
                55.0,
                35.0,
                35.0,
                0.0,
                None,
                Some(([0.4, 20.0], Some([1.0, -176.8]), 0.0, 24.0, false)),
                Some([1.0, 0.0]),
                None,
                false,
            ),
            // `CONTROL OPTIONS` top rows (`Top`): `gamefont_tiny`, left-aligned labels at `[0.15, 0]`,
            // `top = 121` (`pitch = 24.5`), relative `0.25 * safe_w` sliders at `[0.4, 0]`, numbers at `[1.0, -2]`.
            OptionsLayoutKind::Controls => (
                FONT_TINY,
                [0.15, 0.0],
                None,
                121.0,
                24.5,
                25.0,
                0.0,
                None,
                Some(([0.4, 0.0], None, 0.25, 24.0, true)),
                Some([1.0, -2.0]),
                None,
                true,
            ),
        };

        let font = fonts.by_name(font_name);
        let caps = font.caps_middle();

        // Count non-hidden option items; if the page has no visible items, return early.
        let total_visible = opts.items.iter().filter(|it| !it.hidden).count();
        if total_visible == 0 {
            return;
        }
        // Compute how many visible option rows fit above `bottom` (`VIDEO OPTIONS` reserves 60 units
        // at the bottom for the yellow note and bottom buttons) and clamp the `[first, end)` window.
        let shown = if let Some(bottom) = bottom_opt {
            let room = safe_h - top - bottom;
            ((room / pitch).floor() as usize).max(1)
        } else {
            total_visible
        };
        let first = opts.first.min(total_visible.saturating_sub(shown));
        let end = (first + shown).min(total_visible);

        // Precompute the safe-area column anchor matrices for the label and value columns
        // so every visible row in the loop reuses the same 4x4 matrices.
        let labels_m = anchor(labels_at[0], 0.0, true, surface.height);
        let values_m = values_at.map(|va| anchor(va[0], 0.0, true, surface.height));

        let mut vis_pos = 0usize;
        for (item_idx, item) in opts.items.iter().enumerate() {
            if item.hidden {
                continue;
            }
            // Skip visible rows outside the current pagination window `[first, end)`.
            if vis_pos < first || vis_pos >= end {
                vis_pos += 1;
                continue;
            }
            let k = vis_pos - first;
            vis_pos += 1;

            // Center each visible row vertically on `cy = top + k * pitch` using `caps_middle()`.
            // Read-only `OptionKind::Fixed` rows with `item.dim` (or disabled rows) draw their value
            // in `GREY`; the focused row pulses yellow via `selected_color(row_pulse)`.
            let cy = top + (k as f64) * pitch;
            let y = cy - caps + text_dy;
            let row_focused = focused && item_idx == opts.selected;
            let enabled = !opts.disabled && (!item.disabled || item.kind == OptionKind::Fixed);
            let greyed = !enabled || (item.kind == OptionKind::Fixed && item.dim);

            // `Game`, `Video`, and `Audio` right-align (`align = 1`) their labels against `labels_at`;
            // `Controls` left-aligns (`align = 0`) its top-row labels at `0.15` of the safe width.
            let label_align = if align_left { 0 } else { 1 };
            if !item.label.is_empty() {
                draw_str(
                    font,
                    batch,
                    labels_m,
                    labels_at[1],
                    y,
                    &item.label,
                    WHITE,
                    label_align,
                );
            }

            let row_pulse = if row_focused { pulse } else { 1.0 };
            let value_color = if greyed {
                GREY
            } else if row_focused {
                selected_color(row_pulse)
            } else {
                WHITE
            };

            // For `OptionKind::Slider` rows, resolve the right end `x1` of the track (either from
            // a second safe-area anchor `to_at` on `AUDIO OPTIONS`, a fraction of `safe_w` on
            // `CONTROL OPTIONS`, or a fixed width on `VIDEO OPTIONS`), draw the slider bar, and
            // draw the numeric readout at `number_at`.
            let mut slider_x = None;
            if item.kind == OptionKind::Slider {
                if let Some((from_at, to_opt, s_width, s_height, relative)) = slider_cfg {
                    // When `to_opt` is `Some(to_at)` (`AUDIO OPTIONS` sliders spanning from
                    // `[0.4, 20.0]` to `[1.0, -176.8]`), convert both safe-area anchors into
                    // `1024`-unit horizontal coordinates so `x1 - x0` spans the exact distance
                    // between the two anchors in the `from_m` coordinate frame (`paintOptions` in `classic/index.js`).
                    let from_m = anchor(from_at[0], 0.0, true, surface.height);
                    let x0 = from_at[1];
                    let x1 = if let Some(to_at) = to_opt {
                        let to_x = (0.05 + 0.9 * to_at[0]) * 1024.0 + to_at[1];
                        let from_x = (0.05 + 0.9 * from_at[0]) * 1024.0 + from_at[1];
                        x0 + (to_x - from_x)
                    } else if relative {
                        x0 + s_width * safe_w
                    } else {
                        x0 + s_width
                    };
                    draw_slider_bar(
                        batch,
                        surface,
                        from_m,
                        x0,
                        cy - s_height * 0.5,
                        x1 - x0,
                        s_height,
                        item.ratio,
                        row_focused,
                        row_pulse,
                    );
                    slider_x = Some([x0, x1]);
                    // Draw the slider's formatted value string at `number_at`, right-aligning
                    // when `num_at[0] >= 1.0` (e.g., `AUDIO` and `CONTROLS` at the right safe edge)
                    // and left-aligning when placed next to a fixed-width slider (`VIDEO` at `[0.5, 140]`).
                    if let Some(num_at) = number_at {
                        let num_m = anchor(num_at[0], 0.0, true, surface.height);
                        let num_align = if num_at[0] >= 1.0 { 1 } else { 0 };
                        draw_str(
                            font,
                            batch,
                            num_m,
                            num_at[1],
                            y,
                            &item.value_text,
                            value_color,
                            num_align,
                        );
                    }
                }
            // Choice, toggle, action, and fixed rows draw their text left-aligned at `values_at`
            // (`[0.5, 15]` on Game/Video, `[0.4, 35]` on Audio), or right-aligned at `number_at`
            // when `values_at` is `None` (`CONTROL OPTIONS` top rows).
            } else if let (Some(vm), Some(va)) = (values_m, values_at) {
                draw_str(font, batch, vm, va[1], y, &item.value_text, value_color, 0);
            } else if let Some(num_at) = number_at {
                let num_m = anchor(num_at[0], 0.0, true, surface.height);
                draw_str(
                    font,
                    batch,
                    num_m,
                    num_at[1],
                    y,
                    &item.value_text,
                    value_color,
                    1,
                );
            }

            // Span the row hotspot from the label column (`-400.0`) across the value/slider column
            // (`+450.0`) and attach `slider_x` so `MenuInput::on_pointer_down` can detect track drags.
            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m: labels_m,
                    rect: [-400.0, cy - row_h * 0.5, 450.0, cy + row_h * 0.5],
                    target: HitTarget::OptionsItem {
                        widget_index,
                        item_index: item_idx,
                        slider_x,
                    },
                });
            }
        }

        // Focused row's note under the rows (`paintOptions` in `classic/index.js`):
        // drawn in `gamefont_tiny` with `NOTE_YELLOW` (`rgba(223, 223, 32)`) either 8 units below
        // the reserved `bottom` margin (on `VIDEO OPTIONS`) or 6 units below the last row.
        if focused {
            if let Some(item) = opts.items.get(opts.selected) {
                if !item.note.is_empty() {
                    let note_y = if let Some(bottom) = bottom_opt {
                        safe_h - bottom + 8.0
                    } else {
                        top + (total_visible.min(shown) as f64) * pitch + 6.0
                    };
                    let note_m = anchor(0.5, 0.0, true, surface.height);
                    let tiny = fonts.by_name(FONT_TINY);
                    draw_str(tiny, batch, note_m, 0.0, note_y, &item.note, NOTE_YELLOW, 2);
                }
            }
        }

        // Scrollbar when rows exceed `shown` (`paintOptions` in `classic/index.js`):
        // `VIDEO OPTIONS` places its scrollbar at safe-area anchor `[0.5, 380.0]`.
        if let Some(sc_at) = scroll_at {
            if total_visible > shown {
                let sc_m = anchor(sc_at[0], 0.0, true, surface.height);
                let sc_x = sc_at[1];
                let sc_top = top - pitch * 0.5;
                let sc_h = (shown as f64) * pitch;
                draw_scroll_bar(batch, surface, sc_m, sc_x, sc_top, sc_h, first, shown, total_visible);
            }
        }
    }

    /// Draws a standalone [`OptionRowWidget`] with a left-aligned label at `row.x`, an optional
    /// horizontal slider bar, and a right-aligned value string at `row.x + row.width`.
    pub fn paint_option_row(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        row: &OptionRowWidget,
        focused: bool,
        pulse: f64,
    ) {
        if row.hidden {
            return;
        }
        let m = anchor(row.at[0], row.at[1], row.safe, surface.height);
        let font = fonts.by_name(row.font);
        let color = if row.disabled {
            GREY
        } else if focused {
            selected_color(pulse)
        } else {
            WHITE
        };
        let ty = row.y - font.caps_middle();
        draw_str(font, batch, m, row.x, ty, &row.label, color, 0);
        // Position the standalone slider bar 56 units left of the right-aligned value readout.
        if row.kind == OptionKind::Slider {
            let sx = row.x + row.width - row.slider_width - 56.0;
            draw_slider_bar(
                batch,
                surface,
                m,
                sx,
                row.y - row.height * 0.5,
                row.slider_width,
                row.height,
                row.ratio,
                focused,
                pulse,
            );
        }
        draw_str(
            font,
            batch,
            m,
            row.x + row.width,
            ty,
            &row.value_text,
            color,
            1,
        );
    }

    /// Paints a single [`MenuScreen`] onto `batch` without allocating.
    /// When `!is_top` (a modal popup is over this screen), `Rows`, `Options`, `List`, and `Tabs`
    /// are hidden (`UNDER_POPUP` in `src/menus/themes/classic/index.js`).
    pub fn paint_screen(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        screen: &MenuScreen,
        is_top: bool,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        // Option sub-pages (`dim_behind == true`) halve the brightness of the backdrop or 3D world
        // behind them with a full-screen `DIM = rgba(0, 0, 0, 128)` quad (`screen.dim` in `classic/index.js`).
        if screen.dim_behind {
            let full_m = anchor(0.0, 0.0, false, surface.height);
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, surface.height],
                surface.solid,
                UV,
                DIM,
                0.0,
                0,
            );
        }
        // Freeze the pulse at `1.0` and suppress focus highlights on screens underneath a
        // modal popup so only the active popup's buttons pulse.
        let pulse = if is_top {
            menu_pulse(screen.elapsed)
        } else {
            1.0
        };
        let focused_idx = if is_top { screen.focused_index() } else { None };

        // When a modal popup sits above this screen (`!is_top`), hide interactive content widgets
        // (`Rows`, `Options`, `List`, `Tabs` — matching `UNDER_POPUP` in `classic/index.js`) while
        // keeping the underlying screen's title and bottom corner buttons visible.
        for (idx, widget) in screen.widgets.iter().enumerate() {
            if !is_top
                && matches!(
                    widget,
                    Widget::Rows(_) | Widget::Options(_) | Widget::List(_) | Widget::Tabs(_)
                )
            {
                continue;
            }
            // Only the topmost screen on the stack pulses its focused widget or records hotspots
            // (`top` in `ClassicTheme.draw` in `src/menus/themes/classic/index.js`).
            let is_focused = focused_idx == Some(idx);
            let h_ref = if is_top { hits.as_deref_mut() } else { None };
            // Dispatch each visible widget variant to its corresponding `ClassicTheme` painter
            // (`PAINTERS` table in `src/menus/themes/classic/index.js`).
            match widget {
                // Top-level angled menu row stack (`OPTIONS` menu).
                Widget::Rows(rows) => {
                    Self::paint_rows_slice(
                        batch,
                        fonts,
                        surface,
                        &rows.title,
                        &rows.rows,
                        rows.selected,
                        is_focused,
                        pulse,
                        rows.layout,
                        idx,
                        h_ref,
                    );
                }
                // Framed scrollable list box (`CONTROL OPTIONS` keybinding table).
                Widget::List(list) => {
                    Self::paint_list(
                        batch,
                        fonts,
                        surface,
                        list,
                        is_focused,
                        screen.elapsed,
                        pulse,
                        idx,
                        h_ref,
                    );
                }
                // Bottom-corner or center action button (`BACK`, `RESET TO DEFAULTS`, `AUDIO MIXER`).
                Widget::Button(btn) => {
                    Self::paint_button(batch, fonts, surface, btn, is_focused, pulse, idx, h_ref);
                }
                // Screen title, heading, popup question, or static note.
                Widget::Text(txt) => {
                    Self::paint_text(batch, fonts, surface, txt);
                }
                // 9-slice `border_3_pieces` window frame.
                Widget::Panel(panel) => {
                    Self::paint_panel(batch, fonts, surface, panel);
                }
                // Centered horizontal popup answer buttons (`YES` / `NO` / `OK`).
                Widget::HorizontalButtons(hb) => {
                    Self::paint_horizontal_buttons(
                        batch, fonts, surface, hb, is_focused, pulse, idx, h_ref,
                    );
                }
                // 3-piece orange header-bar tabs (`INFANTRY` / `VEHICLE` / `STARFIGHTER`).
                Widget::Tabs(tabs) => {
                    Self::paint_tabs(batch, fonts, surface, tabs, is_focused, pulse, idx, h_ref);
                }
                // Two-column `label | value` option rows (`GAME`, `VIDEO`, `AUDIO`, `CONTROLS`).
                Widget::Options(opts) => {
                    Self::paint_options(
                        batch,
                        fonts,
                        surface,
                        opts,
                        is_focused,
                        screen.elapsed,
                        pulse,
                        idx,
                        h_ref,
                    );
                }
                // Standalone single option row.
                Widget::OptionRow(row) => {
                    Self::paint_option_row(batch, fonts, surface, row, is_focused, pulse);
                }
            }
        }
    }

    /// Paints an entire [`ScreenStack`] over `backdrop_texture` (`0` uses fallback dark background),
    /// performing zero heap allocations per frame when `hits` is `None`.
    ///
    /// Mirrors `ClassicTheme.draw` in `src/menus/themes/classic/index.js`: clears `hits`,
    /// fills the full `1024 x H` plane with the backdrop, and paints `stack.visible_screens()`
    /// in order while recording interactive hotspots exclusively for the topmost screen.
    pub fn paint_stack(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        stack: &ScreenStack,
        backdrop_texture: u32,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        // Reset the quad batch and clear the caller's hit-region buffer at the start of the frame.
        batch.begin();
        if let Some(h) = hits.as_deref_mut() {
            h.clear();
        }
        // Fill the `1024 x H` plane with `backdrop_texture` (e.g., `coreship_screen`) or a dark
        // navy fallback quad when no backdrop texture is loaded, then paint all visible screens
        // from back to front and collect `HitRegion`s only for the topmost active screen.
        let full_m = anchor(0.0, 0.0, false, surface.height);
        if backdrop_texture != 0 {
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, surface.height],
                backdrop_texture,
                UV,
                WHITE,
                0.0,
                0,
            );
        } else {
            // Fallback dark slate-blue background (`[0.04, 0.06, 0.10, 1.0]`) when `backdrop_texture == 0`.
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, surface.height],
                surface.solid,
                UV,
                [0.04, 0.06, 0.10, 1.0],
                0.0,
                0,
            );
        }

        // Draw `stack.visible_screens()` from the last non-modal screen up through any modal
        // overlays above it (`ScreenStack.visible` in `src/menus/stack.js`).
        let visible = stack.visible_screens();
        let len = visible.len();
        for (i, screen) in visible.iter().enumerate() {
            let is_top = i + 1 == len;
            let h_ref = if is_top { hits.as_deref_mut() } else { None };
            Self::paint_screen(batch, fonts, surface, screen, is_top, h_ref);
        }
    }
}
