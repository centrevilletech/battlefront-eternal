//! Release version parsing and ordering (`0.1.0-preview.262`, Debian `0.1.0~preview.262`,
//! and SemVer 2.0 release strings).
//!
//! Mirrors the desktop host (`parseVersion`, `compareVersions`, `isNewer`).
//!
//! ## Ordering Rules
//! - `major`, `minor`, and `patch` are compared numerically first.
//! - A stable release (`0.1.0`) sorts after any prerelease of the same base version
//!   (`0.1.0-preview.262` or `0.1.0~preview.262`).
//! - Dot-separated prerelease identifiers are compared left-to-right: numeric identifiers
//!   compare as integers (`preview.9 < preview.10`), numeric identifiers sort before
//!   alphanumeric identifiers, and alphanumeric identifiers compare ASCII-lexicographically.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// Parsed semantic version with optional prerelease components.
///
/// Both `0.1.0-preview.262` (`package.json`) and `0.1.0~preview.262` (Debian package
/// version) normalize to the same prerelease identifiers so they compare equal,
/// matching `parseVersion` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedVersion {
    /// Original normalized version string (without a leading `v` or `V`).
    pub raw: String,
    /// Major version number.
    pub major: u64,
    /// Minor version number.
    pub minor: u64,
    /// Patch version number.
    pub patch: u64,
    /// Dot-separated prerelease identifiers (empty for stable releases).
    pub prerelease: Vec<PrereleasePart>,
}

/// Single dot-separated prerelease identifier (`numeric` or `string`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrereleasePart {
    /// Numeric identifier (`0..=u64::MAX`), compared numerically.
    Number(u64),
    /// Alphanumeric identifier, compared lexicographically.
    Text(String),
}

/// Parses a version string (`"0.1.0-preview.262"`, `"0.1.0~preview.262"`, `"1.2.3"`)
/// into a [`ParsedVersion`].
///
/// Returns `None` if the version does not conform to `MAJOR.MINOR.PATCH[(-|~)PRERELEASE][+BUILD]`.
/// Mirrors `parseVersion` in the desktop host.
pub fn parse_version(value: &str) -> Option<ParsedVersion> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let clean = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);

    // Strip optional `+build` metadata suffix before splitting `-prerelease` or `~prerelease`.
    let without_build = clean.split('+').next().unwrap_or("");
    if clean.contains('+') {
        let build_part = clean.split_once('+').map(|(_, b)| b).unwrap_or("");
        if build_part.is_empty()
            || !build_part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return None;
        }
    }

    // Support both `-` (`0.1.0-preview.262`) and Debian `~` (`0.1.0~preview.262`).
    let split_pos = without_build.find(['-', '~']);
    let (core, pre_opt) = match split_pos {
        Some(idx) => {
            let c = &without_build[..idx];
            let p = &without_build[idx + 1..];
            if p.is_empty()
                || !p
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
            {
                return None;
            }
            (c, Some(p))
        }
        None => (without_build, None),
    };

    let mut core_parts = core.split('.');
    let major_str = core_parts.next()?;
    let minor_str = core_parts.next()?;
    let patch_str = core_parts.next()?;
    if core_parts.next().is_some() {
        return None;
    }
    if !major_str.chars().all(|c| c.is_ascii_digit())
        || !minor_str.chars().all(|c| c.is_ascii_digit())
        || !patch_str.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let major = major_str.parse::<u64>().ok()?;
    let minor = minor_str.parse::<u64>().ok()?;
    let patch = patch_str.parse::<u64>().ok()?;

    let mut prerelease = Vec::new();
    if let Some(pre) = pre_opt {
        for part in pre.split('.') {
            if part.is_empty() {
                return None;
            }
            if part.chars().all(|c| c.is_ascii_digit()) {
                prerelease.push(PrereleasePart::Number(part.parse::<u64>().ok()?));
            } else {
                prerelease.push(PrereleasePart::Text(part.to_string()));
            }
        }
    }

    Some(ParsedVersion {
        raw: clean.to_string(),
        major,
        minor,
        patch,
        prerelease,
    })
}

/// Compares two version strings, returning `-1`, `0`, or `1` (or `None` if either
/// version string is invalid).
///
/// A release without a prerelease suffix (`prerelease.is_empty()`) sorts after
/// every prerelease/preview of the same `major.minor.patch`.
///
/// Mirrors `compareVersions` in the desktop host.
pub fn compare_versions(a: &str, b: &str) -> Option<i32> {
    let va = parse_version(a)?;
    let vb = parse_version(b)?;

    if va.major != vb.major {
        return Some(if va.major > vb.major { 1 } else { -1 });
    }
    if va.minor != vb.minor {
        return Some(if va.minor > vb.minor { 1 } else { -1 });
    }
    if va.patch != vb.patch {
        return Some(if va.patch > vb.patch { 1 } else { -1 });
    }

    let pre_a = &va.prerelease;
    let pre_b = &vb.prerelease;
    if pre_a.is_empty() && pre_b.is_empty() {
        return Some(0);
    }
    // A release without a preview number sorts after every preview of the same base version.
    if pre_a.is_empty() {
        return Some(1);
    }
    if pre_b.is_empty() {
        return Some(-1);
    }

    let len = pre_a.len().max(pre_b.len());
    for i in 0..len {
        let Some(pa) = pre_a.get(i) else {
            return Some(-1);
        };
        let Some(pb) = pre_b.get(i) else {
            return Some(1);
        };
        if pa == pb {
            continue;
        }
        return Some(match (pa, pb) {
            (PrereleasePart::Number(na), PrereleasePart::Number(nb)) => {
                if na > nb {
                    1
                } else {
                    -1
                }
            }
            (PrereleasePart::Number(_), PrereleasePart::Text(_)) => -1,
            (PrereleasePart::Text(_), PrereleasePart::Number(_)) => 1,
            (PrereleasePart::Text(sa), PrereleasePart::Text(sb)) => match sa.cmp(sb) {
                Ordering::Greater => 1,
                Ordering::Less => -1,
                Ordering::Equal => 0,
            },
        });
    }
    Some(0)
}

/// Checks whether `candidate` is strictly newer than `current` (`compare_versions(candidate, current) == Some(1)`).
///
/// Mirrors `isNewer` in the desktop host.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == Some(1)
}

/// Alias for [`is_newer`].
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    is_newer(candidate, current)
}
