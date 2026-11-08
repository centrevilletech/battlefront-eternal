//! Shell data readers (`common/shell/shell.json`, `common/common.json`,
//! `common/raster-hud/manifest.json`).
//!
//! This module ports `src/shell/data.js` (`SHELL_ROOT`, `FADES`,
//! `MOVIE_SCREENS`, `resolveUrl`, `readClip`, `readMovies`, `readMapLoad`,
//! `readMaps`, `readLoad`, `textureFiles`, `readFonts`, `readShellData`, and
//! native filesystem loading). Any manifest or section may be missing (nothing
//! extracted yet, or an older extraction); the reader runs on what is present
//! and fills in the movie config fades (`shell.lvl` `mcfg`) where `shell.json`
//! leaves them out.
//!
//! Key rules ported from `src/shell/data.js`:
//! - `FADES`: base `[fadeIn, fadeOut]` pairs for `intro` (`[0.0, 0.2]`),
//!   `loop` (`[0.1, 0.5]`), and `fly` (`[1.0, 1.0]`), plus `OWN_FADES`
//!   overrides for `main` (`intro: [0, 0]`, `loop: [0, 0.5]`), `briefing`
//!   (`intro: [1, 1]`), and `meta` (`intro: [1, 1]`).
//! - `MOVIE_SCREENS`: indexes each movie under both its own lowercased key in
//!   `shell.json` and the port's canonical screen id (`start`, `main`, `sp`,
//!   `options`, `briefing`, `meta`, `galaxy`).
//! - `textureFiles` / `readFonts`: resolves `common.json` texture names to
//!   their 4x upscales in `/common/raster-hud/` when `manifest.json` has an
//!   entry, falling back to `/common/<path>`.

use crate::launch::GameFamily;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

/// Root prefix for shell assets in the prepared content tree.
/// Mirrors `SHELL_ROOT` in `src/shell/data.js`.
pub const SHELL_ROOT: &str = "/common/shell";

/// The movie config's three base fade pairs (`[fadeIn, fadeOut]` in seconds):
/// intros and one-shots, screen loops, and map flyovers.
/// Mirrors `FADES` in `src/shell/data.js`.
pub mod fades {
    /// Base `[fade_in, fade_out]` in seconds (`[0.0, 0.2]`) for screen intro clips.
    pub const INTRO: [f64; 2] = [0.0, 0.2];
    /// Base `[fade_in, fade_out]` in seconds (`[0.1, 0.5]`) for looping screen backdrops.
    pub const LOOP: [f64; 2] = [0.1, 0.5];
    /// Base `[fade_in, fade_out]` in seconds (`[1.0, 1.0]`) for Instant Action map flyovers.
    pub const FLY: [f64; 2] = [1.0, 1.0];
}

/// The port's screen ids for movies, and the original 2004 screen names that
/// may key them in `shell.json`'s `"movies"` (the first found is taken).
/// Mirrors `MOVIE_SCREENS` in `src/shell/data.js`.
pub const MOVIE_SCREENS: [(&str, &[&str]); 7] = [
    ("start", &["ifs_start", "ifs_login", "start_screen"]),
    ("main", &["ifs_main", "ifs_main_intro"]),
    ("sp", &["ifs_sp", "ifs_sp_intro", "atat_screen"]),
    (
        "options",
        &[
            "ifs_opt_contmain",
            "ifs_opt_top",
            "ifs_opt_top_intro",
            "coreship_screen",
        ],
    ),
    (
        "briefing",
        &["ifs_sp_briefing", "ifs_sp_briefing_intro", "generic2_screen"],
    ),
    (
        "meta",
        &[
            "ifs_meta_top",
            "ifs_meta_new_load",
            "ifs_meta",
            "ifs_meta_intro",
            "generic3_screen",
        ],
    ),
    ("galaxy", &["ifs_meta_main", "ifs_meta_screen", "gc_screen"]),
];

/// One movie clip `{ url, fade_in, fade_out }` returned by [`read_clip`].
/// Mirrors the `{ url, fadeIn, fadeOut }` object returned by `readClip` in
/// `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MovieClip {
    /// Resolved content URL (e.g. `"/common/shell/movies/main.webm"`).
    pub url: String,
    /// Fade-in duration in seconds (`fadeIn` in `src/shell/data.js`).
    pub fade_in: f64,
    /// Fade-out duration in seconds (`fadeOut` in `src/shell/data.js`).
    pub fade_out: f64,
}

/// A screen's movie pair `{ intro, loop }` returned by [`read_movies`].
/// Mirrors the `{ intro, loop }` entry in `readMovies` in `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenMovie {
    /// Optional transition intro clip played when entering the screen moving forward.
    pub intro: Option<MovieClip>,
    /// Looping background clip played while the screen remains open (`"loop"` in JSON).
    #[serde(rename = "loop")]
    pub loop_clip: Option<MovieClip>,
}

impl ScreenMovie {
    /// Returns the `common.json` poster still texture name (`"main"`,
    /// `"atat_screen"`, `"coreship_screen"`, `"generic2_screen"`,
    /// `"generic3_screen"`, `"gc_screen"`, `"hoth_screen"`, `"atte_screen"`)
    /// corresponding to this movie's loop or intro URL.
    pub fn poster_still(&self) -> Option<&'static str> {
        self.loop_clip
            .as_ref()
            .or(self.intro.as_ref())
            .and_then(|c| poster_still_for_movie(&c.url))
    }
}

/// Maps a movie path (`"/common/shell/movies/atat_screen.webm"`) to the
/// corresponding poster still texture key in `common.json`.
/// Maps a movie path (`"/common/shell/movies/atat_screen.webm"`) to the
/// corresponding poster still texture key in `common.json` (`"main"`,
/// `"atat_screen"`, `"coreship_screen"`, `"atte_screen"`, `"hoth_screen"`,
/// `"generic2_screen"`, `"generic3_screen"`, `"gc_screen"`, or `"map_<id>"`
/// for Instant Action map flyovers; `"tat3fly"` maps to `"map_unk"`).
pub fn poster_still_for_movie(url: &str) -> Option<&'static str> {
    let stem = url
        .rsplit('/')
        .next()
        .unwrap_or(url)
        .strip_suffix(".webm")
        .unwrap_or(url);
    match stem {
        "main" | "main_trans" | "start_screen" => Some("main"),
        "atat_screen" | "atat_in" => Some("atat_screen"),
        "coreship_screen" | "coreship_in" => Some("coreship_screen"),
        "atte_screen" | "atte_in" => Some("atte_screen"),
        "hoth_screen" | "hoth_in" => Some("hoth_screen"),
        "generic2_screen" | "generic2_in" => Some("generic2_screen"),
        "generic3_screen" | "generic3_in" => Some("generic3_screen"),
        "gc_screen" => Some("gc_screen"),
        "bes1fly" => Some("map_bes1"),
        "bes2fly" => Some("map_bes2"),
        "end1fly" => Some("map_end1"),
        "geo1fly" => Some("map_geo1"),
        "hot1fly" => Some("map_hot1"),
        "kam1fly" => Some("map_kam1"),
        "kas1fly" => Some("map_kas1"),
        "kas2fly" => Some("map_kas2"),
        "nab1fly" => Some("map_nab1"),
        "nab2fly" => Some("map_nab2"),
        "rhn1fly" => Some("map_rhn1"),
        "rhn2fly" => Some("map_rhn2"),
        "tat1fly" => Some("map_tat1"),
        "tat2fly" => Some("map_tat2"),
        "tat3fly" => Some("map_unk"),
        "yav1fly" => Some("map_yav1"),
        "yav2fly" => Some("map_yav2"),
        _ => None,
    }
}

/// One stage image and optional target rectangle `[x, y, w, h]` on a map's
/// loading screen. Mirrors the `{ url, target }` entries in `readMapLoad` in
/// `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoadStage {
    /// Resolved content URL of the stage picture (`"/common/shell/load/..."`).
    pub url: String,
    /// Target crop box `[x, y, w, h]` in pixels for zooming into the next stage (`None` on last stage).
    pub target: Option<[f64; 4]>,
}

/// A map's loading screen metadata `{ planet, planet_key, stages }`.
/// Mirrors `readMapLoad` in `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct MapLoad {
    /// Localized or direct display name of the planet (`"GEONOSIS"`, `"NABOO"`, etc.).
    pub planet: Option<String>,
    /// Localization string key (`"planets.geonosis.name"`, etc.) when specified.
    pub planet_key: Option<String>,
    /// Ordered sequence of loading stage pictures (galaxy -> locale -> landsat -> hologram grid).
    pub stages: Vec<LoadStage>,
}

/// Metadata for one era (`"cw"` or `"gcw"`) on a map in `shell.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct MapEraInfo {
    /// Whether this era's sub-level was extracted (`Some(false)` marks an unextracted era).
    pub extracted: Option<bool>,
    /// Faction identifiers (`["rep", "cis"]` or `["all", "imp"]`) fighting in this era.
    pub teams: Vec<String>,
    /// Attacking team index (`1` or `2`) from the mission configuration, if present.
    pub attacker: Option<u32>,
    /// Unmodified JSON value of the era entry from `shell.json`.
    pub raw: Value,
}

impl MapEraInfo {
    /// Whether the import holds a playable battle for this era (`extracted !== false`).
    pub fn is_extracted(&self) -> bool {
        self.extracted != Some(false)
    }
}

/// One map entry in `shell.json`'s `"maps"`:
/// `{ family, id, name, name_key, extracted, eras, fly, load }`.
///
/// Mirrors `readMaps` in `src/shell/data.js` plus `family` (`swbf1` by default,
/// or parsed from `"swbf1/hot1"` / `"swbf2/hot1"` per `MULTIPLAYER.md`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellMap {
    /// Game family (`swbf1` by default, or `swbf2` when prefixed with `"swbf2/"`).
    #[serde(default)]
    pub family: GameFamily,
    /// Bare lowercase map identifier (`"bes1"`, `"geo1"`, `"nab2"`, etc.).
    pub id: String,
    /// Localized display title (`"NABOO: THEED"`, etc.), falling back to `id.to_ascii_uppercase()`.
    pub name: String,
    /// Optional localization key (`"mapname.name.nab2"`, etc.) in `common.json` / `shell.json`.
    pub name_key: Option<String>,
    /// Top-level extraction flag (`None` when `shell.json` did not specify a boolean).
    pub extracted: Option<bool>,
    /// Per-era (`"cw"`, `"gcw"`) availability and team metadata.
    pub eras: BTreeMap<String, MapEraInfo>,
    /// Optional Instant Action flyover preview movie clip (`fades::FLY = [1.0, 1.0]`).
    pub fly: Option<MovieClip>,
    /// Optional loading screen planet and stage zoom sequence (`readMapLoad`).
    pub load: Option<MapLoad>,
}

impl ShellMap {
    /// Returns the eras (`"cw"`, `"gcw"`) that this map has extracted battles
    /// for, matching `eras(map)` in `src/shell/screens.js`.
    pub fn playable_eras(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for era in ["cw", "gcw"] {
            if let Some(info) = self.eras.get(era) {
                if info.is_extracted() {
                    out.push(era);
                }
            }
        }
        out
    }

    /// Returns the family-prefixed map id (`"swbf1/nab2"`).
    pub fn prefixed_id(&self) -> String {
        format!("{}/{}", self.family, self.id)
    }

    /// Returns the `common.json` texture name for this map's still preview
    /// (`"map_<id>"`, or `"map_unk"` for `tat3` / unknown maps).
    pub fn preview_still(&self) -> String {
        format!("map_{}", self.id)
    }
}

/// Crosshair edge texture paths for the loading screen (`{ horz, vert, corner }`).
/// Mirrors `edges` in `readLoad` in `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoadEdges {
    /// Horizontal crosshair line texture path (`load_edge_horz`).
    pub horz: Option<String>,
    /// Vertical crosshair line texture path (`load_edge_vert`).
    pub vert: Option<String>,
    /// Optional crosshair corner texture path.
    pub corner: Option<String>,
}

/// Shared loading screen art and timing `{ backdrop, disk, leds, edges, text,
/// track, zoom, hold }`.
/// Mirrors `readLoad` in `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoadChrome {
    /// Fallback backdrop texture path shown when a map has no stage pictures.
    pub backdrop: Option<String>,
    /// Rotating `LOADING` disk texture path (`load_rotating_disk`).
    pub disk: Option<String>,
    /// LED square texture paths (`load_led_0`, etc.).
    pub leds: Vec<String>,
    /// Crosshair tracking line textures (`horz`, `vert`, `corner`).
    pub edges: LoadEdges,
    /// Localization string key for the `"LOADING"` label (`"common.loading"` by default).
    pub text: String,
    /// Duration in seconds (`3.5` default) of the horizontal + vertical crosshair tracking phase.
    pub track: f64,
    /// Duration in seconds (`1.0` default) of each stage zoom transition.
    pub zoom: f64,
    /// Duration in seconds (`0.0` default) to hold the final stage after the last zoom finishes.
    pub hold: f64,
}

impl Default for LoadChrome {
    fn default() -> Self {
        Self {
            backdrop: None,
            disk: None,
            leds: Vec::new(),
            edges: LoadEdges::default(),
            text: "common.loading".to_string(),
            track: 3.5,
            zoom: 1.0,
            hold: 0.0,
        }
    }
}

/// Music track entry from `shell.json`'s `"music"`:
/// `{ name, url, properties, soundtrack }`.
/// Mirrors the `music` array entries in `readShellData` in `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicTrack {
    /// Cue identifier (`"shell_clonewars"`, `"shell_galacticconquest"`, or `"track1"`).
    pub name: String,
    /// Resolved content URL of the `.opus` / `.ogg` music file.
    pub url: String,
    /// Optional list of `SoundProperties` group names this track belongs to (`"shell_soundtrack"`, etc.).
    pub properties: Option<Vec<String>>,
    /// Whether this track participates in the default `shell_soundtrack` rotation (`soundtrack !== false`).
    pub soundtrack: bool,
}

/// Sound override entry from `shell.json`'s `"sounds"`:
/// `{ url, gain, pitch }`.
/// Mirrors the `sounds` object entries in `readShellData` in `src/shell/data.js`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellSoundDef {
    /// Resolved content URL of the sound file.
    pub url: String,
    /// Linear amplitude multiplier (`1.0` default).
    pub gain: f64,
    /// Playback pitch multiplier (`1.0` default).
    pub pitch: f64,
}

/// Bitmap font descriptor from `common.json`'s `"fonts"`.
/// Each glyph row has up to 11 numbers:
/// `[code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellFont {
    /// Vertical distance in pixels between baselines (`16.0` default).
    pub line_height: f64,
    /// Distance in pixels from the top of the line box to the alphabetic baseline.
    pub baseline: f64,
    /// Horizontal and vertical glyph scale factors (`[1.0, 1.0]` default).
    pub scale: [f64; 2],
    /// Texture names in `common.json` for each atlas page of this bitmap font.
    pub pages: Vec<String>,
    /// Glyph metric and UV rows `[code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1]`.
    pub glyphs: Vec<Vec<f64>>,
}

/// Lookup table resolving a `common.json` texture name to its content path
/// (using the 4x upscale in `/common/raster-hud/` when `manifest.json` has one,
/// else `/common/<path>`).
/// Mirrors `textureFiles(common, raster)` in `src/shell/data.js`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureFiles {
    /// Map from `common.json` texture name (`"load_rotating_disk"`) to relative path (`"textures/load_rotating_disk.png"`).
    common_textures: BTreeMap<String, String>,
    /// Map from `common.json` relative texture path to its 4x upscaled filename in `raster-hud/manifest.json`.
    raster_files: BTreeMap<String, String>,
}

impl TextureFiles {
    /// Resolves a `common.json` texture name (e.g. `"load_rotating_disk"`) to
    /// its content path, or `None` if `common.json` does not define `name`.
    pub fn get(&self, name: &str) -> Option<String> {
        let path = self.common_textures.get(name)?;
        if let Some(upscaled) = self.raster_files.get(path) {
            Some(format!("/common/raster-hud/{upscaled}"))
        } else {
            Some(format!("/common/{path}"))
        }
    }

    /// Returns the raw `common.json` texture map (`name -> relative_path`)
    /// for callers that inspect available HUD and shell textures directly.
    pub fn common_textures(&self) -> &BTreeMap<String, String> {
        &self.common_textures
    }
}

/// Normalized shell data returned by [`read_shell_data`].
/// Mirrors the object returned by `readShellData(shell, common, raster)` in
/// `src/shell/data.js`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShellData {
    /// `true` when `shell.json` was absent or `null` (`missing: !shell` in JS).
    pub missing: bool,
    /// Resolver mapping `common.json` texture names to `/common/raster-hud/` or `/common/` paths.
    pub textures: TextureFiles,
    /// Screen movies indexed by both raw lowercased `shell.json` keys and canonical screen ids.
    pub movies: BTreeMap<String, ScreenMovie>,
    /// Extracted map entries with per-era metadata, flyover clips, and loading stage sequences.
    pub maps: Vec<ShellMap>,
    /// Shell music tracks (`shell_soundtrack` and screen-specific cues like `shell_galacticconquest`).
    pub music: Vec<MusicTrack>,
    /// Transition whoosh sound URLs (`shell_transition1..10`).
    pub whooshes: Vec<String>,
    /// Sound overrides declared directly in `shell.json`'s `"sounds"` table.
    pub sounds: BTreeMap<String, ShellSoundDef>,
    /// Loading screen disk, LED, crosshair edge textures, and `track`/`zoom`/`hold` timing.
    pub load: LoadChrome,
    /// Merged localization string table (`common.strings` overlaid with `shell.strings`).
    pub strings: BTreeMap<String, String>,
    /// Bitmap font descriptors (`gamefont_tiny`, `gamefont_small`, `gamefont_medium`, `gamefont_large`).
    pub fonts: BTreeMap<String, ShellFont>,
    /// Resolved texture URLs for every atlas page referenced by `fonts`.
    pub pages: BTreeMap<String, String>,
}

/// Resolves a path from `shell.json`: relative to `root` (defaulting to
/// [`SHELL_ROOT`]), or unchanged when it starts with `"/"` or a URI scheme.
///
/// Mirrors `resolveUrl(path, root)` in `src/shell/data.js`.
pub fn resolve_url(path: Option<&str>, root: &str) -> Option<String> {
    let path = path?;
    if path.is_empty() {
        return None;
    }
    if path.starts_with('/') || has_scheme(path) {
        return Some(path.to_string());
    }
    let stripped = path.strip_prefix("./").unwrap_or(path);
    Some(format!("{root}/{stripped}"))
}

/// Checks whether `path` begins with an RFC 3986 URI scheme (`^[a-zA-Z][a-zA-Z0-9+.-]*:`).
/// Mirrors `/^[a-z][a-z0-9+.-]*:/i.test(path)` in `resolveUrl` (`src/shell/data.js`).
fn has_scheme(path: &str) -> bool {
    let Some((scheme, _)) = path.split_once(':') else {
        return false;
    };
    let mut bytes = scheme.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'.' || b == b'-')
}

/// Coerces an optional JSON number or non-empty numeric string into a finite `f64`,
/// returning `fallback` for `None`, `null`, `""`, or `NaN`/`±inf`.
/// Mirrors `const number = (value, fallback) => ...` in `src/shell/data.js`.
fn number(value: Option<&Value>, fallback: f64) -> f64 {
    let Some(v) = value else {
        return fallback;
    };
    if v.is_null() {
        return fallback;
    }
    if let Some(n) = v.as_f64() {
        if n.is_finite() {
            return n;
        }
    } else if let Some(s) = v.as_str() {
        if !s.is_empty() {
            if let Ok(n) = s.parse::<f64>() {
                if n.is_finite() {
                    return n;
                }
            }
        }
    }
    fallback
}

/// Normalizes one movie clip `{ url, fade_in, fade_out }` from a path string or
/// `{ file | url | movie, fadeIn, fadeOut }`, using `own` and `[fade_in, fade_out]`
/// defaults where omitted.
///
/// Mirrors `readClip(value, [fadeIn, fadeOut], own)` in `src/shell/data.js`.
/// Normalizes one movie clip `{ url, fade_in, fade_out }` from a path string or
/// `{ file | url | movie, fadeIn, fadeOut }`, resolving `fadeIn` and `fadeOut`
/// in priority order: explicit clip field -> `own_fades` override (`OWN_FADES`)
/// -> `default_fades` (`FADES.intro`, `FADES.loop`, or `FADES.fly`).
/// Returns `None` if `value` has no non-empty movie path.
pub fn read_clip(
    value: Option<&Value>,
    default_fades: [f64; 2],
    own_fades: [Option<f64>; 2],
) -> Option<MovieClip> {
    let value = value?;
    let path = if let Some(s) = value.as_str() {
        Some(s)
    } else if let Some(obj) = value.as_object() {
        obj.get("file")
            .or_else(|| obj.get("url"))
            .or_else(|| obj.get("movie"))
            .and_then(Value::as_str)
    } else {
        None
    };
    let url = resolve_url(path, SHELL_ROOT)?;
    let obj = value.as_object();
    let def_in = own_fades[0].filter(|n| n.is_finite()).unwrap_or(default_fades[0]);
    let def_out = own_fades[1].filter(|n| n.is_finite()).unwrap_or(default_fades[1]);
    let fade_in = number(obj.and_then(|o| o.get("fadeIn")), def_in);
    let fade_out = number(obj.and_then(|o| o.get("fadeOut")), def_out);
    Some(MovieClip {
        url,
        fade_in,
        fade_out,
    })
}

/// Returns per-screen `(intro_fades, loop_fades)` overrides from `OWN_FADES` in
/// `src/shell/data.js`:
/// - `"main"`: intro cuts straight into loop (`[0, 0]`), loop fades `[0, 0.5]`.
/// - `"briefing"` and `"meta"`: intro fades both ways over 1 second (`[1, 1]`).
fn own_screen_fades(id: &str) -> (Option<[f64; 2]>, Option<[f64; 2]>) {
    match id {
        "main" => (Some([0.0, 0.0]), Some([0.0, 0.5])),
        "briefing" => (Some([1.0, 1.0]), None),
        "meta" => (Some([1.0, 1.0]), None),
        _ => (None, None),
    }
}

/// Parses a single `shell.json` movie entry (`{ intro, loop | background, fadeIn,
/// fadeOut, introFadeIn, introFadeOut }` or a bare loop path string) into a
/// [`ScreenMovie`], applying `own_intro` / `own_loop` overrides before [`fades`].
fn read_screen_movie(
    entry: &Value,
    own_intro: Option<[f64; 2]>,
    own_loop: Option<[f64; 2]>,
) -> Option<ScreenMovie> {
    let obj = entry.as_object();
    let intro_fades = own_intro.unwrap_or(fades::INTRO);
    let loop_fades = own_loop.unwrap_or(fades::LOOP);

    let intro_own = [
        obj.and_then(|o| o.get("introFadeIn")).and_then(Value::as_f64),
        obj.and_then(|o| o.get("introFadeOut")).and_then(Value::as_f64),
    ];
    let intro = read_clip(obj.and_then(|o| o.get("intro")), intro_fades, intro_own);

    let loop_val = if let Some(o) = obj {
        o.get("loop").or_else(|| o.get("background"))
    } else if entry.is_string() {
        Some(entry)
    } else {
        None
    };
    let loop_own = [
        obj.and_then(|o| o.get("fadeIn")).and_then(Value::as_f64),
        obj.and_then(|o| o.get("fadeOut")).and_then(Value::as_f64),
    ];
    let loop_clip = read_clip(loop_val, loop_fades, loop_own);

    if intro.is_some() || loop_clip.is_some() {
        Some(ScreenMovie { intro, loop_clip })
    } else {
        None
    }
}

/// Normalizes `shell.json`'s `"movies"` under both each entry's own lowercased
/// name and the port's screen ids ([`MOVIE_SCREENS`]).
///
/// Mirrors `readMovies(movies)` in `src/shell/data.js`.
pub fn read_movies(movies: Option<&Value>) -> BTreeMap<String, ScreenMovie> {
    let mut out = BTreeMap::new();
    let mut by_name: Vec<(String, &Value)> = Vec::new();
    if let Some(obj) = movies.and_then(Value::as_object) {
        for (k, v) in obj {
            let lower = k.to_ascii_lowercase();
            if let Some(existing) = by_name.iter_mut().find(|(ek, _)| ek == &lower) {
                existing.1 = v;
            } else {
                by_name.push((lower, v));
            }
        }
    }
    // First preserve every entry under its own lowercased name (`ifs_meta_load`, etc.)
    // using the base `FADES` defaults.
    for (name, entry) in &by_name {
        if !entry.is_null() {
            if let Some(movie) = read_screen_movie(entry, None, None) {
                out.insert(name.clone(), movie);
            }
        }
    }
    // Then bind each canonical screen id (`start`, `main`, `sp`, `options`,
    // `briefing`, `meta`, `galaxy`) to the first matching alias in `MOVIE_SCREENS`,
    // applying `own_screen_fades(id)`.
    for (id, aliases) in MOVIE_SCREENS {
        let found = std::iter::once(id)
            .chain(aliases.iter().copied())
            .find_map(|candidate| {
                by_name
                    .iter()
                    .find(|(k, v)| k == candidate && !v.is_null())
                    .map(|(_, v)| *v)
            });
        if let Some(entry) = found {
            let (own_intro, own_loop) = own_screen_fades(id);
            if let Some(movie) = read_screen_movie(entry, own_intro, own_loop) {
                out.insert(id.to_string(), movie);
            }
        }
    }
    out
}

/// Parses a 4-number target rectangle `[x, y, w, h]` from an array or an
/// `{ x, y, w | width, h | height }` object.
/// Mirrors `rect(value)` in `src/shell/data.js`.
fn rect(value: Option<&Value>) -> Option<[f64; 4]> {
    let value = value?;
    if let Some(arr) = value.as_array() {
        if arr.len() < 4 {
            return None;
        }
        let mut out = [0.0; 4];
        for (i, item) in arr.iter().take(4).enumerate() {
            let n = item.as_f64().filter(|v| v.is_finite())?;
            out[i] = n;
        }
        return Some(out);
    }
    let obj = value.as_object()?;
    let x = obj.get("x")?.as_f64().filter(|v| v.is_finite())?;
    let y = obj.get("y")?.as_f64().filter(|v| v.is_finite())?;
    let w = obj
        .get("w")
        .or_else(|| obj.get("width"))?
        .as_f64()
        .filter(|v| v.is_finite())?;
    let h = obj
        .get("h")
        .or_else(|| obj.get("height"))?
        .as_f64()
        .filter(|v| v.is_finite())?;
    Some([x, y, w, h])
}

/// Reads a map's loading screen `{ planet, planet_key, stages }` from `load`.
/// Mirrors `readMapLoad(load, strings)` in `src/shell/data.js`.
pub fn read_map_load(load: Option<&Value>, strings: &BTreeMap<String, String>) -> Option<MapLoad> {
    let load = load?;
    if load.is_null() {
        return None;
    }
    let obj = load.as_object();
    let list = if let Some(arr) = load.as_array() {
        Some(arr)
    } else if let Some(o) = obj {
        o.get("stages")
            .or_else(|| o.get("zoom"))
            .or_else(|| o.get("images"))
            .and_then(Value::as_array)
    } else {
        None
    };

    // Extract each stage's image URL (`image`, `texture`, `file`, `url`, or a
    // bare string) and optional 4-number zoom target rectangle (`target` or `rect`),
    // dropping stages whose URL is missing or empty.
    let mut stages = Vec::new();
    if let Some(arr) = list {
        for stage in arr {
            let raw_url = if let Some(s) = stage.as_str() {
                Some(s)
            } else if let Some(s_obj) = stage.as_object() {
                s_obj
                    .get("image")
                    .or_else(|| s_obj.get("texture"))
                    .or_else(|| s_obj.get("file"))
                    .or_else(|| s_obj.get("url"))
                    .and_then(Value::as_str)
            } else {
                None
            };
            if let Some(url) = resolve_url(raw_url, SHELL_ROOT) {
                let target = stage.as_object().and_then(|s_obj| {
                    rect(s_obj.get("target").or_else(|| s_obj.get("rect")))
                });
                stages.push(LoadStage { url, target });
            }
        }
    }

    // Resolve the planet's display name: strings starting with `"planets."` are
    // treated as localization keys into `strings`, while any other string in
    // `planet` or `name` is used as a direct fallback label.
    let is_key = |v: &str| v.starts_with("planets.");
    let planet_str = obj.and_then(|o| o.get("planet")).and_then(Value::as_str);
    let name_str = obj.and_then(|o| o.get("name")).and_then(Value::as_str);
    let key = obj
        .and_then(|o| o.get("planetKey").or_else(|| o.get("nameKey")))
        .and_then(Value::as_str)
        .or_else(|| planet_str.filter(|s| is_key(s)))
        .map(str::to_string);
    let text = [planet_str, name_str]
        .into_iter()
        .flatten()
        .find(|s| !is_key(s))
        .map(str::to_string);
    let planet = key
        .as_deref()
        .and_then(|k| strings.get(k))
        .filter(|s| !s.is_empty())
        .cloned()
        .or(text);

    Some(MapLoad {
        planet,
        planet_key: key,
        stages,
    })
}

/// Reads `shell.json`'s `"maps"` array or object into a list of [`ShellMap`]s.
/// Mirrors `readMaps(maps, strings)` in `src/shell/data.js`.
pub fn read_maps(maps: Option<&Value>, strings: &BTreeMap<String, String>) -> Vec<ShellMap> {
    let Some(maps) = maps else {
        return Vec::new();
    };
    // Accept `maps` either as an array of `{ id, ... }` objects or as an
    // `{ [id]: { ... } }` map object, matching `readMaps` in `src/shell/data.js`.
    let mut entries: Vec<(String, &Value)> = Vec::new();
    if let Some(arr) = maps.as_array() {
        for item in arr {
            if let Some(id) = item.as_object().and_then(|o| o.get("id")).and_then(Value::as_str) {
                entries.push((id.to_string(), item));
            }
        }
    } else if let Some(obj) = maps.as_object() {
        for (id, val) in obj {
            if val.is_object() {
                entries.push((id.clone(), val));
            }
        }
    }

    entries
        .into_iter()
        .map(|(raw_id, map)| {
            let obj = map.as_object();
            // Lowercase the map id and split any `"swbf1/"` or `"swbf2/"` family
            // prefix; an explicit `"family"` field on the map object overrides the prefix.
            let lower_id = raw_id.to_ascii_lowercase();
            let (id_family, bare_id) = match lower_id.split_once('/') {
                Some((f, rest)) => (GameFamily::parse(f).unwrap_or(GameFamily::Swbf1), rest.to_string()),
                None => (GameFamily::Swbf1, lower_id),
            };
            let family = obj
                .and_then(|o| o.get("family"))
                .and_then(Value::as_str)
                .and_then(GameFamily::parse)
                .unwrap_or(id_family);
            let name_key = obj
                .and_then(|o| o.get("nameKey"))
                .and_then(Value::as_str)
                .map(str::to_string);
            // Resolve the map's display title: explicit `map.name` first, then
            // `strings[map.nameKey]`, falling back to `bare_id.to_ascii_uppercase()`.
            let name = obj
                .and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| name_key.as_deref().and_then(|k| strings.get(k)).cloned())
                .unwrap_or_else(|| bare_id.to_ascii_uppercase());
            let extracted = obj.and_then(|o| o.get("extracted")).and_then(Value::as_bool);
            // Copy per-era (`"cw"`, `"gcw"`) extraction status, faction `teams`,
            // and `attacker` index from `map.eras`.
            let mut eras = BTreeMap::new();
            if let Some(eras_obj) = obj.and_then(|o| o.get("eras")).and_then(Value::as_object) {
                for (era_id, era_val) in eras_obj {
                    let e_obj = era_val.as_object();
                    let era_extracted =
                        e_obj.and_then(|eo| eo.get("extracted")).and_then(Value::as_bool);
                    let teams = e_obj
                        .and_then(|eo| eo.get("teams"))
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    let attacker = e_obj
                        .and_then(|eo| eo.get("attacker"))
                        .and_then(Value::as_u64)
                        .map(|n| n as u32);
                    eras.insert(
                        era_id.clone(),
                        MapEraInfo {
                            extracted: era_extracted,
                            teams,
                            attacker,
                            raw: era_val.clone(),
                        },
                    );
                }
            }
            let fly = read_clip(obj.and_then(|o| o.get("fly")), fades::FLY, [None, None]);
            let load = read_map_load(obj.and_then(|o| o.get("load")), strings);
            ShellMap {
                family,
                id: bare_id,
                name,
                name_key,
                extracted,
                eras,
                fly,
                load,
            }
        })
        .collect()
}

/// Reads the loading screen's own art and timing `{ backdrop, disk, leds, edges,
/// text, track, zoom, hold }`.
///
/// Art entries that contain no `'/'` or `'.'` are resolved as `common.json`
/// texture names via `textures.get(name)`; paths with `'/'` or `'.'` are
/// resolved via [`resolve_url`].
///
/// Mirrors `readLoad(load, texture)` in `src/shell/data.js`.
pub fn read_load(load: Option<&Value>, textures: &TextureFiles) -> LoadChrome {
    let obj = load.and_then(Value::as_object);
    // Resolve a loading art reference: bare identifiers without `'/'` or `'.'`
    // are looked up in `common.json` via `textures.get(s)` (preferring 4x
    // upscales in `/common/raster-hud/`), while file paths are resolved relative
    // to `SHELL_ROOT` (`/common/shell`), matching `art(value)` in `readLoad`
    // (`src/shell/data.js`).
    let art = |val: Option<&Value>| -> Option<String> {
        let s = val?.as_str()?;
        if s.is_empty() {
            return None;
        }
        if !s.contains('/') && !s.contains('.') {
            textures.get(s)
        } else {
            resolve_url(Some(s), SHELL_ROOT)
        }
    };

    // Read `load.targetTime` / `load.timing` (`[track, zoom, hold]` array or
    // `{ track | stage, zoom, hold }` object); defaults are `track = 3.5s`,
    // `zoom = 1.0s`, and `hold = 0.0s` (`LoadDisplay`'s `TargetTime` pair).
    let pair = obj.and_then(|o| o.get("targetTime").or_else(|| o.get("timing")));
    let (track_val, zoom_val, hold_val) = if let Some(arr) = pair.and_then(Value::as_array) {
        (arr.first(), arr.get(1), arr.get(2))
    } else if let Some(p_obj) = pair.and_then(Value::as_object) {
        (
            p_obj.get("track").or_else(|| p_obj.get("stage")),
            p_obj.get("zoom"),
            p_obj.get("hold"),
        )
    } else {
        (None, None, None)
    };

    // Resolve the horizontal, vertical, and corner crosshair textures from
    // `load.crosshair` or `load.edges` (`horz`/`horizontal`, `vert`/`vertical`, `corner`).
    let edges_obj = obj
        .and_then(|o| o.get("crosshair").or_else(|| o.get("edges")))
        .and_then(Value::as_object);
    let edges = LoadEdges {
        horz: art(edges_obj.and_then(|e| e.get("horz").or_else(|| e.get("horizontal")))),
        vert: art(edges_obj.and_then(|e| e.get("vert").or_else(|| e.get("vertical")))),
        corner: art(edges_obj.and_then(|e| e.get("corner"))),
    };

    let leds = obj
        .and_then(|o| o.get("leds"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|v| art(Some(v))).collect())
        .unwrap_or_default();

    let text = obj
        .and_then(|o| o.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("common.loading")
        .to_string();

    LoadChrome {
        backdrop: art(obj.and_then(|o| o.get("backdrop"))),
        disk: art(obj.and_then(|o| o.get("disk").or_else(|| o.get("rotatingDisk")))),
        leds,
        edges,
        text,
        track: number(track_val, 3.5),
        zoom: number(zoom_val, 1.0),
        hold: number(hold_val, 0.0),
    }
}

/// Builds a [`TextureFiles`] lookup from `common.json` and
/// `raster-hud/manifest.json`.
///
/// Mirrors `textureFiles(common, raster)` in `src/shell/data.js`:
/// `common.textures[name]` gives `path` (e.g. `"textures/load_rotating_disk.png"`),
/// and `raster.textures[path].file` gives its 4x upscale under
/// `/common/raster-hud/` when present, else `/common/${path}`.
pub fn texture_files(common: Option<&Value>, raster: Option<&Value>) -> TextureFiles {
    let mut common_textures = BTreeMap::new();
    if let Some(obj) = common
        .and_then(|c| c.get("textures"))
        .and_then(Value::as_object)
    {
        for (name, path) in obj {
            if let Some(s) = path.as_str() {
                common_textures.insert(name.clone(), s.to_string());
            }
        }
    }
    let mut raster_files = BTreeMap::new();
    if let Some(obj) = raster
        .and_then(|r| r.get("textures"))
        .and_then(Value::as_object)
    {
        for (path, info) in obj {
            if let Some(file) = info
                .as_object()
                .and_then(|o| o.get("file"))
                .and_then(Value::as_str)
            {
                raster_files.insert(path.clone(), file.to_string());
            }
        }
    }
    TextureFiles {
        common_textures,
        raster_files,
    }
}

/// Reads bitmap fonts and their resolved page texture paths from `common.json`
/// and `raster-hud/manifest.json`.
///
/// Mirrors `readFonts(common, raster)` in `src/shell/data.js`.
pub fn read_fonts(
    common: Option<&Value>,
    raster: Option<&Value>,
) -> (BTreeMap<String, ShellFont>, BTreeMap<String, String>) {
    let file = texture_files(common, raster);
    let mut fonts = BTreeMap::new();
    let mut pages = BTreeMap::new();

    if let Some(fonts_obj) = common
        .and_then(|c| c.get("fonts"))
        .and_then(Value::as_object)
    {
        for (name, font_val) in fonts_obj {
            let Some(f_obj) = font_val.as_object() else {
                continue;
            };
            // Read bitmap font metrics (`lineHeight`, `baseline`, `scale`), resolve
            // each atlas page texture name through `TextureFiles`, and collect
            // numeric glyph rows (`[code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1]`).
            let line_height = number(f_obj.get("lineHeight"), 16.0);
            let baseline = number(f_obj.get("baseline"), line_height);
            let scale = f_obj
                .get("scale")
                .and_then(Value::as_array)
                .filter(|a| a.len() >= 2)
                .map(|a| [number(a.first(), 1.0), number(a.get(1), 1.0)])
                .unwrap_or([1.0, 1.0]);
            let mut font_pages = Vec::new();
            if let Some(p_arr) = f_obj.get("pages").and_then(Value::as_array) {
                for p in p_arr {
                    if let Some(page_name) = p.as_str() {
                        font_pages.push(page_name.to_string());
                        if let Some(url) = file.get(page_name) {
                            pages.insert(page_name.to_string(), url);
                        }
                    }
                }
            }
            let mut glyphs = Vec::new();
            if let Some(g_arr) = f_obj.get("glyphs").and_then(Value::as_array) {
                for g in g_arr {
                    if let Some(row_arr) = g.as_array().filter(|a| a.len() >= 8) {
                        let row: Vec<f64> =
                            row_arr.iter().map(|v| number(Some(v), 0.0)).collect();
                        glyphs.push(row);
                    }
                }
            }
            fonts.insert(
                name.clone(),
                ShellFont {
                    line_height,
                    baseline,
                    scale,
                    pages: font_pages,
                    glyphs,
                },
            );
        }
    }
    (fonts, pages)
}

/// Normalizes `shell.json`, `common.json`, and `raster-hud/manifest.json` into
/// a [`ShellData`] struct.
///
/// Mirrors `readShellData(shell, common, raster)` in `src/shell/data.js`.
pub fn read_shell_data(
    shell: Option<&Value>,
    common: Option<&Value>,
    raster: Option<&Value>,
) -> ShellData {
    // Merge localization strings with `shell.strings` taking precedence over
    // `common.strings` (`{ ...common.strings, ...shell.strings }` in JS).
    let mut strings = BTreeMap::new();
    if let Some(c_strings) = common
        .and_then(|c| c.get("strings"))
        .and_then(Value::as_object)
    {
        for (k, v) in c_strings {
            if let Some(s) = v.as_str() {
                strings.insert(k.clone(), s.to_string());
            }
        }
    }
    if let Some(s_strings) = shell
        .and_then(|s| s.get("strings"))
        .and_then(Value::as_object)
    {
        for (k, v) in s_strings {
            if let Some(s) = v.as_str() {
                strings.insert(k.clone(), s.to_string());
            }
        }
    }

    // Normalize `shell.sounds` overrides (`name -> { url, gain, pitch }`),
    // defaulting `gain` and `pitch` to `1.0` when omitted.
    let mut sounds = BTreeMap::new();
    if let Some(s_sounds) = shell
        .and_then(|s| s.get("sounds"))
        .and_then(Value::as_object)
    {
        for (name, value) in s_sounds {
            let raw_path = if let Some(s) = value.as_str() {
                Some(s)
            } else if let Some(obj) = value.as_object() {
                obj.get("file").or_else(|| obj.get("url")).and_then(Value::as_str)
            } else {
                None
            };
            if let Some(url) = resolve_url(raw_path, SHELL_ROOT) {
                let obj = value.as_object();
                let gain = number(obj.and_then(|o| o.get("gain")), 1.0);
                let pitch = number(obj.and_then(|o| o.get("pitch")), 1.0);
                sounds.insert(name.clone(), ShellSoundDef { url, gain, pitch });
            }
        }
    }

    let textures = texture_files(common, raster);
    let movies = read_movies(shell.and_then(|s| s.get("movies")));
    let maps = read_maps(shell.and_then(|s| s.get("maps")), &strings);

    // Normalize `shell.music`: each entry may be a path string or an object
    // `{ name, file | url, properties, soundtrack }`, defaulting `name` to
    // `"track1"`, `"track2"`, ... and `soundtrack` to `true` unless explicitly `false`.
    let mut music = Vec::new();
    if let Some(m_arr) = shell
        .and_then(|s| s.get("music"))
        .and_then(Value::as_array)
    {
        for (i, track) in m_arr.iter().enumerate() {
            let obj = track.as_object();
            let raw_path = if let Some(s) = track.as_str() {
                Some(s)
            } else if let Some(o) = obj {
                o.get("file").or_else(|| o.get("url")).and_then(Value::as_str)
            } else {
                None
            };
            let Some(url) = resolve_url(raw_path, SHELL_ROOT) else {
                continue;
            };
            let name = obj
                .and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("track{}", i + 1));
            let properties = obj
                .and_then(|o| o.get("properties"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                });
            let soundtrack = obj
                .and_then(|o| o.get("soundtrack"))
                .and_then(Value::as_bool)
                != Some(false);
            music.push(MusicTrack {
                name,
                url,
                properties,
                soundtrack,
            });
        }
    }

    // Normalize `shell.whooshes` (`shell_transition1..10` camera flight sounds),
    // accepting either path strings or `{ file | url }` objects.
    let mut whooshes = Vec::new();
    if let Some(w_arr) = shell
        .and_then(|s| s.get("whooshes"))
        .and_then(Value::as_array)
    {
        for item in w_arr {
            let raw_path = if let Some(s) = item.as_str() {
                Some(s)
            } else if let Some(o) = item.as_object() {
                o.get("file").or_else(|| o.get("url")).and_then(Value::as_str)
            } else {
                None
            };
            if let Some(url) = resolve_url(raw_path, SHELL_ROOT) {
                whooshes.push(url);
            }
        }
    }

    let load = read_load(shell.and_then(|s| s.get("load")), &textures);
    let (fonts, pages) = read_fonts(common, raster);

    ShellData {
        missing: shell.is_none() || shell == Some(&Value::Null),
        textures,
        movies,
        maps,
        music,
        whooshes,
        sounds,
        load,
        strings,
        fonts,
        pages,
    }
}

/// Loads `common/shell/shell.json`, `common/common.json`, and
/// `common/raster-hud/manifest.json` from a native `content_dir` on disk,
/// returning an error naming the exact file if reading or JSON parsing fails.
/// Strict filesystem loader for native callers that require all three extracted
/// manifests (`common/shell/shell.json`, `common/common.json`, and
/// `common/raster-hud/manifest.json`) to exist and parse cleanly under `content_dir`.
/// Returns an `Err` naming the exact file path and I/O or JSON error on failure.
pub fn try_load_shell_data_from_dir(content_dir: &Path) -> Result<ShellData, String> {
    let read_json = |rel: &str| -> Result<Value, String> {
        let file_path = content_dir.join(rel);
        let bytes = std::fs::read(&file_path)
            .map_err(|e| format!("{}: {e}", file_path.display()))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("{}: {e}", file_path.display()))
    };
    let shell = read_json("common/shell/shell.json")?;
    let common = read_json("common/common.json")?;
    let raster = read_json("common/raster-hud/manifest.json")?;
    Ok(read_shell_data(Some(&shell), Some(&common), Some(&raster)))
}

/// Loads `common/shell/shell.json`, `common/common.json`, and
/// `common/raster-hud/manifest.json` from a native `content_dir` on disk.
/// Mirrors `loadShellData` in `src/shell/data.js`.
/// Permissive filesystem loader that treats missing or unparseable manifests as
/// `None` (setting `ShellData::missing = true` when `shell.json` is absent) so
/// the shell can still boot with fallback defaults before extraction completes.
/// Mirrors `loadShellData` in `src/shell/data.js`.
pub fn load_shell_data_from_dir(content_dir: &Path) -> ShellData {
    let read_json = |rel: &str| -> Option<Value> {
        let bytes = std::fs::read(content_dir.join(rel)).ok()?;
        serde_json::from_slice(&bytes).ok()
    };
    let shell = read_json("common/shell/shell.json");
    let common = read_json("common/common.json");
    let raster = read_json("common/raster-hud/manifest.json");
    read_shell_data(shell.as_ref(), common.as_ref(), raster.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Verifies that `resolve_url` prefixes relative paths with `SHELL_ROOT`
    // (`/common/shell`), strips leading `./`, preserves root-relative paths,
    // and that `read_clip` fills in `fades::LOOP` (`[0.1, 0.5]`) when omitted.
    #[test]
    fn resolve_url_and_read_clip_normalize_paths_and_fades() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            resolve_url(Some("movies/main.webm"), SHELL_ROOT).as_deref(),
            Some("/common/shell/movies/main.webm")
        );
        assert_eq!(
            resolve_url(Some("./load/geo1.png"), SHELL_ROOT).as_deref(),
            Some("/common/shell/load/geo1.png")
        );
        assert_eq!(
            resolve_url(Some("/common/textures/x.png"), SHELL_ROOT).as_deref(),
            Some("/common/textures/x.png")
        );
        assert_eq!(resolve_url(None, SHELL_ROOT), None);

        // Bare string movie entry resolves under `SHELL_ROOT` and inherits `fades::LOOP`
        // (`fadeIn = 0.1`, `fadeOut = 0.5`).
        let clip = read_clip(Some(&json!("movies/atat.webm")), fades::LOOP, [None, None])
            .ok_or("expected clip")?;
        assert_eq!(clip.url, "/common/shell/movies/atat.webm");
        assert_eq!(clip.fade_in, 0.1);
        assert_eq!(clip.fade_out, 0.5);
        Ok(())
    }

    // Verifies end-to-end normalization of `shell.json`, `common.json`, and
    // `raster-hud/manifest.json`: map localization, playable era filtering,
    // `OWN_FADES` on `main`, poster still resolution, 4x upscaled HUD textures,
    // and 11-column bitmap font glyph rows.
    #[test]
    fn read_shell_data_populates_maps_movies_load_and_upscaled_textures() -> Result<(), Box<dyn std::error::Error>> {
        // Construct synthetic `shell.json`, `common.json`, and `raster-hud/manifest.json`
        // fixtures covering screen movies, map eras, loading stages, music, whooshes,
        // 4x upscaled HUD textures, and bitmap font glyph metrics.
        let shell = json!({
            "movies": {
                "ifs_main": { "intro": "movies/main_trans.webm", "loop": "movies/atat_screen.webm" }
            },
            "maps": [
                {
                    "id": "nab2",
                    "nameKey": "mapname.name.nab2",
                    "extracted": true,
                    "eras": {
                        "cw": { "teams": ["rep", "cis"], "attacker": 1, "extracted": true },
                        "gcw": { "teams": ["all", "imp"], "attacker": 1, "extracted": false }
                    },
                    "fly": "movies/nab2fly.webm",
                    "load": {
                        "planetKey": "planets.naboo.name",
                        "stages": [
                            { "image": "load/galaxy.png", "target": [10, 20, 30, 40] },
                            { "image": "load/level.png", "target": null }
                        ]
                    }
                }
            ],
            "music": [
                { "name": "shell_clonewars", "file": "audio/clonewars.opus", "properties": ["shell_soundtrack"] }
            ],
            "whooshes": ["audio/whoosh1.flac"],
            "load": {
                "disk": "load_rotating_disk",
                "edges": { "horz": "load_edge_horz", "vert": "load_edge_vert" },
                "timing": [3.5, 1.0, 0.25]
            }
        });
        let common = json!({
            "strings": {
                "mapname.name.nab2": "NABOO: THEED",
                "planets.naboo.name": "NABOO"
            },
            "textures": {
                "load_rotating_disk": "textures/load_rotating_disk.png",
                "load_edge_horz": "textures/load_edge_horz.png",
                "load_edge_vert": "textures/load_edge_vert.png",
                "font_p0": "textures/font_p0.png"
            },
            "fonts": {
                "gamefont_medium": {
                    "lineHeight": 20,
                    "baseline": 16,
                    "pages": ["font_p0"],
                    "glyphs": [[65, 0, 12, 0, 11, 0, 16, 0.0, 0.0, 0.5, 0.5]]
                }
            }
        });
        let raster = json!({
            "textures": {
                "textures/load_rotating_disk.png": { "file": "load_rotating_disk_4x.png" }
            }
        });

        // Normalize the three manifests and verify every section of `ShellData`:
        // 1. `maps[0]` resolves its localized name (`"NABOO: THEED"`), playable
        //    Clone Wars era (`["cw"]`), and 2-stage loading sequence on `"NABOO"`.
        // 2. `movies["main"]` applies `OWN_FADES.main` (`intro: [0, 0]`, `loop: [0, 0.5]`)
        //    and maps `"atat_screen.webm"` to the `"atat_screen"` poster still.
        // 3. `load.disk` resolves to the 4x upscale in `/common/raster-hud/`, while
        //    non-upscaled textures (`load_edge_horz`, `font_p0`) resolve under `/common/textures/`.
        let data = read_shell_data(Some(&shell), Some(&common), Some(&raster));
        assert!(!data.missing);
        assert_eq!(data.maps.len(), 1);
        assert_eq!(data.maps[0].id, "nab2");
        assert_eq!(data.maps[0].family, GameFamily::Swbf1);
        assert_eq!(data.maps[0].name, "NABOO: THEED");
        assert_eq!(data.maps[0].playable_eras(), vec!["cw"]);
        let map_load = data.maps[0].load.as_ref().ok_or("missing map load")?;
        assert_eq!(map_load.planet.as_deref(), Some("NABOO"));
        assert_eq!(map_load.stages.len(), 2);

        let main_movie = data.movies.get("main").ok_or("missing main movie")?;
        let main_intro = main_movie.intro.as_ref().ok_or("missing main intro")?;
        let main_loop = main_movie.loop_clip.as_ref().ok_or("missing main loop")?;
        assert_eq!(main_intro.fade_in, 0.0);
        assert_eq!(main_intro.fade_out, 0.0);
        assert_eq!(main_loop.fade_in, 0.0);
        assert_eq!(main_loop.fade_out, 0.5);
        assert_eq!(main_movie.poster_still(), Some("atat_screen"));

        assert_eq!(
            data.load.disk.as_deref(),
            Some("/common/raster-hud/load_rotating_disk_4x.png")
        );
        assert_eq!(
            data.load.edges.horz.as_deref(),
            Some("/common/textures/load_edge_horz.png")
        );
        assert_eq!(data.load.hold, 0.25);
        assert_eq!(
            data.pages.get("font_p0").map(String::as_str),
            Some("/common/textures/font_p0.png")
        );
        assert_eq!(data.fonts["gamefont_medium"].glyphs[0].len(), 11);
        Ok(())
    }
}
