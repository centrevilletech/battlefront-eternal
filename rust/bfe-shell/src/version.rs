//! Main menu version line formatting (`src/shell/version.js`).
//!
//! Ports `versionLine(about, simulation)` from `src/shell/version.js`:
//! - Displays the build tag (`PREVIEW <N>` for `X.Y.Z-preview.N` or
//!   `X.Y.Z~preview.N`, else `VERSION <version>` in uppercase, else
//!   `WEB BUILD` when `about` has no `version`).
//! - Displays the simulation contract (`SIMULATION <N>` from the trailing
//!   digits of `simulation`, e.g. `"bfe-sim-28"` -> `"SIMULATION 28"`).
//! - Displays the prepared game-data generations (`GAME DATA M<maps> S<sides> C<common>`),
//!   appending `(NEEDS M<maps> S<sides> C<common>)` when
//!   `prepared.current == Some(false)` and `wanted` is provided, or
//!   `GAME DATA NOT PREPARED` when `about` is `Some` without `prepared`.
//! - Joins the parts with three spaces (`"   "`).
//! - Wanted content generations are parsed directly from the repository's
//!   shared `tools/content-version.json` via `include_str!`.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Default simulation identifier from the shared network contract
/// (`src/net/layout.js` / `rust/bfe-transport/src/layout.rs`).
pub const DEFAULT_SIMULATION: &str = "bfe-sim-28";

const CONTENT_VERSION_JSON: &str = include_str!("../../../tools/content-version.json");

/// Generations of the game data (`maps`, `sides`, `common`, and optional
/// `current` match flag).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Generations {
    pub maps: Option<u32>,
    pub sides: Option<u32>,
    pub common: Option<u32>,
    pub current: Option<bool>,
}

impl Generations {
    /// Parses the wanted generations (`maps`, `sides`, `common`) from the
    /// shared `tools/content-version.json` file.
    pub fn wanted_from_repo() -> Self {
        let v: serde_json::Value =
            serde_json::from_str(CONTENT_VERSION_JSON).unwrap_or(serde_json::Value::Null);
        Self {
            maps: v.get("maps").and_then(|n| n.as_u64()).map(|n| n as u32),
            sides: v.get("sides").and_then(|n| n.as_u64()).map(|n| n as u32),
            common: v.get("common").and_then(|n| n.as_u64()).map(|n| n as u32),
            current: None,
        }
    }

    /// Reads the prepared generations from `<content_dir>/.prepare.json` and
    /// compares them against `wanted`.
    pub fn from_prepare_json(content_dir: &Path, wanted: Option<&Self>) -> Option<Self> {
        let bytes = std::fs::read(content_dir.join(".prepare.json")).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let maps = v
            .get("mapImportVersion")
            .or_else(|| v.get("maps"))
            .and_then(|n| n.as_u64())
            .map(|n| n as u32);
        let sides = v
            .get("version")
            .or_else(|| v.get("sides"))
            .and_then(|n| n.as_u64())
            .map(|n| n as u32);
        let common = v
            .get("commonVersion")
            .or_else(|| v.get("common"))
            .and_then(|n| n.as_u64())
            .map(|n| n as u32);
        let current = wanted.map(|w| maps == w.maps && sides == w.sides && common == w.common);
        Some(Self {
            maps,
            sides,
            common,
            current,
        })
    }
}

/// Build and content-generation metadata passed to [`version_line`].
/// Mirrors the `about` object (`{ version, wanted, prepared }`) in
/// `src/shell/version.js`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AboutInfo {
    pub version: Option<String>,
    pub wanted: Option<Generations>,
    pub prepared: Option<Generations>,
}

impl AboutInfo {
    /// Builds `AboutInfo` for a native `content_dir` and build `version` string,
    /// reading `wanted` from `tools/content-version.json` and `prepared` from
    /// `<content_dir>/.prepare.json`.
    pub fn from_content_dir(content_dir: &Path, version: Option<&str>) -> Self {
        let wanted = Generations::wanted_from_repo();
        let prepared = Generations::from_prepare_json(content_dir, Some(&wanted));
        Self {
            version: version.map(str::to_string),
            wanted: Some(wanted),
            prepared,
        }
    }
}

fn format_generations(g: &Generations) -> String {
    let m = g.maps.map(|n| n.to_string()).unwrap_or_else(|| "?".to_string());
    let s = g.sides.map(|n| n.to_string()).unwrap_or_else(|| "?".to_string());
    let c = g
        .common
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("M{m} S{s} C{c}")
}

fn parse_preview_number(version: &str) -> Option<&str> {
    let (xyz, rest) = version
        .split_once("-preview.")
        .or_else(|| version.split_once("~preview."))?;
    let mut parts = xyz.split('.');
    let a = parts.next()?;
    let b = parts.next()?;
    let c = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if [a, b, c, rest]
        .iter()
        .all(|s| !s.is_empty() && s.bytes().all(|ch| ch.is_ascii_digit()))
    {
        Some(rest)
    } else {
        None
    }
}

fn trailing_digits(s: &str) -> Option<&str> {
    let start = s
        .as_bytes()
        .iter()
        .rposition(|b| !b.is_ascii_digit())
        .map(|i| i + 1)
        .unwrap_or(0);
    let tail = &s[start..];
    if tail.is_empty() {
        None
    } else {
        Some(tail)
    }
}

/// Formats the main menu's bottom-right version line.
///
/// Mirrors `versionLine(about, simulation)` in `src/shell/version.js`.
pub fn version_line(about: Option<&AboutInfo>, simulation: Option<&str>) -> String {
    let mut parts = Vec::with_capacity(3);
    let ver = about.and_then(|a| a.version.as_deref()).unwrap_or("");
    if let Some(preview_num) = parse_preview_number(ver) {
        parts.push(format!("PREVIEW {preview_num}"));
    } else if !ver.is_empty() {
        parts.push(format!("VERSION {}", ver.to_ascii_uppercase()));
    } else {
        parts.push("WEB BUILD".to_string());
    }

    let sim_raw = simulation.unwrap_or(DEFAULT_SIMULATION);
    if let Some(digits) = trailing_digits(sim_raw) {
        parts.push(format!("SIMULATION {digits}"));
    } else {
        let up = if sim_raw.is_empty() {
            "?".to_string()
        } else {
            sim_raw.to_ascii_uppercase()
        };
        parts.push(format!("SIMULATION {up}"));
    }

    if let Some(about) = about {
        if let Some(prepared) = &about.prepared {
            let data = format!("GAME DATA {}", format_generations(prepared));
            if prepared.current == Some(false) {
                if let Some(wanted) = &about.wanted {
                    parts.push(format!("{data} (NEEDS {})", format_generations(wanted)));
                } else {
                    parts.push(data);
                }
            } else {
                parts.push(data);
            }
        } else {
            parts.push("GAME DATA NOT PREPARED".to_string());
        }
    }

    parts.join("   ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_version_line_matching_js_exactly() {
        assert_eq!(version_line(None, None), "WEB BUILD   SIMULATION 28");

        let preview = AboutInfo {
            version: Some("0.1.0-preview.4".to_string()),
            wanted: Some(Generations {
                maps: Some(16),
                sides: Some(3),
                common: Some(2),
                current: None,
            }),
            prepared: Some(Generations {
                maps: Some(16),
                sides: Some(3),
                common: Some(2),
                current: Some(true),
            }),
        };
        assert_eq!(
            version_line(Some(&preview), Some("bfe-sim-28")),
            "PREVIEW 4   SIMULATION 28   GAME DATA M16 S3 C2"
        );

        let outdated = AboutInfo {
            version: Some("0.2.0".to_string()),
            wanted: Some(Generations {
                maps: Some(16),
                sides: Some(3),
                common: Some(2),
                current: None,
            }),
            prepared: Some(Generations {
                maps: Some(15),
                sides: Some(2),
                common: Some(2),
                current: Some(false),
            }),
        };
        assert_eq!(
            version_line(Some(&outdated), Some("bfe-sim-28")),
            "VERSION 0.2.0   SIMULATION 28   GAME DATA M15 S2 C2 (NEEDS M16 S3 C2)"
        );

        let unprepared = AboutInfo {
            version: Some("0.1.0~preview.1".to_string()),
            wanted: None,
            prepared: None,
        };
        assert_eq!(
            version_line(Some(&unprepared), Some("bfe-sim-28")),
            "PREVIEW 1   SIMULATION 28   GAME DATA NOT PREPARED"
        );

        let wanted = Generations::wanted_from_repo();
        assert_eq!(wanted.maps, Some(16));
        assert_eq!(wanted.sides, Some(3));
        assert_eq!(wanted.common, Some(2));
    }
}
