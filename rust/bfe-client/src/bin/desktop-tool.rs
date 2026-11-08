//! CLI tool (`desktop-tool`) for exercising `bfe-desktop` paths, settings,
//! content preparation, update feed verification, dry-run installer handoff,
//! and rich presence formatting.
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
//!
//! ## Exit Codes
//! - `0`: Subcommand completed cleanly and printed its JSON or NDJSON output.
//! - `1`: Runtime validation, file I/O, or subprocess error.
//! - `2`: Missing command-line subcommand.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bfe_desktop::paths::{resolve_desktop_paths, update_paths, DesktopPathsOptions};
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
use bfe_desktop::update::feed::{
    normalize_channel, parse_and_verify_feed, resolve_channel_feed_url, select_update_target,
    SelectTargetOptions, VerifyFeedOptions,
};
use bfe_desktop::update::install::{apply_pending_installer, ApplyInstallerOptions};
use bfe_desktop::update::keys::FEED;

/// Returns the platform string matching Node/Electron conventions (`"linux"`, `"darwin"`, `"win32"`).
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

/// Returns the CPU architecture string matching Node/Electron conventions (`"x64"`, `"arm64"`).
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

    // Resolve the release channel feed endpoint URL from `BFE_UPDATE_FEED_URL` or the default feed constant.
    let env_map = collect_env_map();
    let base_url = env_map
        .get("BFE_UPDATE_FEED_URL")
        .cloned()
        .unwrap_or_else(|| FEED.to_string());
    let feed_url = resolve_channel_feed_url(&base_url, Some(&channel), true)?;

    // When a local feed file is supplied, verify its Ed25519 signature and select the update target.
    if let Some(path) = feed_file.or_else(|| env_map.get("BFE_UPDATE_FEED_FILE").map(PathBuf::from))
    {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("Failed reading {}: {}", path.display(), e))?;
        let val: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON: {}", e))?;
        let feed = parse_and_verify_feed(
            &val,
            &VerifyFeedOptions {
                expected_channel: Some(channel.clone()),
                public_keys: None,
                allow_insecure_local: true,
                allow_unsigned: false,
            },
        )?;
        let selection = select_update_target(
            &feed,
            &SelectTargetOptions {
                current_version: env!("CARGO_PKG_VERSION").to_string(),
                shell_version: env!("CARGO_PKG_VERSION").to_string(),
                platform: current_platform_str().to_string(),
                arch: current_arch_str().to_string(),
                allow_payload: true,
            },
        )?;
        println!(
            "{}",
            serde_json::to_string_pretty(&selection).unwrap_or_default()
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
/// Requires `--dry-run` so the CLI never overwrites the running binary or spawns a
/// detached OS installer during diagnostics.
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

    // Resolve the target `userData` directory from `--state-dir` or default platform paths.
    let env_map = collect_env_map();
    let user_data = state_dir.unwrap_or_else(|| {
        let home = env_map
            .get("HOME")
            .or_else(|| env_map.get("USERPROFILE"))
            .map(PathBuf::from);
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        resolve_desktop_paths(&DesktopPathsOptions {
            platform: current_platform_str().to_string(),
            home,
            cwd,
            project_root: resolve_project_root(),
            exec_dir: None,
            resources_path: None,
            packaged: false,
            env: env_map.clone(),
        })
        .user_data
    });

    // Inspect the update state file or run `apply_pending_installer` in dry-run mode.
    let up = update_paths(&user_data);
    if !up.state_file.exists() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "action": "dry-run",
                "status": "no-pending-installer",
                "stateFile": up.state_file,
            }))
            .unwrap_or_default()
        );
        return Ok(());
    }

    let res = apply_pending_installer(
        &up,
        &ApplyInstallerOptions {
            platform: current_platform_str().to_string(),
            env: env_map,
            exec_path: env::current_exe().unwrap_or_else(|_| PathBuf::from("bfe-client")),
            dry_run: true,
        },
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&res).unwrap_or_default()
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

/// Entrypoint for `desktop-tool` subcommand dispatch.
///
/// Returns exit code `0` on success, `1` on runtime errors, and `2` when invoked
/// without a subcommand.
fn main() -> ExitCode {
    // Skip `argv[0]` (the binary path) and inspect the top-level subcommand name.
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(sub) = args.first().map(|s| s.as_str()) else {
        eprintln!("Usage: desktop-tool <paths|settings|prepare|update|presence> ...");
        return ExitCode::from(2);
    };

    // Dispatch to the selected subcommand handler:
    // - `paths`: print resolved platform directories as JSON
    // - `settings get <KEY>`: print setting value from settings.json or player-settings.json
    // - `prepare`: run tools/prepare_content.py and stream NDJSON progress snapshots
    // - `update check` / `update install --dry-run`: inspect release feeds or staged installers
    // - `presence test`: normalize and format Discord + Steam rich presence payloads
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
