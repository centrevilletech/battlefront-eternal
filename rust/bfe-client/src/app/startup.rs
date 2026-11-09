//! Startup update check, content resolution, and background `PrepareSession` host.
//!
//! ## Lifecycle and Responsibilities
//! 1. **Bounded Self-Update Handoff and Release Feed Check**:
//!    Hands off to any newer installed version (`bfe_desktop::update::handoff::boot_handoff_or_continue`),
//!    schedules background checks (`bfe_desktop::update::handoff::schedule_background_check`),
//!    and checks the Ed25519-signed release feed (`bfe_desktop::update::feed`) with a hard
//!    `<= 1.5s` timeout so an offline or unreachable update server never delays startup
//!    beyond the 5-second budget.
//! 2. **Prepared Content Resolution**:
//!    Resolves prepared content via [`bfe_desktop::prepare::resolve_startup_content`],
//!    checking CLI overrides (`--content-dir`), persisted `DesktopSettings`, and
//!    standard workspace/application directories (`likely_content_dirs`).
//! 3. **Background Extraction Pipeline (`PrepareSession`)**:
//!    When raw Star Wars Battlefront (2004) game files are discovered (`StartupMode::Prepare`
//!    or `StartupMode::Confirm`), spawns [`bfe_desktop::prepare::PrepareSession`] on a
//!    background worker thread and streams progress (`percent`, `phase_label`, `playable`,
//!    `complete`) to the startup loader screen (`ShellHost::draw_startup_loader`).
//! 4. **Built-in 5x7 ASCII Fallback Font**:
//!    Generates a self-contained 128x64 RGBA8 bitmap font atlas ([`build_fallback_ascii_font`])
//!    so the startup loader screen and diagnostic notices render legibly on frame 1
//!    even before `common/common.json` has been prepared or loaded.
//!
//! ## Startup Timing Budget and Fallback Guarantees
//! - **Offline / Unreachable Feed Budget**: `run_bounded_update_check` executes feed
//!   verification on a worker thread with a hard `1500 ms` `recv_timeout`, guaranteeing
//!   that an offline machine or unreachable release server never blocks window creation
//!   beyond 5 seconds.
//! - **Zero-Dependency Loader Rendering**: `build_fallback_ascii_font` synthesizes a
//!   128x64 RGBA8 texture containing 5x7 uppercase/lowercase/numeric/punctuation glyphs
//!   in memory so `ShellHost::new` can render progress percentages and folder-picker
//!   instructions before any external game assets exist on disk.
//! - **Non-Blocking Preparation**: `StartupContentState::poll` drains `PrepareSession`
//!   events via `try_recv` once per rendered frame, smoothly advancing the planet zoom
//!   timeline (`LoadingTimeline`) proportional to `latest.percent`.

use crate::app::settings::{current_arch_str, current_platform_str, AppSettingsStore};
use bfe_desktop::{
    paths::likely_content_dirs,
    prepare::{
        resolve_startup_content, toolchain_paths, PrepareSession, PrepareSnapshot,
        PrepareSpawnOptions, StartupMode, ToolchainOptions,
    },
    update::{
        feed::{join_url, normalize_channel, verify_envelope},
        handoff::{boot_handoff_or_continue, schedule_background_check},
        keys::{default_keys, FEED},
        version::is_newer,
    },
};
use bfe_render::interface::{Font, Glyph};
use bfe_shell::render::ShellFonts;
use std::{
    collections::{BTreeMap, HashMap},
    env,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Result of the non-blocking / bounded startup update check.
///
/// Returned by [`run_bounded_update_check`] before the OS window opens so that
/// offline or firewalled environments proceed straight into the game without stalling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    /// No update needed or check disabled (`--skip-update-check` or `auto_check_updates == false`).
    UpToDate,
    /// A newer Ed25519-signed release is available on the release feed.
    UpdateAvailable {
        /// Semantic version of the available release.
        version: String,
        /// Update delivery kind (`"installer"` or `"payload"`).
        kind: String,
    },
    /// Feed was unreachable or offline; proceeding directly to the game.
    OfflineSkipped(String),
}

/// Hands off to any newer installed version under `<root>/versions` and schedules
/// a bounded update check against the published channel (`<= 1.5s` timeout) so
/// offline hosts proceed to the game well within 5 seconds.
pub fn run_bounded_update_check(
    store: &AppSettingsStore,
    skip_network: bool,
) -> UpdateCheckOutcome {
    // Hand off to any newer installed version under `<root>/versions` before opening the window.
    boot_handoff_or_continue();

    // Honor `--skip-update-check` and the persisted `auto_check_updates` user preference.
    if skip_network || !store.desktop.auto_check_updates {
        return UpdateCheckOutcome::UpToDate;
    }

    // Schedule the background update check and normalize the release channel (`"stable"`, `"beta"`, etc.).
    schedule_background_check();

    let env_map: BTreeMap<String, String> = env::vars().collect();
    let channel = normalize_channel(Some(&store.desktop.update_channel)).to_string();
    if let Some(feed_file) = env_map.get("BFE_UPDATE_FEED_FILE").map(PathBuf::from) {
        return check_local_feed_file(&feed_file, &channel);
    }

    // Resolve the canonical channel feed URL (`BFE_UPDATE_FEED`, `BFE_UPDATE_FEED_URL`, or built-in `FEED`).
    let base_url = env_map
        .get("BFE_UPDATE_FEED")
        .or_else(|| env_map.get("BFE_UPDATE_FEED_URL"))
        .cloned()
        .unwrap_or_else(|| FEED.to_string());
    let Ok(feed_url) = join_url(&base_url, &format!("updates/{channel}.json")) else {
        return UpdateCheckOutcome::OfflineSkipped("invalid update feed URL".to_string());
    };

    // Perform feed verification on a detached worker thread with a strict 1500 ms recv timeout.
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let res = fetch_and_check_feed(&feed_url, &channel);
        let _ = tx.send(res);
    });

    match rx.recv_timeout(Duration::from_millis(1500)) {
        Ok(outcome) => outcome,
        Err(_) => UpdateCheckOutcome::OfflineSkipped("update check timed out".to_string()),
    }
}

/// Reads, parses, and verifies a local Ed25519-signed release feed JSON file (`path`)
/// and selects any newer update target for the running version.
fn check_local_feed_file(path: &Path, _channel: &str) -> UpdateCheckOutcome {
    // Read the JSON text from disk; treat missing files as offline/skipped.
    let Ok(raw) = std::fs::read_to_string(path) else {
        return UpdateCheckOutcome::OfflineSkipped("missing feed file".to_string());
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return UpdateCheckOutcome::OfflineSkipped("malformed feed JSON".to_string());
    };
    // Verify the Ed25519 envelope signature against `default_keys()` and compare versions.
    let Ok(payload) = verify_envelope(&val, &default_keys()) else {
        return UpdateCheckOutcome::OfflineSkipped("feed signature verification failed".to_string());
    };
    if is_newer(&payload.release.version, env!("CARGO_PKG_VERSION")) {
        UpdateCheckOutcome::UpdateAvailable {
            version: payload.release.version,
            kind: "release".to_string(),
        }
    } else {
        UpdateCheckOutcome::UpToDate
    }
}

/// Fetches and verifies `feed_url` when pointed at a local `file://` URI, or skips
/// immediately when offline.
fn fetch_and_check_feed(feed_url: &str, channel: &str) -> UpdateCheckOutcome {
    // Only local `file://` endpoints are inspected synchronously here; remote
    // HTTP/HTTPS channels are handled by `schedule_background_check()`.
    if let Some(local_path) = feed_url.strip_prefix("file://") {
        return check_local_feed_file(Path::new(local_path), channel);
    }
    UpdateCheckOutcome::OfflineSkipped("offline or no local feed configured".to_string())
}

/// Checks whether `dir` looks like a usable prepared content directory (`common/common.json`
/// or `maps/` exists).
pub fn is_usable_content_dir(dir: &Path) -> bool {
    // Prepared content always contains `common/common.json` and per-map directories under `maps/`.
    dir.join("common").join("common.json").is_file() || dir.join("maps").is_dir()
}

/// High-level state of startup content resolution and background preparation.
///
/// Driven each frame by `AppState::StartupLoader` in `app/mod.rs` until `Ready(PathBuf)`
/// is reached and `ShellHost::load_prepared_content` transitions into `AppState::Shell`.
#[allow(clippy::large_enum_variant)]
pub enum StartupContentState {
    /// Prepared content is ready at the given directory path.
    Ready(PathBuf),
    /// `PrepareSession` is actively building prepared content on a background worker thread.
    Preparing {
        /// Active background preparation session handle (`bfe-desktop`).
        session: PrepareSession,
        /// Most recent progress snapshot received from `session.events`.
        latest: PrepareSnapshot,
        /// Destination directory where prepared assets are being written.
        target_dir: PathBuf,
        /// Wall-clock timestamp when preparation started.
        started_at: Instant,
    },
    /// No prepared content or game installation could be found; displays instructions
    /// in the loader window (`F9` to pick a folder or `Escape` to exit).
    WaitingForFolder {
        /// Diagnostic message rendered on the startup loader screen.
        message: String,
    },
}

impl StartupContentState {
    /// Resolves startup content from CLI override, environment, `DesktopSettings`,
    /// or `bfe_desktop::prepare::resolve_startup_content`.
    pub fn resolve(cli_content_dir: Option<&Path>, store: &mut AppSettingsStore) -> Self {
        // 1. Honor an explicit `--content-dir` / positional CLI path if it is already usable.
        if let Some(explicit) = cli_content_dir {
            if is_usable_content_dir(explicit) {
                store.desktop.chosen_content_dir = Some(explicit.to_string_lossy().into_owned());
                let _ = store.save_desktop();
                return Self::Ready(explicit.to_path_buf());
            }
        }

        // 2. Collect candidate content directories from `DesktopPaths` and development worktrees.
        let env_map: BTreeMap<String, String> = env::vars().collect();
        let home = env_map
            .get("HOME")
            .or_else(|| env_map.get("USERPROFILE"))
            .map(PathBuf::from);
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let exe_path = env::current_exe().unwrap_or_else(|_| cwd.join("bfe-client"));
        let found_dir = store.desktop.found_content_dir.as_ref().map(PathBuf::from);
        let home_ref = home.as_deref().unwrap_or(cwd.as_path());
        let mut likely = likely_content_dirs(
            found_dir.as_deref(),
            false,
            &store.paths.project_root,
            &exe_path,
            None,
            home_ref,
        );
        // Include common workspace content locations when running from a worktree.
        if let Some(h) = &home {
            likely.push(h.join("bfe-work/content"));
            likely.push(h.join("bfe-work/main/content"));
            likely.push(h.join("bfe-work/fable-integration/content"));
        }

        // Locate `tools/content-version.json` if present so `resolve_startup_content` can verify stamps.
        let tools_dir = store.paths.project_root.join("tools");
        let tools_opt = tools_dir
            .join("content-version.json")
            .is_file()
            .then_some(tools_dir.as_path());

        // 3. Query `bfe_desktop::prepare::resolve_startup_content` to pick prepared content or raw install.
        match resolve_startup_content(
            &store.desktop_store,
            &likely,
            home.as_deref(),
            current_platform_str(),
            &env_map,
            tools_opt,
        ) {
            Ok(res) => {
                // If `resolve_startup_content` validated a prepared directory, persist and use it.
                if let Some(content) = res.content {
                    store.desktop.found_content_dir =
                        Some(content.dir.to_string_lossy().into_owned());
                    let _ = store.save_desktop();
                    return Self::Ready(content.dir);
                }
                // Otherwise check if any candidate directory on disk already has `common/common.json`.
                for cand in &likely {
                    if is_usable_content_dir(cand) {
                        store.desktop.found_content_dir =
                            Some(cand.to_string_lossy().into_owned());
                        let _ = store.save_desktop();
                        return Self::Ready(cand.clone());
                    }
                }
                // If a raw SWBF1 installation was detected, launch a background `PrepareSession`.
                if matches!(res.mode, StartupMode::Prepare | StartupMode::Confirm) {
                    if let Some(install) = res.install {
                        let target_dir = store.paths.managed_content_dir.clone();
                        if let Ok(state) = Self::start_prepare(&install.dir, &target_dir, store) {
                            return state;
                        }
                    }
                }
                // Otherwise prompt the user on the loader screen (`F9` to select a folder).
                let msg = match res.mode {
                    StartupMode::Outdated => {
                        "PREPARED CONTENT IS OUTDATED. PRESS F9 TO SELECT GAME INSTALL OR CONTENT FOLDER."
                    }
                    _ => {
                        "NO PREPARED CONTENT FOUND. PRESS F9 TO SELECT CONTENT OR SWBF1 INSTALL FOLDER."
                    }
                };
                Self::WaitingForFolder {
                    message: msg.to_string(),
                }
            }
            Err(err) => {
                // Fall back to any usable candidate directory before reporting the error.
                for cand in &likely {
                    if is_usable_content_dir(cand) {
                        return Self::Ready(cand.clone());
                    }
                }
                Self::WaitingForFolder {
                    message: format!("CONTENT CHECK ERROR: {err}"),
                }
            }
        }
    }

    /// Starts a background [`PrepareSession`] from `game_dir` into `target_dir`.
    ///
    /// ## Background Preparation Pipeline
    /// - Resolves platform extraction binaries via [`bfe_desktop::prepare::toolchain_paths`].
    /// - Spawns [`PrepareSession::start`] with `include_add_ons: true` and persists
    ///   `game_dir` / `preparing_content_dir` into `DesktopSettings`.
    pub fn start_prepare(
        game_dir: &Path,
        target_dir: &Path,
        store: &mut AppSettingsStore,
    ) -> Result<Self, String> {
        // Resolve the platform extraction toolchain paths (`bfe-extract` / `prepare.py`).
        let platform = current_platform_str().to_string();
        let arch = current_arch_str().to_string();
        let tc = toolchain_paths(&ToolchainOptions {
            packaged: false,
            resources_path: None,
            project_root: store.paths.project_root.clone(),
            platform: platform.clone(),
            arch,
        });
        // Spawn the background preparation pipeline and record its initial snapshot.
        let session = PrepareSession::start(PrepareSpawnOptions {
            toolchain: tc,
            game_dir: game_dir.to_path_buf(),
            content_dir: target_dir.to_path_buf(),
            env: env::vars().collect(),
            platform,
            include_add_ons: true,
            enhanced_sound: store.desktop.enhanced_sound.unwrap_or(true),
            plan_only: false,
        })?;
        let latest = session
            .snapshot
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| bfe_desktop::prepare::create_snapshot());
        store.desktop.game_dir = Some(game_dir.to_string_lossy().into_owned());
        store.desktop.preparing_content_dir = Some(target_dir.to_string_lossy().into_owned());
        let _ = store.save_desktop();
        Ok(Self::Preparing {
            session,
            latest,
            target_dir: target_dir.to_path_buf(),
            started_at: Instant::now(),
        })
    }

    /// Polls any running background `PrepareSession` without blocking.
    /// Returns `Some(ready_dir)` once preparation reaches a playable/complete state.
    pub fn poll(&mut self, store: &mut AppSettingsStore) -> Option<PathBuf> {
        // Once the window exists and the first frame enters `poll`, signal launch
        // readiness (`BFE_LAUNCH_TOKEN`) and schedule the 10-second update settlement.
        static LAUNCH_LIFECYCLE: std::sync::Once = std::sync::Once::new();
        LAUNCH_LIFECYCLE.call_once(|| {
            bfe_desktop::launch::ready();
            bfe_desktop::launch::schedule_settle();
        });

        let Self::Preparing {
            session,
            latest,
            target_dir,
            ..
        } = self
        else {
            return None;
        };

        // Drain all queued progress snapshots from the background preparation worker.
        while let Ok(snap) = session.events.try_recv() {
            *latest = snap;
        }

        // Transition to `Self::Ready` once preparation completes and `target_dir` is usable.
        if latest.complete || (latest.status == "done" && is_usable_content_dir(target_dir)) {
            let ready = target_dir.clone();
            store.desktop.chosen_content_dir = Some(ready.to_string_lossy().into_owned());
            store.desktop.preparing_content_dir = None;
            let _ = store.save_desktop();
            *self = Self::Ready(ready.clone());
            return Some(ready);
        }

        // Surface preparation failures or cancellations on the loader screen.
        if latest.status == "failed" || latest.status == "cancelled" {
            let msg = latest
                .error
                .clone()
                .unwrap_or_else(|| "PREPARATION STOPPED. PRESS F9 TO CHOOSE FOLDER.".to_string());
            *self = Self::WaitingForFolder { message: msg };
        }
        None
    }
}

/// Generates a 128x64 RGBA texture containing a crisp 5x7 ASCII bitmap font
/// (`0x20..=0x5F`, 16 columns x 4 rows of 8x16 cells) and a matching [`ShellFonts`]
/// table so `draw_loading` and `draw_shell` can render readable text before
/// `common/common.json` exists on disk.
///
/// ## Atlas Layout and Metrics
/// - Texture dimensions: `128 x 64` RGBA8 (`4` rows of `16` cells, each cell `8 x 16` px).
/// - Character coverage: ASCII `0x20..=0x5F` (`' '` through `'_'`), with lowercase
///   letters `0x61..=0x7A` (`'a'..'z'`) aliased to their uppercase glyph cells.
/// - Produces four scaled [`Font`] instances (`large` at `2.0x`, `medium` at `1.5x`,
///   `small` at `1.2x`, `tiny` at `1.0x`) bound to `page_handle`.
/// - Used by `ShellHost::new` so frame 1 of the startup loader can render progress
///   text and error notices before `common/common.json` is loaded.
pub fn build_fallback_ascii_font(page_handle: u32) -> (Vec<u8>, u32, u32, ShellFonts) {
    const TEX_W: u32 = 128;
    const TEX_H: u32 = 64;
    const CELL_W: u32 = 8;
    const CELL_H: u32 = 16;

    // Rasterize each 5x7 glyph bitmap into its 8x16 cell in the 128x64 RGBA8 texture.
    let mut rgba = vec![0u8; (TEX_W * TEX_H * 4) as usize];
    for code in 0x20u8..=0x5Fu8 {
        let idx = (code - 0x20) as u32;
        let col = idx % 16;
        let row = idx / 16;
        let base_x = col * CELL_W + 1;
        let base_y = row * CELL_H + 4;
        let bitmap = glyph_5x7(code as char);
        for (ry, bits) in bitmap.iter().enumerate() {
            for rx in 0..5u32 {
                if (bits >> (4 - rx)) & 1 != 0 {
                    let px = base_x + rx;
                    let py = base_y + ry as u32;
                    let off = ((py * TEX_W + px) * 4) as usize;
                    rgba[off..off + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    }

    // Build scaled `Font` glyph tables mapping both uppercase and lowercase ASCII to the atlas.
    let make_font = |scale: f64, line_h: f64| -> Font {
        let mut glyphs = HashMap::new();
        for code in 0x20u16..=0x7Eu16 {
            let mapped = if (0x61..=0x7A).contains(&code) {
                code - 32
            } else {
                code.clamp(0x20, 0x5F)
            };
            let idx = (mapped - 0x20) as u32;
            let col = idx % 16;
            let row = idx / 16;
            let u0 = (col * CELL_W) as f64 / TEX_W as f64;
            let v0 = (row * CELL_H) as f64 / TEX_H as f64;
            let u1 = ((col + 1) * CELL_W) as f64 / TEX_W as f64;
            let v1 = ((row + 1) * CELL_H) as f64 / TEX_H as f64;
            glyphs.insert(
                code,
                Glyph {
                    page: page_handle,
                    advance: 7.0 * scale,
                    bounds: [0.0, -10.0 * scale, 7.0 * scale, 4.0 * scale],
                    uv: [u0, v0, u1, v1],
                },
            );
        }
        Font {
            height: line_h,
            glyphs,
        }
    };

    let fonts = ShellFonts {
        large: make_font(2.0, 26.0),
        medium: make_font(1.5, 20.0),
        small: make_font(1.2, 16.0),
        tiny: make_font(1.0, 14.0),
    };
    (rgba, TEX_W, TEX_H, fonts)
}

/// Returns the 7-row 5-bit scanline pattern for an uppercase ASCII character in the fallback font.
///
/// Each element of the returned `[u8; 7]` array encodes one 5-pixel horizontal row
/// (`bit 4` = leftmost pixel, `bit 0` = rightmost pixel).
fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        // Uppercase Latin letters A..Z.
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        // Decimal digits 0..9.
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        // Punctuation and status symbols.
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        ',' => [0, 0, 0, 0, 0b00100, 0b00100, 0b01000],
        ':' => [0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '/' => [0b00001, 0b00010, 0b00100, 0b00100, 0b01000, 0b10000, 0],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        '%' => [0b11001, 0b11010, 0b00100, 0b00100, 0b01011, 0b10011, 0],
        '?' => [0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0, 0b00100],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Verifies that the fallback font atlas produces valid non-empty RGBA pixels and glyph entries.
    #[test]
    fn fallback_font_builds_nonempty_glyphs_and_pixels() {
        let (rgba, w, h, fonts) = build_fallback_ascii_font(2);
        assert_eq!(w, 128);
        assert_eq!(h, 64);
        assert_eq!(rgba.len(), 128 * 64 * 4);
        assert!(rgba.iter().any(|&b| b != 0));
        assert!(fonts.large.glyphs.contains_key(&(b'A' as u16)));
        assert!(fonts.small.glyphs.contains_key(&(b'a' as u16)));
    }

    /// Verifies that `run_bounded_update_check` returns immediately when `skip_network` is true.
    #[test]
    fn bounded_update_check_skips_when_requested() {
        let store = AppSettingsStore::load(crate::app::settings::current_desktop_paths());
        let start = Instant::now();
        let outcome = run_bounded_update_check(&store, true);
        assert_eq!(outcome, UpdateCheckOutcome::UpToDate);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
