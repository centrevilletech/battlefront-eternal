use super::*;
impl HudModel {
    // drawWeapons: preserve panel order, then names, and wakes after the
    // current fade was sampled. Ammo number changes affect the next frame.
    // The rounds, pulsing green while a droid tops them up.
    // A weapon with no IconTexture (the TIE fighter's and the Jedi
    // starfighter's cannons) leaves its panel without one.
    // The bar: in the 2004 game's weapon state 4, a launcher's lock under
    // way or a held charge (the arc caster's, the mortar's), its timer
    // against its limit in green; else a reload's progress, or
    // what heat leaves.
    // Rounds: clip / reserve; one total for single-round clips or a
    // reserve of a clip or less; the clip alone with an endless reserve.
    // Each panel's name, for 2 s after its weapon changes, over its shadow
    // (taking a seat changes both). Seated, the seat's name in its place
    // (its VehiclePosition: PILOT, MAIN GUNNER...), or none for a seat
    // with none, as the original game labels a seat's panels.
    pub(super) fn weapons(&mut self) {
        if self.input[H::ARSENAL] == 0. {
            return;
        }
        let group = rotate_y(translate(self.at(0., 0., true), 0., 21., 40.), PI / 6.);
        // drawWeapons samples the group fade once. A channel can wake that fader
        // while choosing its progress bar or discovering new ammo numbers, but both
        // channels and their names keep the same sampled alpha for this draw.
        let fade = self.weapon_fader.alpha;
        let tint = alpha(WHITE, fade);
        let count = if self.input[H::SUPPLY_AMMO] != 0. {
            alpha(mix(WHITE, GREEN, 0.75 * self.supply_pulse()), fade)
        } else {
            tint
        };
        let mut groups = [IDENTITY; 2];
        for ch in 0..2 {
            let o = WEAPONS + ch * W::WORDS;
            if self.input[o + W::ID] == 0. {
                continue;
            }
            let m = if ch == 0 { group } else { translate(group, 27., 16., 0.) };
            groups[ch] = m;
            let (pw, puv, bx, bw, bh, buv, tx) = if ch == 0 {
                (
                    118.,
                    [0.976562, 0.171875, 0.0551181, 0.429688],
                    30.,
                    84.,
                    13.,
                    [0.828125, 0.0390625, 0.171875, 0.140625],
                    66.,
                )
            } else {
                (
                    84.,
                    [0.945312, 0.445312, 0.289062, 0.703125],
                    32.,
                    48.,
                    12.,
                    [0.679688, 0.757812, 0.304688, 0.851562],
                    48.,
                )
            };
            self.quad(m, [0., 0., pw, 33.], HUD1, puv, tint);
            // The original icon call uses (24,24)-(2,2), including its reversed
            // UVs; correcting that unusual rectangle would change the image.
            self.quad(
                translate(m, 6., 3., 0.),
                [24., 24., 2., 2.],
                self.input[o + W::ICON] as u32,
                [1., 1., 0., 0.],
                tint,
            );
            // drawWeapons gives acquisition priority over charge, charge over reload,
            // and reload over heat. Every active progress state wakes the panel, even
            // when an empty or temporarily negative fill produces no visible bar.
            let mut fill = -1.;
            let mut color = GREEN;
            let heats = self.input[o + W::BOTTOMLESS] != 0.;
            if self.input[o + W::LOCK] != 0. && self.input[o + W::LOCK_TARGET] >= 0. {
                let t = self.input[o + W::LOCK_TIME];
                fill = if t > 0. { (self.input[o + W::LOCK_ELAPSED] / t).min(1.) } else { 1. };
            } else if self.input[o + W::CHARGING] != 0. {
                fill = (self.input[o + W::HELD] / self.input[o + W::MAX_PRESSED].max(1e-3)).min(1.);
            } else if self.input[o + W::RELOADING] != 0. {
                fill = 1. - self.input[o + W::RELOAD_LEFT] / self.input[o + W::RELOAD_TIME].max(1e-3);
            } else if heats && self.input[o + W::HEAT_PER_SHOT] > 0. {
                fill = 1. - self.input[o + W::HEAT].min(1.);
                color = if self.input[o + W::OVERHEATED] != 0. { RED } else { BAR };
            }
            if self.input[o + W::LOCK] != 0. && self.input[o + W::LOCK_TARGET] >= 0.
                || self.input[o + W::CHARGING] != 0.
                || self.input[o + W::RELOADING] != 0.
                || heats && self.input[o + W::HEAT_PER_SHOT] > 0.
            {
                self.weapon_fader.wake();
            }
            if fill > 0. {
                self.quad(
                    m,
                    [bx, 2., bx + bw * fill, 2. + bh],
                    HUD1,
                    [buv[0], buv[1], buv[0] + (buv[2] - buv[0]) * fill, buv[3]],
                    alpha(color, fade),
                );
            }
            if heats {
                continue;
            }
            // Ammo uses three layouts: an endless reserve shows only the clip; small
            // clips or a clip-or-less reserve show the total; larger reserves show a
            // separately aligned clip, slash and reserve. The combined label exists only
            // to detect changes, while those three pieces keep their original alignment.
            let at = translate(m, tx, -3., 0.);
            let clip = self.input[o + W::LOADED];
            let reserve = self.input[o + W::SPARE];
            let mut shown = Label::default();
            if reserve == f64::INFINITY {
                let _ = write!(shown, "{clip:.0}");
                self.label(at, 18., 0., &shown, 0, count, 1, 1.);
            } else if self.input[o + W::ROUNDS_PER_CLIP] < 2. || self.input[o + W::RESERVE] <= 1. {
                let total = clip + reserve;
                let _ = write!(shown, "{total:.0}");
                self.label(at, 0., 0., &shown, 0, count, 0, 1.);
            } else {
                let _ = write!(shown, "{clip:.0}/{reserve:.0}");
                self.number(at, 0., 0., clip, 0, count, 1, 1.);
                self.fonts[0].draw(&mut self.batch, at, 3., 0., &[47], count, 2, 1., 1., 0.);
                self.number(at, 6., 0., reserve, 0, count, 0, 1.);
            }
            if shown.text() != self.numbers[ch].text() {
                self.numbers[ch] = shown;
                self.weapon_fader.wake();
            }
        }
        // drawWeapons places names after both channels' panels. Changing a weapon
        // starts its two-second name timer, including when a channel disappears.
        // Seat names replace weapon labels, but do not change the identity used to
        // restart that timer. The shadow is a separate glyph pass three units behind.
        for ch in 0..2 {
            let o = WEAPONS + ch * W::WORDS;
            let id = self.input[o + W::ID];
            if id != self.named[ch] {
                if self.named[ch] != 0. || id != 0. {
                    self.name_left[ch] = 2.;
                }
                self.named[ch] = id;
                self.weapon_fader.wake();
            }
            if id == 0. || self.name_left[ch] <= 0. {
                continue;
            }
            let k = self.name_left[ch] / 2. * fade;
            let label = if self.input[H::VEHICLE] != 0. { self.input[H::SEAT_NAME] } else { self.input[o + W::LABEL] };
            let at = translate(groups[ch], 123., 1., 0.);
            self.text(at, 0., 0., label as usize, 0, alpha(SHADOW, k), 0, 1., 3.);
            self.text(at, 0., 0., label as usize, 0, alpha(WHITE, k), 0, 1., 0.);
        }
    }
    fn supply_pulse(&self) -> f64 {
        0.5 - 0.5 * (self.time * 3. * PI).cos()
    }
    fn piece(&mut self, m: Matrix, x: f64, y: f64, w: f64, h: f64, t: u32, uv: [f64; 4], c: Color) {
        self.quad(m, [x - w, y, x, y + h], t, uv, c);
    }

    // drawHealth: own health is above the hull/secondary and enemy bars.
    // A hull's seat pips precede the panels, preserving translucent overlap.
    // The health group, in the original game's order: seated, the hull's seat
    // pips; his own bar; the second panel (what he sits in, or on foot what
    // he has besides health); the enemy panel.
    // Right-aligned: each piece ends at its x.
    // A pip per seat of the hull, the first five: its slot, and
    // over a held seat the fill, white for him, green for anyone else with
    // a Character (a bot included), yellow for anyone without.
    // The second panel, cut from its left end like the first: what he sits
    // in, blinking and flashing as his own bar does; on foot, what he has
    // besides health.
    // The enemy his aim is on, fading for 2 s once it is off:
    // hud2's panel, its bar cut from the left, and its HealthTexture, all
    // in the HUD's red, where the hexagons interlock under the rest.
    pub(super) fn health(&mut self) {
        if self.input[H::ARSENAL] == 0. {
            return;
        }
        let m = rotate_y(translate(self.at(1., 0., true), 0., 25., 40.), -PI / 6.);
        let fade = self.health_fader.alpha;
        let tint = alpha(WHITE, fade);
        // drawHealth reserves at most five seat pips. A negative sentinel ends the
        // seat list, zero draws an empty slot, and a held seat adds its fill over it.
        // The player is white, another Character green, and an unowned body yellow.
        for i in 0..5 {
            let seat = self.input[H::SEATS + i];
            if seat < 0. {
                break;
            }
            let x = -89. + 7. * i as f64;
            self.piece(m, x, 35., 11., 10., HUD2, [0.015625, 0.351562, 0.101562, 0.429688], tint);
            if seat > 0. {
                self.piece(
                    m,
                    x,
                    35.,
                    11.,
                    10.,
                    HUD2,
                    [0.015625, 0.5, 0.101562, 0.578125],
                    alpha(
                        match seat as u32 {
                            1 => WHITE,
                            2 => GREEN,
                            _ => YELLOW,
                        },
                        fade,
                    ),
                );
            }
        }
        self.piece(m, 0., 0., 118., 33., HUD1, [0.0546875, 0.171875, 0.984252, 0.429688], tint);
        // drawHealth cuts the bar from its left end, including the UV endpoint.
        // Low health shortens the blink period quadratically; supply green is mixed
        // before a damage flash mixes toward white. Reordering those mixes changes
        // colours when healing and damage feedback overlap.
        let f = clamp(self.input[H::HEALTH]);
        let mut color = BAR;
        if f <= 0.5 {
            color = mix(BAR, LOW, blink(self.time, 4. * f * f + 0.25));
        }
        if self.input[H::SUPPLY_HEALTH] != 0. {
            color = mix(color, GREEN, 0.75 * self.supply_pulse());
        }
        color = mix(color, WHITE, self.health_flash);
        if f > 0. {
            self.piece(
                m,
                -30.,
                2.,
                84. * f,
                13.,
                HUD1,
                [0.828125 + (0.171875 - 0.828125) * f, 0.0390625, 0.828125, 0.140625],
                alpha(color, fade),
            );
        }
        let icon = self.input[H::HEALTH_ICON] as u32;
        self.piece(m, -6., 4., 24., 24., if self.texture(icon) != 0 { icon } else { HEALTH }, UV, tint);
        let hull = self.input[H::HULL_ICON] != 0.;
        let second = if hull { self.input[H::HULL_ICON] } else { self.input[H::SECOND_ICON] };
        if second != 0. {
            self.piece(m, -27., 16., 84., 33., HUD1, [0.289062, 0.445312, 0.953125, 0.710938], tint);
            let f = clamp(self.input[if hull { H::HULL_FILL } else { H::SECOND_FILL }]);
            if f > 0. {
                let look = if hull { 3 } else { self.input[H::SECOND_LOOK] as u32 };
                let mut color = BAR;
                if look == 1 {
                    color = LOW;
                } else if look == 2 || (look == 3 && f <= 0.5) {
                    color = mix(BAR, LOW, blink(self.time, 4. * f * f + 0.25));
                }
                if hull {
                    color = mix(color, WHITE, self.hull_flash);
                }
                self.piece(
                    m,
                    -59.,
                    18.,
                    48. * f,
                    12.,
                    HUD1,
                    [0.679688 + (0.304688 - 0.679688) * f, 0.757812, 0.679688, 0.851562],
                    alpha(color, fade),
                );
            }
            let icon = second as u32;
            self.piece(
                m,
                -35.,
                20.,
                24.,
                24.,
                if self.texture(icon) != 0 {
                    icon
                } else if hull {
                    HULL
                } else {
                    0
                },
                UV,
                tint,
            );
        }
        // enemyHealth holds the last target for two seconds after aim leaves it.
        // The bar continues to fade with the health group. A null/dead target command
        // does not clear it early; only a live enemy observation replaces its fill.
        if self.enemy_left > 0. {
            let red = alpha(RED, fade * self.enemy_left / 2.);
            self.piece(m, 0., 32., 96., 33., HUD2, [0.132812, 0.359375, 0.889764, 0.617188], red);
            let f = self.enemy_fill;
            if f > 0. {
                self.piece(
                    m,
                    -31.,
                    49.,
                    62. * f,
                    13.,
                    HUD2,
                    [0.734375 - (0.734375 - 0.25) * f, 0.765625, 0.734375, 0.867188],
                    red,
                );
            }
            self.piece(m, -6., 36., 24., 24., self.enemy_icon, UV, red);
        }
    }

    // drawTeams: layer team overrides are facts from its own view. Each new
    // countdown second flares before the large shrinking number is drawn.
    // view: a layer's { own, foe, flip }, counted from its team and moved left
    // (the spawn screen's), or null for the player's.
    // The countdown for a team that holds no posts: the last 20 s large and
    // growing, under VICTORY IN or DEFEAT IN; each new second flares.
    pub(super) fn teams(&mut self) {
        if self.input[H::BATTLE] == 0. {
            return;
        }
        let mut m = translate(self.at(0.5, 0., true), 0., -3., 0.);
        if self.input[H::VIEW_TEAMS] != 0. {
            m = rotate_y(translate(m, -10., 0., 0.), self.input[H::FLIP]);
        }
        m = scale(m, 1.7);
        self.quad(m, [-59., 0., 59., 19.], HUD2, [0.0390625, 0.0390625, 0.96484375, 0.1875], alpha(WHITE, 204. / 255.));
        for side in 0..2 {
            let (n, start, rate, base) = if side == 0 {
                (H::OWN, H::OWN_START, H::OWN_BLEED, GREEN)
            } else {
                (H::FOE, H::FOE_START, H::FOE_BLEED, RED)
            };
            let count = self.input[n].ceil().max(0.);
            let s = if self.input[start] != 0. {
                self.input[start]
            } else if count != 0. {
                count
            } else {
                1.
            };
            let f = clamp(count / s);
            let rate = self.input[rate];
            let k = if rate > 0. { blink(self.time, 2. / rate) } else { 0. };
            let color = alpha(mix(base, WHITE, k), 204. / 255.);
            if f > 0. {
                if side == 0 {
                    self.quad(
                        m,
                        [-1. - 55. * f, 1., -1., 5.],
                        HUD2,
                        [0.078125, 0.6875, 0.078125 + 0.4140625 * f, 0.703125],
                        color,
                    );
                } else {
                    self.quad(
                        m,
                        [1., 1., 1. + 55. * f, 5.],
                        HUD2,
                        [0.921875 - 0.4140625 * f, 0.6875, 0.921875, 0.703125],
                        color,
                    );
                }
            }
            self.number(m, if side == 0 { -4. } else { 4. }, 3., count, 0, color, if side == 0 { 1 } else { 0 }, 0.6);
        }
        let countdown = self.input[H::COUNTDOWN];
        if countdown.is_nan() {
            self.countdown_second = f64::NAN;
            return;
        }
        // drawTeams uses floor for the no-post countdown, but ceil for tickets.
        // Crossing a whole second resets its flare before text is emitted. The final
        // 20 seconds scale the large bitmap digits independently of their group.
        let seconds = countdown.floor().max(0.);
        if seconds != self.countdown_second {
            self.countdown_second = seconds;
            self.countdown_alpha = 1.;
        }
        let m = translate(m, 0., 30., 0.);
        let color = alpha(if self.input[H::LOSING] != 0. { RED } else { GREEN }, self.countdown_alpha);
        if seconds <= 20. {
            self.text(
                m,
                0.,
                0.,
                self.input[if self.input[H::LOSING] != 0. { H::DEFEAT } else { H::VICTORY }] as usize,
                0,
                color,
                2,
                1.,
                0.,
            );
            self.number(m, 0., 15., seconds, 3, color, 2, 2. - seconds / 20.);
        } else if seconds < 60. {
            self.number(m, 0., 0., seconds, 2, color, 2, 1.);
        } else {
            let mut text = Label::default();
            let _ = write!(text, "{}:{:02}", seconds as u32 / 60, seconds as u32 % 60);
            self.label(m, 0., 0., &text, 0, color, 2, 1.);
        }
    }
    // A craft's pilot's prompt (flightPrompt), from the craft he holds now,
    // as the original game lays out its tool tips (ToolTipsDisplay, which the
    // PC game never shows): white gamefont_small at 80%, centred across the
    // screen in the band from 80% to 95% of its height, fading in over half a
    // second; the key bound to the flight action first, as the controls
    // screen names it (keyName(action), set by the game).
    pub(super) fn flight_prompt(&mut self) {
        let id = self.prompt_line() as usize;
        if id == 0 {
            return;
        }
        let age = if self.prompt == id as f64 { self.prompt_age } else { 0. };
        self.text(
            self.at(0.5, 0.875, false),
            0.,
            -self.fonts[1].caps_middle(),
            id,
            1,
            alpha(WHITE, 0.8 * (age / 0.5).min(1.)),
            2,
            1.,
            0.,
        );
    }

    // drawMessages: newest at the bottom, with a fixed five-line ring and
    // scrolling held separately from expiry and the squad panel's offset.
    // Keep the command list legible while both bottom-right overlays live.
    pub(super) fn messages(&mut self) {
        let m = translate(self.at(1., 0.9, true), 0., if self.command_left > 0. { -76. } else { 0. }, 0.);
        for i in 0..self.message_count {
            let line = &self.messages[i];
            let text = if line.text == 0 { &line.line[..line.len] } else { self.texts.get(line.text).map_or(&[][..], |s| s.as_slice()) };
            let y = -self.fonts[0].height - (self.message_count - 1 - i) as f64 * 20. + self.scroll;
            self.fonts[0].draw(
                &mut self.batch,
                m,
                0.,
                y,
                text,
                alpha(line.color, ((10. - line.age) / 0.5).min(1.)),
                1,
                1.,
                1.,
                0.,
            );
        }
    }
    pub(super) fn damage(&mut self) {
        let places = [(1., 0.5, -PI / 2.), (0.5, 1., 0.), (0., 0.5, PI / 2.), (0.5, 0., PI)];
        for (i, (x, y, a)) in places.into_iter().enumerate() {
            if self.damage[i] > 0. {
                self.quad(
                    rotate_z(self.at(x, y, true), a),
                    [-64., -16., 64., 0.],
                    DAMAGE,
                    UV,
                    alpha(WHITE, self.damage[i]),
                );
            }
        }
    }
    // A quarter of the overlay per screen quarter, its (1, 1) corner at the
    // centre.
    pub(super) fn scope(&mut self) {
        let t = self.input[H::SCOPE] as u32;
        if self.texture(t) == 0 {
            return;
        }
        let m = self.at(0.5, 0.5, false);
        let h = self.input[H::HEIGHT] / 2.;
        for (w, h) in [(-512., -h), (512., -h), (-512., h), (512., h)] {
            self.quad(m, [w, h, 0., 0.], t, UV, WHITE);
        }
    }
    // Outside the battlefield: core.lvl's LEAVING BATTLEFIELD, centred on the
    // screen in gamefont_large, flashing in the HUD's red.
    pub(super) fn leaving(&mut self) {
        let a = self.input[H::LEAVING];
        if a > 0. {
            self.text(
                self.at(0.5, 0.5, true),
                0.,
                -self.fonts[3].height / 2.,
                self.input[H::LEAVING_TEXT] as usize,
                3,
                alpha(RED, a.min(1.)),
                2,
                1.,
                0.,
            );
        }
    }
    // The stat popup (the original game's StatIconDisplay): a row each for
    // KILLS, DEATHS and CP POINTS, centred on its line. A changed row's icon
    // and number turn yellow if it rose, red if it fell, hold for half a
    // second and blend back over the next. Gone once the game is over.
    // drawStats is a temporary popup even though its rest tint is faint white.
    // A changed row holds yellow/red for half its flash, then blends to that rest
    // tint. Another row changing must not restart every existing flash timer.
    pub(super) fn stats(&mut self) {
        if !self.has_stats || self.stats_left <= 0. || self.input[H::GAME_OVER] != 0. {
            return;
        }
        let m = self.at(0., 0.5, true);
        let fade = self.stats_left.min(1.);
        let fh = self.fonts[1].height * 0.75;
        for (i, icon) in STATS.into_iter().enumerate() {
            let y = i as f64 * 32.;
            let flash = self.stat_flash[i];
            let lit = if self.stat_changes[i] > 0. { YELLOW } else { RED };
            let rest = [1., 1., 1., 64. / 255.];
            let color = alpha(if flash >= 0.5 { lit } else { mix(rest, lit, 2. * flash) }, fade);
            if self.texture(icon) != 0 {
                self.quad(m, [9., y - 12., 33., y + 12.], icon, UV, color);
            } else {
                self.fonts[1].draw(
                    &mut self.batch,
                    m,
                    33.,
                    y - fh / 2.,
                    match i {
                        0 => &[75],
                        1 => &[68],
                        _ => &[67, 80],
                    },
                    color,
                    1,
                    0.75,
                    1.,
                    0.,
                );
            }
            self.number(m, 63., y - fh / 2., self.stats[i], 1, color, 1, 0.75);
        }
    }
    pub(super) fn squad(&mut self) {
        if self.command_left <= 0. {
            return;
        }
        let m = self.at(1., 1., true);
        for (i, s) in ["MOVE OUT", "HOLD POSITION", "FOLLOW ME", "AT EASE"].into_iter().enumerate() {
            let mut text = Label::default();
            let _ = text.write_str(s);
            self.label(
                m,
                0.,
                -80. + i as f64 * 18.,
                &text,
                0,
                alpha(WHITE, self.command_left / 2. * if i == self.command as usize { 1. } else { 0.5 }),
                1,
                1.,
            );
        }
    }

    // drawLine/drawWeaponReticle: absent art on a seat draws spread ticks;
    // an authored reticle adds eight lock ticks around its aim anchor.
    // A line of quads from (ax, ay) to (bx, by) under m, one unit wide.
    pub(super) fn line(&mut self, m: Matrix, ax: f64, ay: f64, bx: f64, by: f64, color: Color) {
        let length = (bx - ax).hypot(by - ay);
        let at = rotate_z(translate(m, ax, ay, 0.), (by - ay).atan2(bx - ax));
        self.quad(at, [0., -0.5, length, 0.5], SOLID, UV, color);
    }
    // A channel's reticle colour: green on a friend, red on an enemy or the
    // locals, blue on nothing.
    fn reticle_color(&self, ch: usize) -> Color {
        match self.input[if ch == 0 { H::TARGET_RELATION } else { H::SECOND_RELATION }] as u32 {
            1 => GREEN,
            2 | 3 => RED,
            _ => RETICLE,
        }
    }
    fn radii(&self, angle: f64) -> (f64, f64) {
        let vfov = self.input[H::FOV] * PI / 180.;
        let hfov = 2. * ((vfov / 2.).tan() * self.input[H::ASPECT]).atan();
        let angle = angle.max(0.) * PI / 180.;
        (1024. * angle / hfov, self.input[H::HEIGHT] * angle / (hfov * 0.75))
    }
    // A seat's weapon with no reticle art: four ticks, 5 units long, as
    // far out as the weapon's spread reaches now; no lock cursor.
    // LockOffAngle draws eight radial ticks at the AIM anchor,
    // even without a target. It is not a circle attached to the target.
    // drawReticle can consume two seat channels but emits a shared weapon once.
    // Each distinct weapon has its own cooldown alpha and relationship colour.
    // No-art spread ticks are a seat-specific display; a soldier without reticle
    // art gets no substitute graphic.
    pub(super) fn reticle(&mut self) {
        if self.input[H::RETICLE] == 0. {
            return;
        }
        let at = self.at(self.input[H::RETICLE_X], self.input[H::RETICLE_Y], false);
        for ch in 0..2 {
            let o = WEAPONS + (ch + 2) * W::WORDS;
            let id = self.input[o + W::ID];
            if id == 0. || ch == 1 && id == self.input[WEAPONS + 2 * W::WORDS + W::ID] {
                continue;
            }
            // Dim between shots (63 rising to 127), reloading or overheated (63).
            let a = if self.input[o + W::RELOADING] != 0. || self.input[o + W::OVERHEATED] != 0. {
                63. / 255.
            } else if self.input[o + W::WAIT] > 0. && self.input[o + W::SHOT_DELAY] > 0. {
                (63. + 64. * (1. - self.input[o + W::WAIT] / self.input[o + W::SHOT_DELAY])) / 255.
            } else {
                1.
            };
            let color = alpha(self.reticle_color(ch), a);
            let t = self.input[o + W::RETICLE] as u32;
            let asset = self.asset(t);
            if t == 0 || !asset.ready {
                if self.input[H::VEHICLE] == 0. {
                    continue;
                }
                let rx = self
                    .radii(
                        self.input[o + W::KICK].max(0.)
                            + self.input[o + W::STILL_SPREAD]
                            + self.input[o + W::YAW_SPREAD],
                    )
                    .0;
                let ry = self
                    .radii(
                        self.input[o + W::KICK].max(0.)
                            + self.input[o + W::STILL_SPREAD]
                            + self.input[o + W::PITCH_SPREAD],
                    )
                    .1;
                self.line(at, rx, 0., rx + 5., 0., color);
                self.line(at, -rx, 0., -rx - 5., 0., color);
                self.line(at, 0., ry, 0., ry + 5., color);
                self.line(at, 0., -ry, 0., -ry - 5., color);
                continue;
            }
            let (w, h) = (
                if asset.width != 0. { asset.width } else { 64. },
                if asset.height != 0. { asset.height } else { 64. },
            );
            self.quad(at, [-w / 2., -h / 2., w / 2., h / 2.], t, UV, color);
            let angle = self.input[o + W::LOCK_OFF_ANGLE];
            if angle <= 0. {
                continue;
            }
            let (rx, ry) = self.radii(angle);
            for i in 0..8 {
                let angle = i as f64 * PI / 4.;
                let (x, y) = (angle.cos(), angle.sin());
                self.line(at, rx * x, ry * y, (rx + 5.) * x, (ry + 5.) * y, color);
            }
        }
    }
    // Only the launcher's acquisition state draws the target diamond, over
    // the reticles and the hit ring, in its channel's colour.
    // drawLockDiamond belongs to the selected acquiring weapon, after the hit
    // ring. Its size shrinks linearly with progress, but its alpha grows with
    // progress squared and is floored to a byte before normalization. An authored
    // reticle and LockOffAngle are both required, just as for the original cursor.
    pub(super) fn lock_diamond(&mut self) {
        let ch = self.input[H::LOCK_CHANNEL] as usize;
        let o = WEAPONS + (ch.min(1) + 2) * W::WORDS;
        if self.input[H::RETICLE] == 0.
            || self.input[H::MARKER] == 0.
            || self.input[o + W::LOCK_OFF_ANGLE] <= 0.
            || !self.asset(self.input[o + W::RETICLE] as u32).ready
        {
            return;
        }
        let p = clamp(self.input[H::MARKER_PROGRESS]);
        let size = 32. - 24. * p;
        let at = self.at(self.input[H::MARKER_X], self.input[H::MARKER_Y], false);
        let tint = alpha(self.reticle_color(ch), (15. + 240. * p * p).floor() / 255.);
        self.line(at, 0., -size, size, 0., tint);
        self.line(at, size, 0., 0., size, tint);
        self.line(at, 0., size, -size, 0., tint);
        self.line(at, -size, 0., 0., -size, tint);
    }
    // HitRing.color normalizes all three relationship channels by their maximum.
    // Repeated hits add duration rather than replacing it. Locals count as enemy
    // hits for the ring, even though their radar markers remain yellow.
    pub(super) fn hit_ring(&mut self) {
        if self.input[H::RETICLE] == 0. {
            return;
        }
        let e = clamp(10. * self.hit[0]);
        let f = clamp(10. * self.hit[1]);
        let n = clamp(10. * self.hit[2]);
        let max = e.max(f).max(n);
        if max == 0. {
            return;
        }
        let color = [
            (32. + 192. * e / max) / 255.,
            (32. + 192. * f / max) / 255.,
            (32. + 192. * n / max) / 255.,
            128. * max / 255.,
        ];
        self.quad(
            self.at(self.input[H::RETICLE_X], self.input[H::RETICLE_Y], false),
            [-32., -32., 32., 32.],
            HIT,
            UV,
            color,
        );
    }
}
