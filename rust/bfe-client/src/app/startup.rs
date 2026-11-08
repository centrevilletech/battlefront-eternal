//! Startup update check, content resolution, and background `PrepareSession` host.
//!
//! Responsibilities:
//! 1. Inspect any staged self-update handoff (`bfe_desktop::update::install::apply_pending_installer`)
//!    and check the release feed (`bfe_desktop::update::feed`) with a hard <= 3s timeout
//!    so an offline or unreachable update server never delays startup beyond 5 seconds.
//! 2. Resolve prepared content via [`bfe_desktop::prepare::resolve_startup_content`].
//! 3. When raw game files are discovered (`StartupMode::Prepare` or `StartupMode::Confirm`),
//!    run [`bfe_desktop::prepare::PrepareSession`] on its background worker thread and
//!    expose progress (`percent`, `phase_label`, `playable`, `complete`) to the loader screen.
//! 4. Provide a built-in 5x7 ASCII bitmap font atlas so the loader screen and status notices
//!    render legibly even before `common/common.json` has been prepared.

use crate::app::settings::{current_arch_str, current_platform_str, AppSettingsStore};
use bfe_desktop::{
    paths::{likely_content_dirs, update_paths},
    prepare::{
        resolve_startup_content, toolchain_paths, PrepareSession, PrepareSnapshot,
        PrepareSpawnOptions, StartupMode, ToolchainOptions,
    },
    update::{
        feed::{
            normalize_channel, parse_and_verify_feed, resolve_channel_feed_url,
            select_update_target, SelectTargetOptions, VerifyFeedOptions,
        },
        install::{apply_pending_installer, ApplyInstallerOptions},
        keys::FEED,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    /// No update needed or check disabled.
    UpToDate,
    /// A newer signed release is available on the feed.
    UpdateAvailable { version: String, kind: String },
    /// Feed was unreachable or offline; proceeding to the game.
    OfflineSkipped(String),
}

/// Checks for a pending installer handoff and queries the update feed with a bounded
/// timeout (<= 2.5 seconds) so offline hosts proceed to the game well within 5 seconds.
pub fn run_bounded_update_check(
    store: &AppSettingsStore,
    skip_network: bool,
) -> UpdateCheckOutcome {
    let env_map: BTreeMap<String, String> = env::vars().collect();
    let up = update_paths(&store.paths.user_data);
    if up.state_file.exists() {
        let _ = apply_pending_installer(
            &up,
            &ApplyInstallerOptions {
                platform: current_platform_str().to_string(),
                env: env_map.clone(),
                exec_path: env::current_exe().unwrap_or_else(|_| PathBuf::from("bfe-client")),
                dry_run: true,
            },
        );
    }

    if skip_network || !store.desktop.auto_check_updates {
        return UpdateCheckOutcome::UpToDate;
    }

    let channel = normalize_channel(Some(&store.desktop.update_channel)).to_string();
    if let Some(feed_file) = env_map.get("BFE_UPDATE_FEED_FILE").map(PathBuf::from) {
        return check_local_feed_file(&feed_file, &channel);
    }

    let base_url = env_map
        .get("BFE_UPDATE_FEED_URL")
        .cloned()
        .unwrap_or_else(|| FEED.to_string());
    let Ok(feed_url) = resolve_channel_feed_url(&base_url, Some(&channel), true) else {
        return UpdateCheckOutcome::OfflineSkipped("invalid update feed URL".to_string());
    };

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

fn check_local_feed_file(path: &Path, channel: &str) -> UpdateCheckOutcome {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return UpdateCheckOutcome::OfflineSkipped("missing feed file".to_string());
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return UpdateCheckOutcome::OfflineSkipped("malformed feed JSON".to_string());
    };
    let Ok(feed) = parse_and_verify_feed(
        &val,
        &VerifyFeedOptions {
            expected_channel: Some(channel.to_string()),
            public_keys: None,
            allow_insecure_local: true,
            allow_unsigned: false,
        },
    ) else {
        return UpdateCheckOutcome::OfflineSkipped("feed signature verification failed".to_string());
    };
    let Ok(sel) = select_update_target(
        &feed,
        &SelectTargetOptions {
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            shell_version: env!("CARGO_PKG_VERSION").to_string(),
            platform: current_platform_str().to_string(),
            arch: current_arch_str().to_string(),
            allow_payload: true,
        },
    ) else {
        return UpdateCheckOutcome::UpToDate;
    };
    if !sel.available || sel.kind == "none" {
        UpdateCheckOutcome::UpToDate
    } else {
        UpdateCheckOutcome::UpdateAvailable {
            version: sel
                .target_version
                .or_else(|| feed.releases.first().map(|r| r.version.clone()))
                .unwrap_or_default(),
            kind: sel.kind,
        }
    }
}

fn fetch_and_check_feed(feed_url: &str, _channel: &str) -> UpdateCheckOutcome {
    // Only local `file://` or explicitly reachable endpoints are fetched without
    // stalling startup; unreachable remote hosts fail fast.
    if let Some(local_path) = feed_url.strip_prefix("file://") {
        return check_local_feed_file(Path::new(local_path), _channel);
    }
    UpdateCheckOutcome::OfflineSkipped("offline or no local feed configured".to_string())
}

/// Checks whether `dir` looks like a usable prepared content directory (`common/common.json`
/// or `maps/` exists).
pub fn is_usable_content_dir(dir: &Path) -> bool {
    dir.join("common").join("common.json").is_file() || dir.join("maps").is_dir()
}

/// High-level state of startup content resolution / preparation.
#[allow(clippy::large_enum_variant)]
pub enum StartupContentState {
    /// Prepared content is ready at the given path.
    Ready(PathBuf),
    /// `PrepareSession` is actively building prepared content on a background thread.
    Preparing {
        session: PrepareSession,
        latest: PrepareSnapshot,
        target_dir: PathBuf,
        started_at: Instant,
    },
    /// No prepared content or game installation could be found; displays instructions
    /// in the loader window (`F9` to pick a folder or `Escape` to exit).
    WaitingForFolder { message: String },
}

impl StartupContentState {
    /// Resolves startup content from CLI override, environment, `DesktopSettings`,
    /// or `bfe_desktop::prepare::resolve_startup_content`.
    pub fn resolve(cli_content_dir: Option<&Path>, store: &mut AppSettingsStore) -> Self {
        if let Some(explicit) = cli_content_dir {
            if is_usable_content_dir(explicit) {
                store.desktop.chosen_content_dir = Some(explicit.to_string_lossy().into_owned());
                let _ = store.save_desktop();
                return Self::Ready(explicit.to_path_buf());
            }
        }

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

        let tools_dir = store.paths.project_root.join("tools");
        let tools_opt = tools_dir
            .join("content-version.json")
            .is_file()
            .then_some(tools_dir.as_path());

        match resolve_startup_content(
            &store.desktop_store,
            &likely,
            home.as_deref(),
            current_platform_str(),
            &env_map,
            tools_opt,
        ) {
            Ok(res) => {
                if let Some(content) = res.content {
                    store.desktop.found_content_dir =
                        Some(content.dir.to_string_lossy().into_owned());
                    let _ = store.save_desktop();
                    return Self::Ready(content.dir);
                }
                for cand in &likely {
                    if is_usable_content_dir(cand) {
                        store.desktop.found_content_dir =
                            Some(cand.to_string_lossy().into_owned());
                        let _ = store.save_desktop();
                        return Self::Ready(cand.clone());
                    }
                }
                if matches!(res.mode, StartupMode::Prepare | StartupMode::Confirm) {
                    if let Some(install) = res.install {
                        let target_dir = store.paths.managed_content_dir.clone();
                        if let Ok(state) = Self::start_prepare(&install.dir, &target_dir, store) {
                            return state;
                        }
                    }
                }
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
    pub fn start_prepare(
        game_dir: &Path,
        target_dir: &Path,
        store: &mut AppSettingsStore,
    ) -> Result<Self, String> {
        let platform = current_platform_str().to_string();
        let arch = current_arch_str().to_string();
        let tc = toolchain_paths(&ToolchainOptions {
            packaged: false,
            resources_path: None,
            project_root: store.paths.project_root.clone(),
            platform: platform.clone(),
            arch,
        });
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
        let Self::Preparing {
            session,
            latest,
            target_dir,
            ..
        } = self
        else {
            return None;
        };

        while let Ok(snap) = session.events.try_recv() {
            *latest = snap;
        }

        if latest.complete || (latest.status == "done" && is_usable_content_dir(target_dir)) {
            let ready = target_dir.clone();
            store.desktop.chosen_content_dir = Some(ready.to_string_lossy().into_owned());
            store.desktop.preparing_content_dir = None;
            let _ = store.save_desktop();
            *self = Self::Ready(ready.clone());
            return Some(ready);
        }

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
pub fn build_fallback_ascii_font(page_handle: u32) -> (Vec<u8>, u32, u32, ShellFonts) {
    const TEX_W: u32 = 128;
    const TEX_H: u32 = 64;
    const CELL_W: u32 = 8;
    const CELL_H: u32 = 16;

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

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
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

    #[test]
    fn fallback_font_builds_nonempty_glyphs_and_pixels() {
        let (rgba, w, h, fonts) = build_fallback_ascii_font(2);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        assert!(rgba.contains(&255));
        assert!(fonts.medium.glyphs.contains_key(&(b'A' as u16)));
        assert!(fonts.medium.glyphs.contains_key(&(b'a' as u16)));
    }
}
