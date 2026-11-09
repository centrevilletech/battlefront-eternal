//! Command-line argument parsing and mode dispatch for `bfe-client`.
//!
//! Running `bfe-client` with no arguments (or with an optional prepared content
//! directory) launches the standalone native game (`Startup` -> `Loader` ->
//! `Gate` -> `Shell` -> `InstantAction` / `Online` -> `Shell`).
//!
//! Existing diagnostic and CLI sub-modes (`--map`, `--join`, `--local`, and
//! recorded `SESSION_DIR`) remain supported for backwards compatibility.
//! `--help` / `-h` and `--version` / `-V` print to stdout and exit 0 before
//! creating any window, audio device, or OpenGL context.

use std::path::PathBuf;

/// Version string for `bfe-client`, embedded from Cargo package metadata at compile time.
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Usage text printed for `--help` / `-h`.
///
/// Covers both the default standalone game mode and the legacy diagnostic sub-modes
/// (`--map`, `--join`, `--local`, and `<SESSION_DIR>`).
pub const HELP_TEXT: &str = "\
Battlefront Eternal Native Client (bfe-client)

USAGE:
    bfe-client [CONTENT_DIR] [OPTIONS]
    bfe-client [CONTENT_DIR] --map <MAP> [INSPECT_OPTIONS]
    bfe-client [CONTENT_DIR] --join <TARGET> [ONLINE_OPTIONS]
    bfe-client [CONTENT_DIR] --local [<MISSION>] [LOCAL_OPTIONS]
    bfe-client <SESSION_DIR> [REPLAY_OPTIONS]

STANDALONE OPTIONS:
    -h, --help                Print this help text and exit 0
    -V, --version             Print version and exit 0
    --content-dir <DIR>       Use prepared content directory (overrides persisted setting)
    --no-audio                Disable audio device initialization
    --desktop-gl              Request desktop OpenGL 4.5 context instead of GLES 3.0
    --width <PX>              Initial window width in pixels (default: 1280)
    --height <PX>             Initial window height in pixels (default: 720)
    --fullscreen              Start in borderless fullscreen mode
    --skip-update-check       Skip startup update check against master server
    --script <MODE>           Run automated verification script (gate, ia:<MAP>, platform-smoke)
    --shots-dir <DIR>         Output directory for scripted screenshots
    --server <HOST:PORT>      Remote bfeserver address for scripted online join

SUB-MODES:
    --map <MAP>               Launch static map inspector (see --seconds, --screenshot)
    --join <TARGET>           Direct online battle join (HOST:PORT or master:URL#ID)
    --local [<MISSION>]       Direct local battle host (e.g. nab2 or map=nab2&era=cw)
";

/// Options for the default standalone native game mode (`CliDispatch::Standalone`).
///
/// Controls window geometry, OpenGL context profile selection, audio device
/// opening, startup content overrides, and automated `--script gate` choreography.
#[derive(Clone, Debug, PartialEq)]
pub struct StandaloneOptions {
    /// Optional explicit prepared content directory (`--content-dir` or positional `CONTENT_DIR`).
    pub content_dir: Option<PathBuf>,
    /// Whether to open a live `cpal` audio output stream (`false` when `--no-audio` is passed).
    pub audio: bool,
    /// Whether to request a desktop OpenGL 3.3+ core context instead of GLES 3.0 (`--desktop-gl`).
    pub desktop_gl: bool,
    /// Initial physical window dimensions `[width, height]` in pixels (`1280x720` by default).
    pub size: [i32; 2],
    /// Whether to start the window in borderless fullscreen mode (`--fullscreen`).
    pub fullscreen: bool,
    /// Whether to skip the bounded startup update check (`--skip-update-check`).
    pub skip_update_check: bool,
    /// Optional automated verification script name (`--script gate` or `--script gate-fast`).
    pub script: Option<String>,
    /// Optional directory where `--script gate` writes PNG screenshots and `gate-summary.json`.
    pub shots_dir: Option<PathBuf>,
    /// Optional `HOST:PORT` target for the online leg of `--script gate` (`--server`).
    pub server_target: Option<String>,
}

impl Default for StandaloneOptions {
    /// Returns the default standalone options (`1280x720` windowed, audio enabled, GLES 3.0).
    fn default() -> Self {
        Self {
            content_dir: None,
            audio: true,
            desktop_gl: false,
            size: [1280, 720],
            fullscreen: false,
            skip_update_check: false,
            script: None,
            shots_dir: None,
            server_target: None,
        }
    }
}

/// Top-level CLI dispatch target returned by [`parse_args`].
#[derive(Debug)]
pub enum CliDispatch {
    /// Print help text to stdout and exit 0 immediately before opening a window.
    Help(String),
    /// Print version string to stdout and exit 0 immediately before opening a window.
    Version(String),
    /// Run the standalone full native game (`Startup` -> `Loader` -> `Shell` -> `Battle`).
    Standalone(StandaloneOptions),
    /// Run the legacy `--local` single-battle CLI host (`crate::local::launch`).
    LegacyLocal(Vec<String>),
    /// Run the legacy `--join` direct online battle window (`crate::session_window`).
    LegacyJoin(Vec<String>),
    /// Run the legacy `--map` static viewer or `SESSION_DIR` replay viewer (`crate::session_window`).
    LegacyWindow(Vec<String>),
}

/// Parses command-line arguments from an iterator (excluding `argv[0]`) and
/// returns the selected [`CliDispatch`] without touching any OS window, audio,
/// or OpenGL state.
pub fn parse_args<I>(args: I) -> Result<CliDispatch, String>
where
    I: IntoIterator<Item = String>,
{
    // Collect the raw argument strings so we can scan for priority flags before consuming tokens.
    let raw: Vec<String> = args.into_iter().collect();

    // Check `--help` / `-h` and `--version` / `-V` first so they always succeed
    // cleanly before any window, audio, or GL initialization.
    for arg in &raw {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliDispatch::Help(HELP_TEXT.to_string())),
            "--version" | "-V" => {
                return Ok(CliDispatch::Version(format!(
                    "bfe-client {CLIENT_VERSION}"
                )));
            }
            _ => {}
        }
    }

    // Dispatch legacy sub-modes when their selector flags are present anywhere on the command line.
    if raw.iter().any(|a| a == "--local") {
        return Ok(CliDispatch::LegacyLocal(raw));
    }
    if raw.iter().any(|a| a == "--join") {
        return Ok(CliDispatch::LegacyJoin(raw));
    }
    if raw.iter().any(|a| a == "--map") {
        return Ok(CliDispatch::LegacyWindow(raw));
    }

    // Check if a positional argument is a recorded session or legacy capture directory
    // containing `session.json` or `calls.json`.
    if raw.iter().any(|a| {
        if a.starts_with('-') {
            return false;
        }
        let p = std::path::Path::new(a);
        p.join("session.json").is_file() || p.join("calls.json").is_file()
    }) {
        return Ok(CliDispatch::LegacyWindow(raw));
    }

    // Seed standalone options with `BFE_CONTENT_DIR` from the environment if present.
    let mut opts = StandaloneOptions {
        content_dir: std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from),
        ..StandaloneOptions::default()
    };

    // Parse standalone option flags and at most one positional content directory path.
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--content-dir" => {
                // Explicit `--content-dir <DIR>` overrides both `BFE_CONTENT_DIR` and persisted settings.
                let dir = it.next().ok_or("missing value for --content-dir")?;
                opts.content_dir = Some(PathBuf::from(dir));
            }
            s if s.starts_with("--content-dir=") => {
                let dir = &s["--content-dir=".len()..];
                if dir.is_empty() {
                    return Err("missing value for --content-dir".into());
                }
                opts.content_dir = Some(PathBuf::from(dir));
            }
            "--no-sandbox"
            | "--disable-setuid-sandbox"
            | "--in-process-gpu"
            | "--disable-direct-composition"
            | "--force_high_performance_gpu"
            | "--steam"
            | "--appimage-extract-and-run" => {}
            s if s.starts_with("--dev-url=")
                || s.starts_with("--query=")
                || s.starts_with("--network-family-autoselection-attempt-timeout=") => {}
            "--no-audio" => {
                // Run with headless audio pumping instead of opening a hardware `cpal` stream.
                opts.audio = false;
            }
            "--desktop-gl" => {
                // Request a desktop OpenGL 3.3+ context instead of OpenGL ES 3.0.
                opts.desktop_gl = true;
            }
            "--fullscreen" => {
                // Start the window in borderless fullscreen mode.
                opts.fullscreen = true;
            }
            "--skip-update-check" => {
                // Bypass the startup release-feed check.
                opts.skip_update_check = true;
            }
            "--width" => {
                // Parse and validate horizontal window resolution in physical pixels (`320..=8192`).
                let w: i32 = it
                    .next()
                    .ok_or("missing value for --width")?
                    .parse()
                    .map_err(|_| "invalid --width value")?;
                if !(320..=8192).contains(&w) {
                    return Err("--width must be between 320 and 8192".into());
                }
                opts.size[0] = w;
            }
            "--height" => {
                // Parse and validate vertical window resolution in physical pixels (`240..=8192`).
                let h: i32 = it
                    .next()
                    .ok_or("missing value for --height")?
                    .parse()
                    .map_err(|_| "invalid --height value")?;
                if !(240..=8192).contains(&h) {
                    return Err("--height must be between 240 and 8192".into());
                }
                opts.size[1] = h;
            }
            "--script" => {
                // Enable automated gate choreography (`gate` or `gate-fast`).
                let s = it.next().ok_or("missing value for --script")?;
                opts.script = Some(s);
            }
            "--shots-dir" => {
                // Override output directory for gate PNG screenshots and `gate-summary.json`.
                let d = it.next().ok_or("missing value for --shots-dir")?;
                opts.shots_dir = Some(PathBuf::from(d));
            }
            "--server" => {
                // Override target `bfeserver` address (`HOST:PORT`) for the online gate phase.
                let t = it.next().ok_or("missing value for --server")?;
                opts.server_target = Some(t);
            }
            other if other.starts_with('-') => {
                // Reject unrecognized flags with a clear diagnostic and the full help text.
                return Err(format!("unknown option: {other}\n\n{HELP_TEXT}"));
            }
            positional => {
                // Accept a single positional `CONTENT_DIR` argument if `--content-dir` was not already set.
                if opts.content_dir.is_some()
                    && std::env::var_os("BFE_CONTENT_DIR")
                        .map(PathBuf::from)
                        .as_ref()
                        != opts.content_dir.as_ref()
                {
                    return Err(format!("unexpected positional argument: {positional}"));
                }
                opts.content_dir = Some(PathBuf::from(positional));
            }
        }
    }

    Ok(CliDispatch::Standalone(opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies `--help`, `-h`, `--version`, and `-V` return `CliDispatch::Help` / `Version`.
    #[test]
    fn help_and_version_exit_cleanly() {
        let h = parse_args(vec!["--help".to_string()]).expect("help");
        assert!(matches!(h, CliDispatch::Help(ref s) if s.contains("USAGE:")));

        let h_short = parse_args(vec!["-h".to_string()]).expect("short help");
        assert!(matches!(h_short, CliDispatch::Help(_)));

        let v = parse_args(vec!["--version".to_string()]).expect("version");
        assert!(matches!(v, CliDispatch::Version(ref s) if s.starts_with("bfe-client ")));

        let v_short = parse_args(vec!["-V".to_string()]).expect("short version");
        assert!(matches!(v_short, CliDispatch::Version(_)));
    }

    /// Verifies unknown flags fail fast with an error message containing the offending flag.
    #[test]
    fn unknown_flag_returns_err() {
        let err = parse_args(vec!["--bogus-flag".to_string()]).unwrap_err();
        assert!(err.contains("unknown option: --bogus-flag"));
    }

    /// Verifies invoking `bfe-client` with no arguments selects `CliDispatch::Standalone`.
    #[test]
    fn default_no_args_selects_standalone() {
        let res = parse_args(Vec::<String>::new()).expect("standalone");
        assert!(matches!(res, CliDispatch::Standalone(_)));
    }

    /// Verifies `--map`, `--join`, and `--local` route to their respective legacy handlers.
    #[test]
    fn bootstrap_and_launcher_flags_are_ignored() {
        let res = parse_args(vec![
            "--no-sandbox".into(),
            "--disable-setuid-sandbox".into(),
            "--in-process-gpu".into(),
            "--disable-direct-composition".into(),
            "--force_high_performance_gpu".into(),
            "--steam".into(),
            "--appimage-extract-and-run".into(),
            "--dev-url=http://127.0.0.1:18449".into(),
            "--query=map=nab2".into(),
            "--network-family-autoselection-attempt-timeout=250".into(),
            "--content-dir=/opt/example/content".into(),
        ])
        .expect("standalone with bootstrap flags");
        match res {
            CliDispatch::Standalone(opts) => {
                assert_eq!(opts.content_dir, Some(PathBuf::from("/opt/example/content")));
            }
            other => panic!("expected Standalone, got {other:?}"),
        }
    }

    #[test]
    fn legacy_modes_dispatch_correctly() {
        let m = parse_args(vec!["/opt/example/content".into(), "--map".into(), "nab2".into()]).unwrap();
        assert!(matches!(m, CliDispatch::LegacyWindow(_)));

        let j = parse_args(vec![
            "/opt/example/content".into(),
            "--join".into(),
            "127.0.0.1:18451".into(),
        ])
        .unwrap();
        assert!(matches!(j, CliDispatch::LegacyJoin(_)));

        let l = parse_args(vec!["/opt/example/content".into(), "--local".into(), "nab2".into()]).unwrap();
        assert!(matches!(l, CliDispatch::LegacyLocal(_)));
    }
}
