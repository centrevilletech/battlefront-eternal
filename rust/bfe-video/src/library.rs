//! Where each front-end movie is: its Bink file inside the player's game archives. Built once when the
//! menus start, from the first game folder candidate that holds `GameData/Data/_LVL_PC/Movies/shell.mvs`
//! (letter case ignored), plus every add-on's `GameData/AddOn/<name>/Data/_lvl_pc/Movies/*.mvs`.
//! Movies are named as in shell.json (`/common/shell/movies/<name>.webm`); a name maps to its archive
//! entry through the prepared content's `common/shell/movies/manifest.json` (`hash`), else through the
//! name's own hash. Without a game folder the library is empty and every movie fails at once.
use crate::archive::{self, Entry};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// A movie's Bink file: a file, the offset where the movie starts in it and its length there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub offset: u64,
    pub length: u64,
}

#[derive(Default)]
pub struct MovieLibrary {
    game: Option<PathBuf>,
    archives: usize,
    by_hash: HashMap<u32, Source>,
    by_name: HashMap<String, Source>,
}

/// `base` joined with `parts`, each part matched without regard to letter case, under the name the folder
/// or file has on disk. A case-insensitive file system (Windows, macOS by default) opens `MOVIES` as
/// `Movies` too, but the path found keeps the disk's letters, the same on every system. Where two names
/// differ only by case (possible on Linux) the one spelled as asked wins, else the first in sorted order.
pub fn find_ignoring_case(base: &Path, parts: &[&str]) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    for part in parts {
        let mut names: Vec<_> = std::fs::read_dir(&path)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.eq_ignore_ascii_case(part))
            .collect();
        names.sort();
        match names.iter().find(|n| n.as_str() == *part).or(names.first()) {
            Some(name) => path.push(name),
            // A folder that can be opened but not listed.
            None if path.join(part).exists() => path.push(part),
            None => return None,
        }
    }
    Some(path)
}

/// The movie name in a shell.json url (`/common/shell/movies/main.webm` -> `main`).
fn name_of(url: &str) -> String {
    let file = url.rsplit(['/', '\\']).next().unwrap_or(url);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    stem.to_ascii_lowercase()
}

impl MovieLibrary {
    /// Indexes the movie archives of the first candidate folder that has them. A candidate may be the
    /// game's folder or a folder inside it (the preparation record names `GameData/Data/_LVL_PC`).
    pub fn open(content: Option<&Path>, candidates: &[PathBuf]) -> Self {
        let mut library = Self::default();
        'search: for candidate in candidates {
            for folder in candidate.ancestors().take(4) {
                let Some(shell) = find_ignoring_case(
                    folder,
                    &["GameData", "Data", "_LVL_PC", "Movies", "shell.mvs"],
                ) else {
                    continue;
                };
                match archive::read_index(&shell) {
                    Ok(entries) => {
                        library.game = Some(folder.to_path_buf());
                        library.add(&shell, entries);
                        break 'search;
                    }
                    Err(error) => eprintln!("Movies: {error}"),
                }
            }
        }
        if let Some(game) = library.game.clone() {
            if let Some(addons) = find_ignoring_case(&game, &["GameData", "AddOn"]) {
                let mut dirs: Vec<_> = std::fs::read_dir(addons)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .collect();
                dirs.sort();
                for dir in dirs {
                    let Some(movies) = find_ignoring_case(&dir, &["Data", "_lvl_pc", "Movies"])
                    else {
                        continue;
                    };
                    let mut files: Vec<_> = std::fs::read_dir(movies)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| {
                            p.extension()
                                .and_then(|e| e.to_str())
                                .is_some_and(|e| e.eq_ignore_ascii_case("mvs"))
                        })
                        .collect();
                    files.sort();
                    for file in files {
                        match archive::read_index(&file) {
                            Ok(entries) => library.add(&file, entries),
                            Err(error) => eprintln!("Movies: {error}"),
                        }
                    }
                }
            }
        }
        if let Some(manifest) = content
            .map(|c| c.join("common/shell/movies/manifest.json"))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        {
            if let Some(movies) = manifest.get("movies").and_then(|m| m.as_object()) {
                for (name, entry) in movies {
                    let hash = entry
                        .get("hash")
                        .and_then(|h| h.as_str())
                        .and_then(|h| u32::from_str_radix(h.trim_start_matches("0x"), 16).ok());
                    if let Some(source) = hash.and_then(|h| library.by_hash.get(&h)) {
                        library
                            .by_name
                            .insert(name.to_ascii_lowercase(), source.clone());
                    }
                }
            }
        }
        library
    }

    fn add(&mut self, path: &Path, entries: Vec<Entry>) {
        self.archives += 1;
        for e in entries {
            self.by_hash.entry(e.hash).or_insert(Source {
                path: path.to_path_buf(),
                offset: e.offset,
                length: e.length,
            });
        }
    }

    /// The movie a shell.json url names, when the game's archives have it.
    pub fn resolve(&self, url: &str) -> Option<Source> {
        let name = name_of(url);
        self.by_name
            .get(&name)
            .or_else(|| self.by_hash.get(&archive::hash(&name)))
            .cloned()
    }

    /// Names a movie directly (tests, tools).
    pub fn insert(&mut self, name: &str, source: Source) {
        self.by_name.insert(name_of(name), source);
    }

    /// The game folder the movies come from.
    pub fn game_dir(&self) -> Option<&Path> {
        self.game.as_deref()
    }

    pub fn archive_count(&self) -> usize {
        self.archives
    }

    /// Every movie of the archives.
    pub fn sources(&self) -> impl Iterator<Item = &Source> {
        self.by_hash.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::hash;

    fn chunk(tag: u32, body: &[u8]) -> Vec<u8> {
        let mut out = tag.to_le_bytes().to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        out
    }

    /// A movie archive holding the synthetic movie under each name.
    fn archive(names: &[&str]) -> Vec<u8> {
        let movie = crate::tests::PATTERN;
        let mut info = Vec::new();
        let mut data = Vec::new();
        for name in names {
            for w in [
                hash("Segment"),
                hash("Name"),
                hash(name),
                hash("Length"),
                movie.len() as u32,
                hash("Padding"),
                8,
                hash("SegmentEnd"),
            ] {
                info.extend(w.to_le_bytes());
            }
            data.extend(movie);
            data.extend([0u8; 8]);
        }
        let mut inner = chunk(hash("Info"), &info);
        inner.extend(chunk(hash("Data"), &data));
        let outer = chunk(hash("shell"), &inner);
        let mut file = b"ucfb".to_vec();
        file.extend((outer.len() as u32).to_le_bytes());
        file.extend(outer);
        file
    }

    #[test]
    fn finds_the_game_and_its_add_ons_whatever_the_case() {
        let root = std::env::temp_dir().join(format!("bfe-movielib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("Game Folder");
        let shell = game.join("GameData/Data/_LVL_PC/Movies");
        let addon = game.join("GameData/AddOn/Tat3/Data/_lvl_pc/MOVIES");
        std::fs::create_dir_all(&shell).unwrap();
        std::fs::create_dir_all(&addon).unwrap();
        std::fs::write(shell.join("shell.mvs"), archive(&["main", "atat_in"])).unwrap();
        std::fs::write(addon.join("tat3.MVS"), archive(&["tat3fly"])).unwrap();
        // The prepared content names the Galactic Conquest ending by its own hash.
        let content = root.join("content");
        std::fs::create_dir_all(content.join("common/shell/movies")).unwrap();
        std::fs::write(
            content.join("common/shell/movies/manifest.json"),
            format!(
                r#"{{"version":1,"movies":{{"rebel_win":{{"hash":"0x{:08x}"}}}}}}"#,
                hash("atat_in")
            ),
        )
        .unwrap();

        // The preparation record's path (inside the game) works as well as the game folder; a missing
        // candidate is passed over.
        let candidates = [root.join("moved away"), game.join("GameData/Data/_LVL_PC")];
        let library = MovieLibrary::open(Some(&content), &candidates);
        assert_eq!(library.game_dir(), Some(game.as_path()));
        assert_eq!(library.archive_count(), 2);
        let main = library.resolve("/common/shell/movies/main.webm").unwrap();
        assert_eq!(main.path, shell.join("shell.mvs"));
        let atat = library
            .resolve("/common/shell/movies/atat_in.webm")
            .unwrap();
        assert_eq!(library.resolve("movies/rebel_win.webm"), Some(atat.clone()));
        let fly = library
            .resolve("/common/shell/movies/tat3fly.webm")
            .unwrap();
        assert_eq!(fly.path, addon.join("tat3.MVS"));
        assert!(library
            .resolve("/common/shell/movies/nothing.webm")
            .is_none());

        // The movie found plays.
        let movie = crate::Movie::open(Some(main), false);
        let frame = loop {
            if let Some(f) = movie.poll() {
                break f.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!((frame.width, frame.height), (32, 24));
        drop(movie);

        // No game folder: nothing resolves.
        let empty = MovieLibrary::open(Some(&content), &[root.join("moved away")]);
        assert_eq!(empty.archive_count(), 0);
        assert!(empty.resolve("/common/shell/movies/main.webm").is_none());

        // Every folder of the main archive in other letters: found, under the names on disk.
        let other = root.join("Other Game");
        let movies = other.join("gamedata").join("DATA").join("_lvl_pc").join("movies");
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::write(movies.join("SHELL.MVS"), archive(&["main"])).unwrap();
        let library = MovieLibrary::open(None, &[other.clone()]);
        assert_eq!(library.game_dir(), Some(other.as_path()));
        let main = library.resolve("/common/shell/movies/main.webm").unwrap();
        assert_eq!(main.path, movies.join("SHELL.MVS"));
        // A decoder thread may still hold a file for a moment; Windows does not remove an open file.
        let _ = std::fs::remove_dir_all(&root);
    }
}
