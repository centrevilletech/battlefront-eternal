//! Linux: an AppImage registers itself with the desktop on every start, so app
//! launchers list Battlefront Eternal and `battlefront` starts it from a
//! terminal. Only what is missing or stale is rewritten, and a `battlefront`
//! command this app did not create is left alone. The files and their names
//! are the ones earlier releases wrote, so a player's existing entry is
//! simply brought up to date. `BFE_NO_DESKTOP_INTEGRATION=1` turns it off.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DISPLAY_NAME: &str = "Battlefront Eternal";
const ID: &str = "battlefront-eternal";
const MARKER: &str = "# Created by Battlefront Eternal";

/// What one registration changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub icon: bool,
    pub entry: bool,
    /// `None` when a `battlefront` command of someone else's was left alone.
    pub command: Option<bool>,
}

/// The desktop entry that starts `executable` (the AppImage).
pub fn desktop_entry(executable: &Path) -> String {
    let mut quoted = String::new();
    for ch in executable.to_string_lossy().chars() {
        if matches!(ch, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    [
        "[Desktop Entry]".to_string(),
        "Type=Application".to_string(),
        format!("Name={DISPLAY_NAME}"),
        "GenericName=Star Wars Battlefront".to_string(),
        "Comment=Star Wars: Battlefront (2004) on a new engine".to_string(),
        format!("Exec=\"{quoted}\" %U"),
        format!("Icon={ID}"),
        "Terminal=false".to_string(),
        "Categories=Game;ActionGame;".to_string(),
        "Keywords=battlefront;star wars;swbf;bfe;".to_string(),
        format!("StartupWMClass={ID}"),
        "X-AppImage-Integrate=false".to_string(),
        String::new(),
    ]
    .join("\n")
}

/// The `battlefront` command: the newest update the app installed in the
/// user's folder when there is one (the updater keeps `current` pointing at
/// it), else the AppImage.
pub fn launcher_script(executable: &Path) -> String {
    let quoted = executable.to_string_lossy().replace('\'', "'\\''");
    [
        "#!/bin/sh".to_string(),
        format!("{MARKER}; it follows the AppImage when it moves."),
        "current=\"${XDG_DATA_HOME:-$HOME/.local/share}/battlefront-eternal/current/battlefront-eternal\"".to_string(),
        "if [ -x \"$current\" ]; then exec \"$current\" \"$@\"; fi".to_string(),
        format!("exec '{quoted}' \"$@\""),
        String::new(),
    ]
    .join("\n")
}

fn write_if_changed(
    file: &Path, content: &[u8], mode: u32, before_rename: impl FnOnce() -> io::Result<()>,
) -> io::Result<bool> {
    if fs::read(file).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = file.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    if let Err(e) = before_rename().and_then(|()| fs::rename(&temp, file)) {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(true)
}

/// Registers `appimage` with the desktop under `data` (XDG data home) and
/// `bin` (`~/.local/bin`), with `icon` (a PNG) as its icon.
pub fn integrate(appimage: &Path, icon: &Path, data: &Path, bin: &Path) -> io::Result<Outcome> {
    let icon_bytes = fs::read(icon)?;
    let icon = write_if_changed(
        &data.join("icons/hicolor/512x512/apps").join(format!("{ID}.png")),
        &icon_bytes,
        0o644,
        || Ok(()),
    )?;
    let entry = write_if_changed(
        &data.join("applications").join(format!("{ID}.desktop")),
        desktop_entry(appimage).as_bytes(),
        0o755,
        || Ok(()),
    )?;
    let command_file = bin.join("battlefront");
    let command = match fs::read_to_string(&command_file) {
        Ok(existing) if !existing.contains(MARKER) => None,
        _ => Some(write_if_changed(&command_file, launcher_script(appimage).as_bytes(), 0o755, || {
            bfe_storage::preserve_unreadable_text(&command_file, |_| true).map(|_| ())
        })?),
    };
    Ok(Outcome { icon, entry, command })
}

/// The AppImage this process belongs to, its icon, and where to register it,
/// read from the environment; `None` when not run from an AppImage or when
/// integration is turned off.
pub fn target_from_env(env: &BTreeMap<String, String>, exe: Option<&Path>) -> Option<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    if env.get("BFE_NO_DESKTOP_INTEGRATION").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    // An installed update runs outside the AppImage; the launcher passes the AppImage on as BFE_APPIMAGE.
    let appimage = ["BFE_APPIMAGE", "APPIMAGE"]
        .iter()
        .find_map(|key| env.get(*key).filter(|v| !v.is_empty()))
        .map(PathBuf::from)?;
    let icon = exe?.parent()?.join("resources/assets/icon-512.png");
    let home = PathBuf::from(env.get("HOME").filter(|v| !v.is_empty())?);
    let data = env
        .get("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    Some((appimage, icon, data.clone(), home.join(".local/bin")))
}

/// Registers the running AppImage, on a background thread so startup never
/// waits on the disk; refreshes the desktop's caches when something changed.
pub fn integrate_in_background() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let exe = std::env::current_exe().ok();
    let Some((appimage, icon, data, bin)) = target_from_env(&env, exe.as_deref()) else {
        return;
    };
    let _ = std::thread::Builder::new().name("desktop-integration".into()).spawn(move || {
        match integrate(&appimage, &icon, &data, &bin) {
            Ok(outcome) => {
                // Missing tools are fine: most launchers watch the folders themselves.
                if outcome.entry {
                    let _ = Command::new("update-desktop-database").arg(data.join("applications")).status();
                }
                if outcome.icon {
                    let _ = Command::new("gtk-update-icon-cache").arg("-q").arg("-t").arg(data.join("icons/hicolor")).status();
                }
            }
            Err(err) => eprintln!("desktop integration: {err}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_once_and_leaves_a_foreign_command_alone() {
        let root = std::env::temp_dir().join(format!("bfe-linux-integration-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (data, bin) = (root.join("share"), root.join("bin"));
        fs::create_dir_all(&root).unwrap();
        let icon = root.join("icon.png");
        fs::write(&icon, b"png").unwrap();
        let appimage = root.join("My \"Games\"/Battlefront-Eternal.AppImage");

        let first = integrate(&appimage, &icon, &data, &bin).unwrap();
        assert_eq!(first, Outcome { icon: true, entry: true, command: Some(true) });
        let entry = fs::read_to_string(data.join("applications/battlefront-eternal.desktop")).unwrap();
        assert!(entry.contains("Exec=\"") && entry.contains("My \\\"Games\\\"/Battlefront-Eternal.AppImage\" %U"));
        assert!(fs::read_to_string(bin.join("battlefront")).unwrap().contains(MARKER));
        assert_eq!(integrate(&appimage, &icon, &data, &bin).unwrap(), Outcome { icon: false, entry: false, command: Some(false) });

        fs::write(bin.join("battlefront"), "#!/bin/sh\necho mine\n").unwrap();
        let moved = root.join("Battlefront-Eternal.AppImage");
        assert_eq!(integrate(&moved, &icon, &data, &bin).unwrap(), Outcome { icon: false, entry: true, command: None });
        assert_eq!(fs::read_to_string(bin.join("battlefront")).unwrap(), "#!/bin/sh\necho mine\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn only_an_appimage_registers() {
        let exe = Path::new("/apps/bfe/bfe-client");
        let mut env = BTreeMap::from([("HOME".to_string(), "/home/p".to_string())]);
        assert!(target_from_env(&env, Some(exe)).is_none());
        env.insert("APPIMAGE".into(), "/home/p/BFE.AppImage".into());
        let (appimage, icon, data, bin) = target_from_env(&env, Some(exe)).unwrap();
        assert_eq!(appimage, Path::new("/home/p/BFE.AppImage"));
        assert_eq!(icon, Path::new("/apps/bfe/resources/assets/icon-512.png"));
        assert_eq!((data.as_path(), bin.as_path()), (Path::new("/home/p/.local/share"), Path::new("/home/p/.local/bin")));
        env.insert("BFE_NO_DESKTOP_INTEGRATION".into(), "1".into());
        assert!(target_from_env(&env, Some(exe)).is_none());
    }
}
