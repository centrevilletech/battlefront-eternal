//! CLI tool (`desktop-tool`) for exercising `bfe-desktop` paths, settings,
//! content preparation, update feed verification, dry-run installer handoff,
//! rich presence formatting, and detached local server hosting (`HostController`).
//!
//! ## Subcommands
//! - `desktop-tool paths`
//!   Resolves and prints the platform-specific `user_data`, `settings_file`,
//!   `player_settings_file`, and `default_prepared_dir` paths as formatted JSON.
//! - `desktop-tool settings get <KEY>`
//!   Reads `<KEY>` from `settings.json` (`SettingsStore`), falling back to
//!   `player-settings.json` (`PlayerSettingsStore`) or printing `null`.
//! - `desktop-tool prepare --game <DIR> --content <DIR> [--plan-only]`
//!   Resolves the Python + `ffmpeg` toolchain, runs `tools/prepare_content.py`,
//!   streams NDJSON progress snapshots to stdout, and verifies the resulting
//!   `.prepare.json` against `tools/content-version.json`.
//! - `desktop-tool update check [--channel <stable|nightly>] [--feed-file <FILE>]`
//!   Resolves the release channel feed URL or verifies a signed feed JSON file
//!   and prints the selected update action (`none`, `payload`, or `installer`).
//! - `desktop-tool update install --dry-run [--state-dir <DIR>]`
//!   Validates a pending full installer in `update-state.json` and prints the
//!   planned platform handoff action without replacing the running binary.
//! - `desktop-tool presence test`
//!   Normalizes a sample multiplayer battle presence report, formats both the
//!   Discord `SET_ACTIVITY` payload and Steam Friends rich-presence pairs, and
//!   attempts a non-fatal local Discord IPC publish.
//! - `desktop-tool host <start|status|stop|attach|join|restart|kick|ban|unban> [flags]`
//!   Starts, queries, attaches to, or stops a detached `bfeserver` instance via
//!   [`HostController`](bfe_desktop::host::HostController), porting the desktop host
//!   and the keep-running close workflow from the desktop host.
//!   Supports `--state-dir`, `--content-dir`, `--server-bin`, `--admin-port`, `--game-port`,
//!   `--name`, `--mode <lan|public>`, `--map`, `--side`, `--max-players`, and `--relay`.
//!
//! ## Exit Codes
//! - `0`: Subcommand completed cleanly and printed its JSON or NDJSON output.
//! - `1`: Runtime validation, file I/O, or subprocess error.
//! - `2`: Missing command-line subcommand.

#![deny(warnings, clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bfe_desktop::host::{
    resolve_admin_port, resolve_game_port, resolve_server_executable, AiDifficulty, CloseChoice,
    HostController, HostControllerConfig, HostMode, HostOptions, RelayPolicy, TeamAssignment,
};
use bfe_desktop::paths::{resolve_desktop_paths, DesktopPathsOptions};
use bfe_desktop::prepare::{
    checkout_content_version, read_prepare_state, toolchain_paths, verify_prepared_outputs,
    PrepareSession, PrepareSpawnOptions, ToolchainOptions,
};
use bfe_desktop::presence::activity::{
    format_discord_activity, format_steam_rich_presence, normalize_presence_state,
    RawPresenceInput, DISCORD_APPLICATION_ID,
};
use bfe_desktop::presence::discord::try_publish_discord_activity;
use bfe_desktop::settings::{PlayerSettingsStore, SettingsStore};
use bfe_desktop::update::feed::{join_url, normalize_channel, verify_envelope};
use bfe_desktop::update::keys::{default_keys, FEED};
use bfe_desktop::update::state::{choose_launch, installed_versions, platform_key, read_state, state_file_path};
use bfe_desktop::update::version::is_newer;

/// Returns the platform string matching platform conventions (`"linux"`, `"darwin"`, `"win32"`).
fn current_platform_str() -> &'static str {
    // Map Rust target_os values to the platform strings expected by `bfe-desktop`.
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// Returns the CPU architecture string matching platform conventions (`"x64"`, `"arm64"`).
fn current_arch_str() -> &'static str {
    // Map Rust target_arch values to the architecture strings expected by `bfe-desktop`.
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Collects the current process environment into a sorted `BTreeMap`.
///
/// Using a `BTreeMap` keeps environment lookups deterministic across subcommands.
fn collect_env_map() -> BTreeMap<String, String> {
    // Collect process environment key/value pairs into an ordered map.
    env::vars().collect()
}

/// Resolves the repository or packaged root directory relative to the running binary
/// or current working directory.
///
/// Walks from the current working directory up to its parent looking for
/// `tools/content-version.json` so invoking `cargo run` from either the repository
/// root or `rust/` finds the shared tools directory.
fn resolve_project_root() -> PathBuf {
    // Check whether the current working directory or its parent contains `tools/content-version.json`.
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("tools").join("content-version.json").exists() {
        return cwd;
    }
    if let Some(parent) = cwd.parent() {
        if parent.join("tools").join("content-version.json").exists() {
            return parent.to_path_buf();
        }
    }
    cwd
}

/// Executes `desktop-tool paths` and prints resolved platform directories as JSON.
///
/// Honors `BFE_USER_DATA_DIR`, `XDG_CONFIG_HOME`, `APPDATA`, `HOME`, and `USERPROFILE`
/// via [`resolve_desktop_paths`].
fn cmd_paths() -> Result<(), String> {
    // Gather environment variables and resolve `user_data`, `settings_file`, `player_settings_file`,
    // and `default_prepared_dir` for the current host platform.
    let env_map = collect_env_map();
    let home = env_map
        .get("HOME")
        .or_else(|| env_map.get("USERPROFILE"))
        .map(PathBuf::from);
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = resolve_project_root();
    let dp = resolve_desktop_paths(&DesktopPathsOptions {
        platform: current_platform_str().to_string(),
        home,
        cwd,
        project_root,
        exec_dir: env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
        resources_path: None,
        packaged: false,
        env: env_map,
    });
    // Serialize the resolved DesktopPaths struct as pretty-printed JSON.
    let json = serde_json::to_string_pretty(&dp).map_err(|e| e.to_string())?;
    println!("{}", json);
    Ok(())
}

/// Executes `desktop-tool settings get <KEY>` across `settings.json` and `player-settings.json`.
///
/// Checks `settings.json` (`installDir`, `contentDir`, `prepared`, `updateChannel`) first,
/// then falls back to `player-settings.json` (`settings`, `keybindings`, `audioOutputDeviceId`,
/// `seenTutorialVersion`). Prints `null` when the key is unset in both files.
fn cmd_settings_get(args: &[String]) -> Result<(), String> {
    // Require a setting key name on the command line.
    let key = args
        .first()
        .ok_or_else(|| "Usage: desktop-tool settings get <KEY>".to_string())?;
    let env_map = collect_env_map();
    let home = env_map
        .get("HOME")
        .or_else(|| env_map.get("USERPROFILE"))
        .map(PathBuf::from);
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = resolve_project_root();
    let dp = resolve_desktop_paths(&DesktopPathsOptions {
        platform: current_platform_str().to_string(),
        home,
        cwd,
        project_root,
        exec_dir: None,
        resources_path: None,
        packaged: false,
        env: env_map,
    });

    // Check `settings.json` first (`installDir`, `contentDir`, `prepared`, etc.).
    let desktop_store = SettingsStore::new(&dp.settings_file);
    let desktop_val = serde_json::to_value(desktop_store.load()).map_err(|e| e.to_string())?;
    if let Some(val) = desktop_val.get(key).filter(|v| !v.is_null()) {
        println!("{}", serde_json::to_string(val).unwrap_or_default());
        return Ok(());
    }

    // Fall back to `player-settings.json` (`settings`, `keybindings`, `audioOutputDeviceId`, etc.).
    let player_store = PlayerSettingsStore::new(&dp.player_settings_file);
    let player_val = serde_json::to_value(player_store.load()).map_err(|e| e.to_string())?;
    if let Some(val) = player_val.get(key) {
        println!("{}", serde_json::to_string(val).unwrap_or_default());
        return Ok(());
    }

    // Print JSON `null` when the requested key is not present in either store.
    println!("null");
    Ok(())
}

/// Executes `desktop-tool prepare --game <DIR> --content <DIR> [--plan-only]`.
///
/// Spawns `tools/prepare_content.py` via [`PrepareSession::start`], emits each progress
/// snapshot as a single-line JSON object to stdout, and validates `.prepare.json` against
/// `tools/content-version.json` when performing a full preparation pass.
fn cmd_prepare(args: &[String]) -> Result<(), String> {
    // Parse `--game <DIR>`, `--content <DIR>`, and optional `--plan-only` flags.
    let mut game_dir: Option<PathBuf> = None;
    let mut content_dir: Option<PathBuf> = None;
    let mut plan_only = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game" => {
                i += 1;
                game_dir = args.get(i).map(PathBuf::from);
            }
            "--content" => {
                i += 1;
                content_dir = args.get(i).map(PathBuf::from);
            }
            "--plan-only" => {
                plan_only = true;
            }
            _ => {}
        }
        i += 1;
    }

    // Both `--game` and `--content` are mandatory for running `tools/prepare_content.py`.
    let game = game_dir.ok_or_else(|| "Missing required --game <DIR>".to_string())?;
    let content = content_dir.ok_or_else(|| "Missing required --content <DIR>".to_string())?;
    let project_root = resolve_project_root();
    let platform = current_platform_str().to_string();
    let arch = current_arch_str().to_string();

    // Resolve the Python interpreter, `tools/prepare_content.py`, and `ffmpeg` directories.
    let tc = toolchain_paths(&ToolchainOptions {
        packaged: false,
        resources_path: None,
        project_root,
        platform: platform.clone(),
        arch,
    });

    // Spawn the preparation subprocess and stream NDJSON progress snapshots to stdout.
    let session = PrepareSession::start(PrepareSpawnOptions {
        toolchain: tc.clone(),
        game_dir: game.clone(),
        content_dir: content.clone(),
        env: collect_env_map(),
        platform: platform.clone(),
        include_add_ons: true,
        enhanced_sound: true,
        plan_only,
    })?;

    // Forward each progress snapshot as a single-line JSON object until the session finishes.
    while let Ok(snap) = session.events.recv() {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "status": snap.status,
                "done": snap.done,
                "total": snap.total,
                "percent": snap.percent,
                "phaseLabel": snap.phase_label,
                "etaSeconds": snap.eta_seconds,
                "playable": snap.playable,
                "complete": snap.complete,
            }))
            .unwrap_or_default()
        );
        // Stop reading once the subprocess reaches a terminal status (`done`, `failed`, or `cancelled`).
        if snap.status == "done" || snap.status == "failed" || snap.status == "cancelled" {
            break;
        }
    }

    // Verify the prepared output directory against `tools/content-version.json` when running a full prepare.
    if !plan_only {
        let state = read_prepare_state(Some(&content));
        let verified = verify_prepared_outputs(&content, &game, &platform, Some(&tc.tools))?;
        if !verified && state.and_then(|s| s.complete) != Some(true) {
            return Err("Content preparation did not complete cleanly.".to_string());
        }
    }
    Ok(())
}

/// Executes `desktop-tool update check [--channel <stable|nightly>] [--feed-file <FILE>]`.
///
/// When `--feed-file` (or `BFE_UPDATE_FEED_FILE`) is provided, parses and verifies the
/// Ed25519 signature on the release feed and prints the [`UpdateSelection`](bfe_desktop::update::feed::UpdateSelection).
/// Otherwise prints the resolved feed URL, shell version, and `tools/content-version.json` generations.
fn cmd_update_check(args: &[String]) -> Result<(), String> {
    // Parse `--channel` and optional `--feed-file` arguments.
    let mut channel = "stable".to_string();
    let mut feed_file: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--channel" => {
                i += 1;
                if let Some(c) = args.get(i) {
                    channel = normalize_channel(Some(c)).to_string();
                }
            }
            "--feed-file" => {
                i += 1;
                feed_file = args.get(i).map(PathBuf::from);
            }
            _ => {}
        }
        i += 1;
    }

    // Resolve the release channel feed endpoint URL from `BFE_UPDATE_FEED` / `BFE_UPDATE_FEED_URL` or `FEED`.
    let env_map = collect_env_map();
    let base_url = env_map
        .get("BFE_UPDATE_FEED")
        .or_else(|| env_map.get("BFE_UPDATE_FEED_URL"))
        .cloned()
        .unwrap_or_else(|| FEED.to_string());
    let feed_url = join_url(&base_url, &format!("updates/{channel}.json"))?;

    // When a local channel envelope file is supplied, verify its Ed25519 signature and report release metadata.
    if let Some(path) = feed_file.or_else(|| env_map.get("BFE_UPDATE_FEED_FILE").map(PathBuf::from))
    {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("Failed reading {}: {}", path.display(), e))?;
        let val: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON: {}", e))?;
        let payload = verify_envelope(&val, &default_keys())?;
        let plat = platform_key(current_platform_str(), current_arch_str());
        let has_platform = plat
            .map(|p| payload.release.platforms.contains_key(p))
            .unwrap_or(false);
        let available = is_newer(&payload.release.version, env!("CARGO_PKG_VERSION")) && has_platform;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "available": available,
                "channel": payload.channel,
                "version": payload.release.version,
                "releaseId": payload.release.id,
                "platform": plat,
            }))
            .unwrap_or_default()
        );
        return Ok(());
    }

    // Otherwise report the active channel, resolved feed URL, shell version, and content version.
    let content_ver = checkout_content_version()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "channel": channel,
            "feedUrl": feed_url,
            "shellVersion": env!("CARGO_PKG_VERSION"),
            "contentVersion": content_ver,
        }))
        .unwrap_or_default()
    );
    Ok(())
}

/// Executes `desktop-tool update install --dry-run [--state-dir <DIR>]`.
///
/// Inspects `<root>/state.json` and `<root>/versions/` in dry-run mode without spawning
/// a child process.
fn cmd_update_install(args: &[String]) -> Result<(), String> {
    // Require `--dry-run` so the CLI never replaces the running executable unintentionally.
    let dry_run = args.iter().any(|a| a == "--dry-run");
    if !dry_run {
        return Err("Refusing to run installer handoff without --dry-run.".to_string());
    }
    let mut state_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--state-dir" {
            i += 1;
            state_dir = args.get(i).map(PathBuf::from);
        }
        i += 1;
    }

    let env_map = collect_env_map();
    let root = state_dir.unwrap_or_else(|| {
        let home = env_map
            .get("HOME")
            .or_else(|| env_map.get("USERPROFILE"))
            .map(PathBuf::from);
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let dp = resolve_desktop_paths(&DesktopPathsOptions {
            platform: current_platform_str().to_string(),
            home,
            cwd,
            project_root: resolve_project_root(),
            exec_dir: None,
            resources_path: None,
            packaged: false,
            env: env_map.clone(),
        });
        dp.update_root.unwrap_or(dp.user_data)
    });

    let plat = platform_key(current_platform_str(), current_arch_str());
    let state = read_state(&root);
    let versions: Vec<String> = installed_versions(&root, plat)
        .into_iter()
        .map(|v| v.version)
        .collect();
    let candidate = choose_launch(&root, env!("CARGO_PKG_VERSION"), plat, None).map(|v| v.version);

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "action": "dry-run",
            "stateFile": state_file_path(&root),
            "installedVersions": versions,
            "launchCandidate": candidate,
            "healthy": state.healthy,
            "bad": state.bad,
        }))
        .unwrap_or_default()
    );
    Ok(())
}

/// Executes `desktop-tool presence test`.
///
/// Normalizes a synthetic multiplayer match on Cloud City (`bes2a`), formats both
/// Discord and Steam rich presence payloads, attempts a non-fatal Discord IPC publish,
/// and prints the resulting JSON bundle.
fn cmd_presence_test() -> Result<(), String> {
    // Normalize a sample multiplayer match state and format both Discord and Steam payloads.
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
    let discord = format_discord_activity(&state);
    let steam = format_steam_rich_presence(&state);
    let env_map = collect_env_map();
    // Attempt a non-fatal Discord IPC publish if a local Discord socket is present.
    let ipc_socket = try_publish_discord_activity(
        DISCORD_APPLICATION_ID,
        Some(&discord),
        current_platform_str(),
        &env_map,
    )
    .ok();

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "normalized": state,
            "discordActivity": discord,
            "steamRichPresence": steam,
            "connectedDiscordSocket": ipc_socket,
        }))
        .unwrap_or_default()
    );
    Ok(())
}

/// Executes `desktop-tool host <start|status|stop|attach|join|restart|kick|ban|unban|close-prompt|keep-running> [flags]`.
///
/// Exercises [`HostController`](bfe_desktop::host::HostController) (`createHostController` in
/// the desktop host) and the close-decision flow (`before-quit` in the desktop host).
///
/// ## Supported Actions
/// - `start`: writes `<state>/config.json` (mode `0600`), rotates `<state>/hosting.log` (mode `0600`)
///   when larger than 8 MiB, spawns `bfeserver --serve` detached (`process_group(0)` on Unix;
///   `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW` on Windows; or `systemd-run --user`
///   via `start_outside` when launched under native Linux Steam), waits up to 60 s for `/info`,
///   and prints the `HostJoinInfo` JSON. If a server is already active, attaches to it instead.
/// - `status`: queries `POST http://127.0.0.1:<admin_port>/admin` (`{"action": "status"}`) with
///   `<state>/operator-token` and `GET http://127.0.0.1:<game_port>/info`, printing `HostStatus`.
/// - `stop`: sends `POST /admin {"action": "stop"}` with `<state>/operator-token`, removes
///   `<state>/attach.json`, and prints `{"ok": true}`.
/// - `attach`: probes `<state>/operator-token`, `/admin`, and `/info` to adopt an existing
///   server left running by a previous client session.
/// - `join` / `restart` / `kick` / `ban` / `unban` / `close-prompt` / `keep-running`:
///   exercises join info lookup, match restart, player moderation, and the close confirmation prompt.
///
/// Resolves `<userData>/server` (`stateRoot` in the desktop host) and `content_root` from
/// [`resolve_desktop_paths`], allowing CLI flags (`--state-dir`, `--content-dir`, `--server-bin`,
/// `--admin-port`, `--game-port`, `--name`, `--mode`, `--map`, `--side`, `--max-players`,
/// `--relay`, `--playlist`, `--bots`, `--auto-assign-teams`, `--consent`) to override defaults.
fn cmd_host(args: &[String]) -> Result<(), String> {
    // Require a host action (`start`, `status`, `stop`, `attach`, `join`, `restart`, `kick`, `ban`, `unban`).
    let action = args.first().map(String::as_str).ok_or_else(|| {
        "Usage: desktop-tool host <start|status|stop|attach|join|restart|kick|ban|unban> [flags]"
            .to_string()
    })?;

    // Collect environment variables and resolve default desktop paths (`userData`, `contentRoot`, `settingsFile`).
    let env_map = collect_env_map();
    let home = env_map
        .get("HOME")
        .or_else(|| env_map.get("USERPROFILE"))
        .map(PathBuf::from);
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = resolve_project_root();
    let current_exe = env::current_exe().ok();
    let dp = resolve_desktop_paths(&DesktopPathsOptions {
        platform: current_platform_str().to_string(),
        home,
        cwd,
        project_root: project_root.clone(),
        exec_dir: current_exe
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
        resources_path: None,
        packaged: false,
        env: env_map.clone(),
    });

    // Check `settings.json` for `onlineNoticeAccepted` and `contentDir` when present.
    let desktop_store = SettingsStore::new(&dp.settings_file);
    let desktop_settings = desktop_store.load();

    // Default `<state>` to `<userData>/server` (`path.join(app.getPath('userData'), 'server')` in the desktop host).
    let mut state_dir = env_map
        .get("BFE_HOST_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dp.user_data.join("server"));
    // Resolve the prepared content directory from `BFE_CONTENT_DIR`, `BFE_DATA_DIR`, `settings.json`, or `dp.content_root`.
    let mut content_dir = env_map
        .get("BFE_CONTENT_DIR")
        .or_else(|| env_map.get("BFE_DATA_DIR"))
        .map(PathBuf::from)
        .or_else(|| desktop_settings.content_dir.clone().map(PathBuf::from))
        .unwrap_or_else(|| dp.content_root.clone());
    // Resolve the bundled `bfeserver` binary alongside `desktop-tool` / `bfe-client` or in `rust/target/{release,debug}`.
    let mut server_bin = resolve_server_executable(
        &env_map,
        current_exe.as_deref(),
        Some(&project_root),
    );
    // Resolve `ADMIN_PORT` (`18460`) and `GAME_PORT` (`18461`) from `BFE_HOST_ADMIN_PORT` / `BFE_HOST_GAME_PORT`.
    let mut admin_port = resolve_admin_port(&env_map);
    let mut game_port = resolve_game_port(&env_map);
    let mut online_consent = desktop_settings
        .extra
        .get("onlineNoticeAccepted")
        .and_then(|v| v.as_bool())
        == Some(true);

    let mut opts = HostOptions::default();
    let mut player_id: Option<String> = None;
    let mut reason = String::new();

    // Parse optional CLI flags for `desktop-tool host`, mapping each flag onto `HostControllerConfig`
    // or `HostOptions` (`createHostController` in the desktop host).
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            // Override the host state directory (`<userData>/server` by default).
            "--state-dir" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    state_dir = PathBuf::from(v);
                }
            }
            // Override the prepared game content directory (`contentRoot` in the desktop host).
            "--content-dir" | "--content" | "--data" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    content_dir = PathBuf::from(v);
                }
            }
            // Override the `bfeserver` binary path (`executable` in the desktop host).
            "--server-bin" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    server_bin = PathBuf::from(v);
                }
            }
            // Override the loopback `/admin` HTTP port (`BFE_HOST_ADMIN_PORT`, default `18460`).
            "--admin-port" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse::<u16>().ok()) {
                    admin_port = v;
                }
            }
            // Override the UDP game and TCP `/info` port (`BFE_HOST_GAME_PORT`, default `18461`).
            "--game-port" | "--port" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse::<u16>().ok()) {
                    game_port = v;
                }
            }
            // Set the hosted server display name (`options.name` in the desktop host).
            "--name" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    opts.name = v.clone();
                }
            }
            // Set visibility mode (`"lan"` or `"public"`, matching `options.mode` in the desktop host).
            "--mode" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    opts.mode = HostMode::parse(v)
                        .ok_or_else(|| "Invalid host settings".to_string())?;
                }
            }
            // Shorthand flag for `--mode public`.
            "--public" => {
                opts.mode = HostMode::Public;
            }
            // Shorthand flag for `--mode lan` (private / LAN-only game).
            "--lan" | "--private" => {
                opts.mode = HostMode::Lan;
            }
            // Acknowledge the online privacy notice for `--mode public` (`consent` in the desktop host).
            "--consent" => {
                online_consent = true;
            }
            // Select the initial map ID (`options.map` in the desktop host, e.g. `nab2`).
            "--map" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    opts.map = v.clone();
                }
            }
            // Select the faction ID (`options.side` in the desktop host: `rep`, `cis`, `all`, `imp`).
            "--side" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    opts.side = v.clone();
                }
            }
            // Set maximum human player capacity (`options.maxPlayers` in the desktop host, `2..=64`).
            "--max-players" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse::<u32>().ok()) {
                    opts.max_players = v;
                } else {
                    return Err("Invalid host settings".to_string());
                }
            }
            // Set TURN relay policy (`options.relay` in the desktop host: `auto`, `always`, `never`).
            "--relay" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    opts.relay = RelayPolicy::parse(v)
                        .ok_or_else(|| "Invalid host settings".to_string())?;
                }
            }
            // Set an explicit comma-separated map rotation (`swbf1/<map>:<side>:conquest`).
            "--playlist" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    opts.playlist = v
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            // Set AI bot count per team (`SET GAME OPTIONS` `ai_units_per_team`).
            "--bots" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse::<u32>().ok()) {
                    opts.ai_units_per_team = Some(v);
                }
            }
            // Set AI difficulty (`easy`, `medium`, or `hard`).
            "--difficulty" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    opts.ai_difficulty = match v.as_str() {
                        "easy" => Some(AiDifficulty::Easy),
                        "medium" => Some(AiDifficulty::Medium),
                        "hard" => Some(AiDifficulty::Hard),
                        _ => return Err("Invalid host settings".to_string()),
                    };
                }
            }
            // Enable automatic team assignment (`autoAssignTeams: true` in `config.json`).
            "--auto-assign-teams" => {
                opts.team_assignment = Some(TeamAssignment::AutoAssign);
            }
            // Enable immediate player spawning (`autoSpawn: true` in `config.json`).
            "--auto-spawn" => {
                opts.auto_spawn = Some(true);
            }
            // Specify target 64-hex player identity for `kick`, `ban`, or `unban`.
            "--id" => {
                i += 1;
                player_id = args.get(i).cloned();
            }
            // Specify moderation reason for `kick`, `ban`, or `unban`.
            "--reason" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    reason = v.clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    // Assemble the `HostControllerConfig` from resolved paths, ports, binary, and consent flag
    // (`createHostController` parameters in the desktop host).
    let mut cfg = HostControllerConfig::new(state_dir, content_dir);
    cfg.executable = server_bin;
    cfg.engine_root = project_root;
    cfg.admin_port = admin_port;
    cfg.game_port = game_port;
    cfg.online_consent = online_consent;

    // Initialize `HostController` and ensure `<state>` exists with `0700` permissions on Unix.
    let controller = HostController::new(cfg)?;

    // Dispatch the requested `desktop-tool host` action through `HostController`.
    match action {
        // Start a detached `bfeserver` (or attach if already running) and print its `HostJoinInfo`.
        "start" => {
            let join = controller.start(&opts)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&join).unwrap_or_default()
            );
            Ok(())
        }
        // Query `/admin` (`{"action": "status"}`) and `/info` and print the combined `HostStatus`.
        "status" => {
            let status = controller.status();
            println!(
                "{}",
                serde_json::to_string_pretty(&status).unwrap_or_default()
            );
            Ok(())
        }
        // Stop the running server via `POST /admin {"action": "stop"}` and remove `attach.json`.
        "stop" => {
            let res = controller.stop()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&res).unwrap_or_default()
            );
            Ok(())
        }
        // Probe `<state>/operator-token`, `/admin`, and `/info` to attach to an existing server.
        "attach" => {
            let attached = controller.attach()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "attached": attached.is_some(),
                    "server": attached,
                    "attachRecord": controller.read_attach_record(),
                }))
                .unwrap_or_default()
            );
            Ok(())
        }
        // Query `joinInfo()` (`127.0.0.1:<game_port>` and `/info` metadata) for a running server.
        "join" => {
            let join = controller.join_info()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&join).unwrap_or_default()
            );
            Ok(())
        }
        // Trigger a match restart via `POST /admin {"action": "restart"}`.
        "restart" => {
            let res = controller.restart()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&res).unwrap_or_default()
            );
            Ok(())
        }
        // Kick a connected player by 64-hex identity (`POST /admin {"action": "kick", ...}`).
        "kick" => {
            let id = player_id.ok_or_else(|| "Missing --id <64-hex>".to_string())?;
            let res = controller.kick(&id, &reason)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&res).unwrap_or_default()
            );
            Ok(())
        }
        // Ban a player for 24 hours by 64-hex identity (`POST /admin {"action": "ban", "hours": 24, ...}`).
        "ban" => {
            let id = player_id.ok_or_else(|| "Missing --id <64-hex>".to_string())?;
            let res = controller.ban(&id, &reason)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&res).unwrap_or_default()
            );
            Ok(())
        }
        // Remove a local ban for a 64-hex identity (`POST /admin {"action": "unban", ...}`).
        "unban" => {
            let id = player_id.ok_or_else(|| "Missing --id <64-hex>".to_string())?;
            let res = controller.unban(&id, &reason)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&res).unwrap_or_default()
            );
            Ok(())
        }
        // Print the `ClosePrompt` dialog strings when a server is active (`before-quit` in the desktop host).
        "close-prompt" => {
            let prompt = controller.close_prompt();
            println!(
                "{}",
                serde_json::to_string_pretty(&prompt).unwrap_or_default()
            );
            Ok(())
        }
        // Record `CloseChoice::KeepRunning` in `<state>/attach.json` without stopping the server.
        "keep-running" => {
            let allow_quit = controller.apply_close_choice(CloseChoice::KeepRunning)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "allowQuit": allow_quit,
                    "attachRecord": controller.read_attach_record(),
                }))
                .unwrap_or_default()
            );
            Ok(())
        }
        other => Err(format!("Unknown host action: {other}")),
    }
}

/// Entrypoint for `desktop-tool` subcommand dispatch.
///
/// Returns exit code `0` on success, `1` on runtime errors, and `2` when invoked
/// without a subcommand.
fn main() -> ExitCode {
    // Skip `argv[0]` (the binary path) and inspect the top-level subcommand name.
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(sub) = args.first().map(|s| s.as_str()) else {
        eprintln!(
            "Usage: desktop-tool <paths|settings|prepare|update|presence|host> ..."
        );
        return ExitCode::from(2);
    };

    // Dispatch to the selected subcommand handler:
    // - `paths`: print resolved platform directories as JSON
    // - `settings get <KEY>`: print setting value from settings.json or player-settings.json
    // - `prepare`: run tools/prepare_content.py and stream NDJSON progress snapshots
    // - `update check` / `update install --dry-run`: inspect release feeds or staged installers
    // - `presence test`: normalize and format Discord + Steam rich presence payloads
    // - `host <start|status|stop|attach|...>`: manage a detached `bfeserver` instance
    let result = match sub {
        "paths" => cmd_paths(),
        "settings" => {
            if args.get(1).map(|s| s.as_str()) == Some("get") {
                cmd_settings_get(&args[2..])
            } else {
                Err("Usage: desktop-tool settings get <KEY>".to_string())
            }
        }
        "prepare" => cmd_prepare(&args[1..]),
        "update" => match args.get(1).map(|s| s.as_str()) {
            Some("check") => cmd_update_check(&args[2..]),
            Some("install") => cmd_update_install(&args[2..]),
            _ => Err("Usage: desktop-tool update <check|install> ...".to_string()),
        },
        "presence" => {
            if args.get(1).map(|s| s.as_str()) == Some("test") {
                cmd_presence_test()
            } else {
                Err("Usage: desktop-tool presence test".to_string())
            }
        }
        "host" => cmd_host(&args[1..]),
        other => Err(format!("Unknown subcommand: {}", other)),
    };

    let _ = Path::new("");
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {}", err);
            ExitCode::from(1)
        }
    }
}
