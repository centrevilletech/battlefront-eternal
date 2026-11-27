use super::paint::Painter;
use super::playing::{equipment, gauge, team_color, weapon_rings, CENTRE};
use super::{Gauge, Seat, Vehicle};

pub(super) fn flight(
    p: &mut Painter<'_>,
    seat: &Vehicle<'_>,
    tick: u32,
    f: f32,
    takeoff_key: &str,
) {
    gauge(p, "player1.vehicle.healthFraction", seat.hull, f, None);
    if let Some(energy) = seat.boost {
        gauge(p, "player1.energyFraction", energy, f, None);
    }
    equipment(p, &seat.slots, tick, f);
    p.place(CENTRE);
    weapon_rings(p, &seat.slots, tick, f, [0., 0.]);
    if seat.seat == Seat::Pilot {
        if let Some(prompt) = seat.landing {
            tooltip(p, takeoff_key, prompt);
        }
    }
}

/// The game's control hints use the tooltip strip at the bottom centre.
fn tooltip(p: &mut Painter<'_>, key: &str, prompt: super::Caption<'_>) {
    let Some(w) = p.catalog.layout.binding("text", "player1.tooltips") else {
        return;
    };
    p.place(w.pin);
    let layout = &p.catalog.layout;
    if let Some(strip) = layout.parent(w).filter(|b| b.kind == "BorderedBox") {
        p.bordered(strip, strip.color);
    }
    let text = prompt.resolve(p.catalog);
    let gap = if key.is_empty() { "" } else { " " };
    p.game_text(w, w.at, w.color, format_args!("{key}{gap}{text}"));
}

/// Colour of a critical system's mark by the health left (red, yellow, green), as the 2005
/// game ramps it; black once destroyed.
fn system_color(fraction: f64) -> bfe_render::interface::Color {
    const LOW: [f64; 3] = [223., 32., 32.];
    const MID: [f64; 3] = [223., 223., 32.];
    const HIGH: [f64; 3] = [32., 223., 32.];
    let f = fraction.clamp(0., 1.);
    let (a, b, t) = if f <= 0.5 {
        (LOW, MID, f * 2.)
    } else {
        (MID, HIGH, (f - 0.5) * 2.)
    };
    [
        (a[0] + (b[0] - a[0]) * t) / 255.,
        (a[1] + (b[1] - a[1]) * t) / 255.,
        (a[2] + (b[2] - a[2]) * t) / 255.,
        1.,
    ]
}

/// A shield picture's colour by the shield left: red rises while the cyan fades out.
fn shield_color(fraction: f64) -> bfe_render::interface::Color {
    let f = fraction.clamp(0., 1.);
    [1. - f, f * 200. / 255., f * 150. / 255., f]
}

/// How far a mark or shield still shows the white of a hit, one second after it.
/// `at` is the observed tick's time and `now` the drawn time, both in seconds.
/// A drop is a lower value than the gauge's previous sample, or than the value last drawn
/// (a source that gives no previous sample, such as an online battle's replicated systems).
fn flash(memory: &mut Flashes, key: (u32, u32, bool), gauge: Gauge, at: f64, now: f64) -> f64 {
    let slot = memory.slot(key);
    let current = gauge.value.current;
    let dropped = current < gauge.value.previous || current < slot.last;
    if dropped && slot.since < at {
        slot.since = at;
    }
    slot.last = current;
    (1. - (now - slot.since)).clamp(0., 1.)
}

fn whiten(color: bfe_render::interface::Color, by: f64) -> bfe_render::interface::Color {
    std::array::from_fn(|i| color[i] + (1. - color[i]) * by)
}

/// Hits remembered for the Space Assault group: up to one per system and one per shield.
#[derive(Clone, Copy, Debug)]
pub(super) struct Flashes {
    keys: [(u32, u32, bool); 40],
    slots: [Flash; 40],
    len: usize,
}

#[derive(Clone, Copy, Debug)]
struct Flash {
    since: f64,
    /// The value last drawn (NaN until one is).
    last: f32,
}

impl Default for Flashes {
    fn default() -> Self {
        Self {
            keys: [(u32::MAX, 0, false); 40],
            slots: [Flash { since: f64::NEG_INFINITY, last: f32::NAN }; 40],
            len: 0,
        }
    }
}

impl Flashes {
    fn slot(&mut self, key: (u32, u32, bool)) -> &mut Flash {
        let i = self.keys[..self.len].iter().position(|k| *k == key).unwrap_or_else(|| {
            let i = if self.len < self.keys.len() {
                self.len += 1;
                self.len - 1
            } else {
                self.len - 1
            };
            self.keys[i] = key;
            self.slots[i] = Flash { since: f64::NEG_INFINITY, last: f32::NAN };
            i
        });
        &mut self.slots[i]
    }
}

/// The Space Assault group as the 2005 game draws it in its corner: the player's own team's
/// ship picture on the right half and the other team's on the left, each with its shield
/// picture over it, and each critical system's mark where the mission puts it on its ship.
pub(super) fn assault(p: &mut Painter<'_>, v: &super::BattleView<'_>, f: f32, memory: &mut Flashes) {
    let Some(group) = p.catalog.layout.named("player1spaceassaultgroup") else {
        return;
    };
    let [x, y, width, height] = group.rect;
    if width <= 0. || height <= 0. || v.systems.is_empty() {
        return;
    }
    p.place(group.pin);
    let frac = if f.is_finite() { f64::from(f.clamp(0., 1.)) } else { 0. };
    let at = v.tick as f64 / 60.;
    let now = at + frac / 60.;
    let half = width * 0.5;
    let other = v
        .systems
        .iter()
        .map(|s| s.team)
        .find(|&t| t != v.team)
        .unwrap_or(0);
    for (team, left) in [(other, x), (v.team, x + half)] {
        let rect = [left, y, half, height];
        let Some(first) = v.systems.iter().find(|s| s.team == team) else {
            continue;
        };
        // The ship pictures are white: each takes its team's colour as the game's frames show
        // (own team friend blue, the other enemy red, both times the picture's own alpha).
        p.icon(rect, first.ship, team_color(team == v.team));
        // The team's first system with shields carries the ship's shield.
        if let Some((s, shield)) = v
            .systems
            .iter()
            .filter(|s| s.team == team)
            .find_map(|s| s.shield.filter(|g| g.maximum > 1e-4).map(|g| (s, g)))
        {
            let base = shield_color(f64::from(shield.fill(f)));
            let lit = flash(memory, (s.subject.id, s.subject.generation, true), shield, at, now);
            p.icon(rect, s.shield_art, whiten(base, lit));
        }
    }
    let size = [width * MARK, height * MARK];
    for s in v.systems.iter().filter(|s| s.marker) {
        let left = if s.team == v.team { x + half } else { x };
        let spot = [
            left + f64::from(s.hud[0]) * half,
            y + f64::from(s.hud[1]) * height,
        ];
        let color = if s.destroyed {
            [0., 0., 0., 1.]
        } else {
            let base = system_color(f64::from(s.hull.fill(f)));
            let lit = flash(memory, (s.subject.id, s.subject.generation, false), s.hull, at, now);
            whiten(base, lit)
        };
        p.icon(
            [spot[0] - size[0] * 0.5, spot[1] - size[1] * 0.5, size[0], size[1]],
            s.icon,
            color,
        );
    }
}

/// A mark's size as a share of the group's width and height.
const MARK: f64 = 0.15;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Sample;

    fn sampled(previous: f32, current: f32) -> Gauge {
        Gauge { value: Sample { previous, current }, maximum: 1. }
    }

    // A local battle's gauge says its previous sample; an online battle's
    // replicated systems give the same value as both. Either way a drop
    // whitens the mark for a second from the tick it is seen, a rise or a
    // steady value does not, and a mark seen first carries no old hit.
    #[test]
    fn a_system_drop_flashes_once_whether_or_not_its_source_gives_the_previous_sample() {
        let key = (3, 1, false);
        let mut memory = Flashes::default();
        assert_eq!(flash(&mut memory, key, sampled(0.8, 0.8), 10., 10.), 0.);
        assert_eq!(flash(&mut memory, key, sampled(0.8, 0.8), 10.5, 10.5), 0.);
        // Online: a lower value, the same as both samples.
        assert_eq!(flash(&mut memory, key, sampled(0.6, 0.6), 11., 11.), 1.);
        assert!((flash(&mut memory, key, sampled(0.6, 0.6), 11.25, 11.5) - 0.5).abs() < 1e-9);
        assert_eq!(flash(&mut memory, key, sampled(0.7, 0.7), 12.5, 12.5), 0.);
        // Locally: the gauge's own previous sample says it.
        let mut memory = Flashes::default();
        assert_eq!(flash(&mut memory, key, sampled(0.8, 0.5), 20., 20.), 1.);
        assert_eq!(flash(&mut memory, key, sampled(0.5, 0.5), 20.1, 21.), 0.);
        // Another system's memory is its own.
        assert_eq!(flash(&mut memory, (4, 1, false), sampled(0.5, 0.5), 21., 21.), 0.);
    }
}
