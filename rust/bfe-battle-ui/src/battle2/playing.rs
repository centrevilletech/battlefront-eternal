use super::paint::{point, Painter, Rows, Setting, WHITE};
use super::{
    Aim, AimRelation, BattleView, Caption, Event, Gauge, Lock, Message, Mode, Role, Slot,
    SlotKind, Target,
};
use bfe_render::hud_model2::{Widget, PLANE};
use bfe_render::interface::{rotate_z, translate, Color, UV};


pub(super) fn draw(
    p: &mut Painter<'_>,
    v: &BattleView<'_>,
    f: f32,
    takeoff_key: &str,
    memory: &mut Stats,
    flashes: &mut super::space::Flashes,
    outside: &mut Leaving,
) {
    // Bound labels attached to instruments before admitting optional lists into the frame.
    p.glyph_limit = 32;
    let body = v.body.as_ref();
    let scoped = v.vehicle.is_none() && v.aim.scope.is_some_and(|name| scope_overlay(p, name));
    let show_reticle = v.vehicle.is_some()
        || body.is_some_and(|b| {
            !b.reticle.is_empty() || !b.slots.iter().flatten().any(|s| s.kind == SlotKind::Saber)
        });
    if show_reticle && v.aim.visible {
        // The reticle follows the projected aim, which is a point of the whole window.
        p.place(CENTRE);
        let anchor = p
            .catalog
            .layout
            .named("player1reticule")
            .map_or([512., 384.], |w| w.at);
        let at = v
            .aim
            .position
            .filter(|_| !scoped)
            .map_or(anchor, |xy| p.viewport_point([f64::from(xy[0]), f64::from(xy[1])]));
        let offset = [at[0] - anchor[0], at[1] - anchor[1]];
        reticle(
            p,
            body.map_or("", |b| b.reticle),
            v.target.as_ref(),
            &v.aim,
            at,
            v.tick,
            f,
            scoped,
        );
        if v.vehicle.is_none() && !scoped {
            if let Some(body) = body {
                weapon_rings(p, &body.slots, v.tick, f, offset);
            }
        }
    }
    if let Some(target) = &v.target {
        target_info(p, target, v.team, f);
    }
    if let Some(vehicle) = v.vehicle.as_ref() {
        super::space::flight(p, vehicle, v.tick, f, takeoff_key);
    } else if let Some(body) = body {
        let awarded = body.role == Role::AwardedHero;
        let health = if awarded {
            "player1.hero.healthFraction"
        } else {
            "player1.healthFraction"
        };
        gauge(p, health, body.health, f, None);
        if awarded {
            if let Some(hilt) = p.catalog.layout.named("player1herobar_herobarsaberhilt") {
                p.place(hilt.pin);
                p.icon(hilt.rect, &hilt.bitmap, WHITE);
            }
        }
        gauge(p, "player1.energyFraction", body.stamina, f, None);
        equipment(p, &body.slots, v.tick, f);
    }
    scores(p, v);
    // Space Assault switches the small map off; its critical systems use that corner.
    if v.mode == super::Mode::SpaceAssault {
        super::space::assault(p, v, f, flashes);
    } else {
        radar(p, v, f);
    }
    timer(p, v);
    stats(p, v, f, memory);
    messages(p, v);
    leaving(p, v, f, outside);
}

/// When the "Leaving Battlefield" warning appeared, in battle seconds, while it shows.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Leaving {
    since: Option<f64>,
}

/// The warning's localized text.
pub(super) const LEAVING_KEY: &str = "game.tooltips.leaving_battlefield";
/// The 2005 game's warning red (the red of the other team).
pub(super) const WARNING_RED: [f64; 3] = [223. / 255., 32. / 255., 32. / 255.];
/// The warning's alpha, 0 to 255, is a spring: it starts at 254 at rest and is pulled toward
/// 128 with this stiffness per second squared, without damping, so it pulses between 254 and
/// 2 (its floor) about every 0.28 s.
pub(super) const LEAVING_START: f64 = 254.;
pub(super) const LEAVING_REST: f64 = 128.;
pub(super) const LEAVING_STIFFNESS: f64 = 500.;
/// The warning's size against the HUD's other texts: the game sets it at text scale 1 with no
/// space between glyphs, which draws "Leaving Battlefield" 186 pixels wide at 720 lines (the
/// game's frame); without spacing, the HUD's text size draws it 162 wide.
const LEAVING_TEXT_SCALE: f64 = 1.15;

/// "Leaving Battlefield" while what the player controls (soldier or craft) is outside the
/// battlefield, as the 2005 game shows it: the localized tooltip in the large game font, red,
/// centred on the centre of the screen, pulsing; no countdown.
fn leaving(p: &mut Painter<'_>, v: &BattleView<'_>, f: f32, memory: &mut Leaving) {
    if !v.leaving || (v.body.is_none() && v.vehicle.is_none()) {
        memory.since = None;
        return;
    }
    let frac = if f.is_finite() { f64::from(f.clamp(0., 1.)) } else { 0. };
    let now = (f64::from(v.tick) + frac) / 60.;
    let since = *memory.since.get_or_insert(now);
    let age = (now - since).max(0.);
    let value = LEAVING_REST
        + (LEAVING_START - LEAVING_REST) * (LEAVING_STIFFNESS.sqrt() * age).cos();
    let [r, g, b] = WARNING_RED;
    let color = [r, g, b, value.trunc().clamp(2., 255.) / 255.];
    p.place(CENTRE);
    let text = Caption::Key(LEAVING_KEY).resolve(p.catalog);
    let setting = Setting {
        font: "gamefont_large",
        align: 2,
        valign: 2,
        shadow: false,
        scale: LEAVING_TEXT_SCALE,
        spacing: 0.,
    };
    let centre = [PLANE[0] * 0.5, PLANE[1] * 0.5];
    p.set_line(setting, centre, [0.; 2], color, format_args!("{text}"));
}

/// The player's own points and kills as last drawn, the last change of each, and the battle
/// time of that change in seconds.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Stats {
    seen: Option<(i32, u32)>,
    delta: (i32, i32),
    changed: f64,
}

/// Opacity `age` seconds after the element was enabled, by its fade times.
fn fade(w: &Widget, age: f64) -> f64 {
    let [rise, hold, fall] = w.fade;
    if rise > 0. && age < rise {
        age / rise
    } else if age < rise + hold + w.sustain {
        1.
    } else if fall > 0. {
        (1. - (age - rise - hold - w.sustain) / fall).max(0.)
    } else {
        0.
    }
}

/// The player's points and kills with their last change, shown when they change and faded
/// out after the group's hold time.
fn stats(p: &mut Painter<'_>, v: &BattleView<'_>, f: f32, memory: &mut Stats) {
    let Some(row) = v.scores.iter().find(|r| r.player == v.player) else {
        return;
    };
    let frac = if f.is_finite() { f.clamp(0., 1.) } else { 0. };
    let seconds = (v.tick as f64 + f64::from(frac)) / 60.;
    let now = (row.score, row.kills);
    let count = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
    match memory.seen {
        Some(old) if old != now => {
            memory.delta = (
                now.0.saturating_sub(old.0),
                count(now.1).saturating_sub(count(old.1)),
            );
            memory.changed = (v.tick as f64) / 60.;
        }
        None => {
            memory.delta = (0, 0);
            memory.changed = (v.tick as f64) / 60.;
        }
        _ => {}
    }
    memory.seen = Some(now);
    let layout = &p.catalog.layout;
    let Some(group) = layout.named("player1stats") else {
        return;
    };
    let opacity = fade(group, seconds - memory.changed);
    if opacity <= 0. {
        return;
    }
    p.place(group.pin);
    let faded = |w: &Widget| [w.color[0], w.color[1], w.color[2], w.color[3] * opacity];
    for icon in ["player1statspoints.icon", "player1statskills.icon"] {
        if let Some(w) = layout.named(icon) {
            p.icon(w.rect, &w.bitmap, faded(w));
        }
    }
    for (signal, change, value, delta) in [
        (
            "player1.statistic.points",
            "player1.statistic.pointsDelta",
            now.0,
            memory.delta.0,
        ),
        (
            "player1.statistic.kills",
            "player1.statistic.killsDelta",
            count(now.1),
            memory.delta.1,
        ),
    ] {
        if let Some(w) = layout.binding("text", signal) {
            p.game_text(w, w.at, faded(w), format_args!("{value}"));
        }
        if delta != 0 {
            if let Some(w) = layout.binding("text", change) {
                p.game_text(w, w.at, faded(w), format_args!("{delta:+}"));
            }
        }
    }
}

/// The small map's diameter in plane units at a `Scale` of one, and a command post's marker
/// on it as drawn (measured on the game's frames: 123 pixels across at 720 lines for the
/// map's 0.82, markers 16 pixels).
pub(super) const MAP_ACROSS: f64 = 160.;
pub(super) const POST_ACROSS: f64 = 17.;
/// A command post rides the contact list under this icon name: the small map draws it in its
/// own pass (in the team's colour, on the rim when out of range), not as a soldier's mark.
pub const POST_CONTACT: &str = "@command-post";

/// The pin of what follows the whole window rather than an edge.
pub(super) const CENTRE: [f64; 2] = [0.5, 0.5];

/// The game's colours for the player's own team and for the others (measured on the
/// target health bar of two frames, through the bar group's tint).
pub(super) fn team_color(same: bool) -> Color {
    if same {
        [1. / 255., 86. / 255., 213. / 255., 1.]
    } else {
        [223. / 255., 32. / 255., 32. / 255., 1.]
    }
}

fn bar_piece(p: &mut Painter<'_>, w: &Widget, amount: f64, color: Color) {
    let mut rect = w.rect;
    rect[2] *= amount;
    if rect[2] <= 0. {
        return;
    }
    let texture = p.art(&w.bitmap);
    if texture == 0 {
        p.solid(rect, color);
    } else {
        let mut uv = w.uv;
        uv[2] = uv[0] + (uv[2] - uv[0]) * amount;
        p.image(rect, texture, uv, color, 0);
    }
}

/// Up to eight elements, back to front (larger `ZOrder` first).
fn back_to_front<'a>(items: impl Iterator<Item = &'a Widget>) -> ([Option<&'a Widget>; 8], usize) {
    let mut out = [None; 8];
    let mut n = 0;
    for w in items.take(8) {
        let mut i = n;
        while i > 0 && out[i - 1].is_some_and(|o: &Widget| o.z < w.z) {
            out[i] = out[i - 1];
            i -= 1;
        }
        out[i] = Some(w);
        n += 1;
    }
    (out, n)
}

/// A bar as the definition builds it: the static pieces of its group that share the fill's
/// bitmap (backing, blank fill) behind every fill bound to `signal`, cropped to the observed
/// fraction. A fill's colour is the code-supplied `event` colour, else its colour ramp, else
/// its own; the group's tint applies to all of them.
pub(super) fn gauge(
    p: &mut Painter<'_>,
    signal: &str,
    observation: Gauge,
    f: f32,
    event: Option<Color>,
) {
    let layout = &p.catalog.layout;
    let Some(first) = layout.value_bindings("BarBitmap", signal).next() else {
        return;
    };
    let amount = f64::from(observation.fill(f)).clamp(0., 1.);
    p.place(first.pin);
    let (pieces, n) = back_to_front(layout.siblings(first).filter(|w| {
        matches!(w.kind.as_str(), "BarBitmap" | "Bitmap")
            && w.bitmap.eq_ignore_ascii_case(&first.bitmap)
            && !w.value.contains("Fraction")
    }));
    for w in pieces[..n].iter().flatten() {
        bar_piece(p, w, 1., w.color);
    }
    let (fills, n) = back_to_front(layout.value_bindings("BarBitmap", signal));
    for w in fills[..n].iter().flatten() {
        let own = [w.color[0], w.color[1], w.color[2], 1.];
        // The ramp fed by the bar's own value wins over another ramp sharing its output.
        let ramp = if layout.has_ramp(signal) {
            signal
        } else {
            w.tint.as_str()
        };
        let base = event.unwrap_or_else(|| {
            if ramp.is_empty() {
                own
            } else {
                let rgb = layout.color(ramp, amount, own);
                [
                    rgb[0] * w.inherit[0],
                    rgb[1] * w.inherit[1],
                    rgb[2] * w.inherit[2],
                    1.,
                ]
            }
        });
        let color = if event.is_some() {
            [
                base[0] * w.inherit[0],
                base[1] * w.inherit[1],
                base[2] * w.inherit[2],
                w.color[3],
            ]
        } else {
            [base[0], base[1], base[2], w.color[3]]
        };
        bar_piece(p, w, amount, color);
    }
}

/// The team counters at the top: points against the target ("9/180") where the mission
/// shows points, else the reinforcement count, in the definition's colours and font.
fn scores(p: &mut Painter<'_>, v: &BattleView<'_>) {
    let signals = [
        ("player1.team1.pointsText", "player1.team1.reinforcements"),
        ("player1.team2.pointsText", "player1.team2.reinforcements"),
    ];
    for (team, (points, reserve)) in v.teams.iter().zip(signals) {
        let layout = &p.catalog.layout;
        if team.show_score {
            let Some(w) = layout.binding("text", points) else {
                continue;
            };
            p.place(w.pin);
            match team.target {
                Some(target) => p.game_text(w, w.at, w.color, format_args!("{}/{target}", team.score)),
                None => p.game_text(w, w.at, w.color, format_args!("{}", team.score)),
            }
        } else if let Some(count) = team.reinforcements {
            let Some(w) = layout.binding("text", reserve) else {
                continue;
            };
            p.place(w.pin);
            p.game_text(w, w.at, w.color, format_args!("{count}"));
        }
    }
}

/// The game's weapon boxes: the backdrop model, the weapon's own model where its `MeshInfo`
/// puts it, the total ammunition (or the infinity model) and the name that fades after a change.
pub(super) fn equipment(p: &mut Painter<'_>, slots: &[Option<Slot<'_>>; 2], tick: u32, f: f32) {
    const SIGNALS: [[&str; 6]; 2] = [
        [
            "player1.weapon1.name",
            "player1.weapon1.totalAmmoBullets",
            "player1.weapon1.mesh",
            "player1.weapon1.ammoInfinite",
            "player1weapon1backdrop",
            "player1weapon1mesh",
        ],
        [
            "player1.weapon2.name",
            "player1.weapon2.totalAmmoBullets",
            "player1.weapon2.mesh",
            "player1.weapon2.ammoInfinite",
            "player1weapon2backdrop",
            "player1weapon2mesh",
        ],
    ];
    for (slot, [name, ammo, mesh, infinite, backdrop, model]) in slots.iter().zip(SIGNALS) {
        let Some(slot) = slot else { continue };
        let layout = &p.catalog.layout;
        // Larger `ZOrder` lies further back: backdrop, weapon, count, infinity.
        if let Some(w) = layout.named(backdrop) {
            p.place(w.pin);
            p.model(&w.mesh, w.at, w.scale, w.color);
        }
        let art = layout.mesh(mesh, slot.icon);
        if let Some(w) = layout.mesh_info(model, art) {
            p.place(w.pin);
            p.model(art, w.at, w.scale, w.color);
        }
        if let Some(count) = slot.rounds {
            if let Some(w) = layout.binding("text", ammo) {
                let total = count.saturating_add(slot.reserve.unwrap_or(0));
                p.place(w.pin);
                p.game_text(w, w.at, w.color, format_args!("{total}"));
            }
        } else if let Some(w) = layout.binding("Model3D", infinite) {
            p.place(w.pin);
            p.model(&w.mesh, w.at, w.scale, w.color);
        }
        if let Some(label) = layout.binding("text", name) {
            let opacity = if label.enable.is_empty() {
                1.
            } else {
                slot.changed_at
                    .filter(|&start| start <= tick)
                    .map_or(0., |start| {
                        let fraction = if f.is_finite() { f.clamp(0., 1.) } else { 0. };
                        let age = ((tick - start) as f64 + f64::from(fraction)) / 60.;
                        let [rise, hold, fall] = label.fade;
                        if age < rise {
                            age / rise
                        } else if age < rise + hold {
                            1.
                        } else if age < rise + hold + fall {
                            1. - (age - rise - hold) / fall
                        } else {
                            0.
                        }
                    })
            };
            if opacity > 0. {
                let mut color = label.color;
                color[3] *= opacity;
                p.place(label.pin);
                p.game_label(label, label.at, color, slot.name);
            }
        }
    }
}

pub(super) fn segmented_bar(
    p: &mut Painter<'_>,
    w: &bfe_render::hud_model2::Widget,
    fill: f64,
    color: Color,
    offset: [f64; 2],
) {
    let n = usize::from(w.segments);
    let fill = fill.clamp(0., 1.);
    if n == 0 || fill <= 0. {
        return;
    }
    let cx = w.rect[0] + w.rect[2] * 0.5 + offset[0];
    let cy = w.rect[1] + w.rect[3] * 0.5 + offset[1];
    let rx = w.rect[2].abs() * 0.5;
    let ry = w.rect[3].abs() * 0.5;
    let (texture, [sw, sh], base_color) = if let Some(seg) = &w.segment {
        let tex = if seg.bitmap.is_empty() {
            0
        } else {
            p.art(&seg.bitmap)
        };
        let texture = if tex == 0 {
            p.surface.solid_texture()
        } else {
            tex
        };
        let size = if seg.size[0] > 0. && seg.size[1] > 0. {
            seg.size
        } else {
            [6., 6.]
        };
        let col = if !w.tint.is_empty() {
            [color[0], color[1], color[2], seg.color[3]]
        } else if seg.has_color {
            seg.color
        } else {
            [color[0], color[1], color[2], seg.color[3]]
        };
        (texture, size, col)
    } else {
        (p.surface.solid_texture(), [6., 6.], color)
    };
    let bounds = [-sw * 0.5, -sh * 0.5, sw * 0.5, sh * 0.5];
    for i in 0..n {
        let amount = (fill * n as f64 - i as f64).clamp(0., 1.);
        if amount <= 1e-4 {
            break;
        }
        let t = (i as f64 + 0.5) / n as f64;
        let deg = w.angles[0] + (w.angles[1] - w.angles[0]) * t;
        let rad = (if w.mirror_x { -deg } else { deg }).to_radians();
        let px = cx + rad.sin() * rx;
        let py = cy - rad.cos() * ry;
        let local = rotate_z(translate(p.matrix, px, py, 0.), rad);
        let mut col = base_color;
        col[3] *= amount;
        p.projected_image(local, bounds, texture, UV, col, 0);
    }
}

pub(super) fn weapon_rings(
    p: &mut Painter<'_>,
    slots: &[Option<Slot<'_>>; 2],
    tick: u32,
    f: f32,
    offset: [f64; 2],
) {
    let layout = &p.catalog.layout;
    if let Some(primary) = slots[0] {
        if let Some(clip) = primary.clip {
            let fill = f64::from(clip.fill(f));
            if fill > 0. {
                for w in layout.value_bindings("BarSegmented", "player1.weapon1.totalClipFraction") {
                    segmented_bar(p, w, fill, w.color, offset);
                }
            }
        }
        if let Some(heat) = primary.heat {
            let fill = f64::from(heat.fill(f));
            if fill > 0. {
                for w in layout.value_bindings("BarSegmented", "player1.weapon1.heat") {
                    let col = if w.tint.is_empty() {
                        w.color
                    } else {
                        layout.color(&w.tint, fill, w.color)
                    };
                    segmented_bar(p, w, fill, col, offset);
                }
            }
        }
        if let Some(refire) = primary.refire {
            let fill = f64::from(refire.fill(f));
            if fill > 0. {
                for w in layout.value_bindings("BarSegmented", "player1.weapon1.refire") {
                    segmented_bar(p, w, fill, w.color, offset);
                }
            }
        }
        if primary.overheated {
            if let Some(w) = layout.binding("text", "player1.weaponsOverheat") {
                let frac = if f.is_finite() { f.clamp(0., 1.) } else { 0. };
                let secs = (f64::from(tick) + f64::from(frac)) / 60.;
                let pulse = if w.pulse > 0. {
                    0.75 + 0.25 * (secs * w.pulse * std::f64::consts::TAU).sin()
                } else {
                    1.
                };
                let mut col = w.color;
                col[3] *= pulse;
                let key = if w.text.is_empty() {
                    "game.weaponOverheat"
                } else {
                    &w.text
                };
                p.place(w.pin);
                p.game_label(w, w.at, col, Caption::Key(key));
            }
        }
    }
    if let Some(secondary) = slots[1] {
        if let Some(refire) = secondary.refire {
            let fill = f64::from(refire.fill(f));
            if fill > 0. {
                for w in layout.value_bindings("BarSegmented", "player1.weapon2.refire") {
                    segmented_bar(p, w, fill, w.color, offset);
                }
            }
        }
    }
}

fn scope_overlay(p: &mut Painter<'_>, name: &str) -> bool {
    let tex = p.art(name);
    if tex == 0 {
        return false;
    }
    let [vl, vt] = p.viewport_point([0., 0.]);
    let [vr, vb] = p.viewport_point([1., 1.]);
    let cx = (vl + vr) * 0.5;
    let cy = (vt + vb) * 0.5;
    // The 2005 scope quadrant art is authored for the 4:3 HUD plane.
    let ry = ((vb - vt) * 0.5).max(1.);
    let rx = ry * (4. / 3.);
    let left_pad = (cx - rx - vl).max(0.);
    if left_pad > 0.5 {
        let edge_uv = [0., 0., 0.01, 0.01];
        p.image([vl, vt, left_pad, vb - vt], tex, edge_uv, WHITE, 0);
        p.image(
            [cx + rx, vt, (vr - (cx + rx)).max(0.), vb - vt],
            tex,
            edge_uv,
            WHITE,
            0,
        );
    }
    for (rect, uv) in [
        ([cx - rx, cy - ry, rx, ry], [0., 0., 1., 1.]),
        ([cx, cy - ry, rx, ry], [1., 0., 0., 1.]),
        ([cx - rx, cy, rx, ry], [0., 1., 1., 0.]),
        ([cx, cy, rx, ry], [1., 1., 0., 0.]),
    ] {
        p.image(rect, tex, uv, WHITE, 0);
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub(super) fn reticle(
    p: &mut Painter<'_>,
    desired: &str,
    target: Option<&Target<'_>>,
    aim: &Aim<'_>,
    at: [f64; 2],
    tick: u32,
    f: f32,
    scoped: bool,
) {
    let color = match aim.relation {
        AimRelation::Enemy => team_color(false),
        AimRelation::Friendly => team_color(true),
        _ if target.is_some_and(|t| t.lock == Lock::Locked) => team_color(false),
        _ => WHITE,
    };
    let layout = &p.catalog.layout;
    let tinted = |w: &bfe_render::hud_model2::Widget| [color[0], color[1], color[2], w.color[3]];
    // Back to front as the definition lists them: outline, shape, bullseye. A scope keeps only
    // the bullseye. A weapon's own reticle picture takes the shape's place.
    if !scoped {
        if let Some(w) = layout.named("player1reticule_outline") {
            p.model(&w.mesh, at, w.scale, w.color);
        }
        if let Some(w) = layout.named("player1reticule_shape") {
            match p.model_rect(&w.mesh, at, w.scale) {
                Some(rect) if !desired.is_empty() && p.art(desired) != 0 => {
                    p.icon(rect, desired, tinted(w));
                }
                _ => {
                    p.model(&w.mesh, at, w.scale, tinted(w));
                }
            }
        }
    }
    if let Some(w) = layout.named("player1reticule_bullseye") {
        p.model(&w.mesh, at, w.scale, tinted(w));
    }
    if let Some(hit) = aim.hit.filter(|h| tick >= h.tick) {
        let name = if hit.critical {
            "player1reticule_criticalhit"
        } else {
            "player1reticule_hit"
        };
        let Some(w) = layout.named(name) else {
            return;
        };
        let frac = if f.is_finite() { f.clamp(0., 1.) } else { 0. };
        let age = ((tick - hit.tick) as f64 + f64::from(frac)) / 60.;
        let [fade_in, hold, fade_out] = w.fade;
        let shown = fade_in + hold + w.sustain;
        let factor = if fade_in > 0. && age < fade_in {
            age / fade_in
        } else if age < shown {
            1.
        } else if fade_out > 0. {
            (1. - (age - shown) / fade_out).clamp(0., 1.)
        } else {
            0.
        };
        if factor > 0. {
            let mut col = w.color;
            col[3] *= factor;
            p.model(&w.mesh, at, w.scale, col);
        }
    }
}

/// The small map's backdrop under its picture: black, as the 2005 game builds it, at its own
/// alpha (63 of 255), which with the picture's own alpha darkens the world under the disc to
/// 0.43 as the game's frames show.
const BACKDROP: Color = [0., 0., 0., 63. / 255.];

fn radar(p: &mut Painter<'_>, v: &BattleView<'_>, f: f32) -> Option<[f64; 4]> {
    let layout = &p.catalog.layout;
    let w = layout.named("player1map")?;
    p.place(w.pin);
    let pose = v.vehicle.and_then(|seat| seat.map_pose).or_else(|| {
        v.body.map(|b| super::MapPose {
            map: b.map,
            yaw: b.yaw,
        })
    })?;
    let [x, y] = w.at;
    let r = 0.5 * MAP_ACROSS * w.scale[0].abs();
    let rect = [x - r, y - r, 2. * r, 2. * r];
    let backdrop = layout.named("BackdropSmall");
    let mask = p.art(backdrop.map_or("map_mask", |art| &art.mask));
    let solid = p.surface.solid_texture();
    p.image(rect, solid, UV, BACKDROP, mask);
    let center = point(pose.map, f);
    let heading = -f64::from(pose.yaw.angle_at(f));
    let (sin, cos) = heading.sin_cos();
    let rotate = |dx: f64, dy: f64| [cos * dx - sin * dy, sin * dx + cos * dy];
    let local = rotate_z(translate(p.matrix, x, y, 0.), heading);
    let uv = backdrop.map_or(UV, |art| art.uv);
    let span = [[uv[0], uv[2]], [uv[1], uv[3]]].map(|[a, b]| (b - a).abs().clamp(1e-5, 1.));
    let start = [center[0] - span[0] / 2., center[1] - span[1] / 2.];
    let end = [start[0] + span[0], start[1] + span[1]];
    // At a map edge leave the uncharted portion empty while keeping the circular mask fixed.
    let lo = start.map(|v| v.max(0.));
    let hi = end.map(|v| v.min(1.));
    if lo[0] < hi[0] && lo[1] < hi[1] {
        let mask_start = [(lo[0] - start[0]) / span[0], (lo[1] - start[1]) / span[1]];
        let mask_end = [(hi[0] - start[0]) / span[0], (hi[1] - start[1]) / span[1]];
        let [left, top] = mask_start.map(|v| r * (2. * v - 1.));
        let [right, bottom] = mask_end.map(|v| r * (2. * v - 1.));
        let first = p.surface.batch_mut().count;
        p.projected_image(
            local,
            [left, top, right, bottom],
            p.art(v.map_image),
            [lo[0], lo[1], hi[0], hi[1]],
            backdrop.map_or(WHITE, |art| art.color),
            mask,
        );
        let batch = p.surface.batch_mut();
        if batch.count > first {
            for (i, [dx, dy]) in [[left, top], [left, bottom], [right, top], [right, bottom]]
                .into_iter()
                .enumerate()
            {
                let vertex = first * 4 + i;
                let uv = rotate(dx, dy).map(|offset| (0.5 + offset / (2. * r)) as f32);
                if batch.mask_uv[vertex * 2..vertex * 2 + 2] != uv {
                    batch.mask_uv[vertex * 2..vertex * 2 + 2].copy_from_slice(&uv);
                    batch.start = batch.start.min(vertex);
                    batch.end = batch.end.max(vertex + 1);
                }
            }
        }
    }
    // Both authored meshes point up. Their centre is the player's location in the texture.
    let mut has_pointer = false;
    for key in ["PlayerDirectionSmall", "PlayerFOVSmall"] {
        if let Some(art) = layout.named(key).filter(|w| p.mesh_art(&w.mesh) != 0) {
            p.image(rect, p.mesh_art(&art.mesh), UV, art.color, mask);
            has_pointer = true;
        }
    }
    if !has_pointer {
        p.solid([x - 2., y - 7., 4., 11.], WHITE);
    }
    let visible = v.contacts.iter().filter(|c| c.icon != POST_CONTACT).filter_map(|c| {
        let uv = point(c.map, f);
        let dx = (uv[0] - center[0]) * 2. / span[0];
        let dy = (uv[1] - center[1]) * 2. / span[1];
        (dx.hypot(dy) * r <= r - 5.).then_some((c, r * dx, r * dy))
    });
    for (c, dx, dy) in visible.take(128) {
        let [dx, dy] = rotate(dx, dy);
        let (u, w) = (x + dx, y + dy);
        let color = team_color(c.team == v.team);
        if p.art(c.icon) != 0 {
            p.icon([u - 5., w - 5., 10., 10.], c.icon, color);
        } else {
            p.solid([u - 2., w - 2., 4., 4.], color);
        }
    }
    // Command posts with the definition's command-post picture in their team's colour (white
    // while nobody holds one); a post out of range rests on the rim.
    let post_art = layout
        .named("player1mapkey_CPIcon")
        .map_or("", |w| w.bitmap.as_str());
    for post in v.contacts.iter().filter(|c| c.icon == POST_CONTACT) {
        let uv = point(post.map, f);
        let [mut dx, mut dy] = [
            (uv[0] - center[0]) * 2. / span[0],
            (uv[1] - center[1]) * 2. / span[1],
        ];
        let reach = dx.hypot(dy);
        if reach > 1. {
            dx /= reach;
            dy /= reach;
        }
        let [dx, dy] = rotate(r * dx, r * dy);
        let half = 0.5 * POST_ACROSS;
        let color = if post.team == 0 {
            [1.; 4]
        } else {
            team_color(post.team == v.team)
        };
        p.icon([x + dx - half, y + dy - half, 2. * half, 2. * half], post_art, color);
    }
    if let Some(north) = layout.named("NorthSmall") {
        let [dx, dy] = [north.at[0] - x, north.at[1] - y];
        p.projected_image(
            local,
            [dx - 6., dy - 5., dx + 6., dy + 5.],
            p.art(&north.bitmap),
            UV,
            north.color,
            0,
        );
    }
    Some(rect)
}

/// The target's name and health at the top centre, or, once locked, the lock-on group at the
/// lower right; the four corners of a lock around the target on screen.
fn target_info(p: &mut Painter<'_>, t: &Target<'_>, team: u8, f: f32) {
    let locked = t.lock != Lock::None;
    let layout = &p.catalog.layout;
    let key = if locked {
        "player1.lockOnClassName"
    } else {
        "player1.weapon1.target.className"
    };
    if let Some(w) = layout.binding("text", key) {
        p.place(w.pin);
        p.game_label(w, w.at, w.color, t.name);
    }
    if let Some(health) = t.health {
        let key = if locked {
            "player1.lockOnHealthFraction"
        } else {
            "player1.weapon1.target.healthFraction"
        };
        gauge(p, key, health, f, Some(team_color(t.team == team)));
    }
    let Some(bounds) = t.bracket.filter(|_| locked) else {
        return;
    };
    let Some(art) = layout
        .named("LockedOnScreen")
        .filter(|art| p.art(&art.bitmap) != 0)
    else {
        return;
    };
    p.place(CENTRE);
    let [left, top, right, bottom] = bounds.map(|s| f64::from(s.at(f)));
    if right <= left || bottom <= top || left >= 1. || top >= 1. || right <= 0. || bottom <= 0. {
        return;
    }
    let [l, t0] = p.viewport_point([left.max(0.), top.max(0.)]);
    let [r, b] = p.viewport_point([right.min(1.), bottom.min(1.)]);
    // The game tints the corners blue around a friendly target.
    let tint = if t.team == team {
        [0., 0., 1., art.color[3]]
    } else {
        art.color
    };
    for (x, dx) in [(l, 1.), (r, -1.)] {
        for (y, dy) in [(t0, 1.), (b, -1.)] {
            let [u0, v0, u1, v1] = art.uv;
            let (u0, u1) = if dx > 0. { (u0, u1) } else { (u1, u0) };
            let (v0, v1) = if dy > 0. { (v0, v1) } else { (v1, v0) };
            let w = art.rect[2].abs().min((r - l) / 2.);
            let h = art.rect[3].abs().min((b - t0) / 2.);
            p.image(
                [x.min(x + dx * w), y.min(y + dy * h), w, h],
                p.art(&art.bitmap),
                [u0, v0, u1, v1],
                tint,
                0,
            );
        }
    }
}

/// The message list at the top left: the newest visible messages in the definition's
/// format, kill messages in the game's own wording ("A killed B").
/// Glyphs one message may hold: two player names at their longest and the game's wording.
const MESSAGE_GLYPHS: usize = 96;

fn messages(p: &mut Painter<'_>, v: &BattleView<'_>) {
    let layout = &p.catalog.layout;
    let Some(list) = layout.binding("MultilineText", "player1.message") else {
        return;
    };
    // The list's format element carries the font, alignment, box and default colour.
    let format = layout.child(list, "format").unwrap_or(list);
    p.place(list.pin);
    let step = p.surface.font(super::paint::font_role(&format.font)).height * super::paint::TEXT_SCALE;
    // The definition's NumLines, else as many rows as the list's rect holds.
    let rows = if list.lines > 0 {
        usize::from(list.lines)
    } else {
        ((list.rect[3] / step).floor() as usize).max(1)
    };
    // A message longer than the format's box breaks at spaces onto the next row (TextBreak Word).
    let width = if format.text_box[0] > 0. { format.text_box[0] } else { list.rect[2] };
    let setting = Setting {
        font: &format.font,
        align: format.align,
        valign: 0,
        shadow: format.shadow,
        scale: 1.,
        spacing: super::paint::TEXT_SPACING,
    };
    let catalog = p.catalog;
    let labels = p.glyph_limit;
    p.glyph_limit = MESSAGE_GLYPHS;
    // Producers publish sequence order: the newest messages that fit, each with its rows.
    let mut picked = [(0usize, 0usize); 64];
    let mut count = 0;
    let mut used = 0;
    for (index, e) in v.events.iter().enumerate().rev() {
        if used >= rows || count == picked.len() {
            break;
        }
        if e.until_tick <= v.tick || !e.audience.includes(v.player, v.team) {
            continue;
        }
        let ([a, b, c, d, f], _) = message_text(catalog, v.mode, e, format.color);
        let at = Rows { at: [0.; 2], width, step, count: rows - used };
        let need = p.wrapped_rows(setting, at, format_args!("{a}{b}{c}{d}{f}"));
        if need > 0 {
            picked[count] = (index, need);
            count += 1;
            used += need;
        }
    }
    // Oldest at the top, newest at the bottom of the list.
    let mut row = 0;
    for &(index, need) in picked[..count].iter().rev() {
        let ([a, b, c, d, f], color) = message_text(catalog, v.mode, &v.events[index], format.color);
        let at = Rows {
            at: [list.rect[0], list.rect[1] + row as f64 * step],
            width,
            step,
            count: need,
        };
        p.set_wrapped(setting, at, color, format_args!("{a}{b}{c}{d}{f}"));
        row += need;
    }
    p.glyph_limit = labels;
}

/// A message's text in five pieces (written one after another) and its colour: a notice in the
/// player's team colour, a kill in the game's own wording for the case in the list's colour.
fn message_text<'a>(
    catalog: &'a bfe_render::hud_model2::catalog::Catalog,
    mode: Mode,
    e: &Event<'a>,
    color: Color,
) -> ([&'a str; 5], Color) {
    match e.message {
        Message::Notice(c) => ([c.resolve(catalog), "", "", "", ""], team_color(true)),
        Message::Kill {
            killer,
            victim,
            weapon,
            weapon_name,
        } => {
            // A manned kill names the killer; an unmanned one names the gun from the keys its
            // producer supplies. The wording is the game's own message for the case.
            let actor = if killer.is_empty() {
                weapon
                    .split('|')
                    .filter(|k| !k.is_empty())
                    .find_map(|k| catalog.localized(k))
                    .or(weapon_name)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            } else {
                Some(killer)
            };
            let localized = |key: &str| catalog.localized(key).map(str::trim);
            let (pattern, first, second) = match actor {
                Some(who) if who == victim => {
                    (localized("game.infomsg.suicide").unwrap_or("%s"), victim, "")
                }
                Some(who) => {
                    let killed = localized("game.infomsg.killed");
                    let wording = if mode == Mode::HeroAssault {
                        localized("game.infomsg.defeated").or(killed)
                    } else {
                        killed
                    };
                    (wording.unwrap_or("%s %s"), who, victim)
                }
                None => (localized("game.infomsg.died").unwrap_or("%s"), victim, ""),
            };
            let (head, rest) = pattern.split_once("%s").unwrap_or(("", pattern));
            let (middle, tail) = rest.split_once("%s").unwrap_or((rest, ""));
            let second = if rest.contains("%s") { second } else { "" };
            ([head, first, middle, second, tail], color)
        }
    }
}

/// The objective timer group shows the selected mission timer's clock.
fn timer(p: &mut Painter<'_>, v: &BattleView<'_>) {
    let Some(timer) = v
        .timers
        .iter()
        .find(|t| t.audience.includes(v.player, v.team))
    else {
        return;
    };
    let Some(w) = p.catalog.layout.binding("text", "objectivetimer") else {
        return;
    };
    let left = timer.due.saturating_sub(v.tick).div_ceil(60);
    p.place(w.pin);
    p.game_text(w, w.at, w.color, format_args!("{:02}:{:02}", left / 60, left % 60));
}
