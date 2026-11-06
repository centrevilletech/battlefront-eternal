use super::*;
impl HudModel {
    // drawCapture: the current objective is a game fact. Rust chooses the
    // contested blink and slices both texture and geometry at the same fill.
    // The post the soldier stands in, if his team doesn't hold it: the timer
    // fills from the bottom in the capturing team's colour, over grey (or the
    // other team's colour, every other half second, while contested).
    // Seated he captures nothing (src/game.js updateBattle), and the
    // original game shows the timer only for what captures.
    pub(super) fn capture(&mut self) {
        let index = self.input[H::CAPTURE_POST] as i32;
        if index < 0 || index as usize >= (self.input[H::POSTS] as usize).min(MAX_POSTS) || self.input[H::MOUNT] != 0. {
            return;
        }
        let o = POSTS + index as usize * P::WORDS;
        let team = self.input[o + P::TEAM];
        if team == self.input[H::PLAYER_TEAM] {
            return;
        }
        let owned = team != 0.;
        let mut p = if owned { self.input[o + P::NEUTRALIZE] } else { 1. - self.input[o + P::PROGRESS] };
        let bottom = if owned {
            relation(self.input[o + P::RELATION])
        } else if self.input[o + P::TOWARD] != 0. {
            relation(self.input[o + P::TOWARD_RELATION])
        } else {
            GRAY
        };
        let mut top = GRAY;
        if !owned && self.input[o + P::CONTESTED] != 0. && (self.time * 2.).floor() % 2. != 0. {
            let start = self.input[o + P::COUNTS] as usize;
            let n = (self.input[o + P::COUNTS_LENGTH] as usize).min(MAX_TEAMS);
            for i in 0..n {
                let r = start + i * PC::WORDS;
                if self.input[r + PC::TEAM] != self.input[o + P::TOWARD] && self.input[r + PC::COUNT] > 0. {
                    if self.input[r + PC::TEAM] != 0. {
                        top = relation(self.input[r + PC::RELATION]);
                    }
                    break;
                }
            }
        }
        p = clamp(p);
        let m = translate(self.at(1., 0.5, true), 0., -30., 0.);
        if p > 0. {
            self.quad(m, [-64., 0., 0., 64. * p], CAPTURE, [0., 0., 1., p], top);
        }
        if p < 1. {
            self.quad(m, [-64., 64. * p, 0., 64.], CAPTURE, [0., p, 1., 1.], bottom);
        }
    }
    // minimapRange and mapPoint keep world metres separate from interface
    // units. The speed-dependent range is measured from the centre to rim.
    // minimapRange has a slow-speed plateau through 10 m/s and reaches its far
    // range at 100 m/s. It reads ground speed of the controllable, not vertical
    // speed or the camera's motion; flying and seated HUD inputs use the same rule.
    fn map_range(&self) -> f64 {
        125. + 125. * clamp((self.input[H::SPEED] - 10.) / 90.)
    }
    fn map_point(&self, x: f64, z: f64, size: f64) -> (f64, f64) {
        (
            ((x - self.input[H::MAP_X]) / self.input[H::MAP_HALF]) * (size / 2.),
            ((z - self.input[H::MAP_Z]) / self.input[H::MAP_HALF]) * (size / 2.),
        )
    }
    // A post's map icon: its class's MapTexture (a destructible objective's
    // hud_dest_icon), sized by its MapScale against the capture posts' 2.5,
    // else the flag. A destroyed objective is off the maps.
    // postIcon prefers an authored objective texture. Its MapScale is measured
    // against a capture post's scale of 2.5. Ordinary flags keep the map's common
    // post size, so the fallback must not inherit a destructible object's scale.
    fn post_art(&self, o: usize, size: f64) -> (u32, f64) {
        let t = self.texture(self.input[o + P::TEXTURE] as u32);
        if t == 0 || t == FLAG {
            (FLAG, size)
        } else {
            (t, size * self.input[o + P::SCALE] / 2.5)
        }
    }
    // A post in its owner's relation colour (as `team` sees it), blinking
    // toward the colour of a team taking it: every other quarter second, or
    // on the spawn screen the first quarter of each second, as the original's
    // spawn map does.
    fn post_color(&self, o: usize, spawn: bool) -> Color {
        let base = relation(self.input[o + P::RELATION]);
        let mut taking = None;
        if self.input[o + P::TEAM] != 0. && self.input[o + P::NEUTRALIZE] > 0. {
            let start = self.input[o + P::COUNTS] as usize;
            let n = (self.input[o + P::COUNTS_LENGTH] as usize).min(MAX_TEAMS);
            for i in 0..n {
                let r = start + i * PC::WORDS;
                if self.input[r + PC::TEAM] != self.input[o + P::TEAM] {
                    if self.input[r + PC::TEAM] != 0. {
                        taking = Some(self.input[r + PC::RELATION]);
                    }
                    break;
                }
            }
        } else if self.input[o + P::TEAM] == 0. && self.input[o + P::PROGRESS] > 0. && self.input[o + P::TOWARD] != 0. {
            taking = Some(self.input[o + P::TOWARD_RELATION]);
        }
        let Some(taking) = taking else {
            return base;
        };
        let lit = if spawn { self.time % 1. < 0.25 } else { (self.time * 4.).floor() % 2. != 0. };
        if lit {
            relation(taking)
        } else {
            base
        }
    }
    // drawMapMarker/mapIconRotation: authored directional icons point up;
    // battlefield positive Z points down, so north-up is PI minus yaw.
    fn marker(&mut self, m: Matrix, x: f64, y: f64, size: f64, t: u32, color: Color, yaw: f64, turn: f64) {
        let face = rotate_z(translate(m, x, y, 0.), PI - yaw + turn);
        self.quad(face, [-size / 2., -size / 2., size / 2., size / 2.], t, UV, color);
    }
    // drawMap and drawMapPostDetails deliberately differ at the tiny centre
    // radius: detail placement divides by at least .001 before clamping, while
    // the base pin only scales points outside the rim. Preserve both operations
    // because labels and flashing flag overlays are independent draws.
    fn mini_point(&self, x: f64, z: f64, detail: bool) -> (f64, f64) {
        let turn = PI - self.input[H::PLAYER_YAW];
        let (cos, sin) = (turn.cos(), turn.sin());
        let dx = x - self.input[H::PLAYER_X];
        let dz = z - self.input[H::PLAYER_Z];
        let k = 71.68 / self.map_range();
        let (mut x, mut y) = ((cos * dx + sin * dz) * k, (-sin * dx + cos * dz) * k);
        let limit = 71.68 - 6. * self.input[H::MAP_SCALE] / 2.;
        let d = x.hypot(y);
        if detail {
            let scale = (limit / d.max(0.001)).min(1.);
            x *= scale;
            y *= scale;
        } else if d > limit {
            x *= limit / d;
            y *= limit / d;
        }
        (x, y)
    }

    // drawMap: posts are pinned to the circular rim, units are clipped by
    // their centre in world range. The player's arrow is always drawn last.
    // Facing (sin yaw, cos yaw) on the map's x and z; turned to point up.
    // The whole battlefield, fixed, in the middle of the screen.
    // The map's texture coordinates at the minimap's corners: the soldier at
    // the centre, mapRange to the rim, turned.
    // The posts, pinned inside the rim.
    pub(super) fn map(&mut self) {
        if self.input[H::MAP] == 0. || self.input[H::PLAYER] == 0. || self.input[H::MAP_IMAGE] == 0. {
            return;
        }
        if self.expanded {
            let m = self.at(0.5, 0.5, false);
            self.write_map_draw(m, 600., true, false, true, 0);
            self.battlefield();
            return;
        }
        let m = translate(self.at(0., 1., true), 71.68, -71.68, 0.);
        let turn = PI - self.input[H::PLAYER_YAW];
        let (cos, sin) = (turn.cos(), turn.sin());
        let half = self.input[H::MAP_HALF];
        let cu = 0.5 + ((self.input[H::PLAYER_X] - self.input[H::MAP_X]) * 0.5) / half;
        let cv = 0.5 + ((self.input[H::PLAYER_Z] - self.input[H::MAP_Z]) * 0.5) / half;
        let r = (self.map_range() * 0.5) / half;
        let mut corners = [[0.; 2]; 4];
        for (i, (s, t)) in [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.)].into_iter().enumerate() {
            corners[i] = [cu + (cos * s - sin * t) * r, cv + (sin * s + cos * t) * r];
        }
        // drawMap keeps the player at the centre of the art's UV window and rotates
        // the map beneath the fixed round mask. The mask uses the full quad, while
        // its art corners are sampled from the world-to-map rectangle in source order.
        self.batch.corners(
            m,
            [-71.68, -71.68, 71.68, 71.68],
            self.input[H::MAP_IMAGE] as u32,
            corners,
            alpha(WHITE, 128. / 255.),
            0.,
            MASK,
        );
        let size = 6. * self.input[H::MAP_SCALE];
        let posts = (self.input[H::POSTS] as usize).min(MAX_POSTS);
        for i in 0..posts {
            let o = POSTS + i * P::WORDS;
            if self.input[o + P::MAPPED] == 0. || self.input[o + P::ACTIVE] == 0. {
                continue;
            }
            let (x, y) = self.mini_point(self.input[o + P::X], self.input[o + P::Z], false);
            let (t, size) = self.post_art(o, size);
            self.quad(
                m,
                [x - size / 2., y - size / 2., x + size / 2., y + size / 2.],
                t,
                UV,
                relation(self.input[o + P::RELATION]),
            );
        }
        // The draw order matters: base posts first, then unit icons, then blinking
        // post overlays and labels, with the player's own arrow on top. A unit's
        // centre outside range is omitted rather than pinned onto the post-icon rim.
        self.map_units(m, 143.36, true, -turn);
        self.post_details(m, 143.36, true, true, false);
        self.quad(m, [-8., -8., 8., 8.], PLAYER, UV, WHITE);
    }
    // drawMapUnits uses eligibility from update, with current presentation
    // positions from the draw block. It never reruns radar discovery here.
    // HUD eligibility runs before bodies are posed; sample the latest drawn
    // frame here so both map views track the same smooth pose as the world.
    fn map_units(&mut self, m: Matrix, size: f64, mini: bool, turn: f64) {
        for i in 0..self.radar_icons.len() {
            let icon = self.radar_icons[i];
            let o = UNITS + icon.row * U::WORDS;
            let x = self.input[o + U::VISUAL_X];
            let z = self.input[o + U::VISUAL_Z];
            let (x, y) = if mini {
                let dx = x - self.input[H::PLAYER_X];
                let dz = z - self.input[H::PLAYER_Z];
                let range = self.map_range();
                if dx * dx + dz * dz > range * range {
                    continue;
                }
                let a = PI - self.input[H::PLAYER_YAW];
                let (c, s) = (a.cos(), a.sin());
                {
                    let k = 71.68 / range;
                    ((c * dx + s * dz) * k, (-s * dx + c * dz) * k)
                }
            } else {
                self.map_point(x, z, size)
            };
            let yaw = self.heading(o, self.input[o + U::ID] as usize);
            self.marker(m, x, y, icon.size, icon.texture, icon.color, yaw, turn);
        }
    }
    // drawMapPostDetails deliberately draws flashing flags before labels,
    // after the unit markers, even when the base post art was an objective.
    // team: whose view (the player's by default); labels: the posts' names;
    // spawn: the spawn screen's blink.
    fn post_details(&mut self, m: Matrix, size: f64, mini: bool, labels: bool, spawn: bool) {
        let icon = 6. * self.input[H::MAP_SCALE];
        let n = (self.input[H::POSTS] as usize).min(MAX_POSTS);
        for i in 0..n {
            let o = POSTS + i * P::WORDS;
            if self.input[o + P::MAPPED] == 0. || self.input[o + P::ACTIVE] == 0. {
                continue;
            }
            let color = self.post_color(o, spawn);
            if color == relation(self.input[o + P::RELATION]) {
                continue;
            }
            let (x, y) = if mini {
                self.mini_point(self.input[o + P::X], self.input[o + P::Z], true)
            } else {
                self.map_point(self.input[o + P::X], self.input[o + P::Z], size)
            };
            self.quad(m, [x - icon / 2., y - icon / 2., x + icon / 2., y + icon / 2.], FLAG, UV, color);
        }
        if labels {
            for i in 0..n {
                let o = POSTS + i * P::WORDS;
                if self.input[o + P::MAPPED] == 0. || self.input[o + P::ACTIVE] == 0. {
                    continue;
                }
                let id = self.input[o + P::LABEL] as usize;
                if id == 0 {
                    continue;
                }
                let (x, y) = if mini {
                    self.mini_point(self.input[o + P::X], self.input[o + P::Z], true)
                } else {
                    self.map_point(self.input[o + P::X], self.input[o + P::Z], size)
                };
                self.text(m, x, y + icon / 2., id, 0, self.post_color(o, spawn), 2, 0.5, 0.);
            }
        }
    }
    // drawBattlefield is shared by the expanded map and the spawn screen's
    // own map surface. Its matrix and ring facts are supplied by that surface.
    // The whole battlefield, north up, `size` units across and centred under
    // m: the map at half alpha, the posts in relation colours (blinking toward
    // a team taking them), the vehicles, and the radar's icons, as `team`
    // sees them (the player's team by default). The M key's map adds the
    // posts' names and the player's arrow ({ position, turn }). The spawn
    // screen's draws through a mask, with rings ({ x, z, size, alpha }: a
    // world point, the icon's size, and its highlight) around the post
    // picked.
    pub(super) fn battlefield(&mut self) {
        if self.input[H::MAP] == 0. || self.input[H::MAP_IMAGE] == 0. {
            return;
        }
        let mut m = [0.; 16];
        m.copy_from_slice(&self.input[MAP_DRAW + M::MATRIX..MAP_DRAW + M::MATRIX + 16]);
        let size = self.input[MAP_DRAW + M::SIZE];
        let half = size / 2.;
        let mask = self.input[MAP_DRAW + M::MASK] as u32;
        self.batch.quad(
            m,
            [-half, -half, half, half],
            self.input[H::MAP_IMAGE] as u32,
            UV,
            alpha(WHITE, 128. / 255.),
            0.,
            mask,
        );
        let n = (self.input[H::POSTS] as usize).min(MAX_POSTS);
        let icon = 6. * self.input[H::MAP_SCALE];
        for i in 0..n {
            let o = POSTS + i * P::WORDS;
            if self.input[o + P::MAPPED] == 0. || self.input[o + P::ACTIVE] == 0. {
                continue;
            }
            let (x, y) = self.map_point(self.input[o + P::X], self.input[o + P::Z], size);
            let (t, size) = self.post_art(o, icon);
            self.quad(
                m,
                [x - size / 2., y - size / 2., x + size / 2., y + size / 2.],
                t,
                UV,
                relation(self.input[o + P::RELATION]),
            );
        }
        self.map_units(m, size, false, 0.);
        self.post_details(
            m,
            size,
            false,
            self.input[MAP_DRAW + M::LABELS] != 0.,
            self.input[MAP_DRAW + M::SPAWN] != 0.,
        );
        // drawBattlefield draws selection rings after icons and labels. A ring's
        // highlight grows its radius before the authored .86 scale is applied.
        // These rings belong to the shell's selection facts, not radar discovery.
        let rings = (self.input[MAP_DRAW + M::RINGS] as usize).min(128);
        for i in 0..rings {
            let o = RINGS + i * R::WORDS;
            let alpha = self.input[o + R::ALPHA];
            if alpha <= 0. {
                continue;
            }
            let (x, y) = self.map_point(self.input[o + R::X], self.input[o + R::Z], size);
            let size = self.input[o + R::SIZE];
            self.ring(m, x, y, 0.86 * (size + 3. * alpha), crate::interface::alpha(GREEN, alpha));
        }
        if self.input[MAP_DRAW + M::PLAYER] != 0. {
            let (x, y) = self.map_point(self.input[MAP_DRAW + M::PLAYER_X], self.input[MAP_DRAW + M::PLAYER_Z], size);
            let face = rotate_z(translate(m, x, y, 0.), self.input[MAP_DRAW + M::PLAYER_TURN]);
            self.quad(face, [-8., -8., 8., 8.], PLAYER, UV, WHITE);
        }
    }
    fn write_map_draw(&mut self, m: Matrix, size: f64, labels: bool, spawn: bool, player: bool, mask: u32) {
        self.input[MAP_DRAW + M::MATRIX..MAP_DRAW + M::MATRIX + 16].copy_from_slice(&m);
        self.input[MAP_DRAW + M::SIZE] = size;
        self.input[MAP_DRAW + M::LABELS] = labels as u8 as f64;
        self.input[MAP_DRAW + M::SPAWN] = spawn as u8 as f64;
        self.input[MAP_DRAW + M::PLAYER] = player as u8 as f64;
        self.input[MAP_DRAW + M::PLAYER_X] = self.input[H::PLAYER_X];
        self.input[MAP_DRAW + M::PLAYER_Z] = self.input[H::PLAYER_Z];
        self.input[MAP_DRAW + M::PLAYER_TURN] = PI - self.input[H::PLAYER_YAW];
        self.input[MAP_DRAW + M::MASK] = mask as f64;
        self.input[MAP_DRAW + M::RINGS] = 0.;
    }
    // The spawn screen's map (view: the layer's battlefield).
    pub(super) fn spawn_map(&mut self) {
        let size = 328. * (1024. / 800.);
        let m = translate(
            self.at(0., 0., false),
            (0.05 + 0.9) * 1024. - 128. * (1024. / 800.),
            self.input[H::HEIGHT] / 2.,
            0.,
        );
        let mut saved = [0.; M::WORDS];
        saved.copy_from_slice(&self.input[MAP_DRAW..MAP_DRAW + M::WORDS]);
        self.write_map_draw(m, size, false, true, false, MASK);
        self.input[MAP_DRAW + M::RINGS] = saved[M::RINGS];
        self.battlefield();
        self.input[MAP_DRAW..MAP_DRAW + M::WORDS].copy_from_slice(&saved);
    }
    // drawRing preserves the extra half-unit overlap between segments so a
    // selected post's thin ring has no cracks at translucent join edges.
    // A thin circle of `radius` units around (x, y) under m.
    fn ring(&mut self, m: Matrix, x: f64, y: f64, r: f64, color: Color) {
        for i in 0..32 {
            let a0 = i as f64 / 32. * 2. * PI;
            let a1 = (i + 1) as f64 / 32. * 2. * PI;
            let (ax, ay) = (x + r * a0.cos(), y + r * a0.sin());
            let (bx, by) = (x + r * a1.cos(), y + r * a1.sin());
            let at = rotate_z(translate(m, ax, ay, 0.), (by - ay).atan2(bx - ax));
            self.quad(at, [0., -0.75, (bx - ax).hypot(by - ay) + 0.5, 0.75], SOLID, UV, color);
        }
    }
}
