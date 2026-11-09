//! Linux AppImage desktop integration (`~/.local/share/applications/battlefront-eternal.desktop`,
//! hicolor icon installation, and `~/.local/bin/battlefront` launcher scripts).
//!
//! Mirrors the desktop host (`DISPLAY_NAME`, `desktopEntry`,
//! `launcherScript`, `updatingLauncher`, `adoptInstallerLauncher`,
//! `integrateLinuxDesktop`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// Human-readable application name written into `battlefront-eternal.desktop`.
///
/// Mirrors `DISPLAY_NAME` in the desktop host.
pub const DISPLAY_NAME: &str = "Battlefront Eternal";

/// Desktop entry and icon base identifier (`"battlefront-eternal"`).
///
/// Mirrors `ID` in the desktop host.
pub const ID: &str = "battlefront-eternal";

/// Ownership marker comment placed in `~/.local/bin/battlefront` so a foreign
/// `battlefront` binary or script installed by another package is never overwritten.
///
/// Mirrors `MARKER` in the desktop host.
pub const MARKER: &str = "# Created by Battlefront Eternal";

/// Ownership marker comment placed in `~/.local/bin/battlefront-eternal` when
/// adopting the `install.sh` wrapper script.
///
/// Mirrors `UPDATING_MARKER` in the desktop host.
pub const UPDATING_MARKER: &str =
    "# Battlefront Eternal: the newest installed update, else the AppImage.";

/// Shell snippet resolving the `current` symlink maintained by the desktop host.
///
/// Mirrors `CURRENT` in the desktop host.
const CURRENT: &str =
    "current=\"${XDG_DATA_HOME:-$HOME/.local/share}/battlefront-eternal/current/battlefront-eternal\"";

/// Shell snippet that launches `$current` directly (adding `--no-sandbox` when
/// unprivileged user namespaces are unavailable).
///
/// Mirrors `RUN_CURRENT` in the desktop host.
const RUN_CURRENT: &[&str] = &[
    "if [ -x \"$current\" ]; then",
    "  unshare -Ur true 2>/dev/null || set -- --no-sandbox \"$@\"",
    "  exec \"$current\" \"$@\"",
    "fi",
];

/// Formats the contents of `battlefront-eternal.desktop` for `executable`.
///
/// Escapes `"`, `` ` ``, `$`, and `\` inside the double-quoted `Exec=` argument
/// identically to `desktopEntry` in the desktop host.
pub fn desktop_entry(executable: &str) -> String {
    let mut escaped = String::with_capacity(executable.len() + 4);
    for ch in executable.chars() {
        if matches!(ch, '"' | '`' | '$' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    let exec = format!("\"{}\"", escaped);
    [
        "[Desktop Entry]",
        "Type=Application",
        &format!("Name={}", DISPLAY_NAME),
        "GenericName=Star Wars Battlefront",
        "Comment=Star Wars: Battlefront (2004) on a new engine",
        &format!("Exec={} %U", exec),
        &format!("Icon={}", ID),
        "Terminal=false",
        "Categories=Game;ActionGame;",
        "Keywords=battlefront;star wars;swbf;bfe;",
        &format!("StartupWMClass={}", ID),
        "X-AppImage-Integrate=false",
        "",
    ]
    .join("\n")
}

/// Formats the `~/.local/bin/battlefront` POSIX shell launcher script for `executable`.
///
/// Execs the newest installed update under `${XDG_DATA_HOME:-$HOME/.local/share}/battlefront-eternal/current`
/// when present, falling back to `executable`.
///
/// Mirrors `launcherScript` in the desktop host.
pub fn launcher_script(executable: &str) -> String {
    let single_quoted = executable.replace('\'', "'\\''");
    let mut lines = Vec::with_capacity(8);
    lines.push("#!/bin/sh".to_string());
    lines.push(format!("{}; it follows the AppImage when it moves.", MARKER));
    lines.push(CURRENT.to_string());
    for &line in RUN_CURRENT {
        lines.push(line.to_string());
    }
    lines.push(format!("exec '{}' \"$@\"", single_quoted));
    lines.push(String::new());
    lines.join("\n")
}

/// Formats the `~/.local/bin/battlefront-eternal` bash launcher that prefers
/// the newest installed update before falling back to `exec <command> --appimage-extract-and-run "$@"`.
///
/// Mirrors `updatingLauncher` in the desktop host.
pub fn updating_launcher(command: &str) -> String {
    let mut lines = Vec::with_capacity(9);
    lines.push("#!/usr/bin/env bash".to_string());
    lines.push(UPDATING_MARKER.to_string());
    lines.push("export BFE_NO_DESKTOP_INTEGRATION=1".to_string());
    lines.push(CURRENT.to_string());
    for &line in RUN_CURRENT {
        lines.push(line.to_string());
    }
    lines.push(format!("exec {} --appimage-extract-and-run \"$@\"", command));
    lines.push(String::new());
    lines.join("\n")
}

/// Extracts the quoted AppImage command from an `install.sh`-created or
/// previously adopted `~/.local/bin/battlefront-eternal` script.
fn parse_installer_launcher_command(existing: &str) -> Option<String> {
    // Match exact `install.sh` 3-line script:
    // #!/usr/bin/env bash\nexport BFE_NO_DESKTOP_INTEGRATION=1\nexec (.+) --appimage-extract-and-run "$@"\n
    if let Some(rest) = existing.strip_prefix("#!/usr/bin/env bash\nexport BFE_NO_DESKTOP_INTEGRATION=1\nexec ") {
        if let Some(cmd) = rest.strip_suffix(" --appimage-extract-and-run \"$@\"\n") {
            if !cmd.is_empty() && !cmd.contains('\n') {
                return Some(cmd.to_string());
            }
        }
    }
    // Or an already-adopted updatingLauncher script containing UPDATING_MARKER.
    if existing.contains(UPDATING_MARKER) {
        for line in existing.lines() {
            if let Some(rest) = line.strip_prefix("exec ") {
                if let Some(cmd) = rest.strip_suffix(" --appimage-extract-and-run \"$@\"") {
                    if !cmd.is_empty() {
                        return Some(cmd.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Writes `content` to `file` with `mode` via a PID-tagged temporary file only
/// when `file` is missing or its bytes differ from `content`.
///
/// Mirrors `writeIfChanged` in the desktop host.
pub fn write_if_changed(file: &Path, content: &[u8], mode: u32) -> Result<bool, String> {
    if let Ok(current) = fs::read(file) {
        if current == content {
            return Ok(false);
        }
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = PathBuf::from(format!("{}.{}.tmp", file.display(), std::process::id()));
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&tmp) {
            let mut perms = meta.permissions();
            perms.set_mode(mode);
            let _ = fs::set_permissions(&tmp, perms);
        }
    }
    #[cfg(not(unix))]
    let _ = mode;
    fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Upgrades `~/.local/bin/battlefront-eternal` written by `install.sh` so it
/// starts the newest installed update directly instead of unpacking the AppImage
/// on every launch.
///
/// Returns `"missing"`, `"foreign"`, `"updated"`, or `"current"`.
/// Mirrors `adoptInstallerLauncher` in the desktop host.
pub fn adopt_installer_launcher(home: &Path) -> Result<&'static str, String> {
    let file = home.join(".local").join("bin").join("battlefront-eternal");
    let Ok(existing) = fs::read_to_string(&file) else {
        return Ok("missing");
    };
    let Some(command) = parse_installer_launcher_command(&existing) else {
        return Ok("foreign");
    };
    let next = updating_launcher(&command);
    if write_if_changed(&file, next.as_bytes(), 0o755)? {
        Ok("updated")
    } else {
        Ok("current")
    }
}

/// Options for [`integrate_linux_desktop`].
#[derive(Debug, Clone)]
pub struct IntegrateLinuxDesktopOptions {
    /// Executable or AppImage path to register.
    pub executable: Option<PathBuf>,
    /// Path to the 512x512 PNG icon file.
    pub icon: PathBuf,
    /// User's home directory (`~`).
    pub home: PathBuf,
    /// Environment variables (`BFE_NO_DESKTOP_INTEGRATION`, `XDG_DATA_HOME`).
    pub env: BTreeMap<String, String>,
    /// Whether to spawn `update-desktop-database` and `gtk-update-icon-cache`.
    pub update_caches: bool,
}

/// Outcome of `~/.local/bin/battlefront` launcher registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherCommandStatus {
    /// File was created or updated.
    Updated,
    /// File already matched `launcher_script`.
    Current,
    /// File belongs to another program (lacks [`MARKER`]) and was left untouched.
    Foreign,
}

/// Result returned by [`integrate_linux_desktop`].
///
/// Mirrors the return object of `integrateLinuxDesktop` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrateLinuxDesktopResult {
    /// True when `executable` is missing or `BFE_NO_DESKTOP_INTEGRATION` is set.
    pub skipped: bool,
    /// True if the hicolor PNG icon was written or updated.
    pub icon: bool,
    /// True if `battlefront-eternal.desktop` was written or updated.
    pub entry: bool,
    /// Status of `~/.local/bin/battlefront`.
    pub command: Option<LauncherCommandStatus>,
}

/// Registers the AppImage with the Linux desktop environment (`battlefront-eternal.png`,
/// `battlefront-eternal.desktop`, and `~/.local/bin/battlefront`), rewriting only
/// files that are missing or stale and leaving any foreign `battlefront` command untouched.
///
/// Mirrors `integrateLinuxDesktop` in the desktop host.
pub fn integrate_linux_desktop(
    opts: &IntegrateLinuxDesktopOptions,
) -> Result<IntegrateLinuxDesktopResult, String> {
    let Some(executable) = opts
        .executable
        .as_ref()
        .filter(|p| !p.as_os_str().is_empty())
    else {
        return Ok(IntegrateLinuxDesktopResult {
            skipped: true,
            icon: false,
            entry: false,
            command: None,
        });
    };
    if opts
        .env
        .get("BFE_NO_DESKTOP_INTEGRATION")
        .filter(|v| !v.is_empty())
        .is_some()
    {
        return Ok(IntegrateLinuxDesktopResult {
            skipped: true,
            icon: false,
            entry: false,
            command: None,
        });
    }

    let data = opts
        .env
        .get("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| opts.home.join(".local").join("share"));
    let applications = data.join("applications");

    let icon_bytes = fs::read(&opts.icon).map_err(|e| e.to_string())?;
    let icon_dest = data
        .join("icons")
        .join("hicolor")
        .join("512x512")
        .join("apps")
        .join(format!("{}.png", ID));
    let icon_changed = write_if_changed(&icon_dest, &icon_bytes, 0o644)?;

    let exec_str = executable.to_string_lossy();
    let desktop_dest = applications.join(format!("{}.desktop", ID));
    let entry_changed = write_if_changed(
        &desktop_dest,
        desktop_entry(&exec_str).as_bytes(),
        0o755,
    )?;

    let command_path = opts.home.join(".local").join("bin").join("battlefront");
    let existing = fs::read_to_string(&command_path).ok();
    let command_status = if existing.as_ref().is_none_or(|s| s.contains(MARKER)) {
        let updated = write_if_changed(
            &command_path,
            launcher_script(&exec_str).as_bytes(),
            0o755,
        )?;
        if updated {
            LauncherCommandStatus::Updated
        } else {
            LauncherCommandStatus::Current
        }
    } else {
        LauncherCommandStatus::Foreign
    };

    if opts.update_caches {
        if entry_changed {
            let _ = Command::new("update-desktop-database")
                .arg(&applications)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        if icon_changed {
            let _ = Command::new("gtk-update-icon-cache")
                .args(["-q", "-t"])
                .arg(data.join("icons").join("hicolor"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    Ok(IntegrateLinuxDesktopResult {
        skipped: false,
        icon: icon_changed,
        entry: entry_changed,
        command: Some(command_status),
    })
}
