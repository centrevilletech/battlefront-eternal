//! Linked programs kept on disk between runs, where the driver can hand them
//! back (program binaries). An entry is found by the driver's identity and the
//! program's final source; one that is missing, damaged or refused by the
//! driver is compiled from source as before and written again.
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime},
};

static DIR: OnceLock<PathBuf> = OnceLock::new();
const MAGIC: &[u8; 8] = b"BFEPROG1";
/// Entries not used for this long are removed when a run first uses the cache.
const UNUSED: Duration = Duration::from_secs(60 * 60 * 24 * 30);

/// Where the cache lives (once per process; unset means no cache).
pub fn set_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

pub(crate) fn enabled() -> bool {
    DIR.get().is_some()
}

fn hash(seed: u8, parts: &[&[u8]]) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    for part in parts {
        part.hash(&mut hasher);
    }
    hasher.finish()
}

/// One program's place in the cache.
pub(crate) struct Entry {
    pub path: PathBuf,
    check: u64,
}

/// The folder for this driver, with entries of other drivers and long-unused
/// ones removed; None without a cache folder.
pub(crate) fn driver_dir(driver: &str) -> Option<PathBuf> {
    let root = DIR.get()?;
    let name = format!("{:016x}", hash(0, &[driver.as_bytes()]));
    let dir = root.join(&name);
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        if entry.file_name() != name.as_str() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    let now = SystemTime::now();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).unwrap_or_default() > UNUSED);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Some(dir)
}

pub(crate) fn entry(dir: &Path, vs: &str, fs: &str, attributes: &[(String, u32)]) -> Entry {
    let mut names = Vec::new();
    for (name, location) in attributes {
        names.extend_from_slice(name.as_bytes());
        names.extend_from_slice(&location.to_le_bytes());
    }
    let parts: [&[u8]; 3] = [vs.as_bytes(), fs.as_bytes(), &names];
    Entry { path: dir.join(format!("{:016x}.bin", hash(1, &parts))), check: hash(2, &parts) }
}

/// The stored binary format and bytes, if the entry is whole.
pub(crate) fn load(entry: &Entry) -> Option<(u32, Vec<u8>)> {
    let bytes = std::fs::read(&entry.path).ok()?;
    let rest = bytes.strip_prefix(MAGIC.as_slice())?;
    let (check, rest) = rest.split_first_chunk::<8>()?;
    let (format, rest) = rest.split_first_chunk::<4>()?;
    if u64::from_le_bytes(*check) != entry.check || rest.is_empty() {
        return None;
    }
    // Marks the entry as used (see UNUSED).
    let _ = std::fs::File::options().append(true).open(&entry.path).and_then(|f| f.set_modified(SystemTime::now()));
    Some((u32::from_le_bytes(*format), rest.to_vec()))
}

/// Writes the entry whole or not at all.
pub(crate) fn store(entry: &Entry, format: u32, binary: &[u8]) {
    let Some(dir) = entry.path.parent() else { return };
    let mut bytes = Vec::with_capacity(MAGIC.len() + 12 + binary.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&entry.check.to_le_bytes());
    bytes.extend_from_slice(&format.to_le_bytes());
    bytes.extend_from_slice(binary);
    let temporary = entry.path.with_extension(format!("tmp{}", std::process::id()));
    let written = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&temporary, &bytes)).and_then(|()| std::fs::rename(&temporary, &entry.path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

pub(crate) fn forget(entry: &Entry) {
    let _ = std::fs::remove_file(&entry.path);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_entry_reads_back_and_a_damaged_or_other_source_entry_does_not() {
        let dir = std::env::temp_dir().join(format!("bfe-program-cache-{}", std::process::id()));
        let attributes = [("position".to_string(), 0)];
        let a = entry(&dir, "vertex", "fragment", &attributes);
        assert!(load(&a).is_none());
        store(&a, 7, &[1, 2, 3]);
        assert_eq!(load(&a), Some((7, vec![1, 2, 3])));
        let other = entry(&dir, "vertex", "fragment 2", &attributes);
        assert_ne!(other.path, a.path);
        assert!(load(&other).is_none());
        let mut bytes = std::fs::read(&a.path).unwrap();
        bytes[10] ^= 1;
        std::fs::write(&a.path, &bytes).unwrap();
        assert!(load(&a).is_none());
        forget(&a);
        assert!(!a.path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
