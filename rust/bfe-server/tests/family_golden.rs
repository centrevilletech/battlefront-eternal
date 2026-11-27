//! The server's rotation boundary against outputs captured from the released
//! build (Preview 390) over made-up entries: the rotation query, the menu's
//! check of an edited entry, and the admission options the world worker reads
//! from that query. Every entry the released server read as the 2004 game
//! (anything without the literal `swbf2/` prefix) must give the released
//! answers byte for byte; 2005 entries follow the shared 2005 grammar.

use bfe_family::{swbf2, GameFamily};
use bfe_server::{
    config::{query_for_rotation, rotation_family, valid_playlist_entry},
    world_worker::parse_admit_options,
};
use serde_json::{json, Value};

const RELEASED: &str = include_str!("../../bfe-family/tests/fixtures/corpus_2004_server_golden.json");

#[test]
fn every_2004_entry_keeps_the_released_query_menu_answer_and_admission() {
    let rows: Vec<Value> = serde_json::from_str(RELEASED).expect("released outputs");
    let (mut old, mut new) = (0, 0);
    let mut differences = Vec::new();
    for row in &rows {
        let input = row["input"].as_str().unwrap();
        let server = query_for_rotation(input, "enhanced");
        if rotation_family(input) == GameFamily::Swbf2 {
            new += 1;
            let parsed = swbf2::Entry::parse_complete(input);
            assert_eq!(server.is_ok(), parsed.is_some(), "{input:?}");
            if let (Ok(query), Some(entry)) = (&server, &parsed) {
                assert!(query.starts_with(&format!("?map={}&", entry.qualified_map())), "{query}");
            }
            continue;
        }
        old += 1;
        let worker = server.as_ref().ok().map(|q| parse_admit_options(q, "nab2").query());
        let server = match server {
            Ok(q) => json!({ "ok": q }),
            Err(e) => json!({ "err": e }),
        };
        if server != row["server"] || json!(worker) != row["worker"] {
            differences.push(format!("{input:?}: released {} {}, now {server} {}", row["server"], row["worker"], json!(worker)));
        }
        // The menu trims first, so an entry with spaces can reach it as a 2005 entry.
        let trimmed = input.trim();
        if rotation_family(trimmed) == GameFamily::Swbf1 && json!(valid_playlist_entry(trimmed)) != row["menu"] {
            differences.push(format!("menu {input:?}: released {}", row["menu"]));
        }
    }
    assert!(old > 190 && new > 10, "{old} 2004 entries, {new} 2005 entries");
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
