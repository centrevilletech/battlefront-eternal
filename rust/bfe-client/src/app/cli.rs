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

/// Version string for `bfe-client`.
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Usage text printed for `--help` / `-h`.
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
    --script <MODE>           Run automated verification script (gate, ia:<MAP>)
    --shots-dir <DIR>         Output directory for scripted screenshots
    --server <HOST:PORT>      Remote bfeserver address for scripted online join

SUB-MODES:
    --map <MAP>               Launch static map inspector (see --seconds, --screenshot)
    --join <TARGET>           Direct online battle join (HOST:PORT or master:URL#ID)
    --local [<MISSION>]       Direct local battle host (e.g. nab2 or map=nab2&era=cw)
";

/// Options for the default standalone native game mode.
#[derive(Clone, Debug, PartialEq)]
pub struct StandaloneOptions {
    pub content_dir: Option<PathBuf>,
    pub audio: bool,
    pub desktop_gl: bool,
    pub size: [i32; 2],
    pub fullscreen: bool,
    pub skip_update_check: bool,
    pub script: Option<String>,
    pub shots_dir: Option<PathBuf>,
    pub server_target: Option<String>,
}

impl Default for StandaloneOptions {
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

/// Top-level CLI dispatch target.
#[derive(Debug)]
pub enum CliDispatch {
    /// Print help text to stdout and exit 0 immediately.
    Help(String),
    /// Print version string to stdout and exit 0 immediately.
    Version(String),
    /// Run the standalone full native game (`App`).
    Standalone(StandaloneOptions),
    /// Run the legacy `--local` battle host.
    LegacyLocal(Vec<String>),
    /// Run the legacy `--join` online battle host.
    LegacyJoin(Vec<String>),
    /// Run the legacy `--map` or `SESSION_DIR` static/replay viewer.
    LegacyWindow(Vec<String>),
}

/// Parses command-line arguments from an iterator (excluding `argv[0]`) and
/// returns the selected [`CliDispatch`] without touching any OS window, audio,
/// or OpenGL state.
pub fn parse_args<I>(args: I) -> Result<CliDispatch, String>
where
    I: IntoIterator<Item = String>,
{
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

    // Dispatch legacy sub-modes when their selector flags are present.
    if raw.iter().any(|a| a == "--local") {
        return Ok(CliDispatch::LegacyLocal(raw));
    }
    if raw.iter().any(|a| a == "--join") {
        return Ok(CliDispatch::LegacyJoin(raw));
    }
    if raw.iter().any(|a| a == "--map") {
        return Ok(CliDispatch::LegacyWindow(raw));
    }

    // Check if a positional argument is a recorded session or legacy capture directory.
    if raw.iter().any(|a| {
        if a.starts_with('-') {
            return false;
        }
        let p = std::path::Path::new(a);
        p.join("session.json").is_file() || p.join("calls.json").is_file()
    }) {
        return Ok(CliDispatch::LegacyWindow(raw));
    }

    let mut opts = StandaloneOptions {
        content_dir: std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from),
        ..StandaloneOptions::default()
    };

    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--content-dir" => {
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
                opts.audio = false;
            }
            "--desktop-gl" => {
                opts.desktop_gl = true;
            }
            "--fullscreen" => {
                opts.fullscreen = true;
            }
            "--skip-update-check" => {
                opts.skip_update_check = true;
            }
            "--width" => {
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
                let s = it.next().ok_or("missing value for --script")?;
                opts.script = Some(s);
            }
            "--shots-dir" => {
                let d = it.next().ok_or("missing value for --shots-dir")?;
                opts.shots_dir = Some(PathBuf::from(d));
            }
            "--server" => {
                let t = it.next().ok_or("missing value for --server")?;
                opts.server_target = Some(t);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}\n\n{HELP_TEXT}"));
            }
            positional => {
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

    #[test]
    fn unknown_flag_returns_err() {
        let err = parse_args(vec!["--bogus-flag".to_string()]).unwrap_err();
        assert!(err.contains("unknown option: --bogus-flag"));
    }

    #[test]
    fn default_no_args_selects_standalone() {
        let res = parse_args(Vec::<String>::new()).expect("standalone");
        assert!(matches!(res, CliDispatch::Standalone(_)));
    }

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
