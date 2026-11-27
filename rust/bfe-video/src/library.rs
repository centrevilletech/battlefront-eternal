//! Where each front-end movie is: its Bink file inside the player's game archives. Built once per
//! game family when the menus start:
//! - `swbf1` (2004): `GameData/Data/_LVL_PC/Movies/shell.mvs` (letter case ignored), plus every
//!   add-on's `GameData/AddOn/<name>/Data/_lvl_pc/Movies/*.mvs`.
//! - `swbf2` (2005): `GameData/data/_lvl_pc/movies/*.mvs` (letter case ignored).
//!
//! Movies are named as in shell.json (`/common/shell/movies/<name>.bik`; `.webm` in content prepared by
//! earlier versions, the same name); a name maps to its archive entry through the prepared content's
//! `common/shell/movies/manifest.json` (for `swbf1`) or `swbf2/.content/common/shell/movies/manifest.json`
//! (for `swbf2`) (`hash`), else through the name's own hash. Without a game folder the library is empty
//! and every movie fails at once.
use crate::archive::{self, Entry};
use bfe_content::layout::family_dir;
use bfe_family::GameFamily;
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
    family: GameFamily,
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

/// The movie name in a shell.json url (`/common/shell/movies/main.bik` -> `main`), whatever the extension.
fn name_of(url: &str) -> String {
    let file = url.rsplit(['/', '\\']).next().unwrap_or(url);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    stem.to_ascii_lowercase()
}

/// Lists `*.mvs` files in `dir`, sorted case-insensitively so base archives
/// (`shell.mvs`) precede localized variants (`shellfr.mvs`).
fn mvs_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("mvs"))
        })
        .collect();
    files.sort_by(|a, b| {
        let an = a
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let bn = b
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        an.cmp(&bn).then_with(|| a.cmp(b))
    });
    files
}

fn looks_like_swbf2_install(folder: &Path) -> bool {
    find_ignoring_case(folder, &["GameData", "BattlefrontII.exe"]).is_some_and(|p| p.is_file())
}

fn looks_like_swbf1_only_install(folder: &Path) -> bool {
    if looks_like_swbf2_install(folder) {
        return false;
    }
    let Some(common) = find_ignoring_case(folder, &["GameData", "Data", "_LVL_PC", "Common"])
    else {
        return false;
    };
    std::fs::read_dir(common)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            e.file_type().is_ok_and(|ft| ft.is_dir())
                && find_ignoring_case(&e.path(), &["common.lvl"]).is_some_and(|p| p.is_file())
        })
}

impl MovieLibrary {
    /// Indexes the movie archives of the highest-priority candidate folder for `family` that has them.
    /// A candidate may be the game's folder or a folder inside it (the preparation record names
    /// `GameData/Data/_LVL_PC` or `GameData/data/_lvl_pc`).
    pub fn open(family: GameFamily, content: Option<&Path>, candidates: &[PathBuf]) -> Self {
        let mut library = Self {
            family,
            ..Self::default()
        };
        match family {
            GameFamily::Swbf1 => {
                // Exact released 2004 behavior: no BattlefrontII.exe veto and exact path sort order.
                'search_swbf1: for candidate in candidates {
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
                                break 'search_swbf1;
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
                            let Some(movies) =
                                find_ignoring_case(&dir, &["Data", "_lvl_pc", "Movies"])
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
            }
            GameFamily::Swbf2 => {
                'search_swbf2: for candidate in candidates {
                    for folder in candidate.ancestors().take(4) {
                        if looks_like_swbf1_only_install(folder) {
                            continue;
                        }
                        let Some(movies) =
                            find_ignoring_case(folder, &["GameData", "data", "_lvl_pc", "movies"])
                        else {
                            continue;
                        };
                        let files = mvs_files_in(&movies);
                        if files.is_empty() {
                            continue;
                        }
                        let mut added_any = false;
                        for file in files {
                            match archive::read_index(&file) {
                                Ok(entries) => {
                                    library.add(&file, entries);
                                    added_any = true;
                                }
                                Err(error) => eprintln!("Movies: {error}"),
                            }
                        }
                        if added_any {
                            library.game = Some(folder.to_path_buf());
                            break 'search_swbf2;
                        }
                    }
                }
            }
        }
        let manifest_path = match family {
            GameFamily::Swbf1 => content.map(|c| c.join("common/shell/movies/manifest.json")),
            GameFamily::Swbf2 => content
                .map(|c| family_dir(c, GameFamily::Swbf2).join("common/shell/movies/manifest.json"))
                .filter(|p| p.is_file()),
        };
        if let Some(manifest) = manifest_path
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

    /// Game family this library indexes.
    pub fn family(&self) -> GameFamily {
        self.family
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
        // The prepared content names the Galactic Conquest ending by its own hash. This is content an
        // earlier version prepared: its manifest describes a WebM copy, which lies beside it and is never
        // read (the movie plays from the game's archive).
        let content = root.join("content");
        std::fs::create_dir_all(content.join("common/shell/movies")).unwrap();
        std::fs::write(
            content.join("common/shell/movies/manifest.json"),
            format!(
                r#"{{"version":1,"movies":{{"rebel_win":{{"file":"rebel_win.webm","hash":"0x{:08x}","settings":"-c:v libvpx-vp9","bytes":4}}}}}}"#,
                hash("atat_in")
            ),
        )
        .unwrap();
        std::fs::write(content.join("common/shell/movies/rebel_win.webm"), b"junk").unwrap();

        // The preparation record's path (inside the game) works as well as the game folder; a missing
        // candidate is passed over.
        let candidates = [root.join("moved away"), game.join("GameData/Data/_LVL_PC")];
        let library = MovieLibrary::open(GameFamily::Swbf1, Some(&content), &candidates);
        assert_eq!(library.family(), GameFamily::Swbf1);
        assert_eq!(library.game_dir(), Some(game.as_path()));
        assert_eq!(library.archive_count(), 2);
        let main = library.resolve("/common/shell/movies/main.bik").unwrap();
        assert_eq!(main.path, shell.join("shell.mvs"));
        // An earlier version's shell.json names the same movie with .webm.
        assert_eq!(
            library.resolve("/common/shell/movies/main.webm"),
            Some(main.clone())
        );
        let atat = library.resolve("/common/shell/movies/atat_in.bik").unwrap();
        assert_eq!(library.resolve("movies/rebel_win.webm"), Some(atat.clone()));
        assert_eq!(library.resolve("movies/rebel_win.bik"), Some(atat.clone()));
        let fly = library.resolve("/common/shell/movies/tat3fly.bik").unwrap();
        assert_eq!(fly.path, addon.join("tat3.MVS"));
        assert!(library
            .resolve("/common/shell/movies/nothing.bik")
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
        let empty = MovieLibrary::open(GameFamily::Swbf1, Some(&content), &[root.join("moved away")]);
        assert_eq!(empty.archive_count(), 0);
        assert!(empty.resolve("/common/shell/movies/main.bik").is_none());

        // Every folder of the main archive in other letters: found, under the names on disk.
        let other = root.join("Other Game");
        let movies = other
            .join("gamedata")
            .join("DATA")
            .join("_lvl_pc")
            .join("movies");
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::write(movies.join("SHELL.MVS"), archive(&["main"])).unwrap();
        let library = MovieLibrary::open(GameFamily::Swbf1, None, std::slice::from_ref(&other));
        assert_eq!(library.game_dir(), Some(other.as_path()));
        let main = library.resolve("/common/shell/movies/main.bik").unwrap();
        assert_eq!(main.path, movies.join("SHELL.MVS"));
        // A decoder thread may still hold a file for a moment; Windows does not remove an open file.
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn indexes_swbf2_movies_and_keeps_families_separate() {
        let root = std::env::temp_dir().join(format!("bfe-movielib-bf2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let bf1 = root.join("SWBF1");
        std::fs::create_dir_all(bf1.join("GameData/Data/_LVL_PC/Common/ENG")).unwrap();
        std::fs::write(bf1.join("GameData/Data/_LVL_PC/Common/ENG/common.lvl"), b"lvl").unwrap();
        std::fs::create_dir_all(bf1.join("GameData/Data/_LVL_PC/Movies")).unwrap();
        std::fs::write(
            bf1.join("GameData/Data/_LVL_PC/Movies/shell.mvs"),
            archive(&["intro_a"]),
        )
        .unwrap();

        let bf2 = root.join("SWBF2");
        let bf2_movies = bf2.join("GameData/data/_lvl_pc/movies");
        std::fs::create_dir_all(&bf2_movies).unwrap();
        std::fs::write(bf2.join("GameData/BattlefrontII.exe"), b"exe").unwrap();
        std::fs::write(bf2_movies.join("shell.mvs"), archive(&["intro_a", "flyover_a"])).unwrap();
        std::fs::write(bf2_movies.join("shellfr.mvs"), archive(&["intro_a"])).unwrap();
        std::fs::write(bf2_movies.join("crawl.mvs"), archive(&["crawl_a"])).unwrap();

        let content = root.join("content");
        let bf2_movies_manifest =
            family_dir(&content, GameFamily::Swbf2).join("common/shell/movies");
        std::fs::create_dir_all(&bf2_movies_manifest).unwrap();
        std::fs::write(
            bf2_movies_manifest.join("manifest.json"),
            format!(
                r#"{{"version":1,"movies":{{"intro_crawl":{{"hash":"0x{:08x}"}}}}}}"#,
                hash("crawl_a")
            ),
        )
        .unwrap();

        // Both candidates in one list: each family opens only its own game's archives.
        let candidates = [bf1.clone(), bf2.clone()];
        let lib2 = MovieLibrary::open(GameFamily::Swbf2, Some(&content), &candidates);
        assert_eq!(lib2.family(), GameFamily::Swbf2);
        assert_eq!(lib2.game_dir(), Some(bf2.as_path()));
        assert_eq!(lib2.archive_count(), 3);
        // Base `shell.mvs` sorts before `shellfr.mvs`, so base wins.
        assert_eq!(
            lib2.resolve("/common/shell/movies/intro_a.bik").unwrap().path,
            bf2_movies.join("shell.mvs")
        );
        assert_eq!(
            lib2.resolve("/common/shell/movies/crawl_a.bik").unwrap().path,
            bf2_movies.join("crawl.mvs")
        );
        assert_eq!(
            lib2.resolve("/common/shell/movies/intro_crawl.bik")
                .unwrap()
                .path,
            bf2_movies.join("crawl.mvs")
        );

        let lib1 = MovieLibrary::open(GameFamily::Swbf1, Some(&content), std::slice::from_ref(&bf1));
        assert_eq!(lib1.family(), GameFamily::Swbf1);
        assert_eq!(lib1.game_dir(), Some(bf1.as_path()));
        assert_eq!(lib1.archive_count(), 1);
        assert!(lib1.resolve("/common/shell/movies/crawl_a.bik").is_none());
        assert!(lib1.resolve("/common/shell/movies/intro_crawl.bik").is_none());

        let _ = std::fs::remove_dir_all(&root);
    }
}
