//! Loading timeline, tracking crosshairs, zoom view, rotating disk, 28 LEDs,
//! and sound plan (`src/shell/loading.js` and `src/shell/loading-audio.js`).
//!
//! This module ports `src/shell/loading.js` and `src/shell/loading-audio.js`:
//! - `STEPS = { data: 0.3, world: 0.5, shaders: 0.85, ready: 1.0 }`,
//!   `STEP = 0.05`, `STILL = 0.001`, `LEAD = 0.1`, `SEQUENCE_CAP = 30000`,
//!   `LOAD_CAP = 60000`.
//! - `approach(t, h, step)`: continuous integral of the original 2004 game's
//!   stepwise crosshair tracking `(1 - j * step / h)`.
//! - `pulse(travel, h, step, still)`: `[from, to]` interval within a half-track
//!   while the leading line moves more than `STILL` of the screen per step.
//! - `LoadingTimeline`: pure clock timeline (`at(t)`, `sounds()`, `cue(t)`).
//! - `zoomView(u, iw, ih, target)` and `onScreen(rect, view, w, h)`: geometric
//!   scale interpolation into the next stage's rectangle.
//! - `diskPlace(w, h)`, `lightLevel(behind, ...)`, `mixColor(stops, level)`,
//!   `loadingLook(classic_menus, theme)`, `planetName(map, strings, load)`.

use crate::data::MapLoad;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Progress weights for each step of battle loading (`createGame`'s `onProgress`).
/// Mirrors `STEPS` in `src/shell/loading.js`.
pub mod steps {
    pub const DATA: f64 = 0.3;
    pub const WORLD: f64 = 0.5;
    pub const SHADERS: f64 = 0.85;
    pub const READY: f64 = 1.0;

    /// Maps a named loading step (`"data"`, `"world"`, `"shaders"`, `"ready"`)
    /// to its progress fraction.
    pub fn weight(step: &str) -> Option<f64> {
        match step {
            "data" => Some(DATA),
            "world" => Some(WORLD),
            "shaders" => Some(SHADERS),
            "ready" => Some(READY),
            _ => None,
        }
    }
}

/// The original redraws the loading screen at most every `50ms`; its lines move
/// and its tracking pulse starts and stops on that step.
/// Mirrors `STEP` in `src/shell/loading.js`.
pub const STEP: f64 = 0.05;

/// A tracking line is considered still once it moves less than a thousandth of
/// the screen in one [`STEP`].
/// Mirrors `STILL` in `src/shell/loading.js`.
pub const STILL: f64 = 0.001;

/// Lead time in seconds between picking the sequence start and starting playback.
/// Mirrors `LEAD` in `src/shell/loading.js`.
pub const LEAD: f64 = 0.1;

/// Interface coordinate width (`1024`) used by the loading screen and HUD.
/// Mirrors `IW` in `src/shell/loading.js`.
pub const IW: f64 = 1024.0;

/// Maximum time in milliseconds the loading screen may outlast a ready battle
/// waiting for its own animation sequence.
/// Mirrors `SEQUENCE_CAP` in `src/shell/loading.js`.
pub const SEQUENCE_CAP: u64 = 30_000;

/// Maximum time in milliseconds to wait for battle asset loading.
/// Mirrors `LOAD_CAP` in `src/shell/loading.js`.
pub const LOAD_CAP: u64 = 60_000;

/// Geometry and animation constants for the bottom-right rotating `LOADING` disk
/// and 28-LED ring in `1024x768` interface units.
/// Mirrors `DISK` in `src/shell/loading.js`.
pub mod disk {
    /// Horizontal offset (`252.0` units) of the disk center from the right screen edge.
    pub const RIGHT: f64 = 252.0;
    /// Vertical offset (`223.0` units) of the disk center from the bottom screen edge.
    pub const BOTTOM: f64 = 223.0;
    /// Horizontal perspective foreshortening factor (`0.866`, i.e. `cos(30 deg)`).
    pub const TURN: f64 = 0.866;
    /// Radius (`52.0` units) of the 28-LED ring around the disk center.
    pub const RING: f64 = 52.0;
    /// Number of LED squares (`28`) arranged evenly around the ring.
    pub const LEDS: usize = 28;
    /// Counter-clockwise tilt (`-14.0` degrees) of the `LOADING` label across the disk.
    pub const TEXT_DEG: f64 = -14.0;
    /// `[size, turn_rad_per_sec, alpha]` for the two counter-rotating disk layers;
    /// each layer's thin ring (at a quarter of its size) lands at `60` and `44` units,
    /// outside and inside the `52`-unit LED ring.
    pub const LAYERS: [[f64; 3]; 2] = [[240.0, 0.25, 0.85], [176.0, -0.4, 0.75]];
}

/// Top-left planet name coordinates in `1024xH` interface units (`5` units into
/// the safe area's top-left corner).
/// Mirrors `NAME` in `src/shell/loading.js`.
pub mod name_pos {
    /// Left X anchor (`51.2` units, `0.05 * 1024`) at the safe area's left margin.
    pub const X: f64 = 51.2;
    /// Top safe-area fraction (`0.05`) of interface height `H`.
    pub const TOP: f64 = 0.05;
    /// Vertical offset (`5.0` units) below the top safe-area boundary.
    pub const Y: f64 = 5.0;
}

/// Parameters of the 28-LED lit arc chasing around the loading ring.
/// Mirrors `LIGHTS` in `src/shell/loading.js`.
pub mod lights {
    /// Angular speed of the lit arc's head in full turns per second (`0.6`).
    pub const SPEED: f64 = 0.6;
    /// Smoothstep lead-in ramp (`1 / 28` of a turn, one LED spacing) ahead of the head.
    pub const RAMP: f64 = 1.0 / 28.0;
    /// Fraction of the ring (`0.75` turns) over which the trailing tail decays to off.
    pub const TAIL: f64 = 0.75;
    /// Power-law exponent (`2.0`) shaping the tail decay (`(1 - b / tail) ^ ease`).
    pub const EASE: f64 = 2.0;
}

/// Sound constants from `src/shell/loading-audio.js` (`SOUNDS`, `BUS`, `AV`, `LATE`).
pub mod audio_constants {
    /// `core.lvl`'s `shellfx` bus gain (`0.3`).
    /// Mirrors `BUS` in `src/shell/loading-audio.js`.
    pub const BUS: f64 = 0.3;
    /// Audio-visual offset (`1 / 30` s): a sound is heard one or two refreshes
    /// after its frame moment. Mirrors `AV` in `src/shell/loading-audio.js`.
    pub const AV: f64 = 1.0 / 30.0;
    /// Maximum lateness (`0.05` s) before a one-shot zoom sound is dropped.
    /// Mirrors `LATE` in `src/shell/loading-audio.js`.
    pub const LATE: f64 = 0.05;
    /// Fade-out duration (`0.01` s) when a tracking pulse stops.
    pub const STOP: f64 = 0.01;
    /// Fade-out duration (`0.08` s) when the loading screen closes.
    pub const FADE: f64 = 0.08;

    /// Maps a [`super::LoadingCue`] to its `core.lvl` sound name.
    /// Mirrors `SOUNDS` in `src/shell/loading-audio.js`.
    pub fn sound_name(cue: super::LoadingCue) -> &'static str {
        match cue {
            super::LoadingCue::YTrack => "load_ytracking",
            super::LoadingCue::XTrack => "load_xtracking",
            super::LoadingCue::Zoom => "load_zoom",
            super::LoadingCue::Transition => "load_transition",
        }
    }
}

/// Theme look identifier for the loading screen (`"bf1"`, `"eternal"`, `"bf2"`).
/// Mirrors the keys of `LOOKS` in `src/shell/loading.js`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LookId {
    /// Original 2004 look (`"bf1"`): grey lights on cyan disk, bitmap fonts.
    Bf1,
    /// Battlefront Eternal look (`"eternal"`): amber/ember lights on black glass.
    #[default]
    Eternal,
    /// Battlefront II 2005 look (`"bf2"`): red lights with dark rim on navy glass.
    Bf2,
}

impl LookId {
    /// Returns the lowercase theme identifier (`"bf1"`, `"eternal"`, or `"bf2"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bf1 => "bf1",
            Self::Eternal => "eternal",
            Self::Bf2 => "bf2",
        }
    }

    /// Normalizes an optional `menuTheme` setting into a [`LookId`], defaulting
    /// any unrecognized or absent value to [`LookId::Eternal`] (`themeOf` in JS).
    pub fn from_theme(theme: Option<&str>) -> Self {
        match theme {
            Some("bf1") => Self::Bf1,
            Some("bf2") => Self::Bf2,
            _ => Self::Eternal,
        }
    }

    /// Returns the hex color stops for the 28 ring lights in this look.
    /// Mirrors `LOOKS[id].lights` in `src/shell/loading.js`.
    pub fn light_stops(self) -> &'static [&'static str] {
        match self {
            Self::Bf1 => &["#0b0d0f", "#5c6166", "#b9bcbe"],
            Self::Eternal => &["#15100c", "#4a2407", "#b5560b", "#ff9a1f", "#ffd27a"],
            Self::Bf2 => &["#101826", "#3d0c08", "#a5180e", "#e0362a", "#ff6450"],
        }
    }

    /// Optional rim color around each LED (`"#06080c"` on `bf2`, `None` otherwise).
    /// Mirrors `LOOKS[id].edge` in `src/shell/loading.js`.
    pub fn light_edge(self) -> Option<[u8; 4]> {
        match self {
            Self::Bf2 => Some([6, 8, 12, 255]),
            Self::Bf1 | Self::Eternal => None,
        }
    }

    /// Color of the `LOADING` label (`LOOKS[id].text`).
    pub fn text_color(self) -> [u8; 4] {
        match self {
            Self::Bf1 => [220, 220, 255, 255],
            Self::Eternal => [232, 236, 242, 255],
            Self::Bf2 => [255, 255, 255, 255],
        }
    }
}

/// Resolves which loading screen look (`"bf1"`, `"eternal"`, `"bf2"`) applies.
/// Under classic menus (`classic_menus == true`), always returns `LookId::Bf1`;
/// otherwise normalizes `theme` (`"bf1"`, `"bf2"`, or default `"eternal"`).
///
/// Mirrors `loadingLook(params, theme)` in `src/shell/loading.js`.
pub fn loading_look(classic_menus: bool, theme: Option<&str>) -> LookId {
    if classic_menus {
        LookId::Bf1
    } else {
        LookId::from_theme(theme)
    }
}

/// Three-letter map prefix table `(prefix, localization_slug, fallback_display_name)`
/// used by [`planet_name`] when `shell.json`'s `load.planet` does not supply a name.
/// Mirrors `PLANETS` in `src/shell/loading.js`.
const PLANETS: [(&str, &str, &str); 10] = [
    ("bes", "bespin", "BESPIN"),
    ("end", "endor", "ENDOR"),
    ("geo", "geonosis", "GEONOSIS"),
    ("hot", "hoth", "HOTH"),
    ("kam", "kamino", "KAMINO"),
    ("kas", "kashyyyk", "KASHYYYK"),
    ("nab", "naboo", "NABOO"),
    ("rhn", "rhenvar", "RHEN VAR"),
    ("tat", "tatooine", "TATOOINE"),
    ("yav", "yavin", "YAVIN 4"),
];

/// Resolves the planet display name for `map` (`"geo1"` -> `"GEONOSIS"`,
/// `"yav1"` -> `"YAVIN 4"`), preferring `load.planet` or
/// `strings["planets.<planet>.name"]` when present.
///
/// Mirrors `planetName(map, strings, load)` in `src/shell/loading.js`.
pub fn planet_name(
    map: &str,
    strings: &BTreeMap<String, String>,
    load: Option<&MapLoad>,
) -> String {
    if let Some(p) = load.and_then(|l| l.planet.as_deref()).filter(|s| !s.is_empty()) {
        return p.to_string();
    }
    let prefix: String = map.chars().take(3).collect();
    if let Some((_, key_part, fallback)) = PLANETS.iter().find(|(p, _, _)| *p == prefix) {
        let key = format!("planets.{key_part}.name");
        if let Some(s) = strings.get(&key).filter(|s| !s.is_empty()) {
            return s.clone();
        }
        return (*fallback).to_string();
    }
    map.to_ascii_uppercase()
}

/// Fraction of a tracking line's travel remaining `t` seconds into a half-track
/// of `h` seconds.
///
/// Every `step` (`0.05s`) the 2004 game moves each line `t / h` of what is left,
/// so the remaining distance after `k` steps is the product of `(1 - j * step / h)`.
/// This function evaluates the smooth log-integral of that product.
///
/// Mirrors `approach(t, h, step)` in `src/shell/loading.js`.
pub fn approach(t: f64, h: f64, step: f64) -> f64 {
    if t <= 0.0 {
        return 1.0;
    }
    if t + step * 0.5 >= h {
        return 0.0;
    }
    let f = |s: f64| -(h - s) * (1.0 - s / h).ln() - s;
    ((f(t + step * 0.5) - f(step * 0.5)) / step).exp()
}

/// Computes the time window `[from, to]` (in seconds into a half-track of `h`
/// seconds) during which a tracking line with total travel `travel` moves more
/// than `still` per `step`. Returns `None` if the line is too close to move.
///
/// Mirrors `pulse(travel, h, step, still)` in `src/shell/loading.js`.
pub fn pulse(travel: f64, h: f64, step: f64, still: f64) -> Option<[f64; 2]> {
    let steps = (h / step - 1e-9).ceil() as usize;
    let mut from: Option<f64> = None;
    for k in 1..=steps {
        let t0 = ((k - 1) as f64) * step;
        let t1 = ((k as f64) * step).min(h);
        let moved = travel.abs() * (approach(t0, h, step) - approach(t1, h, step));
        if moved > still && from.is_none() {
            from = Some(t0);
        } else if moved <= still {
            if let Some(start) = from {
                return Some([start, t0]);
            }
        }
    }
    from.map(|start| [start, h])
}

/// Phase of the loading sequence at time `t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadingPhase {
    /// Initial state before `go(start)` starts the clock (`TimelineAt::WAIT`).
    Wait,
    /// Crosshair tracking phase (`3.5s` default: horizontal in first half, vertical in second).
    Track,
    /// Exponential zoom transition (`1.0s` default) into the next stage's target crop.
    Zoom,
    /// Final stage reached after the last zoom finishes (`done` once `hold` elapses).
    Hold,
}

impl LoadingPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wait => "wait",
            Self::Track => "track",
            Self::Zoom => "zoom",
            Self::Hold => "hold",
        }
    }
}

/// Normalized position (`0.0` at screen edges to `1.0` on the target rectangle)
/// of the horizontal (`y`) and vertical (`x`) crosshair pairs during `Track`
/// and `Zoom`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackingLines {
    /// Horizontal crosshair progress (`0.0` at top/bottom screen edges to `1.0` on target box).
    pub y: f64,
    /// Vertical crosshair progress (`None` during the first half of `track`, then `0.0..=1.0`).
    pub x: Option<f64>,
}

/// Snapshot of [`LoadingTimeline::at`] at `t` seconds into the sequence.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineAt {
    /// Zero-based index of the current stage picture (`0..stages`).
    pub stage: usize,
    /// Active phase (`Wait`, `Track`, `Zoom`, or `Hold`) at time `t`.
    pub phase: LoadingPhase,
    /// Elapsed time in seconds within the current phase.
    pub time: f64,
    /// Normalized crosshair positions (`0.0..=1.0`) during `Track` and `Zoom`, or `None` in `Wait`/`Hold`.
    pub lines: Option<TrackingLines>,
    /// Normalized zoom progress (`0.0..=1.0`) during `Zoom`, or `None` in other phases.
    pub zoom: Option<f64>,
    /// Whether the entire sequence (including `hold` after the final zoom) has completed.
    pub done: bool,
}

impl TimelineAt {
    /// Before the sequence starts: stage 0, `"wait"`, still.
    /// Mirrors `WAIT` in `src/shell/loading.js`.
    pub const WAIT: Self = Self {
        stage: 0,
        phase: LoadingPhase::Wait,
        time: 0.0,
        lines: None,
        zoom: None,
        done: false,
    };
}

/// Sound cue kind in [`LoadingTimeline::sounds`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadingCue {
    /// Horizontal crosshair tracking loop (`"ytrack"` -> `"load_ytracking"`).
    YTrack,
    /// Vertical crosshair tracking loop (`"xtrack"` -> `"load_xtracking"`).
    XTrack,
    /// One-shot zoom start sound (`"zoom"` -> `"load_zoom"`).
    Zoom,
    /// End-of-zoom transition marker (`"transition"` -> `"load_transition"`).
    Transition,
}

impl LoadingCue {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::YTrack => "ytrack",
            Self::XTrack => "xtrack",
            Self::Zoom => "zoom",
            Self::Transition => "transition",
        }
    }
}

/// Planned sound event `{ cue, at, until }` in seconds from sequence start
/// (`until == None` for one-shots).
/// Mirrors the entries returned by `LoadingTimeline.sounds()` in `src/shell/loading.js`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlannedSound {
    /// Which loading sound cue (`YTrack`, `XTrack`, `Zoom`, or `Transition`) is scheduled.
    pub cue: LoadingCue,
    /// Start offset in seconds from the beginning of the loading sequence.
    pub at: f64,
    /// End offset in seconds for looping tracking pulses (`None` for one-shot `Zoom`/`Transition`).
    pub until: Option<f64>,
}

/// Pure clock-driven loading sequence timeline (`track`, `zoom`, `hold`,
/// `targets`).
///
/// Mirrors `LoadingTimeline` in `src/shell/loading.js`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoadingTimeline {
    pub track: f64,
    pub zoom: f64,
    pub hold: f64,
    /// For each stage picture, its target rectangle `[x0, y0, x1, y1]` as
    /// fractions of the picture (`None` for the last stage).
    pub targets: Vec<Option<[f64; 4]>>,
}

impl Default for LoadingTimeline {
    fn default() -> Self {
        Self::new(3.5, 1.0, 0.0, Vec::new())
    }
}

impl LoadingTimeline {
    /// Constructs a `LoadingTimeline` with `track` (clamped to `>= STEP * 2`),
    /// `zoom` (clamped to `>= 0.05`), `hold` (clamped to `>= 0.0`), and
    /// fractional stage `targets`.
    pub fn new(track: f64, zoom: f64, hold: f64, targets: Vec<Option<[f64; 4]>>) -> Self {
        let track = if track.is_finite() && track > 0.0 {
            track.max(STEP * 2.0)
        } else {
            STEP * 2.0
        };
        let zoom = if zoom.is_finite() && zoom > 0.0 {
            zoom.max(0.05)
        } else {
            0.05
        };
        let hold = if hold.is_finite() && hold > 0.0 {
            hold
        } else {
            0.0
        };
        Self {
            track,
            zoom,
            hold,
            targets,
        }
    }

    /// Number of stage pictures in the sequence.
    /// Mirrors `get stages()` in `src/shell/loading.js`.
    pub fn stages(&self) -> usize {
        self.targets.len()
    }

    /// Total duration of the sequence in seconds:
    /// `max(stages - 1, 0) * (track + zoom) + hold`.
    /// Mirrors `get duration()` in `src/shell/loading.js`.
    pub fn duration(&self) -> f64 {
        let transitions = self.stages().saturating_sub(1) as f64;
        transitions * (self.track + self.zoom) + self.hold
    }

    /// Evaluates the sequence state `t` seconds after start.
    /// Mirrors `LoadingTimeline.at(t)` in `src/shell/loading.js`.
    pub fn at(&self, t: f64) -> TimelineAt {
        let t = if t.is_finite() && t > 0.0 { t } else { 0.0 };
        let cycle = self.track + self.zoom;
        let last = self.stages().saturating_sub(1);
        let last_end = (last as f64) * cycle;
        if t >= last_end {
            return TimelineAt {
                stage: last,
                phase: LoadingPhase::Hold,
                time: t - last_end,
                lines: None,
                zoom: None,
                done: t >= self.duration(),
            };
        }
        let stage = (t / cycle).floor() as usize;
        let time = t - (stage as f64) * cycle;
        if time >= self.track {
            let z = ((time - self.track) / self.zoom).clamp(0.0, 1.0);
            return TimelineAt {
                stage,
                phase: LoadingPhase::Zoom,
                time: time - self.track,
                lines: Some(TrackingLines {
                    y: 1.0,
                    x: Some(1.0),
                }),
                zoom: Some(z),
                done: false,
            };
        }
        let half = self.track * 0.5;
        let y = 1.0 - approach(time, half, STEP);
        let x = if time < half {
            None
        } else {
            Some(1.0 - approach(time - half, half, STEP))
        };
        TimelineAt {
            stage,
            phase: LoadingPhase::Track,
            time,
            lines: Some(TrackingLines { y, x }),
            zoom: None,
            done: false,
        }
    }

    /// Builds the full sound plan `[{ cue, at, until }]` in seconds from sequence
    /// start.
    /// Mirrors `LoadingTimeline.sounds()` in `src/shell/loading.js`.
    pub fn sounds(&self) -> Vec<PlannedSound> {
        let mut out = Vec::new();
        let cycle = self.track + self.zoom;
        let half = self.track * 0.5;
        let transitions = self.stages().saturating_sub(1);
        for n in 0..transitions {
            let base = (n as f64) * cycle;
            let [x0, y0, _, _] = self.targets.get(n).and_then(|t| *t).unwrap_or([0.0; 4]);
            if let Some([y_from, y_to]) = pulse(y0, half, STEP, STILL) {
                out.push(PlannedSound {
                    cue: LoadingCue::YTrack,
                    at: base + y_from,
                    until: Some(base + y_to),
                });
            }
            if let Some([x_from, x_to]) = pulse(x0, half, STEP, STILL) {
                out.push(PlannedSound {
                    cue: LoadingCue::XTrack,
                    at: base + half + x_from,
                    until: Some(base + half + x_to),
                });
            }
            out.push(PlannedSound {
                cue: LoadingCue::Zoom,
                at: base + self.track,
                until: None,
            });
            out.push(PlannedSound {
                cue: LoadingCue::Transition,
                at: base + cycle,
                until: None,
            });
        }
        out
    }

    /// Returns which sound cue is active at `t` seconds (`"ytrack"`, `"xtrack"`,
    /// `"zoom"`, or `"still"`).
    /// Mirrors `LoadingTimeline.cue(t, sounds)` in `src/shell/loading.js`.
    pub fn cue(&self, t: f64, sounds: Option<&[PlannedSound]>) -> &'static str {
        let owned;
        let list = match sounds {
            Some(s) => s,
            None => {
                owned = self.sounds();
                &owned
            }
        };
        if let Some(loop_sound) = list
            .iter()
            .find(|s| s.until.is_some_and(|u| t >= s.at && t < u))
        {
            return loop_sound.cue.as_str();
        }
        if self.at(t).phase == LoadingPhase::Zoom {
            "zoom"
        } else {
            "still"
        }
    }
}

/// Computes the source crop `[vx, vy, vw, vh]` of an `iw x ih` picture at zoom
/// fraction `u` (`0.0..=1.0`) into `target` (`[tx, ty, tw, th]`): the size
/// shrinks at an even exponential rate of scale, and the center moves in step
/// with the size.
///
/// Mirrors `zoomView(u, iw, ih, target)` in `src/shell/loading.js`.
pub fn zoom_view(u: f64, iw: f64, ih: f64, target: [f64; 4]) -> [f64; 4] {
    let [tx, ty, tw, th] = target;
    let w = (iw.ln() + (tw.max(1.0).ln() - iw.ln()) * u).exp();
    let h = (ih.ln() + (th.max(1.0).ln() - ih.ln()) * u).exp();
    let k = if (iw - tw).abs() < f64::EPSILON {
        u
    } else {
        (iw - w) / (iw - tw)
    };
    let cx = iw * 0.5 + (tx + tw * 0.5 - iw * 0.5) * k;
    let cy = ih * 0.5 + (ty + th * 0.5 - ih * 0.5) * k;
    [cx - w * 0.5, cy - h * 0.5, w, h]
}

/// Maps a rectangle `[x, y, rw, rh]` in picture coordinates onto a `w x h`
/// screen when `view` (`[vx, vy, vw, vh]`) fills the screen.
///
/// Mirrors `onScreen(rect, view, w, h)` in `src/shell/loading.js`.
pub fn on_screen(rect: [f64; 4], view: [f64; 4], w: f64, h: f64) -> [f64; 4] {
    let [x, y, rw, rh] = rect;
    let [vx, vy, vw, vh] = view;
    [
        ((x - vx) / vw) * w,
        ((y - vy) / vh) * h,
        (rw / vw) * w,
        (rh / vh) * h,
    ]
}

/// Placement `{ x, y, unit }` of the rotating `LOADING` disk on a `w x h` screen.
/// Mirrors the object returned by `diskPlace(w, h)` in `src/shell/loading.js`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskPlace {
    /// Horizontal center of the disk in viewport pixels (`w - 252 * unit`).
    pub x: f64,
    /// Vertical center of the disk in viewport pixels (`h - 223 * unit`).
    pub y: f64,
    /// Uniform pixels-per-interface-unit scale factor (`min(w / 1024, h / 768)`).
    pub unit: f64,
}

/// Computes where the `LOADING` disk sits on a `w x h` screen: `unit = min(w / 1024, h / 768)`,
/// centered `(252 * unit, 223 * unit)` from the bottom-right corner.
///
/// Mirrors `diskPlace(w, h)` in `src/shell/loading.js`.
pub fn disk_place(w: f64, h: f64) -> DiskPlace {
    let unit = (w / IW).min(h / 768.0);
    DiskPlace {
        x: w - disk::RIGHT * unit,
        y: h - disk::BOTTOM * unit,
        unit,
    }
}

/// Computes how lit a ring LED is (`0.0` off to `1.0` at the head) when it sits
/// `behind` turns behind the lit arc's head, using the default [`lights`]
/// parameters (`ramp = 1/28`, `tail = 0.75`, `ease = 2`).
///
/// Mirrors `lightLevel(behind)` in `src/shell/loading.js`.
pub fn light_level(behind: f64) -> f64 {
    light_level_with(behind, lights::RAMP, lights::TAIL, lights::EASE)
}

/// Computes `lightLevel(behind, { ramp, tail, ease })` with explicit parameters.
pub fn light_level_with(behind: f64, ramp: f64, tail: f64, ease: f64) -> f64 {
    let b = behind - behind.floor();
    if b > 1.0 - ramp {
        let u = (b - (1.0 - ramp)) / ramp;
        return u * u * (3.0 - 2.0 * u);
    }
    if b < tail {
        (1.0 - b / tail).powf(ease)
    } else {
        0.0
    }
}

/// Parses a `"#rrggbb"` hex stop into `[r, g, b]`.
fn parse_hex_stop(hex: &str) -> [u8; 3] {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return [0, 0, 0];
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    [r, g, b]
}

/// Interpolates `level` (`0.0..=1.0`) across evenly spaced `"#rrggbb"` color
/// stops and returns the RGB triple `[r, g, b]`.
pub fn mix_color_rgb(stops: &[&str], level: f64) -> [u8; 3] {
    if stops.is_empty() {
        return [0, 0, 0];
    }
    if stops.len() == 1 {
        return parse_hex_stop(stops[0]);
    }
    let at = level.clamp(0.0, 1.0) * ((stops.len() - 1) as f64);
    let i = (at.floor() as usize).min(stops.len() - 2);
    let a = parse_hex_stop(stops[i]);
    let b = parse_hex_stop(stops[i + 1]);
    let frac = at - (i as f64);
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * frac).round() as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * frac).round() as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * frac).round() as u8,
    ]
}

/// Formats [`mix_color_rgb`] as `"rgb(r, g, b)"`, matching `mixColor(stops, level)`
/// in `src/shell/loading.js`.
pub fn mix_color(stops: &[&str], level: f64) -> String {
    let [r, g, b] = mix_color_rgb(stops, level);
    format!("rgb({r}, {g}, {b})")
}

/// Checks whether the loading screen is ready to hand over to the battle at
/// `now_ms`: requires both the battle load to have settled (or reached
/// `load_cap_ms`) and the animation sequence to be done (or `sequence_cap_ms`
/// elapsed since `ready_at_ms`).
///
/// Mirrors the completion rule of `handover` in `src/shell/loading.js`.
pub fn handover_ready(
    battle_loaded: bool,
    sequence_done: bool,
    started_at_ms: u64,
    ready_at_ms: Option<u64>,
    now_ms: u64,
    load_cap_ms: u64,
    sequence_cap_ms: u64,
) -> bool {
    let load_settled = battle_loaded || now_ms.saturating_sub(started_at_ms) >= load_cap_ms;
    if !load_settled {
        return false;
    }
    if sequence_done {
        return true;
    }
    if let Some(ready_ms) = ready_at_ms {
        now_ms.saturating_sub(ready_ms) >= sequence_cap_ms
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Floating-point comparison helper for timeline and geometry assertions.
    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    // Converts a pixel `[x, y, w, h]` box on a `1024x1024` stage picture into
    // fractional `[x0, y0, x1, y1]` coordinates for `LoadingTimeline`.
    fn fraction([x, y, w, h]: [f64; 4]) -> Option<[f64; 4]> {
        Some([x / 1024.0, y / 1024.0, (x + w) / 1024.0, (y + h) / 1024.0])
    }

    // Target rectangles from Geonosis (`geo1`) across its four stage pictures
    // (galaxy -> locale -> landsat -> hologram grid).
    fn geo1_targets() -> Vec<Option<[f64; 4]>> {
        vec![
            fraction([390.0, 737.0, 37.0, 43.0]),
            fraction([360.0, 555.0, 38.0, 37.0]),
            fraction([360.0, 555.0, 38.0, 37.0]),
            None,
        ]
    }

    // Evaluates the 2004 executable's discrete 50ms step product `(1 - k * STEP / half)`
    // so `approach` and `pulse` can be checked against the retail recurrence.
    fn exe_left(half: f64, steps: usize) -> Vec<f64> {
        let mut left = vec![1.0];
        for k in 1..=steps {
            let prev = left[k - 1];
            left.push(prev * (1.0 - ((k as f64) * STEP) / half).max(0.0));
        }
        left
    }

    // Verifies that the continuous log-integral `approach(t, h, STEP)` matches
    // the 2004 executable's discrete 50ms step product within 2% at every step
    // and decreases monotonically from `1.0` at `t = 0` to `0.0` at `t = half`.
    #[test]
    fn approach_matches_stepwise_product_and_is_monotonic() {
        let half = 1.75;
        let left = exe_left(half, 35);
        #[allow(clippy::needless_range_loop)]
        for k in 1..=34 {
            let smooth = approach((k as f64) * STEP, half, STEP);
            if left[k] > 1e-9 {
                assert!(
                    (smooth / left[k] - 1.0).abs() < 0.02,
                    "step {k}: {smooth} vs {}",
                    left[k]
                );
            } else {
                assert!(smooth < 1e-8);
            }
        }
        assert_eq!(approach(0.0, half, STEP), 1.0);
        assert_eq!(approach(half, half, STEP), 0.0);
        let mut last = 1.0;
        let mut t = 0.001;
        while t < half {
            let now = approach(t, half, STEP);
            assert!(now <= last + 1e-12);
            last = now;
            t += 0.001;
        }
    }

    // Verifies that `pulse(travel, half, STEP, STILL)` starts on the exact 50ms
    // step where the leading crosshair line moves more than `STILL` (`0.001`) of
    // the screen and stops within one step of the discrete recurrence, returning
    // `None` when total travel is too small (`0.0005`) to exceed `STILL`.
    #[test]
    fn pulse_matches_stepwise_movement_threshold() -> Result<(), Box<dyn std::error::Error>> {
        let half = 1.75;
        let left = exe_left(half, 35);
        let exe = |travel: f64| -> Option<[f64; 2]> {
            let mut from = None;
            for k in 1..=35 {
                let moved = travel * (left[k - 1] - left[k]);
                let t0 = ((k - 1) as f64) * STEP;
                if moved > STILL && from.is_none() {
                    from = Some(t0);
                } else if moved <= STILL {
                    if let Some(s) = from {
                        return Some([s, t0]);
                    }
                }
            }
            from.map(|s| [s, half])
        };

        for travel in [0.72, 0.54, 0.38, 0.35, 0.2, 0.1] {
            let [from, to] = pulse(travel, half, STEP, STILL).ok_or("expected pulse")?;
            let [exe_from, exe_to] = exe(travel).ok_or("expected exe pulse")?;
            assert!(close(from, exe_from, 1e-6));
            assert!((to - exe_to).abs() <= STEP + 1e-9);
            assert!(to > 0.6 && to < 1.1);
        }
        assert!(close(
            pulse(0.03, half, STEP, STILL).ok_or("expected pulse")?[0],
            STEP,
            1e-6
        ));
        assert_eq!(pulse(0.0005, half, STEP, STILL), None);
        Ok(())
    }

    // Verifies `LoadingTimeline` stage progression across 4 pictures (`geo1`):
    // 3 transitions of `3.5s` track + `1.0s` zoom = `13.5s` total duration,
    // plus optional `hold` time on the final stage before `done` becomes `true`.
    #[test]
    fn timeline_tracks_3_5s_zooms_1s_and_finishes_at_end_of_last_zoom(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let timeline = LoadingTimeline::new(3.5, 1.0, 0.0, geo1_targets());
        assert_eq!(timeline.stages(), 4);
        assert_eq!(timeline.duration(), 13.5);

        let at0 = timeline.at(0.0);
        assert_eq!(at0.stage, 0);
        assert_eq!(at0.phase, LoadingPhase::Track);
        assert_eq!(at0.lines, Some(TrackingLines { y: 0.0, x: None }));

        let at1 = timeline.at(1.0);
        let lines1 = at1.lines.ok_or("expected lines at 1.0s")?;
        assert!(lines1.y > 0.999 && lines1.x.is_none());

        let at_half = timeline.at(1.75);
        assert_eq!(
            at_half.lines,
            Some(TrackingLines {
                y: 1.0,
                x: Some(0.0)
            })
        );
        let lines_end = timeline.at(3.49).lines.ok_or("expected lines at 3.49s")?;
        assert!(lines_end.x.ok_or("expected x line at 3.49s")? > 0.999);

        let at_zoom = timeline.at(3.5);
        assert_eq!(at_zoom.stage, 0);
        assert_eq!(at_zoom.phase, LoadingPhase::Zoom);
        assert_eq!(at_zoom.zoom, Some(0.0));
        assert!(close(
            timeline.at(4.0).zoom.ok_or("expected zoom at 4.0s")?,
            0.5,
            1e-6
        ));

        let at_stage1 = timeline.at(4.5);
        assert_eq!(at_stage1.stage, 1);
        assert_eq!(at_stage1.phase, LoadingPhase::Track);
        assert!(close(at_stage1.time, 0.0, 1e-9));

        assert!(!timeline.at(13.49).done);
        let at_end = timeline.at(13.5);
        assert_eq!(at_end.stage, 3);
        assert_eq!(at_end.phase, LoadingPhase::Hold);
        assert!(at_end.lines.is_none());
        assert!(at_end.done);

        let held = LoadingTimeline::new(3.5, 1.0, 2.0, geo1_targets());
        assert_eq!(held.duration(), 15.5);
        assert!(!held.at(15.0).done);
        assert!(held.at(15.5).done);
        Ok(())
    }

    #[test]
    fn timeline_sounds_align_with_crosshairs_and_zooms() {
        let timeline = LoadingTimeline::new(3.5, 1.0, 0.0, geo1_targets());
        let sounds = timeline.sounds();
        let cues: Vec<&str> = sounds.iter().map(|s| s.cue.as_str()).collect();
        assert_eq!(
            cues,
            vec![
                "ytrack",
                "xtrack",
                "zoom",
                "transition",
                "ytrack",
                "xtrack",
                "zoom",
                "transition",
                "ytrack",
                "xtrack",
                "zoom",
                "transition"
            ]
        );
        assert_eq!(timeline.cue(0.5, Some(&sounds)), "ytrack");
        assert_eq!(timeline.cue(1.2, Some(&sounds)), "still");
        assert_eq!(timeline.cue(2.0, Some(&sounds)), "xtrack");
        assert_eq!(timeline.cue(4.0, Some(&sounds)), "zoom");
        assert_eq!(timeline.cue(13.6, Some(&sounds)), "still");
    }

    #[test]
    fn zoom_view_disk_place_light_level_mix_color_and_planet_name_match_js() {
        let target = [390.0, 737.0, 37.0, 43.0];
        assert_eq!(zoom_view(0.0, 1024.0, 1024.0, target), [0.0, 0.0, 1024.0, 1024.0]);
        let end = zoom_view(1.0, 1024.0, 1024.0, target);
        for i in 0..4 {
            assert!(close(end[i], target[i], 1e-6));
        }
        let half = zoom_view(0.5, 1024.0, 1024.0, target);
        assert!(close(half[2], (1024.0_f64 * 37.0).sqrt(), 1e-6));
        assert!(close(half[3], (1024.0_f64 * 43.0).sqrt(), 1e-6));

        let full = on_screen(target, zoom_view(1.0, 1024.0, 1024.0, target), 1280.0, 720.0);
        for (a, b) in full.into_iter().zip([0.0, 0.0, 1280.0, 720.0]) {
            assert!(close(a, b, 1e-6));
        }

        assert_eq!(
            disk_place(1024.0, 768.0),
            DiskPlace {
                x: 772.0,
                y: 545.0,
                unit: 1.0
            }
        );

        assert_eq!(light_level(0.0), 1.0);
        assert_eq!(light_level(3.0), 1.0);
        assert!(close(light_level(-lights::RAMP * 0.5), 0.5, 1e-6));
        assert_eq!(light_level(lights::TAIL), 0.0);

        assert_eq!(mix_color(LookId::Bf1.light_stops(), 0.0), "rgb(11, 13, 15)");
        assert_eq!(mix_color(LookId::Bf1.light_stops(), 0.5), "rgb(92, 97, 102)");
        assert_eq!(
            mix_color(LookId::Bf1.light_stops(), 1.0),
            "rgb(185, 188, 190)"
        );

        let empty = BTreeMap::new();
        let mut with_rhn = BTreeMap::new();
        with_rhn.insert("planets.rhenvar.name".to_string(), "RHEN VAR".to_string());
        assert_eq!(
            planet_name(
                "geo1",
                &empty,
                Some(&MapLoad {
                    planet: Some("GEONOSIS".to_string()),
                    ..Default::default()
                })
            ),
            "GEONOSIS"
        );
        assert_eq!(planet_name("rhn2", &with_rhn, None), "RHEN VAR");
        assert_eq!(planet_name("yav1", &empty, None), "YAVIN 4");
        assert_eq!(planet_name("zzz9", &empty, None), "ZZZ9");
    }
}
