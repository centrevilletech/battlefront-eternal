//! Instant Action dual-list screen and planetary zoom-in loading screen rendering
//! (`src/shell/screens.js`, `src/shell/loading.js`).
//!
//! Mirrors:
//! - `instantAction` in `src/shell/screens.js`, `gamePicker` in `src/shell/index.js`,
//!   and `paintList` / `paintHorizontal` / `paintButton` in
//!   `src/menus/themes/classic/index.js`: two angled 9-slice listboxes
//!   (`x = +-227`, `rotY = -+35 deg`, `w = 442`, `h = min(398, height - 210)`,
//!   `pitch = 26`, `inset = 15`), era faction icons (`rep_icon`/`cis_icon` or
//!   `imp_icon`/`all_icon`), lower-right map preview still (`preview_rect`),
//!   centered `ADD`/`REMOVE` buttons at `y = 220`, and bottom `BACK`,
//!   `PLAY IN ORDER`/`RANDOM ORDER`, and `LAUNCH` buttons.
//! - `LoadingView.prototype.draw` (`drawPictures` and `drawDisk`) in
//!   `src/shell/loading.js`: 4-stage planetary zoom-in pictures (`zoomView`,
//!   `onScreen`), cyan tracking crosshair lines (`load_edge_horz`,
//!   `load_edge_vert`, or `TRACK_LINE_COLOR`), top-left planet name (`NAME`),
//!   and lower-right rotating `LOADING` disk (`diskPlace`, `DISK.turn = 0.866`,
//!   two counter-rotating `load_rotating_disk` layers, 28-LED ring at
//!   `DISK.ring = 52`, and `-14 deg` tilted `LOADING` text).

use super::{
    cursor_pulse, draw_bottom_button, draw_frame, draw_scroll_bar, draw_str, piece_quad,
    selected_color, str_width, tint_color, ShellFonts, ShellTextures, BLACK, GREY,
    TRACK_LINE_COLOR, WHITE,
};
use crate::{
    loading::{
        self, disk, disk_place, light_level, mix_color_rgb, name_pos, on_screen, zoom_view, LookId,
        TimelineAt,
    },
    media::preview_rect,
    screens::{IaFocus, ShellApp},
};
use bfe_render::interface::{
    anchor, mul, rotate_y, rotate_z, translate, Batch, Matrix, IDENTITY, UV,
};

/// Draws the Instant Action dual-list playlist screen:
/// - Top `INSTANT ACTION` title (`gamefont_large`, centered at `y = 10` of safe area),
/// - Left column heading `AVAILABLE MAPS:` + active game picker label
///   (`BATTLEFRONT (2004)`) at `at = [0.25, 0.0]` and right column heading
///   `SELECTED MAPS:` at `at = [0.75, 0.0]`,
/// - Left angled listbox (`x = -227`, `rotY = +35 deg`) of `app.data.maps`,
/// - Right angled listbox (`x = +227`, `rotY = -35 deg`) of `app.ia_entries`
///   with era faction icons (`rep_icon`/`cis_icon` or `imp_icon`/`all_icon`),
/// - Lower-right map preview still (`preview_rect`),
/// - Center `ADD` / `REMOVE` buttons (`y = 220`) and bottom `BACK`,
///   `PLAY IN ORDER` / `RANDOM ORDER`, and `LAUNCH` buttons.
///
/// Mirrors `instantAction` in `src/shell/screens.js`, `gamePicker` in
/// `src/shell/index.js`, and `paintList` in `src/menus/themes/classic/index.js`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_instant_action(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    app: &ShellApp,
    viewport_w: f64,
    viewport_h: f64,
    height: f64,
    pulse: f64,
) {
    // Top title: `INSTANT ACTION` centered in `gamefont_large` 10 units below
    // the top of the safe area (`paintText` look `"title"`).
    let top_m = anchor(0.5, 0.0, true, height);
    draw_str(
        &fonts.large,
        batch,
        top_m,
        0.0,
        10.0,
        "INSTANT ACTION",
        WHITE,
        2,
    );

    // Column headings: AVAILABLE MAPS: + game picker (`BATTLEFRONT (2004)`
    // selected, `BATTLEFRONT II (2005), PLANNED` disabled) and SELECTED MAPS:
    let avail_hdr = anchor(0.25, 0.0, true, height);
    draw_str(
        &fonts.small,
        batch,
        avail_hdr,
        0.0,
        36.0,
        "AVAILABLE MAPS:",
        WHITE,
        2,
    );
    draw_str(
        &fonts.tiny,
        batch,
        avail_hdr,
        0.0,
        54.0,
        app.ia_selected_game_label(),
        WHITE,
        2,
    );
    // Right column heading: `SELECTED MAPS:` centered at `at = [0.75, 0.0]`,
    // `y = 45` (`gamefont_small`).
    let sel_hdr = anchor(0.75, 0.0, true, height);
    draw_str(
        &fonts.small,
        batch,
        sel_hdr,
        0.0,
        45.0,
        "SELECTED MAPS:",
        WHITE,
        2,
    );

    // Listbox dimensions from `look(side)` in `instantAction()` (`src/shell/screens.js`)
    // and `paintList` in `src/menus/themes/classic/index.js`:
    // `box: { x: 0, y: -10, w: 442, h: Math.min(398, surface.height - 210) }`,
    // `font: "gamefont_tiny"`, `pitch: 26`, and 15-unit top/bottom/side inset.
    let box_w = 442.0;
    let box_h = (height - 210.0).clamp(160.0, 398.0);
    let box_y = -10.0;
    let pitch = 26.0;
    let inset = 15.0;
    // Number of rows that fit inside the listbox between top and bottom insets.
    let shown = (((box_h - 2.0 * inset) / pitch).floor() as usize).max(1);

    // Left list: AVAILABLE MAPS (side = -1, x = -227, rotY = 35)
    {
        // Transform from the safe-area center `(0.5, 0.5)` by `x = -227` and
        // `rotY = +35 deg` (`look(-1)` in `instantAction()`).
        let base = anchor(0.5, 0.5, true, height);
        let m = rotate_y(translate(base, -227.0, 0.0, 0.0), 35.0_f64.to_radians());
        // Outer 9-slice window frame (`border_3_pieces`) for `AVAILABLE MAPS`.
        draw_frame(
            batch,
            textures,
            m,
            0.0,
            box_y,
            box_w,
            box_h,
            "border_3_pieces",
            WHITE,
        );
        // Compute visible window `[first .. first + shown)` and whether the
        // 16-unit vertical scroll bar (`slider_bg` / `slider_fg`) is needed.
        let count = app.data.maps.len();
        let scroll = count > shown;
        // Keep the selected available map within the visible scroll window.
        let first = if count > shown && app.ia_available_selected >= shown {
            (app.ia_available_selected + 1 - shown).min(count - shown)
        } else {
            0
        };
        // Inner content area leaves 15 units on each side and reserves 26 units
        // on the right when the scroll bar is visible (`paintList`).
        let top = box_y - box_h * 0.5 + inset;
        let left = -box_w * 0.5 + 15.0;
        let inner = box_w - 30.0 - if scroll { 26.0 } else { 0.0 };
        // Iterate over the visible rows `[first .. first + shown)` in the left box.
        let focused = app.ia_focus == IaFocus::Available;
        for k in 0..shown {
            let idx = first + k;
            let Some(map) = app.data.maps.get(idx) else {
                break;
            };
            // Row vertical center `cy` inside the `AVAILABLE MAPS` listbox.
            let cy = top + (k as f64) * pitch + pitch * 0.5;
            // Highlighted row frame (`listbox_cursor`), pulsing `1.0 -> 0.5 -> 1.0`
            // once per second when `Available` has focus, or solid `1.0` when unfocused.
            if idx == app.ia_available_selected {
                let a = if focused {
                    cursor_pulse(app.pulse_elapsed)
                } else {
                    1.0
                };
                draw_frame(
                    batch,
                    textures,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    pitch + 10.0,
                    "listbox_cursor",
                    tint_color(WHITE, a),
                );
            }
            // Unextracted maps are drawn in `GREY` (`row.disabled = !map.extracted`).
            let disabled = map.extracted == Some(false);
            draw_str(
                &fonts.tiny,
                batch,
                m,
                left + 8.0,
                cy - fonts.tiny.caps_middle(),
                &map.name,
                if disabled { GREY } else { WHITE },
                0,
            );
        }
        // Draw the right-edge scroll bar at `box.x + box.w / 2 - 34`.
        if scroll {
            draw_scroll_bar(
                batch,
                textures,
                m,
                box_w * 0.5 - 34.0,
                top,
                (shown as f64) * pitch,
                first,
                shown,
                count,
            );
        }
    }

    // Right list: SELECTED MAPS (side = 1, x = 227, rotY = -35)
    {
        // Transform from the safe-area center `(0.5, 0.5)` by `x = +227` and
        // `rotY = -35 deg` (`look(1)` in `instantAction()`).
        let base = anchor(0.5, 0.5, true, height);
        let m = rotate_y(translate(base, 227.0, 0.0, 0.0), (-35.0_f64).to_radians());
        // Outer 9-slice window frame (`border_3_pieces`) for `SELECTED MAPS`.
        draw_frame(
            batch,
            textures,
            m,
            0.0,
            box_y,
            box_w,
            box_h,
            "border_3_pieces",
            WHITE,
        );
        // Scroll window `[first .. first + shown)` for the playlist entries.
        let count = app.ia_entries.len();
        let scroll = count > shown;
        // Keep the selected playlist entry within the visible scroll window.
        let first = if count > shown && app.ia_selected_cursor >= shown {
            (app.ia_selected_cursor + 1 - shown).min(count - shown)
        } else {
            0
        };
        // Compute inner row bounds for the right listbox, reserving 26 units
        // on the right when the playlist overflows `shown` rows.
        let top = box_y - box_h * 0.5 + inset;
        let left = -box_w * 0.5 + 15.0;
        let inner = box_w - 30.0 - if scroll { 26.0 } else { 0.0 };
        let focused = app.ia_focus == IaFocus::Selected;
        for k in 0..shown {
            let idx = first + k;
            let Some(entry) = app.ia_entries.get(idx) else {
                break;
            };
            // Row vertical center `cy` inside the `SELECTED MAPS` listbox.
            let cy = top + (k as f64) * pitch + pitch * 0.5;
            // Selection frame (`listbox_cursor`) for the active playlist entry.
            if idx == app.ia_selected_cursor {
                let a = if focused {
                    cursor_pulse(app.pulse_elapsed)
                } else {
                    1.0
                };
                draw_frame(
                    batch,
                    textures,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    pitch + 10.0,
                    "listbox_cursor",
                    tint_color(WHITE, a),
                );
            }
            // Resolve the display name from `data.maps`, falling back to the raw map id.
            let map_name = app
                .data
                .maps
                .iter()
                .find(|m| m.id == entry.map)
                .map(|m| m.name.as_str())
                .unwrap_or(&entry.map);
            draw_str(
                &fonts.tiny,
                batch,
                m,
                left + 8.0,
                cy - fonts.tiny.caps_middle(),
                map_name,
                WHITE,
                0,
            );
            // Era faction icons (`rep_icon` + `cis_icon` for Clone Wars,
            // `imp_icon` + `all_icon` for Galactic Civil War) aligned to the
            // right edge of the row (`size = 0.65 * pitch = 16.9`, 3 units apart).
            let icons = if entry.era.as_deref() == Some("cw") {
                ["rep_icon", "cis_icon"]
            } else {
                ["imp_icon", "all_icon"]
            };
            // Each faction icon is a `16.9 x 16.9` quad (`0.65 * pitch`),
            // spaced 3 units apart from the right edge of `inner`.
            let size = 16.9;
            for (icon_i, icon_name) in icons.iter().enumerate() {
                let right = left + inner - ((icons.len() - icon_i - 1) as f64) * (size + 3.0);
                piece_quad(
                    batch,
                    textures,
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
        // Draw the right listbox scroll bar when `ia_entries.len() > shown`.
        if scroll {
            draw_scroll_bar(
                batch,
                textures,
                m,
                box_w * 0.5 - 34.0,
                top,
                (shown as f64) * pitch,
                first,
                shown,
                count,
            );
        }
    }

    // Lower-right map preview still (`preview_rect` in `src/shell/media.js`),
    // framed by a `border_3_pieces` 9-slice border extended by 6 units on each side.
    if let Some(map) = app.data.maps.get(app.ia_available_selected) {
        let preview_tex = textures.map_preview_for(&map.id);
        if preview_tex != 0 {
            // Convert `preview_rect` pixel coordinates into `1024 x height`
            // interface units and surround the image with a `border_3_pieces` frame.
            let [px, py, pw, ph] = preview_rect(viewport_w, viewport_h);
            let sx = 1024.0 / viewport_w.max(1.0);
            let sy = height / viewport_h.max(1.0);
            let m = anchor(0.0, 0.0, false, height);
            let x0 = px * sx;
            let y0 = py * sy;
            let x1 = (px + pw) * sx;
            let y1 = (py + ph) * sy;
            draw_frame(
                batch,
                textures,
                m,
                (x0 + x1) * 0.5,
                (y0 + y1) * 0.5,
                (x1 - x0) + 12.0,
                (y1 - y0) + 12.0,
                "border_3_pieces",
                WHITE,
            );
            // Draw the map preview still inside the 9-slice border frame.
            batch.quad(m, [x0, y0, x1, y1], preview_tex, UV, WHITE, 0.0, 0);
        }
    }

    // Horizontal buttons: `ADD` (`x = -40`) and `REMOVE` (`x = 45`) at `y = 220`
    // below screen center (`paintHorizontal` in `src/menus/themes/classic/index.js`).
    let mid_m = anchor(0.5, 0.5, false, height);
    // Highlight `ADD` or `REMOVE` with `selected_color(pulse)` when focused,
    // matching `paintHorizontal` in `src/menus/themes/classic/index.js`.
    let y_edit = 220.0 - fonts.medium.caps_middle();
    let add_color = if app.ia_focus == IaFocus::Add {
        selected_color(pulse)
    } else {
        WHITE
    };
    let rem_color = if app.ia_focus == IaFocus::Remove {
        selected_color(pulse)
    } else {
        WHITE
    };
    draw_str(
        &fonts.medium,
        batch,
        mid_m,
        -40.0,
        y_edit,
        "ADD",
        add_color,
        2,
    );
    draw_str(
        &fonts.medium,
        batch,
        mid_m,
        45.0,
        y_edit,
        "REMOVE",
        rem_color,
        2,
    );

    // Bottom buttons (`paintButton` in `src/menus/themes/classic/index.js`):
    // - `BACK` at bottom-left (`place = "back"`),
    // - `PLAY IN ORDER` / `RANDOM ORDER` at bottom-center (`place = "middle"`),
    // - `LAUNCH` at bottom-right (`place = "accept"`, disabled when `ia_entries` is empty).
    draw_bottom_button(
        batch,
        fonts,
        height,
        "back",
        app.say("common.back", "BACK"),
        app.ia_focus == IaFocus::Back,
        true,
        pulse,
    );
    // Middle toggle button switches between `PLAY IN ORDER` and `RANDOM ORDER`.
    let order_label = if app.ia_random {
        "RANDOM ORDER"
    } else {
        "PLAY IN ORDER"
    };
    draw_bottom_button(
        batch,
        fonts,
        height,
        "middle",
        order_label,
        app.ia_focus == IaFocus::Order,
        true,
        pulse,
    );
    draw_bottom_button(
        batch,
        fonts,
        height,
        "accept",
        "LAUNCH",
        app.ia_focus == IaFocus::Launch,
        !app.ia_entries.is_empty(),
        pulse,
    );
}

/// Populates `batch` with the loading screen's stage picture, zoom crop,
/// tracking crosshairs, top-left planet name, rotating `LOADING` disk, and
/// 28-LED ring (`src/shell/loading.js` `LoadingView.prototype.draw`):
/// 1. Full-screen black clear quad (`BLACK`) and active stage picture (`0..4`),
///    cropped by [`zoom_view`] during `Zoom`, with the next stage picture fading
///    in inside [`on_screen`] and cyan tracking lines riding the rectangle edges.
/// 2. Top-left planet name (`gamefont_tiny` at `NAME.x = 51.2`,
///    `top = NAME.top * height + NAME.y`), hidden during the final `Hold` phase.
/// 3. Lower-right `LOADING` disk at [`disk_place`], horizontally squashed by
///    `DISK.turn = 0.866`, with two counter-rotating `load_rotating_disk`
///    layers, 28 chasing LEDs at `DISK.ring = 52`, and `-14 deg` tilted
///    `LOADING` text (`gamefont_medium`).
///
/// Performs zero heap allocations per frame.
#[allow(clippy::too_many_arguments)]
pub fn draw_loading(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    state: &TimelineAt,
    spin: f64,
    planet: &str,
    loading_text: &str,
    look: LookId,
    viewport_w: f64,
    viewport_h: f64,
) {
    draw_loading_with_typeface(
        batch,
        fonts,
        textures,
        state,
        spin,
        planet,
        loading_text,
        look,
        viewport_w,
        viewport_h,
        None,
    );
}

/// Loading screen using the Page look's admitted Saira fonts. BF1 retains retail type.
#[allow(clippy::too_many_arguments)]
pub fn draw_loading_with_typeface(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    state: &TimelineAt,
    spin: f64,
    planet: &str,
    loading_text: &str,
    look: LookId,
    viewport_w: f64,
    viewport_h: f64,
    page_fonts: Option<(&bfe_render::interface::Font, &bfe_render::interface::Font)>,
) {
    // Convert viewport pixel dimensions into the `1024 x height` interface space.
    batch.begin();
    let w = viewport_w.max(1.0);
    let h = viewport_h.max(1.0);
    let height = 768.0 + (h / w - 0.75) * 768.0;
    let full_m = anchor(0.0, 0.0, false, height);
    let sx = 1024.0 / w;
    let sy = height / h;

    // 1. Black clear quad + stage picture (zooming if in `Zoom` phase) +
    //    next stage picture growing out of its target rectangle + tracking lines.
    batch.quad(
        full_m,
        [0.0, 0.0, 1024.0, height],
        textures.solid,
        UV,
        BLACK,
        0.0,
        0,
    );

    // Resolve the current stage texture (`state.stage`) and the next stage
    // texture (`state.stage + 1`) that grows out of the zoom target box.
    let shown = textures.loading_stages.get(state.stage).copied();
    let next = textures.loading_stages.get(state.stage + 1).copied();

    // Draw the current stage picture cropped to `zoom_view` during the `Zoom`
    // phase, or full-frame `[0, 0, iw, ih]` during `Track` / `Hold`.
    if let Some(stage_pic) = shown.filter(|s| s.texture != 0) {
        let iw = stage_pic.width.max(1.0);
        let ih = stage_pic.height.max(1.0);
        let view = match (state.zoom, stage_pic.target) {
            (Some(u), Some(target)) => zoom_view(u, iw, ih, target),
            _ => [0.0, 0.0, iw, ih],
        };
        // Normalize the pixel crop rectangle `view = [vx, vy, vw, vh]` into
        // `[u0, v0, u1, v1]` texture coordinates.
        let uv = [
            view[0] / iw,
            view[1] / ih,
            (view[0] + view[2]) / iw,
            (view[1] + view[3]) / ih,
        ];
        batch.quad(
            full_m,
            [0.0, 0.0, 1024.0, height],
            stage_pic.texture,
            uv,
            WHITE,
            0.0,
            0,
        );

        // When the stage has a zoom target rectangle and active tracking lines:
        // project the target rectangle onto the screen (`on_screen`), fade in the
        // next stage picture inside that rectangle over the first 20% of the zoom
        // (`alpha = clamp(u / 0.2, 0, 1)`), and draw the horizontal and vertical
        // tracking lines.
        if let (Some(target), Some(lines)) = (stage_pic.target, state.lines) {
            let [x, y, rw, rh] = on_screen(target, view, w, h);
            if let (Some(u), Some(next_pic)) = (state.zoom, next.filter(|s| s.texture != 0)) {
                let a = (u / 0.2).clamp(0.0, 1.0);
                batch.quad(
                    full_m,
                    [x * sx, y * sy, (x + rw) * sx, (y + rh) * sy],
                    next_pic.texture,
                    UV,
                    tint_color(WHITE, a),
                    0.0,
                    0,
                );
            }

            // Tracking lines: 2 interface units thick (`src/shell/loading.js`).
            // Horizontal lines (`y_top`, `y_bot`) sweep from the top/bottom screen
            // edges onto the target rectangle's top/bottom edges (`lines.y: 0 -> 1`).
            let line_w = (2.0 * w / loading::IW).max(1.5);
            let y_top = y * lines.y;
            let y_bot = h - (h - (y + rh)) * lines.y;
            draw_tracking_line(batch, textures, full_m, true, y_top, line_w, w, h, sx, sy);
            draw_tracking_line(batch, textures, full_m, true, y_bot, line_w, w, h, sx, sy);

            // Vertical tracking lines appear in the second half of `track`
            // (`lines.x == Some(lx)`).
            if let Some(lx) = lines.x {
                let x_left = x * lx;
                let x_right = w - (w - (x + rw)) * lx;
                draw_tracking_line(batch, textures, full_m, false, x_left, line_w, w, h, sx, sy);
                draw_tracking_line(
                    batch, textures, full_m, false, x_right, line_w, w, h, sx, sy,
                );
            }
        }
    } else {
        // Fallback space backdrop when stage textures have not been uploaded.
        let backdrop = textures.get("iface_bgmeta_space");
        if backdrop != 0 {
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, height],
                backdrop,
                UV,
                WHITE,
                0.0,
                0,
            );
        }
    }

    // 2. Top-left planet name (`NAME.x = 51.2`, `top = NAME.top * H + NAME.y`),
    //    shown on every stage except over the final level (`Hold`) when stage
    //    pictures are present (`src/shell/loading.js`).
    if state.phase != loading::LoadingPhase::Hold || shown.is_none() {
        let top = name_pos::TOP * height + name_pos::Y;
        if let Some(typefaces) = page_fonts.filter(|_| look != LookId::Bf1) {
            loading_type(
                batch,
                full_m,
                typefaces,
                look,
                name_pos::X,
                top + 12.,
                planet,
                false,
            );
        } else {
            draw_str(
                &fonts.tiny,
                batch,
                full_m,
                name_pos::X,
                top,
                planet,
                WHITE,
                0,
            );
        }
    }

    // 3. Lower-right rotating disk, 28-LED ring, and tilted `LOADING` text
    //    (`drawDisk` in `src/shell/loading.js`).
    // Convert the disk center (`diskPlace`: 252 units from right, 223 from bottom
    // scaled by `min(w / 1024, h / 768)`) into interface coordinates.
    let dp = disk_place(w, h);
    let cx = dp.x * sx;
    let cy = dp.y * sy;
    let disk_base = translate(full_m, cx, cy, 0.0);
    // Horizontal perspective squash `DISK.turn = 0.866` (`cos(30 deg)`),
    // matching `c.scale(DISK.turn, 1)` in `drawDisk` (`src/shell/loading.js`).
    let mut squash = IDENTITY;
    squash[0] = disk::TURN * dp.unit * sx;
    squash[5] = dp.unit * sy;
    let disk_m = mul(disk_base, squash);

    // Draw the two counter-rotating layers of `load_rotating_disk`
    // (`[[240, 0.25, 0.85], [176, -0.4, 0.75]]`), tinted per `LookId`.
    let themed = textures.get(match look {
        LookId::Bf1 => "load_rotating_disk",
        LookId::Eternal => "load_rotating_disk_eternal",
        LookId::Bf2 => "load_rotating_disk_bf2",
    });
    let disk_tex = if themed != 0 {
        themed
    } else {
        textures.get("load_rotating_disk")
    };
    let disk_tint = [
        1.,
        1.,
        1.,
        match look {
            LookId::Bf1 => 1.,
            LookId::Eternal => 0.85,
            LookId::Bf2 => 0.90,
        },
    ];
    // Each layer rotates around Z at `spin * speed` (`0.25` and `-0.4` rad/s)
    // and draws a centered `size x size` quad (`240` and `176` units).
    for &[size, speed, alpha] in &disk::LAYERS {
        let layer_m = rotate_z(disk_m, spin * speed);
        let half = size * 0.5;
        let tex = if disk_tex != 0 {
            disk_tex
        } else {
            textures.solid
        };
        let c = tint_color(disk_tint, alpha);
        batch.quad(layer_m, [-half, -half, half, half], tex, UV, c, 0.0, 0);
    }

    // 28 LEDs around the ring at radius `DISK.ring = 52`, with colour stops
    // from `LOOKS[look].lights` in `src/shell/loading.js`.
    let stops: &[&str] = match look {
        LookId::Bf1 => &["#0b0d0f", "#5c6166", "#b9bcbe"],
        LookId::Eternal => &["#15100c", "#4a2407", "#b5560b", "#ff9a1f", "#ffd27a"],
        LookId::Bf2 => &["#101826", "#3d0c08", "#a5180e", "#e0362a", "#ff6450"],
    };
    // Use `load_led_0` when extracted, or `textures.solid` as a fallback.
    let led_tex = {
        let t = textures.get("load_led_0");
        if t != 0 {
            t
        } else {
            textures.solid
        }
    };
    // `head` advances around the ring at `LIGHTS.speed = 0.6` turns per second.
    // Each of the 28 LEDs sits at `angle = (i / 28) * TAU` and radius `52`.
    let head = spin * loading::lights::SPEED;
    for i in 0..disk::LEDS {
        let angle = (i as f64 / disk::LEDS as f64) * std::f64::consts::TAU;
        let led_m = translate(rotate_z(disk_m, angle), disk::RING, 0.0, 0.0);
        // Battlefront II look adds a dark `#06080c` outer rim around each LED.
        if look == LookId::Bf2 {
            batch.quad(
                led_m,
                [-4.5, -4.5, 4.5, 4.5],
                textures.solid,
                UV,
                [6.0 / 255.0, 8.0 / 255.0, 12.0 / 255.0, 1.0],
                0.0,
                0,
            );
        }
        // Compute continuous brightness `[0.0, 1.0]` behind the chasing arc head
        // and interpolate RGB across `stops` (`lightLevel` + `mixColor`).
        let level = light_level(head - (i as f64 / disk::LEDS as f64));
        let [r, g, b] = mix_color_rgb(stops, level);
        batch.quad(
            led_m,
            [-3.5, -3.5, 3.5, 3.5],
            led_tex,
            UV,
            [r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, 1.0],
            0.0,
            0,
        );
    }

    // Tilted `LOADING` text across the center (`DISK.text_deg = -14.0`),
    // drawn in `#dcdcff` (`[220/255, 220/255, 1.0, 1.0]`) with centered alignment.
    let text_m = rotate_z(disk_m, disk::TEXT_DEG.to_radians());
    if let Some(typefaces) = page_fonts.filter(|_| look != LookId::Bf1) {
        loading_type(batch, text_m, typefaces, look, 0., 0., loading_text, true);
    } else {
        let text_w = str_width(&fonts.medium, loading_text);
        let _ = text_w;
        draw_str(
            &fonts.medium,
            batch,
            text_m,
            0.0,
            -12.0,
            loading_text,
            [220.0 / 255.0, 220.0 / 255.0, 1.0, 1.0],
            2,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn loading_type(
    batch: &mut Batch,
    m: Matrix,
    fonts: (&bfe_render::interface::Font, &bfe_render::interface::Font),
    look: LookId,
    x: f64,
    y: f64,
    text: &str,
    centered: bool,
) {
    let (font, size, spacing, color) = if look == LookId::Eternal {
        (
            fonts.0,
            if centered { 13. } else { 15. },
            0.24,
            [232. / 255., 236. / 255., 242. / 255., 1.],
        )
    } else {
        (fonts.1, if centered { 15. } else { 16. }, 0.04, WHITE)
    };
    let chars: Vec<u16> = text.encode_utf16().collect();
    let scale = size / font.height;
    let (top, bottom) = chars
        .iter()
        .filter_map(|c| font.glyphs.get(c))
        .fold((0_f64, 0_f64), |(a, z), g| {
            (a.min(g.bounds[1]), z.max(g.bounds[3]))
        });
    let base = y - (top + bottom) * scale / 2.;
    let align = if centered { 2 } else { 0 };
    let spacing = size * spacing;
    let black = [0., 0., 0., 0.85];
    if look == LookId::Bf2 {
        for (dx, dy) in [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)] {
            crate::page::Page::draw_text(
                font,
                batch,
                m,
                x + dx,
                base + dy,
                &chars,
                black,
                align,
                size,
                spacing,
            );
        }
    } else {
        crate::page::Page::draw_text(
            font,
            batch,
            m,
            x,
            base + 1.,
            &chars,
            black,
            align,
            size,
            spacing,
        );
    }
    crate::page::Page::draw_text(font, batch, m, x, base, &chars, color, align, size, spacing);
}

/// Draws one horizontal (`across == true`) or vertical (`across == false`)
/// cyan tracking crosshair line across the loading screen at pixel coordinate
/// `at`, using `load_edge_horz` / `load_edge_vert` when loaded or falling back
/// to a solid [`TRACK_LINE_COLOR`] quad.
///
/// Mirrors `drawLine` in `src/shell/loading.js`.
#[allow(clippy::too_many_arguments)]
fn draw_tracking_line(
    batch: &mut Batch,
    textures: &ShellTextures,
    m: Matrix,
    across: bool,
    at: f64,
    width: f64,
    w: f64,
    h: f64,
    sx: f64,
    sy: f64,
) {
    // Choose the horizontal (`load_edge_horz`) or vertical (`load_edge_vert`)
    // tracking line texture.
    let edge_tex = if across {
        textures.get("load_edge_horz")
    } else {
        textures.get("load_edge_vert")
    };
    // Textured edge lines span `+-4 * width` around `at`; solid fallback lines
    // span `+-0.5 * width`.
    if edge_tex != 0 {
        if across {
            // Textured horizontal tracking line spanning the full viewport width.
            batch.quad(
                m,
                [
                    0.0,
                    (at - width * 4.0) * sy,
                    w * sx,
                    (at + width * 4.0) * sy,
                ],
                edge_tex,
                UV,
                WHITE,
                0.0,
                0,
            );
        } else {
            // Textured vertical tracking line spanning the full viewport height.
            batch.quad(
                m,
                [
                    (at - width * 4.0) * sx,
                    0.0,
                    (at + width * 4.0) * sx,
                    h * sy,
                ],
                edge_tex,
                UV,
                WHITE,
                0.0,
                0,
            );
        }
    } else if across {
        // Untextured horizontal tracking line fallback (`TRACK_LINE_COLOR`).
        batch.quad(
            m,
            [
                0.0,
                (at - width * 0.5) * sy,
                w * sx,
                (at + width * 0.5) * sy,
            ],
            textures.solid,
            UV,
            TRACK_LINE_COLOR,
            0.0,
            0,
        );
    } else {
        // Untextured vertical tracking line fallback (`TRACK_LINE_COLOR`).
        batch.quad(
            m,
            [
                (at - width * 0.5) * sx,
                0.0,
                (at + width * 0.5) * sx,
                h * sy,
            ],
            textures.solid,
            UV,
            TRACK_LINE_COLOR,
            0.0,
            0,
        );
    }
}
