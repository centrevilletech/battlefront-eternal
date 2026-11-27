//! Parity tests comparing `MovieLibrary::open(GameFamily::Swbf1, ...)`
//! against the released build's implementation.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bfe_family::GameFamily;
use bfe_video::archive::{self, hash};
use bfe_video::library::{find_ignoring_case, MovieLibrary, Source};

fn chunk(tag: u32, body: &[u8]) -> Vec<u8> {
    let mut out = tag.to_le_bytes().to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

fn synthetic_mvs(names: &[&str]) -> Vec<u8> {
    let payload = b"BIKi\x00\x00\x00\x00";
    let mut info = Vec::new();
    let mut data = Vec::new();
    for name in names {
        for w in [
            hash("Segment"),
            hash("Name"),
            hash(name),
            hash("Length"),
            payload.len() as u32,
            hash("Padding"),
            4,
            hash("SegmentEnd"),
        ] {
            info.extend(w.to_le_bytes());
        }
        data.extend_from_slice(payload);
        data.extend([0u8; 4]);
    }
    let mut inner = chunk(hash("Info"), &info);
    inner.extend(chunk(hash("Data"), &data));
    let outer = chunk(hash("shell"), &inner);
    let mut file = b"ucfb".to_vec();
    file.extend((outer.len() as u32).to_le_bytes());
    file.extend(outer);
    file
}

/// The released build's version of the 2004 movie search.
fn released_open_swbf1(candidates: &[PathBuf]) -> (Option<PathBuf>, HashMap<u32, Source>) {
    let mut game = None;
    let mut by_hash = HashMap::new();
    'search: for candidate in candidates {
        for folder in candidate.ancestors().take(4) {
            let Some(shell) = find_ignoring_case(
                folder,
                &["GameData", "Data", "_LVL_PC", "Movies", "shell.mvs"],
            ) else {
                continue;
            };
            if let Ok(entries) = archive::read_index(&shell) {
                game = Some(folder.to_path_buf());
                for e in entries {
                    by_hash.entry(e.hash).or_insert(Source {
                        path: shell.clone(),
                        offset: e.offset,
                        length: e.length,
                    });
                }
                break 'search;
            }
        }
    }
    if let Some(g) = game.clone() {
        if let Some(addons) = find_ignoring_case(&g, &["GameData", "AddOn"]) {
            let mut dirs: Vec<_> = fs::read_dir(addons)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            dirs.sort();
            for dir in dirs {
                let Some(movies) = find_ignoring_case(&dir, &["Data", "_lvl_pc", "Movies"]) else {
                    continue;
                };
                let mut files: Vec<_> = fs::read_dir(movies)
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
                    if let Ok(entries) = archive::read_index(&file) {
                        for e in entries {
                            by_hash.entry(e.hash).or_insert(Source {
                                path: file.clone(),
                                offset: e.offset,
                                length: e.length,
                            });
                        }
                    }
                }
            }
        }
    }
    (game, by_hash)
}

#[test]
fn swbf1_movies_match_released_behavior_on_stray_exe_and_addon_sort() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bfe-video-parity-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);

    // 1. 2004 install with a stray `GameData/BattlefrontII.exe` and competing add-on `Z.mvs` vs `a.mvs`.
    let game = root.join("BF1WithStrayExe");
    let movies = game.join("GameData/Data/_LVL_PC/Movies");
    let addon = game.join("GameData/AddOn/MapX/Data/_lvl_pc/Movies");
    fs::create_dir_all(&movies).unwrap();
    fs::create_dir_all(&addon).unwrap();
    fs::write(game.join("GameData/BattlefrontII.exe"), b"stray").unwrap();
    fs::write(movies.join("shell.mvs"), synthetic_mvs(&["intro_a"])).unwrap();
    fs::write(addon.join("Z.mvs"), synthetic_mvs(&["shared_fly"])).unwrap();
    fs::write(addon.join("a.mvs"), synthetic_mvs(&["shared_fly"])).unwrap();

    let candidates = [game.clone()];
    let (base_game, base_map) = released_open_swbf1(&candidates);
    let head_lib = MovieLibrary::open(GameFamily::Swbf1, None, &candidates);

    assert_eq!(
        head_lib.game_dir().map(Path::to_path_buf),
        base_game,
        "2004 MovieLibrary must not veto install containing stray BattlefrontII.exe"
    );
    assert_eq!(
        head_lib.resolve("/common/shell/movies/shared_fly.bik"),
        base_map.get(&hash("shared_fly")).cloned(),
        "2004 add-on .mvs tie-breaker order must match released base (Z.mvs before a.mvs)"
    );

    let _ = fs::remove_dir_all(&root);
}
