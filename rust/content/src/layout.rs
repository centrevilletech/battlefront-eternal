//! Per-game-family layout under the prepared content root.
//!
//! Prepared content for Star Wars: Battlefront (2004) (`swbf1`) stays directly
//! at the content root (`<content>/{maps,sides,common,audio,.prepare.json}`) so
//! released builds and newer builds share the same folder with no move.
//!
//! Prepared content for Star Wars: Battlefront II (2005) (`swbf2`) lives inside
//! `<content>/swbf2/.content/` (`maps`, `sides`, `common`, `audio`, `scripts` and
//! the record `.prepare-swbf2.json`), with the marker file `.bfe-family.json`
//! inside that folder. The outer `<content>/swbf2/` container is never marked, so
//! a player's own 2004 data folder named `swbf2` is never refused. Released 2004
//! builds skip dot-prefixed folders in home searches, look for
//! `<dir>/common/common.json` directly, and look for `.prepare.json`, so they do not
//! find a 2005-only folder by themselves.

use bfe_family::GameFamily;
use std::fs;
use std::path::{Path, PathBuf};

/// Positive family marker written for 2005 (`swbf2`) content directories.
pub const SWBF2_MARKER_FILE: &str = ".bfe-family.json";

/// Canonical JSON body written to [`SWBF2_MARKER_FILE`].
pub const SWBF2_MARKER_BODY: &str = "{\n  \"family\": \"swbf2\",\n  \"format\": 1\n}\n";

/// Preparation state receipt filename for 2005 (`swbf2`) content.
pub const SWBF2_PREPARE_STATE_FILE: &str = ".prepare-swbf2.json";

/// Returns the directory holding prepared `maps/`, `sides/`, `common/`,
/// `audio/`, and the preparation receipt for `family` under `root`.
///
/// - `GameFamily::Swbf1` -> `root` (flat layout, unchanged from released builds)
/// - `GameFamily::Swbf2` -> `root/swbf2/.content`
pub fn family_dir(root: &Path, family: GameFamily) -> PathBuf {
    match family {
        GameFamily::Swbf1 => root.to_path_buf(),
        GameFamily::Swbf2 => root.join(family.as_str()).join(".content"),
    }
}

/// Formats [`family_dir`] for the target platform's path conventions (`BFE_CONTENT_DIR`).
///
/// For `GameFamily::Swbf1`, returns `root` unchanged. For `GameFamily::Swbf2` on
/// Windows, preserves root-only (`\`, `/`), drive-relative (`D:`), drive-absolute
/// (`D:\`), and UNC (`\\server\share`) prefixes without collapsing them to a
/// relative path or converting drive-relative paths to drive-absolute.
pub fn family_dir_for_platform(root: &Path, family: GameFamily, is_windows: bool) -> String {
    match family {
        GameFamily::Swbf1 => root.to_string_lossy().into_owned(),
        GameFamily::Swbf2 if !is_windows => {
            family_dir(root, GameFamily::Swbf2).to_string_lossy().into_owned()
        }
        GameFamily::Swbf2 => {
            let raw = root.to_string_lossy();
            if raw.is_empty() {
                return format!("{}\\.content", family.as_str());
            }
            let bytes = raw.as_bytes();
            if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                return format!("{raw}{}\\.content", family.as_str());
            }
            if raw.chars().all(|c| c == '\\' || c == '/') {
                return format!("\\{}\\.content", family.as_str());
            }
            let trimmed = raw.trim_end_matches(['\\', '/']);
            format!("{trimmed}\\{}\\.content", family.as_str())
        }
    }
}

/// Returns the preparation receipt filename for `family`.
///
/// - `GameFamily::Swbf1` -> `.prepare.json` (unchanged from released builds)
/// - `GameFamily::Swbf2` -> `.prepare-swbf2.json`
pub fn prepare_state_file(family: GameFamily) -> &'static str {
    match family {
        GameFamily::Swbf1 => ".prepare.json",
        GameFamily::Swbf2 => SWBF2_PREPARE_STATE_FILE,
    }
}

/// The dedicated record identifies a 2005 tree. A marker by itself identifies the
/// physical interior, even when reached through a link. Old markers on flat content
/// folders leave the released selection intact.
pub fn is_swbf2_marked_dir(dir: &Path) -> bool {
    use crate::prepare::{checkout_content_version, prepared_content_current, read_prepare_state};
    if dir.as_os_str().is_empty() {
        return false;
    }
    let plain = |name: &str| fs::symlink_metadata(dir.join(name)).is_ok_and(|m| m.is_file());
    let dedicated = plain(SWBF2_PREPARE_STATE_FILE);
    if !dedicated && !plain(SWBF2_MARKER_FILE) {
        return false;
    }
    if let Some(state) = read_prepare_state(Some(dir)) {
        if checkout_content_version().is_ok_and(|v| prepared_content_current(Some(&state), &v)) {
            return false;
        }
    }
    dedicated || fs::canonicalize(dir).is_ok_and(|p| p.ends_with(Path::new("swbf2").join(".content")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swbf1_uses_root_and_swbf2_uses_hidden_content_subfolder() {
        let root = Path::new("/data/content");
        assert_eq!(
            family_dir(root, GameFamily::Swbf1),
            PathBuf::from("/data/content")
        );
        assert_eq!(
            family_dir(root, GameFamily::Swbf2),
            PathBuf::from("/data/content/swbf2/.content")
        );
        assert_eq!(prepare_state_file(GameFamily::Swbf1), ".prepare.json");
        assert_eq!(prepare_state_file(GameFamily::Swbf2), ".prepare-swbf2.json");
    }

    #[test]
    fn windows_family_dir_preserves_special_roots() {
        for (input, expected) in [
            ("", r"swbf2\.content"),
            (r"\", r"\swbf2\.content"),
            ("/", r"\swbf2\.content"),
            ("D:", r"D:swbf2\.content"),
            (r"D:\", r"D:\swbf2\.content"),
            (r"\\server\share", r"\\server\share\swbf2\.content"),
            (r"\\server\share\", r"\\server\share\swbf2\.content"),
            (r"C:\data\content", r"C:\data\content\swbf2\.content"),
            (r"C:\data\content\", r"C:\data\content\swbf2\.content"),
            (r"\\?\C:\data\content", r"\\?\C:\data\content\swbf2\.content"),
            (r"\\?\UNC\server\share\data", r"\\?\UNC\server\share\data\swbf2\.content"),
            ("D:games", r"D:games\swbf2\.content"),
            ("C:/data/content/", r"C:/data/content\swbf2\.content"),
            (r"\data", r"\data\swbf2\.content"),
        ] {
            assert_eq!(
                family_dir_for_platform(Path::new(input), GameFamily::Swbf2, true),
                expected,
                "input={input:?}"
            );
            assert_eq!(
                family_dir_for_platform(Path::new(input), GameFamily::Swbf1, true),
                input,
                "swbf1 must remain untouched for input={input:?}"
            );
        }
    }

    #[test]
    fn a_2005_tree_is_found_by_positive_evidence_through_every_spelling() {
        let dir = std::env::temp_dir().join(format!("bfe-layout-marker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let inner = family_dir(&dir, GameFamily::Swbf2);
        fs::create_dir_all(&inner).unwrap();
        assert!(!is_swbf2_marked_dir(&inner));
        fs::write(inner.join(SWBF2_MARKER_FILE), SWBF2_MARKER_BODY).unwrap();
        assert!(is_swbf2_marked_dir(&inner));
        assert!(is_swbf2_marked_dir(&dir.join("swbf2").join(".").join(".content").join("")));
        assert!(is_swbf2_marked_dir(&dir.join("swbf2").join("..").join("swbf2").join(".content")));
        #[cfg(unix)]
        {
            let link = dir.join("elsewhere");
            std::os::unix::fs::symlink(&inner, &link).unwrap();
            assert!(is_swbf2_marked_dir(&link));
        }
        // The container and the content folder above it carry no evidence.
        assert!(!is_swbf2_marked_dir(&dir.join("swbf2")));
        assert!(!is_swbf2_marked_dir(&dir));
        // An empty marker (an interrupted publication) still marks the tree, and so does
        // the dedicated record alone (the tools' own output, before any marker).
        fs::write(inner.join(SWBF2_MARKER_FILE), "").unwrap();
        assert!(is_swbf2_marked_dir(&inner));
        fs::remove_file(inner.join(SWBF2_MARKER_FILE)).unwrap();
        fs::write(inner.join(SWBF2_PREPARE_STATE_FILE), "{}").unwrap();
        assert!(is_swbf2_marked_dir(&inner));
        // A flat 2004 folder with its own record and an obsolete marker stays 2004.
        let flat = dir.join("flat");
        fs::create_dir_all(&flat).unwrap();
        fs::write(flat.join(SWBF2_MARKER_FILE), SWBF2_MARKER_BODY).unwrap();
        fs::write(flat.join(".prepare.json"), "{\"version\":2,\"complete\":true}").unwrap();
        assert!(!is_swbf2_marked_dir(&flat));
        // An extension field cannot turn an obsolete outer marker into an interior.
        fs::write(flat.join(".prepare.json"), "{\"family\":\"swbf2\"}").unwrap();
        assert!(!is_swbf2_marked_dir(&flat));
        let _ = fs::remove_dir_all(&dir);
    }
}
