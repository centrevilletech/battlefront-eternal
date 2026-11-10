//! Planetary zoom-in loading screen rendering (`src/shell/loading.js`).
//!
//! Mirrors `LoadingView.prototype.draw` (`drawPictures` and `drawDisk`) in
//! `src/shell/loading.js`: 4-stage planetary zoom-in pictures (`zoomView`,
//! `onScreen`), cyan tracking crosshair lines (`load_edge_horz`,
//! `load_edge_vert`, or `TRACK_LINE_COLOR`), top-left planet name (`NAME`),
//! and lower-right rotating `LOADING` disk (`diskPlace`, `DISK.turn = 0.866`,
//! two counter-rotating `load_rotating_disk` layers, 28-LED ring at
//! `DISK.ring = 52`, and `-14 deg` tilted `LOADING` text).

use super::{
    draw_str, tint_color, ShellFonts, ShellTextures, BLACK, TRACK_LINE_COLOR, WHITE,
};
use crate::loading::{
    self, disk, disk_place, light_level, mix_color_rgb, name_pos, on_screen, zoom_view, LookId,
    TimelineAt,
};
use bfe_render::interface::{
    anchor, mul, rotate_z, translate, Batch, Matrix, IDENTITY, UV,
};

/// Populates `batch` with the loading screen's stage picture, zoom crop,
/// tracking crosshairs, top-left planet name (in BF1 retail look; Eternal and
/// BF2 looks render Saira typography through Slint `LoadingPageOverlay`),
/// rotating `LOADING` disk, and 28-LED ring (`src/shell/loading.js`
/// `LoadingView.prototype.draw`):
/// 1. Full-screen black clear quad (`BLACK`) and active stage picture (`0..4`),
///    cropped by [`zoom_view`] during `Zoom`, with the next stage picture fading
///    in inside [`on_screen`] and cyan tracking lines riding the rectangle edges.
/// 2. Top-left planet name (`gamefont_tiny` at `NAME.x = 51.2`,
///    `top = NAME.top * height + NAME.y` in BF1 retail look), hidden during the
///    final `Hold` phase.
/// 3. Lower-right `LOADING` disk at [`disk_place`], horizontally squashed by
///    `DISK.turn = 0.866`, with two counter-rotating `load_rotating_disk`
///    layers, 28 chasing LEDs at `DISK.ring = 52`, and `-14 deg` tilted
///    `LOADING` text (`gamefont_medium` in BF1 retail look).
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
    //    pictures are present (`src/shell/loading.js`). Eternal and BF2 looks
    //    render their Saira planet name in Slint (`LoadingPageOverlay`).
    if look == LookId::Bf1 && (state.phase != loading::LoadingPhase::Hold || shown.is_none()) {
        let top = name_pos::TOP * height + name_pos::Y;
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
    // drawn in `#dcdcff` (`[220/255, 220/255, 1.0, 1.0]`) with centered alignment
    // in BF1 retail look (Eternal and BF2 looks render Saira in `LoadingPageOverlay`).
    if look == LookId::Bf1 {
        let text_m = rotate_z(disk_m, disk::TEXT_DEG.to_radians());
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
