// Classic in-battle pause menu drawing (`ClassicTheme.draw` + `overlay.draw` in `src/menus/themes/classic/`).

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

const POINTER_SIZE: f64 = 32.0;
const BAR_X: f64 = -4.7;
const TEXT_X: f64 = 0.8;
const BUTTON_INSET: f64 = 21.0;

const HEADER_PIECES: [&str; 3] = ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"];
const BUTTON_PIECES: [&str; 3] = ["buttonleft", "buttonmid", "buttonright"];

impl PauseMenu {
    /// Draws the Classic pause menu, sub-screens, confirm popups, or `"CLICK TO RESUME"` into `batch`,
    /// recording interactive hotspots for hit testing (`ClassicTheme.draw` + `overlay.draw`).
    pub fn draw(&mut self, surface: &dyn BattleUiSurface, batch: &mut Batch) {
        self.hotspots.clear();
        let h = surface.height();
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
        } else if let Some(PauseScreenState::Options { focused_row }) = self.stack.last().cloned() {
            self.draw_options_screen(surface, batch, focused_row);
            self.draw_back_button(surface, batch, true);
        } else if let Some(PauseScreenState::PlayerList { selected, first }) =
            self.stack.last().cloned()
        {
            self.draw_player_list_screen(surface, batch, selected, first);
            self.draw_back_button(surface, batch, true);
        }

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

    fn draw_main_rows(
        &mut self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        interactive: bool,
    ) {
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

        let bars = row_bars(LAYOUT.width, p, n);
        let pulse = self.pulse();
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

    fn draw_options_screen(
        &mut self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        focused_row: usize,
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
        let title_m = anchor(0.5, 0.0, true, h);
        let title = surface.string("ifs.main.options", "OPTIONS");
        surface.draw_text(batch, title_m, 0.0, 10.0, title, FONT_NAME_LARGE, WHITE, 2, 1.0);

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
        let title_m = anchor(0.5, 0.0, true, h);
        let title = surface.string("ifs.pause.playerlist", "PLAYER LIST");
        surface.draw_text(batch, title_m, 0.0, 10.0, title, FONT_NAME_LARGE, WHITE, 2, 1.0);

        let m = anchor(0.5, 0.5, true, h);
        let box_w = 560.0;
        let box_h = 340.0;
        Self::draw_frame(surface, batch, m, 0.0, 0.0, box_w, box_h, "border_3_pieces", WHITE);

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
        let h = y1 - y0;
        let mid_end = x0 + width.max(h);
        let end = mid_end + h;
        let visible = x0 + (end - x0) * grow;
        let pieces = [
            (x0, x0 + h, names[0], false),
            (x0 + h, mid_end, names[1], false),
            (mid_end, end, names[2], true),
        ];
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
        let fh = surface.line_height(font);
        let width = surface.text_width(font, text, 1.0);
        let state = intro.map(|i| i.frame()).unwrap_or_default();
        let left = match align {
            1 => x - width,
            2 => x - width / 2.0,
            _ => x,
        };
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
