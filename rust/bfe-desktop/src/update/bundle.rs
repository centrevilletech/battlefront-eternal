//! macOS: keeping the installed app bundle current.
//!
//! An update is installed beside the app the player put in /Applications (or wherever it
//! is) and started from there by the hand-off, as on the other systems. On macOS the
//! installed bundle is then replaced by the new version once that version has proven
//! healthy, so the app in Finder and the Dock is the one that runs and later starts need no
//! hand-off.
//!
//! The copy is made beside the installed bundle, checked with `codesign`, and exchanged
//! with it in one step. Its files are those of the signed release byte for byte, so its
//! signature and its notarization ticket stay valid, and none of them carries the
//! quarantine mark of a download. Where the bundle cannot be replaced (no permission, a
//! read-only location, another copy still running from it) nothing changes and the
//! hand-off keeps working at every start.

use std::fs;
use std::path::{Path, PathBuf};

use crate::launch::read_release_info;
use crate::paths::normalize_lexical;
use crate::update::install::clear_quarantine;
use crate::update::state::versions_dir;
use crate::update::version::is_newer;

/// Records the version whose replacement failed, so one refusal (a system that does not
/// let this user change the app) is logged once and not tried again at every start.
const REFUSAL_RECORD: &str = "bundle-replace.json";

/// The app bundle that holds `own_dir` (`<Name>.app/Contents/MacOS`), if it is in one.
pub fn bundle_of(own_dir: &Path) -> Option<PathBuf> {
    if own_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents = own_dir.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let bundle = contents.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// What [`replace_installed_bundle`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceOutcome {
    /// The installed bundle at this path now holds the running version.
    Replaced(PathBuf),
    /// Nothing to do or not possible now; tried again at a later start.
    Skipped(&'static str),
    /// Refused or failed; recorded, and not tried again for this version.
    Failed(String),
}

/// Inputs to [`replace_installed_bundle`].
#[derive(Debug, Clone)]
pub struct ReplaceOptions {
    /// Update root (`versions/` lives inside).
    pub root: PathBuf,
    /// Folder of the running executable (`.../<Name>.app/Contents/MacOS`).
    pub own_dir: PathBuf,
    /// Version of the running build, which has just been recorded healthy.
    pub own_version: String,
    /// Folder of the installed app's executable (`BFE_BOOTSTRAP_DIR`).
    pub installed_dir: PathBuf,
    /// Check the copy with `codesign` before it replaces the installed bundle.
    pub verify_signature: bool,
}

/// Replaces the installed app bundle with the running, healthy version from the update
/// folder. See the module documentation.
pub fn replace_installed_bundle(opts: &ReplaceOptions) -> ReplaceOutcome {
    let Some(own_bundle) = bundle_of(&opts.own_dir) else {
        return ReplaceOutcome::Skipped("not running from an app bundle");
    };
    let version_dir = normalize_lexical(&versions_dir(&opts.root).join(&opts.own_version));
    if !normalize_lexical(&own_bundle).starts_with(&version_dir) {
        return ReplaceOutcome::Skipped("not running from the update folder");
    }
    let Some(installed) = bundle_of(&opts.installed_dir) else {
        return ReplaceOutcome::Skipped("the installed copy is not an app bundle");
    };
    if normalize_lexical(&installed).starts_with(normalize_lexical(&opts.root)) {
        return ReplaceOutcome::Skipped("started from the update folder");
    }
    // The player may have installed something newer by hand since the hand-off.
    match read_release_info(&opts.installed_dir).version {
        Some(installed_version) if is_newer(&opts.own_version, &installed_version) => {}
        _ => return ReplaceOutcome::Skipped("the installed app is not older"),
    }
    let refusal = opts.root.join(REFUSAL_RECORD);
    if fs::read_to_string(&refusal).is_ok_and(|text| text.contains(&format!("\"{}\"", opts.own_version))) {
        return ReplaceOutcome::Skipped("refused before for this version");
    }
    // A downloaded app that was never moved runs from a read-only copy the system made.
    if installed.components().any(|part| part.as_os_str() == "AppTranslocation") {
        return ReplaceOutcome::Skipped("the app runs from a quarantine location");
    }
    match programs_running_from(&installed) {
        Some(false) => {}
        _ => return ReplaceOutcome::Skipped("the installed app is in use"),
    }

    let result = (|| -> Result<(), String> {
        let parent = installed.parent().ok_or("the installed app has no folder")?;
        let name = installed.file_name().and_then(|n| n.to_str()).ok_or("unreadable app name")?;
        let staged = parent.join(format!(".{name}.bfe-update"));
        let _ = fs::remove_dir_all(&staged);
        let outcome = copy_tree(&own_bundle, &staged)
            .map_err(|e| format!("cannot write beside the installed app: {e}"))
            .and_then(|()| {
                if opts.verify_signature {
                    verify_signature(&staged)?;
                }
                exchange(&staged, &installed).map_err(|e| format!("cannot replace the installed app: {e}"))
            });
        // After the exchange `staged` holds the old bundle; before it, the unused copy.
        let _ = fs::remove_dir_all(&staged);
        outcome
    })();

    match result {
        Ok(()) => ReplaceOutcome::Replaced(installed),
        Err(reason) => {
            let _ = fs::write(&refusal, format!("{{\"version\":\"{}\"}}\n", opts.own_version));
            ReplaceOutcome::Failed(reason)
        }
    }
}

/// Copies a bundle: folders, files with their permissions, and links as links.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (source, dest) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(&source)?, &dest)?;
        } else if kind.is_dir() {
            copy_tree(&source, &dest)?;
        } else {
            // On APFS this shares the file's blocks with the source: no time, no space.
            fs::copy(&source, &dest)?;
            clear_quarantine(&dest);
        }
    }
    Ok(())
}

fn verify_signature(bundle: &Path) -> Result<(), String> {
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(bundle)
        .output()
        .map_err(|e| format!("cannot run codesign: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "the copy's signature does not verify: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

/// Exchanges two folders of one volume in a single step: at no moment is there no app at
/// the installed path, and a failure leaves both as they were.
#[cfg(target_os = "macos")]
fn exchange(a: &Path, b: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let to_c = |path: &Path| std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(std::io::Error::other);
    let (c_a, c_b) = (to_c(a)?, to_c(b)?);
    // SAFETY: both pointers are valid NUL-terminated paths for the duration of the call.
    if unsafe { libc::renamex_np(c_a.as_ptr(), c_b.as_ptr(), libc::RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn exchange(_a: &Path, _b: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("only macOS installs app bundles"))
}

/// Whether any process runs a program inside `bundle`; `None` when that cannot be told.
#[cfg(target_os = "macos")]
fn programs_running_from(bundle: &Path) -> Option<bool> {
    use std::os::unix::ffi::OsStrExt;
    let bundle = fs::canonicalize(bundle).ok()?;
    // SAFETY: a null buffer asks for the number of processes only.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return None;
    }
    let mut pids = vec![0 as libc::pid_t; count as usize + 64];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: the buffer is `bytes` long and outlives the call.
    let found = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if found <= 0 {
        return None;
    }
    let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    for pid in &pids[..(found as usize).min(pids.len())] {
        // SAFETY: the buffer is as long as the size passed and outlives the call.
        let len = unsafe { libc::proc_pidpath(*pid, path.as_mut_ptr().cast(), path.len() as u32) };
        if len > 0 && Path::new(std::ffi::OsStr::from_bytes(&path[..len as usize])).starts_with(&bundle) {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(not(target_os = "macos"))]
fn programs_running_from(_bundle: &Path) -> Option<bool> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(label: &str) -> PathBuf {
        let mut nonce = [0u8; 6];
        let _ = getrandom::getrandom(&mut nonce);
        let hex: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("bfe-bundle-test-{label}-{hex}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A bundle as build/native/package-macos.py lays it out, holding `version`.
    fn write_bundle(parent: &Path, version: &str) -> PathBuf {
        let bundle = parent.join("Battlefront Eternal.app");
        let macos = bundle.join("Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        fs::write(macos.join("bfe-client"), format!("client {version}")).unwrap();
        fs::set_permissions(macos.join("bfe-client"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("../Resources", macos.join("resources")).unwrap();
        fs::write(
            bundle.join("Contents/Resources/bfe-release.json"),
            format!(r#"{{"version":"{version}","channel":"development-preview"}}"#),
        )
        .unwrap();
        macos
    }

    #[test]
    fn finds_the_bundle_of_a_program_and_copies_it_whole() {
        let work = temp_dir("copy");
        let macos = write_bundle(&work.join("a"), "0.1.0-preview.2");
        let bundle = bundle_of(&macos).unwrap();
        assert_eq!(bundle, work.join("a/Battlefront Eternal.app"));
        assert_eq!(bundle_of(&work), None);
        assert_eq!(bundle_of(Path::new("/opt/Battlefront Eternal")), None);

        let copy = work.join("b.app");
        copy_tree(&bundle, &copy).unwrap();
        assert_eq!(fs::read_link(copy.join("Contents/MacOS/resources")).unwrap(), Path::new("../Resources"));
        let client = copy.join("Contents/MacOS/bfe-client");
        assert_eq!(fs::read_to_string(&client).unwrap(), "client 0.1.0-preview.2");
        assert_eq!(fs::metadata(&client).unwrap().permissions().mode() & 0o777, 0o755);
        // The release record is read through the link, as the programs read it.
        assert_eq!(
            read_release_info(&copy.join("Contents/MacOS")).version.as_deref(),
            Some("0.1.0-preview.2")
        );
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn a_mac_follows_its_own_channel_entry_and_keeps_updates_beside_its_settings() {
        use crate::update::state::{platform_key, resolve_update_root};
        use std::collections::HashMap;
        // What the running program reports (std::env::consts) and the release feed's names.
        assert_eq!(platform_key("macos", "aarch64"), Some("darwin-arm64"));
        assert_eq!(platform_key("darwin", "x64"), Some("darwin-x64"));
        let home = Path::new("/Users/player");
        assert_eq!(
            resolve_update_root("macos", &HashMap::new(), home),
            Some(home.join("Library/Application Support/Battlefront Eternal/updates"))
        );
    }

    #[test]
    fn leaves_the_installed_app_alone_unless_it_is_older_and_the_update_is_running() {
        let work = temp_dir("skip");
        let root = work.join("updates");
        let own = write_bundle(&versions_dir(&root).join("0.1.0-preview.2"), "0.1.0-preview.2");
        let newer = write_bundle(&work.join("Applications"), "0.1.0-preview.3");
        let mut opts = ReplaceOptions {
            root: root.clone(),
            own_dir: own.clone(),
            own_version: "0.1.0-preview.2".to_string(),
            installed_dir: newer.clone(),
            verify_signature: false,
        };
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Skipped("the installed app is not older"));
        // The installed app itself never replaces anything.
        opts.own_dir = newer.clone();
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Skipped("not running from the update folder"));
        assert_eq!(fs::read_to_string(newer.join("bfe-client")).unwrap(), "client 0.1.0-preview.3");
        let _ = fs::remove_dir_all(&work);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replaces_an_older_installed_app_in_one_step() {
        let work = temp_dir("replace");
        let root = work.join("updates");
        let own = write_bundle(&versions_dir(&root).join("0.1.0-preview.2"), "0.1.0-preview.2");
        let installed = write_bundle(&work.join("Applications"), "0.1.0-preview.1");
        let opts = ReplaceOptions {
            root: root.clone(),
            own_dir: own,
            own_version: "0.1.0-preview.2".to_string(),
            installed_dir: installed.clone(),
            verify_signature: false,
        };
        let bundle = bundle_of(&installed).unwrap();
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Replaced(bundle));
        assert_eq!(fs::read_to_string(installed.join("bfe-client")).unwrap(), "client 0.1.0-preview.2");
        // Nothing of the exchange is left beside the app, and a second run has nothing to do.
        let left: Vec<_> = fs::read_dir(work.join("Applications")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("Battlefront Eternal.app")]);
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Skipped("the installed app is not older"));
        let _ = fs::remove_dir_all(&work);
    }
}
