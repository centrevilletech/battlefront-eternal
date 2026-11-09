# Native game client (`bfe-client`)

`bfe-client` is the standalone native Battlefront Eternal executable. Running
`bfe-client` with no sub-mode arguments opens a single `winit` window, creates
its OpenGL ES 3.0 (or `--desktop-gl` OpenGL 4.5) context and `cpal` audio stream,
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
    bfe-client [CONTENT_DIR] --local [<MISSION>] [LOCAL_OPTIONS]
    bfe-client [CONTENT_DIR] --join <TARGET> [ONLINE_OPTIONS]
    bfe-client [CONTENT_DIR] --map <MAP> [INSPECT_OPTIONS]
    bfe-client <SESSION_DIR> [REPLAY_OPTIONS]
```

- **Default (`bfe-client [CONTENT_DIR]`)**: Runs the standalone native game
  through the startup loader, first-run content/update gate (when needed), main
  menu shell, `bfe-interface` options pages, Instant Action playlists, and
  multiplayer server browser / local server hosting.
- **`--local [<MISSION>]`**: Skips the shell menu and launches an Instant Action
  battle directly (for example `--local nab2` or `--local "map=nab2&era=cw"`).
  Pressing `Quit to Main Menu` from the in-battle pause menu returns to the
  shell rather than exiting the process.
- **`--join <TARGET>`**: Joins an online `bfeserver` directly (`HOST:PORT` or
  `master:URL#ID`). When launched inside the standalone shell, disconnecting or
  leaving an online match returns to the Multiplayer server browser.
- **`--map <MAP>`**: Admits a static map inspection scene with a free camera
  (`--seconds <S>`, `--screenshot <PNG>`).
- **`<SESSION_DIR>`**: Replays a version-2 recorded native session archive
  against prepared content (`--pcm STEREO_F32LE`, `--sound ARCHIVE_DIR`).
- **Common flags**:
  - `--content-dir <DIR>` (or `BFE_CONTENT_DIR`): Prepared content root.
  - `--no-audio`: Disables `cpal` audio device initialization.
  - `--desktop-gl`: Requests a desktop OpenGL 4.5 context instead of GLES 3.0.
  - `--width <PX>`, `--height <PX>`, `--fullscreen`: Initial window geometry and
    borderless fullscreen mode (`F11` toggles fullscreen at runtime).
  - `--skip-update-check`: Skips the startup master-server version check.
  - `--script gate` (`--shots-dir <DIR>`, `--server <HOST:PORT>`): Drives the
    end-to-end automated verification sequence across all shell pages, a
    three-mission Instant Action playlist (`nab2` -> `tat1` -> `bes1`), and a
    live online battle, writing the 24 numbered PNG screenshots and
    `gate-summary.json`.

## Application state machine and host services

`rust/bfe-client/rust/app/` owns the top-level state machine:

1. **`Startup` / `StartupLoader`**: Initializes platform directories, loads
   the 59-key gameplay/audio/video/controls schema via `bfe_interface::Settings`
   together with `desktop-settings.json` (`bfe-desktop`), calls
   `HostController::attach()` on a background thread to re-attach any running
   local `bfeserver`, starts non-blocking Discord rich presence
   (`bfe-desktop::presence`), and renders the startup progress bar while scanning
   prepared content (`bfe-desktop::content::verify_prepared_content`) and
   loading `ui_common.font` + shell audio (`ui.loaded`).
2. **`Gate`**: Shown when no valid prepared content directory is configured or
   when a newer release is available. Allows selecting a content folder,
   extracting raw CD/ISO assets (`bfe-desktop::prepare`), or continuing to the
   main menu.
3. **`Shell`**: Runs `bfe-shell::ShellApp` and `ShellRenderer` inside the shared
   GL context, backed by `LiveServerBrowser` (`bfe-online::ServerBrowser` on
   background worker threads for master HTTPS queries, LAN UDP broadcast
   discovery on port `18452`, and concurrent TCP ping/info probes).
   - **`bfe-interface` Options and `Settings`**: Selecting `OPTIONS` from the
     Main Menu delegates rendering and input to `bfe_interface::OptionsController`
     and `bfe_interface::ClassicTheme::paint_stack`, persisting all 59
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
   through the same `BattleController` (`rust/bfe-client/rust/app/battle.rs`),
   `Scene` (`bfe-render` + `bfe-presentation`), and `bfe-battle-ui` overlays
   (loading screen, classic spawn screen, 3-page Tab scoreboard, Esc pause menu
   with live options and player list, and `T`/`Y` chat).

One `cpal` output stream stays open for the lifetime of the window.
`SharedAudioRouter` (`rust/bfe-client (audio.rs)`) switches atomically between
`Shell` (`bfe-shell` UI sound mixer), `Battle` (`audio::MasterProcessor` +
`audio_control::Runtime`), and `Muted` (when unfocused with `mute_unfocused`
enabled) without recreating the OS audio stream.

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
  render thread drains published slots exclusively via `try_lock()`, guaranteeing
  `worldRendererBlocks == 0`, while the world thread records its per-tick
  execution time distribution (`WorldTickStats`: `min_ms`, `p50_ms`, `p95_ms`,
  `p99_ms`, `max_ms`, `mean_ms`).
- **Zero steady-state render allocations**: After initial warmup frames, active
  battle frames (without cold UI overlays open) perform zero heap allocations on
  the render thread (`steadyFrameAllocationsAfterWarmup == 0`), tracked by the
  counting `GlobalAlloc` wrapper in `rust/bfe-client (main.rs)`.

## Direct online sessions, map inspection, and recorded-session replay

`bfe-client --join HOST:PORT CONTENT_DIR` reads the server's public battle
information, loads that mission, and authenticates `BFE-UDP/1` with a persistent
Ed25519 identity (`bfe-online::PlayerIdentity`). Online sessions receive
snapshots directly into the Rust network owner (`bfe_client::online::client::OnlineClient`)
and sample its retained timeline from the presentation phases without creating a
local authoritative world.

`bfe-content-native::Loader` reads prepared maps, sides, and common files using
the shared native definition, clip, model, collision, and BVH loaders, then
constructs visible geometry, material descriptors, texture resources, and
unit/first-person presentation content.

`bfe-client --map MAP CONTENT_DIR --seconds 20` admits an inspection scene from
prepared files with a free camera, opaque near/far world and terrain, sky layers,
initial built/unbuilt prop rigs, grounded trap pads, placed foliage, and Low
water or Classic ocean surfaces.

`bfe-client SESSION_DIR CONTENT_DIR` replays version-2 recorded session archives.
`BFE_SESSION_VALIDATE=1` validates recorded renderer submissions and
`BFE_SESSION_GL_CHECK=1` checks GL errors after each pass.

## Hosting from the native client (`HostController`)

`bfe_desktop::host::HostController` (`rust/bfe-desktop/src/host/mod.rs`) manages
a detached `bfeserver` instance from the native client (`MULTIPLAYER -> CREATE ->
SET GAME OPTIONS -> LAUNCH`) and via `desktop-tool host`:

- **What runs**: `HostController::start` writes `<state>/config.json` (mode
  `0600`, where `<state>` defaults to `<userData>/server` and can be overridden
  with `BFE_HOST_STATE_DIR` or `--state-dir`) using the server's configuration
  schema (`name`, `engine`, `data`, `rotation`, `maxPlayers`,
  `onlineNoticeAccepted`, `master`, `relayPolicy`, `honorGlobalBans`, and
  `gamePort`) and spawns the bundled `bfeserver --serve` binary detached from
  the client (`process_group(0)` on Unix; `DETACHED_PROCESS |
  CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW` on Windows; or a transient
  `systemd-run --user` unit when launched under native Linux Steam).
- **Ports and environment overrides**:
  - Loopback operator HTTP port (`/admin` and `/healthz`): `18460` by default,
    overridable with `BFE_HOST_ADMIN_PORT` (or `--admin-port`).
  - Game port (UDP `BFE-UDP/1` gameplay and TCP `GET /info`): `18461` by
    default, overridable with `BFE_HOST_GAME_PORT` (or `--game-port`).
  - Server executable path: resolved next to `bfe-client` (`bfeserver` on Linux,
    `bfeserver.exe` on Windows), overridable with `BFE_HOST_SERVER_BIN` (or
    `--server-bin`).
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
- **CLI subcommands (`desktop-tool host`)**:
  ```bash
  desktop-tool host start [--mode lan|public] [--map nab2] [--side rep] [--max-players 8]
  desktop-tool host status
  desktop-tool host attach
  desktop-tool host stop
  ```
