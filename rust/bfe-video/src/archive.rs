//! The game's movie archives (`Movies/shell.mvs`, an add-on's `Movies/*.mvs`): a ucfb tree whose tags are
//! name hashes. Its `Info` chunk lists each movie (`Segment`: `Name` hash, `Length`, `Padding`) in file
//! order; its `Data` chunk holds the Bink files back to back, each followed by its padding. Only the
//! chunk headers and the small `Info` chunk are read. Each movie must lie inside the `Data` chunk.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// One movie in an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub hash: u32,
    /// Byte offset of its Bink file in the archive.
    pub offset: u64,
    pub length: u64,
}

/// The engine's name hash: 32-bit FNV-1a over each byte OR 0x20.
pub fn hash(name: &str) -> u32 {
    name.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ (b | 0x20) as u32).wrapping_mul(16_777_619)
    })
}

const INFO: u32 = 0x0fb4_0705;
const DATA: u32 = 0xd872_e2a5;
const NAME: u32 = 0x8d39_bde6;
const SEGMENT_INFO: u32 = 0xb969_be96;
const SEGMENT: u32 = 0xf3c3_42e6;
const SEGMENT_END: u32 = 0x7db1_d043;
const LENGTH: u32 = 0x83d0_3615;
const PADDING: u32 = 0x8096_08b6;
const MAX_INFO: u64 = 1 << 20;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// The first chunk tagged `tag` below `start..end`, not descending into `Data`: its body's range.
fn find(
    f: &mut File,
    start: u64,
    end: u64,
    tag: u32,
    depth: u32,
) -> std::io::Result<Option<(u64, u64)>> {
    let mut pos = start;
    let mut head = [0u8; 8];
    while pos + 8 <= end {
        f.seek(SeekFrom::Start(pos))?;
        f.read_exact(&mut head)?;
        let (t, size) = (u32_at(&head, 0), u32_at(&head, 4) as u64);
        let (body, stop) = (pos + 8, pos + 8 + size);
        if stop > end {
            break;
        }
        if t == tag {
            return Ok(Some((body, stop)));
        }
        if depth < 3 && t != DATA {
            if let Some(found) = find(f, body, stop, tag, depth + 1)? {
                return Ok(Some(found));
            }
        }
        pos = (stop + 3) & !3;
    }
    Ok(None)
}

/// Every movie of an archive, in file order. Each entry is checked to start with a Bink header.
pub fn read_index(path: &Path) -> Result<Vec<Entry>, String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut f = File::open(path).map_err(err)?;
    let len = f.metadata().map_err(err)?.len();
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).map_err(err)?;
    if &magic != b"ucfb" {
        return Err(format!("{}: not a movie archive", path.display()));
    }
    let missing = || format!("{}: no Info and Data in the movie archive", path.display());
    let (info_start, info_end) = find(&mut f, 8, len, INFO, 0)
        .map_err(err)?
        .ok_or_else(missing)?;
    let (data_start, data_end) = find(&mut f, 8, len, DATA, 0)
        .map_err(err)?
        .ok_or_else(missing)?;
    if info_end - info_start > MAX_INFO {
        return Err(missing());
    }
    let mut info = vec![0u8; (info_end - info_start) as usize];
    f.seek(SeekFrom::Start(info_start)).map_err(err)?;
    f.read_exact(&mut info).map_err(err)?;
    let mut records = Vec::new();
    let mut current: Option<(u32, u64, u64)> = None;
    let mut pos = 0;
    while pos + 4 <= info.len() {
        let key = u32_at(&info, pos);
        match key {
            0 => break,
            SEGMENT_INFO => pos += 4,
            SEGMENT => {
                current = Some((0, 0, 0));
                pos += 4;
            }
            SEGMENT_END => {
                records.extend(current.take());
                pos += 4;
            }
            _ => {
                if pos + 8 > info.len() {
                    break;
                }
                let value = u32_at(&info, pos + 4);
                if let Some(record) = current.as_mut() {
                    match key {
                        NAME => record.0 = value,
                        LENGTH => record.1 = value as u64,
                        PADDING => record.2 = value as u64,
                        _ => (),
                    }
                }
                pos += 8;
            }
        }
    }
    let mut entries = Vec::with_capacity(records.len());
    let mut offset = data_start;
    for (hash, length, padding) in records {
        // Each movie lies inside the Data chunk, and reading one never goes past its own length.
        if length < 4 || offset + length > data_end {
            return Err(format!(
                "{}: movie index runs past the movie data",
                path.display()
            ));
        }
        f.seek(SeekFrom::Start(offset)).map_err(err)?;
        f.read_exact(&mut magic).map_err(err)?;
        if &magic[..3] != b"BIK" {
            return Err(format!("{}: no Bink movie at {offset:#x}", path.display()));
        }
        entries.push(Entry {
            hash,
            offset,
            length,
        });
        offset += length + padding;
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_names_as_the_engine_does() {
        assert_eq!(hash("Info"), INFO);
        assert_eq!(hash("Data"), DATA);
        assert_eq!(hash("SegmentEnd"), SEGMENT_END);
        assert_eq!(hash("main"), 0xea90_e208);
        assert_eq!(hash("start_screen"), 0x304e_5d04);
    }

    fn chunk(tag: u32, body: &[u8]) -> Vec<u8> {
        let mut out = tag.to_le_bytes().to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        out
    }

    /// An archive of two fake movies (a Bink magic and filler), the second padded.
    fn synthetic_archive(movies: &[(&str, Vec<u8>, u32)]) -> Vec<u8> {
        let mut info = Vec::new();
        for w in [NAME, hash("shell"), SEGMENT_INFO] {
            info.extend(w.to_le_bytes());
        }
        let mut data = Vec::new();
        for (name, bytes, padding) in movies {
            for w in [
                SEGMENT,
                NAME,
                hash(name),
                LENGTH,
                bytes.len() as u32,
                PADDING,
                *padding,
                SEGMENT_END,
            ] {
                info.extend(w.to_le_bytes());
            }
            data.extend(bytes);
            data.extend(vec![0u8; *padding as usize]);
        }
        let mut inner = chunk(INFO, &info);
        inner.extend(chunk(DATA, &data));
        let outer = chunk(hash("shell"), &inner);
        let mut file = b"ucfb".to_vec();
        file.extend((outer.len() as u32).to_le_bytes());
        file.extend(outer);
        file
    }

    #[test]
    fn indexes_a_synthetic_archive() {
        let a = b"BIKi-first".to_vec();
        let b = b"BIKi-second-movie".to_vec();
        let bytes = synthetic_archive(&[("main", a.clone(), 6), ("atat_in", b.clone(), 0)]);
        let dir = std::env::temp_dir().join(format!("bfe-mvs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.mvs");
        std::fs::write(&path, &bytes).unwrap();
        let index = read_index(&path).unwrap();
        assert_eq!(index.len(), 2);
        assert_eq!(index[0].hash, hash("main"));
        assert_eq!(index[1].hash, hash("atat_in"));
        let at = |e: &Entry| &bytes[e.offset as usize..(e.offset + e.length) as usize];
        assert_eq!(at(&index[0]), &a[..]);
        assert_eq!(at(&index[1]), &b[..]);
        for end in 0..bytes.len() {
            std::fs::write(&path, &bytes[..end]).unwrap();
            let _ = read_index(&path);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An entry whose length runs past the end of the Data chunk (into bytes that follow it in the
    /// file) is not a movie of the archive.
    #[test]
    fn an_entry_running_past_the_data_chunk_is_refused() {
        let movie = b"BIKi-movie".to_vec();
        let mut bytes = synthetic_archive(&[("main", movie.clone(), 0)]);
        // Claim more bytes than the Data chunk holds, then put bytes after the archive's chunks.
        let length_at = bytes
            .windows(8)
            .position(|w| w[..4] == LENGTH.to_le_bytes() && w[4..] == (movie.len() as u32).to_le_bytes())
            .unwrap();
        bytes[length_at + 4..length_at + 8].copy_from_slice(&64u32.to_le_bytes());
        bytes.extend([0x55u8; 128]);
        let dir = std::env::temp_dir().join(format!("bfe-mvs-past-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.mvs");
        std::fs::write(&path, &bytes).unwrap();
        assert!(read_index(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
