//! Unit and integration tests for `bfe-desktop`.
//!
//! Ports the test suites from:
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - the desktop host
//! - `tests/gpu-info.test.js`
//!
//! ## Test Architecture
//! Each test creates an isolated, uniquely named [`TempDir`] under `std::env::temp_dir()`
//! that is automatically removed on drop, so tests can run concurrently without sharing
//! mutable filesystem state or touching the developer's home directory.
//!
//! ## Coverage Map
//! 1. **Discover (`discover_*`)**:
//!    Prepared content inspection, map-count ranking, depth-bounded home directory search,
//!    case-insensitive SWBF1 vs SWBF2 validation, `AddOn/` campaign scanning, Valve VDF/ACF
//!    parsing, secondary Steam library resolution, and GOG/Wine/Heroic detection.
//! 2. **Prepare & `ContentVersion` (`content_version_*`, `prepare_*`)**:
//!    Runtime and compile-time `tools/content-version.json` parsing (verifying missing or
//!    corrupt files fail hard with no literal fallback), bundled toolchain environment setup,
//!    `.prepare.json` staleness checks, add-on failure summaries, and NDJSON ETA estimation.
//! 3. **Startup Content & CLI Paths (`startup_content_*`, `paths_*`)**:
//!    `Confirm` / `Prepare` / `Menu` mode transitions and `--content` / `BFE_CONTENT_DIR`
//!    candidate ordering.
//! 4. **Linux Desktop, GPU, & Slow-Links (`linux_desktop_*`)**:
//!    `.desktop` generation, legacy AppImage launcher adoption, `gpu_switches` / `gpu_report`,
//!    and `allow_slow_links` (`2500` ms Happy Eyeballs timeout).
//! 5. **Settings Stores (`settings_*`)**:
//!    `PlayerSettingsStore` and `SettingsStore` atomic persistence, schema sanitization, and
//!    patch merging.
//! 6. **Rich Presence & Discord IPC (`presence_*`, `discord_ipc_*`)**:
//!    `normalize_presence_state`, `format_discord_activity`, `format_steam_rich_presence`,
//!    and Discord IPC binary frame encoding/decoding.
//! 7. **Steam VDF & Overlay (`steam_vdf_*`)**:
//!    Binary `shortcuts.vdf` roundtripping, CRC32 AppID / 64-bit GameID computation,
//!    `steam://rungameid/<gameId>` relaunch decisions, and `gameoverlayrenderer.so` preload.
//! 8. **Updater (`update_*`)**:
//!    SemVer/Debian version comparison, canonical JSON, Ed25519 feed verification,
//!    hot-swappable payload slot staging, crash-loop rollback, tamper detection, and
//!    dry-run installer handoff.
//! 9. **Content Archives (`content_archive_*`)**:
//!    `.bfecontent` export, central directory inspection, atomic import, hardlink
//!    deduplication, `relocate_preparation`, `safe_path` traversal rejection, and
//!    bounds-checked rejection of truncated, garbage, and out-of-bounds archives.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

use bfe_desktop::archive::{
    export_content, import_content, import_content_with_progress, inspect_archive,
    read_zip_entries, relocate_preparation, safe_path, RECEIPT_NAME,
};
use bfe_desktop::discover::{
    find_content, find_game_install, find_game_installs, find_gog_install, find_steam_install,
    inspect_content, inspect_install, installed_add_ons, parse_gog_registry,
    parse_heroic_installed, parse_install_dir, parse_library_folders, search_content,
    steam_libraries, FindContentOptions, FindGameOptions, SearchContentOptions, BF1_STEAM_APP,
};
use bfe_desktop::gpu::{gpu_report, gpu_switches, GpuDevice};
use bfe_desktop::launch::{settle_with, SettleOptions};
use bfe_desktop::linux::{
    adopt_installer_launcher, desktop_entry, integrate_linux_desktop, launcher_script,
    updating_launcher, IntegrateLinuxDesktopOptions, LauncherCommandStatus,
};
use bfe_desktop::paths::{
    content_candidates, default_prepared_dir, parse_content_arg, ContentCandidateEnv,
};
use bfe_desktop::prepare::{
    apply_event, checkout_content_version, clear_prepared_for, create_snapshot, estimate,
    parse_content_version, plan_estimate, prepared_content_current, prepared_for, prepared_from,
    prepared_summary, read_content_version, record_timing, resolve_startup_content, tool_env,
    toolchain_paths, ContentVersion, PlanCounts, PrepareFailureRecord, PrepareState,
    PrepareStateExtras, PrepareStep, StartupMode, StepTimings, ToolEnvOptions, ToolchainOptions,
    WantedPrepareOptions,
};
use bfe_desktop::presence::activity::{
    format_discord_activity, format_steam_rich_presence, normalize_presence_state, RawPresenceInput,
};
use bfe_desktop::presence::discord::{
    candidate_ipc_paths, decode_frames, encode_frame, MAX_FRAME_BYTES, OP_FRAME, OP_HANDSHAKE,
};
use bfe_desktop::presence::PresenceController;
use bfe_desktop::settings::{
    DesktopSettings, PlayerSettingsPatch, PlayerSettingsStore, PreparationRecord, SettingsPatch,
    SettingsStore,
};
use bfe_desktop::slow_links::{allow_slow_links, CONNECT_ATTEMPT, SLOW_LINKS_FLAG};
use bfe_desktop::steam::client::{
    decide_steam_relaunch, find_overlay_library, launched_under_steam, prepare_steam_environment,
};
use bfe_desktop::steam::shortcut::{
    ensure_steam_shortcut, inspect_steam_shortcut, upsert_shortcut_in_list, ResolveTargetOptions,
    ShortcutTarget, SHORTCUT_NAME,
};
use bfe_desktop::steam::vdf::{
    compute_shortcut_app_id, compute_shortcut_game_id, parse_shortcuts_vdf,
    serialize_shortcuts_vdf, ShortcutEntry,
};
use bfe_desktop::update::feed::{
    decode_and_validate_manifest, encode_zstd_store_frame, join_url, verify_envelope,
    ChannelPayload, ChannelPlatformEntry, ChannelRelease, ManifestFileEntry, PlatformManifest,
    SignedChannelEnvelope,
};
use bfe_desktop::update::handoff::{check_and_install, CheckOutcome, UpdaterContext};
use bfe_desktop::update::install::InstallProgress;
use bfe_desktop::update::keys::{
    encode_spki_pem, key_id_from_verifying_key, KEYS, PRODUCTION_KEY_ID,
};
use bfe_desktop::update::state::{
    choose_launch, collect, installed_versions, read_state, record_attempt, record_healthy,
    versions_dir, INSTALL_RECORD, MAX_ATTEMPTS,
};
use bfe_desktop::update::tree::{safe_relative_path, sha256_bytes};
use bfe_desktop::update::version::{compare_versions, is_newer_version, parse_version};
use sha2::{Digest, Sha256};

/// Scoped temporary directory that cleans itself up on drop.
///
/// Combines a caller-supplied `prefix`, the current process ID, and 6 bytes of
/// cryptographic randomness so parallel test threads never collide.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a new temporary directory with the given prefix.
    fn new(prefix: &str) -> Self {
        // Sample 6 random bytes via `getrandom` to build a unique directory suffix.
        let mut rand_bytes = [0u8; 6];
        let _ = getrandom::getrandom(&mut rand_bytes);
        let mut hex = String::with_capacity(12);
        for b in rand_bytes {
            use std::fmt::Write;
            let _ = write!(&mut hex, "{:02x}", b);
        }
        let path = std::env::temp_dir().join(format!(
            "bfe-test-{}-{}-{}",
            prefix,
            std::process::id(),
            hex
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    /// Returns the filesystem path of the temporary directory.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    /// Recursively removes the temporary directory when the guard goes out of scope.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Creates a synthetic prepared content fixture matching `tools/content-version.json`.
///
/// Writes `common/common.json`, `sides/all/manifest.json`, `<root>/maps/<id>/scene.json`
/// for each map in `maps`, and a `.prepare.json` state record whose generation numbers
/// match `checkout_content_version()` when `current` is true.
fn make_content(root: &Path, maps: &[&str], current: bool, game: Option<&Path>) {
    // Load the shared generation numbers from `tools/content-version.json` rather than literals.
    let ver = checkout_content_version().expect("checkout content-version.json must parse");
    fs::create_dir_all(root.join("common")).unwrap();
    fs::write(root.join("common/common.json"), "{\"ok\":true}\n").unwrap();
    fs::create_dir_all(root.join("sides/all")).unwrap();
    fs::write(root.join("sides/all/manifest.json"), "{\"units\":[]}\n").unwrap();
    // Create one `<root>/maps/<id>/scene.json` per requested map ID.
    for map in maps {
        let mdir = root.join("maps").join(map);
        fs::create_dir_all(&mdir).unwrap();
        fs::write(mdir.join("scene.json"), format!("{{\"id\":\"{}\"}}\n", map)).unwrap();
        // The browser importer treats `maps/<id>/map.json` as the proof of a playable map.
        fs::write(mdir.join("map.json"), format!("{{\"id\":\"{}\"}}\n", map)).unwrap();
    }
    // Write `.prepare.json` with either the current `ver.state` or an outdated `1`.
    let state = json!({
        "version": if current { ver.state } else { 1 },
        "mapImportVersion": ver.maps,
        "muzzleVersion": ver.sides,
        "commonVersion": ver.common,
        "complete": true,
        "game": game.map(|g| g.join("GameData").join("Data").join("_LVL_PC").to_string_lossy().into_owned()),
        "addOns": [],
        "failed": [],
        "skipped": [],
    });
    fs::write(
        root.join(".prepare.json"),
        format!("{}\n", serde_json::to_string_pretty(&state).unwrap()),
    )
    .unwrap();
}

/// Creates a synthetic Star Wars Battlefront (2004) install directory with `SIDE`
/// and `Common/ENG/common.lvl`.
///
/// When `lowercase` is true, creates `gamedata/data/_lvl_pc/side` and `common/eng/COMMON.LVL`
/// to exercise case-insensitive path traversal on Linux/macOS filesystems.
fn make_install(root: &Path, lowercase: bool) {
    // Support both standard mixed-case (`GameData/Data/_LVL_PC`) and all-lowercase layouts.
    let lvl = if lowercase {
        root.join("gamedata/data/_lvl_pc")
    } else {
        root.join("GameData/Data/_LVL_PC")
    };
    let side = lvl.join(if lowercase { "side" } else { "SIDE" });
    let common = lvl.join(if lowercase {
        "common/eng"
    } else {
        "Common/ENG"
    });
    fs::create_dir_all(&side).unwrap();
    fs::create_dir_all(&common).unwrap();
    fs::write(side.join("all.lvl"), "lvl").unwrap();
    fs::write(
        common.join(if lowercase {
            "COMMON.LVL"
        } else {
            "common.lvl"
        }),
        "lvl",
    )
    .unwrap();
}

/// Creates a synthetic `GameData/AddOn/<id>` directory with `Data/_LVL_PC` and
/// (when `valid` is true) `addme.script`.
fn make_add_on(root: &Path, id: &str, valid: bool) {
    // Every valid SWBF1 add-on has both `Data/_LVL_PC` and `addme.script`.
    let dir = root.join("GameData/AddOn").join(id);
    fs::create_dir_all(dir.join("Data/_LVL_PC")).unwrap();
    if valid {
        fs::write(dir.join("addme.script"), "ucfb").unwrap();
    }
}

// ---------------------------------------------------------------------------
// 1. Discover tests (ports the desktop host)
// ---------------------------------------------------------------------------

/// Verifies `inspect_content` and `find_content` prefer the candidate with the largest map count.
///
/// Mirrors `findContent` ranking behavior in the desktop host:
/// when two `likely` content directories are both valid, the one with more
/// prepared `maps/<id>/scene.json` entries wins.
#[test]
fn discover_validates_prepared_content_and_prefers_richest_map_set() {
    let tmp = TempDir::new("discover-content");
    let partial = tmp.path().join("partial");
    let full = tmp.path().join("full");
    // Build a 1-map directory (`partial`) and a 3-map directory (`full`).
    make_content(&partial, &["bes1a"], true, None);
    make_content(&full, &["bes1a", "bes2a", "tat1i"], true, None);

    // Non-existent directories return None; likely directories pick the richer map set.
    assert_eq!(inspect_content(&tmp.path().join("missing")), None);
    let found = find_content(&FindContentOptions {
        explicit: vec![],
        likely: vec![partial, full.clone()],
        home: None,
        search: false,
    })
    .expect("should find content");
    assert_eq!(found.dir, full);
    assert_eq!(found.source.as_deref(), Some("found"));
    assert_eq!(found.maps, vec!["bes1a", "bes2a", "tat1i"]);
}

/// Verifies `search_content` traverses nested project folders while skipping `SKIP_DIRS` (`node_modules`).
///
/// Mirrors the bounded recursive directory search test in the desktop host.
#[test]
fn discover_searches_home_folder_within_depth_and_skips_heavy_dirs() {
    let tmp = TempDir::new("discover-search");
    let nested = tmp.path().join("Projects").join("bf1").join("content");
    let ignored = tmp.path().join("node_modules").join("bf1").join("content");
    // Create a 2-map folder under `Projects/` and a 4-map folder inside `node_modules/`.
    make_content(&nested, &["bes2a", "kam1c"], true, None);
    make_content(&ignored, &["bes1a", "bes2a", "End1a", "tat1i"], true, None);

    // Even though `ignored` has more maps, `node_modules` is in `SKIP_DIRS` so `nested` wins.
    let found = search_content(tmp.path(), &SearchContentOptions::default())
        .expect("should find nested content");
    assert_eq!(found.dir, nested);
}

/// Verifies `inspect_install` accepts retail SWBF1 regardless of casing and rejects SWBF2.
///
/// Mirrors the SWBF1 vs SWBF2 `Common/<LANG>/common.lvl` check in the desktop host.
#[test]
fn discover_validates_classic_swbf1_installs_case_insensitively() {
    let tmp = TempDir::new("discover-install");
    let win_install = tmp.path().join("swbf1");
    let linux_install = tmp.path().join("swbf1-lower");
    let swbf2_install = tmp.path().join("swbf2");

    // Standard mixed-case Windows layout and all-lowercase Linux/Wine layout both pass.
    make_install(&win_install, false);
    make_install(&linux_install, true);
    // SWBF2 places `common.lvl` directly under `Common/` rather than `Common/<LANG>/common.lvl`.
    fs::create_dir_all(swbf2_install.join("GameData/Data/_LVL_PC/SIDE")).unwrap();
    fs::create_dir_all(swbf2_install.join("GameData/Data/_LVL_PC/Common")).unwrap();
    fs::write(
        swbf2_install.join("GameData/Data/_LVL_PC/Common/common.lvl"),
        "swbf2",
    )
    .unwrap();

    assert_eq!(inspect_install(&win_install).unwrap().dir, win_install);
    assert_eq!(inspect_install(&linux_install).unwrap().dir, linux_install);
    assert_eq!(inspect_install(&swbf2_install), None);
}

/// Verifies `installed_add_ons` returns sorted lowercase IDs for valid `AddOn/<id>` folders.
///
/// Mirrors `installedAddOns` in the desktop host.
#[test]
fn discover_finds_valid_addon_campaigns_sorted_by_id() {
    let tmp = TempDir::new("discover-addons");
    make_install(tmp.path(), false);
    // `TAT3` and `jabba` have `addme.script`; `broken` is missing `addme.script` and must be skipped.
    make_add_on(tmp.path(), "TAT3", true);
    make_add_on(tmp.path(), "jabba", true);
    make_add_on(tmp.path(), "broken", false);

    assert_eq!(installed_add_ons(tmp.path()), vec!["jabba", "tat3"]);
}

/// Verifies `parse_library_folders` and `parse_install_dir` extract paths from Valve VDF/ACF text.
///
/// Mirrors the VDF/ACF parser tests in the desktop host.
#[test]
fn discover_parses_steam_libraryfolders_and_appmanifest() {
    // Double-escaped Windows paths in `libraryfolders.vdf` must unescape to single backslashes.
    let vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"      "C:\\Program Files (x86)\\Steam"
    }
    "1"
    {
        "path"      "D:\\SteamLibrary"
    }
}
"#;
    assert_eq!(
        parse_library_folders(vdf),
        vec![
            "C:\\Program Files (x86)\\Steam".to_string(),
            "D:\\SteamLibrary".to_string()
        ]
    );

    // Valve `appmanifest_1058020.acf` stores the install folder leaf under `"installdir"`.
    let acf = r#"
"AppState"
{
    "appid"     "1058020"
    "name"      "Star Wars Battlefront (Classic, 2004)"
    "installdir"    "Star Wars Battlefront (Classic, 2004)"
}
"#;
    assert_eq!(
        parse_install_dir(acf).as_deref(),
        Some("Star Wars Battlefront (Classic, 2004)")
    );
}

/// Verifies `steam_libraries` and `find_steam_install` follow secondary library roots in `libraryfolders.vdf`.
///
/// Mirrors the multi-library Steam discovery test in the desktop host.
#[test]
fn discover_finds_steam_install_across_secondary_libraries() {
    let tmp = TempDir::new("discover-steam");
    let primary = tmp.path().join("Steam");
    let secondary = tmp.path().join("SteamLibrary");
    let game = secondary
        .join("steamapps/common")
        .join("Star Wars Battlefront (Classic, 2004)");

    fs::create_dir_all(primary.join("steamapps")).unwrap();
    fs::create_dir_all(secondary.join("steamapps")).unwrap();
    make_install(&game, false);

    // Point `primary/steamapps/libraryfolders.vdf` at `secondary`, where `appmanifest_1058020.acf` lives.
    let escaped = secondary.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        primary.join("steamapps/libraryfolders.vdf"),
        format!(
            "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}\n",
            escaped
        ),
    )
    .unwrap();
    fs::write(
        secondary
            .join("steamapps")
            .join(format!("appmanifest_{}.acf", BF1_STEAM_APP)),
        "\"AppState\" { \"installdir\" \"Star Wars Battlefront (Classic, 2004)\" }\n",
    )
    .unwrap();

    // `steam_libraries` must include both `primary` and the referenced `secondary` library.
    let libs = steam_libraries(
        "linux",
        Some(tmp.path()),
        &BTreeMap::new(),
        std::slice::from_ref(&primary),
    );
    assert!(libs.contains(&secondary));

    // `find_steam_install` must resolve the game inside `secondary` with `"steam"` provenance.
    let found = find_steam_install(&FindGameOptions {
        platform: "linux".to_string(),
        home: Some(tmp.path().to_path_buf()),
        env: BTreeMap::new(),
        extra: vec![primary],
    })
    .expect("should find steam install");
    assert_eq!(found.dir, game);
    assert_eq!(found.source, "steam");
}

/// Verifies GOG discovery across Wine `system.reg` files and Heroic Launcher `installed.json`.
///
/// Mirrors the GOG / Wine / Heroic discovery tests in the desktop host.
#[test]
fn discover_finds_gog_installs_from_heroic_wine_and_gog_folders() {
    let tmp = TempDir::new("discover-gog");
    let heroic_game = tmp.path().join("Games/Heroic/Star Wars - Battlefront");
    let wine_prefix = tmp.path().join(".wine");
    let wine_game = wine_prefix.join("drive_c/GOG Games/Star Wars - Battlefront");
    make_install(&heroic_game, false);
    make_install(&wine_game, false);

    // Wine registry format with escaped Windows backslashes (`C:\\GOG Games\\...` -> `<prefix>/drive_c/GOG Games/...`).
    let reg = r#"
[Software\\Wow6432Node\\GOG.com\\Games\\1421404581]
"gameName"="STAR WARS Battlefront"
"path"="C:\\GOG Games\\Star Wars - Battlefront"
"#;
    assert_eq!(
        parse_gog_registry(reg, Some(&wine_prefix)),
        vec![wine_game.clone()]
    );

    // Heroic GOG store `installed.json` format.
    let heroic_json = serde_json::to_string(&json!({
        "installed": [
            { "title": "STAR WARS Battlefront", "install_path": heroic_game }
        ]
    }))
    .unwrap();
    assert_eq!(
        parse_heroic_installed(&heroic_json),
        vec![heroic_game.clone()]
    );

    // Write `~/.config/heroic/gog_store/installed.json` and verify `find_gog_install` discovers it.
    fs::create_dir_all(tmp.path().join(".config/heroic/gog_store")).unwrap();
    fs::write(
        tmp.path().join(".config/heroic/gog_store/installed.json"),
        &heroic_json,
    )
    .unwrap();

    let found = find_gog_install(&FindGameOptions {
        platform: "linux".to_string(),
        home: Some(tmp.path().to_path_buf()),
        env: BTreeMap::new(),
        extra: vec![],
    })
    .expect("should find GOG install");
    assert_eq!(found.dir, heroic_game);
    assert_eq!(found.source, "gog");
}

/// Verifies `find_game_installs` orders candidates and upgrades `"chosen"` entries when also found in Steam/GOG.
///
/// Mirrors `findGameInstalls` provenance ordering and upgrade tests in the desktop host.
#[test]
fn discover_lists_multiple_installs_and_upgrades_chosen_provenance() {
    let tmp = TempDir::new("discover-multi");
    let steam_root = tmp.path().join(".local/share/Steam");
    let steam_game = steam_root.join("steamapps/common/Star Wars Battlefront (Classic, 2004)");
    let gog_game = tmp.path().join("GOG Games/Star Wars - Battlefront");
    let custom_game = tmp.path().join("Custom/SWBF");

    // Set up three distinct valid SWBF1 installs: Steam, GOG folder, and user-chosen custom folder.
    fs::create_dir_all(steam_root.join("steamapps")).unwrap();
    fs::write(
        steam_root
            .join("steamapps")
            .join(format!("appmanifest_{}.acf", BF1_STEAM_APP)),
        "\"AppState\" { \"installdir\" \"Star Wars Battlefront (Classic, 2004)\" }\n",
    )
    .unwrap();
    make_install(&steam_game, false);
    make_install(&gog_game, false);
    make_install(&custom_game, false);

    // User-chosen `extra` directories appear first, followed by Steam, GOG, and known folders.
    let installs = find_game_installs(&FindGameOptions {
        platform: "linux".to_string(),
        home: Some(tmp.path().to_path_buf()),
        env: BTreeMap::new(),
        extra: vec![custom_game.clone()],
    });
    assert_eq!(
        installs
            .iter()
            .map(|i| (&i.dir, i.source.as_str(), i.chosen))
            .collect::<Vec<_>>(),
        vec![
            (&custom_game, "chosen", true),
            (&steam_game, "steam", false),
            (&gog_game, "folder", false),
        ]
    );

    // When `extra` points to the Steam install itself, its source upgrades to `"steam"` while `chosen` stays true.
    let with_chosen_steam = find_game_installs(&FindGameOptions {
        platform: "linux".to_string(),
        home: Some(tmp.path().to_path_buf()),
        env: BTreeMap::new(),
        extra: vec![steam_game.clone()],
    });
    assert_eq!(with_chosen_steam[0].dir, steam_game);
    assert_eq!(with_chosen_steam[0].source, "steam");
    assert!(with_chosen_steam[0].chosen);

    // `find_game_install` returns the first entry from `find_game_installs`.
    let single = find_game_install(&FindGameOptions {
        platform: "linux".to_string(),
        home: Some(tmp.path().to_path_buf()),
        env: BTreeMap::new(),
        extra: vec![],
    })
    .unwrap();
    assert_eq!(single.dir, steam_game);
}

// ---------------------------------------------------------------------------
// 2. Prepare & ContentVersion tests (ports the desktop host)
// ---------------------------------------------------------------------------

/// Verifies `ContentVersion` reads `tools/content-version.json` and errors on missing or corrupt files
/// without falling back to hardcoded literals.
#[test]
fn content_version_loads_from_shared_file_and_errors_when_missing_or_corrupt() {
    // Compile-time embedded `tools/content-version.json` must parse cleanly.
    let checkout_ver = checkout_content_version().expect("embedded tools/content-version.json");
    assert!(checkout_ver.state >= 2);
    assert!(checkout_ver.maps >= 16);

    let tmp = TempDir::new("content-version");
    // Missing tools/content-version.json must return an error, never a fallback.
    let missing_err = read_content_version(tmp.path()).expect_err("missing file must error");
    assert!(missing_err.contains("Missing or unreadable content version file"));

    // Corrupt JSON must return an error, never a fallback.
    fs::write(tmp.path().join("content-version.json"), "{not-valid-json").unwrap();
    let bad_err = read_content_version(tmp.path()).expect_err("corrupt file must error");
    assert!(bad_err.contains("Failed to parse content version file"));

    // Partial JSON missing a required key must return an error.
    let partial_err =
        parse_content_version("{\"state\":2,\"maps\":16}", "partial").expect_err("missing keys");
    assert!(partial_err.contains("Failed to parse content version file"));

    // Valid runtime tools/content-version.json parses identically to checkout.
    let custom = ContentVersion {
        state: 9,
        maps: 99,
        sides: 7,
        common: 5,
    };
    fs::write(
        tmp.path().join("content-version.json"),
        serde_json::to_string(&custom).unwrap(),
    )
    .unwrap();
    assert_eq!(read_content_version(tmp.path()).unwrap(), custom);
}

/// Verifies `toolchain_paths` and `tool_env` configure `PATH`, `KTX2_ENCODER`, and `LD_LIBRARY_PATH`
/// while stripping `PYTHONHOME` and `PYTHONPATH`.
///
/// Mirrors the `toolchainPaths` and `toolEnv` tests in the desktop host.
#[test]
fn prepare_builds_isolated_tool_environment_with_bundled_binaries() {
    let tmp = TempDir::new("tool-env");
    let bin = tmp.path().join("bin");
    let ktx_bin = tmp.path().join("ktx").join("bin");
    let ktx_lib = tmp.path().join("ktx").join("lib");
    let python_bin = tmp.path().join("python").join("bin");
    // Create synthetic bundled `bin`, `ktx/bin/ktx` (plus `ktx.exe`), `ktx/lib`, and `python` paths.
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&ktx_bin).unwrap();
    fs::create_dir_all(&ktx_lib).unwrap();
    fs::create_dir_all(&python_bin).unwrap();
    fs::write(ktx_bin.join("ktx"), "").unwrap();
    fs::write(ktx_bin.join("ktx.exe"), "").unwrap();
    fs::write(python_bin.join("python3"), "").unwrap();
    fs::write(tmp.path().join("python").join("python.exe"), "").unwrap();

    // Packaged builds look for `tools` under `resources_path`.
    let paths = toolchain_paths(&ToolchainOptions {
        packaged: true,
        resources_path: Some(tmp.path().parent().unwrap_or(tmp.path()).to_path_buf()),
        project_root: tmp.path().to_path_buf(),
        platform: if cfg!(windows) {
            "win32".to_string()
        } else {
            "linux".to_string()
        },
        arch: "x64".to_string(),
    });
    assert_eq!(paths.tools.file_name().unwrap().to_str().unwrap(), "tools");

    // Verify Unix (`":"`-joined `PATH`, `ktx`, and `LD_LIBRARY_PATH`) when running on Unix.
    #[cfg(unix)]
    {
        let mut base_env = BTreeMap::new();
        base_env.insert("PATH".to_string(), "/usr/bin".to_string());
        base_env.insert("PYTHONHOME".to_string(), "/bad".to_string());
        base_env.insert("PYTHONPATH".to_string(), "/bad".to_string());

        let env_out = tool_env(&ToolEnvOptions {
            toolchain: tmp.path().to_path_buf(),
            game: Some(PathBuf::from("/games/swbf1")),
            content: Some(PathBuf::from("/data/content")),
            env: base_env,
            platform: "linux".to_string(),
        });
        assert!(env_out["PATH"].starts_with(&format!("{}:{}", bin.display(), ktx_bin.display())));
        assert_eq!(env_out["BFE_GAME_DIR"], "/games/swbf1");
        assert_eq!(env_out["BFE_CONTENT_DIR"], "/data/content");
        assert_eq!(
            env_out["KTX2_ENCODER"],
            ktx_bin.join("ktx").to_string_lossy().as_ref()
        );
        assert_eq!(
            env_out["LD_LIBRARY_PATH"],
            ktx_lib.to_string_lossy().as_ref()
        );
        assert!(!env_out.contains_key("PYTHONHOME"));
        assert!(!env_out.contains_key("PYTHONPATH"));
    }

    // Verify Windows (`";"`-joined `PATH` and `ktx.exe`) across platforms.
    {
        let mut win_base_env = BTreeMap::new();
        win_base_env.insert("PATH".to_string(), r"C:\Windows\System32".to_string());
        win_base_env.insert("PYTHONHOME".to_string(), r"C:\bad".to_string());
        win_base_env.insert("PYTHONPATH".to_string(), r"C:\bad".to_string());

        let win_out = tool_env(&ToolEnvOptions {
            toolchain: tmp.path().to_path_buf(),
            game: Some(PathBuf::from(r"C:\games\swbf1")),
            content: Some(PathBuf::from(r"C:\data\content")),
            env: win_base_env,
            platform: "win32".to_string(),
        });
        assert!(win_out["PATH"].starts_with(&format!("{};{}", bin.display(), ktx_bin.display())));
        assert_eq!(win_out["BFE_GAME_DIR"], r"C:\games\swbf1");
        assert_eq!(win_out["BFE_CONTENT_DIR"], r"C:\data\content");
        assert_eq!(
            win_out["KTX2_ENCODER"],
            ktx_bin.join("ktx.exe").to_string_lossy().as_ref()
        );
        assert!(!win_out.contains_key("PYTHONHOME"));
        assert!(!win_out.contains_key("PYTHONPATH"));
    }
}

/// Verifies `prepared_content_current`, `prepared_from`, and `prepared_for` against `ContentVersion`.
///
/// Mirrors the `.prepare.json` version and provenance tests in the desktop host.
#[test]
fn prepare_checks_whether_prepared_content_matches_current_importer_versions() {
    let ver = checkout_content_version().unwrap();
    let valid_state = PrepareState {
        version: Some(ver.state),
        map_import_version: Some(ver.maps),
        muzzle_version: Some(ver.sides),
        common_version: Some(ver.common),
        complete: Some(true),
        game: Some("/games/swbf1/GameData/Data/_LVL_PC".to_string()),
        add_ons: Some(vec!["bes1g".to_string(), "tat3".to_string()]),
        failed: Some(vec![]),
        skipped: Some(vec![]),
        failures: None,
        extras: None,
    };

    assert!(prepared_content_current(Some(&valid_state), &ver));

    // A stale `map_import_version` invalidates the prepared state.
    let mut old_maps = valid_state.clone();
    old_maps.map_import_version = Some(ver.maps - 1);
    assert!(!prepared_content_current(Some(&old_maps), &ver));

    // Any recorded step failure invalidates `prepared_content_current`.
    let mut had_failure = valid_state.clone();
    had_failure.failed = Some(vec![json!("map:bes1a")]);
    assert!(!prepared_content_current(Some(&had_failure), &ver));

    assert!(!prepared_content_current(None, &ver));

    // `prepared_from` matches `<game>/GameData/Data/_LVL_PC` against the install root.
    assert!(prepared_from(
        Some(&valid_state),
        Some(Path::new("/games/swbf1")),
        "linux"
    ));
    assert!(!prepared_from(
        Some(&valid_state),
        Some(Path::new("/games/other")),
        "linux"
    ));

    // Add-on lists are compared after sorting and deduplicating.
    assert!(prepared_for(
        Some(&valid_state),
        Some(Path::new("/games/swbf1")),
        &["tat3".to_string(), "bes1g".to_string()],
        &WantedPrepareOptions::default(),
        "linux",
        &ver
    ));
    assert!(!prepared_for(
        Some(&valid_state),
        Some(Path::new("/games/swbf1")),
        &["tat3".to_string()],
        &WantedPrepareOptions::default(),
        "linux",
        &ver
    ));

    // If `enhanced_sound: false` was used during preparation, requesting `enhanced_sound: true`
    // requires re-running preparation to extract higher-quality sound banks.
    let mut no_sound = valid_state.clone();
    no_sound.extras = Some(PrepareStateExtras {
        enhanced_sound: Some(false),
        sound_steps: Some(0),
    });
    assert!(!prepared_for(
        Some(&no_sound),
        Some(Path::new("/games/swbf1")),
        &["bes1g".to_string(), "tat3".to_string()],
        &WantedPrepareOptions {
            enhanced_sound: Some(true)
        },
        "linux",
        &ver
    ));
    assert!(prepared_for(
        Some(&no_sound),
        Some(Path::new("/games/swbf1")),
        &["bes1g".to_string(), "tat3".to_string()],
        &WantedPrepareOptions {
            enhanced_sound: Some(false)
        },
        "linux",
        &ver
    ));
}

/// Verifies `prepared_summary` filters `addon:*` failures and `clear_prepared_for` removes only
/// prepared output directories when switching game installations.
///
/// Mirrors `preparedSummary` and `clearPreparedFor` in the desktop host.
#[test]
fn prepare_summarizes_addon_failures_and_clears_only_prepared_outputs() {
    // Only failures with `id` starting with `"addon:"` are included in `add_on_failures`.
    let st = PrepareState {
        failures: Some(vec![
            PrepareFailureRecord {
                id: "addon:broken".to_string(),
                label: Some("Add-on map: broken".to_string()),
                error: Some("bad lvl".to_string()),
            },
            PrepareFailureRecord {
                id: "movies".to_string(),
                label: Some("Shell movies".to_string()),
                error: Some("missing".to_string()),
            },
        ]),
        extras: Some(PrepareStateExtras {
            enhanced_sound: Some(false),
            sound_steps: Some(0),
        }),
        ..Default::default()
    };
    let summary = prepared_summary(Some(&st));
    assert_eq!(summary.add_on_failures.len(), 1);
    assert_eq!(summary.add_on_failures[0].id, "addon:broken");
    assert!(!summary.enhanced_sound);
    assert_eq!(summary.sound_steps, 0);

    let tmp = TempDir::new("clear-prepared");
    let ver = checkout_content_version().unwrap();
    fs::create_dir_all(tmp.path().join("maps/tat3")).unwrap();
    fs::create_dir_all(tmp.path().join("common")).unwrap();
    fs::write(tmp.path().join("keep.txt"), "keep").unwrap();
    let prep_json = json!({
        "version": ver.state,
        "mapImportVersion": ver.maps,
        "muzzleVersion": ver.sides,
        "commonVersion": ver.common,
        "complete": true,
        "game": "/games/first/GameData/Data/_LVL_PC",
        "addOns": ["tat3"],
        "failed": [],
        "skipped": [],
    });
    fs::write(tmp.path().join(".prepare.json"), prep_json.to_string()).unwrap();

    // Same game directory -> nothing cleared.
    assert!(!clear_prepared_for(
        Some(tmp.path()),
        Some(Path::new("/games/first")),
        "linux"
    ));
    assert!(tmp.path().join("maps/tat3").exists());

    // Different game directory -> `maps`, `common`, and `.prepare.json` removed, `keep.txt` preserved.
    assert!(clear_prepared_for(
        Some(tmp.path()),
        Some(Path::new("/games/second")),
        "linux"
    ));
    assert!(!tmp.path().join("maps").exists());
    assert!(!tmp.path().join("common").exists());
    assert!(!tmp.path().join(".prepare.json").exists());
    assert!(tmp.path().join("keep.txt").exists());
}

/// Verifies `apply_event`, `record_timing`, `estimate`, and `plan_estimate` across NDJSON progress events.
///
/// Mirrors the NDJSON progress state machine and ETA estimator tests in the desktop host.
#[test]
fn prepare_tracks_progress_events_and_estimates_remaining_time() {
    let mut snap = create_snapshot();
    // Apply an initial `plan` event with 3 runnable steps and 1 `unavailable` step.
    apply_event(
        &mut snap,
        &json!({
            "event": "plan",
            "game": "/games/swbf1",
            "content": "/data/content",
            "workers": 2,
            "steps": [
                { "id": "common", "kind": "common", "label": "Common assets", "status": "pending" },
                { "id": "map:bes2a", "kind": "map", "label": "Map: bes2a", "status": "pending" },
                { "id": "map:tat1i", "kind": "map", "label": "Map: tat1i", "status": "pending" },
                { "id": "movies", "kind": "movies", "label": "Shell movies", "status": "unavailable" }
            ]
        }),
        10_000,
    );
    assert_eq!(snap.total, 3);
    assert_eq!(snap.done, 0);

    // Complete `common` and advance `map:bes2a` to 50% progress.
    apply_event(
        &mut snap,
        &json!({ "event": "step", "id": "common", "status": "done", "seconds": 12.0, "cached": false }),
        10_000,
    );
    apply_event(
        &mut snap,
        &json!({ "event": "step", "id": "map:bes2a", "status": "running", "progress": 0.5 }),
        10_000,
    );
    assert_eq!(snap.done, 1);
    assert!((snap.percent - 1.5 / 3.0).abs() < 1e-6);

    // Record a 20-second map timing sample and verify the remaining ETA calculation.
    let mut timings = StepTimings::new();
    timings = record_timing(
        &timings,
        &PrepareStep {
            id: "m".to_string(),
            kind: "map".to_string(),
            label: String::new(),
            status: "done".to_string(),
            detail: None,
            cached: false,
            seconds: Some(20.0),
            started_at: None,
            progress: Some(1.0),
        },
        20.0,
    );
    let est = estimate(&snap, Some(&timings), 15_000);
    assert_eq!(est.phase_label, "Preparing Map: bes2a");
    assert_eq!(est.eta_seconds, Some(15));

    // Mark `bes2a` playable mid-run, then finish the session.
    apply_event(
        &mut snap,
        &json!({ "event": "playable", "maps": ["bes2a"] }),
        15_000,
    );
    assert!(snap.playable);
    assert_eq!(snap.playable_maps, vec!["bes2a"]);

    apply_event(
        &mut snap,
        &json!({ "event": "finished", "complete": true, "playable": true }),
        20_000,
    );
    assert_eq!(snap.status, "done");
    let done_est = estimate(&snap, Some(&timings), 20_000);
    assert_eq!(done_est.eta_seconds, Some(0));
    assert_eq!(done_est.phase_label, "Ready to play");

    // Verify `plan_estimate` includes or omits the sound bank conversion cost based on `enhanced_sound`.
    let plan_with_sound = plan_estimate(
        &PlanCounts {
            sides: 7,
            common: 1,
            shell: 1,
            maps: 4,
            audio: 2,
            sound: 10,
            textures: 1,
            movies: 1,
        },
        2,
        Some(&timings),
        true,
    );
    let plan_no_sound = plan_estimate(
        &PlanCounts {
            sides: 7,
            common: 1,
            shell: 1,
            maps: 4,
            audio: 2,
            sound: 10,
            textures: 1,
            movies: 1,
        },
        2,
        Some(&timings),
        false,
    );
    assert!(plan_with_sound.sound > 0);
    assert_eq!(plan_no_sound.sound, 0);
    assert_eq!(
        plan_with_sound.total - plan_no_sound.total,
        plan_with_sound.sound
    );
}

// ---------------------------------------------------------------------------
// 3. Startup content tests (ports tests/desktop-startup-content.test.js)
// ---------------------------------------------------------------------------

/// Verifies `resolve_startup_content` transitions between `Confirm`, `Prepare`, and `Menu` modes.
///
/// Mirrors `resolveStartupContent` in `tests/desktop-startup-content.test.js`.
#[test]
fn startup_content_resolves_menu_confirm_prepare_and_outdated_modes() {
    let tmp = TempDir::new("startup-content");
    let home = tmp.path().join("home");
    let content_dir = tmp.path().join("content");
    let steam_game = home
        .join(".local/share/Steam")
        .join("steamapps")
        .join("common")
        .join("Star Wars Battlefront (Classic, 2004)");
    fs::create_dir_all(home.join(".local/share/Steam/steamapps")).unwrap();
    fs::write(
        home.join(".local/share/Steam/steamapps")
            .join(format!("appmanifest_{}.acf", BF1_STEAM_APP)),
        "\"AppState\" { \"installdir\" \"Star Wars Battlefront (Classic, 2004)\" }\n",
    )
    .unwrap();
    make_install(&steam_game, false);

    let settings = SettingsStore::new(tmp.path().join("settings.json"));

    // Outdated content with a valid Steam install -> Confirm.
    make_content(&content_dir, &["bes2a"], false, None);
    let res = resolve_startup_content(
        &settings,
        std::slice::from_ref(&content_dir),
        Some(&home),
        "linux",
        &BTreeMap::new(),
        None,
    )
    .unwrap();
    assert_eq!(res.mode, StartupMode::Confirm);
    assert_eq!(res.content, None);
    assert_eq!(res.selected.as_deref(), Some(steam_game.as_path()));

    // Previously prepared game install recorded in settings -> Prepare.
    let mut recs = BTreeMap::new();
    recs.insert(
        content_dir.to_string_lossy().into_owned(),
        PreparationRecord {
            game_dir: steam_game.to_string_lossy().into_owned(),
            add_ons: vec![],
            updated_at: "2026-01-01".to_string(),
        },
    );
    settings
        .save(&SettingsPatch {
            game_dir: Some(Some(steam_game.to_string_lossy().into_owned())),
            preparations: Some(recs),
            ..Default::default()
        })
        .unwrap();
    let res_prep = resolve_startup_content(
        &settings,
        std::slice::from_ref(&content_dir),
        Some(&home),
        "linux",
        &BTreeMap::new(),
        None,
    )
    .unwrap();
    assert_eq!(res_prep.mode, StartupMode::Prepare);

    // Current prepared content -> Menu.
    make_content(&content_dir, &["bes2a"], true, Some(&steam_game));
    let res_menu = resolve_startup_content(
        &settings,
        std::slice::from_ref(&content_dir),
        Some(&home),
        "linux",
        &BTreeMap::new(),
        None,
    )
    .unwrap();
    assert_eq!(res_menu.mode, StartupMode::Menu);
    assert!(res_menu.content.is_some());
}

// ---------------------------------------------------------------------------
// 4. Content candidates & CLI args (ports tests/desktop-content.test.js)
// ---------------------------------------------------------------------------

/// Verifies `parse_content_arg`, `content_candidates`, and `default_prepared_dir` priority order.
///
/// Mirrors `tests/desktop-content.test.js`.
#[test]
fn paths_parses_content_arg_and_orders_candidates() {
    // On Unix hosts, verify POSIX absolute paths and forward-slash joining.
    #[cfg(unix)]
    {
        // Both `--content <dir>` and `--content=<dir>` must resolve relative to `cwd`.
        let cwd = Path::new("/work/app");
        assert_eq!(
            parse_content_arg(&["--content".to_string(), "./data".to_string()], cwd),
            Some(PathBuf::from("/work/app/data"))
        );
        assert_eq!(
            parse_content_arg(&["--content=/srv/bfe".to_string()], cwd),
            Some(PathBuf::from("/srv/bfe"))
        );

        // Priority order: CLI `--content` -> `BFE_CONTENT_DIR` -> `<userData>/content`.
        let mut env = BTreeMap::new();
        env.insert("BFE_CONTENT_DIR".to_string(), "/env/content".to_string());
        let list = content_candidates(&ContentCandidateEnv {
            argv: vec!["--content=/cli/content".to_string()],
            env,
            cwd: PathBuf::from("/work/repo/bin"),
            project_root: PathBuf::from("/work/repo"),
            user_data: Some(PathBuf::from(
                "/opt/example/player/.config/battlefront-eternal",
            )),
            exec_dir: Some(PathBuf::from("/opt/BattlefrontEternal")),
            resources_path: Some(PathBuf::from("/opt/BattlefrontEternal/resources")),
            packaged: true,
        });
        assert_eq!(list[0], PathBuf::from("/cli/content"));
        assert_eq!(list[1], PathBuf::from("/env/content"));
        assert_eq!(
            list[2],
            PathBuf::from("/opt/example/player/.config/battlefront-eternal/content")
        );
        assert_eq!(
            default_prepared_dir(
                Some(Path::new("/opt/example/player/.config/battlefront-eternal")),
                Path::new("/work/repo"),
                true
            ),
            PathBuf::from("/opt/example/player/.config/battlefront-eternal/content")
        );
    }

    // On Windows hosts, verify drive-prefixed paths and backslash joining.
    #[cfg(windows)]
    {
        let cwd = Path::new(r"C:\work\app");
        assert_eq!(
            parse_content_arg(&["--content".to_string(), r".\data".to_string()], cwd),
            Some(PathBuf::from(r"C:\work\app\data"))
        );
        assert_eq!(
            parse_content_arg(&[r"--content=C:\srv\bfe".to_string()], cwd),
            Some(PathBuf::from(r"C:\srv\bfe"))
        );

        let mut env = BTreeMap::new();
        env.insert("BFE_CONTENT_DIR".to_string(), r"C:\env\content".to_string());
        let list = content_candidates(&ContentCandidateEnv {
            argv: vec![r"--content=C:\cli\content".to_string()],
            env,
            cwd: PathBuf::from(r"C:\work\repo\bin"),
            project_root: PathBuf::from(r"C:\work\repo"),
            user_data: Some(PathBuf::from(
                r"C:\opt\example\player\AppData\Roaming\Battlefront Eternal",
            )),
            exec_dir: Some(PathBuf::from(r"C:\opt\BattlefrontEternal")),
            resources_path: Some(PathBuf::from(r"C:\opt\BattlefrontEternal\resources")),
            packaged: true,
        });
        assert_eq!(list[0], PathBuf::from(r"C:\cli\content"));
        assert_eq!(list[1], PathBuf::from(r"C:\env\content"));
        assert_eq!(
            list[2],
            PathBuf::from(r"C:\opt\example\player\AppData\Roaming\Battlefront Eternal\content")
        );
        assert_eq!(
            default_prepared_dir(
                Some(Path::new(
                    r"C:\opt\example\player\AppData\Roaming\Battlefront Eternal"
                )),
                Path::new(r"C:\work\repo"),
                true
            ),
            PathBuf::from(r"C:\opt\example\player\AppData\Roaming\Battlefront Eternal\content")
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Linux desktop integration, GPU & Slow-links tests
// ---------------------------------------------------------------------------

/// Verifies `desktop_entry`, `launcher_script`, `adopt_installer_launcher`, `integrate_linux_desktop`,
/// `gpu_switches`, `gpu_report`, and `allow_slow_links`.
///
/// Mirrors the desktop host, `tests/gpu-info.test.js`, and the desktop host.
#[test]
fn linux_desktop_entry_adopts_installer_launcher_and_gpu_and_slow_links_match_js() {
    // Verify desktopEntry and launcherScript output format from the desktop host
    let entry = desktop_entry("/opt/example/Apps/Battlefront Eternal.AppImage");
    assert!(entry.contains("[Desktop Entry]"));
    assert!(entry.contains("Exec=\"/opt/example/Apps/Battlefront Eternal.AppImage\" %U"));
    assert!(launcher_script("/opt/x.AppImage").contains("current/battlefront-eternal"));

    // Verify adoptInstallerLauncher upgrades legacy AppImage launchers in ~/.local/bin/battlefront-eternal.
    let tmp = TempDir::new("linux-desktop");
    let home = tmp.path();
    let bin_file = home.join(".local/bin/battlefront-eternal");
    assert_eq!(adopt_installer_launcher(home).unwrap(), "missing");
    fs::create_dir_all(bin_file.parent().unwrap()).unwrap();
    let original = "#!/usr/bin/env bash\nexport BFE_NO_DESKTOP_INTEGRATION=1\nexec /opt/example/p/.local/share/battlefront-eternal/Battlefront-Eternal.AppImage --appimage-extract-and-run \"$@\"\n";
    fs::write(&bin_file, original).unwrap();
    assert_eq!(adopt_installer_launcher(home).unwrap(), "updated");
    let adopted = fs::read_to_string(&bin_file).unwrap();
    assert_eq!(
        adopted,
        updating_launcher(
            "/opt/example/p/.local/share/battlefront-eternal/Battlefront-Eternal.AppImage"
        )
    );
    assert_eq!(adopt_installer_launcher(home).unwrap(), "current");
    fs::write(&bin_file, "#!/bin/sh\nexec something-else\n").unwrap();
    assert_eq!(adopt_installer_launcher(home).unwrap(), "foreign");

    // Verify integrateLinuxDesktop writes icon, .desktop entry, and ~/.local/bin/battlefront.
    let app_image = tmp.path().join("Battlefront Eternal.AppImage");
    let icon_source = tmp.path().join("icon.png");
    let data_home = tmp.path().join("data");
    fs::write(&app_image, "appimage").unwrap();
    fs::write(&icon_source, "png").unwrap();

    let mut env = BTreeMap::new();
    env.insert(
        "XDG_DATA_HOME".to_string(),
        data_home.to_string_lossy().into_owned(),
    );

    let res = integrate_linux_desktop(&IntegrateLinuxDesktopOptions {
        executable: Some(app_image),
        icon: icon_source,
        home: home.to_path_buf(),
        env,
        update_caches: false,
    })
    .unwrap();
    assert!(!res.skipped);
    assert!(res.icon);
    assert!(res.entry);
    assert_eq!(res.command, Some(LauncherCommandStatus::Updated));

    // Verify gpuSwitches and gpuReport match the desktop host and tests/gpu-info.test.js.
    assert_eq!(
        gpu_switches("win32"),
        vec![vec!["force_high_performance_gpu"]]
    );
    assert!(gpu_switches("linux").is_empty());
    let raw_status = BTreeMap::from([
        ("webgl2".to_string(), "enabled_readback".to_string()),
        (
            "gpu_compositing".to_string(),
            "disabled_software".to_string(),
        ),
        ("video_decode".to_string(), "enabled".to_string()),
        ("2d_canvas".to_string(), "enabled".to_string()),
    ]);
    let raw_devices = vec![
        GpuDevice {
            vendor_id: 32902,
            device_id: 294,
            active: true,
            driver_version: Some("31.0.14045.5222".to_string()),
        },
        GpuDevice {
            vendor_id: 4318,
            device_id: 4576,
            active: false,
            driver_version: None,
        },
    ];
    let report = gpu_report(&raw_status, &raw_devices, "win32");
    assert_eq!(report.devices.len(), 2);
    assert_eq!(report.devices[0].vendor_id, 32902);
    assert_eq!(
        report.devices[0].driver_version.as_deref(),
        Some("31.0.14045.5222")
    );
    assert_eq!(report.devices[1].driver_version, None);
    assert_eq!(report.switches, vec!["force_high_performance_gpu"]);

    // Verify allowSlowLinks, CONNECT_ATTEMPT, and SLOW_LINKS_FLAG match the desktop host
    assert_eq!(CONNECT_ATTEMPT, 2500);
    assert_eq!(
        SLOW_LINKS_FLAG,
        "--network-family-autoselection-attempt-timeout=2500"
    );
    assert_eq!(allow_slow_links().as_millis(), 2500);
}

// ---------------------------------------------------------------------------
// 6. Player settings & Desktop settings tests (ports the desktop host)
// ---------------------------------------------------------------------------

/// Verifies `PlayerSettingsStore` and `SettingsStore` persist, merge, and sanitize settings.
///
/// Mirrors the desktop host and the desktop host.
#[test]
fn settings_stores_persist_sanitize_and_merge_patches() {
    let sound_profile = TempDir::new("sound-choice");
    let desktop = SettingsStore::new(sound_profile.path().join("desktop-settings.json"));
    desktop
        .save(&SettingsPatch {
            enhanced_sound: Some(false),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(desktop.load().enhanced_sound, Some(false));
    desktop
        .save(&SettingsPatch {
            auto_check_updates: Some(false),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(desktop.load().enhanced_sound, Some(false));

    let tmp = TempDir::new("settings");
    let file = tmp.path().join("nested/player-settings.json");
    let store = PlayerSettingsStore::new(&file);

    // Loading a non-existent file returns a clean schema v1 default document.
    assert_eq!(store.load().version, 1);

    // Save a partial patch with FOV, volume, keybindings, audio device, and server lists.
    let updated = store
        .save(&PlayerSettingsPatch {
            settings: Some(BTreeMap::from([
                ("fov".to_string(), json!(95)),
                ("masterVolume".to_string(), json!(0.6)),
            ])),
            keybindings: Some(BTreeMap::from([(
                "jump".to_string(),
                vec!["Space".to_string()],
            )])),
            audio_output_device_id: Some("usb-headset-1".to_string()),
            has_launched_before: Some(true),
            favorite_servers: Some(vec!["wss://us-east.example.com".to_string()]),
            recent_servers: Some(vec![json!({
                "address": "127.0.0.1:28910",
                "name": "Local Server",
                "lastJoined": 1700000000000u64
            })]),
            updated_at: Some(123456),
        })
        .unwrap();

    assert_eq!(updated.settings["fov"], json!(95));
    assert_eq!(updated.audio_output_device_id, "usb-headset-1");
    assert!(updated.has_launched_before);

    // Reloading from disk must round-trip the saved player settings document.
    let reloaded = PlayerSettingsStore::new(&file).load();
    assert_eq!(reloaded, updated);

    // Verify `SettingsStore` defaults (`updateChannel: "stable"`, `autoCheckUpdates: true`).
    let desktop_file = tmp.path().join("settings.json");
    let dstore = SettingsStore::new(&desktop_file);
    let dloaded: DesktopSettings = dstore.load();
    assert_eq!(dloaded.update_channel, "stable");
    assert!(dloaded.auto_check_updates);
    assert!(dloaded.discord_presence);
}

// ---------------------------------------------------------------------------
// 7. Rich Presence & Discord IPC tests (ports the desktop host)
// ---------------------------------------------------------------------------

/// Verifies `normalize_presence_state`, `format_discord_activity`, and `format_steam_rich_presence`.
///
/// Mirrors the desktop host.
#[test]
fn presence_normalizes_and_formats_discord_and_steam_activities() {
    // Normalize a multiplayer match on `bes2a` (`Bespin: Cloud City`) as `all` (`Rebel Alliance`).
    let state = normalize_presence_state(&RawPresenceInput {
        phase: Some("match".to_string()),
        mode: Some("multiplayer".to_string()),
        map_id: Some("bes2a".to_string()),
        faction: Some("all".to_string()),
        unit_name: Some("Rebel Marksman".to_string()),
        room_name: Some("Cloud City 24/7".to_string()),
        party_id: Some("room-bes2a".to_string()),
        party_size: Some(6),
        party_max: Some(16),
        started_at: Some(1_710_000_000_000),
        ..Default::default()
    });
    assert_eq!(state.map_name.as_deref(), Some("Bespin: Cloud City"));
    assert_eq!(state.faction_name.as_deref(), Some("Rebel Alliance"));
    assert_eq!(state.era.as_deref(), Some("gcw"));
    assert_eq!(state.started_at, Some(1_710_000_000));

    // Verify Discord Rich Presence activity fields (`details`, `state`, and `party`).
    let activity = format_discord_activity(&state);
    assert_eq!(activity["details"], "Multiplayer - Bespin: Cloud City");
    assert_eq!(activity["state"], "Rebel Alliance - Rebel Marksman");
    assert_eq!(activity["party"]["id"], "room-bes2a");
    assert_eq!(activity["party"]["size"], json!([6, 16]));

    // Verify Steam Friends rich presence key/value pairs (`status`, `steam_player_group`, `steam_player_group_size`).
    let steam = format_steam_rich_presence(&state);
    assert_eq!(
        steam["status"],
        "Multiplayer - Bespin: Cloud City: Rebel Alliance - Rebel Marksman"
    );
    assert_eq!(steam["steam_player_group"], "room-bes2a");
    assert_eq!(steam["steam_player_group_size"], "6");

    // Verify `PresenceController` updates its cached activity state on `set_activity`.
    let mut ctrl = PresenceController::default();
    ctrl.set_activity(&RawPresenceInput {
        phase: Some("menu".to_string()),
        ..Default::default()
    });
    assert_eq!(
        ctrl.status().discord_activity.unwrap()["details"],
        "In Main Menu"
    );
}

/// Verifies Discord IPC binary framing (`encode_frame`, `decode_frames`, and `candidate_ipc_paths`).
///
/// Mirrors the Discord IPC packet framing and socket discovery tests in the desktop host.
#[test]
fn discord_ipc_frame_encoding_and_decoding_handles_partial_and_oversized_frames() {
    // Encode a handshake frame (`OP_HANDSHAKE`) followed by an activity frame (`OP_FRAME`).
    let f1 = encode_frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "123" }));
    let f2 = encode_frame(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "nonce": "abc" }));
    let mut combined = Vec::new();
    combined.extend_from_slice(&f1);
    combined.extend_from_slice(&f2);

    // Split mid-way through the second frame to test partial frame buffering.
    let split = f1.len() + 5;
    let first = decode_frames(&combined[..split]).unwrap();
    assert_eq!(first.frames.len(), 1);
    assert_eq!(first.frames[0].op, OP_HANDSHAKE);

    // Feed the remaining bytes of the second frame and verify it decodes cleanly.
    let mut remainder = first.rest;
    remainder.extend_from_slice(&combined[split..]);
    let second = decode_frames(&remainder).unwrap();
    assert_eq!(second.frames.len(), 1);
    assert_eq!(second.frames[0].op, OP_FRAME);
    assert!(second.rest.is_empty());

    // Frames exceeding `MAX_FRAME_BYTES` (64 KiB) are rejected immediately.
    let mut bad = [0u8; 8];
    bad[4..8].copy_from_slice(&((MAX_FRAME_BYTES as u32) + 10).to_le_bytes());
    assert!(decode_frames(&bad).is_err());

    // Windows IPC discovery probes named pipes `\\.\pipe\discord-ipc-0..9`.
    let win_paths = candidate_ipc_paths("win32", &BTreeMap::new(), None);
    assert_eq!(win_paths.len(), 10);
    assert!(win_paths.contains(&r"\\.\pipe\discord-ipc-0".to_string()));
    assert!(win_paths.contains(&r"\\.\pipe\discord-ipc-9".to_string()));

    // Linux IPC discovery probes standard, Flatpak, and Snap runtime directories.
    #[cfg(unix)]
    {
        let linux_paths = candidate_ipc_paths(
            "linux",
            &BTreeMap::from([("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string())]),
            Some(1000),
        );
        assert!(linux_paths.contains(&"/run/user/1000/discord-ipc-0".to_string()));
        assert!(linux_paths
            .contains(&"/run/user/1000/app/com.discordapp.Discord/discord-ipc-0".to_string()));
    }
}

// ---------------------------------------------------------------------------
// 8. Steam VDF, Shortcut & Overlay tests (ports the desktop host)
// ---------------------------------------------------------------------------

/// Verifies binary `shortcuts.vdf` roundtripping, shortcut upsert, relaunch decision, and `LD_PRELOAD` setup.
///
/// Mirrors the desktop host.
#[test]
fn steam_vdf_roundtrips_and_manages_non_steam_shortcuts_and_overlay() {
    // Non-Steam shortcut AppIDs must have bit 31 set, and GameIDs pack `(appId << 32) | 0x02000000`.
    let app_id = compute_shortcut_app_id("/opt/BattlefrontEternal/bfe", SHORTCUT_NAME);
    assert_ne!(app_id & 0x8000_0000, 0);
    let game_id = compute_shortcut_game_id(app_id);
    let parsed_gid: u64 = game_id.parse().unwrap();
    assert_eq!((parsed_gid >> 32) as u32, app_id);
    assert_eq!(parsed_gid & 0xffff_ffff, 0x0200_0000);

    // Serialize two shortcuts into Valve's binary VDF format and parse them back.
    let entries = vec![
        ShortcutEntry {
            appid: app_id,
            app_name: SHORTCUT_NAME.to_string(),
            exe: "\"/opt/BattlefrontEternal/bfe\"".to_string(),
            start_dir: "/opt/BattlefrontEternal".to_string(),
            icon: "/opt/BattlefrontEternal/icon.png".to_string(),
            launch_options: "--steam".to_string(),
            allow_desktop_config: 1,
            allow_overlay: 1,
            tags: vec!["Star Wars".to_string(), "Favorite".to_string()],
            ..Default::default()
        },
        ShortcutEntry {
            app_name: "Another Non-Steam Game".to_string(),
            exe: "/usr/games/other".to_string(),
            start_dir: "/usr/games".to_string(),
            ..Default::default()
        },
    ];
    let buf = serialize_shortcuts_vdf(&entries);
    let parsed = parse_shortcuts_vdf(&buf).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].appid, app_id);
    assert_eq!(parsed[0].tags, vec!["Star Wars", "Favorite"]);

    // Upserting an existing shortcut updates `exe` and `icon` while preserving user tags.
    let updated = upsert_shortcut_in_list(
        &parsed,
        &ShortcutTarget {
            exe: "/opt/BattlefrontEternal/new-bfe".to_string(),
            start_dir: "/opt/BattlefrontEternal".to_string(),
            launch_options: "--steam".to_string(),
            icon: "/opt/BattlefrontEternal/new-icon.png".to_string(),
        },
    );
    assert!(updated.existed);
    assert!(updated.changed);
    assert_eq!(updated.shortcuts.len(), 2);
    assert_eq!(updated.entry.tags, vec!["Star Wars", "Favorite"]);

    // Set up a synthetic Linux Steam installation with `userdata/12345678/config` and `gameoverlayrenderer.so`.
    let tmp = TempDir::new("steam-shortcut");
    let config_dir = tmp
        .path()
        .join(".local/share/Steam/userdata/12345678/config");
    let overlay_dir = tmp.path().join(".local/share/Steam/ubuntu12_64");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&overlay_dir).unwrap();
    let overlay_so = overlay_dir.join("gameoverlayrenderer.so");
    fs::write(&overlay_so, "ELF").unwrap();

    let icon_path = tmp.path().join("icon.png");
    fs::write(&icon_path, "PNG").unwrap();
    let app_image = tmp.path().join("Battlefront-Eternal.AppImage");
    fs::write(&app_image, "ELF").unwrap();

    let target_opts = ResolveTargetOptions {
        platform: "linux".to_string(),
        env: BTreeMap::from([(
            "APPIMAGE".to_string(),
            app_image.to_string_lossy().into_owned(),
        )]),
        exec_path: app_image.clone(),
        packaged: true,
        project_root: tmp.path().to_path_buf(),
        resources_path: None,
    };

    // Inspect before registration (`registered == false`), then register and re-inspect (`registered == true`).
    let before = inspect_steam_shortcut(&target_opts, Some(tmp.path()), &[]);
    assert!(before.steam_detected);
    assert!(!before.registered);

    let res = ensure_steam_shortcut(&target_opts, Some(tmp.path()), &[]);
    assert!(res.ok);
    assert_eq!(res.updated_files.len(), 1);

    let after = inspect_steam_shortcut(&target_opts, Some(tmp.path()), &[]);
    assert!(after.registered);

    // First auto-relaunch when already registered -> relaunch = true (`steam://rungameid/<gameId>`).
    let relaunch =
        decide_steam_relaunch(true, true, &target_opts, Some(tmp.path()), &[], Some(true));
    assert!(relaunch.relaunch);
    assert_eq!(
        relaunch.uri.as_deref(),
        Some(format!("steam://rungameid/{}", after.game_id).as_str())
    );

    // Verify `launched_under_steam`, `find_overlay_library`, and `prepare_steam_environment` (`LD_PRELOAD`).
    assert!(launched_under_steam(
        &BTreeMap::new(),
        &["--steam".to_string()]
    ));
    let found_lib = find_overlay_library("linux", "x64", Some(tmp.path()), &BTreeMap::new(), &[]);
    assert_eq!(found_lib.as_deref(), Some(overlay_so.as_path()));

    let env_res = prepare_steam_environment(
        "linux",
        "x64",
        Some(tmp.path()),
        &BTreeMap::new(),
        &["--steam".to_string()],
        Some(&after.game_id),
        &[],
    );
    assert!(env_res.under_steam);
    assert!(env_res.needs_reexec_for_preload);
    assert!(env_res.env["LD_PRELOAD"].contains("gameoverlayrenderer.so"));
}

// ---------------------------------------------------------------------------
// 9. Update tests (ports tests/desktop-update.test.js)
// ---------------------------------------------------------------------------

/// Verifies SemVer/Debian version ordering, Ed25519 channel envelope verification,
/// Zstandard manifest decompression, content-addressed blob installation with
/// `bfe-tree.json` reuse, `settle`, and `MAX_ATTEMPTS` crash-loop rollback.
///
/// Mirrors `tests/desktop-update.test.js`.
#[test]
fn update_versions_feed_signatures_payload_slots_and_rollback_work() {
    let _ = Value::Null;
    // 1. Version ordering rules (`0.1.0-preview.9 < 0.1.0-preview.10 < 0.1.0`, Debian `~` == `-`).
    assert_eq!(parse_version("v1.2.3").unwrap().major, 1);
    assert_eq!(
        compare_versions("0.1.0-preview.10", "0.1.0-preview.9"),
        Some(1)
    );
    assert_eq!(
        compare_versions("0.1.0~preview.262", "0.1.0-preview.262"),
        Some(0)
    );
    assert_eq!(compare_versions("0.1.0", "0.1.0-preview.999"), Some(1));
    assert!(is_newer_version("0.1.0-preview.332", "0.1.0-preview.331"));

    // 2. Pinned production key table contains only `33913637e5932c25`.
    assert_eq!(KEYS.len(), 1);
    assert_eq!(KEYS[0].0, PRODUCTION_KEY_ID);

    // 3. Relative path and URL safety checks.
    assert_eq!(
        safe_relative_path("resources/app.asar").as_deref(),
        Some("resources/app.asar")
    );
    assert_eq!(safe_relative_path("../escape"), None);
    assert_eq!(safe_relative_path("/abs/path"), None);
    assert_eq!(safe_relative_path(r"win\path"), None);
    assert_eq!(
        join_url(
            "https://download.battlefronteternal.com/",
            "updates/development-preview.json"
        )
        .unwrap(),
        "https://download.battlefronteternal.com/updates/development-preview.json"
    );
    assert!(join_url("https://download.battlefronteternal.com", "../secret").is_err());

    // 4. Synthetic signed channel envelope, Zstandard manifest, blob install, settle, and rollback.
    let tmp = TempDir::new("update-channel");
    let http_root = tmp.path().join("http");
    let update_root = tmp.path().join("root");
    let boot_dir = tmp.path().join("bootstrap");
    fs::create_dir_all(boot_dir.join("resources")).unwrap();

    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let pub_pem = encode_spki_pem(&signing_key.verifying_key());
    let key_id = key_id_from_verifying_key(&signing_key.verifying_key());
    let keys = BTreeMap::from([(key_id.clone(), pub_pem)]);

    let shared_data = b"unchanged-resource-payload";
    let shared_sha = sha256_bytes(shared_data);
    fs::write(boot_dir.join("resources/asset.dat"), shared_data).unwrap();
    fs::write(
        boot_dir.join("resources/bfe-release.json"),
        r#"{"version":"0.1.0-preview.331","channel":"development-preview"}"#,
    )
    .unwrap();
    fs::write(
        boot_dir.join("resources/bfe-tree.json"),
        format!(
            r#"{{"version":"0.1.0-preview.331","platform":"linux-x64","files":[{{"path":"resources/asset.dat","size":{},"sha256":"{shared_sha}","mode":420}}]}}"#,
            shared_data.len()
        ),
    )
    .unwrap();

    let write_release = |rel_id: &str, ver: &str, exe_bytes: &[u8]| -> serde_json::Value {
        let rel_meta = format!(r#"{{"version":"{ver}","channel":"development-preview"}}"#);
        let specs: Vec<(&str, &[u8], u32)> = vec![
            ("bfe-client", exe_bytes, 0o755),
            ("resources/asset.dat", shared_data, 0o644),
            ("resources/bfe-release.json", rel_meta.as_bytes(), 0o644),
        ];
        let mut files = Vec::new();
        for (rel_p, raw, mode) in specs {
            let sha = sha256_bytes(raw);
            let zst = encode_zstd_store_frame(raw);
            let blob_rel = format!("updates/blobs/{}/{sha}.zst", &sha[..2]);
            let full = http_root.join(&blob_rel);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, &zst).unwrap();
            files.push(ManifestFileEntry {
                path: rel_p.to_string(),
                size: raw.len() as u64,
                sha256: sha,
                mode: Some(mode),
                blob: Some(blob_rel),
                compressed_size: Some(zst.len() as u64),
            });
        }
        let manifest = PlatformManifest {
            version: ver.to_string(),
            release_id: Some(rel_id.to_string()),
            revision: Some("4b59932a".to_string()),
            platform: "linux-x64".to_string(),
            executable: "bfe-client".to_string(),
            server_executable: None,
            files,
            links: Vec::new(),
            dirs: Vec::new(),
        };
        let manifest_zst = encode_zstd_store_frame(&serde_json::to_vec(&manifest).unwrap());
        let manifest_sha = sha256_bytes(&manifest_zst);
        let manifest_rel = format!("{rel_id}/update-linux-x64.json.zst");
        let manifest_full = http_root.join(&manifest_rel);
        fs::create_dir_all(manifest_full.parent().unwrap()).unwrap();
        fs::write(&manifest_full, &manifest_zst).unwrap();

        let payload = ChannelPayload {
            channel: "development-preview".to_string(),
            published_at: Some("2026-11-08T00:00:00Z".to_string()),
            release: ChannelRelease {
                id: rel_id.to_string(),
                version: ver.to_string(),
                revision: Some("4b59932a".to_string()),
                platforms: BTreeMap::from([(
                    "linux-x64".to_string(),
                    ChannelPlatformEntry {
                        manifest: manifest_rel,
                        sha256: manifest_sha,
                        size: manifest_zst.len() as u64,
                        total_size: None,
                    },
                )]),
            },
        };
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let sig = signing_key.sign(&payload_bytes);
        let env_obj = SignedChannelEnvelope {
            key: key_id.clone(),
            payload: BASE64_STANDARD.encode(&payload_bytes),
            signature: BASE64_STANDARD.encode(sig.to_bytes()),
        };
        let val = serde_json::to_value(&env_obj).unwrap();
        let chan_file = http_root.join("updates/development-preview.json");
        fs::create_dir_all(chan_file.parent().unwrap()).unwrap();
        fs::write(&chan_file, serde_json::to_vec_pretty(&val).unwrap()).unwrap();
        val
    };

    let env_332 = write_release("preview-332", "0.1.0-preview.332", b"#!/bin/sh\nexit 0\n");
    let verified = verify_envelope(&env_332, &keys).unwrap();
    assert_eq!(verified.release.version, "0.1.0-preview.332");

    // Verify compressed manifest decoding and hash checking.
    let m_bytes = fs::read(http_root.join("preview-332/update-linux-x64.json.zst")).unwrap();
    let m_entry = &verified.release.platforms["linux-x64"];
    let decoded =
        decode_and_validate_manifest(&m_bytes, &m_entry.sha256, "0.1.0-preview.332", "linux-x64")
            .unwrap();
    assert_eq!(decoded.executable, "bfe-client");
    assert!(decode_and_validate_manifest(
        &m_bytes,
        &"0".repeat(64),
        "0.1.0-preview.332",
        "linux-x64"
    )
    .is_err());

    let ctx = UpdaterContext {
        own_dir: boot_dir,
        own_version: None,
        own_channel: None,
        root: Some(update_root.clone()),
        feed_url: Some(format!("file://{}", http_root.display())),
        keys,
        platform: "linux".to_string(),
        arch: "x64".to_string(),
        argv: vec!["bfe-client".to_string()],
        is_packaged: true,
        env: BTreeMap::new(),
        home: tmp.path().to_path_buf(),
        launch_timeout_ms: 2000,
        fetch_timeout_ms: 5000,
        blob_timeout_ms: 5000,
        concurrency: 2,
    };

    let res_332 = check_and_install::<fn(InstallProgress)>(&ctx, None);
    assert!(matches!(
        res_332,
        CheckOutcome::Installed {
            reused: 1,
            downloaded: 2,
            ..
        }
    ));
    assert!(versions_dir(&update_root)
        .join("0.1.0-preview.332")
        .join(INSTALL_RECORD)
        .exists());

    // Settle 0.1.0-preview.332 -> marked healthy in state.json.
    let settle_out = settle_with(&SettleOptions {
        own_dir: versions_dir(&update_root).join("0.1.0-preview.332"),
        own_version: None,
        own_channel: None,
        root: Some(update_root.clone()),
        bootstrap_version: Some("0.1.0-preview.331".to_string()),
        platform: "linux".to_string(),
        arch: "x64".to_string(),
        env: BTreeMap::new(),
        home: tmp.path().to_path_buf(),
    });
    assert!(settle_out.settled);
    assert_eq!(
        read_state(&update_root).healthy,
        vec!["0.1.0-preview.332".to_string()]
    );

    // Install 0.1.0-preview.333, fail MAX_ATTEMPTS (2) times, and verify rollback to 0.1.0-preview.332.
    write_release("preview-333", "0.1.0-preview.333", b"#!/bin/sh\nexit 1\n");
    let res_333 = check_and_install::<fn(InstallProgress)>(&ctx, None);
    assert!(matches!(res_333, CheckOutcome::Installed { .. }));
    assert_eq!(installed_versions(&update_root, Some("linux-x64")).len(), 2);

    for _ in 0..MAX_ATTEMPTS {
        record_attempt(&update_root, "0.1.0-preview.333").unwrap();
    }
    assert!(read_state(&update_root)
        .bad
        .contains(&"0.1.0-preview.333".to_string()));
    assert_eq!(
        choose_launch(&update_root, "0.1.0-preview.331", Some("linux-x64"), None)
            .unwrap()
            .version,
        "0.1.0-preview.332"
    );

    // Calling record_healthy clears bad/attempts; collect prunes versions not in keep/healthy.
    record_healthy(&update_root, "0.1.0-preview.331").unwrap();
    let pruned = collect(&update_root, &[], 2);
    assert_eq!(pruned, vec!["0.1.0-preview.333".to_string()]);
}

// ---------------------------------------------------------------------------
// 10. Content archive (.bfecontent) tests (ports the desktop host)
// ---------------------------------------------------------------------------

/// Verifies `.bfecontent` export, central directory inspection, import, hardlink deduplication,
/// game-install relocation (`relocate_preparation`), and `safe_path` traversal rejection.
///
/// Mirrors the desktop host.
#[test]
fn content_archive_exports_imports_deduplicates_hardlinks_and_relocates() {
    let tmp = TempDir::new("content-archive");
    let game = tmp.path().join("game");
    let source = tmp.path().join("source");
    let archive = tmp.path().join("battlefront.bfecontent");
    let dest = tmp.path().join("dest");

    make_install(&game, false);
    make_content(&source, &["bes1a", "tat1i"], true, Some(&game));

    // Create a hardlink inside maps/tat1i to verify hardlink deduplication.
    let hl_src = source.join("maps/bes1a/scene.json");
    let hl_dst = source.join("maps/tat1i/linked.json");
    let _ = fs::hard_link(&hl_src, &hl_dst);

    // Export the prepared content directory into a `.bfecontent` archive.
    let exported = export_content(&source, &archive, None).unwrap();
    assert_eq!(exported.maps, vec!["bes1a", "tat1i"]);
    assert!(archive.exists());

    // Inspect the archive's central directory and manifest without extracting files.
    let info = inspect_archive(&archive).unwrap();
    assert_eq!(info.maps, vec!["bes1a", "tat1i"]);
    assert_eq!(info.total_bytes, exported.bytes);

    // Cancelling after verified extraction must leave no installed or staging data.
    let cancelled_dest = tmp.path().join("cancelled");
    let result = import_content_with_progress(&archive, &cancelled_dest, |_, _, done, _| {
        if done > 0 {
            Err("cancelled".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.unwrap_err(), "cancelled");
    assert!(fs::read_dir(&cancelled_dest).unwrap().next().is_none());

    // Import the archive into a fresh parent directory and verify `.bfe-import.json`.
    let imported = import_content(&archive, &dest).unwrap();
    assert_eq!(imported.maps, vec!["bes1a", "tat1i"]);
    assert!(imported.content_dir.join(RECEIPT_NAME).exists());

    // Relocate imported content to a matching install copy.
    let second_install = tmp.path().join("second-install");
    make_install(&second_install, false);
    let reloc_ok = relocate_preparation(&imported.content_dir, &second_install, None).unwrap();
    assert!(reloc_ok.ok);

    // Relocate against a modified install must fail verification.
    let bad_install = tmp.path().join("bad-install");
    make_install(&bad_install, false);
    fs::write(
        bad_install.join("GameData/Data/_LVL_PC/SIDE/all.lvl"),
        "different-bytes",
    )
    .unwrap();
    let reloc_bad = relocate_preparation(&imported.content_dir, &bad_install, None).unwrap();
    assert!(!reloc_bad.ok);

    // Path safety checks reject traversal (`..`) and Windows reserved device names (`con.txt`).
    assert!(safe_path("../etc/passwd", true).is_err());
    assert!(safe_path("maps/con.txt", true).is_err());
    assert!(safe_path("maps/bes1a/scene.json", true).is_ok());
}

// ---------------------------------------------------------------------------
// Browser-build archive interop. The fixtures are synthetic bytes made here; the
// archive layout is `electron/content-archive.js`'s (manifest first, `{path, size,
// mtime, [linkTo, hardlink,] sha256}` records, deflated entries, `.prepare.json`).
// ---------------------------------------------------------------------------

/// One synthetic archive entry: name, payload, whether to deflate it (the browser
/// exporter does, level 3), general-purpose flags and the Unix mode in the attributes.
struct ZipItem {
    name: String,
    data: Vec<u8>,
    deflate: bool,
    flags: u16,
    mode: u32,
}

impl ZipItem {
    fn file(name: &str, data: Vec<u8>, deflate: bool) -> Self {
        Self {
            name: name.into(),
            data,
            deflate,
            flags: 0x0800,
            mode: 0o100644,
        }
    }
}

/// Writes a plain ZIP (no ZIP64: the fixtures stay far under 4 GiB) the way `yazl` lays one
/// out: a local header and payload per entry, then the central directory and end record.
fn write_zip(path: &Path, items: &[ZipItem]) {
    use std::io::Write;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for item in items {
        let crc = crc32fast::hash(&item.data);
        let payload = if item.deflate {
            let mut enc =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(3));
            enc.write_all(&item.data).unwrap();
            enc.finish().unwrap()
        } else {
            item.data.clone()
        };
        let method: u16 = if item.deflate { 8 } else { 0 };
        let offset = out.len() as u32;
        let name = item.name.as_bytes();
        // Fields shared by the local and central headers, from the flags on.
        let mut common = Vec::new();
        common.extend_from_slice(&item.flags.to_le_bytes());
        common.extend_from_slice(&method.to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());
        common.extend_from_slice(&0x21u16.to_le_bytes());
        common.extend_from_slice(&crc.to_le_bytes());
        common.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        common.extend_from_slice(&(item.data.len() as u32).to_le_bytes());
        common.extend_from_slice(&(name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&common);
        out.extend_from_slice(name);
        out.extend_from_slice(&payload);
        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&0x031eu16.to_le_bytes()); // made by Unix, version 3.0
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&common);
        central.extend_from_slice(&[0u8; 6]); // comment length, disk, internal attributes
        central.extend_from_slice(&(item.mode << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    out.extend_from_slice(&(items.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    fs::write(path, out).unwrap();
}

fn sha_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// A browser-form manifest record: `path`, `size`, `mtime`, `sha256`, plus `linkTo` and
/// `hardlink` for an alias.
fn record(path: &str, data: &[u8], mtime: f64, alias: Option<(&str, bool)>) -> Value {
    let mut r = json!({"path": path, "size": data.len(), "mtime": mtime, "sha256": sha_hex(data)});
    if let Some((target, hardlink)) = alias {
        r["linkTo"] = json!(target);
        r["hardlink"] = json!(hardlink);
    }
    r
}

/// Writes a synthetic archive in the browser's form and returns the payload of the big file.
fn browser_archive(path: &Path, big_len: usize, deflate: bool) -> Vec<u8> {
    let common = br#"{"ok":true}"#.to_vec();
    let map = br#"{"meshes":[{}]}"#.to_vec();
    let state = br#"{"version":2}"#.to_vec();
    // Zeros deflate to almost nothing, so a 70 MiB entry keeps the test quick while still
    // being past the old importer's 64 MiB in-memory limit.
    let big = vec![0u8; big_len];
    let manifest = json!({
        "format": "battlefront-eternal-content",
        "version": 1,
        "created": "2026-11-09T12:00:00.000Z",
        "source": {"base": "GameData", "files": [{"path": "Data/_LVL_PC/all.lvl", "size": 3, "sha256": sha_hex(b"lvl")}]},
        "files": [
            record(".prepare.json", &state, 1700000000000.5, None),
            record("audio/music/big.bin", &big, 1700000001000.0, None),
            record("common/common.json", &common, 1700000002000.0, None),
            record("common/Alias.json", &common, 1700000002000.0, Some(("common/common.json", false))),
            record("maps/bes1/map.json", &map, 1700000003000.0, None),
            record("maps/bes1/map-link.json", &map, 1700000003000.0, Some(("maps/bes1/map.json", true))),
        ],
    });
    write_zip(
        path,
        &[
            ZipItem {
                mode: 0o100664,
                ..ZipItem::file(
                    "bfe-content-manifest.json",
                    serde_json::to_vec(&manifest).unwrap(),
                    deflate,
                )
            },
            ZipItem::file(".prepare.json", state, deflate),
            ZipItem::file("audio/music/big.bin", big.clone(), deflate),
            ZipItem::file("common/common.json", common, deflate),
            ZipItem::file("maps/bes1/map.json", map, deflate),
        ],
    );
    big
}

#[test]
fn content_archive_imports_the_browser_builds_archive_form() {
    for deflate in [true, false] {
        let tmp = TempDir::new("browser-archive");
        let archive = tmp.path().join("export.bfepack");
        let dest = tmp.path().join("dest");
        let big = browser_archive(&archive, 70 * 1024 * 1024, deflate);

        let info = inspect_archive(&archive).unwrap();
        assert_eq!(info.files, 6);
        assert_eq!(info.maps, vec!["bes1"]);
        assert_eq!(info.created.as_deref(), Some("2026-11-09T12:00:00.000Z"));

        // Progress is in bytes, moves through the big file chunk by chunk, and ends at the total.
        let total_bytes = big.len() as u64 + 11 + 15 + 13 + 11 + 15;
        let mut calls = 0usize;
        let mut last = (String::new(), 0u64, 0u64);
        let imported =
            import_content_with_progress(&archive, &dest, |phase, _file, done, total| {
                calls += 1;
                assert!(
                    done >= last.1 || phase != last.0,
                    "bytes only go forward within a phase"
                );
                last = (phase.to_string(), done, total);
                Ok(())
            })
            .unwrap();
        assert!(
            calls > 1000,
            "a 70 MiB file reports about every 64 KiB, got {calls} calls"
        );
        assert_eq!(
            last,
            (
                "Restoring shared content".to_string(),
                total_bytes,
                total_bytes
            )
        );
        assert_eq!(imported.maps, vec!["bes1"]);

        let dir = &imported.content_dir;
        assert_eq!(fs::read(dir.join("audio/music/big.bin")).unwrap(), big);
        assert_eq!(
            fs::read(dir.join("common/Alias.json")).unwrap(),
            br#"{"ok":true}"#
        );
        assert_eq!(
            fs::read(dir.join("maps/bes1/map-link.json")).unwrap(),
            br#"{"meshes":[{}]}"#
        );
        // The manifest's mtime is restored on the stored files.
        let mtime = fs::metadata(dir.join("common/common.json"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            mtime
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            1700000002000
        );
        // The receipt is the manifest, so the browser build's `relocatePreparation` can read it.
        let receipt: Value =
            serde_json::from_slice(&fs::read(dir.join(RECEIPT_NAME)).unwrap()).unwrap();
        assert_eq!(receipt["format"], "battlefront-eternal-content");
        assert_eq!(receipt["source"]["base"], "GameData");
        assert_eq!(receipt["files"].as_array().unwrap().len(), 6);
    }
}

#[test]
fn content_archive_import_cancels_within_a_chunk_and_cleans_up() {
    let tmp = TempDir::new("cancel-archive");
    let archive = tmp.path().join("export.bfepack");
    let dest = tmp.path().join("dest");
    browser_archive(&archive, 70 * 1024 * 1024, true);
    let mut after_cancel = 0usize;
    let mut cancelled = false;
    let result = import_content_with_progress(&archive, &dest, |_, _, done, _| {
        if cancelled {
            after_cancel += 1;
        }
        if done > 1024 * 1024 {
            cancelled = true;
            return Err("cancelled".into());
        }
        Ok(())
    });
    assert_eq!(result.unwrap_err(), "cancelled");
    // The callback is not called again once it refused: the copy stops at that chunk.
    assert_eq!(after_cancel, 0);
    assert!(
        fs::read_dir(&dest).unwrap().next().is_none(),
        "no staging directory is left"
    );
}

#[test]
fn content_archive_export_is_the_browser_builds_form() {
    let tmp = TempDir::new("export-form");
    let game = tmp.path().join("game");
    let source = tmp.path().join("source");
    let archive = tmp.path().join("out.bfepack");
    make_install(&game, false);
    make_content(&source, &["bes1a"], true, Some(&game));
    fs::create_dir_all(source.join("audio")).unwrap();
    fs::write(source.join("audio/a.bin"), vec![7u8; 100_000]).unwrap();
    fs::hard_link(source.join("audio/a.bin"), source.join("audio/b-link.bin")).unwrap();
    fs::write(source.join("audio/Case.json"), "{}").unwrap();
    fs::write(source.join("audio/case.json"), "{}").unwrap();

    let exported = export_content(&source, &archive, None).unwrap();
    assert_eq!(exported.maps, vec!["bes1a"]);

    let mut file = File::open(&archive).unwrap();
    let entries = read_zip_entries(&mut file).unwrap();
    // The manifest is the first entry in the file, then the stored files (aliases have none).
    assert_eq!(entries["bfe-content-manifest.json"].offset, 0);
    assert_eq!(
        entries["bfe-content-manifest.json"].external_attrs >> 16,
        0o100664
    );
    for name in [
        ".prepare.json",
        "common/common.json",
        "sides/all/manifest.json",
        "audio/a.bin",
    ] {
        assert_eq!(entries[name].external_attrs >> 16, 0o100644, "{name}");
        assert!(entries[name].offset > 0, "{name}");
    }
    assert!(
        !entries.contains_key("audio/b-link.bin"),
        "a hardlink has no payload entry"
    );
    assert!(entries.contains_key("audio/Case.json") != entries.contains_key("audio/case.json"));

    // The manifest is stored, so its text sits right after its local header (30 bytes + name).
    let raw = fs::read(&archive).unwrap();
    let start = 30 + "bfe-content-manifest.json".len();
    let text = std::str::from_utf8(
        &raw[start..start + entries["bfe-content-manifest.json"].size as usize],
    )
    .unwrap();
    assert!(
        text.starts_with(r#"{"format":"battlefront-eternal-content","version":1,"created":""#),
        "compact JSON in the browser's key order, got {text:.80}"
    );
    assert!(
        text.contains(r#""path":".prepare.json","size":"#),
        "records start with path, then size"
    );
    let manifest: Value = serde_json::from_str(text).unwrap();
    let mut keys: Vec<_> = manifest.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["created", "files", "format", "source", "version"]);
    assert_eq!(manifest["source"]["base"], "GameData");
    for rec in manifest["files"].as_array().unwrap() {
        let mut keys: Vec<_> = rec.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        let alias = rec.get("linkTo").is_some();
        let want: &[&str] = if alias {
            &["hardlink", "linkTo", "mtime", "path", "sha256", "size"]
        } else {
            &["mtime", "path", "sha256", "size"]
        };
        assert_eq!(keys, want, "{rec}");
        assert!(rec["mtime"].is_f64() || rec["mtime"].is_u64(), "{rec}");
    }
    let link = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == "audio/b-link.bin")
        .unwrap();
    assert_eq!(link["linkTo"], "audio/a.bin");
    assert_eq!(link["hardlink"], true);
    let created = manifest["created"].as_str().unwrap();
    assert_eq!(created.len(), 24);
    assert!(
        created.ends_with('Z') && created.as_bytes()[10] == b'T',
        "{created}"
    );

    // The archive imports back, and a re-export of the import is the same set of records.
    let imported = import_content(&archive, &tmp.path().join("dest")).unwrap();
    assert_eq!(
        fs::read(imported.content_dir.join("audio/b-link.bin")).unwrap(),
        vec![7u8; 100_000]
    );
}

#[test]
fn content_archive_rejects_what_the_browser_build_rejects() {
    let tmp = TempDir::new("reject-archive");
    let dest = tmp.path().join("dest");
    let common = b"{}".to_vec();
    let map = b"{}".to_vec();
    let rec = |p: &str, d: &[u8]| record(p, d, 1.0, None);
    let manifest = |files: Vec<Value>| {
        serde_json::to_vec(&json!({"format": "battlefront-eternal-content", "version": 1, "created": "x", "source": null, "files": files})).unwrap()
    };
    let good = vec![
        rec("common/common.json", &common),
        rec("maps/m/map.json", &map),
    ];
    let items = |manifest: Vec<u8>| {
        vec![
            ZipItem::file("bfe-content-manifest.json", manifest, true),
            ZipItem::file("common/common.json", common.clone(), true),
            ZipItem::file("maps/m/map.json", map.clone(), true),
        ]
    };
    let attempt = |name: &str, items: Vec<ZipItem>| -> String {
        let path = tmp.path().join(format!("{name}.bfepack"));
        write_zip(&path, &items);
        import_content(&path, &dest).unwrap_err()
    };
    // Not an export at all.
    assert_eq!(
        attempt(
            "plain",
            vec![ZipItem::file("common/x.json", b"x".to_vec(), true)]
        ),
        "This file is not a Battlefront Eternal content export"
    );
    // An entry outside the content roots is refused before the manifest is even looked for.
    assert_eq!(
        attempt(
            "readme",
            vec![ZipItem::file("readme.txt", b"x".to_vec(), true)]
        ),
        "Not prepared game content: readme.txt"
    );
    // No playable map.
    let no_map = vec![rec("common/common.json", &common)];
    assert_eq!(
        attempt(
            "nomap",
            vec![
                ZipItem::file("bfe-content-manifest.json", manifest(no_map), true),
                ZipItem::file("common/common.json", common.clone(), true)
            ]
        ),
        "Export has no playable prepared maps"
    );
    // A wrong hash.
    let mut bad = good.clone();
    bad[1]["sha256"] = json!(sha_hex(b"other"));
    assert_eq!(
        attempt("hash", items(manifest(bad))),
        "Checksum mismatch: maps/m/map.json. The export is damaged or incomplete."
    );
    // A path outside the content roots, and a traversal.
    let mut outside = good.clone();
    outside.push(rec("tools/x.py", b"x"));
    assert_eq!(
        attempt("outside", items(manifest(outside))),
        "Not prepared game content: tools/x.py"
    );
    let mut up = good.clone();
    up.push(rec("common/../x", b"x"));
    assert_eq!(
        attempt("up", items(manifest(up))),
        "Unsafe archive path: common/../x"
    );
    // Encrypted and non-regular entries.
    let mut enc = items(manifest(good.clone()));
    enc[1].flags |= 1;
    assert_eq!(
        attempt("enc", enc),
        "Encrypted content exports are not supported"
    );
    let mut link = items(manifest(good.clone()));
    link[2].mode = 0o120777;
    assert_eq!(
        attempt("symlink", link),
        "Only regular files are allowed in content exports"
    );
    // An entry the manifest does not list.
    let mut extra = items(manifest(good.clone()));
    extra.push(ZipItem::file("common/extra.json", b"{}".to_vec(), true));
    assert_eq!(
        attempt("extra", extra),
        "Archive files do not match the manifest"
    );
    assert!(
        fs::read_dir(&dest).unwrap().next().is_none(),
        "failed imports leave nothing behind"
    );
}

/// Verifies `read_zip_entries`, `inspect_archive`, and `import_content` return errors
/// and never panic when fed a truncated archive, a garbage file, or an archive whose
/// central directory points outside the file.
#[test]
fn content_archive_rejects_truncated_garbage_and_out_of_bounds_archives() {
    let tmp = TempDir::new("malformed-archives");
    let source = tmp.path().join("source");
    let valid_archive = tmp.path().join("valid.bfecontent");
    let dest = tmp.path().join("dest");

    make_content(&source, &["bes1a"], true, None);
    export_content(&source, &valid_archive, None).unwrap();
    let valid_bytes = fs::read(&valid_archive).unwrap();

    // 1. Garbage file: arbitrary non-ZIP bytes (both shorter than 22 bytes and longer than 22 bytes).
    let short_garbage = tmp.path().join("short.bfecontent");
    fs::write(&short_garbage, b"not a zip").unwrap();
    assert!(read_zip_entries(&mut File::open(&short_garbage).unwrap()).is_err());
    assert!(inspect_archive(&short_garbage).is_err());
    assert!(import_content(&short_garbage, &dest).is_err());

    let long_garbage = tmp.path().join("garbage.bfecontent");
    fs::write(&long_garbage, [0xdeu8; 256]).unwrap();
    assert!(read_zip_entries(&mut File::open(&long_garbage).unwrap()).is_err());
    assert!(inspect_archive(&long_garbage).is_err());
    assert!(import_content(&long_garbage, &dest).is_err());

    // 2. Truncated archive: slice off the trailing EOCD and central directory bytes,
    // and also test an archive with a valid EOCD whose central directory entry is truncated mid-header.
    let truncated = tmp.path().join("truncated.bfecontent");
    fs::write(&truncated, &valid_bytes[..valid_bytes.len() / 2]).unwrap();
    assert!(read_zip_entries(&mut File::open(&truncated).unwrap()).is_err());
    assert!(inspect_archive(&truncated).is_err());
    assert!(import_content(&truncated, &dest).is_err());

    // Build an archive whose EOCD claims 1 entry in a 10-byte central directory (smaller than 46-byte header).
    let mut trunc_cd = vec![0u8; 10];
    trunc_cd[0..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
    trunc_cd.extend_from_slice(&0x06054b50u32.to_le_bytes());
    trunc_cd.extend_from_slice(&0u16.to_le_bytes());
    trunc_cd.extend_from_slice(&0u16.to_le_bytes());
    trunc_cd.extend_from_slice(&1u16.to_le_bytes());
    trunc_cd.extend_from_slice(&1u16.to_le_bytes());
    trunc_cd.extend_from_slice(&10u32.to_le_bytes());
    trunc_cd.extend_from_slice(&0u32.to_le_bytes());
    trunc_cd.extend_from_slice(&0u16.to_le_bytes());
    let trunc_cd_file = tmp.path().join("truncated-cd.bfecontent");
    fs::write(&trunc_cd_file, &trunc_cd).unwrap();
    assert!(read_zip_entries(&mut File::open(&trunc_cd_file).unwrap()).is_err());
    assert!(inspect_archive(&trunc_cd_file).is_err());
    assert!(import_content(&trunc_cd_file, &dest).is_err());

    // 3. Archive with a central directory pointing outside the file (both standard EOCD and ZIP64 locator).
    let mut oob_bytes = valid_bytes.clone();
    let eocd_start = oob_bytes.len() - 22;
    // Patch `central_offset` (EOCD bytes 16..20) to point well past `oob_bytes.len()`.
    let bogus_offset = (oob_bytes.len() as u32).saturating_add(100_000);
    oob_bytes[eocd_start + 16..eocd_start + 20].copy_from_slice(&bogus_offset.to_le_bytes());
    let oob_archive = tmp.path().join("oob-cd.bfecontent");
    fs::write(&oob_archive, &oob_bytes).unwrap();
    let oob_err = read_zip_entries(&mut File::open(&oob_archive).unwrap()).unwrap_err();
    assert!(oob_err.contains("out of bounds"));
    assert!(inspect_archive(&oob_archive).is_err());
    assert!(import_content(&oob_archive, &dest).is_err());

    // Also test a hostile ZIP64 EOCD locator whose 64-bit `eocd64_offset` points outside the file.
    let mut zip64_oob = Vec::new();
    zip64_oob.extend_from_slice(&0x07064b50u32.to_le_bytes());
    zip64_oob.extend_from_slice(&0u32.to_le_bytes());
    zip64_oob.extend_from_slice(&999_999_999u64.to_le_bytes());
    zip64_oob.extend_from_slice(&1u32.to_le_bytes());
    zip64_oob.extend_from_slice(&0x06054b50u32.to_le_bytes());
    zip64_oob.extend_from_slice(&[0u8; 18]);
    let zip64_oob_file = tmp.path().join("oob-zip64.bfecontent");
    fs::write(&zip64_oob_file, &zip64_oob).unwrap();
    assert!(read_zip_entries(&mut File::open(&zip64_oob_file).unwrap()).is_err());
    assert!(inspect_archive(&zip64_oob_file).is_err());
    assert!(import_content(&zip64_oob_file, &dest).is_err());
}

#[test]
fn discover_gog_honors_the_browser_xdg_config_manifest_location() {
    let temp = TempDir::new("gog-xdg");
    let home = temp.path().join("home");
    let config = temp.path().join("custom-config");
    let game = temp.path().join("GOG Games/Star Wars Battlefront");
    make_install(&game, false);
    let manifest = config.join("heroic/gog_store/installed.json");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(
        &manifest,
        serde_json::json!({"installed":[{"title":"Star Wars Battlefront","install_path":game}]})
            .to_string(),
    )
    .unwrap();
    let env = BTreeMap::from([(
        "XDG_CONFIG_HOME".into(),
        config.to_string_lossy().into_owned(),
    )]);
    let records = bfe_desktop::discover::gog_records("linux", Some(&home), &env);
    assert_eq!(records, vec![game]);
}
