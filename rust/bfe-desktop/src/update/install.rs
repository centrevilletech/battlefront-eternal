//! Content-addressed Zstandard blob installation (`updates/blobs/<aa>/<sha256>.zst`),
//! local tree deduplication (hardlinking on Linux when inside `root`, copying otherwise),
//! and atomic staging into `<root>/versions/<version>`.
//!
//! Mirrors `electron/update/install.js` (`blobPathFor`, `placeFromLocal`,
//! `downloadToFile`, `installRelease`, `DEFAULT_CONCURRENCY`, `DEFAULT_BLOB_TIMEOUT_MS`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;

use sha2::{Digest, Sha256};

use crate::launch::iso8601_now;
use crate::paths::{normalize_lexical, resolve_from};
use crate::update::feed::{
    decompress_zstd_stream, join_url, open_url_reader, validate_link_target, ManifestFileEntry,
    PlatformManifest,
};
use crate::update::state::{
    versions_dir, InstallFileEntry, InstallRecord, INSTALL_RECORD,
};
use crate::update::tree::{
    index_tree, is_valid_sha256, safe_relative_path, sha256_file,
};
use crate::update::version::parse_version;

/// Default number of concurrent blob download workers (`6`).
///
/// Mirrors `DEFAULT_CONCURRENCY` in `electron/update/install.js`.
pub const DEFAULT_CONCURRENCY: usize = 6;

/// Default timeout in milliseconds (`120_000`) for downloading and decompressing a single blob.
///
/// Mirrors `DEFAULT_BLOB_TIMEOUT_MS` in `electron/update/install.js`.
pub const DEFAULT_BLOB_TIMEOUT_MS: u64 = 120 * 1000;

/// Resolves the relative blob path (`updates/blobs/<aa>/<sha256>.zst`) for `entry`.
///
/// Mirrors `blobPathFor` in `electron/update/install.js`.
pub fn blob_path_for(entry: &ManifestFileEntry) -> Result<String, String> {
    if let Some(blob) = entry.blob.as_deref().filter(|b| !b.is_empty()) {
        if blob.starts_with('/') || blob.contains("..") || blob.contains(':') {
            return Err(format!("Unsafe blob path: {blob}"));
        }
        return Ok(blob.to_string());
    }
    if !is_valid_sha256(&entry.sha256) {
        return Err("Invalid blob hash".to_string());
    }
    Ok(format!(
        "updates/blobs/{}/{}.zst",
        &entry.sha256[..2],
        entry.sha256
    ))
}

fn apply_file_mode(path: &Path, mode: Option<u32>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bits = match mode {
            Some(m) if (m & 0o111) != 0 => 0o755,
            Some(m) if (m & 0o777) != 0 => m & 0o777,
            _ => 0o644,
        };
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(bits));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

/// Indexes local candidate files across `sources` (`[own_dir, bootstrap_dir, ...installed_versions]`).
///
/// When a source directory has a pre-computed tree index (`resources/bfe-tree.json` or
/// `.bfe-install.json` with a `files` array), indexes those entries directly after
/// checking file existence and size. When a source directory does not have a tree index
/// (such as a version installed by an older updater that wrote `"files": <count>` in
/// `.bfe-install.json`), matches manifest paths in `dir` by size and SHA-256 digest.
///
/// Mirrors `localFiles` in `electron/update/install.js`.
fn local_files(
    sources: &[PathBuf],
    manifest_files: &[ManifestFileEntry],
) -> BTreeMap<String, PathBuf> {
    let mut by_hash = BTreeMap::new();
    let wanted: BTreeSet<&str> = manifest_files.iter().map(|f| f.sha256.as_str()).collect();
    for dir in sources {
        let indexed = index_tree(dir);
        if !indexed.is_empty() {
            for (sha256, full) in indexed {
                if wanted.contains(sha256.as_str()) {
                    by_hash.entry(sha256).or_insert(full);
                }
            }
            continue;
        }
        for file in manifest_files {
            if by_hash.contains_key(&file.sha256) {
                continue;
            }
            let Some(rel) = safe_relative_path(&file.path) else {
                continue;
            };
            let candidate = resolve_from(dir, Path::new(&rel));
            if let Ok(meta) = fs::metadata(&candidate) {
                if meta.is_file() && meta.len() == file.size {
                    if let Ok((digest, size)) = sha256_file(&candidate) {
                        if size == file.size && digest == file.sha256 {
                            by_hash.insert(file.sha256.clone(), candidate);
                        }
                    }
                }
            }
        }
    }
    by_hash
}

/// Places a file at `dest` from an existing local installation file `source`.
///
/// On Linux, when `source` already lives inside `root` (`<root>/versions/<prev>/...`),
/// hard-links `source` to `dest` first to avoid duplicating disk blocks across
/// installed versions. When `source` lives outside `root` (e.g. a system `/opt` package
/// or temporary AppImage mount), copies `source` to `dest` and applies `mode`.
///
/// Mirrors `placeFromLocal` in `electron/update/install.js`.
pub fn place_from_local(
    source: &Path,
    dest: &Path,
    mode: Option<u32>,
    root: &Path,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(dest);

    if cfg!(target_os = "linux") {
        let norm_root = normalize_lexical(root);
        let norm_source = normalize_lexical(source);
        if norm_source != norm_root
            && norm_source.starts_with(&norm_root)
            && fs::hard_link(source, dest).is_ok()
        {
            return Ok(());
        }
    }

    fs::copy(source, dest).map_err(|e| e.to_string())?;
    apply_file_mode(dest, mode);
    Ok(())
}

/// Streams a Zstandard-compressed blob from `url`, decompresses it on the fly while
/// computing its SHA-256 and enforcing `expected_size`, and atomically renames the
/// verified file into `dest`.
///
/// Mirrors `downloadToFile` in `electron/update/install.js`.
pub fn download_to_file(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    expected_size: u64,
    mode: Option<u32>,
    timeout_ms: u64,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut nonce = [0u8; 4];
    let _ = getrandom::getrandom(&mut nonce);
    let hex_nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let temp_name = format!(
        "{}.part-{}-{}",
        dest.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("blob"),
        std::process::id(),
        hex_nonce
    );
    let temp = dest.with_file_name(temp_name);
    let _ = fs::remove_file(&temp);

    let result: Result<(), String> = (|| {
        let reader = open_url_reader(url, timeout_ms)?;
        let file = File::create(&temp).map_err(|e| e.to_string())?;
        let mut hashing_writer = HashingFileWriter {
            file,
            hasher: Sha256::new(),
        };

        let written = decompress_zstd_stream(
            reader,
            &mut hashing_writer,
            expected_size,
            "Blob exceeded declared size",
        )?;
        hashing_writer
            .file
            .flush()
            .map_err(|e| format!("Failed to flush blob {}: {e}", dest.display()))?;
        drop(hashing_writer.file);

        if written != expected_size {
            return Err(format!(
                "Size mismatch for {}: expected {expected_size}, got {written}",
                dest.display()
            ));
        }
        let digest = hashing_writer.hasher.finalize();
        let actual_sha256: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        if actual_sha256 != expected_sha256.to_ascii_lowercase() {
            return Err(format!("SHA-256 mismatch for {}", dest.display()));
        }

        apply_file_mode(&temp, mode);
        fs::rename(&temp, dest).map_err(|e| e.to_string())?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

struct HashingFileWriter {
    file: File,
    hasher: Sha256,
}

impl Write for HashingFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Progress snapshot reported during [`install_release`].
///
/// Mirrors the object passed to `onProgress` in `electron/update/install.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallProgress {
    /// Target release version being installed.
    pub version: String,
    /// Number of files completed so far.
    pub completed: usize,
    /// Total number of files in the manifest.
    pub total: usize,
    /// Number of files reused from local installation trees.
    pub reused: usize,
    /// Number of files downloaded from content-addressed blobs.
    pub downloaded: usize,
}

/// Options for [`install_release`].
///
/// Mirrors the parameter object of `installRelease` in `electron/update/install.js`.
#[derive(Debug, Clone)]
pub struct InstallReleaseOptions {
    /// Update root directory (`<root>/versions` and `<root>/staging` live beneath `root`).
    pub root: PathBuf,
    /// Verified platform manifest for the target release.
    pub manifest: PlatformManifest,
    /// Base URL of the update server (`https://get.battlefronteternal.com/downloads/` or test HTTP server).
    pub base_url: String,
    /// Existing local directories (`[own_dir, bootstrap_dir, ...installed_versions]`) to index for file reuse.
    pub local_trees: Vec<PathBuf>,
    /// Maximum concurrent blob downloads (`DEFAULT_CONCURRENCY`).
    pub concurrency: usize,
    /// Per-blob download timeout in milliseconds (`DEFAULT_BLOB_TIMEOUT_MS`).
    pub timeout_ms: u64,
}

/// Outcome returned by [`install_release`].
///
/// Mirrors the return value of `installRelease` in `electron/update/install.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    /// True if the version was newly installed; false if already present.
    pub installed: bool,
    /// Status code: `"installed"` or `"already-installed"`.
    pub reason: String,
    /// Path to `<root>/versions/<version>`.
    pub dir: PathBuf,
    /// Number of files reused from local trees.
    pub reused: usize,
    /// Number of files downloaded from remote `.zst` blobs.
    pub downloaded: usize,
}

/// Installs a verified [`PlatformManifest`] into `<root>/versions/<version>`.
///
/// Stages all files inside `<root>/staging/<version>-<pid>`, reusing any file whose
/// SHA-256 matches an entry in `opts.local_trees` and downloading remaining files from
/// `<base_url>/updates/blobs/<aa>/<sha256>.zst`. Writes `.bfe-install.json` last and
/// renames the staging directory into `<root>/versions/<version>`.
///
/// Mirrors `installRelease` in `electron/update/install.js`.
pub fn install_release<F>(
    opts: &InstallReleaseOptions,
    mut on_progress: Option<F>,
) -> Result<InstallOutcome, String>
where
    F: FnMut(InstallProgress),
{
    if opts.root.as_os_str().is_empty() {
        return Err("Update root is not configured".to_string());
    }
    if parse_version(&opts.manifest.version).is_none() {
        return Err("Invalid update version".to_string());
    }
    let exe_rel = safe_relative_path(&opts.manifest.executable)
        .ok_or_else(|| "Invalid update executable".to_string())?;

    let final_dir = versions_dir(&opts.root).join(&opts.manifest.version);
    if final_dir.join(INSTALL_RECORD).exists() {
        return Ok(InstallOutcome {
            installed: false,
            reason: "already-installed".to_string(),
            dir: final_dir,
            reused: 0,
            downloaded: 0,
        });
    }

    let staging_root = opts.root.join("staging");
    fs::create_dir_all(&staging_root).map_err(|e| e.to_string())?;
    let stage_dir = staging_root.join(format!(
        "{}-{}",
        opts.manifest.version,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&stage_dir);
    fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;

    let norm_stage = normalize_lexical(&stage_dir);
    for dir_entry in &opts.manifest.dirs {
        let Some(rel) = safe_relative_path(dir_entry) else {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(format!("Unsafe directory in update manifest: {dir_entry}"));
        };
        let dest = resolve_from(&norm_stage, Path::new(&rel));
        if !dest.starts_with(&norm_stage) {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(format!("Unsafe directory destination: {rel}"));
        }
        if let Err(err) = fs::create_dir_all(&dest) {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(err.to_string());
        }
    }

    let local_index = Arc::new(local_files(&opts.local_trees, &opts.manifest.files));
    let files = Arc::new(opts.manifest.files.clone());
    let total = files.len();

    let next_index = Arc::new(AtomicUsize::new(0));
    let reused_count = Arc::new(AtomicUsize::new(0));
    let downloaded_count = Arc::new(AtomicUsize::new(0));
    let completed_count = Arc::new(AtomicUsize::new(0));
    let first_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let worker_count = opts.concurrency.max(1).min(total.max(1));
    let install_res: Result<(), String> = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let files_ref = Arc::clone(&files);
            let local_ref = Arc::clone(&local_index);
            let next_ref = Arc::clone(&next_index);
            let reused_ref = Arc::clone(&reused_count);
            let downloaded_ref = Arc::clone(&downloaded_count);
            let completed_ref = Arc::clone(&completed_count);
            let err_ref = Arc::clone(&first_error);
            let root_path = opts.root.clone();
            let base_url = opts.base_url.clone();
            let timeout_ms = opts.timeout_ms;
            let stage_base = norm_stage.clone();

            handles.push(scope.spawn(move || loop {
                if err_ref.lock().map(|g| g.is_some()).unwrap_or(true) {
                    break;
                }
                let idx = next_ref.fetch_add(1, Ordering::SeqCst);
                if idx >= files_ref.len() {
                    break;
                }
                let entry = &files_ref[idx];
                let step_res: Result<(), String> = (|| {
                    let rel = safe_relative_path(&entry.path).ok_or_else(|| {
                        format!("Unsafe path in update manifest: {}", entry.path)
                    })?;
                    let dest = resolve_from(&stage_base, Path::new(&rel));
                    if !dest.starts_with(&stage_base) || dest == stage_base {
                        return Err(format!("Unsafe destination path: {rel}"));
                    }

                    let mut was_reused = false;
                    if let Some(local_src) = local_ref.get(&entry.sha256) {
                        if place_from_local(local_src, &dest, entry.mode, &root_path).is_ok() {
                            // Verify copied files from outside `root` (e.g. AppImage mount)
                            // in case the source changed after `bfe-tree.json` was written.
                            let norm_root = normalize_lexical(&root_path);
                            let norm_local = normalize_lexical(local_src);
                            if norm_local.starts_with(&norm_root) {
                                was_reused = true;
                            } else if let Ok((hash, size)) = sha256_file(&dest) {
                                if hash == entry.sha256 && size == entry.size {
                                    was_reused = true;
                                } else {
                                    let _ = fs::remove_file(&dest);
                                }
                            } else {
                                let _ = fs::remove_file(&dest);
                            }
                        }
                    }

                    if was_reused {
                        reused_ref.fetch_add(1, Ordering::SeqCst);
                    } else {
                        let blob_rel = blob_path_for(entry)?;
                        let url = join_url(&base_url, &blob_rel)?;
                        download_to_file(
                            &url,
                            &dest,
                            &entry.sha256,
                            entry.size,
                            entry.mode,
                            timeout_ms,
                        )?;
                        downloaded_ref.fetch_add(1, Ordering::SeqCst);
                    }
                    completed_ref.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })();

                if let Err(e) = step_res {
                    if let Ok(mut guard) = err_ref.lock() {
                        if guard.is_none() {
                            *guard = Some(e);
                        }
                    }
                    break;
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        if let Some(err) = first_error.lock().ok().and_then(|mut g| g.take()) {
            return Err(err);
        }
        Ok(())
    });

    if let Err(err) = install_res {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(err);
    }

    for link in &opts.manifest.links {
        let Some(rel) = safe_relative_path(&link.path) else {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(format!("Unsafe link path in update manifest: {}", link.path));
        };
        if validate_link_target(&rel, &link.target).is_none() {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(format!("Unsafe link target in update manifest: {}", link.path));
        }
        let dest = resolve_from(&norm_stage, Path::new(&rel));
        if !dest.starts_with(&norm_stage) || dest == norm_stage {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(format!("Unsafe link destination: {rel}"));
        }
        if let Some(parent) = dest.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                let _ = fs::remove_dir_all(&stage_dir);
                return Err(err.to_string());
            }
        }
        let _ = fs::remove_file(&dest);
        #[cfg(unix)]
        {
            if let Err(err) = std::os::unix::fs::symlink(&link.target, &dest) {
                let _ = fs::remove_dir_all(&stage_dir);
                return Err(format!("Failed to create symlink {}: {err}", link.path));
            }
        }
        #[cfg(not(unix))]
        {
            let target_path = dest
                .parent()
                .unwrap_or(&stage_dir)
                .join(&link.target);
            if target_path.exists() {
                let _ = fs::copy(&target_path, &dest);
            }
        }
    }

    let reused = reused_count.load(Ordering::SeqCst);
    let downloaded = downloaded_count.load(Ordering::SeqCst);
    if let Some(cb) = &mut on_progress {
        cb(InstallProgress {
            version: opts.manifest.version.clone(),
            completed: total,
            total,
            reused,
            downloaded,
        });
    }

    let exe_path = stage_dir.join(&exe_rel);
    if !exe_path.exists() {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(format!(
            "Installed update is missing executable: {}",
            opts.manifest.executable
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&exe_path, fs::Permissions::from_mode(0o755));
    }

    let record = InstallRecord {
        version: opts.manifest.version.clone(),
        release_id: opts.manifest.release_id.clone(),
        revision: opts.manifest.revision.clone(),
        platform: Some(opts.manifest.platform.clone()),
        executable: exe_rel,
        files: opts
            .manifest
            .files
            .iter()
            .filter_map(|f| {
                let rel = safe_relative_path(&f.path)?;
                Some(InstallFileEntry {
                    path: rel,
                    size: f.size,
                    sha256: f.sha256.clone(),
                    mode: f.mode,
                })
            })
            .collect(),
        installed_at: Some(iso8601_now()),
    };

    let record_bytes = {
        let mut b = serde_json::to_vec_pretty(&record).map_err(|e| {
            let _ = fs::remove_dir_all(&stage_dir);
            e.to_string()
        })?;
        b.push(b'\n');
        b
    };
    let record_path = stage_dir.join(INSTALL_RECORD);
    if let Err(err) = fs::write(&record_path, &record_bytes) {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(err.to_string());
    }

    if let Err(err) = fs::create_dir_all(versions_dir(&opts.root)) {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(err.to_string());
    }
    if final_dir.exists() && !final_dir.join(INSTALL_RECORD).exists() {
        let _ = fs::remove_dir_all(&final_dir);
    }
    if let Err(err) = fs::rename(&stage_dir, &final_dir) {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(err.to_string());
    }

    Ok(InstallOutcome {
        installed: true,
        reason: "installed".to_string(),
        dir: final_dir,
        reused,
        downloaded,
    })
}
