# Native game client (`bfe-client`)

`bfe-client` is the standalone native Battlefront Eternal executable. Running
`bfe-client` with no sub-mode arguments opens a single `winit` window, creates
its OpenGL ES 3.0 context (falling back to, or with `--desktop-gl` asking for, desktop OpenGL 3.3) and `cpal` audio stream,
loads persisted settings (`bfe_interface::Settings`) and desktop state through
`bfe-desktop`, attaches any already-running hosted server via
`bfe_desktop::host::HostController::attach()`, and drives the full game
lifecycle natively in Rust:

```text
Startup -> StartupLoader -> Gate -> Shell -> HostStarting / BattleLoading -> InstantAction / Online -> Shell -> Exit
```

Build and run with:

```bash
cargo build --manifest-path rust/Cargo.toml -p bfe-client --bin bfe-client
./rust/target/debug/bfe-client [--content-dir /path/to/prepared/content]
```

## Command-line interface

```text
USAGE:
    bfe-client [CONTENT_DIR] [OPTIONS]
```

- **Default (`bfe-client [CONTENT_DIR]`)**: Runs the standalone native game
  through the startup loader, first-run content/update gate (when needed), main
  menu shell, `bfe-interface` options pages, Instant Action playlists, and
  multiplayer server browser / local server hosting.
- **Common flags**:
  - `--content-dir <DIR>` (or `BFE_CONTENT_DIR`): Prepared content root.
  - `--no-audio`: Disables `cpal` audio device initialization.
  - `--desktop-gl`: Requests a desktop OpenGL 3.3 context instead of OpenGL ES 3.0.
  - `--width <PX>`, `--height <PX>`, `--fullscreen`: Initial window geometry and
    borderless fullscreen mode (`F11` toggles fullscreen at runtime).
  - `--skip-update-check`: Skips the startup check for a newer release.
  - `--script ia:<MAP>` (`--shots-dir <DIR>`): Runs Instant Action rounds on one
    map with generated combat input for `BFE_GATE_BATTLE_SECONDS` (default 300)
    of combat and writes screenshots and `gate-summary.json`.
  - `--script platform-smoke` (`--shots-dir <DIR>`): The CI startup, menu and
    synthetic-room smoke on software OpenGL (`tools/platform-smoke-native.py`).
  - `--server <HOST:PORT>`: The `bfeserver` a scripted run joins online.
  - `-h`/`--help` and `-V`/`--version` print and exit before any window opens.

## Application state machine and host services

`rust/bfe-client/src/app/` owns the top-level state machine:

1. **`Startup` / `StartupLoader`**: Initializes platform directories, loads
   the 59-key gameplay/audio/video/controls schema via `bfe_interface::Settings`
   together with `desktop-settings.json` (`bfe-desktop`), calls
   `HostController::attach()` on a background thread to re-attach any running
   local `bfeserver`, starts non-blocking Discord rich presence
   (`bfe-desktop::presence`), and renders the startup progress bar while scanning
   prepared content (`bfe-desktop::content::verify_prepared_content`) and
   loading `ui_common.font` + shell audio (`ui.loaded`).
2. **`Gate`**: Shown when no valid prepared content directory is configured or
   when a newer release is available. Allows choosing the game's folder or
   prepared data, preparing the data from the installation
   (`bfe-desktop::prepare`), importing an export, or continuing to the main
   menu.
3. **`Shell`**: Runs `bfe-shell::ShellApp` and `ShellRenderer` inside the shared
   GL context, backed by `LiveServerBrowser` (`bfe-online::ServerBrowser` on
   background worker threads for master HTTPS queries, LAN UDP broadcast
   discovery on port `18452`, and concurrent TCP ping/info probes).
   - **`bfe-interface` Options and `Settings`**: Selecting `OPTIONS` from the
     Main Menu hands input to `bfe_interface::OptionsController`, which Slint
     draws (`bfe-client/src/app/slint_options.rs`), persisting all 59
     settings keys through `bfe_interface::Settings` (`AppSettingsStore`
     in `rust/bfe-client/src/app/settings.rs`).
   - **`MULTIPLAYER -> HOST` (`SET GAME OPTIONS`)**: Selecting `CREATE` ->
     `LAUNCH` on `ScreenId::HostDialog` maps the user's configured server name,
     visibility (`LAN ONLY` / `PUBLIC`), max players, AI count/difficulty,
     team auto-assign, and playlist rotation into `bfe_desktop::host::HostOptions`,
     transitions to `AppState::HostStarting` while `HostController::start` waits
     for `/admin` and `/info`, and then joins the local player through the
     normal online path (`BattleController::start_online`).
4. **`InstantAction` and `Online` (`BattleController`)**: Both battle modes run
   through the same `BattleController` (`rust/bfe-client/src/app/battle.rs`),
   `Scene` (`bfe-render` + `bfe-presentation`), and `bfe-battle-ui` overlays
   (loading screen, classic spawn screen, 3-page Tab scoreboard, Esc pause menu
   with live options and player list, and `T`/`Y` chat).

One `cpal` output stream stays open for the lifetime of the window.
`SharedAudioRouter` (`rust/bfe-client/src/audio.rs`) switches between the front
end's mixer (`bfe-shell`) and the battle's sound without recreating the OS
audio stream; it reopens the device only when a battle's speaker layout needs
more channels.

## Local 60 Hz world thread (`InstantAction`)

In `InstantAction`, `BattleController` spawns a dedicated 60 Hz simulation
thread (`rust/bfe-client/src/local/thread.rs`, `WorldThread::spawn`, named
`"bfe-world-60hz"`) that owns the local battle authority behind the same command
and publication boundary used by the online path:

- **Exclusive `Context::world_admit` admission**: `Prepared::new`
  (`rust/bfe-client/src/local/runtime.rs`) admits the mission world exclusively
  through `bfe_sim::abi::context::Context::world_admit(&files, &options.admit())`
  before any OpenGL scene or audio stream is created. No stand-in or fallback
  authority exists; if `Context::world_admit` fails, the error is surfaced
  directly to the player.
- **Render -> World (`CommandRing`)**: Pre-allocated 64-entry ring buffer of
  `WorldMessage` (`Step`, `Pause`, `Suspend`, `Spawn`, `Respawn`, `Restart`,
  `Shutdown`). If the ring fills, the oldest `Step` is evicted so control
  messages are never dropped.
- **World -> Render (`PublicationQueue`)**: Pre-allocated 4-slot pool of
  `Publication` records recycled between the world thread and render thread. The
  render thread drains published slots exclusively via `try_lock()`, so it never
  waits on the world thread, while the world thread records its per-tick
  execution time distribution (`WorldTickStats`: `min_ms`, `p50_ms`, `p95_ms`,
  `p99_ms`, `max_ms`, `mean_ms`).

## Online sessions and content

Online sessions authenticate `BFE-UDP/1` with a persistent Ed25519 identity
(`bfe-online::PlayerIdentity`) and receive snapshots directly into the Rust
network owner (`bfe_client::online::client::OnlineClient`) without creating a
local authoritative world. `bfe-content-native::Loader` reads prepared maps,
sides and common files.

## Hosting from the native client (`HostController`)

`bfe_desktop::host::HostController` (`rust/bfe-desktop/src/host/mod.rs`) manages
a detached `bfeserver` instance from the native client (`MULTIPLAYER -> CREATE ->
SET GAME OPTIONS -> LAUNCH`):

- **What runs**: `HostController::start` writes `<state>/config.json` (mode
  `0600`, where `<state>` is `<userData>/server`) using the server's configuration
  schema (`name`, `engine`, `data`, `rotation`, `maxPlayers`,
  `onlineNoticeAccepted`, `master`, `relayPolicy`, `honorGlobalBans`, and
  `gamePort`) and spawns the bundled `bfeserver --serve` binary detached from
  the client (`process_group(0)` on Unix; `DETACHED_PROCESS |
  CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW` on Windows; or a transient
  `systemd-run --user` unit when launched under native Linux Steam).
- **Ports and environment overrides**:
  - Loopback operator HTTP port (`/admin` and `/healthz`): `18460` by default,
    overridable with `BFE_HOST_ADMIN_PORT`.
  - Game port (UDP `BFE-UDP/1` gameplay and TCP `GET /info`): `18461` by
    default, overridable with `BFE_HOST_GAME_PORT`.
  - Server executable path: resolved next to `bfe-client` (`bfeserver` on Linux,
    `bfeserver.exe` on Windows), overridable with `BFE_HOST_SERVER_BIN`.
- **Logs and state files**: Standard output and standard error from `bfeserver`
  are appended to `<state>/hosting.log` (mode `0600`, truncated automatically
  when exceeding 8 MiB). The bearer token in `<state>/operator-token` (mode
  `0600`) authenticates `POST http://127.0.0.1:18460/admin` requests (`status`,
  `stop`, `restart`, `kick`, `ban`, `unban`).
- **Keep-running and attach behavior**: When the user closes the client while a
  hosted server is active, `HostController::close_prompt` supplies the 3-button
  confirmation dialog (`"Hosted server is running"`, `"Other players can keep
  playing after you close the app."`) rendered as `PopupState::HostClosePrompt`
  in `bfe-shell`, mapping button selections via `ClosePrompt::choice_for_index`
  to `CloseChoice::KeepRunning` (`"Keep server running and quit"`),
  `CloseChoice::StopAndQuit` (`"Stop server and quit"`), and
  `CloseChoice::Cancel` (`"Cancel"`). Choosing `KeepRunning` leaves `bfeserver`
  alive and records `<state>/attach.json`. On the next client startup (or a
  subsequent `HostController::start` / `HostController::attach` call), the
  controller probes `<state>/operator-token`, `/admin`, and `/info` and attaches
  to the running server instead of spawning a second `bfeserver` process.
