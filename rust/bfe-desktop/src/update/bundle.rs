//! macOS: keeping the installed app bundle current.
//!
//! An update is installed beside the app the player put in /Applications (or wherever it
//! is) and started from there by the hand-off, as on the other systems. On macOS the
//! installed bundle is then replaced by the new version once that version has proven
//! healthy, so the app in Finder and the Dock is the one that runs and later starts need no
//! hand-off.
//!
//! The copy is made beside the installed bundle, checked with `codesign` (every file as
//! the signature seals it, and signed by whoever signed the installed app: `signer.rs`),
//! written to the disk, and exchanged with the installed bundle in one step. Its files are
//! those of the signed release byte for byte, so its signature and its notarization ticket
//! stay valid, and none of them carries the quarantine mark of a download. The bundle that
//! was replaced is put aside under a hidden name and removed at a later start, so that a
//! copy of the game started from it at that very moment keeps its files. Where the bundle
//! cannot be replaced (no permission, a read-only location, another copy still running
//! from it) nothing changes and the hand-off keeps working at every start.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::launch::read_release_info;
use crate::paths::normalize_lexical;
use crate::update::install::clear_quarantine;
use crate::update::signer::{check_bundle, signer_of};
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
    /// Check the copy with `codesign` before it replaces the installed bundle: its seal, and
    /// that whoever signed the installed app signed it. Always on outside tests.
    pub verify_signature: bool,
}

/// The names beside an installed bundle that a replacement uses: the copy before the
/// exchange, and the replaced bundle after it.
fn beside(installed: &Path) -> Option<(PathBuf, PathBuf)> {
    let parent = installed.parent()?;
    let name = installed.file_name()?.to_str()?;
    Some((parent.join(format!(".{name}.bfe-update")), parent.join(format!(".{name}.bfe-old"))))
}

/// The bundle identifier in an app's Info.plist (the XML form build/native/package-macos.py
/// writes).
fn bundle_identifier(bundle: &Path) -> Option<String> {
    let text = fs::read_to_string(bundle.join("Contents/Info.plist")).ok()?;
    let after = text.split_once("<key>CFBundleIdentifier</key>")?.1;
    let value = after.split_once("<string>")?.1.split_once("</string>")?.0.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// The one app bundle a version may consist of, from the path of its program inside the
/// version folder: `<Name>.app` when that path is `<Name>.app/Contents/MacOS/<program>` and
/// nothing else.
///
/// [`bundle_of`] finds the bundle around a program wherever it is, which is right for the
/// app a player installed. For a version that arrived as an update it is not: a program
/// inside a bundle that is nested in another bundle's resources would have only that inner
/// bundle around it, and a check of the inner bundle's signature says nothing about the
/// files of the outer one.
pub fn top_level_app(program: &Path) -> Option<&std::ffi::OsStr> {
    let mut parts = program.components();
    let shape = (parts.next()?, parts.next()?, parts.next()?, parts.next()?, parts.next());
    let (Component::Normal(app), Component::Normal(contents), Component::Normal(macos), Component::Normal(_), None) = shape
    else {
        return None;
    };
    (Path::new(app).extension()? == "app" && contents == "Contents" && macos == "MacOS").then_some(app)
}

/// The app bundle of an installed version: the version folder's own `<Name>.app`, with the
/// version's program directly in its `Contents/MacOS` ([`top_level_app`]).
pub fn version_bundle(version_dir: &Path, executable: &Path) -> Option<PathBuf> {
    let dir = normalize_lexical(version_dir);
    let program = normalize_lexical(executable);
    Some(dir.join(top_level_app(program.strip_prefix(&dir).ok()?)?))
}

/// Replaces the installed app bundle with the running, healthy version from the update
/// folder. See the module documentation.
pub fn replace_installed_bundle(opts: &ReplaceOptions) -> ReplaceOutcome {
    let Some(own_bundle) = bundle_of(&opts.own_dir) else {
        return ReplaceOutcome::Skipped("not running from an app bundle");
    };
    // This version's own bundle, directly in its version folder, and nothing nested in it.
    let version_dir = normalize_lexical(&versions_dir(&opts.root).join(&opts.own_version));
    if normalize_lexical(&own_bundle).parent() != Some(version_dir.as_path()) {
        return ReplaceOutcome::Skipped("not running from the update folder");
    }
    let Some(installed) = bundle_of(&opts.installed_dir) else {
        return ReplaceOutcome::Skipped("the installed copy is not an app bundle");
    };
    if normalize_lexical(&installed).starts_with(normalize_lexical(&opts.root)) {
        return ReplaceOutcome::Skipped("started from the update folder");
    }
    // The installed path arrives in the environment of the hand-off. Whatever it names, only
    // a copy of this very app is ever replaced.
    match (bundle_identifier(&own_bundle), bundle_identifier(&installed)) {
        (Some(own), Some(theirs)) if own == theirs => {}
        _ => return ReplaceOutcome::Skipped("the installed copy is another app"),
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
    let Some((staged, replaced)) = beside(&installed) else {
        return ReplaceOutcome::Skipped("the installed app has no folder");
    };
    // What an earlier replacement put aside has to be gone before this one puts its own there.
    if replaced.exists() {
        if programs_running_from(&replaced) != Some(false) {
            return ReplaceOutcome::Skipped("the app replaced earlier is in use");
        }
        let _ = fs::remove_dir_all(&replaced);
    }

    let result = (|| -> Result<(), String> {
        // Whoever signed the installed app has to have signed what takes its place.
        let signer = if opts.verify_signature { Some(signer_of(&installed)?) } else { None };
        let _ = fs::remove_dir_all(&staged);
        copy_tree(&own_bundle, &staged).map_err(|e| format!("cannot write beside the installed app: {e}"))?;
        if let Some(signer) = &signer {
            check_bundle(&staged, signer)
                .map_err(|why| format!("the copy is not a build signed like the installed app: {why}"))?;
        }
        // All of it is on the disk before it becomes the app: a power cut then leaves the old
        // app or the whole new one, never a new one with files still missing.
        make_durable(&staged).map_err(|e| format!("cannot write the copy to the disk: {e}"))?;
        exchange(&staged, &installed).map_err(|e| format!("cannot replace the installed app: {e}"))
    })();

    match result {
        Ok(()) => {
            // `staged` now holds the replaced app. A copy of the game started from it a
            // moment ago still reads its files from there, so it is put aside whole and
            // removed at a later start (remove_replaced_bundles).
            if fs::rename(&staged, &replaced).is_err() {
                let _ = fs::remove_dir_all(&staged);
            }
            if let Some(parent) = installed.parent() {
                let _ = make_durable_one(parent, true);
            }
            ReplaceOutcome::Replaced(installed)
        }
        Err(reason) => {
            // The copy that was never used.
            let _ = fs::remove_dir_all(&staged);
            let _ = fs::write(&refusal, format!("{{\"version\":\"{}\"}}\n", opts.own_version));
            ReplaceOutcome::Failed(reason)
        }
    }
}

/// Removes what a replacement left beside the app this program runs from: the bundle it
/// replaced, once nothing runs from it any more, and a copy that a start which was cut off
/// never exchanged.
pub fn remove_replaced_bundles(own_dir: &Path) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Some((staged, replaced)) = bundle_of(own_dir).as_deref().and_then(beside) else {
        return removed;
    };
    // Another copy of the game may be making its copy right now: only an old one is litter.
    let stale = |path: &Path| {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .is_ok_and(|at| at.elapsed().is_ok_and(|age| age > std::time::Duration::from_secs(3600)))
    };
    for path in [replaced, staged.clone()] {
        if !path.is_dir() || (path == staged && !stale(&path)) {
            continue;
        }
        if programs_running_from(&path) == Some(false) && fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
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
            // On one APFS volume this is a clone that shares the file's blocks with its
            // source (no time, no space); anywhere else it is a copy of every byte.
            fs::copy(&source, &dest)?;
            clear_quarantine(&dest);
        }
    }
    Ok(())
}

/// Writes one file or folder to the disk. `full` also makes the drive write what it holds
/// in its own cache, which `fsync` on macOS leaves to the drive.
#[cfg(target_os = "macos")]
fn make_durable_one(path: &Path, full: bool) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let file = fs::File::open(path)?;
    // SAFETY: the descriptor is open for the duration of the call.
    let result = unsafe {
        if full {
            libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC)
        } else {
            libc::fsync(file.as_raw_fd())
        }
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn make_durable_one(_path: &Path, _full: bool) -> std::io::Result<()> {
    Ok(())
}

/// Writes a copied tree to the disk: every file, every folder after its entries, and then
/// the drive's cache once for all of them.
fn make_durable(tree: &Path) -> std::io::Result<()> {
    fn walk(dir: &Path) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(&entry.path())?;
            } else if kind.is_file() {
                make_durable_one(&entry.path(), false)?;
            }
        }
        make_durable_one(dir, false)
    }
    walk(tree)?;
    // Not every file system has this request; what `fsync` did above then has to do.
    let _ = make_durable_one(tree, true);
    Ok(())
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

    const IDENTIFIER: &str = "com.battlefronteternal.client";

    /// A bundle as build/native/package-macos.py lays it out, holding `version`.
    fn write_bundle(parent: &Path, version: &str) -> PathBuf {
        write_bundle_of(parent, version, IDENTIFIER)
    }

    fn write_bundle_of(parent: &Path, version: &str, identifier: &str) -> PathBuf {
        let bundle = parent.join("Battlefront Eternal.app");
        let macos = bundle.join("Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        fs::write(macos.join("bfe-client"), format!("client {version}")).unwrap();
        fs::set_permissions(macos.join("bfe-client"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("../Resources", macos.join("resources")).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                "<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleExecutable</key>\n\t<string>bfe-client</string>\n\
                 \t<key>CFBundleIdentifier</key>\n\t<string>{identifier}</string>\n</dict>\n</plist>\n"
            ),
        )
        .unwrap();
        fs::write(
            bundle.join("Contents/Resources/bfe-release.json"),
            format!(r#"{{"version":"{version}","channel":"development-preview"}}"#),
        )
        .unwrap();
        macos
    }

    fn names_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn finds_the_bundle_of_a_program_and_copies_it_whole() {
        let work = temp_dir("copy");
        let macos = write_bundle(&work.join("a"), "0.1.0-preview.2");
        let bundle = bundle_of(&macos).unwrap();
        assert_eq!(bundle, work.join("a/Battlefront Eternal.app"));
        assert_eq!(bundle_of(&work), None);
        assert_eq!(bundle_of(Path::new("/opt/Battlefront Eternal")), None);

        // A version that arrived as an update is one bundle, with its program directly in it.
        let version = work.join("versions/0.1.0-preview.2");
        let app = version.join("Battlefront Eternal.app");
        assert_eq!(version_bundle(&version, &app.join("Contents/MacOS/bfe-client")), Some(app.clone()));
        assert_eq!(version_bundle(&version, &app.join("Contents/MacOS/bfeserver")), Some(app.clone()));
        for elsewhere in [
            // In a bundle nested in the app's resources: the bundle around it is not the version's.
            "Battlefront Eternal.app/Contents/Resources/Inner.app/Contents/MacOS/bfe-client",
            "Battlefront Eternal.app/Contents/MacOS/Inner.app/Contents/MacOS/bfe-client",
            "folder/Battlefront Eternal.app/Contents/MacOS/bfe-client",
            "Battlefront Eternal.app/Contents/MacOS/folder/bfe-client",
            "Battlefront Eternal.app/Contents/Resources/bfe-client",
            "Battlefront Eternal/Contents/MacOS/bfe-client",
            "Battlefront Eternal.app/Contents/MacOS",
            "bfe-client",
        ] {
            assert_eq!(version_bundle(&version, &version.join(elsewhere)), None, "{elsewhere}");
        }
        // Nor a program of another version's folder, or one reached by stepping out of this one.
        assert_eq!(version_bundle(&version, &work.join("versions/0.1.0-preview.3/Battlefront Eternal.app/Contents/MacOS/bfe-client")), None);
        assert_eq!(version_bundle(&version, &version.join("../0.1.0-preview.2/Battlefront Eternal.app/Contents/MacOS/bfe-client")), Some(app));
        assert_eq!(version_bundle(&version, &version.join("Battlefront Eternal.app/Contents/MacOS/../../../x.app/Contents/MacOS/y")), Some(version.join("x.app")));

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
        // Nor does a program from a bundle nested inside the version's bundle.
        opts.own_dir = write_bundle(&own.join("../Resources"), "0.1.0-preview.2");
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Skipped("not running from the update folder"));
        assert_eq!(fs::read_to_string(newer.join("bfe-client")).unwrap(), "client 0.1.0-preview.3");
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn only_a_copy_of_this_very_app_is_replaced() {
        let work = temp_dir("identifier");
        let root = work.join("updates");
        let own = write_bundle(&versions_dir(&root).join("0.1.0-preview.2"), "0.1.0-preview.2");
        // Older, at the path the environment names, and not this app.
        let other = write_bundle_of(&work.join("Applications"), "0.1.0-preview.1", "org.example.another-app");
        let opts = ReplaceOptions {
            root: root.clone(),
            own_dir: own,
            own_version: "0.1.0-preview.2".to_string(),
            installed_dir: other.clone(),
            verify_signature: false,
        };
        assert_eq!(bundle_identifier(&bundle_of(&opts.own_dir).unwrap()).as_deref(), Some(IDENTIFIER));
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Skipped("the installed copy is another app"));
        // Nor one that does not say what it is.
        fs::remove_file(other.join("../Info.plist")).unwrap();
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Skipped("the installed copy is another app"));
        assert_eq!(fs::read_to_string(other.join("bfe-client")).unwrap(), "client 0.1.0-preview.1");
        assert_eq!(names_in(&work.join("Applications")), ["Battlefront Eternal.app"]);
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
        // The app that was replaced is put aside whole, for a copy of it that may just have
        // started, and a second run has nothing to do.
        let applications = work.join("Applications");
        let aside = applications.join(".Battlefront Eternal.app.bfe-old");
        assert_eq!(names_in(&applications), [".Battlefront Eternal.app.bfe-old", "Battlefront Eternal.app"]);
        assert_eq!(fs::read_to_string(aside.join("Contents/MacOS/bfe-client")).unwrap(), "client 0.1.0-preview.1");
        assert_eq!(replace_installed_bundle(&opts), ReplaceOutcome::Skipped("the installed app is not older"));
        // A later start of the installed app removes it, and a copy that was never exchanged
        // once it is old; a fresh one may be another copy of the game at work.
        let cut_off = applications.join(".Battlefront Eternal.app.bfe-update");
        fs::create_dir(&cut_off).unwrap();
        assert_eq!(remove_replaced_bundles(&installed), [aside]);
        assert_eq!(names_in(&applications), [".Battlefront Eternal.app.bfe-update", "Battlefront Eternal.app"]);
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
        fs::File::open(&cut_off).unwrap().set_modified(long_ago).unwrap();
        assert_eq!(remove_replaced_bundles(&installed), [cut_off]);
        assert_eq!(names_in(&applications), ["Battlefront Eternal.app"]);
        let _ = fs::remove_dir_all(&work);
    }

    /// With the check that every real start makes: real signatures, made by `codesign`.
    #[cfg(target_os = "macos")]
    #[test]
    fn replaces_a_signed_app_only_with_a_build_whose_signature_holds() {
        use crate::update::signer::testing::{sign, signed_bundle, unsigned_bundle};
        let codesign_accepts = |bundle: &Path| check_bundle(bundle, &crate::update::signer::Signer::Anyone);
        let work = temp_dir("signed");
        let root = work.join("updates");
        let applications = work.join("Applications");
        let installed = signed_bundle(&applications, "0.1.0-preview.1", IDENTIFIER);
        let options = |version: &str, own: &Path| ReplaceOptions {
            root: root.clone(),
            own_dir: own.join("Contents/MacOS"),
            own_version: version.to_string(),
            installed_dir: installed.join("Contents/MacOS"),
            verify_signature: true,
        };

        // A build that was changed after it was signed (one file more than the seal lists).
        let changed = unsigned_bundle(&versions_dir(&root).join("0.1.0-preview.2"), "0.1.0-preview.2", IDENTIFIER);
        sign(&changed);
        fs::write(changed.join("Contents/Resources/added.txt"), "not in the seal").unwrap();
        let ReplaceOutcome::Failed(reason) = replace_installed_bundle(&options("0.1.0-preview.2", &changed)) else {
            panic!("a build whose signature does not hold must not replace the app");
        };
        assert!(reason.starts_with("the copy is not a build signed like the installed app"), "{reason}");
        assert_eq!(read_release_info(&installed.join("Contents/MacOS")).version.as_deref(), Some("0.1.0-preview.1"));
        assert_eq!(codesign_accepts(&installed), Ok(()));
        assert_eq!(names_in(&applications), ["Battlefront Eternal.app"], "the unused copy is gone");
        // Refused once, and not copied again at every start.
        assert_eq!(
            replace_installed_bundle(&options("0.1.0-preview.2", &changed)),
            ReplaceOutcome::Skipped("refused before for this version")
        );

        // The next build is whole: it becomes the app, signature and all.
        let whole = signed_bundle(&versions_dir(&root).join("0.1.0-preview.3"), "0.1.0-preview.3", IDENTIFIER);
        assert_eq!(
            replace_installed_bundle(&options("0.1.0-preview.3", &whole)),
            ReplaceOutcome::Replaced(installed.clone())
        );
        assert_eq!(read_release_info(&installed.join("Contents/MacOS")).version.as_deref(), Some("0.1.0-preview.3"));
        assert_eq!(codesign_accepts(&installed), Ok(()));
        assert_eq!(fs::read_link(installed.join("Contents/MacOS/resources")).unwrap(), Path::new("../Resources"));
        assert_eq!(names_in(&applications), [".Battlefront Eternal.app.bfe-old", "Battlefront Eternal.app"]);
        let _ = fs::remove_dir_all(&work);
    }
}
