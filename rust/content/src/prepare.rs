//! The shared reader for released preparation records.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path};

/// Compile-time embedded `tools/content-version.json` from the repository checkout.
///
/// Using `include_str!` ensures that if `tools/content-version.json` is moved
/// or removed, compilation fails rather than silently drifting.
pub const CHECKOUT_CONTENT_VERSION_JSON: &str = include_str!("../../../tools/content-version.json");

/// Expected schema versions for prepared content outputs (`tools/content-version.json`).
///
/// Intentional omission: `ContentVersion` does **not** implement `Default`.
/// Callers must load the version table from `tools/content-version.json` via
/// [`read_content_version`] (runtime) or [`checkout_content_version`] (checkout/tests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentVersion {
    /// Overall `.prepare.json` state format version.
    pub state: u32,
    /// Map importer generation version (`mapImportVersion` in `.prepare.json`).
    pub maps: u32,
    /// Side/muzzle importer generation version (`muzzleVersion` in `.prepare.json`).
    pub sides: u32,
    /// Common asset importer generation version (`commonVersion` in `.prepare.json`).
    pub common: u32,
}

/// Parses a `tools/content-version.json` payload into a [`ContentVersion`].
///
/// Rejects missing fields, non-integer values, or malformed JSON with a
/// human-readable error message suitable for surfacing in the loader UI.
pub fn parse_content_version(raw: &str, source_label: &str) -> Result<ContentVersion, String> {
    // Parse strictly via serde_json so any missing or extra-typed required key fails.
    serde_json::from_str::<ContentVersion>(raw).map_err(|err| {
        format!(
            "Failed to parse content version file ({}): {}",
            source_label, err
        )
    })
}

/// Reads and parses `<tools_dir>/content-version.json` at runtime.
///
/// Mirrors how the desktop host loads `../tools/content-version.json` from
/// the installed `tools` directory (`resourcesPath/tools/content-version.json`
/// in packaged builds). Never falls back to literal numbers.
pub fn read_content_version(tools_dir: &Path) -> Result<ContentVersion, String> {
    let path = tools_dir.join("content-version.json");
    let raw = fs::read_to_string(&path).map_err(|err| {
        format!(
            "Missing or unreadable content version file at {}: {}",
            path.display(),
            err
        )
    })?;
    parse_content_version(&raw, &path.display().to_string())
}

/// Parses the repository checkout's `tools/content-version.json` embedded via `include_str!`.
///
/// Used by unit tests and development checkouts when an explicit runtime tools
/// directory is not supplied. Returns an error if the embedded JSON is invalid.
pub fn checkout_content_version() -> Result<ContentVersion, String> {
    parse_content_version(CHECKOUT_CONTENT_VERSION_JSON, "tools/content-version.json")
}

/// Optional extra preparation flags stored in `.prepare.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareStateExtras {
    /// Whether enhanced sound banks were built (`None` or `Some(true)` means enabled).
    #[serde(default)]
    pub enhanced_sound: Option<bool>,
    /// Number of enhanced sound steps executed.
    #[serde(default)]
    pub sound_steps: Option<u32>,
}

/// Failure record for an individual preparation step stored in `.prepare.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareFailureRecord {
    /// Step identifier (e.g. `"addon:bes1g"`).
    pub id: String,
    /// Optional human-readable label for the step.
    #[serde(default)]
    pub label: Option<String>,
    /// Optional error description.
    #[serde(default)]
    pub error: Option<String>,
}

/// Deserialized representation of `<content>/.prepare.json`.
///
/// Mirrors the state object read by `readPrepareState` in the desktop host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareState {
    /// State format version (`ContentVersion::state`).
    pub version: Option<u32>,
    /// Map import version (`ContentVersion::maps`).
    pub map_import_version: Option<u32>,
    /// Side/muzzle version (`ContentVersion::sides`).
    pub muzzle_version: Option<u32>,
    /// Common assets version (`ContentVersion::common`).
    pub common_version: Option<u32>,
    /// Whether preparation finished all required steps.
    pub complete: Option<bool>,
    /// Path to `<game>/GameData/Data/_LVL_PC` used during preparation.
    pub game: Option<String>,
    /// Add-on map IDs included in the preparation run.
    pub add_ons: Option<Vec<String>>,
    /// Step IDs that failed during preparation.
    pub failed: Option<Vec<Value>>,
    /// Step IDs that were skipped during preparation.
    pub skipped: Option<Vec<Value>>,
    /// Detailed failure records.
    pub failures: Option<Vec<PrepareFailureRecord>>,
    /// Optional extra preparation metadata (such as enhanced sound status).
    pub extras: Option<PrepareStateExtras>,
}

/// Reads and parses the 2004 (`swbf1`) `.prepare.json` under `content_dir`,
/// returning `None` if missing or invalid.
///
/// Mirrors `readPrepareState` in the desktop host.
pub fn read_prepare_state(content_dir: Option<&Path>) -> Option<PrepareState> {
    let dir = content_dir?;
    let raw = fs::read_to_string(dir.join(".prepare.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Checks whether a `.prepare.json` state matches the required [`ContentVersion`]
/// and completed with zero failed or skipped steps.
///
/// Mirrors `preparedContentCurrent` in the desktop host.
pub fn prepared_content_current(
    state: Option<&PrepareState>,
    content_version: &ContentVersion,
) -> bool {
    let Some(st) = state else {
        return false;
    };
    let failed_empty = st.failed.as_ref().map(|v| v.is_empty()).unwrap_or(false);
    let skipped_empty = st.skipped.as_ref().map(|v| v.is_empty()).unwrap_or(false);

    st.version == Some(content_version.state)
        && st.map_import_version == Some(content_version.maps)
        && st.muzzle_version == Some(content_version.sides)
        && st.common_version == Some(content_version.common)
        && st.complete == Some(true)
        && failed_empty
        && skipped_empty
}
