# Native game client (`bfe-client`)

`bfe-client` is the standalone native Battlefront Eternal executable. Running
`bfe-client` with no sub-mode arguments opens a single `winit` window, creates
its OpenGL ES 3.0 (or `--desktop-gl` OpenGL 4.5) context and `cpal` audio stream,
loads persisted settings and profiles through `bfe-desktop`, and drives the full
game lifecycle without a browser or Electron runtime:

```text
Startup -> StartupLoader -> Gate -> Shell -> InstantAction / Online -> Shell -> Exit
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
  menu shell, Instant Action playlists, and multiplayer server browser.
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
    borderless fullscreen mode (`F11` or `Alt+Enter` toggles fullscreen at
    runtime; `F12` saves a PNG screenshot to the user's screenshots directory).
  - `--skip-update-check`: Skips the startup master-server version check.
  - `--script gate` (`--shots-dir <DIR>`, `--server <HOST:PORT>`): Drives the
    end-to-end automated verification sequence across all shell pages, a
    three-mission Instant Action playlist (`nab2` -> `tat1` -> `bes1`), and a
    live online battle, writing the 24 numbered PNG screenshots and
    `gate-summary.json`.

## Application state machine and host services

`rust/bfe-client/src/app/` owns the top-level state machine:

1. **`Startup` / `StartupLoader`**: Initializes platform directories, loads
   `settings.json` via `bfe-desktop::settings`, starts non-blocking Steam/Discord
   rich presence (`bfe-desktop::presence`) and the asynchronous update check
   (`bfe-desktop::updater`), and renders the startup progress bar while scanning
   prepared content (`bfe-desktop::content_gate::verify_content_dir`) and
   loading `ui_common.font` + shell audio (`ui.loaded`).
2. **`Gate`**: Shown when no valid prepared content directory is configured or
   when a newer release is available. Allows selecting a content folder,
   Triggering raw CD/ISO asset extraction via `bfe-desktop::extractor`, or
   continuing to the main menu.
3. **`Shell`**: Runs `bfe-shell::ShellApp` and `ShellRenderer` inside the shared
   GL context, backed by `LiveServerBrowser` (`bfe-online::ServerBrowser` on
   background worker threads for master HTTPS queries, LAN UDP broadcast
   discovery on port `18452`, and concurrent TCP ping/info probes).
   > **Swap point (`bfe-interface`)**: Option pages, hotkeys, and `Settings`
   > persistence in `bfe-client/src/app/settings.rs` and `shell.rs` currently
   > bridge `bfe-shell`'s option models and `bfe-desktop::settings`; `bfe-interface`
   > provides the unified 59-key schema and `ClassicTheme` menu framework ready
   > to replace the shell option page bridge in a follow-up swap.
4. **`InstantAction` and `Online` (`BattleController`)**: Both battle modes run
   through the same `BattleController` (`rust/bfe-client/src/app/battle.rs`),
   `Scene` (`bfe-render` + `bfe-presentation`), and `bfe-battle-ui` overlays
   (loading screen, classic spawn screen, 3-page Tab scoreboard, Esc pause menu
   with live options and player list, and `T`/`Y` chat).

One `cpal` output stream stays open for the lifetime of the window.
`SharedAudioRouter` (`rust/bfe-client/src/audio.rs`) switches atomically between
`Shell` (`bfe-shell` UI sound mixer), `Battle` (`audio::MasterProcessor` +
`audio_control::Runtime`), and `Muted` (when unfocused with `mute_unfocused`
enabled) without recreating the OS audio stream.

## Local 60 Hz world thread (`InstantAction`)

In `InstantAction`, `BattleController` spawns a dedicated 60 Hz simulation
thread (`rust/bfe-client/src/local/thread.rs`, `WorldThread::spawn`) that owns
the local battle authority behind the same command and publication boundary used
by the online path:

- **Render -> World (`CommandRing`)**: Pre-allocated 64-entry ring buffer of
  `WorldMessage` (`Step`, `Pause`, `Suspend`, `Spawn`, `Respawn`, `Restart`,
  `Shutdown`). If the ring fills, the oldest `Step` is evicted so control
  messages are never dropped.
- **World -> Render (`PublicationQueue`)**: Pre-allocated 4-slot pool of
  `Publication` records recycled between the world thread and render thread. The
  render thread drains published slots exclusively via `try_lock()`, guaranteeing
  `worldRendererBlocks == 0`.
- **Single swap point (`Context::world_admit`)**: `rust/bfe-client/src/local/runtime.rs`
  attempts `bfe_sim::context::Context::world_admit` first and runs
  `WorldAuthority` (`world_Frame` + `Publisher::capture_in` into `BlockView`
  wire words) when admitted, falling back cleanly to the self-contained 60 Hz
  `FallbackAuthority` when native map runtime tables are not yet populated.
- **Zero steady-state render allocations**: After initial warmup frames, active
  battle frames (without cold UI overlays open) perform zero heap allocations on
  the render thread (`steadyFrameAllocationsAfterWarmup == 0`), tracked by the
  counting `GlobalAlloc` wrapper in `rust/bfe-client/src/main.rs`.

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

`bfe-client SESSION_DIR CONTENT_DIR` replays version-2 recorded session archives
captured with `tools/native-session/capture.mjs`. `BFE_SESSION_VALIDATE=1`
validates recorded renderer submissions and `BFE_SESSION_GL_CHECK=1` checks GL
errors after each pass.
