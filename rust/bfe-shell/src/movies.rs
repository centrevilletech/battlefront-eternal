//! Native counterpart of MoviePlayer in src/shell/movies.js. Each active layer
//! owns a worker, and renderers borrow its latest I420 frame without copying it.
use crate::data::{MovieClip, ScreenMovie};
use bfe_video::{Frame, Movie};
use std::path::Path;

pub const MAX_LAYERS: usize = 4;
pub struct Layer {
    movie: Movie,
    pub serial: u64,
    pub frame: Option<Frame>,
    pending: Option<Frame>,
    pub changed: bool,
    pub opacity: f64,
    clip: MovieClip,
    elapsed: f64,
    started: bool,
    intro: bool,
    fading: Option<(f64, f64)>,
    failed: bool,
    loading_elapsed: f64,
}
impl Layer {
    fn new(root: &Path, clip: MovieClip, intro: bool, serial: u64) -> Self {
        Self {
            movie: Movie::open(root.join(clip.url.trim_start_matches('/')), !intro),
            serial,
            frame: None,
            pending: None,
            changed: false,
            opacity: 0.,
            clip,
            elapsed: 0.,
            started: false,
            intro,
            fading: None,
            failed: false,
            loading_elapsed: 0.,
        }
    }
    fn advance(&mut self, dt: f64, stats: &mut MovieStats) -> bool {
        self.changed = false;
        if self.started {
            self.elapsed += dt;
        } else {
            self.loading_elapsed += dt;
        }
        loop {
            if self.pending.is_none() {
                match self.movie.poll() {
                    Some(Ok(frame)) => self.pending = Some(frame),
                    Some(Err(error)) => {
                        eprintln!("Movie playback: {error}");
                        self.failed = true;
                    }
                    None => (),
                }
            }
            let Some(frame) = self.pending.as_ref() else {
                break;
            };
            if !self.started {
                self.started = true;
                self.elapsed = 0.;
            }
            if frame.pts > self.elapsed {
                break;
            }
            if let Some(old) = self.frame.take() {
                if self.changed {
                    stats.dropped += 1;
                }
                self.movie.recycle(old);
            }
            let frame = self.pending.take().unwrap();
            stats.decoded += 1;
            stats.decode_ms += frame.decode_ms;
            self.frame = Some(frame);
            self.changed = true;
        }
        self.opacity = if self.clip.fade_in > 0. {
            (self.elapsed / self.clip.fade_in).clamp(0., 1.)
        } else {
            1.
        };
        if self.intro {
            if let Some(frame) = &self.frame {
                if self.clip.fade_out > 0. {
                    self.opacity *=
                        ((frame.duration - self.elapsed) / self.clip.fade_out).clamp(0., 1.);
                }
                if self.elapsed >= frame.duration {
                    return true;
                }
            }
        }
        if let Some((ref mut elapsed, duration)) = self.fading {
            *elapsed += dt;
            self.opacity *= (1. - *elapsed / duration.max(f64::EPSILON)).clamp(0., 1.);
            if *elapsed >= duration {
                return true;
            }
        }
        self.failed || (!self.started && self.loading_elapsed >= 2.5)
    }
}
/// Counts submitted decoder frames and frames superseded before presentation.
#[derive(Default)]
pub struct MovieStats {
    pub decoded: u64,
    pub dropped: u64,
    pub decode_ms: f64,
}
/// Fixed active-layer slots are also stable keys for a renderer's plane textures.
/// New layers draw under older fading layers, as MoviePlayer.add/fadeAll do.
pub struct MoviePlayer {
    pub layers: [Option<Layer>; MAX_LAYERS],
    next: Option<Layer>,
    delayed: Option<(ScreenMovie, bool, f64)>,
    serial: u64,
    activating: Option<u64>,
    pub stats: MovieStats,
    pub intro_ended: bool,
}
impl Default for MoviePlayer {
    fn default() -> Self {
        Self {
            layers: std::array::from_fn(|_| None),
            next: None,
            delayed: None,
            serial: 0,
            activating: None,
            stats: MovieStats::default(),
            intro_ended: false,
        }
    }
}
impl MoviePlayer {
    pub fn play(&mut self, root: &Path, movie: ScreenMovie, intro: bool, delay: f64) {
        self.delayed = Some((movie, intro, delay));
        self.next = None;
        self.intro_ended = false;
        if delay <= 0. {
            self.begin(root);
        }
    }
    fn insert(&mut self, layer: Layer) {
        let slot = self
            .layers
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                self.layers
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, l)| l.as_ref().unwrap().serial)
                    .unwrap()
                    .0
            });
        self.layers[slot] = Some(layer);
    }
    fn begin(&mut self, root: &Path) {
        let Some((movie, intro, _)) = self.delayed.take() else {
            return;
        };
        for layer in self.layers.iter_mut().flatten() {
            layer.intro = false;
        }
        let first = if intro {
            movie.intro.clone().or(movie.loop_clip.clone())
        } else {
            movie.loop_clip.clone()
        };
        let is_intro = intro && movie.intro.is_some();
        if let Some(clip) = first {
            self.serial += 1;
            self.activating = Some(self.serial);
            self.insert(Layer::new(root, clip, is_intro, self.serial));
            if is_intro {
                if let Some(clip) = movie.loop_clip {
                    self.serial += 1;
                    self.next = Some(Layer::new(root, clip, false, self.serial));
                }
            }
        } else {
            self.activating = None;
            for layer in self.layers.iter_mut().flatten() {
                layer.fading = Some((0., layer.clip.fade_out));
            }
            self.intro_ended = true;
        }
    }
    pub fn update(&mut self, root: &Path, dt: f64) {
        let dt = if dt.is_finite() { dt.max(0.) } else { 0. };
        if let Some((_, _, delay)) = self.delayed.as_mut() {
            *delay -= dt;
            if *delay <= 0. {
                self.begin(root);
            }
        }
        let mut finish = false;
        let mut activate = false;
        for slot in &mut self.layers {
            if let Some(layer) = slot {
                let done = layer.advance(dt, &mut self.stats);
                if self.activating == Some(layer.serial) && (layer.frame.is_some() || done) {
                    activate = true;
                }
                if done {
                    finish |= layer.intro && layer.fading.is_none();
                    *slot = None;
                }
            }
        }
        if activate {
            let serial = self.activating.take().unwrap();
            for layer in self.layers.iter_mut().flatten() {
                if layer.serial != serial && layer.fading.is_none() {
                    layer.fading = Some((0., layer.clip.fade_out));
                }
            }
        }
        if finish {
            self.intro_ended = true;
            if let Some(next) = self.next.take() {
                self.insert(next);
            }
        }
    }
    /// Ordered newest first so the old movie fades on top of its replacement.
    pub fn draw_order(&self) -> [usize; MAX_LAYERS] {
        let mut slots = std::array::from_fn(|i| i);
        slots.sort_unstable_by_key(|i| {
            std::cmp::Reverse(self.layers[*i].as_ref().map_or(0, |l| l.serial))
        });
        slots
    }
}
