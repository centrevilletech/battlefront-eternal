//! Steam client detection, process liveness checks, `steam://rungameid/<gameId>`
//! relaunch decision logic, and Steam Overlay environment helpers.
//!
//! Mirrors `electron/steam/client.js` (`FLATPAK_ID`, `rootKind`, `dotSteam`,
//! `alive`, `runningSteam`, `steamCommand`, `steamEnvironment`),
//! `electron/steam/launcher.js` (`startedBySteam`, `steamStays`, `steamLost`,
//! `withoutSteamOverlay`), `electron/steam/overlay.js` (`overlaySupport`,
//! `overlaySwitches`), and `electron/steam/relaunch.js` (`REQUEST_MS`,
//! `BACKOFF_MS`, `relaunchThroughSteam`).
//!
//! ## Steam Integration Overview
//! - **Root classification (`root_kind` & `dot_steam`)**:
//!   Distinguishes native Linux Steam (`~/.steam`), Flatpak Steam
//!   (`~/.var/app/com.valvesoftware.Steam/.steam`), Snap Steam
//!   (`~/snap/steam/common/.steam`), macOS, and Windows.
//! - **Process liveness (`pid_alive` & `is_steam_client_running`)**:
//!   Reads `steam.pid` from the discovered `.steam` and Steam root directories
//!   and checks whether the PID is alive (`kill(pid, 0)` on Unix or
//!   `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` on Windows).
//! - **Overlay environment (`without_steam_overlay` & `prepare_steam_environment`)**:
//!   Either scrubs `gameoverlayrenderer.so` and legacy `ubuntu12_*` paths when
//!   launching a clean subprocess, or injects `SteamAppId`, `SteamGameId`,
//!   `SteamOverlayGameId`, and `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES` when
//!   attaching the Steam Overlay.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discover::{steam_roots, BF1_STEAM_APP};
use crate::steam::shortcut::{
    ensure_steam_shortcut, inspect_steam_shortcut, ResolveTargetOptions, STEAM_ARG,
};

/// Flatpak application ID for the Valve Steam client on Linux.
///
/// Mirrors `FLATPAK_ID` in `electron/steam/client.js`.
pub const FLATPAK_ID: &str = "com.valvesoftware.Steam";

/// How long a `steam-relaunch.json` hand-off request stands before timing out (`20_000` ms).
///
/// Mirrors `REQUEST_MS` in `electron/steam/relaunch.js`.
pub const REQUEST_MS: u64 = 20_000;

/// Cool-down duration (`30` minutes in ms) after a failed Steam relaunch before
/// trying to relaunch through Steam again.
///
/// Mirrors `BACKOFF_MS` in `electron/steam/relaunch.js`.
pub const BACKOFF_MS: u64 = 30 * 60 * 1000;

/// Classifies a Steam installation root path into `"windows"`, `"mac"`, `"flatpak"`,
/// `"snap"`, or `"native"`.
///
/// Mirrors `rootKind` in `electron/steam/client.js`.
pub fn root_kind(root: &Path, platform: &str) -> &'static str {
    // Windows and macOS have single native Steam layouts; Linux distinguishes
    // Flatpak (`/.var/app/com.valvesoftware.Steam/`), Snap (`/snap/steam/`), and native packages.
    if platform == "win32" || platform == "windows" {
        return "windows";
    }
    if platform == "darwin" || platform == "macos" {
        return "mac";
    }
    let s = root.to_string_lossy();
    if s.contains(&format!("/.var/app/{}/", FLATPAK_ID)) {
        return "flatpak";
    }
    if s.contains("/snap/steam/") {
        return "snap";
    }
    "native"
}

/// Returns the `~/.steam` directory holding `registry.vdf` and `steam.pid` for
/// the given Linux Steam installation `kind` (`"flatpak"`, `"snap"`, or `"native"`).
///
/// Mirrors `dotSteam` in `electron/steam/client.js`.
pub fn dot_steam(kind: &str, home: &Path) -> PathBuf {
    // Sandboxed Linux Steam clients keep their `.steam` folder inside their own home prefix.
    match kind {
        "flatpak" => home.join(".var").join("app").join(FLATPAK_ID).join(".steam"),
        "snap" => home.join("snap").join("steam").join("common").join(".steam"),
        _ => home.join(".steam"),
    }
}

/// Checks whether a given process ID (`pid`) is alive on the current host.
///
/// On Unix, sends signal `0` via `libc::kill` and treats both `0` and `EPERM`
/// as alive. On Windows, probes `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`.
///
/// Mirrors `alive` in `electron/steam/client.js` and `electron/steam/relaunch.js`.
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let Ok(raw_pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        if raw_pid <= 0 {
            return false;
        }
        // Signal 0 performs error checking without delivering a signal; EPERM means the
        // process exists under a different user or sandbox namespace.
        // SAFETY: `raw_pid` is verified to be a strictly positive `pid_t` (`> 0`), and
        // passing signal `0` to `libc::kill` performs kernel existence/permission checks
        // without delivering a signal or dereferencing any pointers.
        let rc = unsafe { libc::kill(raw_pid, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: `OpenProcess` takes plain integer arguments (`PROCESS_QUERY_LIMITED_INFORMATION`,
        // `bInheritHandle = 0`, and `pid`) with no pointer inputs. When it returns a non-null
        // process handle, we immediately release that owned handle via `CloseHandle` before returning.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !handle.is_null() {
                CloseHandle(handle);
                true
            } else {
                false
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// Checks whether the local Steam client is currently running by inspecting
/// `<home>/.steam/steam.pid`, sandbox `.steam/steam.pid` locations, and `<root>/steam.pid`.
///
/// Complements `runningSteam` in `electron/steam/client.js`.
pub fn is_steam_client_running(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> bool {
    // Check known `steam.pid` files across native, Flatpak, and Snap Steam directories.
    if platform == "linux" || platform == "darwin" || platform == "macos" {
        let mut pid_files = Vec::new();
        if let Some(h) = home {
            pid_files.push(dot_steam("native", h).join("steam.pid"));
            pid_files.push(h.join(".local/share/Steam/steam.pid"));
            pid_files.push(dot_steam("flatpak", h).join("steam.pid"));
            pid_files.push(dot_steam("snap", h).join("steam.pid"));
        }
        for root in steam_roots(platform, home, env) {
            pid_files.push(root.join("steam.pid"));
        }
        for pid_file in pid_files {
            if let Ok(raw) = fs::read_to_string(&pid_file) {
                if let Ok(pid) = raw.trim().parse::<u32>() {
                    if pid_alive(pid) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Locates the `steam` (or `steam.exe`) executable on the current machine.
///
/// Mirrors the executable lookup in `steamCommand` in `electron/steam/client.js`.
pub fn find_steam_executable(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    // On Windows, look for `steam.exe` inside each discovered Steam installation root.
    if platform == "win32" || platform == "windows" {
        for root in steam_roots(platform, home, env) {
            let exe = root.join("steam.exe");
            if exe.exists() {
                return Some(exe);
            }
        }
        return None;
    }
    // On macOS, check `/Applications/Steam.app` and `~/Applications/Steam.app`.
    if platform == "darwin" || platform == "macos" {
        let mut candidates = vec![PathBuf::from("/Applications/Steam.app/Contents/MacOS/steam_osx")];
        if let Some(h) = home {
            candidates.push(h.join("Applications/Steam.app/Contents/MacOS/steam_osx"));
        }
        return candidates.into_iter().find(|p| p.exists());
    }
    // On Linux, check `<root>/steam.sh` first, then `/usr/bin/steam` and `/usr/games/steam`.
    let mut candidates = Vec::new();
    for root in steam_roots(platform, home, env) {
        candidates.push(root.join("steam.sh"));
    }
    candidates.push(PathBuf::from("/usr/bin/steam"));
    candidates.push(PathBuf::from("/usr/games/steam"));
    candidates.into_iter().find(|p| p.exists())
}

/// Checks whether the current process was started by the Steam client (either via
/// `--steam` on the command line, `SteamGameId` / `SteamAppId` / `SteamOverlayGameId`
/// in the environment, or an injected `gameoverlayrenderer` in `LD_PRELOAD` /
/// `DYLD_INSERT_LIBRARIES`).
///
/// Mirrors `startedBySteam` in `electron/steam/launcher.js`.
pub fn launched_under_steam(env: &BTreeMap<String, String>, argv: &[String]) -> bool {
    // Steam sets `SteamGameId` and `SteamOverlayGameId` when launching a non-Steam shortcut.
    let has_env = env.get("SteamGameId").filter(|s| !s.is_empty()).is_some()
        || env
            .get("SteamAppId")
            .filter(|s| !s.is_empty() && s.as_str() != "0")
            .is_some()
        || env
            .get("SteamOverlayGameId")
            .filter(|s| !s.is_empty())
            .is_some();
    if has_env {
        return true;
    }
    // Also detect Steam's overlay preload or our own `--steam` launch option.
    let ld = env
        .get("LD_PRELOAD")
        .or_else(|| env.get("DYLD_INSERT_LIBRARIES"))
        .map(|s| s.as_str())
        .unwrap_or("");
    if ld.contains("gameoverlayrenderer") {
        return true;
    }
    argv.iter().any(|a| a == STEAM_ARG)
}

/// Removes `gameoverlayrenderer.so`, `ENABLE_VK_LAYER_VALVE_steam_overlay*`, and
/// Steam runtime paths from `LD_PRELOAD` / `LD_LIBRARY_PATH` on Linux when
/// `BFE_STEAM_CLEAN` is not yet set.
///
/// Mirrors `withoutSteamOverlay` in `electron/steam/launcher.js`.
pub fn without_steam_overlay(
    env: &BTreeMap<String, String>,
    platform: &str,
) -> Option<BTreeMap<String, String>> {
    // Only scrub Linux environments that have not already been scrubbed (`BFE_STEAM_CLEAN=1`).
    if platform != "linux" || env.get("BFE_STEAM_CLEAN").filter(|s| !s.is_empty()).is_some() {
        return None;
    }
    let preload = env.get("LD_PRELOAD").map(|s| s.as_str()).unwrap_or("");
    let vk_layers: Vec<String> = env
        .keys()
        .filter(|k| k.starts_with("ENABLE_VK_LAYER_VALVE_steam_overlay"))
        .cloned()
        .collect();
    if !preload.contains("gameoverlayrenderer.so") && vk_layers.is_empty() {
        return None;
    }

    let mut next = env.clone();
    next.insert("BFE_STEAM_CLEAN".to_string(), "1".to_string());

    // Strip 32-bit and 64-bit `gameoverlayrenderer.so` entries from `LD_PRELOAD`.
    let kept_preload: Vec<&str> = preload
        .split([':', ' '])
        .filter(|entry| !entry.is_empty() && !entry.ends_with("gameoverlayrenderer.so"))
        .collect();
    if kept_preload.is_empty() {
        next.remove("LD_PRELOAD");
    } else {
        next.insert("LD_PRELOAD".to_string(), kept_preload.join(":"));
    }

    for k in vk_layers {
        next.remove(&k);
    }

    // Strip Steam's older ubuntu12 / steam-runtime library directories so system Mesa/GTK loads cleanly.
    if let Some(ld_lib) = next.get("LD_LIBRARY_PATH").cloned() {
        let kept_lib: Vec<&str> = ld_lib
            .split(':')
            .filter(|entry| {
                !entry.is_empty()
                    && !entry.contains("/ubuntu12_32")
                    && !entry.contains("/ubuntu12_64")
                    && !entry.contains("/steam-runtime")
            })
            .collect();
        if kept_lib.is_empty() {
            next.remove("LD_LIBRARY_PATH");
        } else {
            next.insert("LD_LIBRARY_PATH".to_string(), kept_lib.join(":"));
        }
    }

    Some(next)
}

/// Locates the Steam Overlay shared library (`gameoverlayrenderer.so` on Linux,
/// `gameoverlayrenderer.dylib` on macOS, `GameOverlayRenderer64.dll` on Windows)
/// inside the user's Steam installation.
pub fn find_overlay_library(
    platform: &str,
    arch: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
    extra_roots: &[PathBuf],
) -> Option<PathBuf> {
    // Search all known Steam installation roots plus any caller-provided roots.
    let mut roots = steam_roots(platform, home, env);
    roots.extend(extra_roots.iter().cloned());

    for root in roots {
        if platform == "linux" {
            let candidates = if arch == "x64" || arch == "x86_64" {
                vec![
                    root.join("ubuntu12_64/gameoverlayrenderer.so"),
                    root.join("linux64/gameoverlayrenderer.so"),
                ]
            } else {
                vec![
                    root.join("ubuntu12_32/gameoverlayrenderer.so"),
                    root.join("linux32/gameoverlayrenderer.so"),
                ]
            };
            if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
                return Some(found);
            }
        } else if platform == "darwin" || platform == "macos" {
            let candidates = [
                root.join("Steam.AppBundle/Steam/Contents/MacOS/gameoverlayrenderer.dylib"),
                PathBuf::from("/Applications/Steam.app/Contents/MacOS/gameoverlayrenderer.dylib"),
            ];
            if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
                return Some(found);
            }
        } else if platform == "win32" || platform == "windows" {
            let candidates = if arch == "x64" || arch == "x86_64" {
                vec![
                    root.join("GameOverlayRenderer64.dll"),
                    root.join("GameOverlayRenderer.dll"),
                ]
            } else {
                vec![root.join("GameOverlayRenderer.dll")]
            };
            if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
                return Some(found);
            }
        }
    }
    None
}

/// Result returned by [`prepare_steam_environment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamEnvironmentResult {
    /// True if the process is running under Steam or had overlay environment variables applied.
    pub under_steam: bool,
    /// Discovered path to the Steam Overlay shared library, if any.
    pub overlay_lib: Option<PathBuf>,
    /// True if `LD_PRELOAD` or `DYLD_INSERT_LIBRARIES` was updated with `overlay_lib`
    /// and the process should re-exec once so the dynamic linker loads the overlay.
    pub needs_reexec_for_preload: bool,
    /// Updated environment variables (`SteamAppId`, `SteamGameId`, `SteamOverlayGameId`, `LD_PRELOAD`, etc.).
    pub env: BTreeMap<String, String>,
}

/// Prepares the process environment for Steam (`SteamAppId`, `SteamGameId`,
/// `SteamOverlayGameId`, and `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`).
pub fn prepare_steam_environment(
    platform: &str,
    arch: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
    argv: &[String],
    shortcut_game_id: Option<&str>,
    extra_roots: &[PathBuf],
) -> SteamEnvironmentResult {
    let mut next_env = env.clone();
    let under_steam = launched_under_steam(&next_env, argv);
    let overlay_lib = find_overlay_library(platform, arch, home, &next_env, extra_roots);

    // Populate SteamAppId / SteamGameId / SteamOverlayGameId when launched under Steam.
    if under_steam {
        if next_env
            .get("SteamAppId")
            .filter(|s| !s.is_empty())
            .is_none()
        {
            next_env.insert("SteamAppId".to_string(), BF1_STEAM_APP.to_string());
        }
        if next_env
            .get("SteamGameId")
            .filter(|s| !s.is_empty())
            .is_none()
        {
            let gid = shortcut_game_id.unwrap_or(BF1_STEAM_APP);
            next_env.insert("SteamGameId".to_string(), gid.to_string());
        }
        if next_env
            .get("SteamOverlayGameId")
            .filter(|s| !s.is_empty())
            .is_none()
        {
            if let Some(gid) = next_env.get("SteamGameId").cloned() {
                next_env.insert("SteamOverlayGameId".to_string(), gid);
            }
        }
    }

    // Prepend the Steam Overlay shared library to `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`
    // when running under Steam and not yet preloaded.
    let mut needs_reexec_for_preload = false;
    if under_steam && next_env.get("BFE_STEAM_REEXEC").map(|s| s.as_str()) != Some("1") {
        if let Some(lib) = &overlay_lib {
            let lib_str = lib.to_string_lossy().into_owned();
            if platform == "linux" {
                let current_preload = next_env.get("LD_PRELOAD").cloned().unwrap_or_default();
                if !current_preload.contains("gameoverlayrenderer.so") {
                    let updated = if current_preload.is_empty() {
                        lib_str
                    } else {
                        format!("{}:{}", lib_str, current_preload)
                    };
                    next_env.insert("LD_PRELOAD".to_string(), updated);
                    needs_reexec_for_preload = true;
                }
            } else if platform == "darwin" || platform == "macos" {
                let current_dyld = next_env
                    .get("DYLD_INSERT_LIBRARIES")
                    .cloned()
                    .unwrap_or_default();
                if !current_dyld.contains("gameoverlayrenderer.dylib") {
                    let updated = if current_dyld.is_empty() {
                        lib_str
                    } else {
                        format!("{}:{}", lib_str, current_dyld)
                    };
                    next_env.insert("DYLD_INSERT_LIBRARIES".to_string(), updated);
                    needs_reexec_for_preload = true;
                }
            }
        }
    }

    SteamEnvironmentResult {
        under_steam,
        overlay_lib,
        needs_reexec_for_preload,
        env: next_env,
    }
}

/// Decision returned by [`decide_steam_relaunch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamRelaunchDecision {
    /// Whether the process should hand off to `steam://rungameid/<gameId>` and exit.
    pub relaunch: bool,
    /// Machine-readable reason (`"disabled"`, `"already-under-steam"`, `"relaunch-via-steam"`, etc.).
    pub reason: String,
    /// Target 64-bit Steam GameID when `relaunch` is true.
    pub game_id: Option<String>,
    /// Target `steam://rungameid/<gameId>` URI when `relaunch` is true.
    pub uri: Option<String>,
}

/// Evaluates whether a packaged desktop launch should re-invoke itself through
/// `steam://rungameid/<gameId>` so Steam shows friends `"playing Battlefront Eternal"`.
///
/// Mirrors the relaunch conditions in `relaunchThroughSteam` and `steamEntry`
/// (`electron/steam/relaunch.js`).
pub fn decide_steam_relaunch(
    enabled: bool,
    packaged: bool,
    target_opts: &ResolveTargetOptions,
    home: Option<&Path>,
    argv: &[String],
    steam_running_override: Option<bool>,
) -> SteamRelaunchDecision {
    // Do not relaunch when disabled in settings or already launched under Steam.
    if !enabled {
        return SteamRelaunchDecision {
            relaunch: false,
            reason: "disabled".to_string(),
            game_id: None,
            uri: None,
        };
    }
    if launched_under_steam(&target_opts.env, argv) {
        return SteamRelaunchDecision {
            relaunch: false,
            reason: "already-under-steam".to_string(),
            game_id: None,
            uri: None,
        };
    }
    // Respect explicit skip flags (`BFE_SKIP_STEAM_RELAUNCH=1`, `BFE_NO_STEAM`, `--no-steam-relaunch`).
    if target_opts.env.get("BFE_SKIP_STEAM_RELAUNCH").map(|s| s.as_str()) == Some("1")
        || target_opts.env.get("BFE_NO_STEAM").filter(|s| !s.is_empty()).is_some()
        || argv.iter().any(|a| a == "--no-steam-relaunch")
    {
        return SteamRelaunchDecision {
            relaunch: false,
            reason: "skipped".to_string(),
            game_id: None,
            uri: None,
        };
    }
    // Development launches stay in the current terminal unless `BFE_FORCE_STEAM_RELAUNCH=1`.
    if !packaged && target_opts.env.get("BFE_FORCE_STEAM_RELAUNCH").map(|s| s.as_str()) != Some("1")
    {
        return SteamRelaunchDecision {
            relaunch: false,
            reason: "unpackaged".to_string(),
            game_id: None,
            uri: None,
        };
    }

    let running = steam_running_override.unwrap_or_else(|| {
        is_steam_client_running(&target_opts.platform, home, &target_opts.env)
    });
    if !running {
        return SteamRelaunchDecision {
            relaunch: false,
            reason: "steam-not-running".to_string(),
            game_id: None,
            uri: None,
        };
    }

    // Ensure the non-Steam shortcut exists in `shortcuts.vdf` before asking Steam to run it.
    let mut status = inspect_steam_shortcut(target_opts, home, &[]);
    if !status.registered {
        let ensured = ensure_steam_shortcut(target_opts, home, &[]);
        if !ensured.ok || !ensured.updated_files.is_empty() {
            return SteamRelaunchDecision {
                relaunch: false,
                reason: "shortcut-needs-steam-restart".to_string(),
                game_id: Some(ensured.game_id),
                uri: None,
            };
        }
        status = inspect_steam_shortcut(target_opts, home, &[]);
    }

    if !status.registered {
        return SteamRelaunchDecision {
            relaunch: false,
            reason: "shortcut-not-registered".to_string(),
            game_id: None,
            uri: None,
        };
    }

    // Build the `steam://rungameid/<gameId>` URI for the registered Non-Steam shortcut.
    let uri = format!("steam://rungameid/{}", status.game_id);
    SteamRelaunchDecision {
        relaunch: true,
        reason: "relaunch-via-steam".to_string(),
        game_id: Some(status.game_id),
        uri: Some(uri),
    }
}
