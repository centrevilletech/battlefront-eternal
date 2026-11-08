//! Screen media transitions (`SCREEN_MEDIA`, `PREVIEW_DELAY`, `previewRect`,
//! `ShellMedia` in `src/shell/media.js` and `MoviePlayer` in `src/shell/movies.js`).
//!
//! Ports `src/shell/media.js` and `src/shell/movies.js`:
//! - `SCREEN_MEDIA`: per-screen `{ movie, music }` assignment (`start`,
//!   `profile`, `main`, `sp`, `options`, `briefing`, `meta`, `galaxy`, `ia`,
//!   `mp`).
//! - `PREVIEW_DELAY = 0.5` seconds and `previewRect(width, height, targetAspect)`.
//! - `ShellMedia`: coordinates screen transitions between the movie backdrop,
//!   whoosh sound effects, `shellmusic` bus dips (`fadeBus("shellmusic", 0, 1)`
//!   on forward moves into a screen whose music isn't already playing, restoring
//!   `fadeBus("shellmusic", 1, 1)` when the intro finishes), and `ShellMusic`.
//! - `StillMoviePlayer`: native backdrop state machine that tracks intro/loop
//!   fade timings and resolves each clip's `common.json` poster still texture
//!   (`main`, `atat_screen`, `coreship_screen`, `generic2_screen`,
//!   `generic3_screen`, `gc_screen`, `hoth_screen`, `atte_screen`, `map_<id>`)
//!   when VP9 video playback is not active.

use crate::{
    data::{poster_still_for_movie, MovieClip, ScreenMovie, ShellData},
    music::{MusicStep, ShellMusic, SOUNDTRACK},
};

/// Delay in seconds (`0.5s`) before Instant Action's highlighted map flyover
/// preview starts. Mirrors `PREVIEW_DELAY` in `src/shell/media.js`.
pub const PREVIEW_DELAY: f64 = 0.5;

/// Movie and music assignment for one shell screen.
/// Mirrors an entry of `SCREEN_MEDIA` in `src/shell/media.js`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenMediaSpec {
    /// Key in `ShellData::movies` (`"start"`, `"main"`, `"sp"`, `"options"`,
    /// `"briefing"`, `"meta"`, `"galaxy"`), or `None` when the screen uses a
    /// static backdrop (`"ia"` and `"mp"`).
    pub movie: Option<&'static str>,
    /// Music cue to set on entry (`Some(SOUNDTRACK)` for the three-track
    /// soundtrack cycle, `Some("shell_galacticconquest")` for Galactic Conquest,
    /// or `None` to leave whatever music is currently playing untouched).
    pub music: Option<&'static str>,
}

/// Table of each shell screen's movie and music cue.
/// Mirrors `SCREEN_MEDIA` in `src/shell/media.js`.
pub const SCREEN_MEDIA: [(&str, ScreenMediaSpec); 10] = [
    (
        "start",
        ScreenMediaSpec {
            movie: Some("start"),
            music: Some(SOUNDTRACK),
        },
    ),
    (
        "profile",
        ScreenMediaSpec {
            movie: Some("start"),
            music: Some(SOUNDTRACK),
        },
    ),
    (
        "main",
        ScreenMediaSpec {
            movie: Some("main"),
            music: Some(SOUNDTRACK),
        },
    ),
    (
        "sp",
        ScreenMediaSpec {
            movie: Some("sp"),
            music: Some(SOUNDTRACK),
        },
    ),
    (
        "options",
        ScreenMediaSpec {
            movie: Some("options"),
            music: None,
        },
    ),
    (
        "briefing",
        ScreenMediaSpec {
            movie: Some("briefing"),
            music: Some(SOUNDTRACK),
        },
    ),
    (
        "meta",
        ScreenMediaSpec {
            movie: Some("meta"),
            music: None,
        },
    ),
    (
        "galaxy",
        ScreenMediaSpec {
            movie: Some("galaxy"),
            music: Some("shell_galacticconquest"),
        },
    ),
    (
        "ia",
        ScreenMediaSpec {
            movie: None,
            music: None,
        },
    ),
    (
        "mp",
        ScreenMediaSpec {
            movie: None,
            music: None,
        },
    ),
];

/// Looks up the [`ScreenMediaSpec`] for `id` in [`SCREEN_MEDIA`].
pub fn screen_media_of(id: &str) -> ScreenMediaSpec {
    SCREEN_MEDIA
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, v)| *v)
        .unwrap_or(ScreenMediaSpec {
            movie: None,
            music: None,
        })
}

/// Computes the Instant Action map preview rectangle `[x, y, w, h]` in the
/// lower-right corner for a screen of `(width, height)` pixels and `target_aspect`
/// (default `4.0 / 3.0`):
/// `w = 530 * (1 + target_aspect - width / height) * (width / 800)`,
/// `h = 400 * (height / 600)`.
///
/// Mirrors `previewRect(width, height, targetAspect)` in `src/shell/media.js`.
pub fn preview_rect_with_aspect(width: f64, height: f64, target_aspect: f64) -> [f64; 4] {
    let w = 530.0 * (1.0 + target_aspect - width / height) * (width / 800.0);
    let h = 400.0 * (height / 600.0);
    [width - w, height - h, w, h]
}

/// Computes the Instant Action map preview rectangle `[x, y, w, h]` using the
/// default `4.0 / 3.0` target aspect ratio.
/// Mirrors `previewRect(width, height)` in `src/shell/media.js`.
pub fn preview_rect(width: f64, height: f64) -> [f64; 4] {
    preview_rect_with_aspect(width, height, 4.0 / 3.0)
}

/// Audio side-effect event emitted by [`ShellMedia`] during screen transitions.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaEvent {
    /// Play a screen movie (`intro_url`, `loop_url`, `play_intro`).
    PlayMovie {
        intro_url: Option<String>,
        loop_url: Option<String>,
        play_intro: bool,
    },
    /// Play a map flyover preview clip after [`PREVIEW_DELAY`].
    PreviewMovie {
        loop_url: Option<String>,
        delay: f64,
    },
    /// Ramp the `shellmusic` bus to `level` over `seconds`.
    FadeBus {
        bus: &'static str,
        level: f64,
        seconds: f64,
    },
    /// Play the next transition whoosh sound (`shell_transition1..10`).
    Whoosh,
    /// Music start/stop transition from [`ShellMusic`].
    Music(MusicStep),
}

/// Active backdrop layer state for native rendering (poster still texture key +
/// opacity).
#[derive(Clone, Debug, PartialEq)]
pub struct BackdropState {
    /// Active full-screen movie clip URL, if any.
    pub clip_url: Option<String>,
    /// Resolved `common.json` poster still texture key (`"main"`, `"atat_screen"`,
    /// etc.), if any.
    pub poster_still: Option<String>,
    /// Current opacity (`0.0..=1.0`) of the backdrop layer.
    pub opacity: f64,
    /// Highlighted map id in Instant Action (`"bes2"`, `"geo1"`, etc.).
    pub preview_map: Option<String>,
    /// Resolved `common.json` texture key for the Instant Action preview frame
    /// (`"map_bes2"`, etc.).
    pub preview_still: Option<String>,
    /// Elapsed time in seconds since the Instant Action selection changed, used
    /// to honor [`PREVIEW_DELAY`].
    pub preview_elapsed: f64,
}

impl Default for BackdropState {
    fn default() -> Self {
        Self {
            clip_url: None,
            poster_still: None,
            opacity: 1.0,
            preview_map: None,
            preview_still: None,
            preview_elapsed: 0.0,
        }
    }
}

/// Coordinates screen movies, transition whooshes, music bus dips, and
/// [`ShellMusic`] across front-end navigation (`ifs_movietrans` behavior).
///
/// Mirrors `ShellMedia` in `src/shell/media.js`.
pub struct ShellMedia {
    /// Identifier of the currently active screen (`"start"`, `"main"`, `"sp"`, etc.).
    pub screen: Option<String>,
    /// Count of forward transitions into screens that played an intro clip.
    pub moves: usize,
    /// Soundtrack and screen-specific music state machine (`src/shell/audio.js`).
    pub music: ShellMusic,
    /// Current movie clip URL, resolved poster still texture, and IA preview state.
    pub backdrop: BackdropState,
    /// In-flight intro transition `(token, screen_id, target_music, loop_clip)`
    /// waiting for [`Self::finish_intro`] before restoring `shellmusic` to `1.0`.
    pending_intro: Option<(u64, String, Option<&'static str>, MovieClip)>,
    /// Monotonic transition counter incremented on every [`Self::begin_enter`] call
    /// so superseded intros are ignored if a second screen move happens mid-intro.
    token: u64,
}

impl ShellMedia {
    /// Creates a new [`ShellMedia`] state machine initialized with `data.music`
    /// and the `10.0s` inter-track silence interval (`crate::music::INTERVAL`),
    /// starting with no active screen (`screen = None`, `moves = 0`).
    /// Mirrors `new ShellMedia({ data })` in `src/shell/media.js`.
    pub fn new(data: &ShellData) -> Self {
        Self {
            screen: None,
            moves: 0,
            music: ShellMusic::new(data.music.clone(), crate::music::INTERVAL),
            backdrop: BackdropState::default(),
            pending_intro: None,
            token: 0,
        }
    }

    /// Looks up the [`ScreenMovie`] for screen `id` in `data.movies`.
    /// Mirrors `ShellMedia.movieOf(id)` in `src/shell/media.js`.
    pub fn movie_of<'a>(&self, data: &'a ShellData, id: &str) -> Option<&'a ScreenMovie> {
        let spec = screen_media_of(id);
        let movie_name = spec.movie?;
        data.movies.get(movie_name)
    }

    /// Enters screen `id` (`back = true` when popping back to an earlier screen).
    ///
    /// Mirrors `ShellMedia.enter(id, { back })` in `src/shell/media.js`:
    /// - When `back == true` or the screen's movie has no `intro`, starts the
    ///   loop immediately, restores `shellmusic` to `1.0` over `1.0s` (when not
    ///   `back`), and sets the screen's music cue.
    /// - When moving forward into a screen with an `intro`, increments `moves`,
    ///   dips `shellmusic` to `0.0` over `1.0s` if the target music isn't
    ///   already playing, starts the intro, and triggers a transition `Whoosh`.
    ///   Once [`Self::finish_intro`] is called (or immediately when `immediate_intro`
    ///   is used in non-blocking native transitions), `shellmusic` fades back to
    ///   `1.0` over `1.0s` and the screen's music cue is set.
    pub fn begin_enter(&mut self, data: &ShellData, id: &str, back: bool) -> Vec<MediaEvent> {
        let mut events = Vec::new();
        let spec = screen_media_of(id);
        let movie = self.movie_of(data, id).cloned();
        self.screen = Some(id.to_string());
        self.token += 1;
        let token = self.token;
        self.pending_intro = None;

        let intro_clip = movie.as_ref().and_then(|m| m.intro.clone());
        let loop_clip = movie.as_ref().and_then(|m| m.loop_clip.clone());

        // Returning (`back == true`) or entering a screen whose movie has no
        // intro clip skips the transition whoosh and starts the loop immediately.
        let Some(intro) = intro_clip.as_ref().filter(|_| !back).cloned() else {
            events.push(MediaEvent::PlayMovie {
                intro_url: intro_clip.as_ref().map(|c| c.url.clone()),
                loop_url: loop_clip.as_ref().map(|c| c.url.clone()),
                play_intro: false,
            });
            self.backdrop.clip_url = loop_clip.as_ref().map(|c| c.url.clone());
            self.backdrop.poster_still = loop_clip
                .as_ref()
                .and_then(|c| poster_still_for_movie(&c.url))
                .map(str::to_string);
            self.backdrop.opacity = 1.0;

            if !back {
                events.push(MediaEvent::FadeBus {
                    bus: "shellmusic",
                    level: 1.0,
                    seconds: 1.0,
                });
            }
            let step = self.music.set(spec.music);
            if step != MusicStep::default() {
                events.push(MediaEvent::Music(step));
            }
            return events;
        };

        // Forward transition into a screen with an intro clip: dip `shellmusic`
        // to `0.0` over `1.0s` when the screen's music isn't already playing,
        // start the intro clip, and fire the next transition whoosh.
        self.moves += 1;
        if !self.music.playing(spec.music) {
            events.push(MediaEvent::FadeBus {
                bus: "shellmusic",
                level: 0.0,
                seconds: 1.0,
            });
        }
        events.push(MediaEvent::PlayMovie {
            intro_url: Some(intro.url.clone()),
            loop_url: loop_clip.as_ref().map(|c| c.url.clone()),
            play_intro: true,
        });
        events.push(MediaEvent::Whoosh);

        let active_clip = loop_clip.unwrap_or_else(|| intro.clone());
        self.backdrop.clip_url = Some(intro.url.clone());
        self.backdrop.poster_still = poster_still_for_movie(&active_clip.url)
            .or_else(|| poster_still_for_movie(&intro.url))
            .map(str::to_string);
        self.backdrop.opacity = 1.0;
        self.pending_intro = Some((token, id.to_string(), spec.music, active_clip));
        events
    }

    /// Completes a pending intro transition if `token` (or the latest transition)
    /// is still the active screen, restoring `shellmusic` to `1.0` over `1.0s`
    /// and setting the screen's music cue.
    ///
    /// Returns `(completed, events)` where `completed` is `false` if another
    /// screen move superseded the intro before it ended.
    pub fn finish_intro(&mut self, for_token: Option<u64>) -> (bool, Vec<MediaEvent>) {
        let Some((token, screen_id, music_cue, loop_clip)) = self.pending_intro.clone() else {
            return (false, Vec::new());
        };
        if let Some(expected) = for_token {
            if expected != token {
                return (false, Vec::new());
            }
        }
        self.pending_intro = None;
        if self.screen.as_deref() != Some(screen_id.as_str()) {
            return (false, Vec::new());
        }
        self.backdrop.clip_url = Some(loop_clip.url.clone());
        self.backdrop.poster_still = poster_still_for_movie(&loop_clip.url).map(str::to_string);
        let mut events = vec![MediaEvent::FadeBus {
            bus: "shellmusic",
            level: 1.0,
            seconds: 1.0,
        }];
        let step = self.music.set(music_cue);
        if step != MusicStep::default() {
            events.push(MediaEvent::Music(step));
        }
        (true, events)
    }

    /// Enters screen `id` and immediately settles any intro (convenience for
    /// synchronous screen changes).
    pub fn enter_immediate(&mut self, data: &ShellData, id: &str, back: bool) -> Vec<MediaEvent> {
        let mut events = self.begin_enter(data, id, back);
        if self.pending_intro.is_some() {
            let (_, tail) = self.finish_intro(None);
            events.extend(tail);
        }
        events
    }

    /// Selects the Instant Action flyover preview for `map_id` after
    /// [`PREVIEW_DELAY`].
    /// Mirrors `ShellMedia.preview(mapId)` in `src/shell/media.js`.
    pub fn preview(&mut self, data: &ShellData, map_id: Option<&str>) -> MediaEvent {
        let map = map_id.and_then(|id| data.maps.iter().find(|m| m.id == id));
        let fly_url = map.and_then(|m| m.fly.as_ref()).map(|c| c.url.clone());
        self.backdrop.preview_map = map.map(|m| m.id.clone());
        self.backdrop.preview_still = map.map(|m| m.preview_still());
        self.backdrop.preview_elapsed = 0.0;
        MediaEvent::PreviewMovie {
            loop_url: fly_url,
            delay: PREVIEW_DELAY,
        }
    }

    /// Advances the music silence timer and preview delay timer by `dt` seconds.
    /// Mirrors `ShellMedia.update(dt)` in `src/shell/media.js`.
    pub fn update(&mut self, dt: f64) -> Option<MusicStep> {
        if dt.is_finite() && dt > 0.0 {
            self.backdrop.preview_elapsed += dt;
        }
        let step = self.music.update(dt);
        if step == MusicStep::default() {
            None
        } else {
            Some(step)
        }
    }

    /// Returns the monotonic transition token assigned by the most recent
    /// [`Self::begin_enter`] call so callers can pass `Some(token)` to
    /// [`Self::finish_intro`] when an intro clip finishes playing.
    pub fn current_token(&self) -> u64 {
        self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Builds a minimal `ShellData` fixture with `start` (loop only), `main`,
    // `sp`, and `options` (intro + loop), one `geo1` map flyover, and 3
    // soundtrack tracks plus `shell_galacticconquest`.
    fn sample_data() -> ShellData {
        let shell = json!({
            "movies": {
                "ifs_start": { "intro": null, "loop": "movies/start.webm", "fadeIn": 0, "fadeOut": 0 },
                "ifs_main": { "intro": "movies/main_trans.webm", "loop": "movies/main.webm", "fadeIn": 0, "fadeOut": 0 },
                "ifs_sp": { "intro": "movies/atat_in.webm", "loop": "movies/atat.webm", "fadeIn": 0, "fadeOut": 0 },
                "ifs_opt_contmain": { "intro": "movies/coreship_in.webm", "loop": "movies/coreship.webm", "fadeIn": 0, "fadeOut": 0 }
            },
            "maps": [
                { "id": "geo1", "fly": "movies/geo1fly.webm" }
            ],
            "music": [
                { "name": "a", "file": "/audio/shell/a.ogg" },
                { "name": "b", "file": "/audio/shell/b.ogg" },
                { "name": "shell_galacticconquest", "file": "/audio/shell/gc.ogg", "soundtrack": false },
                { "name": "c", "file": "/audio/shell/c.ogg" }
            ]
        });
        crate::data::read_shell_data(Some(&shell), None, None)
    }

    #[test]
    fn forward_into_screen_whose_music_already_plays_runs_intro_and_whoosh_without_dip() {
        let data = sample_data();
        let mut media = ShellMedia::new(&data);

        // Entering start plays its loop at once and starts the soundtrack.
        let ev_start = media.begin_enter(&data, "start", false);
        assert_eq!(
            ev_start[0],
            MediaEvent::PlayMovie {
                intro_url: None,
                loop_url: Some("/common/shell/movies/start.webm".to_string()),
                play_intro: false,
            }
        );
        assert_eq!(
            ev_start[1],
            MediaEvent::FadeBus {
                bus: "shellmusic",
                level: 1.0,
                seconds: 1.0
            }
        );
        assert!(matches!(ev_start[2], MediaEvent::Music(_)));

        // Forward into main: soundtrack already playing, so no dip before intro.
        let ev_main = media.begin_enter(&data, "main", false);
        assert_eq!(
            ev_main,
            vec![
                MediaEvent::PlayMovie {
                    intro_url: Some("/common/shell/movies/main_trans.webm".to_string()),
                    loop_url: Some("/common/shell/movies/main.webm".to_string()),
                    play_intro: true,
                },
                MediaEvent::Whoosh,
            ]
        );
        let (ok, tail) = media.finish_intro(None);
        assert!(ok);
        assert_eq!(
            tail,
            vec![MediaEvent::FadeBus {
                bus: "shellmusic",
                level: 1.0,
                seconds: 1.0
            }]
        );
        assert_eq!(media.screen.as_deref(), Some("main"));
        assert_eq!(media.moves, 1);
    }

    // Entering `options` (`music: None`) when `SOUNDTRACK` is playing dips
    // `shellmusic` to 0 over 1s during `coreship_in.webm`, then fades back to 1
    // while keeping `SOUNDTRACK` active (`music.set(None)` is a no-op).
    #[test]
    fn forward_into_options_dips_music_over_intro_and_restores_after() {
        let data = sample_data();
        let mut media = ShellMedia::new(&data);
        media.music.set(Some(SOUNDTRACK));

        let ev_opt = media.begin_enter(&data, "options", false);
        assert_eq!(
            ev_opt,
            vec![
                MediaEvent::FadeBus {
                    bus: "shellmusic",
                    level: 0.0,
                    seconds: 1.0
                },
                MediaEvent::PlayMovie {
                    intro_url: Some("/common/shell/movies/coreship_in.webm".to_string()),
                    loop_url: Some("/common/shell/movies/coreship.webm".to_string()),
                    play_intro: true,
                },
                MediaEvent::Whoosh,
            ]
        );
        let (ok, tail) = media.finish_intro(None);
        assert!(ok);
        assert_eq!(
            tail,
            vec![MediaEvent::FadeBus {
                bus: "shellmusic",
                level: 1.0,
                seconds: 1.0
            }]
        );
        assert!(media.music.playing(Some(SOUNDTRACK)));
    }

    // Verifies two `ShellMedia.enter` invariants from `src/shell/media.js`:
    // 1. Navigating back (`back == true`) starts the destination screen's loop
    //    without playing its intro, dipping `shellmusic`, or triggering a whoosh.
    // 2. Starting a second screen transition before an in-flight intro finishes
    //    invalidates the earlier intro's token so `finish_intro` is a no-op.
    #[test]
    fn back_plays_loop_only_without_whoosh_or_dip_and_second_move_cancels_first_intro() {
        let data = sample_data();
        let mut media = ShellMedia::new(&data);
        media.music.set(Some(SOUNDTRACK));

        let ev_back = media.begin_enter(&data, "main", true);
        assert_eq!(
            ev_back,
            vec![MediaEvent::PlayMovie {
                intro_url: Some("/common/shell/movies/main_trans.webm".to_string()),
                loop_url: Some("/common/shell/movies/main.webm".to_string()),
                play_intro: false,
            }]
        );

        // Start options intro, then immediately go back to sp before options intro finishes.
        media.begin_enter(&data, "options", false);
        let first_token = media.current_token();
        media.begin_enter(&data, "sp", true);
        let (ok, _) = media.finish_intro(Some(first_token));
        assert!(!ok);
        assert_eq!(media.screen.as_deref(), Some("sp"));
    }

    // Verifies `previewRect(1024, 768)` against the 4:3 formula in `src/shell/media.js`
    // (`w = 530 * (1024 / 800) = 678.4`, `h = 400 * (768 / 600) = 512.0`).
    #[test]
    fn preview_rect_matches_1024x768_lower_right_box() {
        let [x, y, w, h] = preview_rect(1024.0, 768.0);
        assert!((w - 678.4).abs() < 1e-9);
        assert!((h - 512.0).abs() < 1e-9);
        assert!((x - 345.6).abs() < 1e-9);
        assert!((y - 256.0).abs() < 1e-9);
    }
}
