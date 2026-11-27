//! Battlefront II bolts that change colour in flight. A class with
//! `FadeLightColor` moves its glow from `LaserGlowColor` to
//! `EndLaserGlowColor` across its damage transition (from
//! `DamageTransitionDelay` for `DamageTransitionPeriod` seconds of flight,
//! all at once without a period), and its light from `LightColor` to
//! `EndLightColor` by the same measure. The draw picks one of a few prepared
//! styles by the bolt's age, so the wire and the shared draw stay as they are.
use super::{Bolt, Combat, Value};
use crate::online::combat::{Cosmetic, Style};
use bfe_presentation::{
    ordnance::style,
    ordnance_layout::{ordnance_limits as limits, ordnance_style as s},
};

/// Prepared steps of the transition after its start; step `STEPS` is the end colour.
const STEPS: usize = 32;

/// One fading class: its own style, and where its `STEPS` prepared ones begin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::online::combat) struct GlowFade {
    base: usize,
    first: usize,
    delay: f64,
    period: f64,
}

impl GlowFade {
    /// The style a bolt of this class shows `age` seconds into its flight.
    pub(in crate::online::combat) fn style(&self, age: f64) -> usize {
        if !(age > self.delay) {
            return self.base;
        }
        let along = if self.period > 0. {
            ((age - self.delay) / self.period).clamp(0., 1.)
        } else {
            1.
        };
        let step = ((along * STEPS as f64).round() as usize).clamp(1, STEPS);
        self.first + step - 1
    }
    /// The class's own style for any style this fade prepared.
    pub(in crate::online::combat) fn base_of(&self, style: usize) -> Option<usize> {
        (style == self.base || (self.first..self.first + STEPS).contains(&style)).then_some(self.base)
    }
}

/// A colour field as four 0 to 255 numbers (a list or "r g b a").
fn rgba(def: &Value, key: &str) -> Option<[f64; 4]> {
    let mut out = [255.; 4];
    match def.get(key)? {
        Value::Array(list) => {
            for (i, v) in list.iter().take(4).enumerate() {
                out[i] = v.as_f64()?;
            }
        }
        Value::String(line) => {
            for (i, v) in line.split_whitespace().take(4).enumerate() {
                out[i] = v.parse().ok()?;
            }
        }
        _ => return None,
    }
    Some(out)
}

fn number(def: &Value, key: &str) -> Option<f64> {
    let v = def.get(key)?;
    v.as_f64()
        .or_else(|| v.as_str()?.trim().parse().ok())
        .filter(|n: &f64| n.is_finite())
}

/// The class's start and end glow and light, and its transition, when it fades.
pub(in crate::online::combat) struct Fading {
    glow: [[f64; 4]; 2],
    light: [[f64; 4]; 2],
    delay: f64,
    period: f64,
}

pub(in crate::online::combat) fn fading(def: &Value) -> Option<Fading> {
    if !number(def, "FadeLightColor").is_some_and(|n| n != 0.) {
        return None;
    }
    let start = rgba(def, "LaserGlowColor").or_else(|| rgba(def, "LightColor"))?;
    let light = rgba(def, "LightColor").unwrap_or(start);
    Some(Fading {
        glow: [start, rgba(def, "EndLaserGlowColor").unwrap_or([0.; 4])],
        light: [light, rgba(def, "EndLightColor").unwrap_or([0.; 4])],
        delay: number(def, "DamageTransitionDelay").unwrap_or(0.),
        period: number(def, "DamageTransitionPeriod").unwrap_or(0.),
    })
}

/// A colour on 0 to 1 with its alpha folded in, as the class's own style has it.
fn folded(c: [f64; 4]) -> [f64; 3] {
    std::array::from_fn(|i| c[i] / 255. * (c[3] / 255.))
}

/// Each channel moved `along` the way from `from` to `to`, kept to whole
/// steps of 0 to 255 as the game keeps its colours.
fn mix(from: [f64; 4], to: [f64; 4], along: f64) -> [f64; 4] {
    std::array::from_fn(|i| (from[i] * (1. - along) + to[i] * along + 0.5).floor())
}

/// The rows of a fading class's steps, made from its own style row: glow and
/// light move together; the sheath of the Enhanced look follows them unless
/// the class's look came from its texture.
pub(in crate::online::combat) fn step_rows(base: &[f64; s::STRIDE], def: &Value, fade: &Fading) -> Vec<[f64; s::STRIDE]> {
    let look = |glow: [f64; 4], light: [f64; 4]| {
        let mut record = bfe_content_native::loader::authored_json(def);
        record["LaserGlowColor"] = serde_json::json!(glow);
        record["LightColor"] = serde_json::json!(light);
        let authored: style::Content = serde_json::from_value(record).unwrap_or_default();
        let row = style::row(&authored, true);
        [row[s::LOOK_COLOR], row[s::LOOK_COLOR + 1], row[s::LOOK_COLOR + 2]]
    };
    let own_look = look(fade.glow[0], fade.light[0]);
    let follows = (0..3).all(|i| (own_look[i] - base[s::LOOK_COLOR + i]).abs() < 1e-9);
    (1..=STEPS)
        .map(|step| {
            let along = step as f64 / STEPS as f64;
            let glow = mix(fade.glow[0], fade.glow[1], along);
            let light = mix(fade.light[0], fade.light[1], along);
            let mut row = *base;
            row[s::GLOW_COLOR..s::GLOW_COLOR + 3].copy_from_slice(&folded(glow));
            row[s::LIGHT_COLOR..s::LIGHT_COLOR + 3].copy_from_slice(&folded(light));
            if follows {
                row[s::LOOK_COLOR..s::LOOK_COLOR + 3].copy_from_slice(&look(glow, light));
            }
            row
        })
        .collect()
}

/// Prepare the steps of every fading class of the battle's combat registry
/// (`wire`, in registry order). Classes that do not fit the style table keep
/// their single colour.
pub(in crate::online::combat) fn install(combat: &mut Combat, wire: &[(String, Value)]) -> Vec<GlowFade> {
    let mut fades = Vec::new();
    for (i, (_, def)) in wire.iter().enumerate() {
        let Some(base) = combat.definitions.get(i + 1).copied().flatten() else { continue };
        let Some(fade) = fading(def) else { continue };
        if fades.iter().any(|f: &GlowFade| f.base == base) || combat.styles.len() + STEPS > limits::STYLES {
            continue;
        }
        let Some(row) = combat.ordnance.style_row(base) else { continue };
        let first = combat.styles.len();
        let own = &combat.styles[base];
        let (impact, explosion, trail, rail, fade_out_ticks) = (own.impact, own.explosion, own.trail, own.rail, own.fade_out_ticks);
        for (k, step) in step_rows(&row, def, &fade).into_iter().enumerate() {
            combat.ordnance.style_input[..s::STRIDE].copy_from_slice(&step);
            combat.ordnance.set_style(first + k);
            combat.styles.push(Style { model: None, impact, explosion, trail, rail, fade_out_ticks });
        }
        fades.push(GlowFade { base, first, delay: fade.delay, period: fade.period });
    }
    fades
}

/// Before the frame's bolts are drawn: every fading bolt takes the style of
/// its age on the display clock (`tick` for replicated bolts, `clock` seconds
/// for the player's own predicted ones).
pub(in crate::online::combat) fn apply(fades: &[GlowFade], tick: f64, clock: f64, bolts: &mut [Bolt], cosmetics: &mut [Cosmetic]) {
    if fades.is_empty() {
        return;
    }
    for bolt in bolts.iter_mut().filter(|b| b.active) {
        if let Some(fade) = fades.iter().find(|f| f.base == bolt.style) {
            bolt.style = fade.style((tick - bolt.flight) / 60.);
        }
    }
    for cosmetic in cosmetics.iter_mut() {
        if let Some(fade) = fades.iter().find(|f| f.base == cosmetic.style) {
            cosmetic.style = fade.style(clock - cosmetic.born);
        }
    }
}

/// After the draw: every bolt back on its class's own style.
pub(in crate::online::combat) fn restore(fades: &[GlowFade], bolts: &mut [Bolt], cosmetics: &mut [Cosmetic]) {
    if fades.is_empty() {
        return;
    }
    for style in bolts.iter_mut().map(|b| &mut b.style).chain(cosmetics.iter_mut().map(|c| &mut c.style)) {
        if let Some(base) = fades.iter().find_map(|f| f.base_of(*style)) {
            *style = base;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(text: &str) -> Value {
        bfe_content::json::parse(text).unwrap()
    }

    #[test]
    fn a_fading_bolt_shows_its_own_style_until_the_transition_then_steps_to_the_end() {
        let fade = GlowFade { base: 7, first: 40, delay: 0.3, period: 0.25 };
        assert_eq!(fade.style(0.), 7);
        assert_eq!(fade.style(0.3), 7, "not before the transition has begun");
        assert_eq!(fade.style(0.3001), 40, "step one as soon as it begins");
        assert_eq!(fade.style(0.425), 40 + STEPS / 2 - 1, "half way");
        assert_eq!(fade.style(0.55), 40 + STEPS - 1);
        assert_eq!(fade.style(4.), 40 + STEPS - 1, "held at the end colour");
        let sudden = GlowFade { period: 0., ..fade };
        assert_eq!(sudden.style(0.31), 40 + STEPS - 1, "no period: the end colour at once");
        assert_eq!(fade.base_of(40 + 5), Some(7));
        assert_eq!(fade.base_of(7), Some(7));
        assert_eq!(fade.base_of(40 + STEPS), None);
    }

    #[test]
    fn the_draw_sees_each_bolt_at_its_age_and_every_bolt_returns_to_its_class() {
        let fades = [GlowFade { base: 2, first: 10, delay: 0.25, period: 0.5 }];
        let bolt = |style, flight, active| Bolt { style, flight, active, ..Bolt::default() };
        // Display tick 90: flown 0.1 s, 0.5 s and 0.9 s; one of another class; one retired.
        let mut bolts = [bolt(2, 84., true), bolt(2, 60., true), bolt(2, 36., true), bolt(4, 60., true), bolt(2, 60., false)];
        apply(&fades, 90., 0., &mut bolts, &mut []);
        let shown: Vec<usize> = bolts.iter().map(|b| b.style).collect();
        assert_eq!(shown, vec![2, 10 + STEPS / 2 - 1, 10 + STEPS - 1, 4, 2]);
        restore(&fades, &mut bolts, &mut []);
        assert!(bolts.iter().enumerate().all(|(i, b)| b.style == if i == 3 { 4 } else { 2 }));
        apply(&[], 90., 0., &mut bolts, &mut []);
        assert_eq!(bolts[1].style, 2, "a battle without fading classes draws as before");
    }

    #[test]
    fn the_players_own_predicted_bolts_fade_on_the_frame_clock() {
        let fades = [GlowFade { base: 3, first: 20, delay: 0.1, period: 0.4 }];
        let shot = |style, born| Cosmetic {
            key: 1, style, definition: 0, tick: 0., channel: 0, origin: [0.; 3], direction: [0., 0., 1.],
            offset: [0.; 3], speed: 100., life: 2., born, until: 9., aimed: true, claimed: false, flown: 0., fade: 0.,
        };
        // Frame clock 5.0 s: flown 0.05 s, 0.3 s and 1.2 s; one of another class.
        let mut own = [shot(3, 4.95), shot(3, 4.7), shot(3, 3.8), shot(6, 4.7)];
        apply(&fades, 0., 5., &mut [], &mut own);
        let shown: Vec<usize> = own.iter().map(|c| c.style).collect();
        assert_eq!(shown, vec![3, 20 + STEPS / 2 - 1, 20 + STEPS - 1, 6]);
        restore(&fades, &mut [], &mut own);
        assert_eq!(own.iter().map(|c| c.style).collect::<Vec<_>>(), vec![3, 3, 3, 6]);
    }

    #[test]
    fn steps_move_the_glow_and_the_light_together_while_the_rest_of_the_row_stays() {
        let class = def(
            r#"{"base": "bolt", "FadeLightColor": "1", "LaserGlowColor": "200 100 40 255",
                "EndLaserGlowColor": "40 100 200 255", "LightColor": "10 20 30 255",
                "EndLightColor": "250 240 230 128", "DamageTransitionDelay": "0.3",
                "DamageTransitionPeriod": "0.25", "LaserWidth": "0.2", "GlowLength": "4"}"#,
        );
        let fade = fading(&class).unwrap();
        assert_eq!((fade.delay, fade.period), (0.3, 0.25));
        assert!(fading(&def(r#"{"FadeLightColor": "0", "LaserGlowColor": "1 2 3 4"}"#)).is_none());
        assert!(fading(&def(r#"{"LaserGlowColor": "1 2 3 4", "EndLaserGlowColor": "5 6 7 8"}"#)).is_none());
        let mut base = [0.; s::STRIDE];
        base[s::LASER_WIDTH] = 0.2;
        base[s::GLOW_COLOR..s::GLOW_COLOR + 3].copy_from_slice(&folded([200., 100., 40., 255.]));
        base[s::LIGHT_COLOR..s::LIGHT_COLOR + 3].copy_from_slice(&folded([10., 20., 30., 255.]));
        let rows = step_rows(&base, &class, &fade);
        assert_eq!(rows.len(), STEPS);
        let half = rows[STEPS / 2 - 1];
        let expect = folded([120., 100., 120., 255.]);
        for i in 0..3 {
            assert!((half[s::GLOW_COLOR + i] - expect[i]).abs() < 1e-12, "half way glow channel {i}");
        }
        let half_light = folded([130., 130., 130., 192.]);
        let step_one_light = folded([18., 27., 36., 251.]);
        let end = rows[STEPS - 1];
        let expect = folded([40., 100., 200., 255.]);
        let light = folded([250., 240., 230., 128.]);
        for i in 0..3 {
            assert!((half[s::LIGHT_COLOR + i] - half_light[i]).abs() < 1e-12, "half way light channel {i}");
            assert!((rows[0][s::LIGHT_COLOR + i] - step_one_light[i]).abs() < 1e-12, "the light starts moving, it does not jump");
            assert!((end[s::GLOW_COLOR + i] - expect[i]).abs() < 1e-12);
            assert!((end[s::LIGHT_COLOR + i] - light[i]).abs() < 1e-12, "the end light at the end");
        }
        assert_eq!(end[s::LASER_WIDTH], 0.2);
    }
}
