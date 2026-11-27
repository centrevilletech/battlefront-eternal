//! The 2005 loading presentation advances once per simulation tick. The map's
//! shared duration controls its end. Art, tip selection and readiness are local.

use crate::launch::GameFamily;
use jsrt::rng::ClassicRandom;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

pub mod prepared;

// Two quads per LED with a rim. Leave room in the 2048-quad batch for up
// to 1024 tip glyphs, both labels, the backdrop and the panel.
pub const MAX_LEDS: usize = 64;
pub const MAX_PICTURES: usize = 256;
pub const MAX_TIPS: usize = 512;
pub const TICKS_PER_SECOND: u32 = 60;

/// Which loading screen a battle runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadingScreenKind {
    /// The 2004 planet-tracking sequence (`crate::loading`).
    PlanetZoom,
    /// The 2005 map backdrop with tips and LED bar (this module).
    MapBackdrop,
}

impl LoadingScreenKind {
    pub fn sound_plan(
        self,
        planet: &crate::loading::LoadingTimeline,
    ) -> Vec<crate::loading::PlannedSound> {
        match self {
            Self::PlanetZoom => planet.sounds(),
            Self::MapBackdrop => Vec::new(),
        }
    }
}

/// The loading screen for a battle of `family`: the structure follows
/// the map's game, the theme only styles it (plan decision 6). This is
/// the one seam the family plumbing calls; until a 2005 battle can be
/// launched every caller passes the 2004 family.
pub fn loading_screen_for(family: GameFamily) -> LoadingScreenKind {
    match family {
        GameFamily::Swbf1 => LoadingScreenKind::PlanetZoom,
        GameFamily::Swbf2 => LoadingScreenKind::MapBackdrop,
    }
}

/// The LED progress bar's shape, from the config's `ProgressBar*`
/// values. Sizes and the gap between LEDs use the same
/// interface units in the 800 by 600 loading frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LedBar {
    /// Number of LEDs (`ProgressBarNumLEDs`, 12 in the retail data).
    pub count: usize,
    /// One LED's width (`ProgressBarLEDWidth`, 20).
    pub width: f64,
    /// One LED's height (`ProgressBarLEDHeight`, 20).
    pub height: f64,
    /// Gap between LEDs in interface units (`ProgressBarLEDSpacing`, 0.5).
    pub spacing: f64,
    /// Light level of an unlit LED (`ProgressBarLEDMinIntensity`, 0.3):
    /// the bar's LEDs are dim, never off.
    pub min_level: f64,
}

impl Default for LedBar {
    /// The retail config's bar.
    fn default() -> Self {
        Self {
            count: 12,
            width: 20.0,
            height: 20.0,
            spacing: 0.5,
            min_level: 0.3,
        }
    }
}

impl LedBar {
    /// Light level (`min_level..=1.0`) of LED `index` at bar fill
    /// `progress` (`0.0..=1.0`): filled LEDs are at the head's level,
    /// the LED being reached is between, the rest sit at `min_level`.
    pub fn level(&self, index: usize, progress: f64) -> f64 {
        let filled = progress.clamp(0.0, 1.0) * self.count.min(MAX_LEDS) as f64 - index as f64;
        self.min_level + (1.0 - self.min_level) * filled.clamp(0.0, 1.0)
    }

    /// Total width of the bar in interface units.
    pub fn total_width(&self) -> f64 {
        let count = self.count.min(MAX_LEDS);
        if count == 0 {
            0.0
        } else {
            count as f64 * self.width + (count - 1) as f64 * self.spacing
        }
    }
}

/// A duration from shared prepared map data, rounded up at preparation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapLoadTicks(NonZeroU32);

impl MapLoadTicks {
    pub fn new(ticks: u32) -> Option<Self> {
        NonZeroU32::new(ticks)
            .filter(|n| n.get() <= 600 * TICKS_PER_SECOND)
            .map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

/// Only the backdrop source needs to know how a load chooses its picture.
pub fn choose_loading_backdrop(pictures: usize, random: &mut ClassicRandom) -> Option<usize> {
    (pictures > 0).then(|| random.next_u32() as usize % pictures)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BackdropFrame {
    pub picture: Option<usize>,
    pub tip: Option<usize>,
    pub tick: u32,
    pub tick_fraction: f64,
    pub progress: f64,
}

/// A readiness wait has no picture, tip, blinking text or clock to animate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapLoadPhase {
    Showing(BackdropFrame),
    WaitingForReady,
    Ready,
}

pub struct MapLoadSequence {
    duration: MapLoadTicks,
    elapsed: u32,
    tip_ticks: NonZeroU32,
    picture: Option<usize>,
    tip: Option<usize>,
    deck: Vec<usize>,
    remaining: usize,
    random: ClassicRandom,
}

impl MapLoadSequence {
    /// The generator belongs to this presentation and is never shared with gameplay.
    pub fn new(
        duration: MapLoadTicks,
        tip_ticks: NonZeroU32,
        pictures: usize,
        tips: usize,
        seed: u32,
    ) -> Result<Self, &'static str> {
        if pictures > MAX_PICTURES || tips > MAX_TIPS {
            return Err("oversized loading selection");
        }
        let mut random = ClassicRandom::from_state(seed);
        let picture = choose_loading_backdrop(pictures, &mut random);
        let mut sequence = Self {
            duration,
            elapsed: 0,
            tip_ticks,
            picture,
            tip: None,
            deck: (0..tips).collect(),
            remaining: tips,
            random,
        };
        sequence.choose_tip();
        Ok(sequence)
    }

    fn choose_tip(&mut self) {
        if self.deck.is_empty() {
            return;
        }
        if self.remaining == 0 {
            self.remaining = self.deck.len();
        }
        let index = self.random.next_u32() as usize % self.remaining;
        self.remaining -= 1;
        self.deck.swap(index, self.remaining);
        self.tip = Some(self.deck[self.remaining]);
    }

    /// Called by the fixed-step clock, including ticks between two display frames.
    pub fn tick(&mut self) {
        if self.elapsed == self.duration.get() {
            return;
        }
        self.elapsed += 1;
        if self.elapsed < self.duration.get() && self.elapsed.is_multiple_of(self.tip_ticks.get()) {
            self.choose_tip();
        }
    }

    /// The render clock may interpolate the lights and label between simulation ticks.
    pub fn phase(&self, ready: bool, tick_fraction: f64) -> MapLoadPhase {
        if self.elapsed == self.duration.get() {
            return if ready {
                MapLoadPhase::Ready
            } else {
                MapLoadPhase::WaitingForReady
            };
        }
        let fraction = if tick_fraction.is_finite() {
            tick_fraction.clamp(0., 1.)
        } else {
            0.
        };
        MapLoadPhase::Showing(BackdropFrame {
            picture: self.picture,
            tip: self.tip,
            tick: self.elapsed,
            tick_fraction: fraction,
            progress: (f64::from(self.elapsed) + fraction) / f64::from(self.duration.get()),
        })
    }
}

/// Triangle-wave blink (`0.0..=1.0`, one cycle per `1 / rate` seconds)
/// for the loading text, from the config's `LoadingTextBlinkRate`.
pub fn blink(t: f64, rate: f64) -> f64 {
    if !(rate.is_finite() && rate > 0.) {
        return 1.;
    }
    let phase = t.max(0.) * rate;
    if !phase.is_finite() {
        return 0.;
    }
    let u = phase.fract() * 2.;
    if u < 1.0 {
        u
    } else {
        2.0 - u
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_word_stays_lit_and_unbounded_time_stays_finite() {
        for time in [0., 0.5, 5., f64::INFINITY] {
            assert_eq!(blink(time, 0.), 1.);
        }
        for time in [f64::INFINITY, f64::NAN, f64::MAX] {
            assert!((0. ..=1.).contains(&blink(time, 4.)));
        }
    }

    #[test]
    fn the_family_picks_the_screen() {
        assert_eq!(
            loading_screen_for(GameFamily::Swbf1),
            LoadingScreenKind::PlanetZoom
        );
        assert_eq!(
            loading_screen_for(GameFamily::Swbf2),
            LoadingScreenKind::MapBackdrop
        );
    }

    #[test]
    fn mixer_receives_only_the_selected_screens_plan() {
        let planet = crate::loading::LoadingTimeline::new(
            3.5,
            1.,
            0.,
            vec![Some([0.25, 0.25, 0.5, 0.5]); 4],
        );
        let full = planet.sounds();
        assert_eq!(full.len(), 12);
        assert_eq!(
            loading_screen_for(GameFamily::Swbf1).sound_plan(&planet),
            full
        );
        assert!(loading_screen_for(GameFamily::Swbf2)
            .sound_plan(&planet)
            .is_empty());
    }

    #[test]
    fn deadline_is_independent_of_presentation_and_readiness() {
        let duration = MapLoadTicks::new(660).unwrap();
        for (pictures, tips, seed, redraw_every) in [(0, 0, 0, 1), (1, 1, 7, 4), (13, 54, 19, 900)]
        {
            let mut load =
                MapLoadSequence::new(duration, NonZeroU32::new(60).unwrap(), pictures, tips, seed)
                    .unwrap();
            for tick in 0..660 {
                let MapLoadPhase::Showing(frame) = load.phase(true, 0.) else {
                    panic!("early handover")
                };
                assert_eq!(frame.tick, tick);
                assert_eq!(frame.progress, f64::from(tick) / 660.);
                if tick % redraw_every == 0 {
                    // Sampling between ticks can neither shorten nor extend the sequence.
                    assert_eq!(load.phase(false, 0.), MapLoadPhase::Showing(frame));
                    let MapLoadPhase::Showing(between) = load.phase(true, 0.75) else {
                        unreachable!()
                    };
                    assert_eq!(between.tick, tick);
                    assert_eq!(between.picture, frame.picture);
                    assert_eq!(between.tip, frame.tip);
                    assert_eq!(between.progress, (f64::from(tick) + 0.75) / 660.);
                    assert_eq!(load.phase(false, f64::NAN), MapLoadPhase::Showing(frame));
                }
                load.tick();
            }
            assert_eq!(load.phase(false, 0.), MapLoadPhase::WaitingForReady);
            for _ in 0..1200 {
                load.tick();
            }
            assert_eq!(load.phase(false, 0.), MapLoadPhase::WaitingForReady);
            assert_eq!(load.phase(true, 0.), MapLoadPhase::Ready);
        }
        assert!(MapLoadTicks::new(0).is_none());
    }

    #[test]
    fn one_backdrop_and_nonrepeating_tips_per_load() {
        let mut pictures = std::collections::BTreeSet::new();
        for seed in 0..16 {
            let mut load = MapLoadSequence::new(
                MapLoadTicks::new(180).unwrap(),
                NonZeroU32::new(10).unwrap(),
                13,
                3,
                seed,
            )
            .unwrap();
            let MapLoadPhase::Showing(start) = load.phase(false, 0.) else {
                unreachable!()
            };
            pictures.insert(start.picture.unwrap());
            let mut tips = std::collections::BTreeSet::new();
            let mut current = start.tip;
            for tick in 0..180 {
                let MapLoadPhase::Showing(frame) = load.phase(false, 0.) else {
                    panic!("early handover")
                };
                assert_eq!(frame.picture, start.picture);
                if tick % 10 == 0 {
                    if tick % 30 == 0 {
                        tips.clear();
                    }
                    assert!(tips.insert(frame.tip.unwrap()));
                    current = frame.tip;
                }
                assert_eq!(frame.tip, current);
                load.tick();
            }
        }
        assert!(pictures.len() > 1);
    }

    #[test]
    fn the_led_bar_fills_and_rests_dim() {
        let bar = LedBar::default();
        assert_eq!(bar.count, 12);
        assert_eq!(bar.level(0, 0.0), bar.min_level);
        assert_eq!(bar.level(11, 0.0), bar.min_level);
        assert_eq!(bar.level(0, 1.0), 1.0);
        assert_eq!(bar.level(11, 1.0), 1.0);
        // Half full: LED 5 is the last lit, LED 6 rests dim.
        assert_eq!(bar.level(5, 0.5), 1.0);
        assert_eq!(bar.level(6, 0.5), bar.min_level);
        assert!(bar.total_width() > 0.0);
        let empty = LedBar {
            count: 0,
            ..LedBar::default()
        };
        assert_eq!(empty.total_width(), 0.0);
    }

    #[test]
    fn blink_is_a_triangle_wave() {
        assert_eq!(blink(0.0, 1.0), 0.0);
        assert_eq!(blink(0.25, 1.0), 0.5);
        assert_eq!(blink(0.5, 1.0), 1.0);
        assert_eq!(blink(1.0, 1.0), 0.0);
        assert_eq!(blink(0.5, 0.0), 1.0);
    }
}
