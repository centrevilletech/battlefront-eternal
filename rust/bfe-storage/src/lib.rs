//! Backups for existing user files that a writer cannot read.

pub mod settings_json;

use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Before replacing a small record, keep the exact bytes its reader rejected.
/// Returns true when a backup was needed, including when a matching copy already existed.
pub fn preserve_unreadable(path: &Path, readable: impl FnOnce(&[u8]) -> bool) -> io::Result<bool> {
    preserve(path, |mut file| {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok((!readable(&bytes)).then(|| Cursor::new(bytes)))
    })
}

/// For readers that require UTF-8 before interpreting a record.
pub fn preserve_unreadable_text(
    path: &Path,
    readable: impl FnOnce(&str) -> bool,
) -> io::Result<bool> {
    preserve_unreadable(path, |bytes| std::str::from_utf8(bytes).is_ok_and(readable))
}

/// Write through an existing path as usual. When recovering a damaged record,
/// replace the directory entry so a hard link to its backup keeps the old bytes.
pub fn write_text_record(
    path: &Path,
    contents: &[u8],
    readable: impl FnOnce(&str) -> bool,
) -> io::Result<()> {
    if !preserve_unreadable_text(path, readable)? {
        return fs::write(path, contents);
    }

    let original = fs::OpenOptions::new().write(true).open(path)?;
    let permissions = original.metadata()?.permissions();
    drop(original);
    let mut replacement = tempfile::NamedTempFile::new_in(parent_dir(path))?;
    replacement.write_all(contents)?;
    replacement.as_file().set_permissions(permissions)?;
    replacement.as_file().sync_all()?;
    replacement.persist(path)?;
    Ok(())
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn preserve<R: Read + Seek>(
    path: &Path,
    rejected: impl FnOnce(File) -> io::Result<Option<R>>,
) -> io::Result<bool> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            if fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == 0) {
                return Ok(false);
            }
            return Err(e);
        }
    };
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    let Some(mut original) = rejected(file)? else {
        return Ok(false);
    };
    let size = original.seek(SeekFrom::End(0))?;
    if size == 0 {
        return Ok(false);
    }
    let backup = path.with_added_extension("bak");
    let mut candidate = backup.clone();
    let mut number = 0u64;
    loop {
        match fs::symlink_metadata(&candidate) {
            Ok(meta) => {
                if meta.is_file() && meta.len() == size && matches_copy(&mut original, &candidate)?
                {
                    return Ok(true);
                }
                number += 1;
                candidate = backup.with_added_extension(number.to_string());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                original.rewind()?;
                let mut copy = tempfile::NamedTempFile::new_in(parent_dir(path))?;
                if io::copy(&mut original, &mut copy)? != size {
                    return Err(io::Error::other("File size changed while making a backup"));
                }
                copy.as_file().sync_all()?;
                // A concurrent writer can claim this name. Only complete copies are visible.
                match copy.persist_noclobber(&candidate) {
                    Ok(_) => return Ok(true),
                    Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e.error),
                }
            }
            Err(e) => return Err(e),
        }
    }
}

fn matches_copy(original: &mut (impl Read + Seek), path: &Path) -> io::Result<bool> {
    let Ok(mut copy) = File::open(path) else {
        return Ok(false);
    };
    original.rewind()?;
    let mut original_chunk = [0; 8192];
    let mut saved_chunk = [0; 8192];
    loop {
        let n = original.read(&mut original_chunk)?;
        if n == 0 {
            return Ok(matches!(copy.read(&mut saved_chunk[..1]), Ok(0)));
        }
        if copy.read_exact(&mut saved_chunk[..n]).is_err()
            || original_chunk[..n] != saved_chunk[..n]
        {
            return Ok(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_preserves_a_backup_linked_to_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences");
        let linked = dir.path().join("other-name");
        fs::write(&path, "old").unwrap();
        fs::hard_link(&path, &linked).unwrap();
        write_text_record(&path, b"new", |_| true).unwrap();
        assert_eq!(fs::read(&linked).unwrap(), b"new");

        let backup = path.with_added_extension("bak");
        fs::hard_link(&path, &backup).unwrap();
        write_text_record(&path, b"recovered", |_| false).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"recovered");
        assert_eq!(fs::read(linked).unwrap(), b"new");
        assert_eq!(fs::read(backup).unwrap(), b"new");
    }

    #[test]
    fn backups_with_a_common_prefix_keep_both_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes");
        let mut bytes = vec![b'+'; 32 * 1024];
        bytes.push(b'x');
        fs::write(path.with_added_extension("bak"), &bytes).unwrap();
        *bytes.last_mut().unwrap() = b'y';
        fs::write(&path, &bytes).unwrap();
        preserve_unreadable(&path, |_| false).unwrap();
        preserve_unreadable(&path, |_| false).unwrap();
        assert_eq!(fs::read(path.with_added_extension("bak.1")).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}
