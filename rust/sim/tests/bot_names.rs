//! Bot and human player naming tests ported from `tests/bot-names.test.js`.
//!
//! These cases exercise `bfe_sim::battle::names::{human_name, BotNames, NameLists,
//! LOCALS}` against `jsrt::rng::ClassicRandom` for both synthetic name lists
//! and (when `BFE_CONTENT_DIR` or `public/maps/geo1/ai-mission.json` is
//! present) the Geonosis retail roster.
//!
//! Coverage includes:
//! - Given and family name selection via raw `ClassicRandom` draws modulo the
//!   side's list lengths (`first[draw % first.len()]` + `last[draw % last.len()]`).
//! - Fallback `AA 000` formatting (two uppercase letters `A-Z`, space, three
//!   digits `0-9`) when either list is empty or `lists` is `None`.
//! - Per-side deterministic seeding (`ClassicRandom::new(side)`) so identical
//!   side types produce identical sequences while distinct side types diverge.
//! - `LOCALS` (`5.0`) constant name repetition, `64`-entry list truncation,
//!   `human_name(0.0) == "Player 1"`, and Geonosis retail end-screen names.

use bfe_sim::battle::names::{human_name, BotNames, NameLists, LOCALS};
use jsrt::rng::ClassicRandom;
use std::collections::HashSet;
use std::path::PathBuf;

/// Builds a `NameLists` table with `sides` indexed `0..=4`.
/// Each side holds `(first_names, last_names)` and an optional `local` label.
/// Side indices correspond to 2004 faction team types (`1 = ALL`, `2 = IMP`, `3 = REP`, `4 = CIS`).
fn make_lists(entries: &[(usize, Vec<&str>, Vec<&str>)], local: Option<&str>) -> NameLists {
    let mut sides = vec![(Vec::new(), Vec::new()); 5];
    for (idx, first, last) in entries {
        if *idx < sides.len() {
            sides[*idx] = (
                first.iter().map(|s| (*s).to_string()).collect(),
                last.iter().map(|s| (*s).to_string()).collect(),
            );
        }
    }
    NameLists {
        sides,
        local: local.map(str::to_string),
    }
}

// Mirrors `tests/bot-names.test.js`:
// "a name: a first name and a last name from the side's lists, each a raw draw modulo the list"
// "empty lists: two letters A-Z, three digits"
#[test]
fn side_lists_and_empty_fallbacks_draw_modulo_from_classic_random() {
    // Populated side 1 lists: 3 first names, 2 last names.
    // Each call to `BotNames::next` draws two raw 15-bit integers from `ClassicRandom::new(1.0)`.
    // The first draw selects `first[draw % 3]` and the second selects `last[draw % 2]`.
    let lists = make_lists(
        &[(1, vec!["HAN", "BEN", "LUKE"], vec!["SOLO", "KENOBI"])],
        None,
    );
    let mut gen = BotNames::new(Some(&lists), 1.0);
    let mut twin = ClassicRandom::new(1.0);
    let first_pool = ["HAN", "BEN", "LUKE"];
    let last_pool = ["SOLO", "KENOBI"];

    for _ in 0..20 {
        let f = first_pool[(twin.next_f64() % 3.0) as usize];
        let l = last_pool[(twin.next_f64() % 2.0) as usize];
        assert_eq!(gen.next(), format!("{f} {l}"));
    }

    // Empty lists on side 2 fall back to two letters A-Z and three digits 0-9.
    // The 2004 engine draws 2 letters (`65 + draw % 26`) for the given name
    // and 3 digits (`48 + draw % 10`) for the family name (five draws per bot).
    let empty_lists = make_lists(&[(2, Vec::new(), Vec::new())], None);
    let mut empty_gen = BotNames::new(Some(&empty_lists), 2.0);
    let mut twin2 = ClassicRandom::new(2.0);
    let letter = |rng: &mut ClassicRandom| (65 + (rng.next_f64() % 26.0) as u8) as char;
    let digit = |rng: &mut ClassicRandom| (48 + (rng.next_f64() % 10.0) as u8) as char;

    for _ in 0..20 {
        let expected = format!(
            "{}{} {}{}{}",
            letter(&mut twin2),
            letter(&mut twin2),
            digit(&mut twin2),
            digit(&mut twin2),
            digit(&mut twin2)
        );
        assert_eq!(empty_gen.next(), expected);
    }

    // Passing `None` for `lists` also produces `^[A-Z]{2} \d{3}$`.
    // This covers custom maps or test worlds that do not ship a side name dictionary.
    let sample = BotNames::new(None, 2.0).next();
    let bytes = sample.as_bytes();
    assert_eq!(bytes.len(), 6);
    assert!(bytes[0].is_ascii_uppercase() && bytes[1].is_ascii_uppercase());
    assert_eq!(bytes[2], b' ');
    assert!(bytes[3].is_ascii_digit() && bytes[4].is_ascii_digit() && bytes[5].is_ascii_digit());
}

// Mirrors `tests/bot-names.test.js`:
// "each side's generator starts from its own side type, so the same side names its bots the same"
// "locals all take the level's local name; the lists hold 64 names at most"
#[test]
fn per_side_seeds_locals_64_entry_cap_and_human_name_match_2004_rules() {
    // Two generators initialized with the same side type (`3.0`) produce identical rosters,
    // whereas a generator initialized with side type `4.0` diverges immediately.
    // Each faction owns an independent `ClassicRandom` instance seeded with its side integer.
    let lists3 = make_lists(&[(3, vec!["ALPHA", "BETA"], Vec::new())], None);
    let lists4 = make_lists(&[(4, vec!["ALPHA", "BETA"], Vec::new())], None);

    let mut a = BotNames::new(Some(&lists3), 3.0);
    let mut b = BotNames::new(Some(&lists3), 3.0);
    let mut c = BotNames::new(Some(&lists4), 4.0);
    let seq_a: Vec<String> = (0..10).map(|_| a.next()).collect();
    let seq_b: Vec<String> = (0..10).map(|_| b.next()).collect();
    let seq_c: Vec<String> = (0..10).map(|_| c.next()).collect();
    assert_eq!(seq_a, seq_b);
    assert_ne!(seq_a, seq_c);

    // Locals (`side == LOCALS == 5.0`) all take the level's single local name ("GEONOSIAN")
    // without advancing any random number generator.
    let local_lists = make_lists(&[], Some("GEONOSIAN"));
    let mut local_gen = BotNames::new(Some(&local_lists), LOCALS);
    assert_eq!(
        [local_gen.next(), local_gen.next()],
        ["GEONOSIAN", "GEONOSIAN"]
    );

    // Lists longer than 64 entries are truncated to 64 before modulo selection,
    // matching the 2004 engine's fixed 64-slot name table per faction.
    let mut long_lists = NameLists {
        sides: vec![(Vec::new(), Vec::new()); 5],
        local: None,
    };
    long_lists.sides[1] = (
        (0..70).map(|i| format!("N{i}")).collect(),
        vec!["X".to_string()],
    );
    let mut long_gen = BotNames::new(Some(&long_lists), 1.0);
    let mut twin1 = ClassicRandom::new(1.0);
    let expected_first = (twin1.next_f64() % 64.0) as usize;
    assert_eq!(long_gen.next(), format!("N{expected_first} X"));

    // Human player 0 is named "Player 1" (`human_name(index)` adds 1 to the 0-based index).
    assert_eq!(human_name(0.0), "Player 1");
}

// Mirrors `tests/bot-names.test.js`:
// "the retail sequences: Geonosis' end screen names"
#[test]
fn retail_sequences_match_geonosis_end_screen_names_when_extracted() {
    // Resolve `maps/geo1/ai-mission.json` from `BFE_CONTENT_DIR` or `public/`.
    // Skips cleanly in CI environments where extracted map files are absent.
    let root = std::env::var_os("BFE_CONTENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public"));
    let mission_path = root.join("maps/geo1/ai-mission.json");
    if !mission_path.exists() {
        return;
    }

    // Parse the `names` dictionary from `maps/geo1/ai-mission.json` into `NameLists`.
    // Each numbered faction entry (`"1"`..`"4"`) provides `"first"` and `"last"` string arrays.
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&mission_path).unwrap()).unwrap();
    let names_obj = raw.get("names").unwrap();
    let mut lists = NameLists {
        sides: vec![(Vec::new(), Vec::new()); 5],
        local: names_obj
            .get("local")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    for side in 1..=4 {
        if let Some(val) = names_obj.get(side.to_string()) {
            let first = val
                .get("first")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let last = val
                .get("last")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            lists.sides[side] = (first, last);
        }
    }

    // Helper closure to generate the first `count` bot names for `faction`.
    // Produces the exact spawn-time roster assigned to bots of that faction.
    let sequence = |faction: f64, count: usize| -> Vec<String> {
        let mut gen = BotNames::new(Some(&lists), faction);
        (0..count).map(|_| gen.next()).collect()
    };

    // Verify the opening sequences for Republic (3), CIS (4), Empire (2), and Rebels (1).
    // Clone troopers use Greek prefixes (`DELTA 855`), battle droids use `UNIT ###`,
    // stormtroopers use two-letter prefixes (`DX 178`), and Rebels use full names.
    assert_eq!(
        sequence(3.0, 8),
        [
            "DELTA 855",
            "THETA 269",
            "CHI 987",
            "OMEGA 370",
            "GAMMA 022",
            "RHO 773",
            "OMIKRON 621",
            "THETA 198"
        ]
    );
    assert_eq!(
        sequence(4.0, 8),
        [
            "UNIT 538", "UNIT 182", "UNIT 790", "UNIT 466", "UNIT 538", "UNIT 464", "UNIT 416",
            "UNIT 754"
        ]
    );
    assert_eq!(sequence(2.0, 4), ["DX 178", "KL 987", "TF 997", "JM 221"]);
    assert_eq!(
        sequence(1.0, 3),
        ["THRAK FARELLE", "RAYCE FARELLE", "LAN OLESA"]
    );

    // Verify all 12 Republic and 12 CIS names shown on the retail Geonosis end-of-battle screen.
    // Every name from the retail screenshot appears in the first 30 Republic / 24 CIS draws.
    let republic: HashSet<String> = sequence(3.0, 30).into_iter().collect();
    let cis: HashSet<String> = sequence(4.0, 24).into_iter().collect();
    for name in [
        "PI 610",
        "DELTA 607",
        "UPSILON 941",
        "OMEGA 370",
        "ALPHA 989",
        "RHO 571",
        "CHI 178",
        "RHO 773",
        "TAU 753",
        "BETA 030",
        "OMIKRON 621",
        "EPSILON 905",
    ] {
        assert!(republic.contains(name), "missing Republic name {name}");
    }
    for name in [
        "UNIT 538", "UNIT 961", "UNIT 073", "UNIT 111", "UNIT 466", "UNIT 182", "UNIT 850",
        "UNIT 972", "UNIT 754", "UNIT 007", "UNIT 322", "UNIT 311",
    ] {
        assert!(cis.contains(name), "missing CIS name {name}");
    }
}
