//! Instant Action playlist and battle request rules (`src/launch.js` and
//! `MULTIPLAYER.md` lines 106-183).
//!
//! In the native client there are no browser URLs or query strings; a session
//! request is a typed [`BattleRequest`] that converts directly into
//! [`bfe_content_native::setup::BattleOptions`], and an Instant Action playlist
//! is a typed [`Playlist`] (`entries`, `round`, `random`) holding up to
//! [`MAX_PLAYLIST`] (64) entries.
//!
//! Every map, playlist entry, and battle request names its [`GameFamily`]
//! (`swbf1` today, `swbf2` later) and `mode` (defaulting to `"conquest"` for
//! 2004 content), parsed from and formatted to the family-prefixed form used by
//! the server configuration and `MULTIPLAYER.md` (`swbf1/nab2:rep:conquest`),
//! while continuing to accept and format compact rotation text (`geo1:cw`) where
//! `src/launch.js` uses bare `swbf1` conquest defaults.
//!
//! Ported from `src/launch.js`:
//! - `MAX_PLAYLIST = 64` (`ScriptCB_GetMaxMissionQueue` limit) and
//!   `ERAS = ["cw", "gcw"]`.
//! - `parseRotation` / `formatRotation`: `"geo1:cw,hot1:gcw"` and
//!   family/mode-prefixed `"swbf1/geo1:cw:conquest"` rotation text, lowercased,
//!   validated against `^[a-z0-9_]+$`, capped at 64 entries.
//! - `shuffle`: the 2004 game's `7 * n` random pair swaps where both indices
//!   are `min(floor(random() * n), n - 1)`.
//! - `launch` (ports `launchUrl`'s playlist launch rule): caps `entries` at
//!   `MAX_PLAYLIST`, shuffles first when `random == true`, starts at
//!   `round = 0`, and selects the first entry's `family`, `map`, `era`, and
//!   `mode`.
//! - `next` (ports `nextUrl`'s playlist progression rule): advances `round + 1`,
//!   wrapping to `0` after the last entry and reshuffling `entries` on the wrap
//!   when `random == true`, while dropping per-map picks (`side`, `mission`).
//!   Without a playlist, `next` restarts the same battle.

use bfe_content_native::setup::BattleOptions;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The original's mission queue limit (`ScriptCB_GetMaxMissionQueue`).
/// Mirrors `MAX_PLAYLIST` in `src/launch.js`.
pub const MAX_PLAYLIST: usize = 64;

/// Valid era identifiers in Instant Action playlists (`"cw"`, `"gcw"`).
/// Mirrors `ERAS` in `src/launch.js`.
pub const ERAS: [&str; 2] = ["cw", "gcw"];

/// Default game mode for 2004 content (`"conquest"`).
/// Mirrors the mode field in `MULTIPLAYER.md` ("Two games from day one").
pub const DEFAULT_MODE: &str = "conquest";

/// Which game a map, its rules, and its interface belong to: `swbf1` (2004) or
/// `swbf2` (2005).
///
/// Defined by `MULTIPLAYER.md` ("Two games from day one").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GameFamily {
    /// Star Wars: Battlefront (2004) (`"swbf1"`).
    #[default]
    #[serde(rename = "swbf1")]
    Swbf1,
    /// Star Wars: Battlefront II (2005) (`"swbf2"`).
    #[serde(rename = "swbf2")]
    Swbf2,
}

impl GameFamily {
    /// Canonical lowercase prefix (`"swbf1"` or `"swbf2"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Swbf1 => "swbf1",
            Self::Swbf2 => "swbf2",
        }
    }

    /// Display label in the Instant Action game picker (`src/shell/index.js`
    /// `gamePicker`).
    pub fn picker_label(self) -> &'static str {
        match self {
            Self::Swbf1 => "BATTLEFRONT (2004)",
            Self::Swbf2 => "BATTLEFRONT II (2005), PLANNED",
        }
    }

    /// Parses `"swbf1"` or `"swbf2"` (case-insensitive).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "swbf1" => Some(Self::Swbf1),
            "swbf2" => Some(Self::Swbf2),
            _ => None,
        }
    }
}

impl fmt::Display for GameFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One entry in an Instant Action or server playlist (`family`, `map`, `era`,
/// `mode`).
///
/// Mirrors the `{ map, era }` objects handled by `parseRotation` and
/// `formatRotation` in `src/launch.js` together with `family` and `mode` from
/// `MULTIPLAYER.md` (`swbf1/nab2:cw:conquest`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub family: GameFamily,
    pub map: String,
    pub era: Option<String>,
    pub mode: String,
}

impl PlaylistEntry {
    /// Creates an entry for `Swbf1` and `"conquest"`.
    pub fn new(map: impl Into<String>, era: Option<impl Into<String>>) -> Self {
        Self {
            family: GameFamily::Swbf1,
            map: map.into(),
            era: era.map(Into::into),
            mode: DEFAULT_MODE.to_string(),
        }
    }

    /// Creates an entry with an explicit [`GameFamily`] and `mode`.
    pub fn with_family_and_mode(
        family: GameFamily,
        map: impl Into<String>,
        era: Option<impl Into<String>>,
        mode: impl Into<String>,
    ) -> Self {
        let mode = mode.into();
        Self {
            family,
            map: map.into(),
            era: era.map(Into::into),
            mode: if mode.is_empty() {
                DEFAULT_MODE.to_string()
            } else {
                mode
            },
        }
    }

    /// Formats this entry in the full family-prefixed form used by server
    /// configs and `MULTIPLAYER.md`: `"swbf1/geo1:cw:conquest"` (or
    /// `"swbf1/geo1::conquest"` when `era` is `None`).
    pub fn format_prefixed(&self) -> String {
        let era = self.era.as_deref().unwrap_or("");
        if era.is_empty() {
            format!("{}/{}:{}", self.family, self.map, self.mode)
        } else {
            format!("{}/{}:{era}:{}", self.family, self.map, self.mode)
        }
    }
}

/// Instant Action playlist state carried across battles (`entries`, `round`,
/// `random`).
///
/// Mirrors the playlist state `{ entries, round, random }` in
/// `src/launch.js` (`readPlaylist`, `launchUrl`, `nextUrl`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Playlist {
    pub entries: Vec<PlaylistEntry>,
    pub round: usize,
    pub random: bool,
}

impl Playlist {
    /// Constructs a playlist from `entries`, wrapping `round` into
    /// `0..entries.len()` with Euclidean remainder (`((round % len) + len) % len`),
    /// matching `readPlaylist` and `battleUrl` in `src/launch.js`.
    pub fn new(entries: Vec<PlaylistEntry>, round: isize, random: bool) -> Option<Self> {
        let entries: Vec<PlaylistEntry> = entries.into_iter().take(MAX_PLAYLIST).collect();
        if entries.is_empty() {
            return None;
        }
        let len = entries.len() as isize;
        let wrapped = (((round % len) + len) % len) as usize;
        Some(Self {
            entries,
            round: wrapped,
            random,
        })
    }

    /// Returns the active [`PlaylistEntry`] for the current `round`.
    pub fn current(&self) -> Option<&PlaylistEntry> {
        if self.entries.is_empty() {
            None
        } else {
            let idx = self.round % self.entries.len();
            self.entries.get(idx)
        }
    }
}

/// Native battle request produced by the shell (or a playlist step) and
/// consumed by [`BattleOptions`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattleRequest {
    pub family: GameFamily,
    pub map: String,
    pub era: Option<String>,
    pub mode: String,
    pub mission: Option<String>,
    pub side: Option<String>,
    pub playlist: Option<Playlist>,
}

impl BattleRequest {
    /// Creates a single-battle request without a playlist, defaulting `family`
    /// to [`GameFamily::Swbf1`] and `mode` to [`DEFAULT_MODE`] (`"conquest"`).
    /// Mirrors `battleUrl({ map, era })` in `src/launch.js` when no rotation is
    /// given.
    pub fn single(map: impl Into<String>, era: Option<impl Into<String>>) -> Result<Self, String> {
        let raw_map = map.into();
        let (family, bare_map) = split_family_prefix(&raw_map).unwrap_or((GameFamily::Swbf1, &raw_map));
        if bare_map.is_empty() {
            return Err("battle request needs a map or a rotation".into());
        }
        Ok(Self {
            family,
            map: bare_map.to_string(),
            era: era.map(Into::into),
            mode: DEFAULT_MODE.to_string(),
            mission: None,
            side: None,
            playlist: None,
        })
    }

    /// Creates a battle request at `round` of `entries` (capped at
    /// [`MAX_PLAYLIST`]), defaulting `family`, `map`, `era`, and `mode` from
    /// `entries[index]` when not explicitly overridden.
    ///
    /// Ports the playlist entry selection of `battleUrl` in `src/launch.js`.
    pub fn from_rotation(
        map: Option<&str>,
        era: Option<&str>,
        entries: Vec<PlaylistEntry>,
        round: isize,
        random: bool,
    ) -> Result<Self, String> {
        let playlist = Playlist::new(entries, round, random);
        let entry = playlist.as_ref().and_then(Playlist::current);
        let (override_family, override_map) = match map.filter(|m| !m.is_empty()) {
            Some(m) => match split_family_prefix(m) {
                Some((f, bare)) => (Some(f), Some(bare.to_string())),
                None => (None, Some(m.to_string())),
            },
            None => (None, None),
        };
        let resolved_map = override_map
            .or_else(|| entry.map(|e| e.map.clone()))
            .ok_or_else(|| "battle request needs a map or a rotation".to_string())?;
        let matching_entry = entry.filter(|e| e.map == resolved_map);
        let resolved_family = override_family
            .or_else(|| matching_entry.map(|e| e.family))
            .unwrap_or(GameFamily::Swbf1);
        let resolved_era = era
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .or_else(|| matching_entry.and_then(|e| e.era.clone()));
        let resolved_mode = matching_entry
            .map(|e| e.mode.clone())
            .unwrap_or_else(|| DEFAULT_MODE.to_string());
        Ok(Self {
            family: resolved_family,
            map: resolved_map,
            era: resolved_era,
            mode: resolved_mode,
            mission: None,
            side: None,
            playlist,
        })
    }

    /// Launches an Instant Action playlist: slices `entries` to [`MAX_PLAYLIST`],
    /// shuffles them first when `random == true`, and starts at `round = 0`.
    ///
    /// Ports `launchUrl(entries, { random, rand })` in `src/launch.js`.
    pub fn launch<F>(entries: &[PlaylistEntry], random: bool, rand: F) -> Result<Self, String>
    where
        F: FnMut() -> f64,
    {
        let capped: Vec<PlaylistEntry> = entries.iter().take(MAX_PLAYLIST).cloned().collect();
        let list = if random {
            shuffle(&capped, rand)
        } else {
            capped
        };
        if list.is_empty() {
            return Err("launch needs at least one entry".into());
        }
        Self::from_rotation(None, None, list, 0, random)
    }

    /// Returns the same battle request unchanged for a restart.
    /// Ports `restartUrl` in `src/launch.js`.
    pub fn restart(&self) -> Self {
        self.clone()
    }

    /// Advances to the next battle in the playlist: increments `round + 1`,
    /// wraps to `0` after the last entry (reshuffling `entries` on the wrap when
    /// `playlist.random` is `true`), and clears single-map picks (`side`,
    /// `mission`). Without a playlist, restarts the current battle.
    ///
    /// Ports `nextUrl(from, { random })` in `src/launch.js`.
    pub fn next<F>(&self, rand: F) -> Self
    where
        F: FnMut() -> f64,
    {
        let Some(playlist) = &self.playlist else {
            return self.restart();
        };
        if playlist.entries.is_empty() {
            return self.restart();
        }
        let mut entries = playlist.entries.clone();
        let mut round = (playlist.round % entries.len()) + 1;
        if round >= entries.len() {
            round = 0;
            if playlist.random {
                entries = shuffle(&entries, rand);
            }
        }
        Self::from_rotation(None, None, entries, round as isize, playlist.random)
            .unwrap_or_else(|_| self.restart())
    }

    /// Formats this battle request in the family-prefixed form (`"swbf1/nab2:cw:conquest"`).
    pub fn format_prefixed(&self) -> String {
        let era = self.era.as_deref().unwrap_or("");
        if era.is_empty() {
            format!("{}/{}:{}", self.family, self.map, self.mode)
        } else {
            format!("{}/{}:{era}:{}", self.family, self.map, self.mode)
        }
    }

    /// Converts this battle request into [`BattleOptions`] for native battle
    /// setup (`bfe_content_native::setup`).
    ///
    /// `BattleOptions` selects the era via `side` (`"rep"`/`"cis"` for `"cw"`,
    /// `"all"`/`"imp"` for `"gcw"`), so when `side` is not explicitly set on the
    /// request, `era == "gcw"` maps to `"all"` and `"cw"` (default) maps to
    /// `"rep"`.
    pub fn to_battle_options(&self) -> BattleOptions {
        let side = self
            .side
            .clone()
            .unwrap_or_else(|| match self.era.as_deref() {
                Some("gcw") => "all".to_string(),
                _ => "rep".to_string(),
            });
        BattleOptions {
            map: self.map.clone(),
            side,
            ..BattleOptions::default()
        }
    }
}

fn split_family_prefix(raw: &str) -> Option<(GameFamily, &str)> {
    let (prefix, rest) = raw.split_once('/')?;
    let family = GameFamily::parse(prefix)?;
    Some((family, rest))
}

/// Parses `"geo1:cw,hot1:gcw"` or family/mode-prefixed
/// `"swbf1/geo1:cw:conquest,swbf2/hot1:gcw:ctf"` into a list of
/// [`PlaylistEntry`] items.
///
/// An entry may omit its family prefix (defaulting to [`GameFamily::Swbf1`]),
/// its era (`"geo1"`), or its mode (defaulting to [`DEFAULT_MODE`] `"conquest"`).
/// Malformed entries are dropped, and the list stops at [`MAX_PLAYLIST`].
///
/// Mirrors `parseRotation(text)` in `src/launch.js` and the family/mode rules
/// in `MULTIPLAYER.md` (lines 106-183).
pub fn parse_rotation(text: Option<&str>) -> Vec<PlaylistEntry> {
    let Some(text) = text else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for part in text.split(',') {
        let lower = part.trim().to_ascii_lowercase();
        if lower.is_empty() {
            continue;
        }
        let mut pieces = lower.split(':');
        let raw_map = pieces.next().unwrap_or("");
        let (family, map) = if raw_map.contains('/') {
            let Some((f, m)) = split_family_prefix(raw_map) else {
                continue;
            };
            (f, m)
        } else {
            (GameFamily::Swbf1, raw_map)
        };
        let era = pieces.next();
        let mode = pieces.next();
        let extra = pieces.next();
        if extra.is_some()
            || !is_ident(map)
            || era.is_some_and(|e| !e.is_empty() && !is_ident(e))
            || mode.is_some_and(|m| !m.is_empty() && !is_ident(m))
        {
            continue;
        }
        // In bare 2-field form (`bad:x:y`), if there was no family prefix and a
        // third field is not a known mode while the second isn't an era or side,
        // `src/launch.js` rejected `bad:x:y`. Preserve that rejection when no
        // family prefix is present and `era` is not a valid era/side identifier.
        if !raw_map.contains('/')
            && mode.is_some()
            && !era.is_some_and(|e| matches!(e, "cw" | "gcw" | "rep" | "cis" | "all" | "imp"))
        {
            continue;
        }
        let era = era.filter(|e| !e.is_empty()).map(str::to_string);
        let mode = mode
            .filter(|m| !m.is_empty())
            .unwrap_or(DEFAULT_MODE)
            .to_string();
        entries.push(PlaylistEntry {
            family,
            map: map.to_string(),
            era,
            mode,
        });
        if entries.len() >= MAX_PLAYLIST {
            break;
        }
    }
    entries
}

/// Formats a slice of [`PlaylistEntry`] items.
///
/// Entries with the default `Swbf1` family and `"conquest"` mode format as
/// `"geo1:cw,nab2"` (matching `formatRotation(entries)` in `src/launch.js`),
/// while entries with a non-default family (`Swbf2`) or non-default mode format
/// with their family prefix and mode (`"swbf2/hot1:gcw:ctf"`).
pub fn format_rotation(entries: &[PlaylistEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            if entry.family == GameFamily::Swbf1 && entry.mode == DEFAULT_MODE {
                match &entry.era {
                    Some(era) if !era.is_empty() => format!("{}:{era}", entry.map),
                    _ => entry.map.clone(),
                }
            } else {
                entry.format_prefixed()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Formats a slice of [`PlaylistEntry`] items unconditionally in the full
/// family-prefixed server configuration form (`"swbf1/geo1:cw:conquest"`).
pub fn format_rotation_prefixed(entries: &[PlaylistEntry]) -> String {
    entries
        .iter()
        .map(PlaylistEntry::format_prefixed)
        .collect::<Vec<_>>()
        .join(",")
}

/// Shuffles `entries` with the 2004 game's mission-queue algorithm: `7 * n`
/// swaps of two entries picked at random (`min(floor(random() * n), n - 1)`),
/// where `random()` yields `[0, 1)`.
///
/// Mirrors `shuffle(entries, random)` in `src/launch.js`.
pub fn shuffle<T: Clone, F>(entries: &[T], mut random: F) -> Vec<T>
where
    F: FnMut() -> f64,
{
    let mut list = entries.to_vec();
    let n = list.len();
    for _ in 0..(7 * n) {
        let a = ((random() * (n as f64)).floor() as isize).min((n as isize) - 1).max(0) as usize;
        let b = ((random() * (n as f64)).floor() as isize).min((n as isize) - 1).max(0) as usize;
        list.swap(a, b);
    }
    list
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seeded `[0, 1)` LCG matching `seeded(seed)` in `tests/launch.test.js`.
    fn seeded(seed: u32) -> impl FnMut() -> f64 {
        let mut s = seed;
        move || {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s as f64) / ((1u64 << 32) as f64)
        }
    }

    #[test]
    fn rotation_text_parses_map_era_family_and_mode_drops_malformed_and_caps_at_64() {
        assert_eq!(
            parse_rotation(Some("geo1:cw,hot1:gcw")),
            vec![
                PlaylistEntry::new("geo1", Some("cw")),
                PlaylistEntry::new("hot1", Some("gcw")),
            ]
        );
        assert_eq!(
            parse_rotation(Some(" GEO1:CW , nab2 ,, bad:x:y, ?:cw, tat2:, kam1:c-w")),
            vec![
                PlaylistEntry::new("geo1", Some("cw")),
                PlaylistEntry::new("nab2", None::<&str>),
                PlaylistEntry::new("tat2", None::<&str>),
            ]
        );
        // Family-prefixed and mode-qualified entries (`MULTIPLAYER.md`):
        let mixed = parse_rotation(Some("swbf1/nab2:cw:conquest,swbf2/hot1:gcw:ctf"));
        assert_eq!(
            mixed,
            vec![
                PlaylistEntry::with_family_and_mode(
                    GameFamily::Swbf1,
                    "nab2",
                    Some("cw"),
                    "conquest"
                ),
                PlaylistEntry::with_family_and_mode(GameFamily::Swbf2, "hot1", Some("gcw"), "ctf"),
            ]
        );
        assert_eq!(
            format_rotation_prefixed(&mixed),
            "swbf1/nab2:cw:conquest,swbf2/hot1:gcw:ctf"
        );
        assert_eq!(format_rotation(&mixed), "nab2:cw,swbf2/hot1:gcw:ctf");

        assert_eq!(parse_rotation(None), Vec::<PlaylistEntry>::new());

        let long = (0..70)
            .map(|i| format!("m{i}:cw"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(parse_rotation(Some(&long)).len(), MAX_PLAYLIST);
        assert_eq!(
            format_rotation(&[
                PlaylistEntry::new("geo1", Some("cw")),
                PlaylistEntry::new("nab2", None::<&str>),
            ]),
            "geo1:cw,nab2"
        );
    }

    #[test]
    fn battle_request_resolves_playlist_entry_and_wraps_out_of_range_rounds() {
        let entries = parse_rotation(Some("geo1:cw,swbf2/hot1:gcw:ctf"));
        let req = BattleRequest::from_rotation(None, None, entries, 1, true).unwrap();
        assert_eq!(req.family, GameFamily::Swbf2);
        assert_eq!(req.map, "hot1");
        assert_eq!(req.era.as_deref(), Some("gcw"));
        assert_eq!(req.mode, "ctf");
        assert_eq!(req.playlist.as_ref().unwrap().round, 1);
        assert!(req.playlist.as_ref().unwrap().random);

        let single = BattleRequest::single("nab2", Some("cw")).unwrap();
        assert_eq!(single.family, GameFamily::Swbf1);
        assert_eq!(single.map, "nab2");
        assert_eq!(single.era.as_deref(), Some("cw"));
        assert_eq!(single.mode, "conquest");
        assert!(single.playlist.is_none());

        let wrapped = BattleRequest::from_rotation(
            None,
            None,
            vec![PlaylistEntry::new("kam1", Some("cw"))],
            5,
            false,
        )
        .unwrap();
        assert_eq!(wrapped.map, "kam1");
        assert_eq!(wrapped.playlist.as_ref().unwrap().round, 0);

        // Negative rounds wrap via Euclidean remainder (`round -1` is the last entry).
        let neg = BattleRequest::from_rotation(
            None,
            None,
            parse_rotation(Some("geo1:cw,hot1:gcw")),
            -1,
            false,
        )
        .unwrap();
        assert_eq!(neg.playlist.as_ref().unwrap().round, 1);
        assert_eq!(neg.map, "hot1");
        assert_eq!(neg.next(seeded(1)).map, "geo1");
    }

    #[test]
    fn launch_and_shuffle_match_js_7_times_n_swaps_and_clamping() {
        let entries = parse_rotation(Some("geo1:cw,hot1:gcw,rhn1:cw,tat2:gcw"));
        let ordered = BattleRequest::launch(&entries, false, seeded(1)).unwrap();
        assert_eq!(ordered.map, "geo1");
        assert_eq!(ordered.era.as_deref(), Some("cw"));
        assert_eq!(
            format_rotation(&ordered.playlist.as_ref().unwrap().entries),
            "geo1:cw,hot1:gcw,rhn1:cw,tat2:gcw"
        );

        let launched = BattleRequest::launch(&entries, true, seeded(7)).unwrap();
        let expected_shuffled = shuffle(&entries, seeded(7));
        assert_eq!(
            launched.playlist.as_ref().unwrap().entries,
            expected_shuffled
        );
        assert_eq!(launched.map, expected_shuffled[0].map);
        assert!(launched.playlist.as_ref().unwrap().random);
        assert_eq!(launched.playlist.as_ref().unwrap().round, 0);

        // 7 * 4 swaps, two random draws each, even for n == 1 (7 * 1 * 2 = 14 draws).
        let mut draws = 0;
        shuffle(&entries, || {
            draws += 1;
            0.5
        });
        assert_eq!(draws, 2 * 7 * 4);

        let mut single_draws = 0;
        let single = vec![PlaylistEntry::new("geo1", Some("cw"))];
        shuffle(&single, || {
            single_draws += 1;
            0.5
        });
        assert_eq!(single_draws, 2 * 7);

        // Extreme random value 1.0 clamps to n - 1 rather than wrapping to 0.
        let clamped = shuffle(&entries, || 1.0);
        assert_eq!(clamped, entries);

        assert!(BattleRequest::launch(&[], false, seeded(1)).is_err());
    }

    #[test]
    fn next_advances_round_in_order_and_wraps_forever() {
        let entries = parse_rotation(Some("geo1:cw,hot1:gcw,nab2:cw"));
        let mut req = BattleRequest::launch(&entries, false, seeded(1)).unwrap();
        let mut maps = Vec::new();
        for _ in 0..7 {
            maps.push(req.map.clone());
            req = req.next(seeded(1));
        }
        assert_eq!(
            maps,
            vec!["geo1", "hot1", "nab2", "geo1", "hot1", "nab2", "geo1"]
        );
        assert_eq!(
            format_rotation(&req.playlist.as_ref().unwrap().entries),
            "geo1:cw,hot1:gcw,nab2:cw"
        );
        assert_eq!(req.era.as_deref(), Some("gcw"));
        assert!(!req.playlist.as_ref().unwrap().random);
    }

    #[test]
    fn next_in_random_order_keeps_lap_order_then_reshuffles_on_wrap() {
        let entries = parse_rotation(Some("bes1:cw,end1:gcw,geo1:cw,hot1:gcw,kam1:cw,nab2:cw"));
        let mut rand = seeded(11);
        let mut req = BattleRequest::launch(&entries, true, &mut rand).unwrap();
        let mut laps = Vec::new();

        for _ in 0..4 {
            let lap_rotation = format_rotation(&req.playlist.as_ref().unwrap().entries);
            let mut played = Vec::new();
            for i in 0..entries.len() {
                let pl = req.playlist.as_ref().unwrap();
                assert_eq!(format_rotation(&pl.entries), lap_rotation);
                assert_eq!(pl.round, i);
                played.push(format!("{}:{}", req.map, req.era.as_deref().unwrap_or("")));
                req = req.next(&mut rand);
            }
            assert_eq!(played.join(","), lap_rotation);
            let mut sorted_played = played.clone();
            sorted_played.sort();
            let mut sorted_orig: Vec<String> = format_rotation(&entries)
                .split(',')
                .map(str::to_string)
                .collect();
            sorted_orig.sort();
            assert_eq!(sorted_played, sorted_orig);
            laps.push(lap_rotation);
        }

        // At least two of the 4 laps must have different shuffled orders.
        let unique: std::collections::BTreeSet<_> = laps.iter().collect();
        assert!(unique.len() > 1);
        assert!(req.playlist.as_ref().unwrap().random);
    }

    #[test]
    fn next_without_playlist_restarts_same_battle_and_converts_to_battle_options() {
        let mut single = BattleRequest::single("nab2", Some("cw")).unwrap();
        single.side = Some("rep".to_string());
        assert_eq!(single.next(seeded(1)), single);

        let opts = single.to_battle_options();
        assert_eq!(opts.map, "nab2");
        assert_eq!(opts.era(), "cw");
        assert_eq!(opts.side, "rep");
    }
}
